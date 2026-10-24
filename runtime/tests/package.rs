use nctool_plugin_sdk::*;
use nctool_runtime::external::ExternalManifest;
#[test]
fn package_rejects_foreign_actions_duplicate_kinds_and_unknown_versions() {
    let root = std::env::temp_dir().join(format!(
        "package-contract-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let manifest = json!({"descriptor":{"id":"demo","version":"0.1.0","protocol_version":2},"command":["program"],"actions":[{"id":"demo.action","title":"Demo","input_schema":{"type":"object"},"output_schema":{"type":"object"}}]});
    std::fs::write(
        root.join("plugin.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    let view = json!({"id":"editor","kind":"project","title":"Editor","analyze_action":"demo.action","generate_action":"demo.action","schema":{"type":"object"},"example":{},"formats":["json"]});
    let package = json!({"id":"demo","format_version":1,"workbenches":[view]});
    let write = |p: &Value| {
        std::fs::write(
            root.join("nctool.package.json"),
            serde_json::to_vec(p).unwrap(),
        )
        .unwrap()
    };
    write(&package);
    ExternalManifest::read(&root).unwrap();
    let mut bad = package.clone();
    bad["workbenches"][0]["generate_action"] = json!("other.generate");
    write(&bad);
    assert_eq!(
        ExternalManifest::read(&root).unwrap_err().code,
        "invalid_package"
    );
    let mut bad = package.clone();
    bad["format_version"] = json!(2);
    write(&bad);
    assert_eq!(
        ExternalManifest::read(&root).unwrap_err().code,
        "invalid_package"
    );
    let mut bad = package.clone();
    let mut second = view.clone();
    second["id"] = json!("second");
    bad["workbenches"].as_array_mut().unwrap().push(second);
    write(&bad);
    assert_eq!(
        ExternalManifest::read(&root).unwrap_err().code,
        "invalid_package"
    );
    assert!(root.starts_with(std::env::temp_dir()));
    std::fs::remove_dir_all(root).unwrap();
}
