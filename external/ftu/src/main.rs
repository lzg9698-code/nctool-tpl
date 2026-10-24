mod geometry;
mod post;
use nctool_plugin_sdk::*;
use std::{
    collections::BTreeMap,
    io::{BufRead, Write},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc, Arc, Mutex,
    },
    time::Duration,
};

struct Server {
    output: Mutex<std::io::Stdout>,
    pending: Mutex<BTreeMap<String, mpsc::Sender<PluginResult<Value>>>>,
    counter: AtomicU64,
}
impl Server {
    fn send(&self, value: Value) {
        let mut out = self.output.lock().unwrap();
        let _ = writeln!(out, "{value}");
        let _ = out.flush();
    }
    fn call(
        &self,
        parent: &str,
        service: &str,
        input: Value,
        cancel: &Cancellation,
    ) -> PluginResult<ActionResult> {
        cancel.check()?;
        let id = format!("plugin-{}", self.counter.fetch_add(1, Ordering::SeqCst));
        let (tx, rx) = mpsc::channel();
        self.pending.lock().unwrap().insert(id.clone(), tx);
        self.send(json!({"jsonrpc":"2.0","id":id,"method":"host.call","params":{"parent_id":parent,"service":service,"version":1,"input":input}}));
        let result = (|| loop {
            cancel.check()?;
            match rx.recv_timeout(Duration::from_millis(20)) {
                Ok(result) => return serde_json::from_value(result?).map_err(Into::into),
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(_) => return Err(PluginError::new("host_disconnected", "宿主连接已结束")),
            }
        })();
        self.pending.lock().unwrap().remove(&id);
        result
    }
}
fn prepare(input: &Value, server: &Server, id: &str, cancel: &Cancellation) -> PluginResult<Value> {
    if input.get("template_snapshot").is_some() {
        if input.get("template").is_some() {
            return Err(PluginError::new(
                "invalid_input",
                "快照与模板选择不能同时提供",
            ));
        }
        let checked = server.call(
            id,
            "template.inspect",
            json!({"snapshot":input["template_snapshot"],"include_snapshot":true}),
            cancel,
        )?;
        let mut frozen = input.clone();
        frozen["template_snapshot"] = checked.data["snapshot"].clone();
        return Ok(frozen);
    }
    let template_input = if let Some(name) = input.get("template").and_then(Value::as_str) {
        json!({"template":name,"include_snapshot":true})
    } else {
        json!({"source":post::TEMPLATE,"include_snapshot":true})
    };
    let snapshot = server
        .call(id, "template.inspect", template_input, cancel)?
        .data["snapshot"]
        .clone();
    let mut frozen = input.clone();
    frozen
        .as_object_mut()
        .ok_or_else(|| PluginError::new("invalid_input", "输入必须为对象"))?
        .remove("template");
    frozen["template_snapshot"] = snapshot;
    Ok(frozen)
}
fn invoke(
    action: &str,
    input: Value,
    server: &Server,
    id: &str,
    cancel: &Cancellation,
) -> PluginResult<ActionResult> {
    if action == "ftu.template" {
        return Ok(ActionResult::data(
            json!({"asset":{"source":post::TEMPLATE,"schema":post::schema(),"metadata":{"title":"SINUMERIK 精车后处理副本"}}}),
        ));
    }
    if action == "ftu.prepare" {
        return Ok(ActionResult::data(
            json!({"input":prepare(&input,server,id,cancel)?}),
        ));
    }
    let project: geometry::Project = serde_json::from_value(input["project"].clone())
        .map_err(|_| PluginError::new("invalid_project", "项目结构或数值未完成"))?;
    let analysis = geometry::analyze(&project, cancel)?;
    let scene = post::scene(&analysis);
    let diagnostics: Vec<_> = analysis
        .diagnostics
        .iter()
        .map(|issue| Diagnostic {
            level: "error".into(),
            code: issue.code.clone(),
            message: issue.message.clone(),
            path: Some(issue.path.clone()),
            template: None,
            line: None,
            column: None,
        })
        .collect();
    if action == "ftu.analyze" {
        return Ok(ActionResult {
            data: json!({"result":analysis,"scene":scene}),
            artifacts: vec![],
            diagnostics,
        });
    }
    if action != "ftu.generate" {
        return Err(PluginError::new("action_not_found", action));
    }
    if !analysis.valid {
        return Err(PluginError {
            code: "invalid_geometry".into(),
            message: "轨迹检查未通过".into(),
            diagnostics,
        });
    }
    let formats = input["formats"]
        .as_array()
        .ok_or_else(|| PluginError::new("invalid_input", "请选择导出格式"))?;
    let frozen = prepare(&input, server, id, cancel)?;
    let model = post::lower(&project, &analysis)?;
    let mut artifacts = vec![];
    for format in formats {
        cancel.check()?;
        let (name, mime, text) = match format.as_str() {
            Some("spf") => {
                let rendered=server.call(id,"template.render",json!({"snapshot":frozen["template_snapshot"],"context":model,"schema":post::schema(),"lenient":false,"trim_blocks":true,"lstrip_blocks":true}),cancel)?;
                let text = rendered.data["text"]
                    .as_str()
                    .ok_or_else(|| PluginError::new("invalid_output", "模板没有返回文本"))?;
                ("FTU_PATH.SPF", "text/plain", post::verify(text, &model)?)
            }
            Some("json") => (
                "trajectory.json",
                "application/json",
                serde_json::to_string_pretty(
                    &json!({"project":project,"result":analysis,"scene":scene}),
                )?,
            ),
            Some("csv") => ("trajectory.csv", "text/csv", post::csv(&analysis)),
            Some("svg") => ("trajectory.svg", "image/svg+xml", post::svg(&scene)),
            _ => return Err(PluginError::new("invalid_input", "不支持的导出格式")),
        };
        artifacts.push(Artifact {
            name: name.into(),
            media_type: mime.into(),
            text,
        });
    }
    cancel.check()?;
    Ok(ActionResult {
        data: json!({"result":analysis,"scene":scene,"postprocessor_version":1,"replay_input":frozen}),
        artifacts,
        diagnostics,
    })
}
fn main() {
    let manifest: Value =
        serde_json::from_str(include_str!("../plugin.json")).expect("embedded manifest");
    let server = Arc::new(Server {
        output: Mutex::new(std::io::stdout()),
        pending: Mutex::new(BTreeMap::new()),
        counter: AtomicU64::new(1),
    });
    let active = Arc::new(Mutex::new(BTreeMap::<String, Cancellation>::new()));
    let mut workers = vec![];
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        if line.len() > 16 * 1024 * 1024 {
            break;
        }
        let Ok(message) = parse_json(line.as_bytes()) else {
            break;
        };
        if message["jsonrpc"] != "2.0" {
            break;
        }
        match message["method"].as_str(){
            Some("initialize")=>{let valid=message["params"]["protocol_version"]==2&&message["params"]["plugin_id"]=="ftu";server.send(if valid{json!({"jsonrpc":"2.0","id":message["id"],"result":{"protocol_version":2,"plugin_id":"ftu"}})}else{json!({"jsonrpc":"2.0","id":message["id"],"error":{"code":-32000,"message":"protocol mismatch","data":{"code":"protocol_version","message":"protocol mismatch","diagnostics":[]}}})});}
            Some("describe")=>server.send(json!({"jsonrpc":"2.0","id":message["id"],"result":{"descriptor":manifest["descriptor"],"actions":manifest["actions"]}})),
            Some("cancel")=>{if let Some(c)=message["params"]["id"].as_str().and_then(|id|active.lock().unwrap().get(id).cloned()){c.cancel();}},
            Some("shutdown")=>break,
            Some("invoke")=>{let Some(id)=message["id"].as_str().map(str::to_string)else{break};let cancel=Cancellation::default();active.lock().unwrap().insert(id.clone(),cancel.clone());let server=server.clone();let active=active.clone();workers.retain(|w:&std::thread::JoinHandle<()>|!w.is_finished());workers.push(std::thread::spawn(move||{let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||invoke(message["params"]["action"].as_str().unwrap_or(""),message["params"]["input"].clone(),&server,&id,&cancel))).unwrap_or_else(|_|Err(PluginError::new("plugin_error","计算发生内部错误")));let response=match result{Ok(result)=>json!({"jsonrpc":"2.0","id":id,"result":result}),Err(error)=>json!({"jsonrpc":"2.0","id":id,"error":{"code":-32000,"message":error.message,"data":error}})};server.send(response);active.lock().unwrap().remove(&id);}));},
            None=>{let Some(id)=message["id"].as_str()else{break};if let Some(tx)=server.pending.lock().unwrap().remove(id){let result=if message.get("error").is_some(){Err(serde_json::from_value(message["error"]["data"].clone()).unwrap_or_else(|_|PluginError::new("host_error","宿主调用失败")))}else{Ok(message["result"].clone())};let _=tx.send(result);}},
            _=>break,
        }
    }
    for token in active.lock().unwrap().values() {
        token.cancel();
    }
    server.pending.lock().unwrap().clear();
    for worker in workers {
        let _ = worker.join();
    }
}

#[cfg(test)]
mod tests;
