//! Boot recovery of interrupted executions from the derived run index.
//!
//! The `_index` summaries are derived state: recovery must repair the authoritative
//! run records, tolerate a stale or malformed index entry, and never lose runs that
//! are still present on disk.
use nctool_cli::workbench::{Run, Workbench};
use nctool_plugin_sdk::*;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

struct Temp(PathBuf);
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
impl Temp {
    fn new() -> Self {
        loop {
            let p = std::env::temp_dir().join(format!(
                "nctool-recovery-{}-{}-{}",
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

fn runtime() -> std::sync::Arc<nctool_runtime::Runtime> {
    nctool_runtime::Runtime::start(vec![], Default::default(), Default::default()).unwrap()
}
fn runs_dir(workspace: &Path) -> PathBuf {
    workspace.join(".nctool/runs")
}
fn record(id: &str, status: &str) -> Run {
    Run {
        id: id.into(),
        action: "demo.run".into(),
        input: json!({}),
        status: status.into(),
        started_at: 1_000,
        finished_at: None,
        result: None,
        error: None,
        plugins: vec![],
        title: id.into(),
        environment: None,
        replay: None,
    }
}
/// Write an authoritative run record plus its derived index summary; returns the exact
/// bytes on disk so a later boot can assert they are untouched.
fn write_run(workspace: &Path, id: &str, status: &str) -> Vec<u8> {
    let bytes = serde_json::to_vec(&record(id, status)).unwrap();
    std::fs::create_dir_all(runs_dir(workspace)).unwrap();
    std::fs::write(runs_dir(workspace).join(format!("{id}.json")), &bytes).unwrap();
    write_summary(workspace, id, status);
    bytes
}
fn write_summary(workspace: &Path, id: &str, status: &str) {
    write_raw_summary(
        workspace,
        &format!("{id}.json"),
        json!({"id": id, "status": status, "started_at": 1_000}),
    );
}
fn write_raw_summary(workspace: &Path, file: &str, body: Value) {
    let index = runs_dir(workspace).join("_index");
    std::fs::create_dir_all(&index).unwrap();
    std::fs::write(index.join(file), serde_json::to_vec(&body).unwrap()).unwrap();
}

#[test]
fn boot_recovery_marks_interrupted_and_preserves_other_runs() {
    let temp = Temp::new();
    let ws = &temp.0;
    let finished = write_run(ws, "run-b", "succeeded");
    write_run(ws, "run-a", "running");

    let wb = Workbench::new(runtime(), ws).unwrap();
    assert_eq!(wb.get("run-a").unwrap().status, "interrupted");
    // A finished run is left byte-for-byte intact and keeps its recorded status.
    assert_eq!(
        std::fs::read(runs_dir(ws).join("run-b.json")).unwrap(),
        finished
    );
    assert_eq!(wb.get("run-b").unwrap().status, "succeeded");

    // Recovery is idempotent: a second boot neither reverts nor loses either run.
    let again = Workbench::new(runtime(), ws).unwrap();
    assert_eq!(again.get("run-a").unwrap().status, "interrupted");
    assert_eq!(
        std::fs::read(runs_dir(ws).join("run-b.json")).unwrap(),
        finished
    );
    let ids = again.list().unwrap()["runs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|run| run["id"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    assert!(ids.contains(&"run-a".to_string()) && ids.contains(&"run-b".to_string()));
}

#[test]
fn orphan_index_entry_is_pruned_without_blocking_boot() {
    let temp = Temp::new();
    let ws = &temp.0;
    write_run(ws, "live", "running");
    // A crash between deleting the run record and its index entry leaves this behind.
    write_summary(ws, "gone", "running");

    let wb = Workbench::new(runtime(), ws).unwrap();
    // The orphan entry is a rebuildable summary, dropped rather than failing boot.
    assert!(!runs_dir(ws).join("_index/gone.json").exists());
    // The remaining run is still recovered, so no existing task is lost.
    assert_eq!(wb.get("live").unwrap().status, "interrupted");
    let ids = wb.list().unwrap()["runs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|run| run["id"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    assert_eq!(ids, vec!["live".to_string()]);
}

#[test]
fn malformed_index_entries_do_not_abort_boot() {
    let temp = Temp::new();
    let ws = &temp.0;
    // Confirmed regression: a running summary with no `id` used to panic at boot.
    write_raw_summary(
        ws,
        "no-id.json",
        json!({"status": "running", "started_at": 1}),
    );
    // An id that is not a usable run id must be skipped, never used as a path.
    write_raw_summary(
        ws,
        "escape.json",
        json!({"id": "../escape", "status": "queued"}),
    );
    write_run(ws, "good", "running");

    let wb = Workbench::new(runtime(), ws).unwrap();
    assert_eq!(wb.get("good").unwrap().status, "interrupted");
    assert!(!ws.join(".nctool/escape.json").exists());
    assert!(!ws.join("../escape.json").exists());
}

#[test]
fn unparseable_or_oversized_index_entries_do_not_abort_boot_or_history() {
    let temp = Temp::new();
    let ws = &temp.0;
    // A truncated write, a summary rejected by the numeric-precision policy, and an
    // oversized summary are all derived-state failures; none may brick the workbench.
    std::fs::create_dir_all(runs_dir(ws).join("_index")).unwrap();
    std::fs::write(runs_dir(ws).join("_index/truncated.json"), b"{").unwrap();
    std::fs::write(
        runs_dir(ws).join("_index/underflow.json"),
        br#"{"id":"underflow","status":"running","started_at":1e-400}"#,
    )
    .unwrap();
    std::fs::write(
        runs_dir(ws).join("_index/oversized.json"),
        vec![b' '; 1024 * 1024 + 1],
    )
    .unwrap();
    write_run(ws, "good", "running");

    let wb = Workbench::new(runtime(), ws).unwrap();
    assert_eq!(wb.get("good").unwrap().status, "interrupted");
    let listed = wb.list().unwrap();
    let ids: Vec<_> = listed["runs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|run| run["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec!["good"]);
    // These three entries have no authoritative record, so they are dropped rather than
    // kept as permanently unusable noise.
    assert!(!runs_dir(ws).join("_index/truncated.json").exists());
    assert!(!runs_dir(ws).join("_index/underflow.json").exists());
    assert!(!runs_dir(ws).join("_index/oversized.json").exists());
}

#[test]
fn a_damaged_summary_is_rebuilt_so_the_run_stays_visible() {
    let temp = Temp::new();
    let ws = &temp.0;
    let finished = write_run(ws, "run-visible", "succeeded");
    // Simulate a torn write of the derived summary after the run completed. Without a
    // rebuild the run would vanish from history forever, because the index is the only
    // listing source.
    std::fs::write(runs_dir(ws).join("_index/run-visible.json"), b"{").unwrap();

    let wb = Workbench::new(runtime(), ws).unwrap();
    let ids: Vec<_> = wb.list().unwrap()["runs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|run| run["id"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(ids, vec!["run-visible".to_string()]);
    // The summary was repaired from the record, and the record itself is untouched.
    let repaired = std::fs::read(runs_dir(ws).join("_index/run-visible.json")).unwrap();
    let repaired: serde_json::Value = serde_json::from_slice(&repaired).unwrap();
    assert_eq!(repaired["status"], "succeeded");
    assert_eq!(
        std::fs::read(runs_dir(ws).join("run-visible.json")).unwrap(),
        finished
    );
}

#[test]
fn an_unreadable_summary_whose_record_is_gone_is_pruned() {
    let temp = Temp::new();
    let ws = &temp.0;
    std::fs::create_dir_all(runs_dir(ws).join("_index")).unwrap();
    std::fs::write(runs_dir(ws).join("_index/ghost.json"), b"{").unwrap();
    let wb = Workbench::new(runtime(), ws).unwrap();
    assert!(wb.list().unwrap()["runs"].as_array().unwrap().is_empty());
    assert!(!runs_dir(ws).join("_index/ghost.json").exists());
}
