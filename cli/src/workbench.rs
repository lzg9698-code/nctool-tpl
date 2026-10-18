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
/// Readable text for a caught panic payload, without leaking panic internals.
fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    let detail = payload
        .downcast_ref::<&str>()
        .map(|text| (*text).to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_default();
    if detail.is_empty() {
        "任务执行发生内部错误".into()
    } else {
        format!("任务执行发生内部错误：{detail}")
    }
}
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
            if !["queued", "running"].contains(&summary["status"].as_str().unwrap_or("")) {
                continue;
            }
            // The index is derived state: a malformed entry (for example a missing or
            // unusable `id`) must not panic or abort boot for every other run.
            let Some(id) = summary["id"]
                .as_str()
                .filter(|id| nctool_runtime::valid_id(id))
            else {
                continue;
            };
            let guard = controller.guard(id)?;
            if guard.try_lock().is_err() {
                continue;
            }
            let mut run = match controller.get(id) {
                Ok(run) => run,
                // The run record was removed but its derived index entry survived (for
                // example, a stop between the two deletions). The index is rebuildable,
                // so drop the orphan entry instead of failing every boot on it.
                Err(error) if error.code == "not_found" => {
                    controller.prune_summary(id)?;
                    continue;
                }
                // Every other read failure (permission, IO) is real and stays visible.
                Err(error) => return Err(error),
            };
            run.status = "interrupted".into();
            run.finished_at = Some(now());
            run.result = None;
            run.error = Some(PluginError::new(
                "interrupted",
                "上次服务停止时任务尚未完成，可恢复输入后重新执行。",
            ));
            // The authoritative record must be written: a real write failure surfaces here
            // rather than being masked as a successful recovery.
            controller.save_run(&run)?;
            // The index summary is rebuilt from run records, so a failure to refresh it does
            // not invalidate the interrupted state already persisted above, and it self-heals
            // on the next successful write; a rebuildable-index failure must not block boot.
            let _ = controller.save_summary(&run);
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
    /// Release one run's concurrency slot and cancellation entry exactly once.
    ///
    /// Called from a `Drop` guard so a panic anywhere in the run thread cannot leave the
    /// workbench permanently at its concurrency limit or with an undeletable run id.
    fn release(&self, id: &str) {
        let removed = self.controls.lock().unwrap().remove(id);
        if removed.is_some() {
            self.active.fetch_sub(1, Ordering::SeqCst);
        }
    }
    fn save(&self, run: &Run) -> PluginResult<()> {
        self.save_run(run)?;
        self.save_summary(run)
    }
    /// Persist the authoritative run record. A failure here is a real write failure and
    /// must never be swallowed.
    fn save_run(&self, run: &Run) -> PluginResult<()> {
        let bytes = serde_json::to_vec(run)?;
        if bytes.len() > 64 * 1024 * 1024 {
            return Err(PluginError::new("record_limit", "任务记录超过 64 MiB 上限"));
        }
        WriteKernel::write_atomic(&self.path(&run.id)?, &bytes).map_err(crate::asset_error)
    }
    /// Refresh the derived `_index` summary for a run. The index is rebuilt from the run
    /// records, so it is written separately from [`Self::save_run`]: an index-only
    /// failure leaves the authoritative record intact.
    fn save_summary(&self, run: &Run) -> PluginResult<()> {
        let folder = self.root.root().join("_index");
        std::fs::create_dir_all(&folder)?;
        let summary = json!({"id":run.id,"title":run.title,"action":run.action,"status":run.status,"started_at":run.started_at,"finished_at":run.finished_at,"diagnostic_count":run.result.as_ref().map(|r|r.diagnostics.len()).or_else(||run.error.as_ref().map(|e|e.diagnostics.len())).unwrap_or(0)});
        WriteKernel::write_atomic(
            &folder.join(format!("{}.json", run.id)),
            &serde_json::to_vec(&summary)?,
        )
        .map_err(crate::asset_error)
    }
    /// Drop a derived `_index` entry whose authoritative run record no longer exists.
    fn prune_summary(&self, id: &str) -> PluginResult<()> {
        let path = self.root.root().join("_index").join(format!("{id}.json"));
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        Ok(())
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
            if !entry.file_type()?.is_file()
                || !entry.path().extension().is_some_and(|e| e == "json")
            {
                continue;
            }
            // The index is derived state: a single unreadable, oversized or unparseable
            // entry must not block boot or every history request. It is skipped like any
            // other unusable summary and re-created on the next write of that run.
            let Ok(bytes) = std::fs::read(entry.path()) else {
                continue;
            };
            if bytes.len() > 1024 * 1024 {
                continue;
            }
            if let Ok(summary) = parse_json(&bytes) {
                summaries.push(summary);
            }
        }
        Ok(summaries)
    }
    fn sort_summaries(summaries: &mut [Value]) {
        summaries.sort_by(|a, b| {
            b["started_at"]
                .as_u64()
                .unwrap_or(0)
                .cmp(&a["started_at"].as_u64().unwrap_or(0))
                .then_with(|| {
                    a["id"]
                        .as_str()
                        .unwrap_or_default()
                        .cmp(b["id"].as_str().unwrap_or_default())
                })
        });
    }
    pub fn list(&self) -> PluginResult<Value> {
        let mut summaries = self.summaries()?;
        summaries.sort_by_key(|r| std::cmp::Reverse(r["started_at"].as_u64().unwrap_or(0)));
        Ok(json!({"runs":summaries.into_iter().take(100).collect::<Vec<_>>()}))
    }
    pub fn query(&self, request: Value) -> PluginResult<Value> {
        let request = request
            .as_object()
            .ok_or_else(|| PluginError::new("invalid_input", "query body must be an object"))?;
        let query = match request.get("query") {
            None => None,
            Some(Value::String(value)) => Some(value.to_lowercase()),
            Some(_) => return Err(PluginError::new("invalid_input", "query must be a string")),
        };
        let status = match request.get("status") {
            None => None,
            Some(Value::String(value)) => {
                if ![
                    "queued",
                    "running",
                    "succeeded",
                    "failed",
                    "cancelled",
                    "interrupted",
                ]
                .contains(&value.as_str())
                {
                    return Err(PluginError::new("invalid_input", "invalid status"));
                }
                Some(value.as_str())
            }
            Some(_) => return Err(PluginError::new("invalid_input", "status must be a string")),
        };
        let offset = match request.get("offset") {
            None => 0,
            Some(value) => value
                .as_u64()
                .and_then(|v| usize::try_from(v).ok())
                .ok_or_else(|| PluginError::new("invalid_input", "offset must be a usize"))?,
        };
        let limit = match request.get("limit") {
            None => 25,
            Some(value) => value
                .as_u64()
                .and_then(|v| usize::try_from(v).ok())
                .filter(|v| (1..=100).contains(v))
                .ok_or_else(|| {
                    PluginError::new("invalid_input", "limit must be between 1 and 100")
                })?,
        };

        let mut summaries = self.summaries()?;
        Self::sort_summaries(&mut summaries);
        summaries.retain(|run| {
            let matches_status = status.is_none_or(|s| run["status"].as_str() == Some(s));
            let matches_query = query.as_ref().is_none_or(|q| {
                ["title", "id", "action"].iter().any(|field| {
                    run[field]
                        .as_str()
                        .is_some_and(|value| value.to_lowercase().contains(q))
                })
            });
            matches_status && matches_query
        });
        let total = summaries.len();
        let runs: Vec<_> = summaries.into_iter().skip(offset).take(limit).collect();
        Ok(json!({"runs":runs,"total":total,"offset":offset,"limit":limit}))
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
        let descriptor = self
            .runtime
            .action_descriptor(action)
            .ok_or_else(|| PluginError::new("action_not_found", action))?;
        nctool_runtime::validate_schema(&descriptor.input_schema, &input, "invalid_input")?;
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
        struct Released(Arc<Workbench>, String);
        impl Drop for Released {
            fn drop(&mut self) {
                self.0.release(&self.1);
            }
        }
        let _released = Released(self.clone(), id.clone());
        std::thread::spawn(move || {
            let _released = _released;
            let _guard = guard;
            let mut run = run;
            run.status = "running".into();
            let _ = controller.save(&run);
            // A panic in a plugin call must not leak the concurrency slot or the
            // cancellation entry: those are released by the guard below, and the run is
            // recorded as a failure instead of staying "running" forever.
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
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
                    // The input schema requires an object, but never panic on untrusted
                    // input: without a map the snapshot is still attached below.
                    if let Some(fields) = run.input.as_object_mut() {
                        for field in ["source", "template", "templates"] {
                            fields.remove(field);
                        }
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
            }));
            let outcome = match outcome {
                Ok(outcome) => outcome,
                Err(panic) => Err(PluginError::new("internal_error", panic_message(panic))),
            };
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
            // The slot, controls entry and cancellation token are released by `_released`.
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
