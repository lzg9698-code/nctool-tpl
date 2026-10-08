//! Managed composition only speaks JSON and versioned services.
use nctool_plugin_sdk::*;
use std::collections::{BTreeMap, BTreeSet};
pub struct ManagedProcessPlugin;
fn invalid(message: &str) -> PluginError {
    PluginError::new("process_validation", message)
}
fn pointer(value: &mut Value, path: &str, source: Value) -> PluginResult<()> {
    if !path.starts_with('/') || path.len() > 1024 {
        return Err(invalid("结果绑定需要有效的 JSON Pointer"));
    }
    let mut parts = path[1..].split('/').peekable();
    let mut current = value;
    while let Some(part) = parts.next() {
        let part = part.replace("~1", "/").replace("~0", "~");
        if part.is_empty() || ["__proto__", "constructor", "prototype"].contains(&part.as_str()) {
            return Err(invalid("无效绑定路径"));
        }
        let map = current
            .as_object_mut()
            .ok_or_else(|| invalid("结果绑定穿过非对象"))?;
        if parts.peek().is_none() {
            map.insert(part, source);
            return Ok(());
        }
        current = map.entry(part).or_insert_with(|| json!({}));
    }
    Err(invalid("空绑定"))
}
impl ManagedProcessPlugin {
    pub fn definition() -> PluginDescriptor {
        let mut d = PluginDescriptor::builtin("process-parameters");
        d.requires = [
            "nc.managed.freeze",
            "nc.generate.managed",
            "nc.managed.check",
            "nc.output.finalize",
        ]
        .iter()
        .map(|id| Requirement::new(id))
        .collect();
        d.provides = vec![Service::new(
            "process.generate.managed",
            "process.generate.managed",
        )];
        d.panels = vec![Panel {
            id: "managed-process".into(),
            title: "受管工序与组合程序".into(),
            view: "nc.managed-process".into(),
            actions: vec!["process.generate.managed".into()],
        }];
        d
    }
    pub fn action_definitions() -> Vec<ActionDescriptor> {
        vec![
            ActionDescriptor::new(
                "process.managed.check",
                "核对组合资源依赖",
                json!({"type":"object","required":["process_snapshot"],"properties":{"process_snapshot":{"type":"object"}},"additionalProperties":false}),
            ),
            ActionDescriptor::new(
                "process.generate.managed",
                "生成受管组合程序",
                json!({"type":"object","properties":{"name":{"type":"string"},"params":{"type":"object"},"operation_inputs":{"type":"object"},"parameter_selection":{"type":"object"},"machine":{"type":"object"},"machine_id":{"type":"string"},"options":{"type":"object"},"ops":{"type":"array","minItems":1,"maxItems":1024,"items":{"type":"object"}},"outputs":{"type":"array","items":{"type":"object","required":["name","segments"],"properties":{"name":{"type":"string"},"segments":{"type":"array","minItems":1,"items":{"type":"object","required":["op"],"properties":{"op":{"type":"string"},"channel":{"type":"integer","minimum":1,"maximum":2}},"additionalProperties":false}}},"additionalProperties":false}},"process_snapshot":{"type":"object"}},"anyOf":[{"required":["ops"]},{"required":["process_snapshot"]}],"additionalProperties":false}),
            ),
        ]
    }
}
impl Plugin for ManagedProcessPlugin {
    fn descriptor(&self) -> PluginDescriptor {
        Self::definition()
    }
    fn actions(&self) -> Vec<ActionDescriptor> {
        Self::action_definitions()
    }
    fn invoke(&self, action: &str, input: Value, ctx: &Context) -> PluginResult<ActionResult> {
        if action == "process.managed.check" {
            let snapshot = &input["process_snapshot"];
            if snapshot["format_version"] != 1 {
                return Err(invalid("组合快照版本无效"));
            }
            let mut changes = vec![];
            for frozen in snapshot["frozen_ops"]
                .as_array()
                .ok_or_else(|| invalid("组合快照不完整"))?
            {
                let checked =
                    ctx.call("nc.managed.check", 1, json!({"managed_snapshot":frozen}))?;
                changes.extend(
                    checked.data["changes"]
                        .as_array()
                        .ok_or_else(|| invalid("依赖检查服务缺少 changes 数组"))?
                        .clone(),
                );
            }
            return Ok(ActionResult::data(
                json!({"current":changes.is_empty(),"changes":changes}),
            ));
        }
        if input.get("process_snapshot").is_some() && input.as_object().is_none_or(|m| m.len() != 1)
        {
            return Err(invalid("组合快照不能混用实时输入"));
        }
        let replay = input.get("process_snapshot");
        if replay.is_some_and(|r| r["format_version"] != 1) {
            return Err(invalid("组合快照版本无效"));
        }
        let source = replay.map(|s| &s["input"]).unwrap_or(&input);
        validate_schema(
            &Self::action_definitions()[1].input_schema,
            source,
            "process_validation",
        )?;
        let ops = source["ops"]
            .as_array()
            .filter(|ops| !ops.is_empty())
            .ok_or_else(|| invalid("至少需要一个工序"))?;
        if ops.len() > 1024 {
            return Err(invalid("工序数量超限"));
        }
        let mut frozen_ops = vec![];
        let mut seen = BTreeSet::new();
        for (index, op) in ops.iter().enumerate() {
            ctx.cancellation.check()?;
            let id = op["id"]
                .as_str()
                .map(str::to_string)
                .unwrap_or(format!("op-{index}"));
            if !seen.insert(id.clone()) {
                return Err(invalid("工序标识重复"));
            }
            if op.get("business").is_some() && op.get("result_bindings").is_some() {
                return Err(invalid(
                    "业务批次不接受程序文本输入绑定；请使用输出 segments 组合本次结果",
                ));
            }
            let frozen = if let Some(replay) = replay {
                replay["frozen_ops"]
                    .get(index)
                    .ok_or_else(|| invalid("组合快照不完整"))?
                    .clone()
            } else {
                let mut request = op.clone();
                for key in ["id", "result_bindings", "sync_from"] {
                    request.as_object_mut().unwrap().remove(key);
                }
                for key in ["machine", "machine_id", "options", "parameter_selection"] {
                    if request.get(key).is_none() {
                        if let Some(value) = source.get(key) {
                            request[key] = value.clone();
                        }
                    }
                }
                for key in ["params", "operation_inputs"] {
                    let mut common = source.get(key).cloned().unwrap_or_else(|| json!({}));
                    if let Some(values) = request.get(key).and_then(Value::as_object) {
                        common
                            .as_object_mut()
                            .ok_or_else(|| invalid("公共参数必须为对象"))?
                            .extend(values.clone());
                    }
                    if !common.as_object().is_none_or(|m| m.is_empty()) {
                        request[key] = common;
                    }
                }
                ctx.call("nc.managed.freeze", 1, request)?.data
            };
            if let Some(first) = frozen_ops.first() {
                let first: &Value = first;
                for key in ["parameter_snapshot", "nc_snapshot"] {
                    if first[key] != frozen[key] {
                        return Err(invalid("冻结期间参数资产已变化，请重试整个组合任务"));
                    }
                }
                let before = &first["template_snapshot"]["templates"];
                let after = &frozen["template_snapshot"]["templates"];
                // Inline roots differ by design; saved dependencies must share one revision.
                for (name, value) in before.as_object().into_iter().flatten() {
                    let inline_root = first["input"].get("source").is_some()
                        && first["template_snapshot"]["template"] == name.as_str();
                    let next_inline_root = frozen["input"].get("source").is_some()
                        && frozen["template_snapshot"]["template"] == name.as_str();
                    if !inline_root && !next_inline_root && after.get(name) != Some(value) {
                        return Err(invalid("冻结期间模板依赖已变化，请重试整个组合任务"));
                    }
                }
            }
            if frozen["kind"] == "business" {
                for previous in &frozen_ops {
                    if previous["business_plugin"] == frozen["business_plugin"]
                        && previous["job_snapshot"]["assets"] != frozen["job_snapshot"]["assets"]
                    {
                        return Err(invalid("冻结期间轨迹资产已变化，请重试整个组合任务"));
                    }
                }
            }
            frozen_ops.push(frozen);
        }
        let mut results = BTreeMap::<String, ActionResult>::new();
        let mut replay_ops = vec![];
        let mut cursor = source["options"]["line_number_start"].as_u64().unwrap_or(0);
        let mut diagnostics = vec![];
        for (index, op) in ops.iter().enumerate() {
            ctx.cancellation.check()?;
            let id = op["id"]
                .as_str()
                .map(str::to_string)
                .unwrap_or(format!("op-{index}"));
            let mut frozen = frozen_ops[index].clone();
            let raw_key = if frozen["input"].get("operation_inputs").is_some() {
                "operation_inputs"
            } else {
                "params"
            };
            if !frozen["input"][raw_key].is_object() {
                frozen["input"][raw_key] = json!({});
            }
            for (target, binding) in op["result_bindings"]
                .as_object()
                .cloned()
                .unwrap_or_default()
            {
                let source_id = binding["op"]
                    .as_str()
                    .ok_or_else(|| invalid("结果绑定缺少工序标识"))?;
                let previous = results
                    .get(source_id)
                    .ok_or_else(|| invalid("结果绑定只能引用已完成的前置工序"))?;
                let value = if let Some(channel) = binding["channel"].as_u64() {
                    previous.data["channels"]
                        .get(
                            channel
                                .checked_sub(1)
                                .ok_or_else(|| invalid("通道编号必须从 1 开始"))?
                                as usize,
                        )
                        .cloned()
                        .ok_or_else(|| invalid("前置工序通道不存在"))?
                } else {
                    previous.data["text"].clone()
                };
                let body = value
                    .as_str()
                    .ok_or_else(|| invalid("前置结果不是程序文本"))?
                    .trim_end_matches('\n');
                let value = json!(body);
                pointer(&mut frozen["input"][raw_key], &target, value.clone())?;
                if !frozen["bound_programs"].is_object() {
                    frozen["bound_programs"] = json!({});
                }
                frozen["bound_programs"][&target] = value;
            }
            if let Some(previous) = op["sync_from"].as_str() {
                let next = results
                    .get(previous)
                    .and_then(|r| r.data["next_sync_num"].as_u64())
                    .ok_or_else(|| invalid("缺少前置同步编号"))?;
                if frozen["kind"] == "business" {
                    let target = frozen["job_snapshot"]["input"]["target"]
                        .as_str()
                        .unwrap_or("all")
                        .to_string();
                    if frozen["business_plugin"] != "sync-turning" || target == "all" {
                        return Err(invalid("同步编号绑定需要同步粗车的明确单侧批次"));
                    }
                    frozen["job_snapshot"]["input"]["sides"][target]["start_sync_num"] =
                        json!(next);
                } else {
                    frozen["input"][raw_key]["current_sync_num"] = json!(next);
                }
            }
            // Named files are numbered once after their ordered fragments are assembled.
            if source.get("outputs").is_some() {
                if !frozen["input"]["options"].is_object() {
                    frozen["input"]["options"] = json!({});
                }
                frozen["input"]["options"]["line_numbers"] = json!(false);
            }
            if source.get("outputs").is_none() {
                if !frozen["input"]["options"].is_object() {
                    frozen["input"]["options"] = json!({});
                }
                frozen["input"]["options"]["line_number_start"] = json!(cursor);
            }
            let result = ctx
                .call("nc.generate.managed", 1, json!({"managed_snapshot":frozen}))
                .map_err(|mut e| {
                    e.message = format!("工序 {id} 失败，未交付任何程序: {}", e.message);
                    if e.diagnostics.is_empty() {
                        e.diagnostics.push(Diagnostic {
                            level: "error".into(),
                            code: e.code.clone(),
                            message: e.message.clone(),
                            path: None,
                        });
                    }
                    for d in &mut e.diagnostics {
                        d.path = Some(format!("ops[{index}].{}", d.path.as_deref().unwrap_or("")));
                    }
                    e
                })?;
            cursor = result.data["end_line_number"].as_u64().unwrap_or(cursor);
            if !result.data["text"].is_string()
                || result.data["channels"].as_array().is_none_or(|channels| {
                    channels.is_empty() || channels.iter().any(|c| !c.is_string())
                })
                || result.data["replay_input"]["managed_snapshot"]["format_version"] != 1
            {
                return Err(invalid("受管生成服务缺少程序、通道或冻结重执行输入"));
            }
            diagnostics.extend(result.diagnostics.clone());
            replay_ops.push(result.data["replay_input"]["managed_snapshot"].clone());
            results.insert(id, result);
        }
        let mut artifacts = vec![];
        let mut output_names = BTreeSet::new();
        if let Some(outputs) = source["outputs"].as_array() {
            for output in outputs {
                let name = output["name"]
                    .as_str()
                    .ok_or_else(|| invalid("输出缺少文件名"))?;
                if name.contains(['/', '\\'])
                    || name == ".."
                    || !output_names.insert(name.to_string())
                {
                    return Err(invalid("输出文件名无效或重复"));
                }
                let mut text = String::new();
                for segment in output["segments"]
                    .as_array()
                    .ok_or_else(|| invalid("输出缺少 segments"))?
                {
                    let result = results
                        .get(segment["op"].as_str().unwrap_or(""))
                        .ok_or_else(|| invalid("输出引用未知工序"))?;
                    let body = if let Some(channel) = segment["channel"].as_u64() {
                        result.data["channels"]
                                .get(channel.checked_sub(1).ok_or_else(|| invalid("无效通道"))?
                                    as usize)
                                .and_then(Value::as_str)
                    } else {
                        result.data["text"].as_str()
                    }
                    .ok_or_else(|| invalid("输出通道不存在"))?;
                    text.push_str(body);
                    if !text.ends_with('\n') {
                        text.push('\n');
                    }
                }
                let first_segment = &output["segments"][0];
                let op_index = ops
                    .iter()
                    .enumerate()
                    .find(|(i, op)| {
                        op["id"]
                            .as_str()
                            .map(str::to_string)
                            .unwrap_or(format!("op-{i}"))
                            == first_segment["op"].as_str().unwrap_or("")
                    })
                    .map(|(i, _)| i)
                    .ok_or_else(|| invalid("输出引用未知工序"))?;
                let options = source
                    .get("options")
                    .or_else(|| ops[op_index].get("options"))
                    .cloned()
                    .unwrap_or(json!({}));
                for segment in output["segments"].as_array().unwrap() {
                    let other = ops
                        .iter()
                        .enumerate()
                        .position(|(i, op)| {
                            op["id"]
                                .as_str()
                                .map(str::to_string)
                                .unwrap_or(format!("op-{i}"))
                                == segment["op"].as_str().unwrap_or("")
                        })
                        .ok_or_else(|| invalid("输出引用未知工序"))?;
                    if frozen_ops[other]["machine"] != frozen_ops[op_index]["machine"] {
                        return Err(invalid("同一输出文件的工序必须使用相同冻结机床"));
                    }
                    let other_options = source
                        .get("options")
                        .or_else(|| ops[other].get("options"))
                        .cloned()
                        .unwrap_or(json!({}));
                    if other_options != options {
                        return Err(invalid("同一输出文件的后处理选项必须一致"));
                    }
                }
                let finalized=ctx.call("nc.output.finalize",1,json!({"name":name,"text":text,"machine":frozen_ops[op_index]["machine"],"options":options}))?;
                cursor = finalized.data["end_line_number"].as_u64().unwrap_or(cursor);
                diagnostics.extend(finalized.diagnostics);
                artifacts.extend(finalized.artifacts);
            }
        } else {
            if results.values().any(|result| {
                result.data["channels"]
                    .as_array()
                    .is_some_and(|channels| channels.len() > 1)
            }) {
                return Err(invalid("多通道工序必须通过 outputs 明确分配独立文件"));
            }
            let mut text = String::new();
            for (index, op) in ops.iter().enumerate() {
                let id = op["id"]
                    .as_str()
                    .map(str::to_string)
                    .unwrap_or(format!("op-{index}"));
                text.push_str(results[&id].data["text"].as_str().unwrap());
                if !text.ends_with('\n') {
                    text.push('\n');
                }
            }
            let name = source["name"].as_str().unwrap_or("program");
            if name.is_empty() || name.contains(['/', '\\']) || name == ".." {
                return Err(invalid("组合文件名无效"));
            }
            artifacts.push(Artifact {
                name: format!("{name}.nc"),
                media_type: "text/plain".into(),
                text,
            });
        }
        if artifacts.is_empty() {
            return Err(invalid("输出计划为空"));
        }
        Ok(ActionResult {
            data: json!({"text":artifacts.iter().map(|a|a.text.as_str()).collect::<Vec<_>>().join("\n"),"end_line_number":cursor,"operations":ops.iter().enumerate().map(|(i,op)|op["id"].as_str().map(str::to_string).unwrap_or(format!("op-{i}"))).collect::<Vec<_>>(),"replay_input":{"process_snapshot":{"format_version":1,"input":source,"frozen_ops":replay_ops}}}),
            artifacts,
            diagnostics,
        })
    }
}
