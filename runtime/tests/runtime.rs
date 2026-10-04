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
        d.provides = vec![Service::new("nc.generate", "replacement.generate")];
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
#[test]
fn process_consumer_works_with_an_unrelated_provider() {
    let runtime = boot(vec![
        Arc::new(nctool_plugin_process::ProcessPlugin),
        Arc::new(Replacement),
    ])
    .unwrap();
    let output=runtime.invoke("process.generate",json!({"params":{"value":1},"ops":[{"source":"unused"},{"source":"unused","params":{"value":2}}]})).unwrap();
    assert_eq!(output.data["text"], "N0010 1\nN0020 2\n");
    assert_eq!(output.data["end_line_number"], 20);
}
#[test]
fn duplicate_provider_requires_explicit_binding() {
    let log = Arc::new(Mutex::new(vec![]));
    let mut first = Probe::new("first", &log);
    first.descriptor.provides[0].id = "nc.generate".into();
    let runtime = Runtime::start(
        vec![
            Arc::new(first),
            Arc::new(Replacement),
            Arc::new(nctool_plugin_process::ProcessPlugin),
        ],
        BTreeMap::new(),
        [("nc.generate".into(), "replacement".into())]
            .into_iter()
            .collect(),
    )
    .unwrap();
    assert_eq!(runtime.bindings()["nc.generate"][2], "replacement");
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
