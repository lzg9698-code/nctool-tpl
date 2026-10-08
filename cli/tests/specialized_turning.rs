#![cfg(feature = "nc-bundle")]
use nctool_cli::composition::App;
use nctool_plugin_sdk::*;
use std::{path::PathBuf, sync::Arc};
struct Fixture {
    root: PathBuf,
    app: Arc<App>,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "turning-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let app = App::boot(&root.join("home"), &root.join("workspace"), Some("nc")).unwrap();
        Self { root, app }
    }
    fn call(&self, id: &str, input: Value) -> ActionResult {
        self.app.runtime.invoke(id, input).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
fn sync() -> Value {
    json!({"target":"right","sync_feed":0.2,"sides":{"right":{"ap_1":2.,"ap2_min":0.,"ap2_max":8.,"start_sync_num":10,"rows":[{"id":"one","sta_x":60.,"sta_z":0.,"end_x":50.,"end_z":-10.,"ap_2":3.}]}}})
}
fn same_text(a: &str, b: &str) {
    let a = a.replace("\r\n", "\n");
    let b = b.replace("\r\n", "\n");
    assert_eq!(
        a.strip_suffix('\n').unwrap_or(&a),
        b.strip_suffix('\n').unwrap_or(&b)
    );
}
#[test]
fn cli_reenables_each_specialized_builtin_after_disable() {
    let f = Fixture::new();
    for id in ["nc-managed", "sync-turning", "undercut"] {
        for verb in ["disable", "enable"] {
            let output = std::process::Command::new(env!("CARGO_BIN_EXE_nctool"))
                .arg("--home")
                .arg(f.root.join("home"))
                .args(["plugins", verb, id])
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{verb} {id}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
    let (config, _) = nctool_runtime::config::load(&f.root.join("home")).unwrap();
    let restarted = App::boot(&f.root.join("home"), &f.root.join("workspace"), Some("nc")).unwrap();
    for id in ["nc-managed", "sync-turning", "undercut"] {
        assert!(config.enabled.iter().any(|item| item == id));
        assert!(!config.disabled.iter().any(|item| item == id));
        assert!(restarted
            .runtime
            .descriptors()
            .iter()
            .any(|plugin| plugin.id == id));
    }
}

#[test]
fn malformed_business_snapshots_are_rejected_and_next_generation_recovers() {
    let f = Fixture::new();
    let frozen = f.call("sync-turning.freeze", sync()).data;
    for assets in [
        json!([]),
        json!(0),
        json!(null),
        json!({"version":1,"records":{}}),
    ] {
        let mut invalid = frozen.clone();
        invalid["assets"] = assets;
        for action in ["sync-turning.prepare", "sync-turning.generate"] {
            assert!(f
                .app
                .runtime
                .invoke(action, json!({"job_snapshot":invalid}))
                .is_err());
        }
    }
    let mut invalid = frozen.clone();
    invalid["nc"] = json!({});
    assert!(f
        .app
        .runtime
        .invoke("sync-turning.generate", json!({"job_snapshot":invalid}))
        .is_err());
    let mut invalid = frozen.clone();
    invalid["assets"]["records"][0]["asset"]["parameters"] = json!(0);
    invalid["input"]["sides"]["right"]["trajectory"] = json!({"overrides":{"clearance_x":10}});
    assert!(f
        .app
        .runtime
        .invoke("sync-turning.prepare", json!({"job_snapshot":invalid}))
        .is_err());
    assert_eq!(
        f.call("sync-turning.generate", json!({"job_snapshot":frozen}))
            .artifacts
            .len(),
        2
    );
}

#[test]
fn every_v3_sync_and_standard_case_matches_calculation_and_program() {
    let f = Fixture::new();
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("../../examples/v3-managed/oracle.json")).unwrap();
    let mut tested = 0;
    for case in cases {
        let kind = case["operation"].as_str().unwrap();
        if !kind.starts_with("sync.") && kind != "groove" {
            continue;
        }
        let raw = &case["inputs"];
        let op = json!({"id":kind,"version":1});
        let p = f
            .call("nc.operation.prepare", json!({"operation":op,"inputs":raw}))
            .data;
        for (index, expected) in case["expected"]["contexts"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
        {
            for (key, value) in expected.as_object().unwrap() {
                let actual = &p["contexts"][index][key];
                if let (Some(a), Some(b)) = (actual.as_f64(), value.as_f64()) {
                    assert!((a - b).abs() <= 1e-9, "{kind}/{key}: {a} != {b}");
                } else {
                    assert_eq!(actual, value, "{kind}/{key}");
                }
            }
        }
        let (action, input) = if kind.starts_with("sync.") {
            let side = if kind == "sync.left" { "left" } else { "right" };
            let row = json!({"id":"oracle","sta_x":raw["sta_x"],"sta_z":raw["sta_z"],"end_x":raw["end_x"],"end_z":raw["end_z"],"ap_2":raw["ap_2"]});
            let mut sides = json!({});
            sides[side] =
                json!({"ap_1":raw["ap_1"],"start_sync_num":raw["current_sync_num"],"rows":[row]});
            (
                "sync-turning.generate",
                json!({"target":side,"sync_feed":raw["sync_feed"],"sides":sides}),
            )
        } else {
            let side = if raw["side"] == "Left" {
                "left"
            } else {
                "right"
            };
            let prefix = raw["gtype"].as_str().unwrap();
            let mut row = json!({"id":"oracle","gtype":raw["gtype"],"std_key":raw["std_key"],"d1":raw[format!("{prefix}_d1")],"z":raw[format!("{prefix}_z")]});
            if prefix == "ES" {
                row["d2"] = raw["ES_d2"].clone();
                row["c"] = raw["ES_c"].clone();
                row["angle"] = raw["ES_c_ang"].clone();
            }
            let mut sides = json!({});
            sides[side] = json!({"rows":[row]});
            (
                "undercut.generate",
                json!({"target":side,"raw_diameter":raw["raw_diameter"],"safety_dis":raw["safety_dis"],"sides":sides}),
            )
        };
        let result = f.call(action, input);
        assert_eq!(
            result.artifacts.len(),
            case["expected"]["texts"].as_array().unwrap().len()
        );
        for (i, a) in result.artifacts.iter().enumerate() {
            same_text(&a.text, case["expected"]["texts"][i].as_str().unwrap());
        }
        if kind.starts_with("sync.") {
            assert_eq!(p["next_sync_num"], case["expected"]["next_sync_num"]);
        }
        tested += 1;
    }
    assert!(tested >= 70, "missing standard/side coverage: {tested}");
}
#[test]
fn recommendation_preserves_raw_and_adopted_precision_and_flags_zero() {
    let f = Fixture::new();
    let r = f
        .call(
            "sync-turning.recommend",
            json!({"sta_x":60.,"end_x":50.,"ap_1":2.,"ap2_min":0.,"ap2_max":2.}),
        )
        .data;
    assert_eq!(r["cut_num"], 3);
    assert!((r["ap_2"].as_f64().unwrap() - 4. / 3.).abs() < 1e-12);
    assert_eq!(r["adopted_ap_2"], 1.33);
    assert_eq!(r["generatable"], false);
    let r = f
        .call(
            "sync-turning.recommend",
            json!({"sta_x":60.,"end_x":50.,"ap_1":5.,"ap2_min":0.,"ap2_max":1.}),
        )
        .data;
    assert_eq!(r["ap_2"], 0.);
    assert_eq!(r["generatable"], false);
}
#[test]
fn batch_sync_cursor_pairing_and_failure_are_atomic() {
    let f = Fixture::new();
    let mut raw = sync();
    let mut row = raw["sides"]["right"]["rows"][0].clone();
    row["id"] = json!("two");
    raw["sides"]["right"]["rows"]
        .as_array_mut()
        .unwrap()
        .push(row);
    let p = f.call("sync-turning.prepare", raw.clone()).data;
    assert_eq!(p["sides"]["right"]["next_sync_num"], 16);
    assert_eq!(
        p["sides"]["right"]["rows"][1]["prepared"]["contexts"][0]["sync_num_start"],
        13
    );
    raw["sides"]["right"]["rows"][1]["end_x"] = json!(70.);
    let error = f
        .app
        .runtime
        .invoke("sync-turning.generate", raw)
        .unwrap_err();
    assert!(error.message.contains("two"));
    assert!(error
        .diagnostics
        .iter()
        .any(|d| d.path.as_deref().unwrap_or("").contains("rows/1")));
}
#[test]
fn adjusted_angle_changes_geometry_and_path_with_confirmation() {
    let f = Fixture::new();
    let mut raw = sync();
    let side = raw["sides"]["right"].take();
    raw["sides"] = json!({"left":side});
    raw["target"] = json!("left");
    raw["sides"]["left"]["rows"][0]["sta_x"] = json!(59.);
    let old = f.call("sync-turning.prepare", raw.clone()).data;
    raw["sides"]["left"]["trajectory"] =
        json!({"preset_id":"v3-compatible","overrides":{"left_entry_angle":30.}});
    assert_eq!(
        f.app
            .runtime
            .invoke("sync-turning.generate", raw.clone())
            .unwrap_err()
            .code,
        "confirmation_required"
    );
    let p = f.call("sync-turning.prepare", raw.clone()).data;
    assert_ne!(
        old["sides"]["left"]["rows"][0]["prepared"]["contexts"][0]["actual_sta_z"],
        p["sides"]["left"]["rows"][0]["prepared"]["contexts"][0]["actual_sta_z"]
    );
    assert!(p["sides"]["left"]["channels"][0]
        .as_str()
        .unwrap()
        .contains("ANG=210"));
    raw["sides"]["left"]["trajectory"]["confirmed_for"] =
        p["sides"]["left"]["confirmed_for"].clone();
    assert_eq!(f.call("sync-turning.generate", raw).artifacts.len(), 2);
}
#[test]
fn presets_conflict_invalidate_results_and_replay_old_snapshot() {
    let f = Fixture::new();
    let mut asset = nctool_plugin_sync_turning::builtin();
    asset["id"] = json!("custom");
    let saved = f.call(
        "sync-turning.asset.save",
        json!({"group":"trajectories","name":"custom","asset":asset}),
    );
    let mut raw = sync();
    raw["sides"]["right"]["trajectory"] = json!({"preset_id":"custom"});
    let result = f.call("sync-turning.generate", raw);
    let snapshot = result.data["replay_input"].clone();
    assert_eq!(
        f.call("sync-turning.check", snapshot.clone()).data["current"],
        true
    );
    asset["parameters"]["clearance_x"] = json!(15.);
    f.call("sync-turning.asset.save",json!({"group":"trajectories","name":"custom","asset":asset,"expected":saved.data["fingerprint"]}));
    assert_eq!(f.app.runtime.invoke("sync-turning.asset.save",json!({"group":"trajectories","name":"custom","asset":asset,"expected":saved.data["fingerprint"]})).unwrap_err().code,"conflict");
    assert_eq!(
        f.call("sync-turning.check", snapshot.clone()).data["current"],
        false
    );
    let replay = f.call("sync-turning.generate", snapshot);
    assert_eq!(result.artifacts, replay.artifacts);
}
#[test]
fn forged_fields_unknown_strategies_and_fs_branch_fields_are_rejected() {
    let f = Fixture::new();
    let mut raw = sync();
    raw["sides"]["right"]["rows"][0]["cut_num"] = json!(2);
    assert!(f.app.runtime.invoke("sync-turning.generate", raw).is_err());
    let mut raw = sync();
    raw["sides"]["right"]["trajectory"] = json!({"overrides":{"waitm":1}});
    assert!(f.app.runtime.invoke("sync-turning.prepare", raw).is_err());
    let mut raw = sync();
    raw["sides"]["right"]["rows"][0]["sta_x"] = json!(9007199254740993u64);
    assert!(f.app.runtime.invoke("sync-turning.generate", raw).is_err());
    let r = json!({"raw_diameter":100.,"safety_dis":2.,"sides":{"right":{"rows":[{"id":"f","gtype":"FS","std_key":"unknown","d1":80.,"z":30.,"d2":90.}]}}});
    assert!(f.app.runtime.invoke("undercut.generate", r).is_err());
}
#[test]
fn business_composition_returns_current_channels_and_rejects_later_failure() {
    let f = Fixture::new();
    let raw = sync();
    let input = json!({"ops":[{"id":"rough","business":{"plugin":"sync-turning","input":raw}}],"outputs":[{"name":"CH1.SPF","segments":[{"op":"rough","channel":1}]},{"name":"CH2.SPF","segments":[{"op":"rough","channel":2}]}]});
    let result = f.call("process.generate.managed", input.clone());
    assert_eq!(result.artifacts.len(), 2);
    assert!(result.artifacts[0].text.contains("WAITM(12,1,2)"));
    let replay = f.call(
        "process.generate.managed",
        result.data["replay_input"].clone(),
    );
    assert_eq!(result.artifacts, replay.artifacts);
    let mut bad = input;
    let mut r = raw;
    r["sides"]["right"]["rows"][0]["ap_2"] = json!(0);
    bad["ops"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id":"broken","business":{"plugin":"sync-turning","input":r}}));
    assert!(f
        .app
        .runtime
        .invoke("process.generate.managed", bad)
        .unwrap_err()
        .message
        .contains("broken"));
}
#[test]
fn editable_template_is_rendered_and_frozen_history_is_preserved() {
    let f = Fixture::new();
    let asset = json!({"source":"{{ trajectory_program }}","metadata":{"nc":{"trajectory_plan_version":1,"operation":{"id":"sync.right","version":1}}}});
    let saved = f.call("template.save", json!({"name":"trajectory","asset":asset}));
    let mut raw = sync();
    raw["sides"]["right"]["template"] = json!("trajectory");
    let generated = f.call("sync-turning.generate", raw.clone());
    let mut changed = asset;
    changed["source"] = json!("G0 X0\n{{ trajectory_program }}");
    f.call(
        "template.save",
        json!({"name":"trajectory","asset":changed,"expected":saved.data["fingerprint"]}),
    );
    assert_eq!(
        f.call("sync-turning.check", generated.data["replay_input"].clone())
            .data["current"],
        false
    );
    let custom = f.call("sync-turning.generate", raw);
    assert_eq!(custom.artifacts.len(), 2);
    for output in &custom.artifacts {
        assert!(output.text.starts_with("G0 X0\n"));
        assert!(output.text.contains("WAITM"));
    }
    assert_eq!(
        f.call(
            "sync-turning.generate",
            generated.data["replay_input"].clone()
        )
        .artifacts,
        generated.artifacts
    );
}
#[test]
fn full_custom_sync_template_and_render_failure_keep_batch_atomic() {
    let f = Fixture::new();
    let asset = json!({"source":"; CUTS {{ cut_num }}\nG0 X{{ actual_sta_x }}", "metadata":{"nc":{"trajectory_plan_version":1,"operation":{"id":"sync.right","version":1}}}});
    let saved = f.call("template.save", json!({"name":"custom-code","asset":asset}));
    let mut raw = sync();
    raw["sides"]["right"]["template"] = json!("custom-code");
    let generated = f.call("sync-turning.generate", raw.clone());
    assert_eq!(generated.artifacts.len(), 2);
    assert!(generated
        .artifacts
        .iter()
        .all(|a| a.text.contains("; CUTS 2") && a.text.contains("G0 X60")));
    let mut broken = asset.clone();
    broken["source"] =
        json!("{% if sync_num_start > 10 %}{{ 1 // 0 }}{% endif %}{{ trajectory_program }}");
    let saved = f.call(
        "template.save",
        json!({"name":"custom-code","asset":broken,"expected":saved.data["fingerprint"]}),
    );
    let mut row = raw["sides"]["right"]["rows"][0].clone();
    row["id"] = json!("second");
    raw["sides"]["right"]["rows"]
        .as_array_mut()
        .unwrap()
        .push(row);
    let error = f
        .app
        .runtime
        .invoke("sync-turning.generate", raw.clone())
        .unwrap_err();
    assert!(error.message.contains("第 2 行"), "{}", error.message);
    let mut legacy = asset;
    legacy["metadata"]["nc"]
        .as_object_mut()
        .unwrap()
        .remove("trajectory_plan_version");
    f.call(
        "template.save",
        json!({"name":"custom-code","asset":legacy,"expected":saved.data["fingerprint"]}),
    );
    assert_eq!(
        f.app
            .runtime
            .invoke("sync-turning.generate", raw)
            .unwrap_err()
            .code,
        "template_incompatible"
    );
}
#[test]
fn full_custom_undercut_template_consumes_derived_geometry() {
    let f = Fixture::new();
    let asset = json!({"source":"; USER GROOVE\nG1 X{{ D1_CUT }} F0.25", "metadata":{"nc":{"trajectory_plan_version":1,"operation":{"id":"groove","version":1}}}});
    f.call(
        "template.save",
        json!({"name":"custom-groove","asset":asset}),
    );
    let raw = json!({"target":"right","raw_diameter":100.,"safety_dis":2.,"sides":{"right":{"template":"custom-groove","rows":[{"id":"one","gtype":"ES","std_key":"DIN509-E 0,2 x 0,1","d1":80.,"z":30.,"d2":85.,"c":0.5,"angle":45.}]}}});
    let generated = f.call("undercut.generate", raw);
    assert_eq!(generated.artifacts.len(), 1);
    assert!(generated.artifacts[0].text.contains("; USER GROOVE"));
    assert!(generated.artifacts[0].text.contains(" F0.25"));
    assert!(!generated.artifacts[0].text.contains("trajectory_program"));
}
#[test]
fn changed_geometry_requires_reconfirmation_of_path_overrides() {
    let f = Fixture::new();
    let mut raw = sync();
    raw["sides"]["right"]["trajectory"] = json!({"overrides":{"clearance_x":12.}});
    let preview = f.call("sync-turning.prepare", raw.clone());
    raw["sides"]["right"]["trajectory"]["confirmed_for"] =
        preview.data["sides"]["right"]["confirmed_for"].clone();
    f.call("sync-turning.generate", raw.clone());
    raw["sides"]["right"]["rows"][0]["end_z"] = json!(-20.);
    assert_eq!(
        f.app
            .runtime
            .invoke("sync-turning.generate", raw)
            .unwrap_err()
            .code,
        "confirmation_required"
    );
}

#[test]
fn paired_path_contract_and_old_strategy_fail_closed() {
    use nctool_plugin_nc::trajectory::{validate_pair, Block, Plan};
    let mut one = Plan::new("right", 1);
    let mut two = Plan::new("right", 2);
    one.sync(10);
    assert!(validate_pair(&[one.clone(), two.clone()]).is_err());
    two.sync(10);
    validate_pair(&[one, two]).unwrap();
    let mut bad = Plan::new("right", 1);
    bad.motion("entry", true, Some(60.), Some(0.), None);
    *bad.last_motion().0 = Some(40);
    assert!(bad.render().is_err());
    bad.blocks = vec![Block::Mode { relative: true }];
    assert!(bad.render().is_err());
    let f = Fixture::new();
    let generated = f.call("sync-turning.generate", sync());
    let mut replay = generated.data["replay_input"].clone();
    replay["job_snapshot"]["strategy_version"] = json!(2);
    assert_eq!(
        f.app
            .runtime
            .invoke("sync-turning.generate", replay)
            .unwrap_err()
            .code,
        "snapshot_invalid"
    );
}

#[test]
fn composed_undercuts_advance_line_numbers_and_reject_text_input_bindings() {
    let f = Fixture::new();
    let case: Vec<Value> =
        serde_json::from_str(include_str!("../../examples/v3-managed/oracle.json")).unwrap();
    let p = &case
        .iter()
        .find(|c| c["operation"] == "groove" && c["inputs"]["gtype"] == "ES")
        .unwrap()["inputs"];
    let side = p["side"].as_str().unwrap().to_lowercase();
    let raw = json!({"target":side,"raw_diameter":p["raw_diameter"],"safety_dis":p["safety_dis"],"sides":{side:{"rows":[{"id":"one","gtype":"ES","std_key":p["std_key"],"d1":p["ES_d1"],"z":p["ES_z"],"d2":p["ES_d2"],"c":p["ES_c"],"angle":p["ES_c_ang"]}]}}});
    let mut request = json!({"options":{"line_numbers":true,"line_number_start":100,"line_number_step":10},"ops":[{"id":"first","business":{"plugin":"undercut","input":raw}},{"id":"second","business":{"plugin":"undercut","input":raw}}]});
    let result = f.call("process.generate.managed", request.clone());
    let numbers = result.artifacts[0]
        .text
        .lines()
        .filter_map(|line| {
            line.strip_prefix('N')
                .and_then(|n| n.split_whitespace().next())
                .and_then(|n| n.parse::<u64>().ok())
        })
        .collect::<Vec<_>>();
    assert!(numbers.len() > 10);
    assert_eq!(numbers[0], 110);
    assert!(numbers.windows(2).all(|p| p[1] == p[0] + 10));
    request["ops"][1]["result_bindings"] = json!({"/program":{"op":"first"}});
    assert!(f
        .app
        .runtime
        .invoke("process.generate.managed", request)
        .is_err());
}

#[test]
fn legacy_free_template_keeps_its_closed_context_schema() {
    let f = Fixture::new();
    let bundle: Value =
        serde_json::from_str(include_str!("../../examples/v3-managed/workspace.json")).unwrap();
    let mut asset = bundle["collections"]["template"]["records"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"] == "v3-sync-turning-right-template")
        .unwrap()["asset"]
        .clone();
    asset["source"] = asset["metadata"]["nc"]["legacy_source"].clone();
    asset["metadata"]["nc"]
        .as_object_mut()
        .unwrap()
        .remove("trajectory_plan_version");
    asset["metadata"]
        .as_object_mut()
        .unwrap()
        .remove("parameters");
    let raw = json!({"sta_x":60.,"sta_z":0.,"end_x":50.,"end_z":-10.,"ap_1":2.,"ap_2":3.,"sync_feed":0.2,"current_sync_num":10});
    let p = f.call(
        "nc.operation.prepare",
        json!({"operation":{"id":"sync.right","version":1},"inputs":raw}),
    );
    let properties = p.data["contexts"][0]
        .as_object()
        .unwrap()
        .keys()
        .filter(|k| k.as_str() != "trajectory_program")
        .map(|k| (k.clone(), json!(true)))
        .collect::<serde_json::Map<_, _>>();
    asset["metadata"]["nc"]["context_schema"] =
        json!({"type":"object","properties":properties,"additionalProperties":false});
    f.call("template.save", json!({"name":"legacy-sync","asset":asset}));
    let result = f.call(
        "nc.generate.managed",
        json!({"template":"legacy-sync","operation_inputs":raw}),
    );
    assert_eq!(result.artifacts.len(), 2);
    assert!(!result.artifacts[0].text.contains("trajectory_program"));
    assert!(result.artifacts[0].text.contains("WAITM(12,1,2)"));
}
