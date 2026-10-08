//! Shared storage/freeze plumbing for NC business providers, not business calculations.
use nctool_plugin_parameters::{library_actions, Library};
use nctool_plugin_sdk::*;
use std::path::Path;
pub struct BusinessAssets {
    pub library: Library,
}
impl BusinessAssets {
    pub fn new(root: &Path, id: &str) -> PluginResult<Self> {
        let base = format!("operations/{id}");
        Ok(Self {
            library: Library::new(
                root,
                &[
                    ("trajectories", &format!("{base}/trajectories")),
                    ("sessions", &format!("{base}/sessions")),
                ],
            )?,
        })
    }
    pub fn actions(id: &str) -> Vec<ActionDescriptor> {
        library_actions(&format!("{id}.asset"), &["trajectories", "sessions"])
    }
    pub fn contribute(d: &mut PluginDescriptor, id: &str) {
        d.asset_collections.push(AssetCollection {
            id: format!("{id}-assets"),
            export_action: format!("{id}.asset.collection.export"),
            import_action: format!("{id}.asset.collection.import"),
        });
    }
    pub fn capture(&self, builtin: Value) -> PluginResult<Value> {
        let mut snapshot = self.library.capture()?;
        snapshot["records"]
            .as_array_mut()
            .unwrap()
            .push(json!({"group":"builtin","name":builtin["id"],"asset":builtin}));
        Ok(snapshot)
    }
}
pub fn fingerprint(v: &Value) -> String {
    nctool_assets::FileFingerprint::of_bytes(&serde_json::to_vec(v).unwrap(), std::time::UNIX_EPOCH)
        .as_string()
}
pub fn preset(snapshot: &Value, choice: &Value) -> PluginResult<Value> {
    let id = choice["preset_id"].as_str().unwrap_or("v3-compatible");
    let asset = snapshot["records"]
        .as_array()
        .and_then(|rows| {
            rows.iter().find(|r| {
                r["name"] == id && (r["group"] == "trajectories" || r["group"] == "builtin")
            })
        })
        .map(|r| r["asset"].clone())
        .ok_or_else(|| PluginError::new("not_found", format!("轨迹预设 {id}")))?;
    if !asset.is_object() || !asset["parameters"].is_object() {
        return Err(PluginError::new(
            "snapshot_invalid",
            "冻结轨迹预设缺少参数对象",
        ));
    }
    let mut effective = asset.clone();
    if let Some(overrides) = choice.get("overrides") {
        let overrides = overrides
            .as_object()
            .ok_or_else(|| PluginError::new("trajectory_validation", "轨迹调整必须为对象"))?;
        for (k, v) in overrides {
            effective["parameters"][k] = v.clone();
        }
        if !overrides.is_empty() && choice["confirmed_for"] != confirmation(&effective, snapshot) {
            return Err(PluginError::new(
                "confirmation_required",
                "请重新预览并确认本次轨迹调整",
            ));
        }
    }
    Ok(effective)
}
pub fn confirmation(asset: &Value, snapshot: &Value) -> String {
    fingerprint(
        &json!({"trajectory":asset,"conditions":snapshot.get("_confirmation_context").cloned().unwrap_or(json!({}))}),
    )
}
pub fn trajectory_resources(saved: &Value, side: &str, standard: &Value) -> Value {
    let mut snapshot = saved["assets"].clone();
    let mut globals = saved["input"].clone();
    globals.as_object_mut().unwrap().remove("sides");
    globals.as_object_mut().unwrap().remove("target");
    // Formatting options do not change the accepted path geometry.
    globals.as_object_mut().unwrap().remove("options");
    let mut settings = saved["input"]["sides"][side].clone();
    settings.as_object_mut().unwrap().remove("trajectory");
    // A composition scheduler may advance barrier numbers without changing geometry.
    settings.as_object_mut().unwrap().remove("start_sync_num");
    snapshot["_confirmation_context"] = json!({"side":side,"settings":settings,"globals":globals,"machine":saved["nc"]["machine"],"standard":standard});
    snapshot
}
pub fn preview_preset(snapshot: &Value, choice: &Value) -> PluginResult<Value> {
    let mut base = choice.clone();
    base.as_object_mut()
        .ok_or_else(|| PluginError::new("trajectory_validation", "轨迹选择必须为对象"))?
        .remove("overrides");
    let mut asset = preset(snapshot, &base)?;
    if let Some(overrides) = choice.get("overrides").and_then(Value::as_object) {
        for (k, v) in overrides {
            asset["parameters"][k] = v.clone();
        }
    }
    Ok(asset)
}
pub fn prepare_metadata(
    mut prepared: Value,
    operation: &Value,
    inputs: &Value,
) -> PluginResult<Value> {
    if operation["version"] != 1 {
        return Err(PluginError::new(
            "operation_validation",
            "不支持的工艺算法版本",
        ));
    }
    for c in prepared["contexts"].as_array().unwrap() {
        let _: crate::model::ParameterSet = serde_json::from_value(c.clone())?;
    }
    prepared["derived_fields"] = json!(prepared["contexts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c
            .as_object()
            .unwrap()
            .keys()
            .filter(|k| inputs.get(*k) != c.get(*k))
            .map(|k| format!("/{k}"))
            .collect::<Vec<_>>())
        .collect::<Vec<_>>());
    prepared["operation"] = operation.clone();
    prepared["inputs"] = inputs.clone();
    prepared["algorithm_version"] = json!(1);
    prepared["output_plan"] = json!({"channels":if operation["id"].as_str().unwrap_or("").starts_with("sync."){vec![1,2]}else{vec![1]},"all_or_nothing":true});
    Ok(prepared)
}
pub fn snapshot(
    input: &Value,
    assets: &BusinessAssets,
    builtin: Value,
    ctx: &Context,
) -> PluginResult<Value> {
    if let Some(saved) = input.get("job_snapshot") {
        if input.as_object().is_none_or(|m| m.len() != 1)
            || saved["format_version"] != 1
            || !saved["input"].is_object()
            || saved["strategy_version"] != 1
            || saved["assets"]["version"] != 1
            || !saved["assets"]["records"].is_array()
            || !saved["templates"].is_object()
        {
            return Err(PluginError::new(
                "snapshot_invalid",
                "冻结快照与实时输入互斥，且必须使用支持的策略版本",
            ));
        }
        ctx.call(
            "nc.foundation.freeze",
            1,
            json!({"managed_snapshot":saved["nc"]}),
        )?;
        return Ok(saved.clone());
    }
    let mut request = json!({"source":"{{ program }}","operation_inputs":{"program":""},"operation":{"id":"direct","version":1}});
    for key in ["machine", "machine_id", "options"] {
        if let Some(v) = input.get(key) {
            request[key] = v.clone();
        }
    }
    let nc = ctx.call("nc.foundation.freeze", 1, request)?.data;
    let mut templates = json!({});
    for side in ["right", "left"] {
        if let Some(name) = input["sides"][side]["template"].as_str() {
            let captured = ctx
                .call(
                    "template.inspect",
                    1,
                    json!({"template":name,"include_snapshot":true}),
                )?
                .data["snapshot"]
                .clone();
            if captured["asset"]["metadata"]["nc"]["trajectory_plan_version"] != 1 {
                return Err(PluginError::new("template_incompatible","所选模板不支持 v1 轨迹计划。请选择内置格式或显式升级模板；旧自由模板仍可通过兼容 API 生成。"));
            }
            let expected = if builtin["plugin"] == "sync-turning" {
                format!("sync.{side}")
            } else {
                "groove".into()
            };
            if captured["asset"]["metadata"]["nc"]["operation"]["id"] != expected {
                return Err(PluginError::new(
                    "template_incompatible",
                    "模板的工艺或侧别与当前批次不匹配",
                ));
            }
            templates[side] = captured;
        }
    }
    Ok(
        json!({"format_version":1,"strategy_version":1,"input":input,"assets":assets.capture(builtin)?,"nc":nc,"templates":templates}),
    )
}
pub fn check(saved: &Value, assets: &BusinessAssets, ctx: &Context) -> PluginResult<Value> {
    let mut current = saved["assets"].clone();
    current["records"]
        .as_array_mut()
        .ok_or_else(|| PluginError::new("snapshot_invalid", "缺少资源"))?
        .retain(|r| r["group"] != "builtin");
    let mut changes = assets.library.changed(&current)?;
    for captured in saved["templates"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(_, v)| v)
    {
        let inspection = ctx.call(
            "template.inspect",
            1,
            json!({"template":captured["template"],"include_snapshot":true}),
        );
        if inspection.is_err() || inspection.unwrap().data["snapshot"] != *captured {
            changes.push(format!("templates/{}", captured["template"]));
        }
    }
    changes.extend(
        ctx.call(
            "nc.foundation.check",
            1,
            json!({"managed_snapshot":saved["nc"]}),
        )?
        .data["changes"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string),
    );
    Ok(json!({"current":changes.is_empty(),"changes":changes}))
}
pub fn render_trajectory(
    side: &str,
    context: &Value,
    program: &str,
    saved: &Value,
    ctx: &Context,
) -> PluginResult<String> {
    if let Some(template) = saved["templates"].get(side) {
        if template["asset"]["metadata"]["nc"]["trajectory_plan_version"] != 1 {
            return Err(PluginError::new(
                "template_incompatible",
                "冻结模板不支持轨迹计划",
            ));
        }
        let mut params = context.clone();
        params["trajectory_program"] = json!(program);
        let result=ctx.call("nc.generate",1,json!({"snapshot":template,"params":params,"schema":template["asset"]["metadata"]["nc"].get("context_schema").cloned().unwrap_or(json!(true)),"machine":saved["nc"]["machine"],"options":{"line_numbers":false}}))?;
        Ok(result.data["text"]
            .as_str()
            .ok_or_else(|| PluginError::new("render_invalid", "模板未返回正文"))?
            .into())
    } else {
        Ok(program.into())
    }
}
pub fn finalize(
    name: &str,
    text: &str,
    saved: &Value,
    ctx: &Context,
) -> PluginResult<ActionResult> {
    ctx.call("nc.output.finalize",1,json!({"name":name,"text":text,"machine":saved["nc"]["machine"],"options":saved["input"].get("options").cloned().unwrap_or(json!({}))}))
}
