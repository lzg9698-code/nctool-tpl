//! Full-duplex JSON-RPC transport. A dedicated reader routes responses and host calls.
use nctool_plugin_sdk::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        mpsc, Arc, Mutex, Weak,
    },
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Limits {
    pub timeout_ms: u64,
    pub max_message_bytes: usize,
    pub max_concurrency: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            timeout_ms: 30_000,
            max_message_bytes: 1024 * 1024,
            max_concurrency: 8,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalManifest {
    pub descriptor: PluginDescriptor,
    pub command: Vec<String>,
    pub actions: Vec<ActionDescriptor>,
    #[serde(default)]
    pub limits: Limits,
}
impl ExternalManifest {
    pub fn read(root: &Path) -> PluginResult<Self> {
        let metadata = std::fs::symlink_metadata(root.join("plugin.json"))?;
        if !metadata.is_file() || metadata.len() > 1024 * 1024 {
            return Err(PluginError::new(
                "invalid_manifest",
                "plugin.json must be a regular file under 1 MiB",
            ));
        }
        let manifest: Self =
            serde_json::from_value(parse_json(&std::fs::read(root.join("plugin.json"))?)?)?;
        manifest.validate()?;
        Ok(manifest)
    }
    pub fn validate(&self) -> PluginResult<()> {
        crate::validate_descriptor(&self.descriptor, &self.actions)?;
        if self.command.is_empty() || self.command.iter().any(|s| s.contains('\0')) {
            return Err(PluginError::new(
                "invalid_command",
                "command must be an argv array",
            ));
        }
        if !(10..=3_600_000).contains(&self.limits.timeout_ms)
            || !(1024..=16 * 1024 * 1024).contains(&self.limits.max_message_bytes)
            || !(1..=64).contains(&self.limits.max_concurrency)
        {
            return Err(PluginError::new(
                "invalid_limits",
                "plugin limits exceed supported bounds",
            ));
        }
        Ok(())
    }
}
struct Pending {
    tx: mpsc::Sender<PluginResult<Value>>,
    context: Context,
}
struct Transport {
    input: mpsc::SyncSender<Vec<u8>>,
    child: Mutex<Child>,
    pending: Mutex<BTreeMap<String, Pending>>,
    counter: AtomicU64,
    failed: AtomicBool,
    callbacks: AtomicUsize,
    limits: Limits,
}
impl Transport {
    fn send(&self, message: Value) -> PluginResult<()> {
        if self.failed.load(Ordering::SeqCst) {
            return Err(PluginError::new("plugin_exited", "plugin transport closed"));
        }
        let mut bytes = serde_json::to_vec(&message)?;
        if bytes.len() > self.limits.max_message_bytes {
            return Err(PluginError::new(
                "message_limit",
                "outgoing RPC message exceeds limit",
            ));
        }
        bytes.push(b'\n');
        self.input
            .try_send(bytes)
            .map_err(|e| PluginError::new("plugin_busy", e.to_string()))
    }
    fn request(&self, method: &str, params: Value, ctx: &Context) -> PluginResult<Value> {
        let id = format!("host-{}", self.counter.fetch_add(1, Ordering::SeqCst));
        let (tx, rx) = mpsc::channel();
        {
            let mut pending = self.pending.lock().unwrap();
            if pending.len() >= self.limits.max_concurrency {
                return Err(PluginError::new(
                    "plugin_busy",
                    "plugin concurrency limit reached",
                ));
            }
            pending.insert(
                id.clone(),
                Pending {
                    tx,
                    context: ctx.clone(),
                },
            );
        }
        let result = (|| {
            self.send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))?;
            let deadline = Instant::now() + Duration::from_millis(self.limits.timeout_ms);
            loop {
                if ctx.cancellation.is_cancelled() {
                    let _ =
                        self.send(json!({"jsonrpc":"2.0","method":"cancel","params":{"id":id}}));
                    self.fail(PluginError::new("cancelled", "plugin request cancelled"));
                    return Err(PluginError::new("cancelled", "request cancelled"));
                }
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    let _ =
                        self.send(json!({"jsonrpc":"2.0","method":"cancel","params":{"id":id}}));
                    self.fail(PluginError::new(
                        "plugin_timeout",
                        "plugin exceeded its request deadline",
                    ));
                    return Err(PluginError::new(
                        "plugin_timeout",
                        "plugin exceeded its request deadline",
                    ));
                }
                match rx.recv_timeout(remaining.min(Duration::from_millis(20))) {
                    Ok(result) => return result,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(_) => return Err(PluginError::new("plugin_exited", "plugin disconnected")),
                }
            }
        })();
        self.pending.lock().unwrap().remove(&id);
        result
    }
    fn fail(&self, error: PluginError) {
        if !self.failed.swap(true, Ordering::SeqCst) {
            for (_, pending) in std::mem::take(&mut *self.pending.lock().unwrap()) {
                let _ = pending.tx.send(Err(error.clone()));
            }
            let mut child = self.child.lock().unwrap();
            let _ = child.kill();
            let _ = child.wait();
        }
    }
    fn receive(self: &Arc<Self>, message: Value) -> PluginResult<()> {
        if message.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
            return Err(PluginError::new("rpc_protocol", "expected jsonrpc 2.0"));
        }
        if let Some(method) = message.get("method").and_then(Value::as_str) {
            if method != "host.call" {
                return Err(PluginError::new(
                    "rpc_protocol",
                    format!("unsupported plugin method: {method}"),
                ));
            }
            let id = message
                .get("id")
                .cloned()
                .filter(|v| v.is_string() || v.is_number())
                .ok_or_else(|| PluginError::new("rpc_protocol", "host.call requires an ID"))?;
            let params = message.get("params").cloned().unwrap_or(Value::Null);
            let parent = params
                .get("parent_id")
                .and_then(Value::as_str)
                .ok_or_else(|| PluginError::new("rpc_protocol", "host.call requires parent_id"))?;
            let context = self
                .pending
                .lock()
                .unwrap()
                .get(parent)
                .map(|p| p.context.clone())
                .ok_or_else(|| {
                    PluginError::new("rpc_protocol", "host.call parent is no longer active")
                })?;
            if self.callbacks.fetch_add(1, Ordering::SeqCst) >= self.limits.max_concurrency {
                self.callbacks.fetch_sub(1, Ordering::SeqCst);
                return self.send(json!({"jsonrpc":"2.0","id":id,"error":{"code":-32000,"message":"callback limit reached","data":{"code":"plugin_busy","message":"callback limit reached","diagnostics":[]}}}));
            }
            let transport = self.clone();
            std::thread::spawn(move || {
                let result = (|| {
                    let service =
                        params
                            .get("service")
                            .and_then(Value::as_str)
                            .ok_or_else(|| {
                                PluginError::new("rpc_protocol", "host.call requires service")
                            })?;
                    let version = params
                        .get("version")
                        .and_then(Value::as_u64)
                        .and_then(|v| u32::try_from(v).ok())
                        .ok_or_else(|| {
                            PluginError::new("rpc_protocol", "host.call requires version")
                        })?;
                    context.call(
                        service,
                        version,
                        params.get("input").cloned().unwrap_or(Value::Null),
                    )
                })();
                let message = match result {
                    Ok(value) => json!({"jsonrpc":"2.0","id":id,"result":value}),
                    Err(error) => rpc_error(id, error),
                };
                if let Err(error) = transport.send(message) {
                    transport.fail(error);
                }
                transport.callbacks.fetch_sub(1, Ordering::SeqCst);
            });
            return Ok(());
        }
        let id = message.get("id").and_then(Value::as_str).ok_or_else(|| {
            PluginError::new("rpc_protocol", "response requires string request ID")
        })?;
        if message.get("result").is_some() == message.get("error").is_some() {
            return Err(PluginError::new(
                "rpc_protocol",
                "response requires exactly one result or error",
            ));
        }
        let pending = self
            .pending
            .lock()
            .unwrap()
            .remove(id)
            .ok_or_else(|| PluginError::new("rpc_protocol", "unknown response ID"))?;
        let result = if let Some(error) = message.get("error") {
            Err(error
                .get("data")
                .cloned()
                .and_then(|d| serde_json::from_value(d).ok())
                .unwrap_or_else(|| {
                    PluginError::new(
                        "plugin_error",
                        error
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("plugin error"),
                    )
                }))
        } else {
            Ok(message["result"].clone())
        };
        let _ = pending.tx.send(result);
        Ok(())
    }
}
impl Drop for Transport {
    fn drop(&mut self) {
        let child = self.child.get_mut().unwrap();
        let _ = child.kill();
        let _ = child.wait();
    }
}
fn rpc_error(id: Value, error: PluginError) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":-32000,"message":error.message,"data":error}})
}
fn read_frame(reader: &mut impl BufRead, limit: usize) -> PluginResult<Option<Value>> {
    let mut line = Vec::new();
    loop {
        let buf = reader.fill_buf()?;
        if buf.is_empty() {
            return if line.is_empty() {
                Ok(None)
            } else {
                Err(PluginError::new("rpc_protocol", "unterminated JSON line"))
            };
        }
        let newline = buf.iter().position(|b| *b == b'\n');
        let count = newline.map(|n| n + 1).unwrap_or(buf.len());
        if line.len() + count > limit + 1 {
            return Err(PluginError::new(
                "message_limit",
                "incoming RPC message exceeds limit",
            ));
        }
        line.extend_from_slice(&buf[..count]);
        reader.consume(count);
        if newline.is_some() {
            return parse_json(&line)
                .map(Some)
                .map_err(|e| PluginError::new("rpc_protocol", e.to_string()));
        }
    }
}
pub struct ExternalPlugin {
    manifest: ExternalManifest,
    root: PathBuf,
    transport: Mutex<Option<Arc<Transport>>>,
}
impl ExternalPlugin {
    pub fn new(manifest: ExternalManifest, root: PathBuf) -> Self {
        Self {
            manifest,
            root,
            transport: Mutex::new(None),
        }
    }
    fn start(&self, ctx: &Context) -> PluginResult<Arc<Transport>> {
        let mut slot = self.transport.lock().unwrap();
        if let Some(t) = slot.as_ref().filter(|t| !t.failed.load(Ordering::SeqCst)) {
            return Ok(t.clone());
        }
        self.manifest.validate()?;
        let program = &self.manifest.command[0];
        let program = if program.starts_with("./") || program.starts_with(".\\") {
            self.root.join(program)
        } else {
            PathBuf::from(program)
        };
        let mut child = Command::new(program)
            .args(&self.manifest.command[1..])
            .current_dir(&self.root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let mut stdin = child.stdin.take().unwrap();
        let output = child.stdout.take().unwrap();
        let (input, outgoing) =
            mpsc::sync_channel::<Vec<u8>>(self.manifest.limits.max_concurrency * 2);
        let transport = Arc::new(Transport {
            input,
            child: Mutex::new(child),
            pending: Mutex::new(BTreeMap::new()),
            counter: AtomicU64::new(1),
            failed: AtomicBool::new(false),
            callbacks: AtomicUsize::new(0),
            limits: self.manifest.limits.clone(),
        });
        let writer = Arc::downgrade(&transport);
        std::thread::spawn(move || {
            while let Ok(bytes) = outgoing.recv() {
                if let Err(e) = stdin.write_all(&bytes).and_then(|_| stdin.flush()) {
                    if let Some(t) = writer.upgrade() {
                        t.fail(PluginError::new("plugin_exited", e.to_string()));
                    }
                    break;
                }
            }
        });
        let weak: Weak<Transport> = Arc::downgrade(&transport);
        let limit = self.manifest.limits.max_message_bytes;
        std::thread::spawn(move || {
            let mut reader = BufReader::new(output);
            loop {
                let frame = read_frame(&mut reader, limit);
                let Some(t) = weak.upgrade() else { break };
                match frame {
                    Ok(Some(message)) => {
                        if let Err(error) = t.receive(message) {
                            t.fail(error);
                            break;
                        }
                    }
                    Ok(None) => {
                        t.fail(PluginError::new("plugin_exited", "plugin stdout closed"));
                        break;
                    }
                    Err(error) => {
                        t.fail(error);
                        break;
                    }
                }
            }
        });
        let hello=transport.request("initialize",json!({"protocol_version":PROTOCOL_VERSION,"plugin_id":self.manifest.descriptor.id,"config":ctx.config}),ctx)?;
        if hello.get("protocol_version").and_then(Value::as_u64) != Some(PROTOCOL_VERSION as u64)
            || hello.get("plugin_id").and_then(Value::as_str) != Some(&self.manifest.descriptor.id)
        {
            transport.fail(PluginError::new(
                "protocol_version",
                "plugin handshake differs from manifest",
            ));
            return Err(PluginError::new(
                "protocol_version",
                "plugin handshake differs from manifest",
            ));
        }
        let described = transport.request("describe", json!({}), ctx)?;
        let expected =
            json!({"descriptor":self.manifest.descriptor,"actions":self.manifest.actions});
        if described != expected {
            transport.fail(PluginError::new(
                "capability_mismatch",
                "plugin capabilities differ from installed manifest",
            ));
            return Err(PluginError::new(
                "capability_mismatch",
                "plugin capabilities differ from installed manifest",
            ));
        }
        *slot = Some(transport.clone());
        Ok(transport)
    }
}
impl Plugin for ExternalPlugin {
    fn descriptor(&self) -> PluginDescriptor {
        self.manifest.descriptor.clone()
    }
    fn actions(&self) -> Vec<ActionDescriptor> {
        self.manifest.actions.clone()
    }
    fn activate(&self, ctx: &Context) -> PluginResult<()> {
        self.start(ctx).map(|_| ())
    }
    fn invoke(&self, action: &str, input: Value, ctx: &Context) -> PluginResult<ActionResult> {
        let value =
            self.start(ctx)?
                .request("invoke", json!({"action":action,"input":input}), ctx)?;
        serde_json::from_value(value).map_err(|e| PluginError::new("rpc_protocol", e.to_string()))
    }
    fn shutdown(&self) {
        if let Some(t) = self.transport.lock().unwrap().take() {
            let _ = t.send(json!({"jsonrpc":"2.0","method":"shutdown","params":{}}));
            t.fail(PluginError::new("closed", "plugin stopped"));
        }
    }
}
