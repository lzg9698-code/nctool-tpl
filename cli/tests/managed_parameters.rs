use nctool_cli::composition::App;
use nctool_plugin_sdk::*;
use std::{path::PathBuf, sync::Arc};
struct Fixture {
    root: PathBuf,
    app: Arc<App>,
}
impl Fixture {
    fn new(profile: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "managed-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        let app = App::boot(&root.join("home"), &root.join("workspace"), Some(profile)).unwrap();
        Self { root, app }
    }
    fn invoke(&self, action: &str, input: Value) -> ActionResult {
        self.app.runtime.invoke(action, input).unwrap()
    }
    fn definition(&self, id: &str, extra: Value) -> Value {
        let mut asset = json!({"id":id,"format_version":1,"schema":{"type":"number"},"unit":"mm"});
        asset
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        self.invoke(
            "parameter.save",
            json!({"group":"definitions","name":id,"asset":asset}),
        )
        .data
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
#[test]
fn shared_asset_removal_requires_a_string_fingerprint_and_preserves_conflicts() {
    let f = Fixture::new("template");
    let saved = f.definition("depth", json!({"default":1}));
    let request = json!({"group":"definitions","name":"depth","expected":saved["fingerprint"]});
    for expected in [Value::Null, json!(false), json!(1), json!({}), json!([])] {
        let mut invalid = request.clone();
        invalid["expected"] = expected;
        assert!(f.app.runtime.invoke("parameter.remove", invalid).is_err());
        assert_eq!(
            f.invoke(
                "parameter.read",
                json!({"group":"definitions","name":"depth"})
            )
            .data["fingerprint"],
            saved["fingerprint"]
        );
    }
    let updated = f.invoke(
        "parameter.save",
        json!({"group":"definitions","name":"depth","expected":saved["fingerprint"],"asset":{"id":"depth","format_version":1,"schema":{"type":"number"},"default":2}}),
    );
    assert_eq!(
        f.app
            .runtime
            .invoke("parameter.remove", request.clone())
            .unwrap_err()
            .code,
        "conflict"
    );
    let mut current = request;
    current["expected"] = updated.data["fingerprint"].clone();
    f.invoke("parameter.remove", current);
    assert_eq!(
        f.app
            .runtime
            .invoke(
                "parameter.read",
                json!({"group":"definitions","name":"depth"})
            )
            .unwrap_err()
            .code,
        "not_found"
    );
}

#[cfg(feature = "nc-bundle")]
#[test]
fn managed_render_rejects_malformed_service_inputs_without_panicking() {
    let f = Fixture::new("nc");
    let frozen = f
        .invoke(
            "nc.managed.freeze",
            json!({"source":"{{ X }}","operation_inputs":{"X":1}}),
        )
        .data;
    let resolved = f
        .invoke("nc.managed.resolve", json!({"managed_snapshot":frozen}))
        .data;
    for snapshot in [json!({}), json!({"format_version":1,"input":{}})] {
        assert!(f
            .app
            .runtime
            .invoke("nc.managed.resolve", json!({"managed_snapshot":snapshot}))
            .is_err());
        assert!(f
            .app
            .runtime
            .invoke(
                "nc.managed.render",
                json!({"managed_snapshot":snapshot,"resolved":{},"prepared":{}})
            )
            .is_err());
    }
    for prepared in [
        json!({}),
        json!({"contexts":{}}),
        json!({"contexts":[]}),
        json!({"contexts":[null]}),
        json!({"contexts":[{"X":null}]}),
    ] {
        assert!(f
            .app
            .runtime
            .invoke(
                "nc.managed.render",
                json!({"managed_snapshot":frozen,"resolved":resolved,"prepared":prepared})
            )
            .is_err());
    }
    let prepared = json!({"contexts":[{"X":1}],"warnings":[]});
    for invalid in [
        json!({}),
        json!({"operation":{"id":"direct","version":1},"inputs":{},"cutting":{},"cutting_request":{},"parameters":{"definitions":[]}}),
    ] {
        assert!(f
            .app
            .runtime
            .invoke(
                "nc.managed.render",
                json!({"managed_snapshot":frozen,"resolved":invalid,"prepared":prepared})
            )
            .is_err());
    }
    assert_eq!(
        f.invoke(
            "nc.managed.render",
            json!({"managed_snapshot":frozen,"resolved":resolved,"prepared":prepared})
        )
        .data["text"]
            .as_str()
            .unwrap()
            .trim(),
        "1"
    );
}

#[test]
fn shared_parameters_intersect_schemas_and_preserve_null_and_snapshots() {
    let f = Fixture::new("template");
    f.definition(
        "depth",
        json!({"schema":{"type":["number","null"],"maximum":20},"default":1}),
    );
    f.invoke("parameter.save",json!({"group":"presets","name":"rough","asset":{"format_version":1,"id":"rough","values":{"depth":4}}}));
    let req = json!({"bindings":{"/part/depth":{"id":"depth","unit":"mm"}},"defaults":{"part":{"depth":3}},"preset_id":"rough","values":{"part":{"depth":5}},"schema":{"type":"object","properties":{"part":{"type":"object","properties":{"depth":{"type":["number","null"],"maximum":10,"x-unit":"mm"}}}}}});
    let result = f.invoke("parameter.resolve", req.clone());
    assert_eq!(result.data["values"]["part"]["depth"], 5);
    assert_eq!(result.data["origins"]["/part/depth"], "manual");
    let mut null = req.clone();
    null["values"]["part"]["depth"] = Value::Null;
    assert!(f.invoke("parameter.resolve", null).data["values"]["part"]["depth"].is_null());
    let mut bad = req.clone();
    bad["values"]["part"]["depth"] = json!(11);
    assert!(f.app.runtime.invoke("parameter.resolve", bad).is_err());
    let mut unit = req.clone();
    unit["bindings"]["/part/depth"]["unit"] = json!("inch");
    assert_eq!(
        f.app
            .runtime
            .invoke("parameter.resolve", unit)
            .unwrap_err()
            .code,
        "unit_conflict"
    );
    let mut ty = req.clone();
    ty["schema"]["properties"]["part"]["properties"]["depth"]["type"] = json!("string");
    assert_eq!(
        f.app
            .runtime
            .invoke("parameter.resolve", ty)
            .unwrap_err()
            .code,
        "type_conflict"
    );
    let rendered=f.invoke("template.render.managed",json!({"source":"{{ part.depth }}","context":{"part":{"depth":5}},"bindings":req["bindings"],"schema":req["schema"]}));
    let preset=f.invoke("template.render.managed",json!({"source":"{{ part.depth }}","context":{},"bindings":req["bindings"],"preset_id":"rough","schema":req["schema"]}));
    assert_eq!(
        f.invoke(
            "template.render.managed",
            preset.data["replay_input"].clone()
        )
        .data["parameters"]["origins"],
        preset.data["parameters"]["origins"]
    );
    let fp = f
        .invoke(
            "parameter.read",
            json!({"group":"definitions","name":"depth"}),
        )
        .data["fingerprint"]
        .clone();
    f.invoke("parameter.save",json!({"group":"definitions","name":"depth","expected":fp,"asset":{"id":"depth","format_version":1,"schema":{"type":["number","null"],"maximum":2},"unit":"mm"}}));
    assert_eq!(
        f.invoke(
            "template.render.managed",
            rendered.data["replay_input"].clone()
        )
        .data["text"],
        rendered.data["text"]
    );
    assert_eq!(
        f.invoke(
            "parameter.dependencies.check",
            json!({"snapshot":rendered.data["replay_input"]["parameter_snapshot"]})
        )
        .data["current"],
        false
    );
    assert_eq!(f.app.runtime.invoke("parameter.save",json!({"group":"definitions","name":"depth","expected":fp,"asset":{"id":"depth","format_version":1,"schema":true}})).unwrap_err().code,"conflict");
    f.definition(
        "payload",
        json!({"schema":{"type":["object","null"]},"default":null}),
    );
    let nested=f.invoke("parameter.resolve",json!({"bindings":{"/payload":{"id":"payload"}},"values":{"payload":{"nested":[1,null,{"enabled":true}]}}}));
    assert_eq!(
        nested.data["values"]["payload"]["nested"][2]["enabled"],
        true
    );
    f.invoke("template.save",json!({"name":"unit-guard","asset":{"source":"{{ depth }}","schema":{"type":"object","properties":{"depth":{"type":"number","x-unit":"inch"}}},"metadata":{"parameters":{"bindings":{"/depth":{"id":"depth","unit":"mm"}}}}}}));
    let widened = json!({"template":"unit-guard","context":{"depth":1},"schema":{"type":"object","properties":{"depth":{"type":"number","x-unit":"mm"}}}});
    assert_eq!(
        f.app
            .runtime
            .invoke("template.render.managed", widened)
            .unwrap_err()
            .code,
        "unit_conflict"
    );
}
#[cfg(feature = "nc-bundle")]
fn migrated() -> Fixture {
    let f = Fixture::new("nc");
    let bundle: Value =
        serde_json::from_str(include_str!("../../examples/v3-managed/workspace.json")).unwrap();
    f.app
        .dispatch("POST", "/api/v2/bundle/import", json!({"bundle":bundle}))
        .unwrap();
    f
}
#[cfg(feature = "nc-bundle")]
#[test]
fn managed_generation_rejects_derived_inputs_and_replays_frozen_assets() {
    let f = migrated();
    let oracle: Value =
        serde_json::from_str(include_str!("../../examples/v3-managed/oracle.json")).unwrap();
    let c = &oracle[0];
    let req = json!({"template":c["id"],"operation_inputs":c["inputs"],"trim_blocks":true,"lstrip_blocks":true});
    let out = f.invoke("nc.generate.managed", req.clone());
    assert_eq!(
        out.data["prepared"]["contexts"][0]["target_depths"],
        json!([5., 10., 13.1])
    );
    let mut spoof = req.clone();
    spoof["operation_inputs"]["target_depths"] = json!([1]);
    assert!(f.app.runtime.invoke("nc.generate.managed", spoof).is_err());
    let mut bypass = req.clone();
    bypass["operation"] = json!({"id":"direct","version":1});
    assert!(f.app.runtime.invoke("nc.generate.managed", bypass).is_err());
    let asset = f.invoke("template.read", json!({"name":c["id"]})).data;
    let mut limited = req.clone();
    limited["machine"] = json!({"id":"limited","vendor":"Example","model":"Virtual","config":{"max_spindle_rpm":"2000"}});
    assert!(f
        .app
        .runtime
        .invoke("nc.generate.managed", limited)
        .is_err());
    let mut draft = req.clone();
    draft["source"] = json!(format!(
        "{}\n;本次源码草稿",
        asset["asset"]["source"].as_str().unwrap()
    ));
    let drafted = f.invoke("nc.generate.managed", draft);
    assert_eq!(
        f.invoke("nc.managed.check", drafted.data["replay_input"].clone())
            .data["current"],
        true
    );
    let mut changed = asset["asset"].clone();
    changed["source"] = json!("NEW SOURCE");
    f.invoke(
        "template.save",
        json!({"name":c["id"],"asset":changed,"expected":asset["fingerprint"]}),
    );
    assert_eq!(
        f.invoke("nc.managed.check", out.data["replay_input"].clone())
            .data["current"],
        false
    );
    assert_eq!(
        f.invoke("nc.generate.managed", out.data["replay_input"].clone())
            .artifacts,
        out.artifacts
    );
    let main = "v3-index-g420-1-0.mpf-a";
    assert!(f
        .app
        .runtime
        .invoke(
            "nc.generate.managed",
            json!({"template":main,"operation_inputs":{}})
        )
        .is_err());
}
#[cfg(feature = "nc-bundle")]
#[test]
fn slot_adjustments_recalculate_feed_require_confirmation_and_refuse_limits() {
    let f = migrated();
    for (group, id, asset) in [
        ("tool_types", "mill", json!({"operation":"milling"})),
        (
            "tools",
            "T1",
            json!({"type_id":"mill","attributes":{"diameter":10,"teeth":4,"tool_num":"T1","tool_offset":"D1"}}),
        ),
        ("materials", "steel", json!({"title":"Steel"})),
        (
            "profiles",
            "fixed",
            json!({"tool_id":"T1","material_id":"steel","operation":"milling","mode":"fixed","feed_mode":"per_minute","values":{"rpm":1000,"feed":200}}),
        ),
        (
            "profiles",
            "calc",
            json!({"type_id":"mill","material_id":"steel","operation":"milling","mode":"calculate","feed_mode":"per_minute","values":{"cutting_speed":100,"feed_per_tooth":0.05}}),
        ),
    ] {
        let mut asset = asset;
        asset["id"] = json!(id);
        asset["format_version"] = json!(1);
        f.invoke(
            "nc.parameter.save",
            json!({"group":group,"name":id,"asset":asset}),
        );
    }
    let mut req = json!({"selection":{"slots":{"rough":{"tool_id":"T1","material_id":"steel","operation":"milling","profile_id":"fixed","overrides":{"rpm":2000}}}}});
    let r = f.invoke("nc.parameters.resolve", req.clone());
    assert_eq!(r.data["slots"]["rough"]["values"]["feed"], 400.);
    assert_eq!(r.data["needs_confirmation"], true);
    req["selection"]["slots"]["rough"]["confirmed_for"] =
        r.data["slots"]["rough"]["confirmed_for"].clone();
    assert_eq!(
        f.invoke("nc.parameters.resolve", req.clone()).data["needs_confirmation"],
        false
    );
    req["selection"]["slots"]["rough"]["overrides"]["rpm"] = json!(2100);
    assert_eq!(
        f.invoke("nc.parameters.resolve", req.clone()).data["needs_confirmation"],
        true
    );
    req["max_spindle_rpm"] = json!(2000);
    assert!(f.app.runtime.invoke("nc.parameters.resolve", req).is_err());
    let matches = f.invoke(
        "nc.parameters.match",
        json!({"tool_id":"T1","material_id":"steel","operation":"milling"}),
    );
    assert_eq!(matches.data["items"][0]["name"], "fixed");
    let snapshot = f.invoke("nc.parameter.collection.export", json!({})).data;
    let calculated=f.invoke("nc.parameters.resolve",json!({"snapshot":snapshot,"selection":{"slots":{"rough":{"tool_id":"T1","material_id":"steel","operation":"milling","profile_id":"calc"}}}}));
    assert_eq!(calculated.data["slots"]["rough"]["values"]["rpm"], 3183.);
    assert_eq!(calculated.data["slots"]["rough"]["values"]["feed"], 636.6);
    let cases: Value =
        serde_json::from_str(include_str!("../../examples/v3-managed/oracle.json")).unwrap();
    let case = &cases[0];
    let mut raw = case["inputs"].clone();
    for field in ["tool_num", "tool_offset", "diameter", "teeth"] {
        raw["rough_tool"].as_object_mut().unwrap().remove(field);
    }
    let selected = json!({"template":case["id"],"operation_inputs":raw,"parameter_selection":{"slots":{"rough":{"tool_id":"T1","material_id":"steel","operation":"milling","profile_id":"fixed"}}}});
    let mut incompatible = selected.clone();
    incompatible["parameter_selection"]["slots"]["rough"] = json!({"tool_id":"T1","material_id":"steel","operation":"milling","manual":true,"mode":"fixed","feed_mode":"per_revolution","inputs":{"rpm":1000,"feed":0.2}});
    assert!(f
        .app
        .runtime
        .invoke("nc.generate.managed", incompatible)
        .is_err());
    assert_eq!(
        f.invoke("nc.generate.managed", selected.clone()).data["prepared"]["contexts"][0]
            ["rough_vf"],
        200.
    );
    let mut spoofed = selected.clone();
    spoofed["operation_inputs"]["rough_tool"]["teeth"] = json!(5);
    assert!(f
        .app
        .runtime
        .invoke("nc.generate.managed", spoofed)
        .is_err());
    let tool = f
        .invoke("nc.parameter.read", json!({"group":"tools","name":"T1"}))
        .data;
    let mut no_teeth = tool["asset"].clone();
    no_teeth["attributes"]
        .as_object_mut()
        .unwrap()
        .remove("teeth");
    f.invoke(
        "nc.parameter.save",
        json!({"group":"tools","name":"T1","asset":no_teeth,"expected":tool["fingerprint"]}),
    );
    assert!(f
        .app
        .runtime
        .invoke("nc.generate.managed", selected)
        .is_ok());
    for (operation, rpm, feed) in [("drilling", 3183., 954.9), ("turning", 637., 191.1)] {
        f.invoke("nc.parameter.save",json!({"group":"tool_types","name":operation,"asset":{"format_version":1,"id":operation,"operation":operation}}));
        f.invoke("nc.parameter.save",json!({"group":"tools","name":operation,"asset":{"format_version":1,"id":operation,"type_id":operation,"attributes":{"diameter":10}}}));
        let selection = json!({"slots":{"cut":{"tool_id":operation,"material_id":"steel","operation":operation,"manual":true,"mode":"calculate","feed_mode":"per_minute","inputs":{"cutting_speed":100,"feed_per_revolution":0.3,"work_diameter":50}}}});
        let resolved = f.invoke("nc.parameters.resolve", json!({"selection":selection}));
        assert_eq!(resolved.data["slots"]["cut"]["values"]["rpm"], rpm);
        assert_eq!(resolved.data["slots"]["cut"]["values"]["feed"], feed);
        let mut per_rev = selection;
        per_rev["slots"]["cut"]["feed_mode"] = json!("per_revolution");
        assert_eq!(
            f.invoke("nc.parameters.resolve", json!({"selection":per_rev}))
                .data["slots"]["cut"]["values"]["feed"],
            0.3
        );
    }
}
#[cfg(feature = "nc-bundle")]
#[test]
fn named_outputs_are_continuously_numbered_and_late_failure_delivers_nothing() {
    let f = Fixture::new("nc");
    let req = json!({"options":{"line_numbers":true},"ops":[{"id":"a","source":"G0 X1"},{"id":"b","source":"G1 X2"}],"outputs":[{"name":"main.MPF","segments":[{"op":"a"},{"op":"b"}]},{"name":"one.SPF","segments":[{"op":"a"}]}]});
    let r = f.invoke("process.generate.managed", req.clone());
    assert_eq!(r.artifacts[0].text, "N0010 G0 X1\nN0020 G1 X2\n");
    assert_eq!(r.artifacts[1].text, "N0010 G0 X1\n");
    assert_eq!(
        f.invoke("process.generate.managed", r.data["replay_input"].clone())
            .artifacts,
        r.artifacts
    );
    let mut fail = req;
    fail["ops"][1]["source"] = json!("G1 X{{ missing }}");
    assert!(f
        .app
        .runtime
        .invoke("process.generate.managed", fail)
        .is_err());
    let malformed =
        json!({"process_snapshot":{"format_version":1,"input":{"ops":[null]},"frozen_ops":[null]}});
    assert!(f
        .app
        .runtime
        .invoke("process.generate.managed", malformed)
        .is_err());
    let mixed = json!({"ops":[{"id":"a","source":"G0 X1"},{"id":"b","source":"G1 X2","machine":{"id":"different","vendor":"Example","model":"Virtual","config":{}}}],"outputs":[{"name":"mixed.SPF","segments":[{"op":"a"},{"op":"b"}]}]});
    assert!(f
        .app
        .runtime
        .invoke("process.generate.managed", mixed)
        .is_err());
}

#[cfg(feature = "nc-bundle")]
#[test]
fn cli_delivers_all_files_atomically_and_preserves_existing_outputs() {
    let f = Fixture::new("nc");
    let input = f.root.join("input.json");
    let request = json!({"ops":[{"id":"a","source":"G0 X1"},{"id":"b","source":"G1 X2"}],"outputs":[{"name":"main.MPF","segments":[{"op":"a"}]},{"name":"sub.SPF","segments":[{"op":"b"}]}]});
    std::fs::write(&input, serde_json::to_vec(&request).unwrap()).unwrap();
    let run = |flag: &str, path: &std::path::Path| {
        std::process::Command::new(env!("CARGO_BIN_EXE_nctool"))
            .arg("--home")
            .arg(f.root.join("home"))
            .arg("--workspace")
            .arg(f.root.join("workspace"))
            .args([
                "--profile",
                "nc",
                "run",
                "process.generate.managed",
                "--input",
            ])
            .arg(&input)
            .arg(flag)
            .arg(path)
            .output()
            .unwrap()
    };
    let output = f.root.join("all-files");
    assert!(run("--out-dir", &output).status.success());
    assert_eq!(
        std::fs::read_to_string(output.join("main.MPF")).unwrap(),
        "G0 X1\n"
    );
    assert_eq!(
        std::fs::read_to_string(output.join("sub.SPF")).unwrap(),
        "G1 X2\n"
    );
    assert!(!run("--out-dir", &output).status.success());
    let single = f.root.join("single.nc");
    std::fs::write(&single, "keep me").unwrap();
    assert!(!run("--out", &single).status.success());
    assert_eq!(std::fs::read_to_string(&single).unwrap(), "keep me");
    let mut fail = request;
    fail["ops"][1]["source"] = json!("G1 X{{ missing }}");
    std::fs::write(&input, serde_json::to_vec(&fail).unwrap()).unwrap();
    let rejected = f.root.join("rejected");
    assert!(!run("--out-dir", &rejected).status.success());
    assert!(!rejected.exists());
}
