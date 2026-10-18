//! Independent measurement harness for the workspace history read path
//! (`Workbench::list` / `Workbench::query`).
//!
//! This is **not** part of the normal test suite. It is a profiling harness that is
//! only useful when driven by `scripts/measure_history.py`, which creates an isolated
//! fixture workspace, sets the environment variables below and records the printed
//! JSON under `target/agent-preview/optimization/`. Every test is `#[ignore]`, so
//! `cargo test --all-targets` compiles but never runs it.
//!
//! Environment:
//!   NCTOOL_HISTORY_WORKSPACE   workspace dir containing `.nctool/runs/_index/*.json`
//!   NCTOOL_HISTORY_HOME        home dir for `App::boot` (default: sibling `home`)
//!   NCTOOL_HISTORY_SIZE        fixture size, echoed back for the report
//!   NCTOOL_HISTORY_ITERATIONS  measured iterations per operation (default 20)
//!   NCTOOL_HISTORY_LABEL       label, echoed back
//!
//! The harness never writes to the workspace: it only dispatches the read endpoints
//! and, for the isolation samples, re-reads the index directory the same way
//! `Workbench::summaries` does. The isolation samples compare the *current* list/query
//! core with the *proposed* partial-selection/reordered core in the same process, so a
//! before/after comparison does not need any production change.
use nctool_cli::composition::App;
use nctool_plugin_sdk::Value;
use serde_json::json;
use std::{
    hint::black_box,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn env_path(name: &str, default: PathBuf) -> PathBuf {
    std::env::var(name).map(PathBuf::from).unwrap_or(default)
}

fn median(mut samples: Vec<f64>) -> f64 {
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = samples.len();
    if n == 0 {
        return 0.0;
    }
    if n % 2 == 1 {
        samples[n / 2]
    } else {
        (samples[n / 2 - 1] + samples[n / 2]) / 2.0
    }
}

/// Replicates `Workbench::summaries` read+parse: read every `_index/*.json` and parse
/// it, without sorting or filtering. This is the dominant cost of `list`/`query`.
fn read_parse_index(dir: &Path) -> Vec<Value> {
    let mut summaries = vec![];
    for entry in std::fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_file()
            && entry.path().extension().is_some_and(|e| e == "json")
        {
            let bytes = std::fs::read(entry.path()).unwrap();
            summaries.push(nctool_plugin_sdk::parse_json(&bytes).unwrap());
        }
    }
    summaries
}

fn started_at(value: &Value) -> u64 {
    value["started_at"].as_u64().unwrap_or(0)
}

/// Deterministic total order used by `query`: `started_at` desc, then `id` asc.
fn sort_full(summaries: &mut [Value]) {
    summaries.sort_by(|a, b| {
        started_at(b).cmp(&started_at(a)).then_with(|| {
            a["id"]
                .as_str()
                .unwrap_or_default()
                .cmp(b["id"].as_str().unwrap_or_default())
        })
    });
}

/// Current `list` core: full sort, then take 100.
fn list_current(mut summaries: Vec<Value>) -> usize {
    summaries.sort_by_key(|r| std::cmp::Reverse(started_at(r)));
    summaries.into_iter().take(100).count()
}

/// Proposed `list` core: partial top-100 selection, then sort only those 100.
fn list_proposed(mut summaries: Vec<Value>) -> usize {
    let take = summaries.len().min(100);
    if take > 0 && take < summaries.len() {
        summaries.select_nth_unstable_by(take - 1, |a, b| started_at(b).cmp(&started_at(a)));
        summaries.truncate(take);
    }
    summaries.sort_by_key(|r| std::cmp::Reverse(started_at(r)));
    summaries.len()
}

/// Current `query` core: full sort, then filter by status.
fn query_current(mut summaries: Vec<Value>) -> usize {
    sort_full(&mut summaries);
    summaries.retain(|r| r["status"].as_str() == Some("succeeded"));
    summaries.len()
}

/// Proposed `query` core: filter first, then sort only the matches.
fn query_proposed(mut summaries: Vec<Value>) -> usize {
    summaries.retain(|r| r["status"].as_str() == Some("succeeded"));
    sort_full(&mut summaries);
    summaries.len()
}

/// Run `op` `iterations` times; return (median_ns, min_ns, max runs).
fn measure_count<F: FnMut() -> usize>(iterations: usize, mut op: F) -> (f64, f64, u64) {
    black_box(op());
    let mut samples = Vec::with_capacity(iterations);
    let mut runs = 0;
    for _ in 0..iterations {
        let start = Instant::now();
        let count = black_box(op());
        let elapsed: Duration = start.elapsed();
        runs = runs.max(count as u64);
        samples.push(elapsed.as_nanos() as f64);
    }
    let min = samples.iter().cloned().fold(f64::INFINITY, f64::min);
    (median(samples), min, runs)
}

/// Dispatch variant: the returned payload's `runs` length is the checksum.
fn measure_dispatch<F: FnMut() -> Value>(iterations: usize, mut op: F) -> (f64, f64, u64) {
    black_box(op());
    let mut samples = Vec::with_capacity(iterations);
    let mut runs = 0;
    for _ in 0..iterations {
        let start = Instant::now();
        let value = black_box(op());
        let elapsed: Duration = start.elapsed();
        runs = runs.max(
            value["runs"]
                .as_array()
                .map(|runs| runs.len() as u64)
                .unwrap_or(0),
        );
        samples.push(elapsed.as_nanos() as f64);
    }
    let min = samples.iter().cloned().fold(f64::INFINITY, f64::min);
    (median(samples), min, runs)
}

fn metric(median_ns: f64, min_ns: f64, runs: u64) -> Value {
    json!({"median_ns": median_ns, "min_ns": min_ns, "runs": runs})
}

#[test]
#[ignore = "profiling harness, driven by scripts/measure_history.py"]
fn measure_history_read_query() {
    let workspace = PathBuf::from(
        std::env::var("NCTOOL_HISTORY_WORKSPACE")
            .expect("NCTOOL_HISTORY_WORKSPACE must point at the fixture workspace"),
    );
    let home = env_path(
        "NCTOOL_HISTORY_HOME",
        workspace.parent().unwrap_or(&workspace).join("home"),
    );
    let size = env_usize("NCTOOL_HISTORY_SIZE", 0);
    let iterations = env_usize("NCTOOL_HISTORY_ITERATIONS", 20).max(1);
    let label = std::env::var("NCTOOL_HISTORY_LABEL").unwrap_or_else(|_| "unlabeled".into());

    let app = App::boot(&home, &workspace, None).expect("fixture app boot");
    let index_dir = workspace.join(".nctool").join("runs").join("_index");

    // Isolation samples: index I/O + parse alone, then the current vs proposed core.
    // Both cores read the same fixture, so the delta isolates the sort/selection work.
    let read_parse = measure_count(iterations, || read_parse_index(&index_dir).len());
    let list_cur = measure_count(iterations, || list_current(read_parse_index(&index_dir)));
    let list_new = measure_count(iterations, || list_proposed(read_parse_index(&index_dir)));
    let query_cur = measure_count(iterations, || query_current(read_parse_index(&index_dir)));
    let query_new = measure_count(iterations, || query_proposed(read_parse_index(&index_dir)));

    let list = measure_dispatch(iterations, || {
        app.dispatch("GET", "/api/v2/runs", json!({})).unwrap()
    });
    let query_default = measure_dispatch(iterations, || {
        app.dispatch("POST", "/api/v2/runs/query", json!({}))
            .unwrap()
    });
    let query_status = measure_dispatch(iterations, || {
        app.dispatch("POST", "/api/v2/runs/query", json!({"status": "succeeded"}))
            .unwrap()
    });
    let query_text = measure_dispatch(iterations, || {
        app.dispatch("POST", "/api/v2/runs/query", json!({"query": "alpha"}))
            .unwrap()
    });
    let query_page = measure_dispatch(iterations, || {
        app.dispatch(
            "POST",
            "/api/v2/runs/query",
            json!({"offset": 100, "limit": 100}),
        )
        .unwrap()
    });

    println!(
        "HISTORY_METRIC_JSON {}",
        serde_json::to_string(&json!({
            "label": label,
            "size": size,
            "iterations": iterations,
            "isolation": {
                "read_parse": metric(read_parse.0, read_parse.1, read_parse.2),
                "list_current": metric(list_cur.0, list_cur.1, list_cur.2),
                "list_proposed": metric(list_new.0, list_new.1, list_new.2),
                "query_current": metric(query_cur.0, query_cur.1, query_cur.2),
                "query_proposed": metric(query_new.0, query_new.1, query_new.2)
            },
            "operations": {
                "list": metric(list.0, list.1, list.2),
                "query_default": metric(query_default.0, query_default.1, query_default.2),
                "query_status": metric(query_status.0, query_status.1, query_status.2),
                "query_text": metric(query_text.0, query_text.1, query_text.2),
                "query_page": metric(query_page.0, query_page.1, query_page.2)
            }
        }))
        .unwrap()
    );
}
