use nctool_cli::composition::App;
use nctool_plugin_sdk::*;
use std::{
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};
struct Temp(PathBuf);
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
impl Temp {
    fn new() -> Self {
        loop {
            let p = std::env::temp_dir().join(format!(
                "nctool-v2-{}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT_TEMP.fetch_add(1, Ordering::Relaxed),
            ));
            match std::fs::create_dir(&p) {
                Ok(()) => return Self(p),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("cannot create test directory {}: {error}", p.display()),
            }
        }
    }
    fn app(&self, profile: &str) -> std::sync::Arc<App> {
        App::boot(
            &self.0.join("home"),
            &self.0.join("workspace"),
            Some(profile),
        )
        .unwrap()
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[test]
fn default_platform_has_no_nc_and_renders_nested_data() {
    let temp = Temp::new();
    let app = temp.app("template");
    let ids: Vec<_> = app
        .runtime
        .capabilities()
        .into_iter()
        .map(|c| c.action.id)
        .collect();
    assert!(!ids.iter().any(|id| id.starts_with("nc.")
        || id.starts_with("machine.")
        || id.starts_with("process.")));
    let result=app.runtime.invoke("template.render",json!({"source":"{{ user.name }}{% for v in values %}:{{ v }}{% endfor %}{% if extra is none %}:none{% endif %}","context":{"user":{"name":"Ada"},"values":[1,2],"extra":null}})).unwrap();
    assert_eq!(result.data["text"], "Ada:1:2:none");
    assert!(app
        .runtime
        .invoke("template.render", json!({"source":"{{ machine.id }}"}))
        .is_err());
    assert!(app
        .runtime
        .invoke("template.render", json!({"source":"{{ 1 | nc_pad(4) }}"}))
        .is_err());
}

#[test]
fn history_replay_records_environment_changes_and_preserves_original_record() {
    let temp = Temp::new();
    let app = temp.app("template");
    let id = app
        .workbench
        .start(
            "template.render",
            json!({"source":"fixed {{ x }}","context":{"x":7}}),
            "Version history",
        )
        .unwrap();
    let record = await_review_run(&app, &id);
    assert_eq!(record["result"]["data"]["text"], "fixed 7");
    assert_eq!(
        app.dispatch("GET", &format!("/api/v2/runs/{id}/replay-check"), json!({}))
            .unwrap()["status"],
        "matching"
    );
    let path = temp
        .0
        .join("workspace/.nctool/runs")
        .join(format!("{id}.json"));
    let mut changed = record;
    changed["environment"]["plugins"]["template"]["version"] = json!("0.1.0");
    changed["environment"]["plugins"]["old-plugin"] =
        json!({"version":"1.0.0","protocol_version":1});
    changed["environment"]["host_version"] = json!("0.0.1");
    changed["environment"]["services"] = json!({});
    let bytes = serde_json::to_vec(&changed).unwrap();
    std::fs::write(&path, &bytes).unwrap();
    let check = app
        .dispatch("GET", &format!("/api/v2/runs/{id}/replay-check"), json!({}))
        .unwrap();
    assert_eq!(check["status"], "changed");
    assert_eq!(check["differences"].as_array().unwrap().len(), 4);
    let created = app
        .dispatch("POST", &format!("/api/v2/runs/{id}/rerun"), json!({}))
        .unwrap();
    let replay = await_review_run(&app, created["id"].as_str().unwrap());
    assert_eq!(replay["result"]["data"]["text"], "fixed 7");
    assert_eq!(replay["replay"]["source_run_id"], id);
    assert_eq!(replay["replay"]["check"], check);
    assert_eq!(
        replay["environment"]["plugins"]["template"]["version"],
        "2.0.0"
    );
    assert_eq!(std::fs::read(path).unwrap(), bytes);
}

#[test]
fn legacy_and_future_history_environments_are_unknown_but_replayable() {
    let temp = Temp::new();
    let app = temp.app("template");
    for format in [None, Some(999)] {
        let id = app
            .workbench
            .start(
                "template.render",
                json!({"source":"legacy"}),
                "Legacy history",
            )
            .unwrap();
        let mut record = await_review_run(&app, &id);
        if let Some(version) = format {
            record["environment"]["format_version"] = json!(version);
        } else {
            record.as_object_mut().unwrap().remove("environment");
            record.as_object_mut().unwrap().remove("replay");
        }
        let path = temp
            .0
            .join("workspace/.nctool/runs")
            .join(format!("{id}.json"));
        let bytes = serde_json::to_vec(&record).unwrap();
        std::fs::write(&path, &bytes).unwrap();
        assert_eq!(app.workbench.replay_check(&id).unwrap().status, "unknown");
        let replay_id = app.workbench.rerun(&id).unwrap();
        let replay = await_review_run(&app, &replay_id);
        assert_eq!(replay["replay"]["check"]["status"], "unknown");
        assert_eq!(replay["result"]["data"]["text"], "legacy");
        assert_eq!(
            app.workbench.replay_check(&replay_id).unwrap().status,
            "matching"
        );
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }
}
#[test]
fn include_extends_import_strictness_and_schema() {
    let temp = Temp::new();
    let app = temp.app("template");
    let result=app.runtime.invoke("template.render",json!({"template":"main","templates":{"base":"{% block content %}{% endblock %}","macros":"{% macro hello(name) %}Hello {{ name }}{% endmacro %}","child":"{{ suffix }}","main":"{% extends 'base' %}{% import 'macros' as m %}{% block content %}{{ m.hello(name) }}{% include 'child' %}{% endblock %}"},"context":{"name":"Ada","suffix":"!"}})).unwrap();
    assert_eq!(result.data["text"], "Hello Ada!");
    assert!(app
        .runtime
        .invoke("template.render", json!({"source":"{{ missing }}"}))
        .is_err());
    assert_eq!(
        app.runtime
            .invoke(
                "template.render",
                json!({"source":"{{ missing }}","lenient":true})
            )
            .unwrap()
            .data["text"],
        ""
    );
    assert!(app.runtime.invoke("template.render",json!({"source":"{{ age }}","context":{"age":"bad"},"schema":{"type":"object","properties":{"age":{"type":"integer"}}}})).is_err());
}
#[test]
fn asset_writes_preserve_fingerprints_and_metadata() {
    let temp = Temp::new();
    let app = temp.app("template");
    let asset = json!({"source":"Hi {{ name }}","tags":["letters"],"defaults":{"name":"Ada"},"metadata":{"custom":{"language":"en"}}});
    let created = app
        .runtime
        .invoke("template.save", json!({"name":"hello","asset":asset}))
        .unwrap();
    assert!(app
        .runtime
        .invoke("template.save", json!({"name":"hello","asset":asset}))
        .is_err());
    let token = created.data["fingerprint"].clone();
    let mut changed = asset.clone();
    changed["source"] = json!("Welcome {{ name }}");
    app.runtime
        .invoke(
            "template.save",
            json!({"name":"hello","asset":changed,"expected":token}),
        )
        .unwrap();
    assert_eq!(
        app.runtime
            .invoke(
                "template.save",
                json!({"name":"hello","asset":asset,"expected":token})
            )
            .unwrap_err()
            .code,
        "conflict"
    );
    assert_eq!(
        app.runtime
            .invoke("template.render", json!({"template":"hello"}))
            .unwrap()
            .data["text"],
        "Welcome Ada"
    );
    assert_eq!(
        app.runtime
            .invoke("template.read", json!({"name":"hello"}))
            .unwrap()
            .data["asset"]["metadata"],
        asset["metadata"]
    );
    assert_eq!(
        app.runtime
            .invoke("template.save", json!({"name":"../escape","asset":asset}))
            .unwrap_err()
            .code,
        "path_escape"
    );
}
#[test]
fn cli_and_http_use_identical_actions_and_failures_do_not_write_output() {
    let temp = Temp::new();
    let input = json!({"source":"Hi {{ name }}","context":{"name":"Ada"}});
    let input_path = temp.0.join("input.json");
    std::fs::write(&input_path, input.to_string()).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_nctool"))
        .args([
            "--home",
            temp.0.join("home").to_str().unwrap(),
            "--workspace",
            temp.0.join("workspace").to_str().unwrap(),
            "run",
            "template.render",
            "--input",
            input_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let cli: Value = serde_json::from_slice(&output.stdout).unwrap();
    let api = temp
        .app("template")
        .dispatch(
            "POST",
            "/api/v2/actions/template.render",
            json!({"input":input}),
        )
        .unwrap();
    assert_eq!(cli, api);
    let out = temp.0.join("out.txt");
    std::fs::write(&out, "existing").unwrap();
    std::fs::write(&input_path, json!({"source":"{{ missing }}"}).to_string()).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_nctool"))
        .args([
            "--home",
            temp.0.join("home").to_str().unwrap(),
            "--workspace",
            temp.0.join("workspace").to_str().unwrap(),
            "run",
            "template.render",
            "--input",
            input_path.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(std::fs::read_to_string(out).unwrap(), "existing");
}
#[test]
fn configuration_changes_apply_only_after_restart() {
    let temp = Temp::new();
    let app = temp.app("template");
    app.dispatch("POST", "/api/v2/plugins/template/disable", json!({}))
        .unwrap();
    assert!(!app.runtime.capabilities().is_empty());
    let inventory = app.inventory().unwrap();
    for id in ["template", "parameters"] {
        let plugin = inventory["plugins"]
            .as_array()
            .unwrap()
            .iter()
            .find(|plugin| plugin["id"] == id)
            .unwrap();
        assert_eq!(plugin["active"], true);
        assert_eq!(plugin["enabled_next_start"], false);
    }
    let saved = app.dispatch("GET", "/api/v2/config", json!({})).unwrap();
    let preview = app
        .dispatch(
            "POST",
            "/api/v2/config/validate",
            json!({"config":saved["config"]}),
        )
        .unwrap();
    assert!(preview["plugins"].as_array().unwrap().is_empty());
    let restarted = App::boot(&app.home, &temp.0.join("workspace"), None).unwrap();
    assert!(restarted.runtime.capabilities().is_empty());
    assert_eq!(inventory["restart_required"], true);
}

#[test]
fn configuration_explicit_companions_keep_dependency_checks() {
    let cases = vec![("template", "parameters", "template")];
    for (profile, companion, provider) in cases {
        let temp = Temp::new();
        let app = temp.app("template");
        let before = app.dispatch("GET", "/api/v2/config", json!({})).unwrap();
        let mut config = before["config"].clone();
        config["profile"] = json!(profile);
        config["enabled"] = json!([companion]);
        config["disabled"] = json!([provider]);
        for path in ["/api/v2/config/validate", "/api/v2/config"] {
            assert_eq!(
                app.dispatch("POST", path, json!({"config":config}))
                    .unwrap_err()
                    .code,
                "service_missing",
                "{companion}: {path}"
            );
        }
        assert_eq!(
            app.dispatch("GET", "/api/v2/config", json!({})).unwrap(),
            before
        );
        // Also check an invalid configuration saved outside the API: inventory
        // must show the explicit selection, and boot must reject the dependency.
        nctool_runtime::config::save(&app.home, &serde_json::from_value(config).unwrap(), None)
            .unwrap();
        let inventory = app.inventory().unwrap();
        let plugins = inventory["plugins"].as_array().unwrap();
        assert_eq!(
            plugins.iter().find(|p| p["id"] == companion).unwrap()["enabled_next_start"],
            true
        );
        assert_eq!(
            plugins.iter().find(|p| p["id"] == provider).unwrap()["enabled_next_start"],
            false
        );
        let error = App::boot(&app.home, &temp.0.join("workspace"), None)
            .err()
            .unwrap();
        assert_eq!(error.code, "service_missing");
    }
}

#[test]
fn configuration_nc_profile_is_rejected_in_every_build_without_silent_conversion() {
    let temp = Temp::new();
    let app = temp.app("template");
    let before = app.dispatch("GET", "/api/v2/config", json!({})).unwrap();
    let mut config = before["config"].clone();
    config["profile"] = json!("nc");
    // Only the template profile exists; the retired profile is refused on preview and
    // on write, in every build, instead of being silently converted.
    for path in ["/api/v2/config/validate", "/api/v2/config"] {
        assert_eq!(
            app.dispatch(
                "POST",
                path,
                json!({"config":config,"expected":before["fingerprint"]})
            )
            .unwrap_err()
            .code,
            "invalid_profile",
            "{path}"
        );
    }
    // The rejected configuration was not written: the stored profile stays template.
    assert_eq!(
        app.dispatch("GET", "/api/v2/config", json!({})).unwrap()["config"],
        before["config"]
    );
    // A profile persisted outside the API fails boot closed and is not rewritten.
    std::fs::create_dir_all(&app.home).unwrap();
    std::fs::write(
        app.home.join("config.json"),
        serde_json::to_vec(&config).unwrap(),
    )
    .unwrap();
    assert_eq!(
        App::boot(&app.home, &temp.0.join("workspace"), None)
            .err()
            .unwrap()
            .code,
        "invalid_profile"
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&std::fs::read(app.home.join("config.json")).unwrap())
            .unwrap()["profile"],
        "nc"
    );
}

#[test]
fn ingress_rejects_nonzero_underflow_but_preserves_subnormals_and_strings() {
    assert_eq!(
        parse_json(br#"{"params":{"x":1e-400}}"#).unwrap_err().code,
        "numeric_underflow"
    );
    let valid = parse_json(br#"{"small":5e-324,"text":"1e-400"}"#).unwrap();
    assert!(valid["small"].as_f64().unwrap() > 0.);
    assert_eq!(valid["text"], "1e-400");
}

#[test]
fn template_snapshot_survives_edits_to_source_includes_and_defaults() {
    let temp = Temp::new();
    let app = temp.app("template");
    app.runtime
        .invoke(
            "template.save",
            json!({"name":"header","asset":{"source":"Hello {{ name }}"}}),
        )
        .unwrap();
    let saved=app.runtime.invoke("template.save",json!({"name":"main","asset":{"source":"{% include 'header' %}!","defaults":{"name":"Ada"}}})).unwrap();
    let inspected = app
        .runtime
        .invoke(
            "template.inspect",
            json!({"template":"main","include_snapshot":true}),
        )
        .unwrap();
    let header = app
        .runtime
        .invoke("template.read", json!({"name":"header"}))
        .unwrap();
    app.runtime.invoke("template.save",json!({"name":"header","asset":{"source":"Changed {{ other }}"},"expected":header.data["fingerprint"]})).unwrap();
    app.runtime.invoke("template.save",json!({"name":"main","asset":{"source":"Changed {{ name }}","defaults":{"name":"Grace"}},"expected":saved.data["fingerprint"]})).unwrap();
    let output = app
        .runtime
        .invoke(
            "template.render",
            json!({"snapshot":inspected.data["snapshot"]}),
        )
        .unwrap();
    assert_eq!(output.data["text"], "Hello Ada!");
}

#[test]
fn catalog_copy_delete_and_references_use_guarded_assets() {
    let temp = Temp::new();
    let app = temp.app("template");
    let saved=app.runtime.invoke("template.save",json!({"name":"header","asset":{"source":"HEADER","metadata":{"title":"程序头"},"tags":["shared"]}})).unwrap();
    assert_eq!(
        app.runtime
            .invoke("template.catalog", json!({}))
            .unwrap()
            .data["items"][0]["title"],
        "程序头"
    );
    app.runtime
        .invoke(
            "template.save",
            json!({"name":"main","asset":{"source":"{% include 'header' %}"}}),
        )
        .unwrap();
    assert_eq!(
        app.runtime
            .invoke(
                "template.remove",
                json!({"name":"header","expected":saved.data["fingerprint"]})
            )
            .unwrap_err()
            .code,
        "asset_referenced"
    );
    let copied = app
        .runtime
        .invoke(
            "template.copy",
            json!({"name":"header","new_name":"copy","title":"副本"}),
        )
        .unwrap();
    assert_eq!(copied.data["asset"]["metadata"]["title"], "副本");
    assert_eq!(
        app.runtime
            .invoke("template.copy", json!({"name":"header","new_name":"copy"}))
            .unwrap_err()
            .code,
        "conflict"
    );
    app.runtime
        .invoke(
            "template.remove",
            json!({"name":"copy","expected":copied.data["fingerprint"]}),
        )
        .unwrap();
    assert!(app
        .runtime
        .invoke("template.read", json!({"name":"copy"}))
        .is_err());
}
#[test]
fn asynchronous_run_keeps_snapshot_and_survives_a_new_host_instance() {
    let temp = Temp::new();
    let app = temp.app("template");
    let response=app.dispatch("POST","/api/v2/runs",json!({"action":"template.render","title":"问候信","input":{"source":"Hello {{ name }}","context":{"name":"Ada"}}})).unwrap();
    let id = response["id"].as_str().unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    let run = loop {
        let run = app.workbench.get(id).unwrap();
        if !["queued", "running"].contains(&run.status.as_str()) {
            break run;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    assert_eq!(run.status, "succeeded");
    assert_eq!(run.result.unwrap().data["text"], "Hello Ada");
    assert_eq!(
        run.input["snapshot"]["templates"]["__inline__"],
        "Hello {{ name }}"
    );
    let reopened = temp.app("template");
    assert_eq!(reopened.workbench.get(id).unwrap().status, "succeeded");
    assert_eq!(
        reopened.workbench.list().unwrap()["runs"][0]["title"],
        "问候信"
    );
    assert!(app
        .dispatch(
            "POST",
            "/api/v2/runs",
            json!({"action":"template.render","input":[]})
        )
        .is_err());
}
#[test]
fn cancellation_before_dispatch_and_other_host_start_do_not_corrupt_run_state() {
    use std::sync::Arc;
    struct Slow;
    impl Plugin for Slow {
        fn descriptor(&self) -> PluginDescriptor {
            PluginDescriptor::builtin("slow")
        }
        fn actions(&self) -> Vec<ActionDescriptor> {
            vec![ActionDescriptor::new("slow.run", "slow", object_schema())]
        }
        fn invoke(&self, _: &str, _: Value, ctx: &Context) -> PluginResult<ActionResult> {
            for _ in 0..100 {
                ctx.cancellation.check()?;
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            Ok(ActionResult::data(json!({})))
        }
    }
    let temp = Temp::new();
    let runtime = nctool_runtime::Runtime::start(
        vec![Arc::new(Slow)],
        Default::default(),
        Default::default(),
    )
    .unwrap();
    let workbench = nctool_cli::workbench::Workbench::new(runtime.clone(), &temp.0).unwrap();
    let id = workbench.start("slow.run", json!({}), "slow").unwrap();
    let other = nctool_cli::workbench::Workbench::new(runtime, &temp.0).unwrap();
    assert!(["queued", "running"].contains(&other.get(&id).unwrap().status.as_str()));
    assert!(workbench.cancel(&id));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        let run = workbench.get(&id).unwrap();
        if run.status == "cancelled" {
            assert!(run.result.is_none());
            break;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[test]
fn config_preview_validates_dependencies_without_starting_or_saving_plugins() {
    let temp = Temp::new();
    let app = temp.app("template");
    let before = app.dispatch("GET", "/api/v2/config", json!({})).unwrap();
    let mut config = before["config"].clone();
    config["enabled"] = json!(["absent-plugin"]);
    // Preview and write both refuse a selection whose plugin cannot be resolved,
    // without starting or persisting a partial configuration.
    for path in ["/api/v2/config/validate", "/api/v2/config"] {
        assert_eq!(
            app.dispatch(
                "POST",
                path,
                json!({"config":config,"expected":before["fingerprint"]})
            )
            .unwrap_err()
            .code,
            "plugin_missing",
            "{path}"
        );
    }
    assert_eq!(
        app.dispatch("GET", "/api/v2/config", json!({})).unwrap()["config"],
        before["config"]
    );
}
#[test]
fn workspace_bundle_preserves_includes_presets_and_conflicts() {
    let source = Temp::new();
    let app = source.app("template");
    app.runtime.invoke("template.save",json!({"name":"header","asset":{"source":"Hello {{ name }}","tags":["shared"],"metadata":{"title":"Header"}}})).unwrap();
    app.runtime.invoke("template.save",json!({"name":"main","asset":{"source":"{% include 'header' %}","schema":{"type":"object"},"defaults":{"name":"Ada"},"metadata":{"nested":{"key":"value"}}}})).unwrap();
    app.runtime
        .invoke(
            "preset.save",
            json!({"name":"ada","asset":{"template":"main","context":{"name":"Ada"}}}),
        )
        .unwrap();
    let bundle = app.dispatch("GET", "/api/v2/bundle", json!({})).unwrap();
    assert_eq!(
        bundle["collections"]["template"]["records"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    let target = Temp::new();
    let imported = target.app("template");
    assert_eq!(
        imported
            .dispatch("POST", "/api/v2/bundle/validate", json!({"bundle":bundle}))
            .unwrap()["valid"],
        true
    );
    assert!(imported
        .runtime
        .invoke("template.read", json!({"name":"main"}))
        .is_err());
    imported
        .dispatch("POST", "/api/v2/bundle/import", json!({"bundle":bundle}))
        .unwrap();
    assert_eq!(
        imported
            .runtime
            .invoke("template.render", json!({"template":"main"}))
            .unwrap()
            .data["text"],
        "Hello Ada"
    );
    assert_eq!(
        imported
            .runtime
            .invoke("preset.read", json!({"name":"ada"}))
            .unwrap()
            .data["asset"]["template"],
        "main"
    );
    assert_eq!(
        imported
            .dispatch("POST", "/api/v2/bundle/import", json!({"bundle":bundle}))
            .unwrap_err()
            .code,
        "conflict"
    );
    let mut damaged = bundle;
    damaged["collections"]["template"]["records"][0]["asset"]["source"] = json!("changed");
    assert_eq!(
        app.dispatch("POST", "/api/v2/bundle/validate", json!({"bundle":damaged}))
            .unwrap_err()
            .code,
        "bundle_fingerprint"
    );
}
#[test]
fn legacy_plugin_descriptors_keep_the_exact_protocol_shape() {
    let descriptor = PluginDescriptor::builtin("legacy");
    let json = serde_json::to_value(descriptor).unwrap();
    assert!(json.get("asset_collections").is_none());
    let action = ActionDescriptor::new("legacy.run", "run", object_schema());
    assert!(serde_json::to_value(action)
        .unwrap()
        .get("ui_schema")
        .is_none());
}

#[test]
fn external_config_preflight_does_not_execute_the_entrypoint() {
    let temp = Temp::new();
    let app = temp.app("template");
    let source = temp.0.join("preview-plugin");
    std::fs::create_dir(&source).unwrap();
    let mut descriptor = PluginDescriptor::builtin("preview-only");
    descriptor.requires.push(Requirement {
        id: "template.render".into(),
        version: 1,
    });
    let manifest = json!({"descriptor":descriptor,"command":["nctool-preflight-must-not-execute"],"actions":[ActionDescriptor::new("preview.run","Preview",object_schema())]});
    std::fs::write(
        source.join("plugin.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    app.dispatch(
        "POST",
        "/api/v2/plugins/install",
        json!({"source":source.to_string_lossy()}),
    )
    .unwrap();
    let saved = app.dispatch("GET", "/api/v2/config", json!({})).unwrap();
    let mut config = saved["config"].clone();
    config["enabled"] = json!(["preview-only"]);
    let checked = app
        .dispatch("POST", "/api/v2/config/validate", json!({"config":config}))
        .unwrap();
    assert_eq!(checked["valid"], true);
    assert!(!app
        .runtime
        .descriptors()
        .iter()
        .any(|p| p.id == "preview-only"));
    assert_eq!(
        app.dispatch("GET", "/api/v2/config", json!({})).unwrap()["config"],
        saved["config"]
    );
    config["disabled"] = json!(["template"]);
    assert_eq!(
        app.dispatch("POST", "/api/v2/config/validate", json!({"config":config}))
            .unwrap_err()
            .code,
        "service_missing"
    );
}

#[test]
fn workspace_bundle_rolls_back_completed_collections_when_a_later_provider_fails() {
    struct FailingCollection;
    impl Plugin for FailingCollection {
        fn descriptor(&self) -> PluginDescriptor {
            let mut d = PluginDescriptor::builtin("failing");
            d.asset_collections.push(AssetCollection {
                id: "z-failing".into(),
                export_action: "failing.export".into(),
                import_action: "failing.import".into(),
            });
            d
        }
        fn actions(&self) -> Vec<ActionDescriptor> {
            vec![
                ActionDescriptor::new("failing.export", "Export", object_schema()),
                ActionDescriptor::new("failing.import", "Import", object_schema()),
            ]
        }
        fn invoke(&self, action: &str, input: Value, _: &Context) -> PluginResult<ActionResult> {
            if action == "failing.export" {
                return Ok(ActionResult::data(json!({})));
            }
            if input["validate_only"] == true {
                return Ok(ActionResult::data(json!({"count":0,"mapping":[]})));
            }
            Err(PluginError::new(
                "injected_import_failure",
                "Provider failed after preflight",
            ))
        }
    }
    let source = Temp::new();
    let source_app = source.app("template");
    source_app
        .runtime
        .invoke(
            "template.save",
            json!({"name":"new","asset":{"source":"Hello"}}),
        )
        .unwrap();
    let mut bundle = source_app
        .dispatch("GET", "/api/v2/bundle", json!({}))
        .unwrap();
    bundle["collections"]["z-failing"] = json!({});
    let fingerprint =
        nctool_assets::FileFingerprint::of_bytes(b"{}", std::time::UNIX_EPOCH).as_string();
    bundle["manifest"].as_array_mut().unwrap().push(json!({"collection":"z-failing","provider":"failing","version":"2.0.0","fingerprint":fingerprint}));
    let target = Temp::new();
    let mut app = target.app("template");
    let runtime = nctool_runtime::Runtime::start(
        vec![
            std::sync::Arc::new(
                nctool_plugin_template::TemplatePlugin::new(&target.0.join("workspace")).unwrap(),
            ),
            std::sync::Arc::new(
                nctool_plugin_parameters::ParametersPlugin::new(&target.0.join("workspace"))
                    .unwrap(),
            ),
            std::sync::Arc::new(FailingCollection),
        ],
        Default::default(),
        Default::default(),
    )
    .unwrap();
    std::sync::Arc::get_mut(&mut app).unwrap().runtime = runtime;
    let error = app
        .dispatch("POST", "/api/v2/bundle/import", json!({"bundle":bundle}))
        .unwrap_err();
    assert_eq!(error.code, "injected_import_failure");
    assert!(error.diagnostics.is_empty(), "rollback should succeed");
    assert_eq!(
        app.runtime.invoke("template.list", json!({})).unwrap().data["names"],
        json!([])
    );
}

fn await_review_run(app: &App, id: &str) -> Value {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let run = app
            .dispatch("GET", &format!("/api/v2/runs/{id}"), json!({}))
            .unwrap();
        if !["queued", "running"].contains(&run["status"].as_str().unwrap()) {
            return run;
        }
        assert!(std::time::Instant::now() < deadline, "run did not finish");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}
#[test]
fn review_r08_optional_and_required_references_keep_cli_task_and_replay_semantics() {
    let temp = Temp::new();
    let app = temp.app("template");
    for (input, expected) in [
        (
            json!({"source":"A{% include 'absent' ignore missing %}B"}),
            "AB",
        ),
        (
            json!({"source":"A{% include ['absent','exists'] %}B","templates":{"exists":"OK"}}),
            "AOKB",
        ),
        (
            json!({"source":"{% if false %}{% include 'absent' %}{% endif %}OK"}),
            "OK",
        ),
    ] {
        assert_eq!(
            app.runtime
                .invoke("template.render", input.clone())
                .unwrap()
                .data["text"],
            expected
        );
        let started = app
            .dispatch(
                "POST",
                "/api/v2/runs",
                json!({"action":"template.render","input":input}),
            )
            .unwrap();
        let run = await_review_run(&app, started["id"].as_str().unwrap());
        assert_eq!(run["status"], "succeeded");
        assert_eq!(run["result"]["data"]["text"], expected);
        let replay = app
            .dispatch(
                "POST",
                &format!("/api/v2/runs/{}/rerun", started["id"].as_str().unwrap()),
                json!({}),
            )
            .unwrap();
        assert_eq!(
            await_review_run(&app, replay["id"].as_str().unwrap())["result"]["data"]["text"],
            expected
        );
    }
    let started = app
        .dispatch(
            "POST",
            "/api/v2/runs",
            json!({"action":"template.render","input":{"source":"{% include 'absent' %}"}}),
        )
        .unwrap();
    let run = await_review_run(&app, started["id"].as_str().unwrap());
    assert_eq!(run["status"], "failed");
    assert!(run["result"].is_null());
}
#[test]
fn review_r09_json_integer_boundaries_are_exact_or_rejected_across_ingress() {
    for literal in [
        "-9223372036854775808",
        "9223372036854775807",
        "9223372036854775808",
        "18446744073709551615",
    ] {
        let parsed = parse_json(format!("{{\"nested\":[{literal}]}}").as_bytes()).unwrap();
        assert_eq!(parsed["nested"][0].to_string(), literal);
    }
    for literal in [
        "-9223372036854775809",
        "18446744073709551616",
        "18446744073709551617",
    ] {
        let error = parse_json(format!("{{\"nested\":[{literal}]}}").as_bytes()).unwrap_err();
        assert_eq!(error.code, "numeric_range");
    }
    assert_eq!(
        parse_json(br#"{"label":"18446744073709551617","escaped":"\"18446744073709551617"}"#)
            .unwrap()["label"],
        "18446744073709551617"
    );
    let temp = Temp::new();
    let app = temp.app("template");
    let result = app
        .runtime
        .invoke(
            "template.render",
            json!({"source":"{{ x }}","context":{"x":u64::MAX}}),
        )
        .unwrap();
    assert_eq!(result.data["text"], u64::MAX.to_string());
    let saved = app
        .runtime
        .invoke(
            "template.save",
            json!({"name":"boundary","asset":{"source":"{{ x }}","defaults":{"x":u64::MAX}}}),
        )
        .unwrap();
    assert!(saved.data["fingerprint"].is_string());
    let bundle = app.dispatch("GET", "/api/v2/bundle", json!({})).unwrap();
    let recipient = Temp::new();
    let other = recipient.app("template");
    other
        .dispatch("POST", "/api/v2/bundle/import", json!({"bundle":bundle}))
        .unwrap();
    assert_eq!(
        other
            .runtime
            .invoke("template.render", json!({"template":"boundary"}))
            .unwrap()
            .data["text"],
        u64::MAX.to_string()
    );
}
#[test]
fn review_r03_disabled_domain_intent_is_not_advertised_as_executable() {
    let temp = Temp::new();
    let app = temp.app("template");
    let caps = app
        .dispatch("GET", "/api/v2/capabilities", json!({}))
        .unwrap();
    let retired = |id: &str| {
        ["nc", "machine", "process", "sync-turning", "undercut"]
            .iter()
            .any(|token| {
                id.strip_prefix(token)
                    .is_some_and(|rest| rest.starts_with('.'))
            })
    };
    assert!(!caps["actions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|a| a["action"]["id"].as_str().is_some_and(retired)));
    // No pseudo executable action and no retired document declaration is published;
    // removed machining intent is recognized from template metadata, not capabilities.
    assert!(!caps["document_actions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|a| a["id"].as_str().is_some_and(retired)));
}
