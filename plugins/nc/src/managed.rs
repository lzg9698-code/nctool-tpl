//! Opt-in managed services; the original NC v1 provider remains unchanged.
use crate::operation;
use nctool_assets::JsonStore;
use nctool_plugin_parameters::{asset_error, library_actions, record, set_pointer, Library};
use nctool_plugin_sdk::*;
use std::path::Path;

pub struct NcFoundationPlugin {
    library: Library,
    machines: JsonStore,
}
fn invalid(message: impl Into<String>) -> PluginError {
    PluginError::new("managed_validation", message)
}
fn finite(v: &Value, field: &str) -> PluginResult<f64> {
    let value = operation::number(v, field)?;
    if value <= 0. {
        Err(invalid(format!("{field} 必须大于零")))
    } else {
        Ok(value)
    }
}
fn signature(value: &Value) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for b in serde_json::to_vec(value).unwrap_or_default() {
        hash = (hash ^ u64::from(b)).wrapping_mul(0x100000001b3);
    }
    format!("fnv1a64:{hash:016x}")
}
fn validate_asset(group: &str, id: &str, asset: &Value) -> PluginResult<()> {
    if asset["format_version"] != 1 || asset["id"] != id {
        return Err(invalid("资产格式版本必须为 1，id 必须与文件名一致"));
    }
    match group {
        "tool_types" => {
            if !asset["operation"].is_string() {
                return Err(invalid("刀具类型缺少 operation"));
            }
        }
        "tools" => {
            if !asset["type_id"].is_string() || !asset["attributes"].is_object() {
                return Err(invalid("刀具缺少类型或属性"));
            }
            for field in ["diameter", "teeth"] {
                if let Some(v) = asset["attributes"].get(field) {
                    let n = finite(v, field)?;
                    if field == "teeth" && n.fract() != 0. {
                        return Err(invalid("齿数必须是整数"));
                    }
                }
            }
        }
        "materials" => {
            if !asset["title"].is_string() {
                return Err(invalid("材料缺少名称"));
            }
        }
        "profiles" => {
            if !["fixed", "calculate"].contains(&asset["mode"].as_str().unwrap_or(""))
                || !asset["values"].is_object()
                || !asset["operation"].is_string()
                || !asset["material_id"].is_string()
                || !(asset["tool_id"].is_string() ^ asset["type_id"].is_string())
                || !["per_minute", "per_revolution"]
                    .contains(&asset["feed_mode"].as_str().unwrap_or(""))
            {
                return Err(invalid(
                    "加工预置需明确模式、刀具或类型、材料、工序和进给单位",
                ));
            }
            for (field, value) in asset["values"].as_object().unwrap() {
                finite(value, field)?;
            }
        }
        "standards" => {
            if !asset["tables"].is_object() {
                return Err(invalid("工艺标准缺少 tables"));
            }
        }
        _ => return Err(invalid("未知 NC 参数资产类型")),
    }
    Ok(())
}
pub fn resolve_cutting(input: &Value, snapshot: &Value) -> PluginResult<Value> {
    let selection = input.get("selection").cloned().unwrap_or_else(|| json!({}));
    if !selection.is_object()
        || selection.get("slots").is_some_and(|v| !v.is_object())
        || selection.get("preset_id").is_some_and(|v| !v.is_string())
    {
        return Err(invalid(
            "参数选择与刀具槽位必须使用对象，预置标识必须使用字符串",
        ));
    }
    if selection
        .as_object()
        .unwrap()
        .keys()
        .any(|key| !["preset_id", "slots"].contains(&key.as_str()))
    {
        return Err(invalid("未支持的参数选择字段"));
    }
    let mut resolved = json!({});
    let mut needs_confirmation = false;
    for (slot, choice) in selection["slots"].as_object().cloned().unwrap_or_default() {
        if choice.as_object().is_none_or(|map| {
            map.keys().any(|key| {
                ![
                    "tool_id",
                    "material_id",
                    "operation",
                    "profile_id",
                    "manual",
                    "mode",
                    "feed_mode",
                    "inputs",
                    "overrides",
                    "confirmed_for",
                ]
                .contains(&key.as_str())
            })
        }) {
            return Err(invalid(format!("{slot}: 刀具选择字段无效")));
        }
        for field in [
            "tool_id",
            "material_id",
            "operation",
            "profile_id",
            "mode",
            "feed_mode",
            "confirmed_for",
        ] {
            if choice.get(field).is_some_and(|v| !v.is_string()) {
                return Err(invalid(format!("{slot}/{field}: 必须使用字符串")));
            }
        }
        if choice.get("inputs").is_some_and(|v| !v.is_object())
            || choice.get("overrides").is_some_and(|v| !v.is_object())
            || choice.get("manual").is_some_and(|v| !v.is_boolean())
        {
            return Err(invalid(format!(
                "{slot}: 加工输入、调整值或手动模式类型无效"
            )));
        }
        for (field, value) in choice["inputs"].as_object().into_iter().flatten() {
            if ![
                "rpm",
                "feed",
                "cutting_speed",
                "feed_per_tooth",
                "feed_per_revolution",
                "work_diameter",
                "semi_finish_feed",
            ]
            .contains(&field.as_str())
            {
                return Err(invalid(format!("{slot}: 未支持的加工输入 {field}")));
            }
            finite(value, field)?;
        }
        let tool_id = choice["tool_id"]
            .as_str()
            .ok_or_else(|| invalid(format!("{slot}: 缺少刀具选择")))?;
        let tool = record(snapshot, "tools", tool_id)?;
        validate_asset("tools", tool_id, tool)?;
        let type_id = tool["type_id"].as_str().unwrap();
        let tool_type = record(snapshot, "tool_types", type_id)?;
        validate_asset("tool_types", type_id, tool_type)?;
        let material_id = choice["material_id"]
            .as_str()
            .ok_or_else(|| invalid("缺少材料选择"))?;
        let material = record(snapshot, "materials", material_id)?;
        validate_asset("materials", material_id, material)?;
        let operation = choice["operation"]
            .as_str()
            .ok_or_else(|| invalid("缺少加工工序"))?;
        if !["milling", "drilling", "turning"].contains(&operation) {
            return Err(invalid("未知切削计算工序"));
        }
        if choice["manual"] == true && choice["profile_id"].is_string() {
            return Err(invalid("加工预置与手动模式互斥"));
        }
        if tool_type["operation"].as_str() != Some(operation) && tool_type["operation"] != "legacy"
        {
            return Err(invalid("刀具类型不适用于所选加工工序"));
        }
        let mut profile = json!({});
        let mut inputs = json!({});
        if let Some(profile_id) = choice["profile_id"].as_str() {
            profile = record(snapshot, "profiles", profile_id)?.clone();
            validate_asset("profiles", profile_id, &profile)?;
            if profile["material_id"] != material_id
                || profile["operation"] != operation
                || !(profile["tool_id"] == tool_id || profile["type_id"] == type_id)
            {
                return Err(invalid("加工预置与刀具、材料或工序不匹配"));
            }
            inputs = profile["values"].clone();
        } else if choice["manual"] != true {
            return Err(invalid("请选择加工预置或显式启用手动模式"));
        }
        if let Some(values) = choice["inputs"].as_object() {
            if choice["profile_id"].is_string() && values.keys().any(|key| key != "work_diameter") {
                return Err(invalid(
                    "预置加工值只能通过 rpm/feed 人工调整；车削加工直径可由本次输入提供",
                ));
            }
            inputs.as_object_mut().unwrap().extend(values.clone());
        }
        let mode = profile["mode"]
            .as_str()
            .or_else(|| choice["mode"].as_str())
            .ok_or_else(|| invalid("缺少计算模式"))?;
        let feed_mode = profile["feed_mode"]
            .as_str()
            .or_else(|| choice["feed_mode"].as_str())
            .ok_or_else(|| invalid("缺少进给单位"))?;
        if !["per_minute", "per_revolution"].contains(&feed_mode) {
            return Err(invalid("无效进给单位"));
        }
        let mut rpm = if mode == "fixed" {
            finite(&inputs["rpm"], "rpm")?
        } else if mode == "calculate" {
            let diameter = if operation == "turning" {
                finite(&inputs["work_diameter"], "work_diameter")?
            } else {
                finite(&tool["attributes"]["diameter"], "diameter")?
            };
            (finite(&inputs["cutting_speed"], "cutting_speed")? * 1000.
                / (std::f64::consts::PI * diameter))
                .round_ties_even()
        } else {
            return Err(invalid("无效计算模式"));
        };
        let overrides = choice["overrides"].as_object().cloned().unwrap_or_default();
        let confirmed_for = signature(
            &json!({"tool":tool,"type":tool_type,"material":material,"profile":profile,"inputs":inputs,"operation":operation,"feed_mode":feed_mode,"overrides":overrides}),
        );
        if overrides
            .keys()
            .any(|key| !["rpm", "feed"].contains(&key.as_str()))
        {
            return Err(invalid("只允许覆盖推荐转速与进给"));
        }
        if let Some(value) = overrides.get("rpm") {
            rpm = finite(value, "rpm")?;
        }
        if rpm <= 0. || rpm.fract() != 0. || rpm > 9_007_199_254_740_992. {
            return Err(invalid("转速必须是可精确表示的正整数"));
        }
        let mut feed = if mode == "fixed" {
            let base = finite(&inputs["feed"], "feed")?;
            if feed_mode == "per_minute" {
                let value = base * rpm / finite(&inputs["rpm"], "rpm")?;
                (value * 1000.).round_ties_even() / 1000.
            } else {
                base
            }
        } else {
            let per_rev = if operation == "milling" {
                let teeth = finite(&tool["attributes"]["teeth"], "teeth")?;
                if teeth.fract() != 0. {
                    return Err(invalid("齿数必须是整数"));
                }
                teeth * finite(&inputs["feed_per_tooth"], "feed_per_tooth")?
            } else {
                finite(&inputs["feed_per_revolution"], "feed_per_revolution")?
            };
            let feed = if feed_mode == "per_minute" {
                rpm * per_rev
            } else {
                per_rev
            };
            (feed * 1000.).round_ties_even() / 1000.
        };
        if let Some(value) = overrides.get("feed") {
            feed = finite(value, "feed")?;
        }
        if !feed.is_finite() || feed <= 0. {
            return Err(invalid("进给计算结果必须有限且大于零"));
        }
        let pending = !overrides.is_empty() && choice["confirmed_for"] != confirmed_for;
        needs_confirmation |= pending;
        if let Some(max) = input["max_spindle_rpm"].as_f64() {
            if !max.is_finite() || max <= 0. || rpm > max {
                return Err(invalid("转速超过机床上限或机床上限无效"));
            }
        }
        let mut values = inputs.clone();
        values["rpm"] = json!(rpm);
        values["feed"] = json!(feed);
        resolved[&slot] = json!({"attributes":tool["attributes"],"values":values,"tool_id":tool_id,"profile_id":choice["profile_id"],"material_id":material_id,"operation":operation,"feed_mode":feed_mode,"feed_unit":if feed_mode=="per_minute"{"mm/min"}else{"mm/rev"},"confirmed_for":confirmed_for,"needs_confirmation":pending,"origins":{"attributes":format!("tool:{tool_id}"),"values":if overrides.is_empty(){"recommended"}else{"manual"}}});
    }
    Ok(json!({"slots":resolved,"needs_confirmation":needs_confirmation,"snapshot":snapshot}))
}
impl NcFoundationPlugin {
    pub fn new(root: &Path) -> PluginResult<Self> {
        Ok(Self {
            library: Library::new(
                root,
                &[
                    ("tool_types", "parameters/nc/tool_types"),
                    ("tools", "parameters/nc/tools"),
                    ("materials", "parameters/nc/materials"),
                    ("profiles", "parameters/nc/profiles"),
                    ("standards", "parameters/nc/standards"),
                ],
            )?,
            machines: JsonStore::new(&root.join("machines")).map_err(asset_error)?,
        })
    }
    pub fn definition() -> PluginDescriptor {
        let mut d = PluginDescriptor::builtin("nc-parameters");
        d.requires = [
            "template.inspect",
            "nc.generate",
            "parameter.resolve",
            "parameter.snapshot",
            "parameter.dependencies.check",
        ]
        .iter()
        .map(|s| Requirement::new(s))
        .collect();
        d.provides = [
            "nc.parameters.resolve",
            "nc.foundation.freeze",
            "nc.managed.resolve",
            "nc.managed.render",
            "nc.foundation.check",
            "nc.output.finalize",
        ]
        .iter()
        .map(|s| Service::new(s, s))
        .collect();
        d.asset_collections = vec![AssetCollection {
            id: "nc-parameters".into(),
            export_action: "nc.parameter.collection.export".into(),
            import_action: "nc.parameter.collection.import".into(),
        }];
        d.panels = vec![Panel {
            id: "nc-parameters".into(),
            title: "刀具与加工参数".into(),
            view: "nc.parameters".into(),
            actions: Self::action_definitions()
                .iter()
                .map(|a| a.id.clone())
                .collect(),
        }];
        d
    }
    pub fn action_definitions() -> Vec<ActionDescriptor> {
        let mut actions = library_actions(
            "nc.parameter",
            &["tool_types", "tools", "materials", "profiles", "standards"],
        );
        actions.push(ActionDescriptor::new("nc.parameters.resolve","预览刀具加工参数",json!({"type":"object","properties":{"selection":{"type":"object"},"snapshot":{"type":"object"},"max_spindle_rpm":{"type":"number"}},"additionalProperties":false})));
        actions.push(ActionDescriptor::new("nc.parameters.match","筛选加工预置",json!({"type":"object","required":["tool_id","material_id","operation"],"properties":{"tool_id":{"type":"string"},"material_id":{"type":"string"},"operation":{"type":"string"}},"additionalProperties":false})));
        actions.push(ActionDescriptor::new("nc.operation.prepare","从原始参数准备工艺",json!({"type":"object","required":["operation","inputs"],"properties":{"operation":{"type":"object"},"inputs":{"type":"object"},"standard":{"type":"object"}},"additionalProperties":false})));
        actions.push(ActionDescriptor::new("nc.foundation.check","核对生成资源依赖",json!({"type":"object","required":["managed_snapshot"],"properties":{"managed_snapshot":{"type":"object"}},"additionalProperties":false})));
        actions.push(ActionDescriptor::new("nc.output.finalize","处理组合文件输出",json!({"type":"object","required":["text","name","machine"],"properties":{"text":{"type":"string"},"name":{"type":"string"},"machine":{"type":"object"},"options":{"type":"object"}},"additionalProperties":false})));
        let schema = json!({"type":"object","properties":{"source":{"type":"string"},"template":{"type":"string"},"templates":{"type":"object"},"snapshot":{"type":"object"},"managed_snapshot":{"type":"object"},"operation":{"type":"object"},"operation_inputs":{"type":"object"},"params":{"type":"object"},"bindings":{"type":"object"},"schema":{"type":["object","boolean"]},"defaults":{"type":"object"},"specs":{"type":"array"},"parameter_selection":{"type":"object"},"machine":{"type":"object"},"machine_id":{"type":"string"},"options":{"type":"object"},"output_name":{"type":"string"},"trim_blocks":{"type":"boolean"},"lstrip_blocks":{"type":"boolean"}},"anyOf":[{"required":["source"]},{"required":["template"]},{"required":["snapshot"]},{"required":["managed_snapshot"]}],"additionalProperties":false});
        actions.push(ActionDescriptor::new(
            "nc.foundation.freeze",
            "冻结受管 NC 输入",
            schema.clone(),
        ));
        let mut generate =
            ActionDescriptor::new("nc.generate.managed", "从原始参数生成 NC", schema);
        generate.ui_schema = Some(
            json!({"document_input":{"title":"受管 NC 程序","description":"解析共享参数并在后端完成工艺计算。","bindings":{"source":"source","template":"id","operation_inputs":"context","schema":"schema","defaults":"defaults","operation":"metadata.nc.operation","bindings":"metadata.parameters.bindings","parameter_selection":"parameter_selection","options":"metadata.nc.options","machine_id":"metadata.nc.machine_id","trim_blocks":"metadata.nc.render_options.trim_blocks","lstrip_blocks":"metadata.nc.render_options.lstrip_blocks"}}}),
        );
        actions.push(generate);
        actions.retain(|a| a.id != "nc.operation.prepare" && a.id != "nc.generate.managed");
        actions.push(ActionDescriptor::new("nc.managed.resolve", "解析冻结工艺输入", json!({"type":"object","required":["managed_snapshot"],"properties":{"managed_snapshot":{"type":"object"}},"additionalProperties":false})));
        actions.push(ActionDescriptor::new("nc.managed.render", "渲染已准备工艺", json!({"type":"object","required":["managed_snapshot","resolved","prepared"],"properties":{"managed_snapshot":{"type":"object"},"resolved":{"type":"object"},"prepared":{"type":"object"}},"additionalProperties":false})));
        actions
    }
    fn freeze(&self, input: &Value, ctx: &Context) -> PluginResult<Value> {
        if let Some(frozen) = input.get("managed_snapshot") {
            if input.as_object().is_none_or(|o| o.len() != 1)
                || frozen["format_version"] != 1
                || !frozen["input"].is_object()
                || !frozen["template_snapshot"].is_object()
                || frozen["parameter_snapshot"]["version"] != 1
                || frozen["nc_snapshot"]["version"] != 1
                || !frozen["nc_snapshot"]["records"].is_array()
            {
                return Err(invalid(
                    "受管快照不能与其它输入混用，且必须包含完整的 v1 资源",
                ));
            }
            if frozen["input"].get("managed_snapshot").is_some() {
                return Err(invalid("冻结输入不能嵌套受管快照"));
            }
            let descriptor = Self::action_definitions()
                .into_iter()
                .find(|d| d.id == "nc.foundation.freeze")
                .unwrap();
            validate_schema(
                &descriptor.input_schema,
                &frozen["input"],
                "managed_validation",
            )?;
            return Ok(frozen.clone());
        }
        if input.get("machine").is_some() && input.get("machine_id").is_some() {
            return Err(invalid("机床对象和机床标识互斥"));
        }
        let mut inspection = json!({"include_snapshot":true});
        for key in ["source", "template", "templates", "snapshot"] {
            if let Some(v) = input.get(key) {
                inspection[key] = v.clone();
            }
        }
        let templates = ctx.call("template.inspect", 1, inspection)?.data["snapshot"].clone();
        let machine = if let Some(v) = input.get("machine") {
            serde_json::to_value(crate::validate_machine_content(v)?)?
        } else if let Some(id) = input["machine_id"].as_str() {
            serde_json::to_value(crate::validate_machine(
                id,
                &self.machines.read(id).map_err(asset_error)?.0,
            )?)?
        } else {
            json!({"id":"generic","vendor":"Example","model":"Virtual","config":{}})
        };
        let mut nc_snapshot = self.library.capture()?;
        if !nc_snapshot["records"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["group"] == "standards" && r["name"] == "v3-grooves")
        {
            nc_snapshot["records"].as_array_mut().unwrap().push(json!({"group":"standards","name":"v3-grooves","asset":serde_json::from_str::<Value>(include_str!("operation_standards.json"))?}));
        }
        Ok(
            json!({"format_version":1,"input":input,"template_snapshot":templates,"parameter_snapshot":ctx.call("parameter.snapshot",1,json!({}))?.data,"nc_snapshot":nc_snapshot,"machine":machine}),
        )
    }
    fn resolve_input(&self, frozen: &Value, _ctx: &Context) -> PluginResult<Value> {
        let input = &frozen["input"];
        let snapshot = &frozen["template_snapshot"];
        let asset = &snapshot["asset"];
        let declared = asset["metadata"]["nc"].get("operation");
        if input
            .get("operation")
            .is_some_and(|op| declared.is_some_and(|d| op != d))
        {
            return Err(invalid("模板声明的工艺处理器不能被请求替换"));
        }
        let op = declared
            .or_else(|| input.get("operation"))
            .cloned()
            .unwrap_or_else(|| json!({"id":"direct","version":1}));
        if input.get("params").is_some() && input.get("operation_inputs").is_some() {
            return Err(invalid("params 与 operation_inputs 互斥"));
        }
        let raw = input
            .get("operation_inputs")
            .or_else(|| input.get("params"))
            .cloned()
            .unwrap_or_else(|| json!({}));
        for path in asset["metadata"]["nc"]["result_inputs"]
            .as_array()
            .into_iter()
            .flatten()
        {
            let path = path.as_str().ok_or_else(|| invalid("结果输入绑定无效"))?;
            if raw.pointer(path) != frozen["bound_programs"].get(path)
                || raw.pointer(path).is_none()
            {
                return Err(invalid("主程序片段必须绑定本次组合任务的前置生成结果"));
            }
        }
        let mut resolve_input = json!({"snapshot":frozen["parameter_snapshot"],"bindings":nctool_plugin_parameters::effective_bindings(&asset["metadata"]["parameters"]["bindings"],input.get("bindings"))?,"values":raw,"defaults":input.get("defaults").or_else(||asset.get("defaults")).cloned().unwrap_or_else(||json!({}))});
        resolve_input["defer_validation"] = json!(true);
        if let Some(schema) = asset.get("schema") {
            resolve_input["schema"] = schema.clone();
        }
        // Specs initialize raw direct inputs below shared definition defaults.
        if op["id"] == "direct" {
            let mut defaults = json!({});
            for spec in input
                .get("specs")
                .or_else(|| asset["metadata"]["nc"].get("specs"))
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                if let (Some(name), Some(value)) = (spec["name"].as_str(), spec.get("default")) {
                    defaults[name] = value.clone();
                }
            }
            resolve_input["fallback_defaults"] = defaults;
        }
        let selection = input
            .get("parameter_selection")
            .or_else(|| asset["metadata"]["parameters"].get("selection"))
            .cloned()
            .unwrap_or_else(|| json!({}));
        // The chosen preset is frozen with the definitions. Resolution never rereads it.
        if let Some(preset) = selection.get("preset_id") {
            resolve_input["preset_id"] = preset.clone();
            resolve_input.as_object_mut().unwrap().remove("snapshot");
        }
        let mut resolved =
            nctool_plugin_parameters::resolve(&resolve_input, &frozen["parameter_snapshot"])?;
        let mut inputs = resolved["values"].clone();
        let declared_specs = asset["metadata"]["nc"].get("specs");
        if input
            .get("specs")
            .is_some_and(|provided| declared_specs.is_some_and(|declared| provided != declared))
        {
            return Err(invalid("请求 specs 不能替换模板的 NC 约束"));
        }
        for spec in input
            .get("specs")
            .or(declared_specs)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if op["id"] != "direct" && spec.get("derive").is_some() {
                return Err(invalid("工艺派生值仅由指定处理器计算"));
            }
            if op["id"] == "direct" {
                if let Some(name) = spec["name"].as_str() {
                    if let Some(definition) = resolved["definitions"].get(format!("/{name}")) {
                        if let Some(unit) = spec.get("unit") {
                            if definition.get("unit") != Some(unit) {
                                return Err(invalid("NC specs 与共享定义单位不兼容"));
                            }
                        }
                    }
                }
            }
        }
        let mut cutting_request = json!({"selection":selection});
        if let Some(max) = frozen["machine"]["config"].get("max_spindle_rpm") {
            cutting_request["max_spindle_rpm"] = if let Some(s) = max.as_str() {
                json!(s.parse::<f64>().map_err(|_| invalid("机床转速上限无效"))?)
            } else {
                max.clone()
            };
        }
        let cutting = resolve_cutting(&cutting_request, &frozen["nc_snapshot"])?;
        if cutting["needs_confirmation"] == true {
            return Err(PluginError::new(
                "confirmation_required",
                "加工条件已变化，请确认人工调整或恢复推荐值",
            ));
        }
        let tool_bindings = asset["metadata"]["parameters"]["tool_bindings"]
            .as_object()
            .cloned()
            .unwrap_or_default();
        for (target, binding) in tool_bindings {
            let slot = binding["slot"]
                .as_str()
                .ok_or_else(|| invalid("刀具绑定缺少 slot"))?;
            let field = binding["field"]
                .as_str()
                .ok_or_else(|| invalid("刀具绑定缺少 field"))?;
            if cutting["slots"].get(slot).is_none() {
                continue;
            }
            let value = cutting["slots"][slot][if binding["kind"] == "attribute" {
                "attributes"
            } else {
                "values"
            }]
            .get(field);
            let Some(value) = value else {
                if binding["optional"] == true && raw.pointer(&target).is_none() {
                    continue;
                }
                return Err(invalid(format!("{slot}/{field}: 缺少刀具数据")));
            };
            if raw.pointer(&target).is_some_and(|v| v != value) {
                return Err(invalid(format!(
                    "{target}: 后端确定的刀具事实或加工值不允许覆盖，请在刀具槽位调整并确认"
                )));
            }
            set_pointer(&mut inputs, &target, value.clone())?;
            resolved["origins"][&target] = json!(if binding["kind"] == "attribute" {
                "tool_fact"
            } else {
                cutting["slots"][slot]["origins"]["values"]
                    .as_str()
                    .unwrap_or("recommended")
            });
        }
        resolved["values"] = inputs.clone();
        if let Some(schema) = input.get("schema") {
            nctool_plugin_parameters::resolve(
                &json!({"bindings":resolve_input["bindings"],"values":inputs,"schema":schema,"defer_validation":true}),
                &frozen["parameter_snapshot"],
            )?;
        }
        if op["id"] != "direct" {
            validate_schema(
                &operation::input_schema(op["id"].as_str().unwrap_or("")),
                &inputs,
                "operation_validation",
            )?;
        }
        let mut raw_validation = inputs.clone();
        for path in asset["metadata"]["nc"]["result_inputs"]
            .as_array()
            .into_iter()
            .flatten()
        {
            if let Some(field) = path.as_str().and_then(|p| p.strip_prefix('/')) {
                raw_validation.as_object_mut().unwrap().remove(field);
            }
        }
        for schema in [asset.get("schema"), input.get("schema")]
            .into_iter()
            .flatten()
        {
            validate_schema(schema, &raw_validation, "operation_validation")?;
        }
        for (pointer, definition) in resolved["definitions"].as_object().unwrap() {
            if let Some(value) = inputs.pointer(pointer) {
                validate_schema(&definition["schema"], value, "parameter_validation")?;
            }
        }
        let standard_id = op["standard_id"].as_str().unwrap_or("v3-grooves");
        let standard = record(&frozen["nc_snapshot"], "standards", standard_id)?.clone();
        Ok(
            json!({"operation":op,"inputs":inputs,"standard":standard,"cutting":cutting,"parameters":resolved,"cutting_request":cutting_request}),
        )
    }
    fn render(
        &self,
        frozen: Value,
        resolved_input: Value,
        prepared: Value,
        ctx: &Context,
    ) -> PluginResult<ActionResult> {
        let frozen = self.freeze(&json!({"managed_snapshot":frozen}), ctx)?;
        let contexts = prepared["contexts"]
            .as_array()
            .ok_or_else(|| invalid("工艺准备结果缺少 contexts 数组"))?;
        if contexts.is_empty() && !prepared["skip_text"].is_string() {
            return Err(invalid("工艺准备结果没有程序上下文"));
        }
        for context in contexts {
            let _: crate::model::ParameterSet = serde_json::from_value(context.clone())?;
        }
        if resolved_input["operation"]["version"] != 1
            || !resolved_input["operation"]["id"].is_string()
            || !resolved_input["inputs"].is_object()
            || !resolved_input["cutting"].is_object()
            || !resolved_input["cutting_request"].is_object()
        {
            return Err(invalid("冻结工艺解析结果不完整"));
        }
        let definitions = resolved_input["parameters"]["definitions"]
            .as_object()
            .ok_or_else(|| invalid("冻结工艺解析结果缺少共享定义"))?;
        let input = &frozen["input"];
        let snapshot = &frozen["template_snapshot"];
        let asset = &snapshot["asset"];
        let op = &resolved_input["operation"];
        let inputs = &resolved_input["inputs"];
        let resolved = &resolved_input["parameters"];
        let cutting = &resolved_input["cutting"];
        let cutting_request = &resolved_input["cutting_request"];
        if let Some(value) = cutting_request.get("max_spindle_rpm") {
            let limit = finite(value, "max_spindle_rpm")?;
            for context in contexts {
                if let Some(rpm) = context.get("spindle_speed") {
                    if finite(rpm, "spindle_speed")? > limit {
                        return Err(invalid("工艺计算转速超过机床上限"));
                    }
                }
            }
            for (path, definition) in definitions {
                if definition["unit"] == "rpm" {
                    if let Some(value) = inputs.pointer(path) {
                        if operation::number(value, path)? > limit {
                            return Err(invalid(format!("{path}: 转速超过机床上限")));
                        }
                    }
                }
            }
        }
        let mut texts = vec![];
        let mut artifacts = vec![];
        let mut end_line = 0;
        let mut diagnostics = vec![];
        let output_name = input["output_name"]
            .as_str()
            .or_else(|| asset["metadata"]["output_filename"].as_str())
            .unwrap_or("program.nc");
        if output_name.trim().is_empty()
            || output_name.contains(['/', '\\', ':'])
            || [".", ".."].contains(&output_name)
            || output_name.chars().any(char::is_control)
        {
            return Err(invalid("输出文件名必须为有效的单个路径段"));
        }
        if let Some(skip) = prepared["skip_text"].as_str() {
            let mut options = asset["metadata"]["nc"]["options"]
                .as_object()
                .cloned()
                .unwrap_or_default();
            options.extend(input["options"].as_object().cloned().unwrap_or_default());
            for channel in [1, 2] {
                let result=self.invoke("nc.output.finalize",json!({"name":format!("channel-{channel}-{output_name}"),"text":skip,"machine":frozen["machine"],"options":options}),ctx)?;
                end_line = result.data["end_line_number"].as_u64().unwrap_or(0);
                diagnostics.extend(result.diagnostics);
                texts.push(result.artifacts[0].text.clone());
                artifacts.extend(result.artifacts);
            }
        }
        for (index, context) in contexts.iter().enumerate() {
            ctx.cancellation.check()?;
            // The new native-plan transport field is not part of legacy closed schemas.
            let mut context = context.clone();
            if ["sync.left", "sync.right", "groove"].contains(&op["id"].as_str().unwrap_or(""))
                && asset["metadata"]["nc"]["trajectory_plan_version"] != 1
            {
                context
                    .as_object_mut()
                    .unwrap()
                    .remove("trajectory_program");
            }
            // Raw-input schema must not be applied to the generated flat context.
            let context_schema = asset["metadata"]["nc"]
                .get("context_schema")
                .cloned()
                .unwrap_or_else(|| {
                    if op["id"] == "direct" {
                        input
                            .get("schema")
                            .or_else(|| asset.get("schema"))
                            .cloned()
                            .unwrap_or(json!(true))
                    } else {
                        json!(true)
                    }
                });
            let mut request = json!({"snapshot":snapshot,"params":context,"schema":context_schema,"defaults":{},"machine":frozen["machine"]});
            for key in ["specs", "options", "trim_blocks", "lstrip_blocks"] {
                if let Some(v) = input.get(key) {
                    request[key] = v.clone();
                }
            }
            let result = ctx.call("nc.generate", 1, request)?;
            let mut text = result.data["text"]
                .as_str()
                .ok_or_else(|| invalid("NC 服务未返回正文"))?
                .to_owned();
            // V3 groove generators strip the rendered fragment before composition.
            if matches!(op["id"].as_str(), Some("groove" | "circlip")) {
                text = format!("{}\n", text.trim());
            }
            end_line = result.data["end_line_number"].as_u64().unwrap_or(0);
            diagnostics.extend(result.diagnostics);
            let name = output_name;
            let name = if contexts.len() > 1 {
                format!("channel-{}-{name}", index + 1)
            } else {
                name.into()
            };
            if name.contains(['/', '\\']) || name == ".." {
                return Err(invalid("输出文件名必须为单个路径段"));
            }
            artifacts.push(Artifact {
                name,
                media_type: "text/plain".into(),
                text: text.clone(),
            });
            texts.push(text);
        }
        if artifacts.is_empty() {
            for (index, text) in texts.iter().enumerate() {
                artifacts.push(Artifact {
                    name: format!("channel-{}.nc", index + 1),
                    media_type: "text/plain".into(),
                    text: text.clone(),
                });
            }
        }
        for warning in prepared["warnings"].as_array().into_iter().flatten() {
            if let Some(message) = warning.as_str() {
                diagnostics.push(Diagnostic::warning("process_warning", message));
            }
        }
        Ok(ActionResult {
            data: json!({"text":texts.join("\n"),"channels":texts,"end_line_number":end_line,"next_sync_num":prepared["next_sync_num"],"parameters":resolved,"cutting":cutting,"prepared":prepared,"replay_input":{"managed_snapshot":frozen}}),
            artifacts,
            diagnostics,
        })
    }
}
impl Plugin for NcFoundationPlugin {
    fn descriptor(&self) -> PluginDescriptor {
        Self::definition()
    }
    fn actions(&self) -> Vec<ActionDescriptor> {
        Self::action_definitions()
    }
    fn invoke(&self, action: &str, input: Value, ctx: &Context) -> PluginResult<ActionResult> {
        match action {
            "nc.output.finalize" => {
                let name = input["name"].as_str().unwrap();
                if name.is_empty() || name.contains(['/', '\\']) || name == ".." {
                    return Err(invalid("输出名称无效"));
                }
                let mut result=ctx.call("nc.generate",1,json!({"source":"{{ program }}","params":{"program":input["text"]},"schema":{"type":"object","properties":{"program":{"type":"string"}},"required":["program"]},"machine":input["machine"],"options":input.get("options").cloned().unwrap_or(json!({}))}))?;
                result.artifacts = vec![Artifact {
                    name: name.into(),
                    media_type: "text/plain".into(),
                    text: result.data["text"].as_str().unwrap().into(),
                }];
                Ok(result)
            }
            "nc.foundation.check" => {
                let frozen = self.freeze(&input, ctx)?;
                let mut changes = ctx
                    .call(
                        "parameter.dependencies.check",
                        1,
                        json!({"snapshot":frozen["parameter_snapshot"]}),
                    )?
                    .data["changes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect::<Vec<_>>();
                let mut checked = frozen["nc_snapshot"].clone();
                for row in checked["records"].as_array().unwrap() {
                    if row["group"] == "standards"
                        && row["name"] == "v3-grooves"
                        && row.get("fingerprint").is_none()
                    {
                        match self.library.read("standards", "v3-grooves") {
                            Ok((asset, _)) if asset != row["asset"] => {
                                changes.push("standards/v3-grooves".into())
                            }
                            Ok(_) => {}
                            Err(e) if e.code == "not_found" => {}
                            Err(e) => return Err(e),
                        }
                    }
                }
                checked["records"].as_array_mut().unwrap().retain(|row| {
                    !(row["group"] == "standards"
                        && row["name"] == "v3-grooves"
                        && row.get("fingerprint").is_none())
                });
                changes.extend(self.library.changed(&checked)?);
                if let Some(id) = frozen["input"]["machine_id"].as_str() {
                    match self.machines.read(id) {
                        Ok((asset, _))
                            if serde_json::to_value(crate::validate_machine(id, &asset)?)?
                                == frozen["machine"] => {}
                        _ => changes.push(format!("machines/{id}")),
                    }
                }
                if frozen["input"].get("template").is_some() {
                    let inspection = ctx.call(
                        "template.inspect",
                        1,
                        json!({"template":frozen["input"]["template"],"include_snapshot":true}),
                    )?;
                    let current = &inspection.data["snapshot"];
                    for (name, source) in frozen["template_snapshot"]["templates"]
                        .as_object()
                        .into_iter()
                        .flatten()
                    {
                        if name
                            == frozen["template_snapshot"]["template"]
                                .as_str()
                                .unwrap_or("")
                            && frozen["input"].get("source").is_some()
                        {
                            continue;
                        }
                        if current["templates"].get(name) != Some(source) {
                            changes.push(format!("templates/{name}"));
                        }
                    }
                    if current["asset"] != frozen["template_snapshot"]["asset"] {
                        changes.push("template_metadata".into());
                    }
                }
                Ok(ActionResult::data(
                    json!({"current":changes.is_empty(),"changes":changes}),
                ))
            }
            "nc.foundation.freeze" => self.freeze(&input, ctx).map(ActionResult::data),
            "nc.managed.resolve" => {
                let frozen = self.freeze(&input, ctx)?;
                self.resolve_input(&frozen, ctx).map(ActionResult::data)
            }
            "nc.managed.render" => self.render(
                input["managed_snapshot"].clone(),
                input["resolved"].clone(),
                input["prepared"].clone(),
                ctx,
            ),
            "nc.parameters.resolve" => {
                let snapshot = input
                    .get("snapshot")
                    .cloned()
                    .map(Ok)
                    .unwrap_or_else(|| self.library.capture())?;
                resolve_cutting(&input, &snapshot).map(ActionResult::data)
            }
            "nc.parameters.match" => {
                let snapshot = self.library.capture()?;
                let tool = record(&snapshot, "tools", input["tool_id"].as_str().unwrap())?;
                let mut items = snapshot["records"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|r| r["group"] == "profiles")
                    .filter(|r| {
                        let a = &r["asset"];
                        a["material_id"] == input["material_id"]
                            && a["operation"] == input["operation"]
                            && (a["tool_id"] == input["tool_id"] || a["type_id"] == tool["type_id"])
                    })
                    .cloned()
                    .collect::<Vec<_>>();
                items.sort_by_key(|r| r["asset"]["tool_id"] != input["tool_id"]);
                Ok(ActionResult::data(json!({"items":items})))
            }
            _ => self.library.invoke(
                action.rsplit('.').next().unwrap_or_default(),
                &input,
                validate_asset,
            ),
        }
    }
}
