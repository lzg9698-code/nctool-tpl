use nctool_plugin_sdk::*;
use nctool_runtime::provenance::*;

#[test]
fn private_config_values_do_not_influence_public_fingerprint() {
    let schema = json!({"properties":{"mode":{"x-nctool-public":true},"password":{"x-nctool-public":true},"hidden":{"writeOnly":true,"x-nctool-public":true},"nested":{"properties":{"count":{"x-nctool-public":true}}}}});
    let original =
        json!({"mode":"fast","password":"one","hidden":"one","other":"one","nested":{"count":3}});
    let changed =
        json!({"mode":"fast","password":"two","hidden":"two","other":"two","nested":{"count":3}});
    assert_eq!(
        public_config(&schema, &original),
        public_config(&schema, &changed)
    );
    let mut public = changed;
    public["nested"]["count"] = json!(4);
    assert_ne!(
        public_config(&schema, &original),
        public_config(&schema, &public)
    );
}

#[test]
fn package_changes_with_same_version_and_excludes_interpreter_cache() {
    let root = std::env::temp_dir().join(format!(
        "provenance-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("plugin.json"), "version same").unwrap();
    std::fs::write(root.join("plugin.py"), "first").unwrap();
    let original = package(&root).unwrap();
    std::fs::create_dir(root.join("__pycache__")).unwrap();
    std::fs::write(root.join("__pycache__/plugin.pyc"), "generated").unwrap();
    assert_eq!(package(&root).unwrap(), original);
    std::fs::write(root.join("plugin.py"), "other").unwrap();
    assert_ne!(package(&root).unwrap(), original);
    std::fs::remove_dir_all(root).unwrap();
}
