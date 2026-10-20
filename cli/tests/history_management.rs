use nctool_cli::composition::App;
use nctool_plugin_sdk::Value;
use serde_json::json;
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

struct Temp(PathBuf);
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
impl Temp {
    fn new() -> Self {
        loop {
            let path = std::env::temp_dir().join(format!(
                "nctool-history-{}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT_TEMP.fetch_add(1, Ordering::Relaxed),
            ));
            match std::fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("cannot create {}: {error}", path.display()),
            }
        }
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn app(temp: &Temp) -> std::sync::Arc<App> {
    let home = temp.0.join("home");
    let workspace = temp.0.join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    App::boot(&home, &workspace, None).unwrap()
}
fn index(workspace: &Path) -> PathBuf {
    workspace.join(".nctool/runs/_index")
}
fn summary(workspace: &Path, id: &str, title: &str, action: &str, status: &str, started_at: u64) {
    std::fs::create_dir_all(index(workspace)).unwrap();
    std::fs::write(
        index(workspace).join(format!("{id}.json")),
        serde_json::to_vec(&json!({
            "id": id,
            "title": title,
            "action": action,
            "status": status,
            "started_at": started_at,
            "finished_at": null,
            "diagnostic_count": 0
        }))
        .unwrap(),
    )
    .unwrap();
}
fn query(app: &App, body: Value) -> Value {
    app.dispatch("POST", "/api/v2/runs/query", body).unwrap()
}
fn ids(response: &Value) -> Vec<String> {
    response["runs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|run| run["id"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn legacy_list_stays_capped_and_query_pages_past_one_hundred() {
    let temp = Temp::new();
    let app = app(&temp);
    let workspace = temp.0.join("workspace");
    for i in 0..137 {
        summary(
            &workspace,
            &format!("run-{i:03}"),
            &format!("Job {i}"),
            if i % 2 == 0 {
                "template.render"
            } else {
                "external.run"
            },
            if i % 3 == 0 { "succeeded" } else { "failed" },
            10_000 - i,
        );
    }

    let legacy = app.dispatch("GET", "/api/v2/runs", json!({})).unwrap();
    assert_eq!(legacy["runs"].as_array().unwrap().len(), 100);
    let first = query(&app, json!({"limit": 100}));
    let second = query(&app, json!({"offset": 100, "limit": 100}));
    assert_eq!(first["total"], 137);
    assert_eq!(second["total"], 137);
    assert_eq!(first["offset"], 0);
    assert_eq!(first["limit"], 100);
    assert_eq!(ids(&first).len(), 100);
    assert_eq!(ids(&second).len(), 37);
    assert_eq!(ids(&first)[99], "run-099");
    assert_eq!(ids(&second)[0], "run-100");
    let all_ids = ids(&first)
        .into_iter()
        .chain(ids(&second))
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(all_ids.len(), 137);
    assert_eq!(query(&app, json!({}))["limit"], 25);
}

#[test]
fn query_filters_status_and_case_insensitive_title_id_action() {
    let temp = Temp::new();
    let app = app(&temp);
    let workspace = temp.0.join("workspace");
    summary(
        &workspace,
        "run-alpha",
        "Monthly REPORT",
        "template.render",
        "succeeded",
        3,
    );
    summary(&workspace, "run-beta", "Other", "external.run", "failed", 2);
    summary(
        &workspace,
        "run-gamma",
        "Other",
        "Template.Import",
        "succeeded",
        1,
    );

    assert_eq!(
        ids(&query(&app, json!({"query":"report"}))),
        vec!["run-alpha"]
    );
    assert_eq!(
        ids(&query(&app, json!({"query":"ALPHA"}))),
        vec!["run-alpha"]
    );
    assert_eq!(
        ids(&query(&app, json!({"query":"TEMPLATE"}))),
        vec!["run-alpha", "run-gamma"]
    );
    assert_eq!(
        ids(&query(&app, json!({"status":"failed"}))),
        vec!["run-beta"]
    );
    assert_eq!(
        ids(&query(
            &app,
            json!({"query":"template", "status":"succeeded"})
        )),
        vec!["run-alpha", "run-gamma"]
    );
}

#[test]
fn equal_timestamps_have_stable_id_order() {
    let temp = Temp::new();
    let app = app(&temp);
    let workspace = temp.0.join("workspace");
    for id in ["run-z", "run-a", "run-m"] {
        summary(&workspace, id, id, "demo", "succeeded", 42);
    }
    assert_eq!(
        ids(&query(&app, json!({}))),
        vec!["run-a", "run-m", "run-z"]
    );
    assert_eq!(
        ids(&query(&app, json!({}))),
        vec!["run-a", "run-m", "run-z"]
    );
}

#[test]
fn rejects_invalid_query_types_status_and_pagination() {
    let temp = Temp::new();
    let app = app(&temp);
    for body in [
        json!(null),
        json!({"query": null}),
        json!({"status": null}),
        json!({"query": 3}),
        json!({"status": false}),
        json!({"status": "unknown"}),
        json!({"offset": -1}),
        serde_json::from_str::<Value>("{\"offset\":18446744073709551616}").unwrap(),
        json!({"limit": 0}),
        json!({"limit": 101}),
        json!({"limit": "10"}),
    ] {
        assert!(app.dispatch("POST", "/api/v2/runs/query", body).is_err());
    }
}

#[test]
fn remove_protects_locked_runs_and_removes_terminal_runs() {
    let temp = Temp::new();
    let app = app(&temp);
    let workspace = temp.0.join("workspace");
    summary(&workspace, "run-live", "Live", "demo", "running", 2);
    summary(&workspace, "run-done", "Done", "demo", "succeeded", 1);
    std::fs::write(
        workspace.join(".nctool/runs/run-done.json"),
        serde_json::to_vec(&json!({
            "id":"run-done", "title":"Done", "action":"demo", "input":{},
            "status":"succeeded", "started_at":1, "finished_at":2,
            "result":null, "error":null, "plugins":[]
        }))
        .unwrap(),
    )
    .unwrap();

    let lock_path = workspace.join(".nctool/runs/run-live.nctool.lock");
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_path)
        .unwrap();
    lock.lock().unwrap();
    let error = app
        .dispatch("POST", "/api/v2/runs/run-live/remove", json!({}))
        .unwrap_err();
    assert_eq!(error.code, "conflict");
    drop(lock);
    assert!(index(&workspace).join("run-live.json").exists());

    assert_eq!(
        app.dispatch("POST", "/api/v2/runs/run-done/remove", json!({}))
            .unwrap()["removed"],
        "run-done"
    );
    assert!(!index(&workspace).join("run-done.json").exists());
}
