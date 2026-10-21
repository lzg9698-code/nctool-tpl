use nctool_cli::composition::App;
use nctool_plugin_sdk::*;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};
/// Tests in one binary run on parallel threads, and the system clock can hand the
/// same nanosecond value to two of them; a process-wide counter keeps each temp
/// directory distinct so one fixture's cleanup cannot delete another's files.
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    root: PathBuf,
    app: Arc<App>,
}
impl Fixture {
    fn new(profile: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "managed-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
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

#[test]
fn malformed_saved_template_bindings_are_rejected_instead_of_ignored() {
    let f = Fixture::new("template");
    f.definition("depth", json!({"default": 1}));
    for (index, bindings) in [json!(null), json!([]), json!("bad")]
        .into_iter()
        .enumerate()
    {
        let name = format!("malformed-bindings-{index}");
        f.invoke("template.save", json!({"name":name,"asset":{"source":"{{ depth }}","metadata":{"parameters":{"bindings":bindings}}}}));
        for input in [
            json!({"template":name,"context":{"depth":99}}),
            json!({"template":name,"context":{"depth":99},"bindings":{}}),
        ] {
            let error = f
                .app
                .runtime
                .invoke("template.render.managed", input)
                .unwrap_err();
            assert_eq!(error.code, "invalid_binding");
        }
    }

    // A genuinely absent bindings field remains equivalent to an empty binding set.
    f.invoke(
        "template.save",
        json!({"name":"no-bindings","asset":{"source":"{{ depth }}","metadata":{"parameters":{}}}}),
    );
    assert_eq!(
        f.invoke(
            "template.render.managed",
            json!({"template":"no-bindings","context":{"depth":99}})
        )
        .data["text"],
        "99"
    );
}

#[test]
fn explicit_null_declines_the_saved_shared_preset_without_losing_asset_inheritance() {
    let f = Fixture::new("template");
    f.definition("depth", json!({"default":1}));
    f.invoke("parameter.save",json!({"group":"presets","name":"rough","asset":{"format_version":1,"id":"rough","values":{"depth":4}}}));
    f.invoke("template.save",json!({"name":"preset-guard","asset":{"source":"{{ depth }}","metadata":{"parameters":{"bindings":{"/depth":{"id":"depth"}},"preset_id":"rough"}}}}));
    let bindings = json!({"/depth":{"id":"depth"}});

    // Absent: the saved asset preset still applies.
    let inherited = f.invoke(
        "template.render.managed",
        json!({"template":"preset-guard","bindings":bindings}),
    );
    assert_eq!(inherited.data["text"], "4");

    // Explicit null declines it, falling back to the definition default.
    let declined = f.invoke(
        "template.render.managed",
        json!({"template":"preset-guard","bindings":bindings,"preset_id":Value::Null}),
    );
    assert_eq!(declined.data["text"], "1");
    assert_eq!(
        declined.data["parameters"]["origins"]["/depth"],
        "definition"
    );
    // The frozen request preserves the explicit decline.
    assert!(declined.data["replay_input"]["parameter_request"]["preset_id"].is_null());
    assert_eq!(
        f.invoke(
            "template.render.managed",
            declined.data["replay_input"].clone()
        )
        .data["text"],
        "1"
    );

    // A chosen preset still wins over the inherited one.
    f.invoke("parameter.save",json!({"group":"presets","name":"finish","asset":{"format_version":1,"id":"finish","values":{"depth":7}}}));
    let chosen = f.invoke(
        "template.render.managed",
        json!({"template":"preset-guard","bindings":bindings,"preset_id":"finish"}),
    );
    assert_eq!(chosen.data["text"], "7");

    // Resolve accepts the same decline, and snapshot + preset stay mutually exclusive.
    assert_eq!(
        f.invoke(
            "parameter.resolve",
            json!({"bindings":bindings,"preset_id":Value::Null})
        )
        .data["values"]["depth"],
        1
    );
    assert_eq!(
        f.app
            .runtime
            .invoke(
                "parameter.resolve",
                json!({"bindings":bindings,"preset_id":"rough","snapshot":f.invoke("parameter.snapshot",json!({})).data}),
            )
            .unwrap_err()
            .code,
        "invalid_input"
    );
}

#[test]
fn dependency_check_detects_a_previously_missing_optional_reference() {
    let f = Fixture::new("template");
    // The optional reference does not resolve yet, so the first result renders AZ.
    let first = f.invoke(
        "template.render.managed",
        json!({"source":"A{% include \"optional\" ignore missing %}Z"}),
    );
    assert_eq!(first.data["text"], "AZ");
    let replay = first.data["replay_input"].clone();
    assert!(replay["snapshot"]["templates"]["optional"].is_null());

    // Creating the referenced template changes what this template actually renders.
    f.invoke(
        "template.save",
        json!({"name":"optional","asset":{"source":"NEW"}}),
    );
    assert_eq!(
        f.invoke(
            "template.render.managed",
            json!({"source":"A{% include \"optional\" ignore missing %}Z"})
        )
        .data["text"],
        "ANEWZ"
    );

    // The frozen result must now be reported as stale, with the resolved dependency named.
    let check = f.invoke(
        "parameter.dependencies.check",
        json!({"snapshot":replay["parameter_snapshot"],"template_snapshot":replay["snapshot"]}),
    );
    assert_eq!(check.data["current"], false);
    assert_eq!(check.data["changes"], json!(["templates/optional"]));

    // The frozen replay itself still reproduces the original AZ: history is never rewritten.
    assert_eq!(
        f.invoke("template.render.managed", replay.clone()).data["text"],
        "AZ"
    );
}

#[test]
fn dependency_check_ignores_new_templates_the_frozen_sources_never_reference() {
    let f = Fixture::new("template");
    let first = f.invoke(
        "template.render.managed",
        json!({"source":"A{% include \"optional\" ignore missing %}Z"}),
    );
    let replay = first.data["replay_input"].clone();
    // An unrelated addition is not a dependency of this result.
    f.invoke(
        "template.save",
        json!({"name":"unrelated","asset":{"source":"OTHER"}}),
    );
    assert_eq!(
        f.invoke(
            "parameter.dependencies.check",
            json!({"snapshot":replay["parameter_snapshot"],"template_snapshot":replay["snapshot"]})
        )
        .data,
        json!({"current":true,"changes":[]})
    );

    // A reference that was frozen but unresolved still cannot be walked into, because the
    // frozen collection has no source for it. Resolving it is reported by its own name.
    f.invoke(
        "template.save",
        json!({"name":"optional","asset":{"source":"NEW"}}),
    );
    assert_eq!(
        f.invoke(
            "parameter.dependencies.check",
            json!({"snapshot":replay["parameter_snapshot"],"template_snapshot":replay["snapshot"]})
        )
        .data["changes"],
        json!(["templates/optional"])
    );
}

#[test]
fn dependency_check_follows_the_frozen_reference_walk_transitively() {
    let f = Fixture::new("template");
    // The optional reference is frozen with a real source, so its own nested reference
    // is part of this result's dependency set.
    f.invoke(
        "template.save",
        json!({"name":"optional","asset":{"source":"{% include \"nested\" ignore missing %}"}}),
    );
    let first = f.invoke(
        "template.render.managed",
        json!({"source":"A{% include \"optional\" ignore missing %}Z"}),
    );
    let replay = first.data["replay_input"].clone();
    assert_eq!(
        f.invoke(
            "parameter.dependencies.check",
            json!({"snapshot":replay["parameter_snapshot"],"template_snapshot":replay["snapshot"]})
        )
        .data,
        json!({"current":true,"changes":[]})
    );

    // Resolving the nested reference behind it is detected one level deeper.
    f.invoke(
        "template.save",
        json!({"name":"nested","asset":{"source":"DEEP"}}),
    );
    assert_eq!(
        f.invoke(
            "parameter.dependencies.check",
            json!({"snapshot":replay["parameter_snapshot"],"template_snapshot":replay["snapshot"]})
        )
        .data,
        json!({"current":false,"changes":["templates/nested"]})
    );
}

#[test]
fn dependency_check_still_detects_removed_and_edited_reachable_dependencies() {
    let f = Fixture::new("template");
    f.invoke(
        "template.save",
        json!({"name":"shared","asset":{"source":"NEW"}}),
    );
    let saved = f.invoke("template.read", json!({"name":"shared"})).data["fingerprint"].clone();
    let first = f.invoke(
        "template.render.managed",
        json!({"source":"X{% include \"shared\" ignore missing %}"}),
    );
    assert_eq!(first.data["text"], "XNEW");
    let replay = first.data["replay_input"].clone();
    let unchanged =
        json!({"snapshot":replay["parameter_snapshot"],"template_snapshot":replay["snapshot"]});
    assert_eq!(
        f.invoke("parameter.dependencies.check", unchanged.clone())
            .data,
        json!({"current":true,"changes":[]})
    );

    // Editing a reachable dependency invalidates the frozen result.
    f.invoke(
        "template.save",
        json!({"name":"shared","asset":{"source":"CHANGED"},"expected":saved}),
    );
    assert_eq!(
        f.invoke("parameter.dependencies.check", unchanged.clone())
            .data,
        json!({"current":false,"changes":["templates/shared"]})
    );

    // Removing it is detected too, and the frozen replay still produces the original text.
    let edited = f.invoke("template.read", json!({"name":"shared"})).data["fingerprint"].clone();
    f.invoke(
        "template.remove",
        json!({"name":"shared","expected":edited}),
    );
    assert_eq!(
        f.invoke("parameter.dependencies.check", unchanged).data,
        json!({"current":false,"changes":["templates/shared"]})
    );
    assert_eq!(
        f.invoke("template.render.managed", replay).data["text"],
        "XNEW"
    );
}
