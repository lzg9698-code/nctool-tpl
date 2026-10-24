use nctool_plugin_sdk::*;
use nctool_runtime::Runtime;
use std::{collections::BTreeMap, sync::Arc};
struct Caller;
impl Plugin for Caller {
    fn descriptor(&self) -> PluginDescriptor {
        let mut d = PluginDescriptor::builtin("caller");
        d.requires.push(Requirement::new("template.render"));
        d
    }
    fn actions(&self) -> Vec<ActionDescriptor> {
        vec![ActionDescriptor::new(
            "caller.call",
            "call",
            object_schema(),
        )]
    }
    fn invoke(&self, _: &str, input: Value, ctx: &Context) -> PluginResult<ActionResult> {
        if input.get("panic").is_some() {
            panic!("sensitive panic body");
        }
        ctx.call(
            "template.render",
            1,
            json!({"source":"{{ value }}","context":{"value":input["secret"]}}),
        )
    }
}
#[test]
fn traces_correlate_nested_calls_without_recording_payloads_and_bound_memory() {
    let workspace = std::env::temp_dir().join(format!(
        "diagnostics-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let runtime = Runtime::start(
        vec![
            Arc::new(nctool_plugin_template::TemplatePlugin::new(&workspace).unwrap()),
            Arc::new(Caller),
        ],
        BTreeMap::new(),
        BTreeMap::new(),
    )
    .unwrap();
    runtime
        .invoke_request("root-test", "caller.call", json!({"secret":"do-not-log"}))
        .unwrap();
    let calls = runtime.diagnostics();
    assert_eq!(calls.len(), 2);
    assert!(calls.iter().all(|c| c.trace_id == "root-test"));
    assert_eq!(calls[0].parent_action.as_deref(), Some("caller.call"));
    assert_eq!(calls[0].depth, 1);
    assert!(!serde_json::to_string(&calls)
        .unwrap()
        .contains("do-not-log"));
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(
        || runtime.invoke("caller.call", json!({"panic":true}))
    ))
    .is_err());
    assert_eq!(
        runtime.diagnostics().last().unwrap().error_code.as_deref(),
        Some("plugin_panic")
    );
    runtime.invoke("caller.call", json!(null)).unwrap_err();
    assert_eq!(
        runtime.diagnostics().last().unwrap().error_code.as_deref(),
        Some("invalid_input")
    );
    for _ in 0..300 {
        let _ = runtime.invoke("missing.call", json!({}));
    }
    assert_eq!(runtime.diagnostics().len(), 256);
    assert!(!serde_json::to_string(&runtime.diagnostics())
        .unwrap()
        .contains("sensitive panic"));
    runtime.shutdown();
    std::fs::remove_dir_all(workspace).unwrap();
}
