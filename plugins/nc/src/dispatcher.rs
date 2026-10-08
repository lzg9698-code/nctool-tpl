//! Compatibility routing. Business providers depend only on the lower NC services.
use crate::{operation, NcFoundationPlugin};
use nctool_plugin_sdk::*;
pub struct NcManagedPlugin {
    sync: bool,
    undercut: bool,
}
impl NcManagedPlugin {
    pub fn new(sync: bool, undercut: bool) -> Self {
        Self { sync, undercut }
    }
    pub fn definition() -> PluginDescriptor {
        Self::new(true, true).descriptor()
    }
    pub fn action_definitions() -> Vec<ActionDescriptor> {
        let mut schema = NcFoundationPlugin::action_definitions()
            .into_iter()
            .find(|a| a.id == "nc.foundation.freeze")
            .unwrap()
            .input_schema;
        schema["properties"]["business"] = json!({"type":"object","required":["plugin","input"],"properties":{"plugin":{"type":"string","enum":["sync-turning","undercut"]},"input":{"type":"object"}},"additionalProperties":false});
        schema["anyOf"]
            .as_array_mut()
            .unwrap()
            .push(json!({"required":["business"]}));
        let freeze = ActionDescriptor::new("nc.managed.freeze", "冻结受管输入", schema.clone());
        let check = ActionDescriptor::new(
            "nc.managed.check",
            "核对受管资源",
            json!({"type":"object","required":["managed_snapshot"],"properties":{"managed_snapshot":{"type":"object"}},"additionalProperties":false}),
        );
        let mut generate =
            ActionDescriptor::new("nc.generate.managed", "从原始参数生成 NC", schema);
        generate.ui_schema = Some(
            json!({"document_input":{"title":"受管 NC 程序","description":"解析共享参数并在后端完成工艺计算。","bindings":{"source":"source","template":"id","operation_inputs":"context","schema":"schema","defaults":"defaults","operation":"metadata.nc.operation","bindings":"metadata.parameters.bindings","parameter_selection":"parameter_selection","options":"metadata.nc.options","machine_id":"metadata.nc.machine_id","trim_blocks":"metadata.nc.render_options.trim_blocks","lstrip_blocks":"metadata.nc.render_options.lstrip_blocks"}}}),
        );
        vec![
            freeze,
            check,
            generate,
            ActionDescriptor::new(
                "nc.operation.prepare",
                "从原始参数准备工艺",
                json!({"type":"object","required":["operation","inputs"],"properties":{"operation":{"type":"object"},"inputs":{"type":"object"},"standard":{"type":"object"}},"additionalProperties":false}),
            ),
        ]
    }
    fn enabled(&self, id: &str) -> PluginResult<()> {
        if (id == "sync-turning" && self.sync) || (id == "undercut" && self.undercut) {
            Ok(())
        } else {
            Err(PluginError::new("plugin_unavailable", "专用业务插件未启用"))
        }
    }
    fn business_id<'a>(&self, frozen: &'a Value) -> PluginResult<&'a str> {
        let id = frozen["business_plugin"]
            .as_str()
            .ok_or_else(|| PluginError::new("snapshot_invalid", "缺少业务插件"))?;
        self.enabled(id)?;
        if frozen["format_version"] != 1 {
            return Err(PluginError::new("snapshot_invalid", "快照版本无效"));
        }
        Ok(id)
    }
    fn prepare(&self, request: Value, ctx: &Context) -> PluginResult<ActionResult> {
        let kind = request["operation"]["id"].as_str().unwrap_or("");
        let service = match kind {
            "sync.left" | "sync.right" if self.sync => Some("sync-turning.operation.prepare"),
            "groove" if self.undercut => Some("undercut.operation.prepare"),
            "sync.left" | "sync.right" | "groove" => {
                return Err(PluginError::new(
                    "plugin_unavailable",
                    "此工艺的专用插件未启用",
                ))
            }
            _ => None,
        };
        if let Some(service) = service {
            ctx.call(service, 1, request)
        } else {
            operation::prepare_with_cutting(
                &request["operation"],
                &request["inputs"],
                &request["standard"],
                &request["cutting"],
            )
            .map(ActionResult::data)
        }
    }
}
impl Plugin for NcManagedPlugin {
    fn descriptor(&self) -> PluginDescriptor {
        let mut d = PluginDescriptor::builtin("nc-managed");
        d.requires = [
            "nc.foundation.freeze",
            "nc.foundation.check",
            "nc.managed.resolve",
            "nc.managed.render",
        ]
        .iter()
        .map(|s| Requirement::new(s))
        .collect();
        if self.sync {
            for suffix in ["operation.prepare", "freeze", "generate", "check"] {
                d.requires
                    .push(Requirement::new(&format!("sync-turning.{suffix}")));
            }
        }
        if self.undercut {
            for suffix in ["operation.prepare", "freeze", "generate", "check"] {
                d.requires
                    .push(Requirement::new(&format!("undercut.{suffix}")));
            }
        }
        d.provides = [
            "nc.managed.freeze",
            "nc.managed.check",
            "nc.generate.managed",
            "nc.operation.prepare",
        ]
        .iter()
        .map(|s| Service::new(s, s))
        .collect();
        d
    }
    fn actions(&self) -> Vec<ActionDescriptor> {
        Self::action_definitions()
    }
    fn invoke(&self, action: &str, input: Value, ctx: &Context) -> PluginResult<ActionResult> {
        if action == "nc.operation.prepare" {
            return self.prepare(input, ctx);
        }
        if action == "nc.managed.check" {
            let frozen = &input["managed_snapshot"];
            if frozen["kind"] == "business" {
                return ctx.call(
                    &format!("{}.check", self.business_id(frozen)?),
                    1,
                    json!({"job_snapshot":frozen["job_snapshot"]}),
                );
            }
            return ctx.call("nc.foundation.check", 1, input);
        }
        let frozen = if let Some(frozen) = input.get("managed_snapshot") {
            if input.as_object().is_none_or(|m| m.len() != 1) {
                return Err(PluginError::new("snapshot_invalid", "快照与实时输入互斥"));
            }
            frozen.clone()
        } else if input.get("business").is_some() {
            if input.as_object().is_none_or(|m| {
                m.keys().any(|k| {
                    !["business", "machine", "machine_id", "options"].contains(&k.as_str())
                })
            }) {
                return Err(PluginError::new(
                    "invalid_input",
                    "业务批次不能混用模板或派生输入",
                ));
            }
            let id = input["business"]["plugin"].as_str().unwrap_or("");
            self.enabled(id)?;
            let mut raw = input["business"]["input"].clone();
            for key in ["machine", "machine_id", "options"] {
                if let Some(v) = input.get(key) {
                    raw[key] = v.clone();
                }
            }
            let saved = ctx.call(&format!("{id}.freeze"), 1, raw)?.data;
            json!({"format_version":1,"kind":"business","business_plugin":id,"job_snapshot":saved,"input":input,"machine":saved["nc"]["machine"],"parameter_snapshot":saved["nc"]["parameter_snapshot"],"nc_snapshot":saved["nc"]["nc_snapshot"]})
        } else {
            ctx.call("nc.foundation.freeze", 1, input)?.data
        };
        if action == "nc.managed.freeze" {
            return Ok(ActionResult::data(frozen));
        }
        if frozen["kind"] == "business" {
            let id = self.business_id(&frozen)?;
            let mut saved = frozen["job_snapshot"].clone();
            if let Some(options) = frozen["input"].get("options") {
                saved["input"]["options"] = options.clone();
            }
            let mut result =
                ctx.call(&format!("{id}.generate"), 1, json!({"job_snapshot":saved}))?;
            let channels = result
                .artifacts
                .iter()
                .map(|a| a.text.clone())
                .collect::<Vec<_>>();
            result.data["text"] = json!(channels.join("\n"));
            result.data["channels"] = json!(channels);
            let target = saved["input"]["target"].as_str().unwrap_or("all");
            result.data["next_sync_num"] =
                result.data["prepared"]["sides"][target]["next_sync_num"].clone();
            let mut replay = frozen.clone();
            replay["job_snapshot"] = result.data["replay_input"]["job_snapshot"].clone();
            result.data["replay_input"] = json!({"managed_snapshot":replay});
            return Ok(result);
        }
        let resolved = ctx
            .call("nc.managed.resolve", 1, json!({"managed_snapshot":frozen}))?
            .data;
        let prepared = self.prepare(json!({"operation":resolved["operation"],"inputs":resolved["inputs"],"standard":resolved["standard"],"cutting":resolved["cutting"]}),ctx)?.data;
        ctx.call(
            "nc.managed.render",
            1,
            json!({"managed_snapshot":frozen,"resolved":resolved,"prepared":prepared}),
        )
    }
}
