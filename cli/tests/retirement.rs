use nctool_cli::composition::App;
use nctool_plugin_sdk::*;
use std::{
    path::PathBuf,
    process::Command,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

/// A process-wide counter keeps parallel tests from colliding when the system
/// clock returns the same nanosecond value for two of them.
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "nctool-retirement-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        Self(root)
    }
    fn home(&self) -> PathBuf {
        self.0.join("home")
    }
    fn workspace(&self) -> PathBuf {
        self.0.join("workspace")
    }
    fn app(&self) -> Arc<App> {
        App::boot(&self.home(), &self.workspace(), None).unwrap()
    }
    fn config(&self, config: Value) -> Vec<u8> {
        std::fs::create_dir_all(self.home()).unwrap();
        let bytes = serde_json::to_vec_pretty(&config).unwrap();
        std::fs::write(self.home().join("config.json"), &bytes).unwrap();
        bytes
    }
    fn cli(&self, args: &[&str]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_nctool"))
            .arg("--home")
            .arg(self.home())
            .arg("--workspace")
            .arg(self.workspace())
            .args(args)
            .output()
            .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn legacy_config_check_is_read_only_and_migration_has_exact_backup_and_conflict_guards() {
    let f = Fixture::new();
    let original = f.config(json!({"profile":"nc","enabled":["nc","process","template"],"disabled":["undercut"],"providers":{"nc.generate":"nc"},"plugins":{"nc":{"custom":"preserved in backup"},"template":{}}}));
    let preview = App::migration_check_at(&f.home()).unwrap();
    assert_eq!(preview["valid"], true);
    assert_eq!(preview["proposed"]["profile"], "template");
    assert_eq!(preview["proposed"]["enabled"], json!(["template"]));
    assert_eq!(preview["changes"].as_array().unwrap().len(), 6);
    assert_eq!(
        std::fs::read(f.home().join("config.json")).unwrap(),
        original
    );
    assert_eq!(std::fs::read_dir(f.home()).unwrap().count(), 1);
    assert!(!f.workspace().exists());
    let checked = f.cli(&["config", "check"]);
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&checked.stdout).unwrap(),
        preview
    );
    assert!(!f.workspace().exists());
    let expected = preview["fingerprint"].as_str().unwrap();
    let updated = f.config(json!({"profile":"nc","enabled":["nc"],"plugins":{}}));
    let conflict = f.cli(&["config", "migrate", "--expected", expected]);
    assert!(!conflict.status.success());
    assert!(String::from_utf8_lossy(&conflict.stderr).contains("conflict"));
    assert_eq!(
        std::fs::read(f.home().join("config.json")).unwrap(),
        updated
    );
    assert_eq!(std::fs::read_dir(f.home()).unwrap().count(), 1);
    let next = App::migration_check_at(&f.home()).unwrap();
    let applied = f.cli(&[
        "config",
        "migrate",
        "--expected",
        next["fingerprint"].as_str().unwrap(),
    ]);
    assert!(
        applied.status.success(),
        "{}",
        String::from_utf8_lossy(&applied.stderr)
    );
    let result: Value = serde_json::from_slice(&applied.stdout).unwrap();
    assert_eq!(
        std::fs::read(result["backup"].as_str().unwrap()).unwrap(),
        updated
    );
    assert_eq!(
        nctool_runtime::config::load(&f.home()).unwrap().0.profile,
        "template"
    );
    assert!(!f.workspace().exists());
    assert_eq!(f.app().profile, "template");
    for command in [
        vec!["config", "check"],
        vec!["config", "migrate", "--expected", expected],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_nctool"))
            .args(command)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("explicit_home_required"));
    }
}

#[test]
fn migration_preserves_external_selection_and_checks_dependencies_without_running_it() {
    let f = Fixture::new();
    let source = f.0.join("source");
    std::fs::create_dir(&source).unwrap();
    let mut descriptor = PluginDescriptor::builtin("reporter");
    descriptor
        .requires
        .push(Requirement::new("template.render"));
    let action = ActionDescriptor::new("reporter.run", "Report", object_schema());
    let manifest = json!({"descriptor":descriptor,"actions":[action],"command":["migration-must-not-start-this-process"]});
    std::fs::write(
        source.join("plugin.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    nctool_runtime::config::install(&f.home(), &source).unwrap();
    let original = f.config(json!({"profile":"nc","enabled":["nc","reporter"],"disabled":["template"],"providers":{"template.render":"template","nc.generate":"nc"},"plugins":{"reporter":{}}}));
    let check = App::migration_check_at(&f.home()).unwrap();
    assert_eq!(check["valid"], false);
    assert_eq!(check["error"]["code"], "provider_missing");
    assert_eq!(check["proposed"]["enabled"], json!(["reporter"]));
    assert_eq!(
        check["proposed"]["providers"],
        json!({"template.render":"template"})
    );
    assert_eq!(check["proposed"]["plugins"], json!({"reporter":{}}));
    let expected = serde_json::from_str(check["fingerprint"].as_str().unwrap()).unwrap();
    assert_eq!(
        App::migration_apply_at(&f.home(), expected)
            .unwrap_err()
            .code,
        "provider_missing"
    );
    assert_eq!(
        std::fs::read(f.home().join("config.json")).unwrap(),
        original
    );
    assert_eq!(std::fs::read_dir(f.home()).unwrap().count(), 2); // config plus installed plugins
    assert!(f.home().join("plugins/reporter/plugin.json").exists());
    assert!(!f.workspace().exists());
}

#[test]
fn retired_nc_plugins_are_gone_and_builtin_ids_remain_reserved() {
    let f = Fixture::new();
    let app = f.app();
    assert_eq!(app.profile, "template");
    let mut active: Vec<_> = app
        .runtime
        .descriptors()
        .into_iter()
        .map(|d| d.id)
        .collect();
    active.sort();
    assert_eq!(active, ["template"]);
    let inventory = app.inventory().unwrap();
    let plugins = inventory["plugins"].as_array().unwrap();
    // Math survives as an optional, non-domain extension.
    nctool_runtime::config::ensure_plugin_allowed("math").unwrap();
    let math = plugins.iter().find(|p| p["id"] == "math").unwrap();
    assert_eq!(math["active"], false);
    assert_eq!(math["available"], cfg!(feature = "math"));
    assert_eq!(math["retired"], false);
    // Every NC identity is retired: unavailable, listed for explanation, and its ID
    // stays reserved so a same-named external plugin cannot take over.
    for id in [
        "nc",
        "nc-parameters",
        "nc-managed",
        "process",
        "process-parameters",
        "sync-turning",
        "undercut",
    ] {
        assert_eq!(
            nctool_runtime::config::ensure_plugin_allowed(id)
                .unwrap_err()
                .code,
            "retired_plugin"
        );
        let plugin = plugins.iter().find(|p| p["id"] == id).unwrap();
        assert_eq!(plugin["active"], false);
        assert_eq!(plugin["available"], false);
        assert_eq!(plugin["retired"], true);
        let source = f.0.join(format!("plugin-{id}"));
        std::fs::create_dir(&source).unwrap();
        let manifest = json!({"descriptor":PluginDescriptor::builtin(id),"actions":[],"command":["never-execute"]});
        std::fs::write(
            source.join("plugin.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        assert_eq!(
            nctool_runtime::config::install(&f.home(), &source)
                .unwrap_err()
                .code,
            "retired_plugin"
        );
    }
    // A builtin that is still present (math) keeps the reserved-ID protection.
    let source = f.0.join("plugin-math");
    std::fs::create_dir(&source).unwrap();
    let manifest = json!({"descriptor":PluginDescriptor::builtin("math"),"actions":[],"command":["never-execute"]});
    std::fs::write(
        source.join("plugin.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    assert_eq!(
        nctool_runtime::config::install(&f.home(), &source)
            .unwrap_err()
            .code,
        "reserved_plugin_id"
    );
    assert!(!f.home().join("config.json").exists());
    // The retired nc profile is rejected instead of silently loading anything.
    assert_eq!(
        App::boot(&f.home(), &f.workspace(), Some("nc"))
            .err()
            .unwrap()
            .code,
        "invalid_profile"
    );
}

#[test]
fn domain_intent_requires_nc_actions_across_live_drafts_snapshots_and_cli() {
    let f = Fixture::new();
    let app = f.app();
    for (index, metadata) in [
        json!({"nc":{}}),
        json!({"nc":null}),
        json!({"execution":{"action":"nc.generate"}}),
        json!({"execution":{"action":"process.generate.managed"}}),
        json!({"execution":{"action":"sync-turning.generate"}}),
    ]
    .into_iter()
    .enumerate()
    {
        let name = format!("old-{index}");
        app.runtime
            .invoke(
                "template.save",
                json!({"name":name,"asset":{"source":"old {{ value }}","metadata":metadata}}),
            )
            .unwrap();
        let snapshot = app
            .runtime
            .invoke(
                "template.inspect",
                json!({"template":name,"include_snapshot":true}),
            )
            .unwrap()
            .data["snapshot"]
            .clone();
        assert_eq!(
            app.runtime
                .invoke("template.read", json!({"name":name}))
                .unwrap()
                .data["asset"]["metadata"],
            metadata
        );
        for input in [
            json!({"template":name,"context":{"value":1}}),
            json!({"template":name,"source":"new {{ value }}","context":{"value":1}}),
            json!({"snapshot":snapshot,"context":{"value":1}}),
        ] {
            assert_eq!(
                app.runtime
                    .invoke("template.render", input.clone())
                    .unwrap_err()
                    .code,
                "domain_action_required"
            );
            assert_eq!(
                app.dispatch(
                    "POST",
                    "/api/v2/actions/template.render",
                    json!({"input":input})
                )
                .unwrap_err()
                .code,
                "domain_action_required"
            );
        }
        let input = f.0.join("input.json");
        std::fs::write(
            &input,
            serde_json::to_vec(&json!({"template":name})).unwrap(),
        )
        .unwrap();
        let out = f.0.join("output.txt");
        let output = f.cli(&[
            "run",
            "template.render",
            "--input",
            input.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ]);
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("domain_action_required"));
        assert!(!out.exists());
    }
    app.runtime.invoke("template.save", json!({"name":"generic","asset":{"source":"{{ payload.value }}:{{ nullable }}","metadata":{"report":{"custom":true},"arbitrary":"metadata"}}})).unwrap();
    assert!(app
        .runtime
        .invoke(
            "template.render",
            json!({"template":"generic","context":{"payload":{"value":2},"nullable":null}})
        )
        .is_ok());
}

#[test]
fn domain_dependencies_are_checked_when_loaded_and_frozen_with_their_intent() {
    let f = Fixture::new();
    let app = f.app();
    app.runtime
        .invoke(
            "template.save",
            json!({"name":"safe","asset":{"source":"SAFE"}}),
        )
        .unwrap();
    for metadata in [
        json!({"nc":null}),
        json!({"nc":{}}),
        json!({"execution":{"action":"nc.generate"}}),
        json!({"execution":{"action":"machine.generate"}}),
        json!({"execution":{"action":"process.generate.managed"}}),
        json!({"execution":{"action":"sync-turning.generate"}}),
        json!({"execution":{"action":"undercut.generate"}}),
    ] {
        let previous = app
            .runtime
            .invoke("template.read", json!({"name":"old"}))
            .ok();
        let expected = previous.as_ref().map(|r| r.data["fingerprint"].clone());
        app.runtime.invoke("template.save", json!({"name":"old","expected":expected,"asset":{"source":"OLD{% macro emit() %}OLD{% endmacro %}{% block body %}OLD{% endblock %}","metadata":metadata}})).unwrap();
        for source in [
            "prefix{% include 'old' %}",
            "{% import 'old' as legacy %}{{ legacy.emit() }}",
            "{% from 'old' import emit %}{{ emit() }}",
            "{% extends 'old' %}",
            "{% include selected %}",
            "{% import selected as legacy %}",
            "{% extends selected %}",
            "{% include 'old' ignore missing %}",
            "{% include ['missing','old','safe'] ignore missing %}",
        ] {
            let input = json!({"source":source,"context":{"selected":"old"}});
            let snapshot = app
                .runtime
                .invoke(
                    "template.inspect",
                    json!({"source":source,"include_snapshot":true}),
                )
                .unwrap()
                .data["snapshot"]
                .clone();
            assert_eq!(snapshot["format_version"], 2);
            assert_eq!(snapshot["metadata"]["old"], metadata);
            for input in [
                input,
                json!({"snapshot":snapshot,"context":{"selected":"old"}}),
                json!({"source":source,"templates":{"old":"replacement"},"context":{"selected":"old"}}),
            ] {
                assert_eq!(
                    app.runtime
                        .invoke("template.render", input.clone())
                        .unwrap_err()
                        .code,
                    "domain_action_required",
                    "{input}"
                );
            }
        }
        for source in [
            "SAFE",
            "{% if false %}{% include 'old' %}{% endif %}SAFE",
            "{% include ['safe','old'] %}",
            "{% include 'missing' ignore missing %}SAFE",
        ] {
            let result = app
                .runtime
                .invoke("template.render", json!({"source":source}))
                .unwrap();
            assert_eq!(result.data["text"], "SAFE");
        }
    }
    // Frozen intent must not change when the live asset is explicitly converted.
    let snapshot = app
        .runtime
        .invoke(
            "template.inspect",
            json!({"source":"{% include selected %}","include_snapshot":true}),
        )
        .unwrap()
        .data["snapshot"]
        .clone();
    let old = app
        .runtime
        .invoke("template.read", json!({"name":"old"}))
        .unwrap();
    app.runtime
        .invoke(
            "template.save",
            json!({"name":"old","expected":old.data["fingerprint"],"asset":{"source":"SAFE"}}),
        )
        .unwrap();
    assert_eq!(
        app.runtime
            .invoke(
                "template.render",
                json!({"snapshot":snapshot,"context":{"selected":"old"}})
            )
            .unwrap_err()
            .code,
        "domain_action_required"
    );
    assert_eq!(
        app.runtime
            .invoke("template.render", json!({"source":"{% include 'old' %}"}))
            .unwrap()
            .data["text"],
        "SAFE"
    );
}

#[test]
fn domain_dependency_failure_never_delivers_cli_or_task_artifacts() {
    let f = Fixture::new();
    let app = f.app();
    for (name, asset) in [
        ("old", json!({"source":"OLD", "metadata":{"nc":null}})),
        ("bridge", json!({"source":"{% include selected %}"})),
    ] {
        app.runtime
            .invoke("template.save", json!({"name":name,"asset":asset}))
            .unwrap();
    }
    {
        let action = "template.render";
        let input = json!({"source":"prefix{% include 'bridge' %}","context":{"selected":"old"}});
        let input_file = f.0.join("input.json");
        std::fs::write(&input_file, serde_json::to_vec(&input).unwrap()).unwrap();
        let output_file = f.0.join("output.txt");
        let output = f.cli(&[
            "run",
            action,
            "--input",
            input_file.to_str().unwrap(),
            "--out",
            output_file.to_str().unwrap(),
        ]);
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("domain_action_required"));
        assert!(!output_file.exists());
        std::fs::write(&output_file, "previous output").unwrap();
        let output = f.cli(&[
            "run",
            action,
            "--input",
            input_file.to_str().unwrap(),
            "--out",
            output_file.to_str().unwrap(),
        ]);
        assert!(!output.status.success());
        assert_eq!(
            std::fs::read_to_string(&output_file).unwrap(),
            "previous output"
        );
        std::fs::remove_file(output_file).unwrap();
        let started = app
            .dispatch(
                "POST",
                "/api/v2/runs",
                json!({"action":action,"input":input}),
            )
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let run = app
                .dispatch(
                    "GET",
                    &format!("/api/v2/runs/{}", started["id"].as_str().unwrap()),
                    json!({}),
                )
                .unwrap();
            if !["queued", "running"].contains(&run["status"].as_str().unwrap()) {
                assert_eq!(run["status"], "failed");
                assert_eq!(run["error"]["code"], "domain_action_required");
                assert!(run["result"].is_null());
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
}

#[test]
fn legacy_snapshot_replay_preserves_root_checks_and_rejects_unknown_dependency_intent() {
    let f = Fixture::new();
    let app = f.app();
    {
        let action = "template.render";
        for (source, expected) in [
            ("SAFE", Ok("SAFE")),
            (
                "{% if false %}{% include 'old' %}{% endif %}SAFE",
                Ok("SAFE"),
            ),
            ("{% include 'absent' ignore missing %}SAFE", Ok("SAFE")),
            (
                "{% include 'old' ignore missing %}",
                Err("snapshot_compatibility"),
            ),
        ] {
            let snapshot =
                json!({"template":"main","templates":{"main":source,"old":"OLD"},"asset":null});
            let result = app.runtime.invoke(action, json!({"snapshot":snapshot}));
            match expected {
                Ok(text) => assert_eq!(result.unwrap().data["text"], text),
                Err(code) => assert_eq!(result.unwrap_err().code, code),
            }
        }
        assert_eq!(app.runtime.invoke(action, json!({"snapshot":{"template":"main","templates":{"main":"OLD"},"asset":{"metadata":{"nc":null}}}})).unwrap_err().code, "domain_action_required");
        for snapshot in [
            json!({"format_version":2,"template":"main","templates":{"main":"OK"}}),
            json!({"format_version":2,"template":"main","templates":{"main":"OK","old":"OLD"},"metadata":{"main":null}}),
            json!({"format_version":1,"template":"main","templates":{"main":"OK"},"metadata":{"main":null}}),
        ] {
            assert_eq!(
                app.runtime
                    .invoke(action, json!({"snapshot":snapshot}))
                    .unwrap_err()
                    .code,
                "snapshot_invalid"
            );
        }
    }
}

#[test]
fn mixed_domain_bundle_requires_its_provider_before_any_collection_is_written() {
    let source = Fixture::new();
    let app = source.app();
    app.runtime
        .invoke(
            "template.save",
            json!({"name":"hello","asset":{"source":"hello"}}),
        )
        .unwrap();
    // No build provides NC anymore; a bundle from one that still holds retired
    // collections must be rejected as a whole before any file is written.
    let bundle = app.dispatch("GET", "/api/v2/bundle", json!({})).unwrap();
    let target = Fixture::new();
    let imported = target.app();
    for collection in [
        "nc",
        "nc-parameters",
        "sync-turning-assets",
        "undercut-assets",
    ] {
        let mut candidate = bundle.clone();
        candidate["collections"][collection] = json!({"records":[]});
        for path in ["/api/v2/bundle/validate", "/api/v2/bundle/import"] {
            assert_eq!(
                imported
                    .dispatch("POST", path, json!({"bundle":candidate}))
                    .unwrap_err()
                    .code,
                "collection_missing",
                "{collection} via {path}"
            );
            assert!(imported
                .runtime
                .invoke("template.list", json!({}))
                .unwrap()
                .data["names"]
                .as_array()
                .unwrap()
                .is_empty());
        }
    }
}

#[test]
fn unavailable_nc_history_keeps_original_artifacts_and_refuses_replay() {
    let f = Fixture::new();
    let app = f.app();
    let path = f.workspace().join(".nctool/runs/old-nc.json");
    let record = json!({"id":"old-nc","action":"nc.generate","input":{"source":"G0 X1"},"status":"succeeded","started_at":1,"finished_at":2,"result":ActionResult::text("old.nc","text/plain","G0 X1\n".to_string()),"error":null,"plugins":[],"title":"Old NC"});
    let original = serde_json::to_vec(&record).unwrap();
    std::fs::write(&path, &original).unwrap();
    assert_eq!(
        app.dispatch("GET", "/api/v2/runs/old-nc", json!({}))
            .unwrap()["result"]["artifacts"][0]["text"],
        "G0 X1\n"
    );
    assert_eq!(
        app.dispatch("GET", "/api/v2/runs/old-nc/replay-check", json!({}))
            .unwrap()["status"],
        "unknown"
    );
    assert_eq!(
        app.dispatch("POST", "/api/v2/runs/old-nc/rerun", json!({}))
            .unwrap_err()
            .code,
        "action_not_found"
    );
    assert_eq!(std::fs::read(&path).unwrap(), original);
}

#[cfg(feature = "math")]
#[test]
fn math_is_opt_in_and_reports_a_clear_error_when_the_extension_is_missing() {
    let f = Fixture::new();
    App::boot(&f.home(), &f.workspace(), None).unwrap();
    nctool_runtime::config::set_enabled(&f.home(), "math", true).unwrap();
    let app = App::boot(&f.home(), &f.workspace(), None).unwrap();
    assert!(app.runtime.descriptors().iter().any(|d| d.id == "math"));
    // The filter is not global: it only exists when the run selects the extension.
    let missing = app
        .runtime
        .invoke(
            "template.render",
            json!({"source":"{{ 2 | pow(10) }}","context":{}}),
        )
        .unwrap_err();
    assert_eq!(missing.code, "template_error");
    let rendered = app
        .runtime
        .invoke(
            "template.render",
            json!({"source":"{{ 2 | pow(10) }}","context":{},"extensions":["math"]}),
        )
        .unwrap();
    assert_eq!(rendered.data["text"], "1024.0");
}

#[cfg(not(feature = "math"))]
#[test]
fn math_enable_is_rejected_on_a_build_without_the_feature() {
    let f = Fixture::new();
    assert_eq!(
        App::boot(&f.home(), &f.workspace(), None)
            .unwrap()
            .inventory()
            .unwrap()["plugins"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["id"] == "math")
            .unwrap()["available"],
        false
    );
}

#[test]
fn retired_identity_is_not_reclaimable_by_an_aliased_external_plugin() {
    let f = Fixture::new();
    let install = |name: &str, manifest: Value| {
        let source = f.0.join(name);
        std::fs::create_dir(&source).unwrap();
        std::fs::write(
            source.join("plugin.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        nctool_runtime::config::install(&f.home(), &source)
    };
    // The retired plugin id stays reserved regardless of the claimed content.
    assert_eq!(
        install(
            "alias-id",
            json!({"descriptor":PluginDescriptor::builtin("nc"),"actions":[],"command":["never-execute"]})
        )
        .unwrap_err()
        .code,
        "retired_plugin"
    );
    // A non-retired plugin id cannot provide a retired action, service or collection;
    // a same-named alias therefore cannot take over old NC history or asset bundles.
    let action = ActionDescriptor::new("nc.generate", "NC", object_schema());
    assert_eq!(
        install(
            "alias-action",
            json!({"descriptor":PluginDescriptor::builtin("alias-action"),"actions":[action],"command":["never-execute"]})
        )
        .unwrap_err()
        .code,
        "retired_identity"
    );
    let mut descriptor = PluginDescriptor::builtin("alias-service");
    descriptor.provides = vec![Service::new("sync-turning.generate", "alias.echo")];
    let action = ActionDescriptor::new("alias.echo", "echo", object_schema());
    assert_eq!(
        install(
            "alias-service",
            json!({"descriptor":descriptor,"actions":[action],"command":["never-execute"]})
        )
        .unwrap_err()
        .code,
        "retired_identity"
    );
    // Every real retired collection is reserved by its exact archived name, so a
    // same-named alias cannot re-supply it.
    for collection in [
        "nc",
        "nc-parameters",
        "sync-turning-assets",
        "undercut-assets",
    ] {
        let mut descriptor = PluginDescriptor::builtin("alias-collection");
        descriptor.asset_collections = vec![AssetCollection {
            id: collection.into(),
            export_action: "alias.export".into(),
            import_action: "alias.import".into(),
        }];
        let actions = vec![
            ActionDescriptor::new("alias.export", "export", object_schema()),
            ActionDescriptor::new("alias.import", "import", object_schema()),
        ];
        assert_eq!(
            install(
                &format!("alias-collection-{collection}"),
                json!({"descriptor":descriptor,"actions":actions,"command":["never-execute"]})
            )
            .unwrap_err()
            .code,
            "retired_identity",
            "{collection}"
        );
    }
    assert!(!f.home().join("plugins").exists());
    // A generic, unrelated plugin is still installed and runs; only the retired
    // namespaces are reserved.
    let descriptor = PluginDescriptor::builtin("reporter");
    let action = ActionDescriptor::new("report.generate", "report", object_schema());
    assert_eq!(
        install(
            "reporter",
            json!({"descriptor":descriptor,"actions":[action],"command":["never-execute"]})
        )
        .unwrap(),
        "reporter"
    );
    let app = f.app();
    let retired = |id: &str| {
        ["nc", "machine", "process", "sync-turning", "undercut"]
            .iter()
            .any(|token| {
                id.strip_prefix(token)
                    .is_some_and(|rest| rest.starts_with('.'))
            })
    };
    assert!(app
        .runtime
        .capabilities()
        .iter()
        .all(|c| !retired(&c.action.id)));
}

#[test]
fn migration_rejects_unknown_profiles_and_strips_retired_provider_keys() {
    let f = Fixture::new();
    // An unrecognized profile is refused instead of being silently rewritten.
    let original = f.config(json!({"profile":"garbage","enabled":[],"plugins":{}}));
    assert_eq!(
        App::migration_check_at(&f.home()).unwrap_err().code,
        "invalid_profile"
    );
    assert_eq!(
        std::fs::read(f.home().join("config.json")).unwrap(),
        original
    );
    assert_eq!(std::fs::read_dir(f.home()).unwrap().count(), 1);
    assert!(!f.workspace().exists());
    // A retired provider service key is stripped even when its provider is not retired.
    f.config(
        json!({"profile":"nc","enabled":[],"providers":{"nc.generate":"reporter"},"plugins":{}}),
    );
    let check = App::migration_check_at(&f.home()).unwrap();
    assert_eq!(check["valid"], true);
    assert_eq!(check["proposed"]["profile"], "template");
    assert_eq!(check["proposed"]["providers"], json!({}));
    assert!(check["changes"].as_array().unwrap().iter().any(|c| c
        .as_str()
        .unwrap()
        .contains("providers: remove nc.generate")));
}
