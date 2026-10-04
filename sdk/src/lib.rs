//! Versioned, domain-neutral plugin contracts shared by every host surface.
pub use nctool_tpl::Renderer;
use serde::{Deserialize, Serialize};
pub use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

pub const PROTOCOL_VERSION: u32 = 1;
pub type PluginResult<T> = Result<T, PluginError>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PluginError {
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub diagnostics: Vec<Diagnostic>,
}
impl PluginError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            diagnostics: vec![],
        }
    }
}
impl std::fmt::Display for PluginError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for PluginError {}
impl From<std::io::Error> for PluginError {
    fn from(e: std::io::Error) -> Self {
        Self::new("io", e.to_string())
    }
}
impl From<serde_json::Error> for PluginError {
    fn from(e: serde_json::Error) -> Self {
        Self::new("invalid_json", e.to_string())
    }
}
impl From<nctool_tpl::TplError> for PluginError {
    fn from(e: nctool_tpl::TplError) -> Self {
        Self::new("template_error", e.to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Diagnostic {
    pub level: String,
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}
impl Diagnostic {
    pub fn warning(code: &str, message: impl Into<String>) -> Self {
        Self {
            level: "warning".into(),
            code: code.into(),
            message: message.into(),
            path: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub name: String,
    pub media_type: String,
    pub text: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ActionResult {
    #[serde(default)]
    pub data: Value,
    #[serde(default)]
    pub artifacts: Vec<Artifact>,
    #[serde(default)]
    pub diagnostics: Vec<Diagnostic>,
}
impl ActionResult {
    pub fn data(data: Value) -> Self {
        Self {
            data,
            artifacts: vec![],
            diagnostics: vec![],
        }
    }
    pub fn text(name: &str, media_type: &str, text: String) -> Self {
        Self {
            data: json!({"text":text}),
            artifacts: vec![Artifact {
                name: name.into(),
                media_type: media_type.into(),
                text,
            }],
            diagnostics: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Service {
    pub id: String,
    pub version: u32,
    pub action: String,
}
impl Service {
    pub fn new(id: &str, action: &str) -> Self {
        Self {
            id: id.into(),
            version: 1,
            action: action.into(),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Requirement {
    pub id: String,
    pub version: u32,
}
impl Requirement {
    pub fn new(id: &str) -> Self {
        Self {
            id: id.into(),
            version: 1,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Panel {
    pub id: String,
    pub title: String,
    pub actions: Vec<String>,
    /// Built-in editor presentation, or schema-driven action forms.
    #[serde(default = "default_view")]
    pub view: String,
}
fn default_view() -> String {
    "form".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PluginDescriptor {
    pub id: String,
    pub version: String,
    pub protocol_version: u32,
    #[serde(default)]
    pub requires: Vec<Requirement>,
    #[serde(default)]
    pub provides: Vec<Service>,
    #[serde(default = "object_schema")]
    pub config_schema: Value,
    #[serde(default)]
    pub panels: Vec<Panel>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub asset_collections: Vec<AssetCollection>,
}
impl PluginDescriptor {
    pub fn builtin(id: &str) -> Self {
        Self {
            id: id.into(),
            version: "2.0.0".into(),
            protocol_version: PROTOCOL_VERSION,
            requires: vec![],
            provides: vec![],
            config_schema: object_schema(),
            panels: vec![],
            asset_collections: vec![],
        }
    }
}
pub fn object_schema() -> Value {
    json!({"type":"object"})
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ActionDescriptor {
    pub id: String,
    pub title: String,
    pub input_schema: Value,
    pub output_schema: Value,
    #[serde(default)]
    pub example: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui_schema: Option<Value>,
}
impl ActionDescriptor {
    pub fn new(id: &str, title: &str, input: Value) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            input_schema: input,
            output_schema: object_schema(),
            example: json!({}),
            ui_schema: None,
        }
    }
}

pub type RendererCallback =
    Arc<dyn Fn(&mut Renderer) -> Result<(), nctool_tpl::TplError> + Send + Sync>;
pub struct RenderExtension {
    pub id: String,
    pub configure: RendererCallback,
}
impl RenderExtension {
    pub fn new(
        id: &str,
        configure: impl Fn(&mut Renderer) -> Result<(), nctool_tpl::TplError> + Send + Sync + 'static,
    ) -> Self {
        Self {
            id: id.into(),
            configure: Arc::new(configure),
        }
    }
}
#[derive(Clone, Default)]
pub struct Cancellation(Arc<AtomicBool>);
impl Cancellation {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
    pub fn check(&self) -> PluginResult<()> {
        if self.is_cancelled() {
            Err(PluginError::new("cancelled", "request cancelled"))
        } else {
            Ok(())
        }
    }
}

pub trait Host: Send + Sync {
    fn call(
        &self,
        owner: &str,
        service: &str,
        version: u32,
        input: Value,
        cancellation: Cancellation,
        stack: &[String],
    ) -> PluginResult<ActionResult>;
    fn extend(&self, renderer: &mut Renderer, extensions: &[String]) -> PluginResult<()>;
}
#[derive(Clone)]
pub struct Context {
    pub owner: String,
    pub config: Value,
    pub cancellation: Cancellation,
    pub stack: Vec<String>,
    pub host: Arc<dyn Host>,
}
impl Context {
    pub fn call(&self, service: &str, version: u32, input: Value) -> PluginResult<ActionResult> {
        self.cancellation.check()?;
        self.host.call(
            &self.owner,
            service,
            version,
            input,
            self.cancellation.clone(),
            &self.stack,
        )
    }
    pub fn extend(&self, renderer: &mut Renderer, extensions: &[String]) -> PluginResult<()> {
        self.host.extend(renderer, extensions)
    }
}
/// Registrations are supplied by the plugin and published transactionally after activation.
pub trait Plugin: Send + Sync {
    fn descriptor(&self) -> PluginDescriptor;
    fn actions(&self) -> Vec<ActionDescriptor> {
        vec![]
    }
    fn extensions(&self) -> Vec<RenderExtension> {
        vec![]
    }
    fn listeners(&self) -> Vec<EventListener> {
        vec![]
    }
    fn activate(&self, _ctx: &Context) -> PluginResult<()> {
        Ok(())
    }
    fn invoke(&self, action: &str, _input: Value, _ctx: &Context) -> PluginResult<ActionResult> {
        Err(PluginError::new("action_not_found", action))
    }
    fn shutdown(&self) {}
}

/// Typed observational events; policy and transformations use explicit services.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind {
    ActionStarted,
    ActionFinished,
}
#[derive(Clone, Debug)]
pub struct Event {
    pub kind: EventKind,
    pub action: String,
    pub owner: String,
    pub succeeded: Option<bool>,
}
pub type EventCallback = Arc<dyn Fn(&Event) + Send + Sync>;
pub struct EventListener {
    pub kind: EventKind,
    pub callback: EventCallback,
}

/// All protocol/host JSON ingress uses the same precision-preserving parser.
pub fn parse_json(bytes: &[u8]) -> PluginResult<Value> {
    nctool_assets::parse_json(bytes).map_err(|e| {
        let code = match e {
            nctool_assets::WriteError::NumUnderflow { .. } => "numeric_underflow",
            nctool_assets::WriteError::NumRange { .. } => "numeric_range",
            _ => "invalid_json",
        };
        PluginError::new(code, e.to_string())
    })
}

/// Immutable template collection and root metadata used across multi-service calls.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateSnapshot {
    pub template: String,
    pub templates: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub asset: Option<Value>,
}

/// Optional storage contributions keep collection export/import out of the host's domain logic.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AssetCollection {
    pub id: String,
    pub export_action: String,
    pub import_action: String,
}

fn reject_remote_refs(schema: &Value) -> PluginResult<()> {
    match schema {
        Value::Object(map) => {
            for (key, value) in map {
                if key == "$ref" && value.as_str().is_some_and(|s| !s.starts_with('#')) {
                    return Err(PluginError::new(
                        "invalid_schema",
                        "only local schema references are supported",
                    ));
                }
                reject_remote_refs(value)?;
            }
        }
        Value::Array(items) => {
            for item in items {
                reject_remote_refs(item)?;
            }
        }
        _ => {}
    }
    Ok(())
}
pub fn compile_schema(schema: &Value) -> PluginResult<jsonschema::Validator> {
    reject_remote_refs(schema)?;
    jsonschema::validator_for(schema).map_err(|e| PluginError::new("invalid_schema", e.to_string()))
}
pub fn validate_schema(schema: &Value, value: &Value, code: &str) -> PluginResult<()> {
    compile_schema(schema)?.validate(value).map_err(|e| {
        let mut error = PluginError::new(code, e.to_string());
        error.diagnostics.push(Diagnostic {
            level: "error".into(),
            code: code.into(),
            message: e.to_string(),
            path: Some(e.instance_path.to_string()),
        });
        error
    })
}
