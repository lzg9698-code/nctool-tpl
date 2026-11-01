//! Minimal external-plugin client for NCtool protocol v2.
//!
//! Transport rules mirrored from the host (`runtime/src/external.rs`):
//! - stdout carries newline-terminated UTF-8 JSON-RPC only; logs go to stderr.
//! - Host requests have string ids; responses must echo the id verbatim.
//! - `cancel`/`shutdown` are notifications; `initialize`/`describe`/`invoke`
//!   are requests handled on worker threads so the reader stays live while an
//!   invoke waits on a `host.call` callback.
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Write};
use std::process;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

pub const PROTOCOL_VERSION: u32 = 2;

#[derive(Debug, Clone)]
pub struct PluginError {
    pub code: String,
    pub message: String,
    pub diagnostics: Vec<Value>,
}

impl PluginError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            diagnostics: vec![],
        }
    }
    fn into_error(self, id: &Value) -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {
                "code": -32000,
                "message": self.message,
                "data": {"code": self.code, "message": self.message, "diagnostics": self.diagnostics},
            }
        })
    }
}

impl std::fmt::Display for PluginError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}
impl std::error::Error for PluginError {}
impl From<std::io::Error> for PluginError {
    fn from(e: std::io::Error) -> Self {
        Self::new("io_error", e.to_string())
    }
}
impl From<serde_json::Error> for PluginError {
    fn from(e: serde_json::Error) -> Self {
        Self::new("invalid_json", e.to_string())
    }
}

pub type PluginResult<T> = Result<T, PluginError>;

/// Callback implemented by the plugin: (action, input, server, host_request_id).
pub type Invoke = dyn Fn(&str, &Value, &Arc<Server>, &str) -> PluginResult<Value> + Send + Sync;

pub struct Server {
    manifest: Value,
    config: Mutex<Value>,
    stdout: Mutex<std::io::Stdout>,
    pending: Mutex<HashMap<String, mpsc::Sender<PluginResult<Value>>>>,
    counter: AtomicU64,
    cancelled: Mutex<HashSet<String>>,
    shutdown: AtomicBool,
    invoke: Box<Invoke>,
}

impl Server {
    pub fn new(
        manifest: Value,
        invoke: impl Fn(&str, &Value, &Arc<Server>, &str) -> PluginResult<Value> + Send + Sync + 'static,
    ) -> Arc<Self> {
        Arc::new(Self {
            manifest,
            config: Mutex::new(json!({})),
            stdout: Mutex::new(std::io::stdout()),
            pending: Mutex::new(HashMap::new()),
            counter: AtomicU64::new(0),
            cancelled: Mutex::new(HashSet::new()),
            shutdown: AtomicBool::new(false),
            invoke: Box::new(invoke),
        })
    }

    /// Plugin configuration delivered by `initialize`.
    pub fn config(&self) -> Value {
        self.config.lock().unwrap().clone()
    }

    /// Whether the host cancelled this in-flight request id. Invoke handlers
    /// should poll this between units of work and return a `cancelled` error.
    pub fn is_cancelled(&self, request_id: &str) -> bool {
        self.cancelled.lock().unwrap().contains(request_id)
    }

    pub fn check_cancelled(&self, request_id: &str) -> PluginResult<()> {
        if self.is_cancelled(request_id) {
            Err(PluginError::new("cancelled", "request cancelled"))
        } else {
            Ok(())
        }
    }

    fn send(&self, message: Value) {
        let encoded = serde_json::to_string(&message).expect("protocol message serializes");
        let mut stdout = self.stdout.lock().unwrap();
        let _ = stdout.write_all(encoded.as_bytes());
        let _ = stdout.write_all(b"\n");
        let _ = stdout.flush();
    }

    /// Call a host service declared in `descriptor.requires`.
    pub fn call(&self, parent_id: &str, service: &str, input: Value) -> PluginResult<Value> {
        self.call_version(parent_id, service, 1, input)
    }

    pub fn call_version(
        &self,
        parent_id: &str,
        service: &str,
        version: u32,
        input: Value,
    ) -> PluginResult<Value> {
        let id = format!("plugin-{}", self.counter.fetch_add(1, Ordering::SeqCst));
        let (tx, rx) = mpsc::channel();
        self.pending.lock().unwrap().insert(id.clone(), tx);
        self.send(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "host.call",
            "params": {"parent_id": parent_id, "service": service, "version": version, "input": input},
        }));
        // Generous local deadline: the host enforces its own per-request limits;
        // this only protects against a wedged transport.
        let result = rx
            .recv_timeout(Duration::from_secs(600))
            .unwrap_or_else(|_| Err(PluginError::new("host_timeout", "host.call timed out")));
        self.pending.lock().unwrap().remove(&id);
        result
    }

    fn respond(&self, id: Value, result: PluginResult<Value>) {
        let message = match result {
            Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
            Err(error) => error.into_error(&id),
        };
        self.send(message);
    }

    fn handle_request(self: &Arc<Self>, message: &Value) -> PluginResult<Value> {
        let method = message["method"].as_str().unwrap_or_default();
        match method {
            "initialize" => {
                let params = &message["params"];
                if params["protocol_version"].as_u64() != Some(PROTOCOL_VERSION as u64) {
                    return Err(PluginError::new(
                        "protocol_version",
                        "unsupported protocol version",
                    ));
                }
                *self.config.lock().unwrap() = params.get("config").cloned().unwrap_or(json!({}));
                Ok(json!({
                    "protocol_version": PROTOCOL_VERSION,
                    "plugin_id": self.manifest["descriptor"]["id"],
                }))
            }
            "describe" => Ok(json!({
                "descriptor": self.manifest["descriptor"],
                "actions": self.manifest["actions"],
            })),
            "invoke" => {
                let params = &message["params"];
                let action = params["action"].as_str().unwrap_or_default();
                let input = params.get("input").cloned().unwrap_or(Value::Null);
                let request_id = message["id"].as_str().unwrap_or_default().to_string();
                (self.invoke)(action, &input, self, &request_id)
            }
            other => Err(PluginError::new(
                "rpc_protocol",
                format!("unsupported method: {other}"),
            )),
        }
    }

    /// Read the manifest next to the executable entry file.
    pub fn manifest_from(path: &std::path::Path) -> PluginResult<Value> {
        Ok(serde_json::from_str(
            &std::fs::read_to_string(path).map_err(|e| {
                PluginError::new(
                    "invalid_manifest",
                    format!("cannot read {}: {e}", path.display()),
                )
            })?,
        )?)
    }

    /// Main loop: stdin reader routes responses and dispatches requests.
    pub fn run(self: &Arc<Self>) {
        let stdin = std::io::stdin();
        let mut reader = BufReader::new(stdin.lock());
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
            let message: Value = match serde_json::from_str(line.trim_end()) {
                Ok(v) => v,
                Err(_) => continue, // malformed frame has no id to answer
            };
            if message.get("method").is_some() {
                match message["method"].as_str() {
                    Some("shutdown") => break,
                    Some("cancel") => {
                        if let Some(id) = message["params"]["id"].as_str() {
                            self.cancelled.lock().unwrap().insert(id.to_string());
                        }
                    }
                    _ => {
                        let server = self.clone();
                        std::thread::spawn(move || {
                            let id = message["id"].clone();
                            let result = server.handle_request(&message);
                            server.respond(id, result);
                        });
                    }
                }
            } else if let Some(id) = message.get("id") {
                let key = match id {
                    Value::String(s) => s.clone(),
                    Value::Number(n) => n.to_string(),
                    _ => continue,
                };
                let tx = self.pending.lock().unwrap().remove(&key);
                if let Some(tx) = tx {
                    let result = if let Some(error) = message.get("error") {
                        let detail = &error["data"];
                        let code = detail["code"].as_str().unwrap_or("plugin_error");
                        let message_text = detail["message"]
                            .as_str()
                            .or_else(|| error["message"].as_str())
                            .unwrap_or("host error");
                        let diagnostics = detail["diagnostics"]
                            .as_array()
                            .cloned()
                            .unwrap_or_default();
                        Err(PluginError {
                            code: code.into(),
                            message: message_text.into(),
                            diagnostics,
                        })
                    } else {
                        Ok(message["result"].clone())
                    };
                    let _ = tx.send(result);
                }
            }
        }
        self.shutdown.store(true, Ordering::SeqCst);
        // Release any in-flight host.call waiters.
        for (_, tx) in std::mem::take(&mut *self.pending.lock().unwrap()) {
            let _ = tx.send(Err(PluginError::new(
                "host_disconnected",
                "host disconnected",
            )));
        }
        process::exit(0);
    }
}
