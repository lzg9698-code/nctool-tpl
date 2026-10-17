//! Regressions for the template plugin's source-snapshot lock order and for the
//! inline-root sentinel. See `target/optimization-20261017/template-parameters.log`.
//!
//! These tests drive the plugin directly through the `Plugin` trait with a minimal
//! host, so they stay deterministic and free of `App`/HTTP setup.
use nctool_plugin_sdk::*;
use nctool_plugin_template::TemplatePlugin;
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

/// Minimal host: rendering never needs a host callback here, but `render` always
/// calls `ctx.extend` (with an empty extension list), so that must succeed.
struct NoHost;
impl Host for NoHost {
    fn call(
        &self,
        owner: &str,
        service: &str,
        version: u32,
        _input: Value,
        _cancellation: Cancellation,
        _stack: &[String],
    ) -> PluginResult<ActionResult> {
        Err(PluginError::new(
            "unsupported",
            format!("{owner} {service}@{version}"),
        ))
    }
    fn extend(&self, _renderer: &mut Renderer, _extensions: &[String]) -> PluginResult<()> {
        Ok(())
    }
}
fn ctx(owner: &str) -> Context {
    Context {
        owner: owner.into(),
        config: json!({}),
        cancellation: Cancellation::default(),
        stack: vec![],
        host: Arc::new(NoHost),
    }
}
/// Parallel tests can read the same clock tick, so pair the timestamp with a
/// counter; otherwise one fixture's cleanup may remove another's directory.
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "nctool-tpl-lock-{}-{}-{}",
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
fn plugin(dir: &Path) -> TemplatePlugin {
    TemplatePlugin::new(dir).unwrap()
}

// ─────────────────────────── inline-root sentinel (P2) ───────────────────────────

/// A plain source draft must not adopt a saved template that merely shares the
/// inline sentinel name `__inline__`, including its domain-intent metadata.
#[test]
fn inline_source_draft_ignores_a_saved_sentinel_with_domain_intent() {
    let temp = Temp::new();
    let plugin = plugin(temp.path());
    plugin
        .invoke(
            "template.save",
            json!({
                "name": "__inline__",
                "asset": { "source": "SEALED", "metadata": { "nc": {} } }
            }),
            &ctx("template"),
        )
        .unwrap();
    let rendered = plugin
        .invoke(
            "template.render",
            json!({ "source": "Hello {{ name }}", "context": { "name": "Ada" } }),
            &ctx("template"),
        )
        .unwrap();
    assert_eq!(rendered.data["text"], "Hello Ada");
}

/// Same collision, but the sentinel asset only carries a schema the draft cannot
/// satisfy: the draft must not be validated against a foreign root asset.
#[test]
fn inline_source_draft_does_not_adopt_a_saved_sentinel_schema() {
    let temp = Temp::new();
    let plugin = plugin(temp.path());
    plugin
        .invoke(
            "template.save",
            json!({
                "name": "__inline__",
                "asset": {
                    "source": "SEALED",
                    "schema": {
                        "type": "object",
                        "properties": { "name": { "type": "integer" } },
                        "required": ["name"]
                    }
                }
            }),
            &ctx("template"),
        )
        .unwrap();
    let rendered = plugin
        .invoke(
            "template.render",
            json!({ "source": "Hello {{ name }}", "context": { "name": "Ada" } }),
            &ctx("template"),
        )
        .unwrap();
    assert_eq!(rendered.data["text"], "Hello Ada");
}

/// An explicitly selected template keeps its frozen metadata, so an edit draft
/// (`template` + `source`) can never bypass the domain-intent refusal.
#[test]
fn explicit_template_edit_draft_cannot_bypass_the_domain_refusal() {
    let temp = Temp::new();
    let plugin = plugin(temp.path());
    plugin
        .invoke(
            "template.save",
            json!({
                "name": "sealed",
                "asset": { "source": "SEALED", "metadata": { "execution": { "action": "nc.generate" } } }
            }),
            &ctx("template"),
        )
        .unwrap();
    for input in [
        json!({ "template": "sealed" }),
        json!({ "template": "sealed", "source": "draft {{ x }}" }),
        json!({ "template": "sealed", "source": "draft", "context": {} }),
    ] {
        let error = plugin
            .invoke("template.render", input.clone(), &ctx("template"))
            .unwrap_err();
        assert_eq!(error.code, "domain_action_required", "input {input}");
    }
}

// ─────────────────────── source-snapshot lock order (P1) ───────────────────────

/// `template.inspect` snapshots its sources itself, so `invoke` must not take the
/// workspace group lock as well: a same-thread re-entry of that cross-process lock
/// would spin to its 2 s deadline and fail.
#[test]
fn inspect_does_not_reenter_the_workspace_group_lock() {
    let temp = Temp::new();
    let plugin = plugin(temp.path());
    plugin
        .invoke(
            "template.save",
            json!({ "name": "base", "asset": { "source": "BASE" } }),
            &ctx("template"),
        )
        .unwrap();
    let started = Instant::now();
    let inspected = plugin
        .invoke(
            "template.inspect",
            json!({ "template": "base" }),
            &ctx("template"),
        )
        .unwrap();
    assert_eq!(inspected.data["template"], "base");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "inspect waited on a re-entered group lock: {:?}",
        started.elapsed()
    );
}

/// Concurrent `template.inspect` (accessed first, group lock inside `sources()`)
/// and `template.save` (group lock for its whole body) must share one lock order.
/// With inverted orders the inspector's group-lock wait expires and it surfaces a
/// spurious `conflict`.
#[test]
fn concurrent_inspect_and_save_never_report_a_lock_ordering_conflict() {
    let temp = Temp::new();
    let plugin = Arc::new(plugin(temp.path()));
    plugin
        .invoke(
            "template.save",
            json!({ "name": "base", "asset": { "source": "BASE" } }),
            &ctx("template"),
        )
        .unwrap();
    // A large source keeps each save inside the workspace group lock long enough to
    // overlap the inspector's own group-lock phase.
    let big = "{{ name }}\n".repeat(20_000);
    let stop = Arc::new(AtomicBool::new(false));
    let inspector = {
        let worker = plugin.clone();
        let stop = stop.clone();
        thread::spawn(move || {
            let mut conflicts = 0usize;
            while !stop.load(Ordering::Relaxed) {
                match worker.invoke(
                    "template.inspect",
                    json!({ "template": "base" }),
                    &ctx("template"),
                ) {
                    Ok(_) => {}
                    Err(error) if error.code == "conflict" => conflicts += 1,
                    Err(error) => panic!("unexpected inspect error: {error}"),
                }
            }
            conflicts
        })
    };
    for index in 0..8 {
        plugin
            .invoke(
                "template.save",
                json!({ "name": format!("writer-{index}"), "asset": { "source": big } }),
                &ctx("template"),
            )
            .unwrap();
    }
    stop.store(true, Ordering::Relaxed);
    let conflicts = inspector.join().unwrap();
    assert_eq!(conflicts, 0, "inspect surfaced a lock-ordering conflict");
}
