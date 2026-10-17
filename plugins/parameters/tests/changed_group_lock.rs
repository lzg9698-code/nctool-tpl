//! `Library::changed` must compare frozen records under the same workspace group
//! lock `capture`/`invoke` use, so a concurrent multi-asset import can never be
//! observed half-applied. See `target/optimization-20261017/template-parameters.log`.
use nctool_assets::GroupGuard;
use nctool_plugin_parameters::{validate_parameter, Library};
use nctool_plugin_sdk::*;
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// Parallel tests can read the same clock tick, so pair the timestamp with a
/// counter; otherwise one fixture's cleanup may remove another's directory.
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "nctool-param-lock-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn library(root: &Path) -> Library {
    Library::new(
        root,
        &[
            ("definitions", "parameters/definitions"),
            ("presets", "parameters/presets"),
        ],
    )
    .unwrap()
}

#[test]
fn changed_reads_under_the_workspace_group_lock() {
    let temp = Temp::new();
    let lib = library(temp.path());
    lib.invoke(
        "save",
        &json!({
            "group": "definitions",
            "name": "feed",
            "asset": { "format_version": 1, "id": "feed", "schema": { "type": "object" } }
        }),
        validate_parameter,
    )
    .unwrap();
    let snapshot = lib.capture().unwrap();

    // Hold the workspace group lock exactly as a concurrent import would.
    let guard = GroupGuard::acquire(temp.path()).unwrap();

    let (tx, rx) = mpsc::channel();
    let reader = library(temp.path());
    let snapshot = snapshot.clone();
    let handle = thread::spawn(move || {
        let _ = tx.send(reader.changed(&snapshot));
    });

    // Deterministic: `changed` must not complete while another writer holds the lock.
    thread::sleep(Duration::from_millis(300));
    assert!(
        rx.try_recv().is_err(),
        "changed read the stores while another writer held the group lock"
    );

    drop(guard);
    let result = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("changed must finish once the group lock is released");
    handle.join().unwrap();
    assert_eq!(result.unwrap(), Vec::<String>::new(), "nothing changed");
}
