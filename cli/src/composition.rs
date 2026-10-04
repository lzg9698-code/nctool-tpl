//! The composition root is the sole place that knows concrete builtin providers.
use nctool_plugin_sdk::*;
use nctool_runtime::{
    config::{self, Config},
    Runtime,
};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

pub struct App {
    pub runtime: Arc<Runtime>,
    pub home: PathBuf,
    pub profile: String,
    pub boot_config: Config,
    pub workbench: Arc<crate::workbench::Workbench>,
    next_request: AtomicU64,
}
impl App {
    pub fn boot(home: &Path, workspace: &Path, profile: Option<&str>) -> PluginResult<Arc<Self>> {
        let (mut config, _) = config::load(home)?;
        let boot_config = config.clone();
        if let Some(profile) = profile {
            config.profile = profile.into();
        }
        config::validate(&config)?;
        let mut ids: BTreeSet<String> = ["template".to_string()].into_iter().collect();
        if config.profile == "nc" {
            ids.extend(["math", "nc", "process"].into_iter().map(str::to_string));
        }
        ids.extend(config.enabled.iter().cloned());
        for id in &config.disabled {
            ids.remove(id);
        }
        let mut plugins: Vec<Arc<dyn Plugin>> = Vec::new();
        if ids.contains("template") {
            plugins.push(Arc::new(nctool_plugin_template::TemplatePlugin::new(
                workspace,
            )?));
        }
        #[cfg(feature = "nc-bundle")]
        {
            if ids.contains("math") {
                plugins.push(Arc::new(nctool_plugin_math::MathPlugin));
            }
            if ids.contains("nc") {
                plugins.push(Arc::new(nctool_plugin_nc::NcPlugin::new(workspace)?));
            }
            if ids.contains("process") {
                plugins.push(Arc::new(nctool_plugin_process::ProcessPlugin));
            }
        }
        #[cfg(not(feature = "nc-bundle"))]
        if ids
            .iter()
            .any(|id| ["math", "nc", "process"].contains(&id.as_str()))
        {
            return Err(PluginError::new("bundle_unavailable","This is the template-only build. Install the full distribution or build with --features nc-bundle."));
        }
        plugins.extend(config::external_plugins(home, &config)?);
        let runtime = Runtime::start(plugins, config.plugins.clone(), config.providers.clone())?;
        let workbench = crate::workbench::Workbench::new(runtime.clone(), workspace)?;
        Ok(Arc::new(Self {
            runtime,
            workbench,
            home: home.into(),
            profile: config.profile.clone(),
            boot_config,
            next_request: AtomicU64::new(1),
        }))
    }
    pub fn request_id(&self) -> String {
        format!(
            "request-{}",
            self.next_request.fetch_add(1, Ordering::SeqCst)
        )
    }
    fn definitions(&self) -> PluginResult<Vec<(PluginDescriptor, Vec<ActionDescriptor>)>> {
        let mut definitions = vec![(
            nctool_plugin_template::TemplatePlugin::definition(),
            nctool_plugin_template::TemplatePlugin::action_definitions(),
        )];
        #[cfg(feature = "nc-bundle")]
        {
            definitions.extend([
                (
                    nctool_plugin_math::MathPlugin::definition(),
                    nctool_plugin_math::MathPlugin::action_definitions(),
                ),
                (
                    nctool_plugin_nc::NcPlugin::definition(),
                    nctool_plugin_nc::NcPlugin::action_definitions(),
                ),
                (
                    nctool_plugin_process::ProcessPlugin::definition(),
                    nctool_plugin_process::ProcessPlugin::action_definitions(),
                ),
            ]);
        }
        definitions.extend(
            config::installed(&self.home)?
                .into_iter()
                .map(|(manifest, _)| (manifest.descriptor, manifest.actions)),
        );
        Ok(definitions)
    }
    fn validate_config(&self, config: &Config) -> PluginResult<Value> {
        config::validate(config)?;
        let definitions = self.definitions()?;
        let mut enabled: BTreeSet<String> = ["template".to_string()].into_iter().collect();
        if config.profile == "nc" {
            enabled.extend(["math", "nc", "process"].iter().map(|id| id.to_string()));
        }
        enabled.extend(config.enabled.iter().cloned());
        for id in &config.disabled {
            enabled.remove(id);
        }
        for id in &enabled {
            if !definitions
                .iter()
                .any(|(descriptor, _)| &descriptor.id == id)
            {
                return Err(PluginError::new(
                    "plugin_missing",
                    format!("当前安装不包含插件 {id}"),
                ));
            }
        }
        let mut selected = vec![];
        let mut actions = BTreeSet::new();
        for (descriptor, descriptions) in definitions {
            if !enabled.contains(&descriptor.id) {
                continue;
            }
            nctool_runtime::validate_descriptor(&descriptor, &descriptions)?;
            nctool_runtime::validate_schema(
                &descriptor.config_schema,
                config.plugins.get(&descriptor.id).unwrap_or(&json!({})),
                "invalid_config",
            )?;
            for action in descriptions {
                if !actions.insert(action.id.clone()) {
                    return Err(PluginError::new("duplicate_action", action.id));
                }
            }
            selected.push(descriptor);
        }
        let plan = nctool_runtime::resolve_composition(&selected, &config.providers)?;
        Ok(json!({"valid":true,"plan":plan,"plugins":selected,"applies":"next_start"}))
    }
    fn source_path(source: &str) -> std::path::PathBuf {
        #[cfg(unix)]
        if source.len() > 2
            && source.as_bytes()[1] == b':'
            && source.as_bytes()[0].is_ascii_alphabetic()
        {
            let path = std::path::PathBuf::from(format!(
                "/mnt/{}/{}",
                (source.as_bytes()[0] as char).to_ascii_lowercase(),
                source[2..]
                    .trim_start_matches(['\\', '/'])
                    .replace('\\', "/")
            ));
            if path.exists() {
                return path;
            }
        }
        source.into()
    }
    fn export_bundle(&self) -> PluginResult<Value> {
        let mut collections = serde_json::Map::new();
        let mut manifest = vec![];
        for descriptor in self.runtime.descriptors() {
            for collection in descriptor.asset_collections {
                let exported = self.runtime.invoke(&collection.export_action, json!({}))?;
                let fingerprint = nctool_assets::FileFingerprint::of_bytes(
                    &serde_json::to_vec(&exported.data)?,
                    std::time::UNIX_EPOCH,
                )
                .as_string();
                if collections
                    .insert(collection.id.clone(), exported.data)
                    .is_some()
                {
                    return Err(PluginError::new("duplicate_collection", collection.id));
                }
                manifest.push(json!({"collection":collection.id,"provider":descriptor.id,"version":descriptor.version,"fingerprint":fingerprint}));
            }
        }
        let bundle = json!({"format":"nctool-workspace","version":1,"collections":collections,"manifest":manifest});
        if serde_json::to_vec(&bundle)?.len() > 64 * 1024 * 1024 {
            return Err(PluginError::new("bundle_limit", "工作区资产包超过 64 MiB"));
        }
        Ok(bundle)
    }
    fn import_bundle(&self, bundle: &Value, validate_only: bool) -> PluginResult<Value> {
        if bundle["format"] != "nctool-workspace" || bundle["version"].as_u64() != Some(1) {
            return Err(PluginError::new("invalid_bundle", "不支持的工作区资产包"));
        }
        let records = bundle["collections"]
            .as_object()
            .ok_or_else(|| PluginError::new("invalid_bundle", "缺少资产集合"))?;
        let mut available = std::collections::BTreeMap::new();
        for descriptor in self.runtime.descriptors() {
            for collection in descriptor.asset_collections {
                if available
                    .insert(collection.id.clone(), collection)
                    .is_some()
                {
                    return Err(PluginError::new("duplicate_collection", "资产集合声明冲突"));
                }
            }
        }
        let mut reports = vec![];
        let mut imports = vec![];
        for (id, data) in records {
            let collection = available.get(id).ok_or_else(|| {
                PluginError::new(
                    "collection_missing",
                    format!("请先启用提供资产集合 {id} 的插件"),
                )
            })?;
            let declared = bundle["manifest"]
                .as_array()
                .and_then(|m| m.iter().find(|item| item["collection"] == *id))
                .ok_or_else(|| PluginError::new("invalid_bundle", "资产集合缺少追踪清单"))?;
            let fingerprint = nctool_assets::FileFingerprint::of_bytes(
                &serde_json::to_vec(data)?,
                std::time::UNIX_EPOCH,
            )
            .as_string();
            if declared["fingerprint"] != fingerprint {
                return Err(PluginError::new(
                    "bundle_fingerprint",
                    format!("资产集合 {id} 与追踪清单不一致"),
                ));
            }
            let validated = self.runtime.invoke(
                &collection.import_action,
                json!({"collection":data,"validate_only":true}),
            )?;
            reports.push(json!({"collection":id,"report":validated.data}));
            imports.push((collection.clone(), data.clone()));
        }
        if validate_only {
            return Ok(json!({"valid":true,"reports":reports}));
        }
        let mut applied: Vec<(String, Value)> = vec![];
        for (collection, data) in imports {
            match self.runtime.invoke(
                &collection.import_action,
                json!({"collection":data,"validate_only":false}),
            ) {
                Ok(result) => {
                    applied.push((collection.import_action, result.data["receipt"].clone()))
                }
                Err(mut error) => {
                    for (action, receipt) in applied.into_iter().rev() {
                        if let Err(rollback) =
                            self.runtime.invoke(&action, json!({"rollback":receipt}))
                        {
                            error.diagnostics.push(Diagnostic {
                                level: "error".into(),
                                code: "rollback_failed".into(),
                                message: rollback.message,
                                path: None,
                            });
                        }
                    }
                    return Err(error);
                }
            }
        }
        Ok(json!({"imported":true,"reports":reports}))
    }
    pub fn inventory(&self) -> PluginResult<Value> {
        let active = self.runtime.descriptors();
        let definitions = self.definitions()?;
        let (saved, _) = config::load(&self.home)?;
        let external = config::installed(&self.home)?;
        let mut plugins = Vec::new();
        for id in ["template", "math", "nc", "process"] {
            let available = id == "template" || cfg!(feature = "nc-bundle");
            plugins.push(json!({"id":id,"builtin":true,"available":available,"descriptor":definitions.iter().find(|(d,_)|d.id==id).map(|(d,_)|d),"active":active.iter().any(|d|d.id==id),"enabled_next_start":!saved.disabled.contains(&id.into()) && (id=="template" || saved.enabled.contains(&id.into()) || saved.profile=="nc")}));
        }
        for (manifest, _) in external {
            let id = &manifest.descriptor.id;
            plugins.push(json!({"id":id,"builtin":false,"available":true,"active":active.iter().any(|d|&d.id==id),"enabled_next_start":saved.enabled.contains(id) && !saved.disabled.contains(id),"descriptor":manifest.descriptor}));
        }
        Ok(
            json!({"profile":self.profile,"plugins":plugins,"active":active,"restart_required":serde_json::to_value(&saved)?!=serde_json::to_value(&self.boot_config)?}),
        )
    }
    pub fn dispatch(&self, method: &str, path: &str, body: Value) -> PluginResult<Value> {
        match (method, path) {
            ("GET", "/api/v2/workspace") => {
                Ok(json!({"id":self.workbench.workspace_id,"profile":self.profile}))
            }
            ("GET", "/api/v2/runs") => self.workbench.list(),
            ("POST", "/api/v2/runs") => {
                let action = body["action"]
                    .as_str()
                    .ok_or_else(|| PluginError::new("invalid_input", "action required"))?;
                let input = body
                    .get("input")
                    .cloned()
                    .ok_or_else(|| PluginError::new("invalid_input", "input required"))?;
                let id = self.workbench.start(
                    action,
                    input,
                    body["title"].as_str().unwrap_or(action),
                )?;
                Ok(json!({"id":id}))
            }
            ("GET", path)
                if path.starts_with("/api/v2/runs/") && path.ends_with("/replay-check") =>
            {
                let id = path["/api/v2/runs/".len()..]
                    .strip_suffix("/replay-check")
                    .unwrap();
                serde_json::to_value(self.workbench.replay_check(id)?).map_err(Into::into)
            }
            ("GET", path) if path.starts_with("/api/v2/runs/") => {
                serde_json::to_value(self.workbench.get(&path["/api/v2/runs/".len()..])?)
                    .map_err(Into::into)
            }
            ("POST", path) if path.starts_with("/api/v2/runs/") => {
                let (id, op) = path["/api/v2/runs/".len()..]
                    .rsplit_once('/')
                    .ok_or_else(|| PluginError::new("not_found", path))?;
                match op {
                    "rerun" => {
                        let new_id = self.workbench.rerun(id)?;
                        Ok(json!({"id":new_id}))
                    }
                    "cancel" => Ok(json!({"cancelled":self.workbench.cancel(id)})),
                    "remove" => {
                        self.workbench.remove(id)?;
                        Ok(json!({"removed":id}))
                    }
                    _ => Err(PluginError::new("not_found", path)),
                }
            }
            ("GET", "/api/v2/plugins") => self.inventory(),
            ("GET", "/api/v2/capabilities") => {
                let mut document_actions: Vec<_> = self
                    .definitions()?
                    .into_iter()
                    .flat_map(|(_, actions)| actions)
                    .filter(|action| {
                        action
                            .ui_schema
                            .as_ref()
                            .is_some_and(|ui| ui.get("document_input").is_some())
                    })
                    .map(|action| json!(action))
                    .collect();
                // Legacy NC metadata intent is known even by template-only distributions.
                // This is an association declaration, not a loaded domain implementation.
                if !document_actions
                    .iter()
                    .any(|action| action["id"] == "nc.generate")
                {
                    document_actions.push(json!({"id":"nc.generate","ui_schema":{"document_input":{"match_metadata":"nc","bindings":{}}}}));
                }
                Ok(
                    json!({"actions":self.runtime.capabilities(),"document_actions":document_actions,"services":self.runtime.bindings(),"panels":self.runtime.descriptors().into_iter().flat_map(|d|d.panels).collect::<Vec<_>>()}),
                )
            }
            ("GET", "/api/v2/config") => {
                let (config, fp) = config::load(&self.home)?;
                Ok(
                    json!({"config":config,"fingerprint":fp.map(|fp|serde_json::to_string(&fp)).transpose()? ,"applies":"next_start"}),
                )
            }
            ("POST", "/api/v2/config/validate") => {
                let config: Config = serde_json::from_value(body["config"].clone())?;
                self.validate_config(&config)
            }
            ("POST", "/api/v2/plugins/inspect") => {
                let source = body["source"]
                    .as_str()
                    .ok_or_else(|| PluginError::new("invalid_input", "请选择本地插件目录"))?;
                let manifest =
                    nctool_runtime::external::ExternalManifest::read(&Self::source_path(source))?;
                Ok(
                    json!({"descriptor":manifest.descriptor,"actions":manifest.actions,"limits":manifest.limits}),
                )
            }
            ("POST", "/api/v2/plugins/install") => {
                let source = body["source"]
                    .as_str()
                    .ok_or_else(|| PluginError::new("invalid_input", "请选择本地插件目录"))?;
                let id = config::install(&self.home, &Self::source_path(source))?;
                Ok(json!({"installed":id,"enabled":false,"restart_required":true}))
            }
            ("POST", "/api/v2/plugins/uninstall") => {
                let id = body["id"]
                    .as_str()
                    .ok_or_else(|| PluginError::new("invalid_input", "缺少插件标识"))?;
                if ["template", "math", "nc", "process"].contains(&id) {
                    return Err(PluginError::new("builtin_plugin", "内置能力只能停用"));
                }
                if self.runtime.descriptors().iter().any(|d| d.id == id) {
                    return Err(PluginError::new(
                        "plugin_active",
                        "请先停用并重启，再卸载这个插件",
                    ));
                }
                config::uninstall(&self.home, id)?;
                Ok(json!({"uninstalled":id}))
            }
            ("GET", "/api/v2/bundle") => self.export_bundle(),
            ("POST", "/api/v2/bundle/validate") => self.import_bundle(&body["bundle"], true),
            ("POST", "/api/v2/bundle/import") => self.import_bundle(&body["bundle"], false),
            ("POST", "/api/v2/config") => {
                let c: Config = serde_json::from_value(
                    body.get("config")
                        .cloned()
                        .ok_or_else(|| PluginError::new("invalid_input", "config required"))?,
                )?;
                let fp = body
                    .get("expected")
                    .and_then(Value::as_str)
                    .map(serde_json::from_str)
                    .transpose()?;
                self.validate_config(&c)?;
                config::save(&self.home, &c, fp)?;
                Ok(json!({"restart_required":true}))
            }
            ("POST", path) if path.starts_with("/api/v2/actions/") => {
                let action = &path["/api/v2/actions/".len()..];
                let id = body
                    .get("request_id")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| self.request_id());
                if !nctool_runtime::valid_id(&id) {
                    return Err(PluginError::new("invalid_request_id", id));
                }
                let input = body
                    .get("input")
                    .cloned()
                    .ok_or_else(|| PluginError::new("invalid_input", "input required"))?;
                self.runtime
                    .invoke_request(&id, action, input)
                    .and_then(|r| serde_json::to_value(r).map_err(Into::into))
            }
            ("POST", path) if path.starts_with("/api/v2/cancel/") => {
                Ok(json!({"cancelled":self.runtime.cancel(&path["/api/v2/cancel/".len()..])}))
            }
            ("POST", path) if path.starts_with("/api/v2/plugins/") => {
                let rest = &path["/api/v2/plugins/".len()..];
                let (id, operation) = rest
                    .rsplit_once('/')
                    .ok_or_else(|| PluginError::new("not_found", path))?;
                if !["enable", "disable"].contains(&operation) {
                    return Err(PluginError::new("not_found", path));
                }
                let known = self.inventory()?["plugins"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|p| p["id"] == id && p["available"] == true);
                if !known {
                    return Err(PluginError::new("plugin_missing", id));
                }
                let (mut config, fingerprint) = config::load(&self.home)?;
                config.enabled.retain(|value| value != id);
                config.disabled.retain(|value| value != id);
                if operation == "enable" {
                    config.enabled.push(id.into());
                } else {
                    config.disabled.push(id.into());
                }
                self.validate_config(&config)?;
                config::save(&self.home, &config, fingerprint)?;
                Ok(json!({"restart_required":true}))
            }
            _ => Err(PluginError::new("not_found", path)),
        }
    }
}
pub fn default_home() -> PathBuf {
    if let Some(home) = std::env::var_os("NCTOOL_HOME") {
        return home.into();
    }
    #[cfg(windows)]
    if let Some(base) = std::env::var_os("LOCALAPPDATA") {
        return PathBuf::from(base).join("nctool");
    }
    if let Some(base) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(base).join("nctool");
    }
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config/nctool")
}

impl Drop for App {
    fn drop(&mut self) {
        // Dispose while the runtime Arc can still service dependency cleanup calls.
        self.runtime.shutdown();
    }
}
