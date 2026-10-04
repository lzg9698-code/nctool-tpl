//! Dependency-driven plugin composition and domain-neutral action dispatch.
pub mod config;
pub mod external;
use nctool_plugin_sdk::*;
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, Weak,
    },
};

struct RegisteredAction {
    owner: String,
    plugin: Arc<dyn Plugin>,
    descriptor: ActionDescriptor,
    input: jsonschema::Validator,
    output: jsonschema::Validator,
}
#[derive(Default)]
struct State {
    descriptors: BTreeMap<String, PluginDescriptor>,
    actions: BTreeMap<String, Arc<RegisteredAction>>,
    services: BTreeMap<String, (u32, String, String)>,
    extensions: BTreeMap<String, (String, RenderExtension)>,
    active: Vec<Arc<dyn Plugin>>,
    listeners: Vec<(String, EventListener)>,
}
#[derive(Serialize)]
pub struct Capability {
    pub plugin: String,
    #[serde(flatten)]
    pub action: ActionDescriptor,
}
pub struct Runtime {
    state: Mutex<State>,
    configs: BTreeMap<String, Value>,
    requests: Mutex<BTreeMap<String, Cancellation>>,
    closing: AtomicBool,
}
struct Bridge(Weak<Runtime>);
impl Host for Bridge {
    fn call(
        &self,
        owner: &str,
        service: &str,
        version: u32,
        input: Value,
        cancellation: Cancellation,
        stack: &[String],
    ) -> PluginResult<ActionResult> {
        let runtime = self
            .0
            .upgrade()
            .ok_or_else(|| PluginError::new("closed", "runtime closed"))?;
        let action = {
            let state = runtime.state.lock().unwrap();
            let descriptor = state
                .descriptors
                .get(owner)
                .ok_or_else(|| PluginError::new("inactive", owner))?;
            if !descriptor
                .requires
                .iter()
                .any(|r| r.id == service && r.version == version)
            {
                return Err(PluginError::new(
                    "undeclared_dependency",
                    format!("{owner} did not declare {service}@{version}"),
                ));
            }
            let (v, action, _) = state
                .services
                .get(service)
                .ok_or_else(|| PluginError::new("service_missing", service))?;
            if *v != version {
                return Err(PluginError::new("service_version", service));
            }
            action.clone()
        };
        runtime.invoke_inner(&action, input, cancellation, stack)
    }
    fn extend(&self, renderer: &mut Renderer, extensions: &[String]) -> PluginResult<()> {
        let runtime = self
            .0
            .upgrade()
            .ok_or_else(|| PluginError::new("closed", "runtime closed"))?;
        let callbacks = {
            let state = runtime.state.lock().unwrap();
            let mut seen = BTreeSet::new();
            let mut callbacks = vec![];
            for id in extensions {
                if !seen.insert(id) {
                    return Err(PluginError::new("duplicate_extension", id));
                }
                let (_, ext) = state
                    .extensions
                    .get(id)
                    .ok_or_else(|| PluginError::new("extension_missing", id))?;
                callbacks.push(ext.configure.clone());
            }
            callbacks
        };
        for configure in callbacks {
            configure(renderer)?;
        }
        Ok(())
    }
}
impl Runtime {
    pub fn start(
        plugins: Vec<Arc<dyn Plugin>>,
        configs: BTreeMap<String, Value>,
        providers: BTreeMap<String, String>,
    ) -> PluginResult<Arc<Self>> {
        let runtime = Arc::new(Self {
            state: Mutex::new(State::default()),
            configs,
            requests: Mutex::new(BTreeMap::new()),
            closing: AtomicBool::new(false),
        });
        let mut available = BTreeMap::new();
        for plugin in plugins {
            let descriptor = plugin.descriptor();
            validate_descriptor(&descriptor, &plugin.actions())?;
            if available.insert(descriptor.id.clone(), plugin).is_some() {
                return Err(PluginError::new("duplicate_plugin", descriptor.id));
            }
        }
        let definitions = available
            .values()
            .map(|plugin| plugin.descriptor())
            .collect::<Vec<_>>();
        let plan = resolve_composition(&definitions, &providers)?;
        let selected = plan.providers;
        for id in plan.order {
            let plugin = available.remove(&id).unwrap();
            let descriptor = plugin.descriptor();
            let cfg = runtime
                .configs
                .get(&id)
                .cloned()
                .unwrap_or_else(|| json!({}));
            validate_schema(&descriptor.config_schema, &cfg, "invalid_config")?;
            let context = runtime.context(&id, Cancellation::default(), vec![]);
            // Publish the descriptor so declared dependency calls work during activation.
            runtime
                .state
                .lock()
                .unwrap()
                .descriptors
                .insert(id.clone(), descriptor.clone());
            if let Err(e) = plugin.activate(&context) {
                plugin.shutdown();
                runtime.shutdown();
                return Err(e);
            }
            let registration = (|| -> PluginResult<()> {
                let mut state = runtime.state.lock().unwrap();
                for action in plugin.actions() {
                    if state.actions.contains_key(&action.id) {
                        return Err(PluginError::new("duplicate_action", action.id));
                    }
                    let input = compile_schema(&action.input_schema)?;
                    let output = compile_schema(&action.output_schema)?;
                    state.actions.insert(
                        action.id.clone(),
                        Arc::new(RegisteredAction {
                            owner: id.clone(),
                            plugin: plugin.clone(),
                            descriptor: action,
                            input,
                            output,
                        }),
                    );
                }
                for extension in plugin.extensions() {
                    if state.extensions.contains_key(&extension.id) {
                        return Err(PluginError::new("duplicate_extension", extension.id));
                    }
                    // Registration failures, including duplicate builtin filters, fail startup.
                    let mut probe = Renderer::new();
                    (extension.configure)(&mut probe)?;
                    state
                        .extensions
                        .insert(extension.id.clone(), (id.clone(), extension));
                }
                for service in descriptor.provides {
                    if selected[&service.id] == id {
                        state
                            .services
                            .insert(service.id, (service.version, service.action, id.clone()));
                    }
                }
                state.listeners.extend(
                    plugin
                        .listeners()
                        .into_iter()
                        .map(|listener| (id.clone(), listener)),
                );
                state.active.push(plugin.clone());
                Ok(())
            })();
            if let Err(e) = registration {
                plugin.shutdown();
                runtime.shutdown();
                return Err(e);
            }
        }
        Ok(runtime)
    }
    fn context(
        self: &Arc<Self>,
        owner: &str,
        cancellation: Cancellation,
        stack: Vec<String>,
    ) -> Context {
        Context {
            owner: owner.into(),
            config: self
                .configs
                .get(owner)
                .cloned()
                .unwrap_or_else(|| json!({})),
            cancellation,
            stack,
            host: Arc::new(Bridge(Arc::downgrade(self))),
        }
    }
    fn invoke_inner(
        self: &Arc<Self>,
        action: &str,
        input: Value,
        cancellation: Cancellation,
        stack: &[String],
    ) -> PluginResult<ActionResult> {
        cancellation.check()?;
        if stack.len() >= 32 || stack.iter().any(|a| a == action) {
            return Err(PluginError::new("call_cycle", action));
        }
        let registered = self
            .state
            .lock()
            .unwrap()
            .actions
            .get(action)
            .cloned()
            .ok_or_else(|| PluginError::new("action_not_found", action))?;
        registered
            .input
            .validate(&input)
            .map_err(|e| PluginError::new("invalid_input", e.to_string()))?;
        let mut stack = stack.to_vec();
        stack.push(action.into());
        self.emit(Event {
            kind: EventKind::ActionStarted,
            action: action.into(),
            owner: registered.owner.clone(),
            succeeded: None,
        });
        let result = (|| {
            let result = registered.plugin.invoke(
                action,
                input,
                &self.context(&registered.owner, cancellation.clone(), stack),
            )?;
            cancellation.check()?;
            registered
                .output
                .validate(&result.data)
                .map_err(|e| PluginError::new("invalid_output", e.to_string()))?;
            Ok(result)
        })();
        self.emit(Event {
            kind: EventKind::ActionFinished,
            action: action.into(),
            owner: registered.owner.clone(),
            succeeded: Some(result.is_ok()),
        });
        result
    }
    fn emit(&self, event: Event) {
        let callbacks: Vec<_> = self
            .state
            .lock()
            .unwrap()
            .listeners
            .iter()
            .filter(|(_, l)| l.kind == event.kind)
            .map(|(_, l)| l.callback.clone())
            .collect();
        for callback in callbacks {
            callback(&event);
        }
    }
    pub fn invoke(self: &Arc<Self>, action: &str, input: Value) -> PluginResult<ActionResult> {
        if self.closing.load(Ordering::SeqCst) {
            return Err(PluginError::new("closed", "runtime closed"));
        }
        self.invoke_inner(action, input, Cancellation::default(), &[])
    }
    pub fn invoke_request(
        self: &Arc<Self>,
        id: &str,
        action: &str,
        input: Value,
    ) -> PluginResult<ActionResult> {
        self.invoke_cancellable_request(id, action, input, Cancellation::default())
    }
    /// Dispatch using a caller-owned token, including cancellation before registration.
    pub fn invoke_cancellable_request(
        self: &Arc<Self>,
        id: &str,
        action: &str,
        input: Value,
        cancellation: Cancellation,
    ) -> PluginResult<ActionResult> {
        if self.closing.load(Ordering::SeqCst) {
            return Err(PluginError::new("closed", "runtime closed"));
        }

        {
            let mut requests = self.requests.lock().unwrap();
            if requests.contains_key(id) {
                return Err(PluginError::new("duplicate_request", id));
            }
            requests.insert(id.into(), cancellation.clone());
        }
        let result = self.invoke_inner(action, input, cancellation, &[]);
        self.requests.lock().unwrap().remove(id);
        result
    }
    pub fn cancel(&self, id: &str) -> bool {
        if let Some(c) = self.requests.lock().unwrap().get(id) {
            c.cancel();
            true
        } else {
            false
        }
    }
    pub fn descriptors(&self) -> Vec<PluginDescriptor> {
        {
            let state = self.state.lock().unwrap();
            state
                .active
                .iter()
                .filter_map(|p| state.descriptors.get(&p.descriptor().id).cloned())
                .collect()
        }
    }
    pub fn capabilities(&self) -> Vec<Capability> {
        self.state
            .lock()
            .unwrap()
            .actions
            .values()
            .map(|a| Capability {
                plugin: a.owner.clone(),
                action: a.descriptor.clone(),
            })
            .collect()
    }
    pub fn bindings(&self) -> Value {
        json!(self.state.lock().unwrap().services)
    }
    pub fn shutdown(&self) {
        if self.closing.swap(true, Ordering::SeqCst) {
            return;
        }
        for cancellation in self.requests.lock().unwrap().values() {
            cancellation.cancel();
        }
        loop {
            let plugin = self.state.lock().unwrap().active.pop();
            let Some(plugin) = plugin else { break };
            let owner = plugin.descriptor().id;
            // Dependencies remain registered until their consumers have disposed.
            plugin.shutdown();
            let mut state = self.state.lock().unwrap();
            state.actions.retain(|_, action| action.owner != owner);
            state
                .services
                .retain(|_, (_, _, provider)| provider != &owner);
            state
                .extensions
                .retain(|_, (provider, _)| provider != &owner);
            state.listeners.retain(|(provider, _)| provider != &owner);
            state.descriptors.remove(&owner);
        }
        // Include partial registrations of the plugin whose activation failed.
        let mut state = self.state.lock().unwrap();
        state.actions.clear();
        state.services.clear();
        state.extensions.clear();
        state.listeners.clear();
        state.descriptors.clear();
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        self.shutdown();
    }
}

pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.as_bytes()[0].is_ascii_alphanumeric()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        && id != "."
        && id != ".."
}
pub fn validate_descriptor(d: &PluginDescriptor, actions: &[ActionDescriptor]) -> PluginResult<()> {
    if !valid_id(&d.id) {
        return Err(PluginError::new("invalid_plugin_id", &d.id));
    }
    semver::Version::parse(&d.version)
        .map_err(|e| PluginError::new("plugin_version", e.to_string()))?;
    if d.protocol_version != PROTOCOL_VERSION {
        return Err(PluginError::new("protocol_version", &d.id));
    }
    compile_schema(&d.config_schema)?;
    let mut ids = BTreeSet::new();
    for a in actions {
        if !valid_id(&a.id) || !ids.insert(a.id.clone()) {
            return Err(PluginError::new("invalid_action_id", &a.id));
        }
        compile_schema(&a.input_schema)?;
        compile_schema(&a.output_schema)?;
    }
    let mut services = BTreeSet::new();
    for s in &d.provides {
        if !valid_id(&s.id) || s.version == 0 || !ids.contains(&s.action) || !services.insert(&s.id)
        {
            return Err(PluginError::new("invalid_service", &s.id));
        }
    }
    for r in &d.requires {
        if !valid_id(&r.id) || r.version == 0 {
            return Err(PluginError::new("invalid_requirement", &r.id));
        }
    }
    let mut collections = BTreeSet::new();
    for collection in &d.asset_collections {
        if !valid_id(&collection.id)
            || !ids.contains(&collection.export_action)
            || !ids.contains(&collection.import_action)
            || !collections.insert(&collection.id)
        {
            return Err(PluginError::new("invalid_collection", &collection.id));
        }
    }
    for panel in &d.panels {
        if !valid_id(&panel.id) || panel.actions.iter().any(|a| !ids.contains(a)) {
            return Err(PluginError::new("invalid_panel", &panel.id));
        }
    }
    Ok(())
}
pub use nctool_plugin_sdk::{compile_schema, validate_schema};

/// The same pure dependency plan is used by startup and configuration previews.
#[derive(Debug, Clone, Serialize)]
pub struct CompositionPlan {
    pub order: Vec<String>,
    pub providers: BTreeMap<String, String>,
}
pub fn resolve_composition(
    descriptors: &[PluginDescriptor],
    providers: &BTreeMap<String, String>,
) -> PluginResult<CompositionPlan> {
    let mut owners: BTreeMap<String, Vec<(String, u32)>> = BTreeMap::new();
    let mut ids = BTreeSet::new();
    for descriptor in descriptors {
        if !ids.insert(descriptor.id.clone()) {
            return Err(PluginError::new("duplicate_plugin", &descriptor.id));
        }
        for service in &descriptor.provides {
            owners
                .entry(service.id.clone())
                .or_default()
                .push((descriptor.id.clone(), service.version));
        }
    }
    let mut selected = BTreeMap::new();
    for (service, candidates) in &owners {
        let id = match providers.get(service) {
            Some(id) if candidates.iter().any(|(candidate, _)| candidate == id) => id.clone(),
            Some(_) => return Err(PluginError::new("provider_missing", service)),
            None if candidates.len() == 1 => candidates[0].0.clone(),
            None => {
                return Err(PluginError::new(
                    "duplicate_service",
                    format!("请为 {service} 选择服务提供方"),
                ))
            }
        };
        selected.insert(service.clone(), id);
    }
    for service in providers.keys() {
        if !owners.contains_key(service) {
            return Err(PluginError::new("provider_missing", service));
        }
    }
    let mut edges: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for descriptor in descriptors {
        let mut deps = BTreeSet::new();
        for requirement in &descriptor.requires {
            let owner = selected.get(&requirement.id).ok_or_else(|| {
                PluginError::new(
                    "service_missing",
                    format!(
                        "{} 需要服务 {}，请启用它的提供方",
                        descriptor.id, requirement.id
                    ),
                )
            })?;
            let version = owners[&requirement.id]
                .iter()
                .find(|(candidate, _)| candidate == owner)
                .unwrap()
                .1;
            if requirement.version != version {
                return Err(PluginError::new(
                    "service_version",
                    format!(
                        "{} 需要 {}@{}",
                        descriptor.id, requirement.id, requirement.version
                    ),
                ));
            }
            deps.insert(owner.clone());
        }
        edges.insert(descriptor.id.clone(), deps);
    }
    let mut order = vec![];
    let mut done = BTreeSet::new();
    while !edges.is_empty() {
        let ready = edges
            .iter()
            .filter(|(_, deps)| deps.is_subset(&done))
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        if ready.is_empty() {
            return Err(PluginError::new(
                "dependency_cycle",
                format!("插件依赖成环: {:?}", edges.keys()),
            ));
        }
        for id in ready {
            edges.remove(&id);
            done.insert(id.clone());
            order.push(id);
        }
    }
    Ok(CompositionPlan {
        order,
        providers: selected,
    })
}
