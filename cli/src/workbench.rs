//! Generic asynchronous executions, immutable template evidence and workspace-scoped history.
use nctool_assets::{SafePath, WriteKernel};
use nctool_plugin_sdk::*;
use nctool_runtime::Runtime;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::{SystemTime, UNIX_EPOCH},
};
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Run {
    pub id: String,
    pub action: String,
    pub input: Value,
    pub status: String,
    pub started_at: u64,
    pub finished_at: Option<u64>,
    pub result: Option<ActionResult>,
    pub error: Option<PluginError>,
    pub plugins: Vec<PluginDescriptor>,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<ExecutionEnvironment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replay: Option<ReplayOrigin>,
}
#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub struct ExecutionEnvironment {
    pub format_version: u32,
    pub host_version: String,
    pub plugins: BTreeMap<String, PluginVersion>,
    pub services: Value,
}
#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub struct PluginVersion {
    pub version: String,
    pub protocol_version: u32,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct ReplayCheck {
    pub status: String,
    pub differences: Vec<String>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct ReplayOrigin {
    pub source_run_id: String,
    pub check: ReplayCheck,
}
fn compare_environment(
    recorded: Option<&ExecutionEnvironment>,
    current: &ExecutionEnvironment,
) -> ReplayCheck {
    let Some(recorded) = recorded.filter(|env| env.format_version == current.format_version) else {
        return ReplayCheck {
            status: "unknown".into(),
            differences: vec!["历史记录缺少可比较的执行环境信息".into()],
        };
    };
    let mut differences = vec![];
    if recorded.host_version != current.host_version {
        differences.push(format!(
            "宿主版本：{} → {}",
            recorded.host_version, current.host_version
        ));
    }
    for (id, previous) in &recorded.plugins {
        match current.plugins.get(id) {
            Some(next) if previous != next => differences.push(format!(
                "插件 {id}：{}（协议 {}）→ {}（协议 {}）",
                previous.version, previous.protocol_version, next.version, next.protocol_version
            )),
            None => differences.push(format!("插件 {id} 当前未启用")),
            _ => {}
        }
    }
    for id in current.plugins.keys() {
        if !recorded.plugins.contains_key(id) {
            differences.push(format!("插件 {id} 为本次新增启用"));
        }
    }
    if recorded.services != current.services {
        differences.push("服务提供方或服务接口版本已改变".into());
    }
    ReplayCheck {
        status: if differences.is_empty() {
            "matching"
        } else {
            "changed"
        }
        .into(),
        differences,
    }
}
pub struct Workbench {
    runtime: Arc<Runtime>,
    root: SafePath,
    controls: Mutex<BTreeMap<String, Cancellation>>,
    counter: AtomicU64,
    active: AtomicUsize,
    pub workspace_id: String,
}
impl Workbench {
    pub fn new(runtime: Arc<Runtime>, workspace: &Path) -> PluginResult<Arc<Self>> {
        let folder = workspace.join(".nctool/runs");
        std::fs::create_dir_all(&folder)?;
        let workspace = workspace.canonicalize()?;
        let id = nctool_assets::FileFingerprint::of_bytes(
            workspace.to_string_lossy().as_bytes(),
            UNIX_EPOCH,
        )
        .as_string()
        .replace(':', "-");
        let root = SafePath::from_root(&folder).map_err(crate::asset_error)?;
        let controller = Arc::new(Self {
            runtime,
            root,
            controls: Mutex::new(BTreeMap::new()),
            counter: AtomicU64::new(0),
            active: AtomicUsize::new(0),
            workspace_id: id,
        });
        // Interrupted records remain failures, never candidates for exporting a partial result.
        for summary in controller.summaries()? {
            if ["queued", "running"].contains(&summary["status"].as_str().unwrap_or("")) {
                let id = summary["id"].as_str().unwrap();
                let guard = controller.guard(id)?;
                if guard.try_lock().is_err() {
                    continue;
                }
                let run = controller.get(id)?;
                let mut run = run;
                run.status = "interrupted".into();
                run.finished_at = Some(now());
                run.result = None;
                run.error = Some(PluginError::new(
                    "interrupted",
                    "上次服务停止时任务尚未完成，可恢复输入后重新执行。",
                ));
                controller.save(&run)?;
            }
        }
        Ok(controller)
    }
    fn path(&self, id: &str) -> PluginResult<std::path::PathBuf> {
        if !nctool_runtime::valid_id(id) {
            return Err(PluginError::new("invalid_run_id", id));
        }
        self.root
            .resolve(&format!("{id}.json"))
            .map_err(crate::asset_error)
    }
    fn guard(&self, id: &str) -> PluginResult<std::fs::File> {
        let path = self
            .root
            .resolve(&format!("{id}.nctool.lock"))
            .map_err(crate::asset_error)?;
        Ok(std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?)
    }
    fn save(&self, run: &Run) -> PluginResult<()> {
        let bytes = serde_json::to_vec(run)?;
        if bytes.len() > 64 * 1024 * 1024 {
            return Err(PluginError::new("record_limit", "任务记录超过 64 MiB 上限"));
        }
        WriteKernel::write_atomic(&self.path(&run.id)?, &bytes).map_err(crate::asset_error)?;
        let folder = self.root.root().join("_index");
        std::fs::create_dir_all(&folder)?;
        let summary = json!({"id":run.id,"title":run.title,"action":run.action,"status":run.status,"started_at":run.started_at,"finished_at":run.finished_at,"diagnostic_count":run.result.as_ref().map(|r|r.diagnostics.len()).or_else(||run.error.as_ref().map(|e|e.diagnostics.len())).unwrap_or(0)});
        WriteKernel::write_atomic(
            &folder.join(format!("{}.json", run.id)),
            &serde_json::to_vec(&summary)?,
        )
        .map_err(crate::asset_error)
    }
    pub fn get(&self, id: &str) -> PluginResult<Run> {
        use std::io::Read;
        let path = self.path(id)?;
        let file = std::fs::File::open(path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                PluginError::new("not_found", id)
            } else {
                e.into()
            }
        })?;
        let mut bytes = Vec::new();
        file.take(64 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
        if bytes.len() > 64 * 1024 * 1024 {
            return Err(PluginError::new("record_limit", "任务记录超过上限"));
        }
        serde_json::from_value(parse_json(&bytes)?).map_err(Into::into)
    }
    fn summaries(&self) -> PluginResult<Vec<Value>> {
        let folder = self.root.root().join("_index");
        if !folder.exists() {
            return Ok(vec![]);
        }
        let mut summaries = vec![];
        for entry in std::fs::read_dir(folder)? {
            let entry = entry?;
            if entry.file_type()?.is_file() && entry.path().extension().is_some_and(|e| e == "json")
            {
                let bytes = std::fs::read(entry.path())?;
                if bytes.len() > 1024 * 1024 {
                    return Err(PluginError::new("record_limit", "任务索引超过上限"));
                }
                summaries.push(parse_json(&bytes)?);
            }
        }
        summaries.sort_by_key(|r| std::cmp::Reverse(r["started_at"].as_u64().unwrap_or(0)));
        Ok(summaries)
    }
    pub fn list(&self) -> PluginResult<Value> {
        Ok(json!({"runs":self.summaries()?.into_iter().take(100).collect::<Vec<_>>()}))
    }
    pub fn start(
        self: &Arc<Self>,
        action: &str,
        input: Value,
        title: &str,
    ) -> PluginResult<String> {
        self.start_with_replay(action, input, title, None)
    }
    fn environment(&self) -> ExecutionEnvironment {
        ExecutionEnvironment {
            format_version: 1,
            host_version: env!("CARGO_PKG_VERSION").into(),
            plugins: self
                .runtime
                .descriptors()
                .into_iter()
                .map(|p| {
                    (
                        p.id,
                        PluginVersion {
                            version: p.version,
                            protocol_version: p.protocol_version,
                        },
                    )
                })
                .collect(),
            services: self.runtime.bindings(),
        }
    }
    pub fn replay_check(&self, id: &str) -> PluginResult<ReplayCheck> {
        let recorded = self.get(id)?;
        Ok(compare_environment(
            recorded.environment.as_ref(),
            &self.environment(),
        ))
    }
    pub fn rerun(self: &Arc<Self>, id: &str) -> PluginResult<String> {
        let recorded = self.get(id)?;
        let check = compare_environment(recorded.environment.as_ref(), &self.environment());
        let input = recorded
            .result
            .as_ref()
            .and_then(|result| result.data.get("replay_input"))
            .cloned()
            .unwrap_or(recorded.input);
        self.start_with_replay(
            &recorded.action,
            input,
            &recorded.title,
            Some(ReplayOrigin {
                source_run_id: id.into(),
                check,
            }),
        )
    }
    fn start_with_replay(
        self: &Arc<Self>,
        action: &str,
        input: Value,
        title: &str,
        replay: Option<ReplayOrigin>,
    ) -> PluginResult<String> {
        let capability = self
            .runtime
            .capabilities()
            .into_iter()
            .find(|c| c.action.id == action)
            .ok_or_else(|| PluginError::new("action_not_found", action))?;
        nctool_runtime::validate_schema(&capability.action.input_schema, &input, "invalid_input")?;
        if self.active.fetch_add(1, Ordering::SeqCst) >= 8 {
            self.active.fetch_sub(1, Ordering::SeqCst);
            return Err(PluginError::new("plugin_busy", "最多同时执行 8 个任务"));
        }
        let id = format!(
            "run-{}-{}-{}",
            now(),
            std::process::id(),
            self.counter.fetch_add(1, Ordering::SeqCst)
        );
        let run = Run {
            id: id.clone(),
            action: action.into(),
            input,
            status: "queued".into(),
            started_at: now(),
            finished_at: None,
            result: None,
            error: None,
            plugins: self.runtime.descriptors(),
            title: title.into(),
            environment: Some(self.environment()),
            replay,
        };
        let guard = match self.guard(&id) {
            Ok(guard) => guard,
            Err(error) => {
                self.active.fetch_sub(1, Ordering::SeqCst);
                return Err(error);
            }
        };
        if let Err(error) = guard.lock() {
            self.active.fetch_sub(1, Ordering::SeqCst);
            return Err(PluginError::new("lock", error.to_string()));
        }
        if let Err(error) = self.save(&run) {
            self.active.fetch_sub(1, Ordering::SeqCst);
            return Err(error);
        }
        let cancellation = Cancellation::default();
        self.controls
            .lock()
            .unwrap()
            .insert(id.clone(), cancellation.clone());
        let controller = self.clone();
        std::thread::spawn(move || {
            let _guard = guard;
            let mut run = run;
            run.status = "running".into();
            let _ = controller.save(&run);
            let outcome = (|| {
                cancellation.check()?;
                // Freeze the generic template service's entire source collection before executing it.
                // Domain and third-party actions retain their own input contracts.
                if run.action == "template.render" && run.input.get("snapshot").is_none() {
                    let mut inspection = run.input.clone();
                    inspection["include_snapshot"] = json!(true);
                    let inspected = controller.runtime.invoke("template.inspect", inspection)?;
                    let snapshot =
                        inspected.data.get("snapshot").cloned().ok_or_else(|| {
                            PluginError::new("invalid_output", "模板检查未返回快照")
                        })?;
                    for field in ["source", "template", "templates"] {
                        run.input.as_object_mut().unwrap().remove(field);
                    }
                    run.input["snapshot"] = snapshot;
                }
                cancellation.check()?;
                controller.runtime.invoke_cancellable_request(
                    &run.id,
                    &run.action,
                    run.input.clone(),
                    cancellation.clone(),
                )
            })();
            match outcome {
                Ok(result) if !cancellation.is_cancelled() => {
                    run.status = "succeeded".into();
                    run.result = Some(result);
                }
                Ok(_) => {
                    run.status = "cancelled".into();
                    run.error = Some(PluginError::new("cancelled", "任务已取消"));
                }
                Err(error) => {
                    run.status = if error.code == "cancelled" {
                        "cancelled"
                    } else {
                        "failed"
                    }
                    .into();
                    run.error = Some(error);
                }
            }
            run.finished_at = Some(now());
            if let Err(error) = controller.save(&run) {
                run.result = None;
                run.status = "failed".into();
                run.error = Some(error);
                let _ = controller.save(&run);
            }
            controller.controls.lock().unwrap().remove(&run.id);
            controller.active.fetch_sub(1, Ordering::SeqCst);
        });
        Ok(id)
    }
    pub fn cancel(&self, id: &str) -> bool {
        if let Some(token) = self.controls.lock().unwrap().get(id) {
            token.cancel();
            self.runtime.cancel(id);
            true
        } else {
            false
        }
    }
    pub fn remove(&self, id: &str) -> PluginResult<()> {
        if self.controls.lock().unwrap().contains_key(id) {
            return Err(PluginError::new("conflict", "运行中的任务不能删除"));
        }
        let guard = self.guard(id)?;
        guard
            .try_lock()
            .map_err(|_| PluginError::new("conflict", "任务正在其他进程中运行"))?;
        std::fs::remove_file(self.path(id)?)?;
        let summary = self.root.root().join("_index").join(format!("{id}.json"));
        if summary.exists() {
            std::fs::remove_file(summary)?;
        }
        Ok(())
    }
}
