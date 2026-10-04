use nctool_cli::composition::App;
use nctool_plugin_sdk::*;
use std::{path::PathBuf, process::Command};
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "nctool-v2-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&p).unwrap();
        Self(p)
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
    let restarted = App::boot(&app.home, &temp.0.join("workspace"), None).unwrap();
    assert!(restarted.runtime.capabilities().is_empty());
    assert_eq!(app.inventory().unwrap()["restart_required"], true);
}
#[cfg(feature = "nc-bundle")]
#[test]
fn nc_process_validation_derivation_and_numbering() {
    let temp = Temp::new();
    let app = temp.app("nc");
    let input = json!({"source":"O{{ program | nc_pad(4) }}\nG1 X{{ x | nc_fixed(3) }}\nM30","params":{"program":1,"x":10.5},"options":{"line_numbers":true}});
    let out = app.runtime.invoke("nc.generate", input).unwrap();
    assert_eq!(out.data["text"], "O0001\nN0010 G1 X10.500\nN0020 M30\n");
    let process = json!({"params":{"x":10},"options":{"line_numbers":true},"ops":[{"source":"G0 X{{ x | nc_fixed(3) }}"},{"source":"G1 X{{ x | nc_fixed(3) }}","params":{"x":20}}]});
    let out = app.runtime.invoke("process.generate", process).unwrap();
    assert_eq!(out.data["text"], "N0010 G0 X10.000\nN0020 G1 X20.000\n");
    let mut spec = nctool_plugin_nc::model::ParamSpec::new(
        "x",
        nctool_plugin_nc::model::ParamKind::Number,
        "X",
    )
    .with_range(0., 100.);
    assert_eq!(
        app.runtime
            .invoke(
                "nc.generate",
                json!({"source":"X{{ x }}","params":{"x":200},"specs":[spec],"lenient":true})
            )
            .unwrap_err()
            .code,
        "nc_validation"
    );
    spec.derive = Some(nctool_plugin_nc::model::DeriveRule {
        from: "kind".into(),
        table: [(
            nctool_plugin_nc::model::ParamValue::String("A".into()),
            nctool_plugin_nc::model::ParamValue::Number(12.),
        )]
        .into_iter()
        .collect(),
        fallback: None,
    });
    let out = app
        .runtime
        .invoke(
            "nc.generate",
            json!({"source":"X{{ x | nc_strip }}","params":{"kind":"A"},"specs":[spec]}),
        )
        .unwrap();
    assert_eq!(out.data["text"], "X12\n");
}
#[cfg(feature = "nc-bundle")]
#[test]
fn failed_operation_returns_diagnostics_without_partial_program() {
    let temp = Temp::new();
    let app = temp.app("nc");
    let error = app
        .runtime
        .invoke(
            "process.generate",
            json!({"ops":[{"source":"G0 X1"},{"source":"G1 X{{ missing }}"}]}),
        )
        .unwrap_err();
    assert_eq!(error.code, "operations_failed");
    assert!(error
        .diagnostics
        .iter()
        .any(|d| d.path.as_deref() == Some("ops[1]")));
    let encoded = serde_json::to_value(error).unwrap();
    assert!(encoded.get("text").is_none());
    assert!(encoded.get("artifacts").is_none());
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

#[cfg(feature = "nc-bundle")]
#[test]
fn nc_uses_the_source_and_specs_it_validated_when_assets_change_mid_call() {
    use std::sync::Arc;
    struct Base(nctool_plugin_template::TemplatePlugin);
    impl Plugin for Base {
        fn descriptor(&self) -> PluginDescriptor {
            let mut descriptor = self.0.descriptor();
            for service in &mut descriptor.provides {
                if service.id == "template.render" {
                    service.id = "template.base-render".into();
                }
            }
            descriptor
        }
        fn actions(&self) -> Vec<ActionDescriptor> {
            self.0.actions()
        }
        fn invoke(&self, action: &str, input: Value, ctx: &Context) -> PluginResult<ActionResult> {
            self.0.invoke(action, input, ctx)
        }
    }
    struct Swap(nctool_assets::JsonStore);
    impl Plugin for Swap {
        fn descriptor(&self) -> PluginDescriptor {
            let mut d = PluginDescriptor::builtin("swap");
            d.requires = vec![Requirement::new("template.base-render")];
            d.provides = vec![Service::new("template.render", "swap.render")];
            d
        }
        fn actions(&self) -> Vec<ActionDescriptor> {
            vec![ActionDescriptor::new(
                "swap.render",
                "render",
                object_schema(),
            )]
        }
        fn invoke(&self, _: &str, input: Value, ctx: &Context) -> PluginResult<ActionResult> {
            let (_, expected) = self.0.read("move").unwrap();
            self.0
                .save(
                    "move",
                    &json!({"source":"X{{ x * 1000 }}","metadata":{"nc":{"specs":[]}}}),
                    Some(&expected),
                )
                .unwrap();
            ctx.call("template.base-render", 1, input)
        }
    }
    let temp = Temp::new();
    let workspace = temp.0.join("workspace");
    let base = nctool_plugin_template::TemplatePlugin::new(&workspace).unwrap();
    let store = nctool_assets::JsonStore::new(&workspace.join("templates")).unwrap();
    let specification = nctool_plugin_nc::model::ParamSpec::new(
        "x",
        nctool_plugin_nc::model::ParamKind::Number,
        "X",
    )
    .with_range(0., 20.);
    store
        .save(
            "move",
            &json!({"source":"X{{ x | nc_strip }}","metadata":{"nc":{"specs":[specification]}}}),
            None,
        )
        .unwrap();
    let runtime = nctool_runtime::Runtime::start(
        vec![
            Arc::new(Base(base)),
            Arc::new(Swap(store)),
            Arc::new(nctool_plugin_math::MathPlugin),
            Arc::new(nctool_plugin_nc::NcPlugin::new(&workspace).unwrap()),
        ],
        Default::default(),
        Default::default(),
    )
    .unwrap();
    assert_eq!(
        runtime
            .invoke("nc.generate", json!({"template":"move","params":{"x":10}}))
            .unwrap()
            .data["text"],
        "X10\n"
    );
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
    config["enabled"] = json!(["process"]);
    assert_eq!(
        app.dispatch("POST", "/api/v2/config/validate", json!({"config":config}))
            .unwrap_err()
            .code,
        if cfg!(feature = "nc-bundle") {
            "service_missing"
        } else {
            "plugin_missing"
        }
    );
    assert_eq!(
        app.dispatch(
            "POST",
            "/api/v2/config",
            json!({"config":config,"expected":before["fingerprint"]})
        )
        .unwrap_err()
        .code,
        if cfg!(feature = "nc-bundle") {
            "service_missing"
        } else {
            "plugin_missing"
        }
    );
    assert_eq!(
        app.dispatch("GET", "/api/v2/config", json!({})).unwrap()["config"],
        before["config"]
    );
    #[cfg(feature = "nc-bundle")]
    {
        config["profile"] = json!("nc");
        config["enabled"] = json!([]);
        let result = app
            .dispatch("POST", "/api/v2/config/validate", json!({"config":config}))
            .unwrap();
        assert_eq!(result["valid"], true);
        assert!(app.runtime.descriptors().iter().all(|p| p.id != "nc"));
    }
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
#[cfg(feature = "nc-bundle")]
#[test]
fn nc_replay_uses_original_source_machine_and_numbering() {
    let temp = Temp::new();
    let app = temp.app("nc");
    let saved = app
        .runtime
        .invoke(
            "template.save",
            json!({"name":"move","asset":{"source":"G1 X{{ x | nc_fixed(3) }}"}}),
        )
        .unwrap();
    let first=app.runtime.invoke("process.generate",json!({"name":"part","params":{"x":10},"options":{"line_numbers":true,"line_number_start":100,"add_header_comment":true},"machine":{"id":"m","vendor":"Test","model":"Virtual","config":{"line_number_prefix":"L"}},"ops":[{"template":"move"},{"template":"move","params":{"x":20}}]})).unwrap();
    app.runtime.invoke("template.save",json!({"name":"move","asset":{"source":"G1 X{{ x * 1000 }}"},"expected":saved.data["fingerprint"]})).unwrap();
    let second = app
        .runtime
        .invoke("process.generate", first.data["replay_input"].clone())
        .unwrap();
    assert_eq!(first.data["text"], second.data["text"]);
    assert!(second.data["text"].as_str().unwrap().contains("L0110"));
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

#[cfg(feature = "nc-bundle")]
#[test]
fn machine_collection_preflight_rejects_invalid_prefix_before_writing() {
    let temp = Temp::new();
    let app = temp.app("nc");
    let collection = json!({"version":1,"records":[
        {"group":"machines","name":"valid","asset":{"id":"valid","vendor":"Test","model":"Virtual","config":{}}},
        {"group":"machines","name":"invalid","asset":{"id":"invalid","vendor":"Test","model":"Virtual","config":{"line_number_prefix":"\n"}}}
    ]});
    assert!(app
        .runtime
        .invoke(
            "machine.collection.import",
            json!({"collection":collection})
        )
        .is_err());
    assert_eq!(
        app.runtime.invoke("machine.list", json!({})).unwrap().data["names"],
        json!([])
    );
}
#[cfg(feature = "nc-bundle")]
#[test]
fn nc_replay_preserves_template_name_and_render_settings() {
    let temp = Temp::new();
    let app = temp.app("nc");
    let first=app.runtime.invoke("nc.generate",json!({"template":"custom","templates":{"custom":"{% if true %}\nX10\n{% endif %}\n{{ absent | default('') }}"},"lenient":true,"trim_blocks":true,"lstrip_blocks":true,"options":{"add_header_comment":true}})).unwrap();
    let second = app
        .runtime
        .invoke("nc.generate", first.data["replay_input"].clone())
        .unwrap();
    assert_eq!(first.data["text"], second.data["text"]);
    assert!(second.data["text"]
        .as_str()
        .unwrap()
        .contains("template: custom"));
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

#[cfg(feature = "nc-bundle")]
#[test]
fn nc_asset_whitespace_is_domain_scoped_overridable_and_replayable() {
    let temp = Temp::new();
    let app = temp.app("nc");
    app.runtime.invoke("template.save",json!({"name":"whitespace","asset":{"source":"{% if true %}\nX10\n{% endif %}\nY20","metadata":{"nc":{"render_options":{"trim_blocks":true,"lstrip_blocks":true}}}}})).unwrap();
    let nc = app
        .runtime
        .invoke("nc.generate", json!({"template":"whitespace"}))
        .unwrap();
    assert_eq!(nc.data["text"], "X10\nY20\n");
    assert_eq!(nc.data["replay_input"]["trim_blocks"], true);
    let replay = app
        .runtime
        .invoke("nc.generate", nc.data["replay_input"].clone())
        .unwrap();
    assert_eq!(nc.data["text"], replay.data["text"]);
    let generic = app
        .runtime
        .invoke("template.render", json!({"template":"whitespace"}))
        .unwrap();
    assert_eq!(generic.data["text"], "\nX10\n\nY20");
    let overridden = app
        .runtime
        .invoke(
            "nc.generate",
            json!({"template":"whitespace","trim_blocks":false}),
        )
        .unwrap();
    assert_eq!(overridden.data["text"], "\nX10\n\nY20\n");
}

#[cfg(feature = "nc-bundle")]
#[test]
fn document_action_bindings_survive_runtime_registration() {
    let temp = Temp::new();
    let app = temp.app("nc");
    let actions = app.runtime.capabilities();
    let nc = &actions
        .iter()
        .find(|entry| entry.action.id == "nc.generate")
        .unwrap()
        .action;
    let ui = nc.ui_schema.as_ref().unwrap();
    assert_eq!(
        ui["document_input"]["bindings"]["params"],
        "effective_context"
    );
    assert_eq!(ui["document_input"]["match_metadata"], "nc");
    assert_eq!(ui["properties"]["params"]["title"], "加工参数");
    let text = &actions
        .iter()
        .find(|entry| entry.action.id == "template.render")
        .unwrap()
        .action;
    assert_eq!(
        text.ui_schema.as_ref().unwrap()["document_input"]["default"],
        true
    );
    // Metadata alone never registers domain filters into the generic renderer.
    assert!(app
        .runtime
        .invoke(
            "template.render",
            json!({"source":"{{ x | nc_fixed(3) }}","context":{"x":10}})
        )
        .is_err());
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
fn review_r03_disabled_domain_intent_is_described_without_loading_domain_code() {
    let temp = Temp::new();
    let app = temp.app("template");
    let caps = app
        .dispatch("GET", "/api/v2/capabilities", json!({}))
        .unwrap();
    assert!(!caps["actions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|a| a["action"]["id"] == "nc.generate"));
    assert!(caps["document_actions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|a| a["id"] == "nc.generate"
            && a["ui_schema"]["document_input"]["match_metadata"] == "nc"));
}
#[cfg(feature = "nc-bundle")]
#[test]
fn review_r01_r02_r05_r06_failures_never_deliver_artifacts_in_any_execution_surface() {
    let temp = Temp::new();
    let app = temp.app("nc");
    let mut cases = vec![
        json!({"source":"G0 X1","machine":{"id":"bad","vendor":"Test","model":"Virtual","config":{"line_number_prefix":"N\nG0 Z-100\nN"}},"options":{"line_numbers":true}}),
        json!({"source":"{% include child %}","templates":{"sub":"X{{ x | nc_fixed(3) }}"},"params":{"child":"sub","x":-10},"specs":[{"name":"x","kind":"number","min":0}]}),
    ];
    for source in [
        "G1 X{{ 1e308 * 1e308 }}Y0",
        "G1 X-infZ1",
        "G1 XnanY0",
        "G1 X{{ machine.zero_x }}",
        "G1 X{{ machine[key] }}",
    ] {
        cases.push(json!({"source":source,"lenient":true,"params":{"key":"zero_x"}}));
    }
    for input in cases {
        assert!(
            app.runtime.invoke("nc.generate", input.clone()).is_err(),
            "{input}"
        );
        assert!(app
            .dispatch(
                "POST",
                "/api/v2/actions/nc.generate",
                json!({"input":input})
            )
            .is_err());
        let started = app
            .dispatch(
                "POST",
                "/api/v2/runs",
                json!({"action":"nc.generate","input":input}),
            )
            .unwrap();
        let run = await_review_run(&app, started["id"].as_str().unwrap());
        assert_eq!(run["status"], "failed");
        assert!(run["result"].is_null());
        assert!(app
            .runtime
            .invoke(
                "process.generate",
                json!({"ops":[{"source":"G0 X1"},input]})
            )
            .is_err());
        let source = temp.0.join("review-input.json");
        let out = temp.0.join("review-output.nc");
        std::fs::write(&source, input.to_string()).unwrap();
        std::fs::write(&out, "existing").unwrap();
        let cli = Command::new(env!("CARGO_BIN_EXE_nctool"))
            .arg("--home")
            .arg(temp.0.join("home"))
            .arg("--workspace")
            .arg(temp.0.join("workspace"))
            .args(["--profile", "nc", "run", "nc.generate", "--input"])
            .arg(&source)
            .arg("--out")
            .arg(&out)
            .output()
            .unwrap();
        assert!(!cli.status.success());
        assert_eq!(std::fs::read_to_string(out).unwrap(), "existing");
    }
    // Validate the final postprocessed output too, including configured numbering prefixes.
    assert!(app.runtime.invoke("nc.generate",json!({"source":"G0 X1","machine":{"id":"m","vendor":"Test","model":"Virtual","config":{"line_number_prefix":"XinfY"}},"options":{"line_numbers":true}})).is_err());
    for source in [
        "G1 X{{ machine.zero_x | default(1) }}",
        "{% if machine.zero_x is defined %}X{{ machine.zero_x }}{% endif %}G0 X1",
        "(XinfY0)\nG0 X1",
        "; Xnan\nG0 X1",
    ] {
        assert!(
            app.runtime
                .invoke("nc.generate", json!({"source":source,"lenient":true}))
                .is_ok(),
            "{source}"
        );
    }
    for (source, child) in [
        (
            "{% import child as m %}{{ m.move(x) }}",
            "{% macro move(x) %}X{{ x | nc_fixed(3) }}{% endmacro %}",
        ),
        (
            "{% extends child %}",
            "{% block body %}X{{ x | nc_fixed(3) }}{% endblock %}",
        ),
        ("{% include child %}", "{% include 'inner' %}"),
    ] {
        for x in [-1, 10] {
            let outcome = app.runtime.invoke("nc.generate", json!({"source":source,"templates":{"sub":child,"inner":"X{{ x | nc_fixed(3) }}"},"params":{"child":"sub","x":x},"specs":[{"name":"x","kind":"number","min":0}]}));
            assert_eq!(outcome.is_ok(), x >= 0, "{source}: {outcome:?}");
        }
    }
    // A valid dynamic dependency still works and all provided specs are checked.
    let result=app.runtime.invoke("nc.generate",json!({"source":"{% include child %}","templates":{"sub":"X{{ x | nc_fixed(3) }}"},"params":{"child":"sub","x":10},"specs":[{"name":"x","kind":"number","min":0}]})).unwrap();
    assert_eq!(result.data["text"], "X10.000\n");
    assert!(app
        .runtime
        .invoke("nc.generate", result.data["replay_input"].clone())
        .is_ok());
}
#[cfg(feature = "nc-bundle")]
#[test]
fn review_r02_disk_and_history_machine_configs_share_content_validation() {
    let temp = Temp::new();
    let app = temp.app("nc");
    let invalid =
        json!({"id":"disk","vendor":"Test","model":"Virtual","config":{"program_prefix":"\nG0"}});
    // Simulate an externally edited file; generation must validate after reading it.
    std::fs::write(
        temp.0.join("workspace/machines/disk.json"),
        invalid.to_string(),
    )
    .unwrap();
    assert_eq!(
        app.runtime
            .invoke("nc.generate", json!({"source":"G0 X1","machine_id":"disk"}))
            .unwrap_err()
            .code,
        "invalid_machine"
    );
    assert_eq!(
        app.runtime
            .invoke("nc.generate", json!({"source":"G0 X1","machine":invalid}))
            .unwrap_err()
            .code,
        "invalid_machine"
    );
    let result=app.runtime.invoke("nc.generate",json!({"source":"G0 X1","machine":{"id":"ok","vendor":"Test","model":"Virtual","config":{"line_number_prefix":"L"}},"options":{"line_numbers":true}})).unwrap();
    assert_eq!(result.data["text"], "L0010 G0 X1\n");
    let mut replay = result.data["replay_input"].clone();
    replay["machine"]["config"]["line_number_prefix"] = json!("N\nG0");
    assert_eq!(
        app.runtime.invoke("nc.generate", replay).unwrap_err().code,
        "invalid_machine"
    );
}
#[cfg(feature = "nc-bundle")]
#[test]
fn review_r04_schema_specs_defaults_and_replay_are_enforced_on_effective_parameters() {
    let temp = Temp::new();
    let app = temp.app("nc");
    let schema = json!({"type":"object","properties":{"x":{"type":"number","minimum":0,"maximum":100},"tool":{"enum":["A","B"]},"passes":{"type":"array","items":{"type":"number","minimum":0}}},"required":["x","tool"],"additionalProperties":false});
    let asset = json!({"source":"G1 X{{ x | nc_fixed(3) }}","schema":schema,"defaults":{"x":10,"tool":"A"},"metadata":{"nc":{"specs":[{"name":"x","kind":"number","max":20}]}}});
    app.runtime
        .invoke("template.save", json!({"name":"constrained","asset":asset}))
        .unwrap();
    for params in [
        json!({"x":-10}),
        json!({"x":30}),
        json!({"tool":"C"}),
        json!({"passes":[-1]}),
        json!({"extra":1}),
    ] {
        for with_source in [false, true] {
            let mut input = json!({"template":"constrained","params":params});
            if with_source {
                input["source"] = asset["source"].clone();
            }
            assert!(
                app.runtime.invoke("nc.generate", input.clone()).is_err(),
                "{input}"
            );
            let started = app
                .dispatch(
                    "POST",
                    "/api/v2/runs",
                    json!({"action":"nc.generate","input":input}),
                )
                .unwrap();
            assert!(await_review_run(&app, started["id"].as_str().unwrap())["result"].is_null());
            assert!(app
                .runtime
                .invoke("process.generate", json!({"ops":[input]}))
                .is_err());
        }
    }
    let result = app
        .runtime
        .invoke(
            "nc.generate",
            json!({"template":"constrained","params":{"x":12,"passes":[1,2]}}),
        )
        .unwrap();
    assert_eq!(result.data["text"], "G1 X12.000\n");
    let mut replay = result.data["replay_input"].clone();
    assert_eq!(
        app.runtime
            .invoke("nc.generate", replay.clone())
            .unwrap()
            .data["text"],
        result.data["text"]
    );
    replay["params"]["x"] = json!(-1);
    assert!(app.runtime.invoke("nc.generate", replay).is_err());
    // Unsaved Schema edits remain active, including explicit specs and strict additional properties.
    let input = json!({"source":asset["source"],"params":{"x":5},"schema":{"type":"object","properties":{"x":{"minimum":10}},"required":["x"],"additionalProperties":false},"specs":[{"name":"x","kind":"number","min":0}]});
    assert!(app.runtime.invoke("nc.generate", input).is_err());
    let defaulted=app.runtime.invoke("nc.generate",json!({"source":"X{{ x }}","schema":{"type":"object","properties":{"x":{"minimum":0}},"required":["x"]},"defaults":{"x":1}})).unwrap();
    assert_eq!(
        app.runtime
            .invoke("nc.generate", defaulted.data["replay_input"].clone())
            .unwrap()
            .data["text"],
        "X1\n"
    );
}
