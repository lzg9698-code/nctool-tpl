use nctool_plugin_sdk::*;
use nctool_runtime::{
    config,
    external::{ExternalManifest, ExternalPlugin},
    Runtime,
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "nctool-rpc-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&p).unwrap();
        Self(p)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn python() -> String {
    for p in ["python3", "python"] {
        if std::process::Command::new(p)
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success())
        {
            return p.into();
        }
    }
    panic!("Python is required for external-plugin integration tests")
}
fn fixture(root: &Path, timeout: u64) {
    let sample = Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/plugins/python-report");
    for name in ["plugin.json", "plugin.py", "nctool_plugin.py"] {
        std::fs::copy(sample.join(name), root.join(name)).unwrap();
    }
    let mut manifest: Value =
        serde_json::from_slice(&std::fs::read(root.join("plugin.json")).unwrap()).unwrap();
    manifest["command"] = json!([python(), "fault.py"]);
    manifest["limits"]["timeout_ms"] = json!(timeout);
    manifest["limits"]["max_message_bytes"] = json!(4096);
    manifest["actions"][0]["input_schema"]["properties"]["mode"] = json!({"type":"string"});
    std::fs::write(
        root.join("plugin.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    std::fs::write(root.join("fault.py"),r#"import json,sys,time,os
from pathlib import Path
from nctool_plugin import PluginServer
from plugin import invoke as compute

def invoke(action,data,server,request_id):
    mode=data.pop('mode','')
    if mode=='crash': os._exit(9)
    if mode=='sleep': time.sleep(2)
    if mode=='malformed':
        print('not-json',flush=True)
        time.sleep(2)
    if mode=='oversized':
        print('x'*8192,flush=True)
        time.sleep(2)
    if mode=='undeclared': server.call(request_id,'template.inspect',{'source':'Hi'})
    if mode=='bad-output': return {'data':{'total':'bad','text':'bad'},'artifacts':[],'diagnostics':[]}
    return compute(action,data,server,request_id)

manifest=json.loads(Path(__file__).with_name('plugin.json').read_text(encoding='utf-8'))
PluginServer(manifest,invoke).run()
"#).unwrap();
}
fn boot(root: &Path, workspace: &Path) -> Arc<Runtime> {
    let manifest = ExternalManifest::read(root).unwrap();
    Runtime::start(
        vec![
            Arc::new(nctool_plugin_template::TemplatePlugin::new(workspace).unwrap()),
            Arc::new(ExternalPlugin::new(manifest, root.into())),
        ],
        BTreeMap::new(),
        BTreeMap::new(),
    )
    .unwrap()
}
#[test]
fn install_enable_handshake_host_callback_and_restart() {
    let temp = Temp::new();
    let source = temp.0.join("source");
    std::fs::create_dir(&source).unwrap();
    fixture(&source, 1000);
    let home = temp.0.join("home");
    assert_eq!(config::install(&home, &source).unwrap(), "python-report");
    assert_eq!(
        config::install(&home, &source).unwrap_err().code,
        "conflict"
    );
    config::set_enabled(&home, "python-report", true).unwrap();
    let (c, _) = config::load(&home).unwrap();
    let mut plugins = config::external_plugins(&home, &c).unwrap();
    plugins.push(Arc::new(
        nctool_plugin_template::TemplatePlugin::new(&temp.0.join("workspace")).unwrap(),
    ));
    let runtime = Runtime::start(plugins, c.plugins, c.providers).unwrap();
    let result = runtime
        .invoke("report.compute", json!({"values":[10,20,30]}))
        .unwrap();
    assert_eq!(result.data["total"], 60);
    assert!(result.data["text"].as_str().unwrap().contains("Total: 60"));
    runtime.shutdown();
    assert!(runtime.capabilities().is_empty());
    let runtime = boot(
        &home.join("plugins/python-report"),
        &temp.0.join("workspace"),
    );
    assert!(runtime
        .invoke("report.compute", json!({"values":[1]}))
        .is_ok());
}
#[test]
fn faults_are_bounded_and_next_invocation_recovers() {
    let temp = Temp::new();
    fixture(&temp.0, 300);
    let runtime = boot(&temp.0, &temp.0.join("workspace"));
    for (mode, code) in [
        ("crash", "plugin_exited"),
        ("malformed", "rpc_protocol"),
        ("oversized", "message_limit"),
        ("sleep", "plugin_timeout"),
    ] {
        let started = Instant::now();
        let error = runtime
            .invoke("report.compute", json!({"values":[1],"mode":mode}))
            .unwrap_err();
        assert_eq!(error.code, code, "{mode}: {error}");
        assert!(started.elapsed() < Duration::from_secs(3));
        assert!(
            runtime
                .invoke("report.compute", json!({"values":[2]}))
                .is_ok(),
            "must recover from {mode}"
        );
    }
    assert_eq!(
        runtime
            .invoke("report.compute", json!({"values":[1],"mode":"bad-output"}))
            .unwrap_err()
            .code,
        "invalid_output"
    );
    let error = runtime
        .invoke("report.compute", json!({"values":[1],"mode":"undeclared"}))
        .unwrap_err();
    assert!(error.message.contains("did not declare"));
}
#[test]
fn cancellation_and_parallel_host_queries_do_not_block() {
    let temp = Temp::new();
    fixture(&temp.0, 5000);
    let runtime = boot(&temp.0, &temp.0.join("workspace"));
    let worker = runtime.clone();
    let thread = std::thread::spawn(move || {
        worker.invoke_request(
            "slow",
            "report.compute",
            json!({"values":[1],"mode":"sleep"}),
        )
    });
    let deadline = Instant::now() + Duration::from_secs(2);
    while !runtime.cancel("slow") {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(runtime
        .invoke("template.render", json!({"source":"OK"}))
        .is_ok());
    assert_eq!(thread.join().unwrap().unwrap_err().code, "cancelled");
    assert!(runtime
        .invoke("report.compute", json!({"values":[2]}))
        .is_ok());
}
#[test]
fn incompatible_manifest_and_package_symlink_rejected() {
    let temp = Temp::new();
    fixture(&temp.0, 1000);
    let mut manifest = ExternalManifest::read(&temp.0).unwrap();
    manifest.descriptor.protocol_version = 2;
    assert_eq!(manifest.validate().unwrap_err().code, "protocol_version");
    #[cfg(unix)]
    {
        let source = temp.0.join("pkg");
        std::fs::create_dir(&source).unwrap();
        fixture(&source, 1000);
        std::os::unix::fs::symlink("/etc/passwd", source.join("escape")).unwrap();
        assert_eq!(
            config::install(&temp.0.join("home"), &source)
                .unwrap_err()
                .code,
            "plugin_symlink"
        );
    }
}
#[test]
fn blocked_stdin_writer_is_interrupted_by_timeout() {
    let temp = Temp::new();
    fixture(&temp.0, 300);
    let mut m: Value =
        serde_json::from_slice(&std::fs::read(temp.0.join("plugin.json")).unwrap()).unwrap();
    m["command"] = json!([python(), "blocked.py"]);
    m["limits"]["max_message_bytes"] = json!(1048576);
    std::fs::write(temp.0.join("plugin.json"), m.to_string()).unwrap();
    std::fs::write(
        temp.0.join("blocked.py"),
        r#"import json,sys,time
from pathlib import Path
m=json.loads(Path(__file__).with_name('plugin.json').read_text(encoding='utf-8'))
for line in sys.stdin:
    r=json.loads(line)
    if r['method']=='initialize': result={'plugin_id':m['descriptor']['id'],'protocol_version':1}
    else: result={'descriptor':m['descriptor'],'actions':m['actions']}
    print(json.dumps({'jsonrpc':'2.0','id':r['id'],'result':result}),flush=True)
    if r['method']=='describe':
        while True: time.sleep(1)
"#,
    )
    .unwrap();
    let runtime = boot(&temp.0, &temp.0.join("workspace"));
    let start = Instant::now();
    assert_eq!(
        runtime
            .invoke(
                "report.compute",
                json!({"values":[1],"title":"x".repeat(900_000)})
            )
            .unwrap_err()
            .code,
        "plugin_timeout"
    );
    assert!(start.elapsed() < Duration::from_secs(3));
}
