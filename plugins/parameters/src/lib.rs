//! Domain-neutral parameter contracts, explicit presets and frozen resolution.
use nctool_assets::{GroupGuard, JsonStore, WriteError};
use nctool_plugin_sdk::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

pub fn asset_error(e: WriteError) -> PluginError {
    let code = match e {
        WriteError::Conflict { .. } | WriteError::LockBusy { .. } => "conflict",
        WriteError::NotFound(_) => "not_found",
        _ => "asset_error",
    };
    PluginError::new(code, e.to_string())
}
/// The library only owns generic JSON storage; callers validate their own domain.
pub struct Library {
    pub root: PathBuf,
    stores: BTreeMap<String, JsonStore>,
}
impl Library {
    pub fn new(root: &Path, groups: &[(&str, &str)]) -> PluginResult<Self> {
        Ok(Self {
            root: root.into(),
            stores: groups
                .iter()
                .map(|(group, dir)| {
                    Ok((
                        group.to_string(),
                        JsonStore::new(&root.join(dir)).map_err(asset_error)?,
                    ))
                })
                .collect::<PluginResult<_>>()?,
        })
    }
    pub fn read(&self, group: &str, name: &str) -> PluginResult<(Value, String)> {
        self.stores
            .get(group)
            .ok_or_else(|| PluginError::new("invalid_input", "未知资产类型"))?
            .read(name)
            .map_err(asset_error)
    }
    pub fn changed(&self, snapshot: &Value) -> PluginResult<Vec<String>> {
        if snapshot["version"] != 1 {
            return Err(PluginError::new("snapshot_version", "不支持的参数快照版本"));
        }
        let mut changed = vec![];
        for row in snapshot["records"]
            .as_array()
            .ok_or_else(|| PluginError::new("snapshot_invalid", "缺少快照记录"))?
        {
            let group = row["group"]
                .as_str()
                .ok_or_else(|| PluginError::new("snapshot_invalid", "缺少资产类型"))?;
            let id = row["name"]
                .as_str()
                .ok_or_else(|| PluginError::new("snapshot_invalid", "缺少资产名称"))?;
            match self.read(group, id) {
                Ok((asset, _)) if asset == row["asset"] => {}
                Ok(_) => changed.push(format!("{group}/{id}")),
                Err(e) if e.code == "not_found" => changed.push(format!("{group}/{id}")),
                Err(e) => return Err(e),
            }
        }
        Ok(changed)
    }
    pub fn capture(&self) -> PluginResult<Value> {
        let _guard = GroupGuard::acquire(&self.root).map_err(asset_error)?;
        let stores = self
            .stores
            .iter()
            .map(|(g, s)| (g.as_str(), s))
            .collect::<Vec<_>>();
        let mut value = nctool_assets::export_collection(&stores).map_err(asset_error)?;
        for record in value["records"].as_array_mut().unwrap() {
            record["fingerprint"] = json!(
                self.read(
                    record["group"].as_str().unwrap(),
                    record["name"].as_str().unwrap()
                )?
                .1
            );
        }
        Ok(value)
    }
    pub fn invoke(
        &self,
        verb: &str,
        input: &Value,
        validate: fn(&str, &str, &Value) -> PluginResult<()>,
    ) -> PluginResult<ActionResult> {
        let _guard = GroupGuard::acquire(&self.root).map_err(asset_error)?;
        let stores = self
            .stores
            .iter()
            .map(|(g, s)| (g.as_str(), s))
            .collect::<Vec<_>>();
        if verb == "export" {
            return nctool_assets::export_collection(&stores)
                .map(ActionResult::data)
                .map_err(asset_error);
        }
        if verb == "import" {
            if let Some(receipt) = input.get("rollback") {
                nctool_assets::rollback_collection(&stores, receipt).map_err(asset_error)?;
                return Ok(ActionResult::data(json!({"rolled_back":true})));
            }
            return nctool_assets::import_collection(
                &stores,
                &input["collection"],
                input["validate_only"].as_bool().unwrap_or(false),
                |g, n, a| validate(g, n, a).map_err(|e| WriteError::Corrupt(e.to_string())),
            )
            .map(ActionResult::data)
            .map_err(asset_error);
        }
        let group = input["group"]
            .as_str()
            .ok_or_else(|| PluginError::new("invalid_input", "缺少资产类型"))?;
        let store = self
            .stores
            .get(group)
            .ok_or_else(|| PluginError::new("invalid_input", "未知资产类型"))?;
        match verb {
            "catalog" => {
                let mut items = vec![];
                for name in store.list().map_err(asset_error)? {
                    let (asset, fingerprint) = store.read(&name).map_err(asset_error)?;
                    validate(group, &name, &asset)?;
                    items.push(json!({"id":name,"title":asset["title"].as_str().unwrap_or(&name),"asset":asset,"fingerprint":fingerprint}));
                }
                Ok(ActionResult::data(json!({"items":items})))
            }
            "read" => {
                let name = input["name"].as_str().unwrap();
                let (asset, fingerprint) = store.read(name).map_err(asset_error)?;
                validate(group, name, &asset)?;
                Ok(ActionResult::data(
                    json!({"asset":asset,"fingerprint":fingerprint}),
                ))
            }
            "save" => {
                let name = input["name"].as_str().unwrap();
                validate(group, name, &input["asset"])?;
                let fingerprint = store
                    .save(name, &input["asset"], input["expected"].as_str())
                    .map_err(asset_error)?;
                Ok(ActionResult::data(json!({"fingerprint":fingerprint})))
            }
            "remove" => {
                let expected = input["expected"]
                    .as_str()
                    .ok_or_else(|| PluginError::new("invalid_input", "删除资产需要字符串指纹"))?;
                store
                    .remove(input["name"].as_str().unwrap(), expected)
                    .map_err(asset_error)?;
                Ok(ActionResult::data(json!({"removed":true})))
            }
            _ => Err(PluginError::new("action_not_found", verb)),
        }
    }
}
pub fn library_actions(prefix: &str, groups: &[&str]) -> Vec<ActionDescriptor> {
    let properties = json!({"group":{"type":"string","enum":groups},"name":{"type":"string"},"asset":{"type":"object"},"expected":{"type":["string","null"]}});
    let mut actions = vec![];
    for (verb, required, title) in [
        ("catalog", vec!["group"], "资产目录"),
        ("read", vec!["group", "name"], "读取资产"),
        ("save", vec!["group", "name", "asset"], "保存资产"),
        ("remove", vec!["group", "name", "expected"], "删除资产"),
    ] {
        let mut properties = properties.clone();
        if verb == "remove" {
            properties["expected"] = json!({"type":"string"});
        }
        actions.push(ActionDescriptor::new(&format!("{prefix}.{verb}"),title,json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})));
    }
    actions.push(ActionDescriptor::new(
        &format!("{prefix}.collection.export"),
        "导出资产集合",
        json!({"type":"object","additionalProperties":false}),
    ));
    actions.push(ActionDescriptor::new(&format!("{prefix}.collection.import"),"导入资产集合",json!({"type":"object","properties":{"collection":{"type":"object"},"validate_only":{"type":"boolean"},"rollback":{"type":"array"}},"anyOf":[{"required":["collection"]},{"required":["rollback"]}],"additionalProperties":false})));
    actions
}
pub fn record<'a>(snapshot: &'a Value, group: &str, id: &str) -> PluginResult<&'a Value> {
    if snapshot["version"] != 1 {
        return Err(PluginError::new("snapshot_version", "不支持的资产快照版本"));
    }
    snapshot["records"]
        .as_array()
        .and_then(|rows| rows.iter().find(|r| r["group"] == group && r["name"] == id))
        .map(|r| &r["asset"])
        .ok_or_else(|| PluginError::new("not_found", format!("{group}/{id}")))
}
pub fn validate_parameter(group: &str, id: &str, asset: &Value) -> PluginResult<()> {
    if asset["format_version"] != 1 || asset["id"] != id {
        return Err(PluginError::new(
            "invalid_asset",
            "格式版本必须为 1，id 必须与文件名一致",
        ));
    }
    if group == "definitions" {
        compile_schema(&asset["schema"])?;
        if let Some(default) = asset.get("default") {
            validate_schema(&asset["schema"], default, "invalid_default")?;
        }
    } else if group == "presets" {
        if !asset["values"].is_object() {
            return Err(PluginError::new("invalid_asset", "预置 values 必须是对象"));
        }
    } else {
        return Err(PluginError::new("invalid_asset", "未知参数资产类型"));
    }
    Ok(())
}
fn tokens(pointer: &str) -> PluginResult<Vec<String>> {
    if !pointer.starts_with('/') || pointer.len() > 1024 {
        return Err(PluginError::new(
            "invalid_binding",
            "绑定必须是非空 JSON Pointer",
        ));
    }
    let tokens = pointer[1..]
        .split('/')
        .map(|s| s.replace("~1", "/").replace("~0", "~"))
        .collect::<Vec<_>>();
    if tokens
        .iter()
        .any(|s| s.is_empty() || s == "__proto__" || s == "constructor" || s == "prototype")
    {
        return Err(PluginError::new("invalid_binding", "不合法的绑定路径"));
    }
    Ok(tokens)
}
pub fn set_pointer(value: &mut Value, pointer: &str, field: Value) -> PluginResult<()> {
    let parts = tokens(pointer)?;
    let mut cursor = value;
    for (i, part) in parts.iter().enumerate() {
        let last = i + 1 == parts.len();
        if cursor.is_array() {
            let array = cursor.as_array_mut().unwrap();
            let index = part
                .parse::<usize>()
                .map_err(|_| PluginError::new("invalid_binding", "列表绑定需要有效下标"))?;
            let element = array
                .get_mut(index)
                .ok_or_else(|| PluginError::new("invalid_binding", "列表绑定下标越界"))?;
            if last {
                *element = field;
                return Ok(());
            }
            cursor = element;
        } else {
            let object = cursor
                .as_object_mut()
                .ok_or_else(|| PluginError::new("invalid_binding", "绑定路径穿过非对象值"))?;
            if last {
                object.insert(part.clone(), field);
                return Ok(());
            }
            cursor = object.entry(part.clone()).or_insert_with(|| json!({}));
        }
    }
    Ok(())
}
pub fn effective_bindings(asset: &Value, requested: Option<&Value>) -> PluginResult<Value> {
    let mut result = asset.as_object().cloned().unwrap_or_default();
    if let Some(requested) = requested {
        let requested = requested
            .as_object()
            .ok_or_else(|| PluginError::new("invalid_binding", "绑定必须为对象"))?;
        for (path, binding) in requested {
            if result.get(path).is_some_and(|old| old != binding) {
                return Err(PluginError::new(
                    "binding_conflict",
                    format!("不能替换模板的共享定义绑定 {path}"),
                ));
            }
            result.insert(path.clone(), binding.clone());
        }
    }
    Ok(Value::Object(result))
}
pub fn resolve(input: &Value, snapshot: &Value) -> PluginResult<Value> {
    if snapshot["version"] != 1 || !snapshot["records"].is_array() {
        return Err(PluginError::new(
            "snapshot_invalid",
            "参数快照格式或版本无效",
        ));
    }
    if input.get("bindings").is_some_and(|v| !v.is_object()) {
        return Err(PluginError::new("invalid_binding", "绑定必须为对象"));
    }
    let bindings = input["bindings"].as_object().cloned().unwrap_or_default();
    let mut values = json!({});
    let defaults = input.get("defaults").cloned().unwrap_or_else(|| json!({}));
    if !defaults.is_object() {
        return Err(PluginError::new("invalid_input", "defaults 必须为对象"));
    }
    let supplied = input.get("values").cloned().unwrap_or_else(|| json!({}));
    if !supplied.is_object() {
        return Err(PluginError::new("invalid_input", "values 必须为对象"));
    }
    let mut origins = json!({});
    let mut definitions = json!({});
    for (pointer, binding) in &bindings {
        tokens(pointer)?;
        if bindings
            .keys()
            .any(|other| other != pointer && other.starts_with(&format!("{pointer}/")))
        {
            return Err(PluginError::new("invalid_binding", "绑定路径重叠"));
        }
        let id = binding
            .as_str()
            .or_else(|| binding["id"].as_str())
            .ok_or_else(|| PluginError::new("invalid_binding", "绑定缺少参数 id"))?;
        let definition = record(snapshot, "definitions", id)?;
        validate_parameter("definitions", id, definition)?;
        if let Some(unit) = binding.get("unit") {
            if definition.get("unit") != Some(unit) {
                return Err(PluginError::new("unit_conflict", pointer));
            }
        }
        if let Some(local) = input
            .get("schema")
            .and_then(|schema| schema_at(schema, pointer))
        {
            if let Some(unit) = local.get("x-unit").or_else(|| local.get("unit")) {
                if definition.get("unit") != Some(unit) {
                    return Err(PluginError::new("unit_conflict", pointer));
                }
            }
            fn types(v: &Value) -> Vec<&str> {
                match v {
                    Value::String(s) => vec![s.as_str()],
                    Value::Array(a) => a.iter().filter_map(Value::as_str).collect(),
                    _ => vec![],
                }
            }
            let a = types(&local["type"]);
            let b = types(&definition["schema"]["type"]);
            if !a.is_empty()
                && !b.is_empty()
                && !a.iter().any(|t| {
                    b.contains(t)
                        || (["number", "integer"].contains(t)
                            && b.iter().any(|t| ["number", "integer"].contains(t)))
                })
            {
                return Err(PluginError::new("type_conflict", pointer));
            }
        }
        definitions[pointer] = definition.clone();
        if values.pointer(pointer).is_none() {
            if let Some(default) = definition.get("default") {
                set_pointer(&mut values, pointer, default.clone())?;
                origins[pointer] = json!("definition");
            }
        }
    }
    fn merge(target: &mut Value, source: &Value) {
        if source.is_object() && !target.is_object() {
            *target = json!({});
        }
        if let (Some(t), Some(s)) = (target.as_object_mut(), source.as_object()) {
            for (k, v) in s {
                if v.is_object() {
                    merge(t.entry(k).or_insert_with(|| json!({})), v);
                } else {
                    t.insert(k.clone(), v.clone());
                }
            }
        }
    }
    for (key, origin) in [
        ("fallback_defaults", "spec_default"),
        ("defaults", "template_default"),
    ] {
        if let Some(layer) = input.get(key) {
            if !layer.is_object() {
                return Err(PluginError::new("invalid_input", "默认层必须为对象"));
            }
            merge(&mut values, layer);
            for pointer in bindings.keys() {
                if layer.pointer(pointer).is_some() {
                    origins[pointer] = json!(origin);
                }
            }
        }
    }
    // An explicit null declines the preset; an absent field inherits the request default.
    if let Some(id) = input.get("preset_id").and_then(Value::as_str) {
        let preset = record(snapshot, "presets", id)?;
        validate_parameter("presets", id, preset)?;
        for (pointer, definition) in definitions.as_object().unwrap() {
            if let Some(value) = preset["values"].get(definition["id"].as_str().unwrap()) {
                set_pointer(&mut values, pointer, value.clone())?;
                origins[pointer] = json!(format!("preset:{id}"));
            }
        }
    }
    merge(&mut values, &supplied);
    for (pointer, definition) in definitions.as_object().unwrap() {
        if supplied.pointer(pointer).is_some() {
            origins[pointer] = json!("manual");
        }
        if let Some(value) = values.pointer(pointer) {
            validate_schema(&definition["schema"], value, "parameter_validation").map_err(
                |mut e| {
                    e.message = format!("{pointer}: {}", e.message);
                    e
                },
            )?;
        }
    }
    if input["defer_validation"] != true {
        if let Some(schema) = input.get("schema") {
            validate_schema(schema, &values, "parameter_validation")?;
        }
    }
    Ok(json!({"values":values,"origins":origins,"definitions":definitions,"snapshot":snapshot}))
}
fn schema_at<'a>(schema: &'a Value, pointer: &str) -> Option<&'a Value> {
    let mut node = schema;
    for part in tokens(pointer).ok()? {
        node = if node["type"] == "array" {
            node.get("items")?
        } else {
            node.get("properties")?.get(&part)?
        };
    }
    Some(node)
}
pub struct ParametersPlugin {
    library: Library,
}
impl ParametersPlugin {
    pub fn new(root: &Path) -> PluginResult<Self> {
        Ok(Self {
            library: Library::new(
                root,
                &[
                    ("definitions", "parameters/definitions"),
                    ("presets", "parameters/presets"),
                ],
            )?,
        })
    }
    pub fn definition() -> PluginDescriptor {
        let mut d = PluginDescriptor::builtin("parameters");
        d.requires = ["template.inspect", "template.render"]
            .iter()
            .map(|id| Requirement::new(id))
            .collect();
        d.provides = [
            "parameter.snapshot",
            "parameter.resolve",
            "parameter.dependencies.check",
            "template.render.managed",
        ]
        .iter()
        .map(|id| Service::new(id, id))
        .collect();
        d.asset_collections = vec![AssetCollection {
            id: "parameters".into(),
            export_action: "parameter.collection.export".into(),
            import_action: "parameter.collection.import".into(),
        }];
        d.panels = vec![Panel {
            id: "parameters".into(),
            title: "参数管理".into(),
            view: "parameter.library".into(),
            actions: library_actions("parameter", &["definitions", "presets"])
                .into_iter()
                .map(|a| a.id)
                .collect(),
        }];
        d
    }
    pub fn action_definitions() -> Vec<ActionDescriptor> {
        let mut actions = library_actions("parameter", &["definitions", "presets"]);
        actions.push(ActionDescriptor::new(
            "parameter.snapshot",
            "冻结参数资产",
            json!({"type":"object","additionalProperties":false}),
        ));
        actions.push(ActionDescriptor::new("parameter.dependencies.check","核对参数依赖",json!({"type":"object","required":["snapshot"],"properties":{"snapshot":{"type":"object"},"template_snapshot":{"type":"object"}},"additionalProperties":false})));
        // An explicit null declines the asset preset; only an absent field falls back to it.
        actions.push(ActionDescriptor::new("parameter.resolve","解析参数",json!({"type":"object","properties":{"bindings":{"type":"object"},"fallback_defaults":{"type":"object"},"defaults":{"type":"object"},"values":{"type":"object"},"schema":{"type":["object","boolean"]},"preset_id":{"type":["string","null"]},"snapshot":{"type":"object"}},"additionalProperties":false})));
        let mut render = ActionDescriptor::new(
            "template.render.managed",
            "共享参数渲染",
            json!({"type":"object","properties":{"source":{"type":"string"},"template":{"type":"string"},"templates":{"type":"object"},"snapshot":{"type":"object"},"context":{"type":"object"},"schema":{"type":["object","boolean"]},"defaults":{"type":"object"},"bindings":{"type":"object"},"preset_id":{"type":["string","null"]},"parameter_snapshot":{"type":"object"},"parameter_request":{"type":"object"},"extensions":{"type":"array","items":{"type":"string"}},"trim_blocks":{"type":"boolean"},"lstrip_blocks":{"type":"boolean"}},"anyOf":[{"required":["source"]},{"required":["template"]},{"required":["snapshot"]}],"additionalProperties":false}),
        );
        render.ui_schema = Some(
            json!({"document_input":{"title":"共享参数模板","match_metadata":"parameters","bindings":{"source":"source","template":"id","context":"context","schema":"schema","defaults":"defaults","bindings":"metadata.parameters.bindings","preset_id":"parameter_preset_id","extensions":"extensions"}}}),
        );
        actions.push(render);
        actions
    }
}
impl Plugin for ParametersPlugin {
    fn descriptor(&self) -> PluginDescriptor {
        Self::definition()
    }
    fn actions(&self) -> Vec<ActionDescriptor> {
        Self::action_definitions()
    }
    fn invoke(&self, action: &str, input: Value, ctx: &Context) -> PluginResult<ActionResult> {
        if action == "parameter.snapshot" {
            return self.library.capture().map(ActionResult::data);
        }
        if action == "parameter.dependencies.check" {
            let mut changes = self.library.changed(&input["snapshot"])?;
            if let Some(templates) = input.get("template_snapshot") {
                let name = templates["template"]
                    .as_str()
                    .ok_or_else(|| PluginError::new("snapshot_invalid", "缺少模板快照入口"))?;
                let frozen = templates["templates"]
                    .as_object()
                    .ok_or_else(|| PluginError::new("snapshot_invalid", "缺少模板快照来源"))?;
                let current = ctx
                    .call(
                        "template.inspect",
                        1,
                        json!({"template":name,"source":"","include_snapshot":true}),
                    )?
                    .data["snapshot"]
                    .clone();
                // Which templates this result depends on is decided by the frozen collection,
                // not by the storage inventory. Inspecting the frozen snapshot walks
                // include/import/from/extends, so an optional reference that was unresolvable
                // when the result was produced stays in the comparison set and its later
                // resolution is detected. Names the frozen sources never reference are not
                // dependencies, so new unrelated assets never report the result as stale.
                let reachable: BTreeSet<String> = ctx
                    .call("template.inspect", 1, json!({"snapshot":templates}))?
                    .data["references"]
                    .as_array()
                    .ok_or_else(|| PluginError::new("snapshot_invalid", "缺少模板快照引用"))?
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect();
                for key in reachable {
                    // The root source stays excluded: an inline root is a caller draft and a
                    // saved root keeps its own asset comparison under template_metadata.
                    if key != name && current["templates"].get(key.as_str()) != frozen.get(&key) {
                        changes.push(format!("templates/{key}"));
                    }
                }
                if current["asset"] != templates["asset"] {
                    changes.push("template_metadata".into());
                }
            }
            return Ok(ActionResult::data(
                json!({"current":changes.is_empty(),"changes":changes}),
            ));
        }
        if action == "parameter.resolve" {
            if input.get("snapshot").is_some()
                && input.get("preset_id").is_some_and(Value::is_string)
            {
                return Err(PluginError::new("invalid_input", "快照与预置选择互斥"));
            }
            let snapshot = match input.get("snapshot") {
                Some(v) => v.clone(),
                None => self.library.capture()?,
            };
            return resolve(&input, &snapshot).map(ActionResult::data);
        }
        if action == "template.render.managed" {
            if input.get("parameter_snapshot").is_some()
                && input.get("preset_id").is_some_and(Value::is_string)
            {
                return Err(PluginError::new("invalid_input", "快照与预置选择互斥"));
            }
            let mut inspect = json!({"include_snapshot":true});
            for key in ["source", "template", "templates", "snapshot"] {
                if let Some(v) = input.get(key) {
                    inspect[key] = v.clone();
                }
            }
            let snapshot = ctx.call("template.inspect", 1, inspect)?.data["snapshot"].clone();
            let asset = &snapshot["asset"];
            let parameters = match input.get("parameter_snapshot") {
                Some(v) => v.clone(),
                None => self.library.capture()?,
            };
            let mut request = json!({"bindings":effective_bindings(&asset["metadata"]["parameters"]["bindings"],input.get("bindings"))?,"values":input.get("context").cloned().unwrap_or_else(||json!({})),"defaults":input.get("defaults").cloned().or_else(||asset.get("defaults").cloned()).unwrap_or_else(||json!({}))});
            for key in ["schema", "preset_id"] {
                // or_else runs only when the field is absent, so an explicit preset_id
                // (including an explicit null declining it) overrides the saved asset choice.
                if let Some(v) = input.get(key).or_else(|| match key {
                    "schema" => asset.get(key),
                    _ => asset["metadata"]["parameters"].get(key),
                }) {
                    request[key] = v.clone();
                }
            }
            if let Some(original) = input.get("parameter_request") {
                if input.get("snapshot").is_none()
                    || input.get("parameter_snapshot").is_none()
                    || ["context", "schema", "defaults", "bindings", "preset_id"]
                        .iter()
                        .any(|key| input.get(key).is_some())
                {
                    return Err(PluginError::new(
                        "invalid_input",
                        "冻结解析请求不能混用实时输入",
                    ));
                }
                let descriptor = Self::action_definitions()
                    .into_iter()
                    .find(|d| d.id == "parameter.resolve")
                    .unwrap();
                validate_schema(&descriptor.input_schema, original, "parameter_validation")?;
                request = original.clone();
                request["bindings"] = effective_bindings(
                    &asset["metadata"]["parameters"]["bindings"],
                    original.get("bindings"),
                )?;
            }
            let resolved = resolve(&request, &parameters)?;
            if let Some(schema) = asset.get("schema") {
                // The request Schema cannot hide type or unit annotations in the asset.
                resolve(
                    &json!({"bindings":request["bindings"],"values":resolved["values"],"schema":schema}),
                    &parameters,
                )?;
            }
            let mut rendered_input = json!({"snapshot":snapshot,"context":resolved["values"],"schema":true,"defaults":{}});
            // The explicit extension list travels with immutable replay input.
            rendered_input["extensions"] = input
                .get("extensions")
                .cloned()
                .unwrap_or_else(|| json!([]));
            for key in ["trim_blocks", "lstrip_blocks"] {
                if let Some(v) = input.get(key) {
                    rendered_input[key] = v.clone();
                }
            }
            let mut result = ctx.call("template.render", 1, rendered_input.clone())?;
            rendered_input.as_object_mut().unwrap().remove("context");
            rendered_input.as_object_mut().unwrap().remove("schema");
            rendered_input.as_object_mut().unwrap().remove("defaults");
            rendered_input["parameter_snapshot"] = parameters;
            rendered_input["parameter_request"] = request;
            result.data["parameters"] = resolved;
            result.data["replay_input"] = rendered_input;
            return Ok(result);
        }
        self.library.invoke(
            action.rsplit('.').next().unwrap_or_default(),
            &input,
            validate_parameter,
        )
    }
}
