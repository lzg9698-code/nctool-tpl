use nctool_cli::history;
use nctool_plugin_sdk::*;
use std::path::PathBuf;
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "history-maintenance-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("workspace/.nctool/runs/_index")).unwrap();
        Self(root)
    }
    fn workspace(&self) -> PathBuf {
        self.0.join("workspace")
    }
    fn record(&self, id: &str, status: &str, time: u64) -> Vec<u8> {
        let value = json!({"id":id,"action":"template.render","input":{"n":9_007_199_254_740_993u64},"title":"record","status":status,"started_at":time,"finished_at":time+1,"result":null,"error":null,"plugins":[],"unknown_future_field":{"preserve":true}});
        let bytes = serde_json::to_vec_pretty(&value).unwrap();
        std::fs::write(
            self.workspace().join(format!(".nctool/runs/{id}.json")),
            &bytes,
        )
        .unwrap();
        bytes
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[test]
fn backup_restore_preserves_bytes_and_rejects_overwrite_tampering_and_running_records() {
    let f = Fixture::new();
    let bytes = f.record("old", "succeeded", 1);
    f.record("active", "running", 2);
    let archive = f.0.join("backup.json");
    let report = history::backup(&f.workspace(), &archive).unwrap();
    assert_eq!(report["records"], 1);
    assert_eq!(report["skipped"], json!(["active"]));
    assert!(history::backup(&f.workspace(), &archive).is_err());
    let restored = f.0.join("restored");
    history::restore(&restored, &archive).unwrap();
    assert_eq!(
        std::fs::read(restored.join(".nctool/runs/old.json")).unwrap(),
        bytes
    );
    assert!(!restored.join(".nctool/restore-incomplete.json").exists());
    assert!(history::restore(&restored, &archive).is_err());
    let mut tampered: Value = serde_json::from_slice(&std::fs::read(&archive).unwrap()).unwrap();
    tampered["records"]["old"]["raw"] = json!("changed");
    std::fs::write(&archive, serde_json::to_vec(&tampered).unwrap()).unwrap();
    let destination = f.0.join("tampered");
    assert_eq!(
        history::restore(&destination, &archive).unwrap_err().code,
        "invalid_archive"
    );
    assert!(!destination.exists());
    assert!(history::backup(
        &f.workspace(),
        &f.workspace().join(".nctool/runs/backup.json")
    )
    .is_err());
}
#[test]
fn retention_preview_apply_requires_unchanged_plan_and_backup_and_skips_locked_records() {
    let f = Fixture::new();
    f.record("old", "succeeded", 1);
    f.record("new", "succeeded", 10);
    f.record("locked", "succeeded", 0);
    let guard = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(f.workspace().join(".nctool/runs/locked.nctool.lock"))
        .unwrap();
    guard.lock().unwrap();
    let plan = history::prune(&f.workspace(), 100, 1, None, None).unwrap();
    assert_eq!(plan["candidates"], json!(["old"]));
    assert_eq!(plan["skipped"], json!(["locked"]));
    let token = plan["fingerprint"].as_str().unwrap();
    assert_eq!(
        history::prune(&f.workspace(), 100, 1, Some(token), None)
            .unwrap_err()
            .code,
        "backup_required"
    );
    f.record("old", "succeeded", 2);
    assert_eq!(
        history::prune(
            &f.workspace(),
            100,
            1,
            Some(token),
            Some(&f.0.join("backup.json"))
        )
        .unwrap_err()
        .code,
        "conflict"
    );
    let plan = history::prune(&f.workspace(), 100, 1, None, None).unwrap();
    history::prune(
        &f.workspace(),
        100,
        1,
        plan["fingerprint"].as_str(),
        Some(&f.0.join("backup.json")),
    )
    .unwrap();
    assert!(!f.workspace().join(".nctool/runs/old.json").exists());
    assert!(f.workspace().join(".nctool/runs/new.json").exists());
    assert!(f.workspace().join(".nctool/runs/locked.json").exists());
    assert!(f.0.join("backup.json").exists());
}
#[test]
fn incomplete_restore_is_rejected_before_plugin_boot() {
    let f = Fixture::new();
    std::fs::write(f.workspace().join(".nctool/restore-incomplete.json"), "{}").unwrap();
    let error = nctool_cli::composition::App::boot(&f.0.join("home"), &f.workspace(), None)
        .err()
        .unwrap();
    assert_eq!(error.code, "incomplete_restore");
    assert!(!f.0.join("home").exists());
}
#[test]
fn invalid_archive_members_fail_before_creating_a_destination() {
    let f = Fixture::new();
    let original = f.record("source", "succeeded", 1);
    for (name, ids, status) in [
        ("reserved", vec!["CON"], "succeeded"),
        ("case-alias", vec!["run-one", "RUN-ONE"], "succeeded"),
        ("active", vec!["run-one"], "running"),
    ] {
        let mut records = serde_json::Map::new();
        for id in ids {
            let mut run: Value = serde_json::from_slice(&original).unwrap();
            run["id"] = json!(id);
            run["status"] = json!(status);
            let raw = serde_json::to_string(&run).unwrap();
            records.insert(id.into(),json!({"fingerprint":nctool_runtime::provenance::fingerprint(raw.as_bytes()),"raw":raw}));
        }
        let archive = f.0.join(format!("{name}.json"));
        std::fs::write(
            &archive,
            serde_json::to_vec(&json!({"format_version":1,"records":records})).unwrap(),
        )
        .unwrap();
        let destination = f.0.join(format!("restore-{name}"));
        assert_eq!(
            history::restore(&destination, &archive).unwrap_err().code,
            "invalid_archive"
        );
        assert!(!destination.exists());
    }
}
#[test]
fn cli_history_commands_work_without_booting_plugins_and_require_confirmed_backup() {
    let f = Fixture::new();
    let old = f.record("old", "succeeded", 1);
    f.record("latest", "succeeded", 20);
    let home = f.0.join("unused-home");
    let invoke = |arguments: Vec<String>| {
        std::process::Command::new(env!("CARGO_BIN_EXE_nctool"))
            .arg("--home")
            .arg(&home)
            .arg("--workspace")
            .arg(f.workspace())
            .args(arguments)
            .output()
            .unwrap()
    };
    let archive = f.0.join("cli-backup.json");
    let saved = invoke(vec![
        "history".into(),
        "backup".into(),
        "--out".into(),
        archive.to_string_lossy().into_owned(),
    ]);
    assert!(
        saved.status.success(),
        "{}",
        String::from_utf8_lossy(&saved.stderr)
    );
    assert!(!home.exists());
    let preview = invoke(vec![
        "history".into(),
        "prune".into(),
        "--before-ms".into(),
        "100".into(),
        "--keep-latest".into(),
        "1".into(),
    ]);
    assert!(preview.status.success());
    let plan: Value = serde_json::from_slice(&preview.stdout).unwrap();
    let mut apply = vec![
        "history".into(),
        "prune".into(),
        "--before-ms".into(),
        "100".into(),
        "--keep-latest".into(),
        "1".into(),
        "--apply".into(),
        plan["fingerprint"].as_str().unwrap().into(),
    ];
    assert!(
        !invoke(apply.clone()).status.success(),
        "backup must be mandatory at the CLI boundary"
    );
    let deleted_backup = f.0.join("cli-deleted.json");
    apply.extend([
        "--backup".into(),
        deleted_backup.to_string_lossy().into_owned(),
    ]);
    assert!(invoke(apply).status.success());
    assert!(!f.workspace().join(".nctool/runs/old.json").exists());
    let restored = f.0.join("cli-restored");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_nctool"))
        .arg("--workspace")
        .arg(&restored)
        .args(["history", "restore", "--archive"])
        .arg(&archive)
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        std::fs::read(restored.join(".nctool/runs/old.json")).unwrap(),
        old
    );
}
