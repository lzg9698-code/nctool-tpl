use nctool_plugin_sdk::*;
use nctool_runtime::Runtime;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
struct Probe {
    descriptor: PluginDescriptor,
    actions: Vec<ActionDescriptor>,
    log: Arc<Mutex<Vec<String>>>,
    fail: bool,
}
impl Probe {
    fn new(id: &str, log: &Arc<Mutex<Vec<String>>>) -> Self {
        let mut descriptor = PluginDescriptor::builtin(id);
        let action = format!("{id}.echo");
        descriptor.provides.push(Service::new(&action, &action));
        Self {
            descriptor,
            actions: vec![ActionDescriptor::new(&action, "echo", object_schema())],
            log: log.clone(),
            fail: false,
        }
    }
}
impl Plugin for Probe {
    fn descriptor(&self) -> PluginDescriptor {
        self.descriptor.clone()
    }
    fn actions(&self) -> Vec<ActionDescriptor> {
        self.actions.clone()
    }
    fn activate(&self, _: &Context) -> PluginResult<()> {
        self.log
            .lock()
            .unwrap()
            .push(format!("start:{}", self.descriptor.id));
        if self.fail {
            Err(PluginError::new("activation", "failed"))
        } else {
            Ok(())
        }
    }
    fn shutdown(&self) {
        self.log
            .lock()
            .unwrap()
            .push(format!("stop:{}", self.descriptor.id));
    }
    fn listeners(&self) -> Vec<EventListener> {
        let log = self.log.clone();
        vec![EventListener {
            kind: EventKind::ActionFinished,
            callback: Arc::new(move |event| {
                log.lock()
                    .unwrap()
                    .push(format!("event:{}:{:?}", event.action, event.succeeded))
            }),
        }]
    }
    fn invoke(&self, _: &str, input: Value, _: &Context) -> PluginResult<ActionResult> {
        Ok(ActionResult::data(input))
    }
}
fn boot(plugins: Vec<Arc<dyn Plugin>>) -> PluginResult<Arc<Runtime>> {
    Runtime::start(plugins, BTreeMap::new(), BTreeMap::new())
}
#[test]
fn dependency_order_reverse_disposal_and_scoped_events() {
    let log = Arc::new(Mutex::new(vec![]));
    let a = Probe::new("a", &log);
    let mut b = Probe::new("b", &log);
    b.descriptor.requires.push(Requirement::new("a.echo"));
    let runtime = boot(vec![Arc::new(b), Arc::new(a)]).unwrap();
    runtime.invoke("a.echo", json!({"value":7})).unwrap();
    runtime.shutdown();
    let log = log.lock().unwrap();
    assert_eq!(&log[..2], ["start:a", "start:b"]);
    assert_eq!(&log[log.len() - 2..], ["stop:b", "stop:a"]);
    assert!(log.iter().any(|s| s == "event:a.echo:Some(true)"));
    assert!(runtime.capabilities().is_empty());
    assert!(runtime.descriptors().is_empty());
}
#[test]
fn failed_activation_rolls_back_predecessors() {
    let log = Arc::new(Mutex::new(vec![]));
    let a = Probe::new("a", &log);
    let mut b = Probe::new("b", &log);
    b.descriptor.requires.push(Requirement::new("a.echo"));
    b.fail = true;
    assert!(boot(vec![Arc::new(a), Arc::new(b)]).is_err());
    assert_eq!(
        *log.lock().unwrap(),
        vec!["start:a", "start:b", "stop:b", "stop:a"]
    );
}
#[test]
fn missing_versions_cycles_and_duplicate_services_fail_before_activation() {
    let log = Arc::new(Mutex::new(vec![]));
    let mut a = Probe::new("a", &log);
    a.descriptor.requires.push(Requirement::new("missing"));
    assert_eq!(
        boot(vec![Arc::new(a)]).err().unwrap().code,
        "service_missing"
    );
    let a = Probe::new("a", &log);
    let mut b = Probe::new("b", &log);
    b.descriptor.requires.push(Requirement {
        id: "a.echo".into(),
        version: 2,
    });
    assert_eq!(
        boot(vec![Arc::new(a), Arc::new(b)]).err().unwrap().code,
        "service_version"
    );
    let mut a = Probe::new("a", &log);
    let mut b = Probe::new("b", &log);
    a.descriptor.requires.push(Requirement::new("b.echo"));
    b.descriptor.requires.push(Requirement::new("a.echo"));
    assert_eq!(
        boot(vec![Arc::new(a), Arc::new(b)]).err().unwrap().code,
        "dependency_cycle"
    );
    let a = Probe::new("a", &log);
    let mut b = Probe::new("b", &log);
    b.descriptor.provides[0].id = "a.echo".into();
    assert_eq!(
        boot(vec![Arc::new(a), Arc::new(b)]).err().unwrap().code,
        "duplicate_service"
    );
    assert!(log.lock().unwrap().is_empty());
}
#[test]
fn schema_validation_and_protocol_version_are_enforced() {
    let log = Arc::new(Mutex::new(vec![]));
    let mut a = Probe::new("a", &log);
    a.actions[0].input_schema = json!({"type":"object","required":["integer"],"properties":{"integer":{"type":"integer"}},"additionalProperties":false});
    let runtime = boot(vec![Arc::new(a)]).unwrap();
    assert_eq!(
        runtime
            .invoke("a.echo", json!({"integer":1.5}))
            .unwrap_err()
            .code,
        "invalid_input"
    );
    assert!(runtime.invoke("a.echo", json!({"integer":8})).is_ok());
    let mut a = Probe::new("a", &log);
    a.descriptor.protocol_version = 2;
    assert_eq!(
        boot(vec![Arc::new(a)]).err().unwrap().code,
        "protocol_version"
    );
}
struct Replacement;
impl Plugin for Replacement {
    fn descriptor(&self) -> PluginDescriptor {
        let mut d = PluginDescriptor::builtin("replacement");
        d.provides = vec![Service::new("text.render", "replacement.generate")];
        d
    }
    fn actions(&self) -> Vec<ActionDescriptor> {
        vec![ActionDescriptor::new(
            "replacement.generate",
            "replace",
            object_schema(),
        )]
    }
    fn invoke(&self, _: &str, input: Value, _: &Context) -> PluginResult<ActionResult> {
        let cursor = input["options"]["line_number_start"].as_u64().unwrap_or(0) + 10;
        Ok(ActionResult::data(
            json!({"text":format!("N{cursor:04} {}",input["params"]["value"]),"end_line_number":cursor}),
        ))
    }
}
/// Generic service consumer: depends only on the versioned `text.render` service,
/// which any provider can satisfy. This is the mechanism external plugins use;
/// it is not tied to any removed domain implementation.
struct Consumer;
impl Plugin for Consumer {
    fn descriptor(&self) -> PluginDescriptor {
        let mut d = PluginDescriptor::builtin("consumer");
        d.requires.push(Requirement::new("text.render"));
        d
    }
    fn actions(&self) -> Vec<ActionDescriptor> {
        vec![ActionDescriptor::new(
            "consumer.generate",
            "consume",
            object_schema(),
        )]
    }
    fn invoke(&self, _: &str, input: Value, ctx: &Context) -> PluginResult<ActionResult> {
        let options = input
            .get("options")
            .cloned()
            .unwrap_or_else(|| json!({"line_number_start": 0}));
        let mut cursor = options["line_number_start"].as_u64().unwrap_or(0);
        let mut lines = String::new();
        for op in input["ops"].as_array().cloned().unwrap_or_default() {
            let result = ctx.call(
                "text.render",
                1,
                json!({"params": op["params"], "options": {"line_number_start": cursor}}),
            )?;
            cursor = result.data["end_line_number"].as_u64().unwrap_or(cursor);
            lines.push_str(result.data["text"].as_str().unwrap_or_default());
            lines.push('\n');
        }
        Ok(ActionResult::data(
            json!({"text": lines.trim_end(), "end_line_number": cursor}),
        ))
    }
}
#[test]
fn service_consumer_works_with_an_unrelated_provider() {
    let runtime = boot(vec![Arc::new(Consumer), Arc::new(Replacement)]).unwrap();
    let output = runtime
        .invoke(
            "consumer.generate",
            json!({"ops":[{"params":{"value":1}},{"params":{"value":2}}]}),
        )
        .unwrap();
    assert_eq!(output.data["text"], "N0010 1\nN0020 2");
    assert_eq!(output.data["end_line_number"], 20);
}
fn duplicate_provider(binding: BTreeMap<String, String>) -> PluginResult<Arc<Runtime>> {
    let log = Arc::new(Mutex::new(vec![]));
    let mut first = Probe::new("first", &log);
    first.descriptor.provides[0].id = "text.render".into();
    Runtime::start(
        vec![Arc::new(first), Arc::new(Replacement), Arc::new(Consumer)],
        BTreeMap::new(),
        binding,
    )
}
#[test]
fn duplicate_provider_requires_explicit_binding() {
    // Two providers for the same service must not resolve without a choice.
    assert_eq!(
        duplicate_provider(BTreeMap::new()).err().unwrap().code,
        "duplicate_service"
    );
    // The explicit binding selects the replacement, so the consumer routes to it.
    let runtime = duplicate_provider(
        [("text.render".to_string(), "replacement".to_string())]
            .into_iter()
            .collect(),
    )
    .unwrap();
    assert_eq!(
        runtime
            .invoke("consumer.generate", json!({"ops":[{"params":{"value":9}}]}))
            .unwrap()
            .data["text"],
        "N0010 9"
    );
}

struct Cleanup {
    context: Mutex<Option<Context>>,
    log: Arc<Mutex<Vec<String>>>,
}
impl Plugin for Cleanup {
    fn descriptor(&self) -> PluginDescriptor {
        let mut d = PluginDescriptor::builtin("cleanup");
        d.requires.push(Requirement::new("a.echo"));
        d
    }
    fn activate(&self, ctx: &Context) -> PluginResult<()> {
        *self.context.lock().unwrap() = Some(ctx.clone());
        Ok(())
    }
    fn shutdown(&self) {
        let result = self
            .context
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .call("a.echo", 1, json!({}));
        self.log
            .lock()
            .unwrap()
            .push(format!("cleanup:{}", result.is_ok()));
    }
}
#[test]
fn dependencies_remain_available_during_consumer_disposal() {
    let log = Arc::new(Mutex::new(vec![]));
    let runtime = boot(vec![
        Arc::new(Probe::new("a", &log)),
        Arc::new(Cleanup {
            context: Mutex::new(None),
            log: log.clone(),
        }),
    ])
    .unwrap();
    runtime.shutdown();
    assert!(log.lock().unwrap().iter().any(|s| s == "cleanup:true"));
    assert_eq!(
        runtime.invoke("a.echo", json!({})).unwrap_err().code,
        "closed"
    );
}

#[test]
fn failed_registration_cleans_its_scope_and_predecessors() {
    let log = Arc::new(Mutex::new(vec![]));
    let a = Probe::new("a", &log);
    let mut b = Probe::new("b", &log);
    b.descriptor.requires.push(Requirement::new("a.echo"));
    b.actions[0].id = "a.echo".into();
    b.descriptor.provides[0].action = "a.echo".into();
    assert_eq!(
        boot(vec![Arc::new(a), Arc::new(b)]).err().unwrap().code,
        "duplicate_action"
    );
    assert_eq!(
        *log.lock().unwrap(),
        vec!["start:a", "start:b", "stop:b", "stop:a"]
    );
}

struct Static {
    descriptor: PluginDescriptor,
    actions: Vec<ActionDescriptor>,
}
impl Plugin for Static {
    fn descriptor(&self) -> PluginDescriptor {
        self.descriptor.clone()
    }
    fn actions(&self) -> Vec<ActionDescriptor> {
        self.actions.clone()
    }
    fn invoke(&self, _: &str, input: Value, _: &Context) -> PluginResult<ActionResult> {
        Ok(ActionResult::data(input))
    }
}
fn static_plugin(descriptor: PluginDescriptor, actions: Vec<ActionDescriptor>) -> Arc<dyn Plugin> {
    Arc::new(Static {
        descriptor,
        actions,
    })
}

#[test]
fn retired_builtin_identities_cannot_be_reclaimed_by_another_plugin() {
    // The retired plugin id itself stays reserved.
    assert_eq!(
        boot(vec![static_plugin(PluginDescriptor::builtin("nc"), vec![])])
            .err()
            .unwrap()
            .code,
        "retired_plugin"
    );
    // An unrelated plugin id may not supply a retired action.
    let actions = vec![ActionDescriptor::new("nc.generate", "NC", object_schema())];
    assert_eq!(
        boot(vec![static_plugin(
            PluginDescriptor::builtin("alias"),
            actions
        )])
        .err()
        .unwrap()
        .code,
        "retired_identity"
    );
    // ... nor declare a retired service.
    let mut descriptor = PluginDescriptor::builtin("alias");
    descriptor.provides = vec![Service::new("sync-turning.generate", "alias.generate")];
    let actions = vec![ActionDescriptor::new(
        "alias.generate",
        "generate",
        object_schema(),
    )];
    assert_eq!(
        boot(vec![static_plugin(descriptor, actions)])
            .err()
            .unwrap()
            .code,
        "retired_identity"
    );
    // ... nor occupy a retired asset collection.
    let mut descriptor = PluginDescriptor::builtin("alias");
    descriptor.asset_collections = vec![AssetCollection {
        id: "sync-turning-assets".into(),
        export_action: "alias.export".into(),
        import_action: "alias.import".into(),
    }];
    let actions = vec![
        ActionDescriptor::new("alias.export", "export", object_schema()),
        ActionDescriptor::new("alias.import", "import", object_schema()),
    ];
    assert_eq!(
        boot(vec![static_plugin(descriptor, actions)])
            .err()
            .unwrap()
            .code,
        "retired_identity"
    );
    // Similar-looking names are never reclassified: only the exact tokens and their
    // dotted namespaces are reserved.
    let mut descriptor = PluginDescriptor::builtin("machine-learning");
    descriptor.provides = vec![Service::new("nctool.ready", "machine-learning.run")];
    let actions = vec![
        ActionDescriptor::new("machine-learning.run", "run", object_schema()),
        ActionDescriptor::new("processing.generate", "process", object_schema()),
    ];
    let runtime = boot(vec![static_plugin(descriptor, actions)]).unwrap();
    assert!(runtime.invoke("processing.generate", json!({})).is_ok());
}
