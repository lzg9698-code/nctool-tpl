use nctool_cli::composition::App;
use nctool_plugin_sdk::*;
use std::{path::PathBuf, process::Command, sync::Arc};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "nctool-retirement-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
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
    let original = f.config(json!({"profile":"nc","enabled":["nc","process","parameters"],"disabled":["undercut"],"providers":{"nc.generate":"nc"},"plugins":{"nc":{"custom":"preserved in backup"},"parameters":{}}}));
    let preview = App::migration_check_at(&f.home()).unwrap();
    assert_eq!(preview["valid"], true);
    assert_eq!(preview["proposed"]["profile"], "template");
    assert_eq!(preview["proposed"]["enabled"], json!(["parameters"]));
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
fn retired_ids_cannot_be_enabled_or_claimed_by_external_plugins() {
    let f = Fixture::new();
    let app = f.app();
    for &id in nctool_runtime::config::RETIRED_PLUGIN_IDS {
        assert_eq!(
            app.dispatch("POST", &format!("/api/v2/plugins/{id}/enable"), json!({}))
                .unwrap_err()
                .code,
            "retired_plugin"
        );
        let output = f.cli(&["plugins", "enable", id]);
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("retired_plugin"));
        let source = f.0.join(format!("plugin-{id}"));
        std::fs::create_dir(&source).unwrap();
        let manifest = json!({"descriptor":PluginDescriptor::builtin(id),"actions":[],"command":["never-execute"]});
        std::fs::write(
            source.join("plugin.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        let code = nctool_runtime::config::install(&f.home(), &source)
            .unwrap_err()
            .code;
        assert!(
            matches!(code.as_str(), "reserved_plugin_id" | "retired_plugin"),
            "unexpected code {code} for {id}"
        );
    }
    assert!(!f.home().join("config.json").exists());
    let inventory = app.inventory().unwrap();
    assert!(inventory["plugins"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|p| p["retired"] == true)
        .all(|p| p["available"] == false && p["active"] == false && p["descriptor"].is_null()));
}

#[test]
fn retired_template_intent_is_blocked_across_live_drafts_snapshots_managed_and_cli() {
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
            for action in ["template.render", "template.render.managed"] {
                assert_eq!(
                    app.runtime.invoke(action, input.clone()).unwrap_err().code,
                    "retired_template"
                );
                assert_eq!(
                    app.dispatch(
                        "POST",
                        &format!("/api/v2/actions/{action}"),
                        json!({"input":input})
                    )
                    .unwrap_err()
                    .code,
                    "retired_template"
                );
            }
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
        assert!(String::from_utf8_lossy(&output.stderr).contains("retired_template"));
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
fn retired_dependencies_are_checked_when_loaded_and_frozen_with_their_intent() {
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
                for action in ["template.render", "template.render.managed"] {
                    assert_eq!(
                        app.runtime.invoke(action, input.clone()).unwrap_err().code,
                        "retired_template",
                        "{action}: {input}"
                    );
                }
            }
        }
        for source in [
            "SAFE",
            "{% if false %}{% include 'old' %}{% endif %}SAFE",
            "{% include ['safe','old'] %}",
            "{% include 'missing' ignore missing %}SAFE",
        ] {
            for action in ["template.render", "template.render.managed"] {
                let result = app
                    .runtime
                    .invoke(action, json!({"source":source}))
                    .unwrap();
                assert_eq!(result.data["text"], "SAFE");
                if action.ends_with("managed") {
                    let replay = &result.data["replay_input"];
                    assert_eq!(replay["snapshot"]["metadata"]["old"], metadata);
                    assert_eq!(
                        app.runtime.invoke(action, replay.clone()).unwrap().data["text"],
                        "SAFE"
                    );
                }
            }
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
    for action in ["template.render", "template.render.managed"] {
        assert_eq!(
            app.runtime
                .invoke(
                    action,
                    json!({"snapshot":snapshot,"context":{"selected":"old"}})
                )
                .unwrap_err()
                .code,
            "retired_template"
        );
        assert_eq!(
            app.runtime
                .invoke(action, json!({"source":"{% include 'old' %}"}))
                .unwrap()
                .data["text"],
            "SAFE"
        );
    }
}

#[test]
fn retired_transitive_dependency_failure_never_delivers_cli_or_task_artifacts() {
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
    for action in ["template.render", "template.render.managed"] {
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
        assert!(String::from_utf8_lossy(&output.stderr).contains("retired_template"));
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
                assert_eq!(run["error"]["code"], "retired_template");
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
    for action in ["template.render", "template.render.managed"] {
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
        assert_eq!(app.runtime.invoke(action, json!({"snapshot":{"template":"main","templates":{"main":"OLD"},"asset":{"metadata":{"nc":null}}}})).unwrap_err().code, "retired_template");
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
fn mixed_retired_bundle_is_rejected_before_any_collection_is_written() {
    let source = Fixture::new();
    let app = source.app();
    app.runtime
        .invoke(
            "template.save",
            json!({"name":"hello","asset":{"source":"hello"}}),
        )
        .unwrap();
    let mut bundle = app.dispatch("GET", "/api/v2/bundle", json!({})).unwrap();
    bundle["collections"]["nc"] = json!({"records":[]});
    let target = Fixture::new();
    let imported = target.app();
    for path in ["/api/v2/bundle/validate", "/api/v2/bundle/import"] {
        assert_eq!(
            imported
                .dispatch("POST", path, json!({"bundle":bundle}))
                .unwrap_err()
                .code,
            "retired_collection"
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

#[test]
fn retired_history_keeps_original_artifacts_and_refuses_replay() {
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
        "retired"
    );
    assert_eq!(
        app.dispatch("POST", "/api/v2/runs/old-nc/rerun", json!({}))
            .unwrap_err()
            .code,
        "retired_action"
    );
    assert_eq!(std::fs::read(path).unwrap(), original);
}
