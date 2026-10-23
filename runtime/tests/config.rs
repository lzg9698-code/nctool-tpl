//! Inventory tolerance for local plugin directories.
//!
//! A directory that is not a plugin, or whose manifest is damaged, must not take down
//! the whole inventory or boot; a plugin that is enabled still fails closed, and a
//! permission/IO failure stays visible instead of being reported as a damaged manifest.
use nctool_plugin_sdk::*;
use nctool_runtime::config::{self, Config};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

struct Temp(PathBuf);
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
impl Temp {
    fn new() -> Self {
        loop {
            let p = std::env::temp_dir().join(format!(
                "nctool-config-{}-{}-{}",
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
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn manifest(id: &str) -> Value {
    json!({
        "descriptor": {"id": id, "version": "1.0.0", "protocol_version": 2},
        "command": [id],
        "actions": [{
            "id": format!("{id}.run"),
            "title": id,
            "input_schema": {"type": "object"},
            "output_schema": {"type": "object"}
        }]
    })
}
/// Write `plugins/<dir>/plugin.json` (the manifest may be any value, valid or not).
fn install(home: &Path, dir: &str, manifest: &Value) {
    let root = home.join("plugins").join(dir);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("plugin.json"),
        serde_json::to_vec_pretty(manifest).unwrap(),
    )
    .unwrap();
}
fn config_ids(enabled: &[&str]) -> Config {
    Config {
        enabled: enabled.iter().map(|id| id.to_string()).collect(),
        ..Config::default()
    }
}
fn ids(found: &[(nctool_runtime::external::ExternalManifest, PathBuf)]) -> Vec<String> {
    found.iter().map(|(m, _)| m.descriptor.id.clone()).collect()
}

#[test]
fn valid_plugin_loads_and_enabled_selection_resolves() {
    let temp = Temp::new();
    install(&temp.0, "demo", &manifest("demo"));
    assert_eq!(ids(&config::installed(&temp.0).unwrap()), vec!["demo"]);
    let selected = config::external_plugins(&temp.0, &config_ids(&["demo"])).unwrap();
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].descriptor().id, "demo");
}

#[test]
fn unrelated_directories_and_files_are_skipped() {
    let temp = Temp::new();
    install(&temp.0, "demo", &manifest("demo"));
    // A directory with no manifest and a stray file are not plugins.
    std::fs::create_dir_all(temp.0.join("plugins/notes")).unwrap();
    std::fs::write(temp.0.join("plugins/notes/todo.txt"), b"x").unwrap();
    std::fs::write(temp.0.join("plugins/loose.txt"), b"x").unwrap();

    assert_eq!(ids(&config::installed(&temp.0).unwrap()), vec!["demo"]);
    let (_, broken) = config::scan_installed(&temp.0).unwrap();
    assert!(
        broken.is_empty(),
        "unrelated entries are not damaged plugins"
    );
    assert!(config::external_plugins(&temp.0, &config_ids(&["demo"])).is_ok());
}

#[test]
fn damaged_manifest_is_recorded_and_only_blocks_when_enabled() {
    let temp = Temp::new();
    // Missing required fields: a manifest that exists but cannot be loaded.
    install(
        &temp.0,
        "broken",
        &json!({"descriptor": {"id": "broken"}, "command": []}),
    );

    // Inventory tolerates it: an unenabled damaged plugin does not block boot.
    assert!(config::installed(&temp.0).unwrap().is_empty());
    let (_, broken) = config::scan_installed(&temp.0).unwrap();
    assert_eq!(broken.len(), 1);
    assert_eq!(broken[0].directory, "broken");
    assert!(!broken[0].error.code.is_empty());
    assert!(config::external_plugins(&temp.0, &config_ids(&[])).is_ok());

    // Selecting it stays fail-closed and names the concrete failure.
    let error = config::external_plugins(&temp.0, &config_ids(&["broken"]))
        .err()
        .unwrap();
    assert_eq!(error.code, "plugin_broken");
    assert!(error.message.contains("broken"));
}

#[test]
fn directory_id_mismatch_is_recorded_not_fatal() {
    let temp = Temp::new();
    install(&temp.0, "wrongname", &manifest("demo"));
    assert!(config::installed(&temp.0).unwrap().is_empty());
    let (_, broken) = config::scan_installed(&temp.0).unwrap();
    assert_eq!(broken.len(), 1);
    assert_eq!(broken[0].directory, "wrongname");
    assert_eq!(broken[0].error.code, "plugin_directory");
    assert!(config::external_plugins(&temp.0, &config_ids(&[])).is_ok());
    assert_eq!(
        config::external_plugins(&temp.0, &config_ids(&["wrongname"]))
            .err()
            .unwrap()
            .code,
        "plugin_broken"
    );
}

#[test]
fn inventory_scan_does_not_modify_plugin_assets() {
    let temp = Temp::new();
    install(&temp.0, "demo", &manifest("demo"));
    let manifest_path = temp.0.join("plugins/demo/plugin.json");
    let listing = || {
        let mut names = std::fs::read_dir(temp.0.join("plugins/demo"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        names.sort();
        names
    };
    let before = std::fs::read(&manifest_path).unwrap();
    let before_listing = listing();

    let _ = config::installed(&temp.0).unwrap();
    let _ = config::scan_installed(&temp.0).unwrap();
    let _ = config::external_plugins(&temp.0, &config_ids(&["demo"])).unwrap();

    assert_eq!(std::fs::read(&manifest_path).unwrap(), before);
    assert_eq!(listing(), before_listing);
}

#[cfg(unix)]
#[test]
fn permission_errors_are_not_masked() {
    use std::os::unix::fs::PermissionsExt;
    let temp = Temp::new();
    install(&temp.0, "demo", &manifest("demo"));
    let locked = temp.0.join("plugins/locked");
    std::fs::create_dir_all(&locked).unwrap();
    // A directory we cannot probe is a real access failure, not a damaged plugin.
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    let result = config::installed(&temp.0);
    // Restore permissions so the temporary directory can be removed.
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(result.unwrap_err().code, "io");
}
