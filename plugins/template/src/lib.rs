//! Generic template assets, schemas, presets and rendering. No NC dependencies.
use nctool_assets::{JsonStore, WriteError};
use nctool_plugin_sdk::*;
use nctool_tpl::{extract_template_refs, extract_undeclared, parse};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::Mutex,
};

pub struct TemplatePlugin {
    templates: JsonStore,
    presets: JsonStore,
    // Protect multi-action edit/read operations inside this process; disk writes retain OS locks.
    access: Mutex<()>,
    root: std::path::PathBuf,
}
impl TemplatePlugin {
    pub fn new(workspace: &Path) -> PluginResult<Self> {
        Ok(Self {
            templates: JsonStore::new(&workspace.join("templates")).map_err(asset_error)?,
            presets: JsonStore::new(&workspace.join("presets")).map_err(asset_error)?,
            access: Mutex::new(()),
            root: workspace.to_path_buf(),
        })
    }
    fn sources(&self, input: &Value) -> PluginResult<TemplateSnapshot> {
        if let Some(snapshot) = input.get("snapshot") {
            if ["source", "template", "templates"]
                .iter()
                .any(|key| input.get(key).is_some())
            {
                return Err(PluginError::new(
                    "invalid_input",
                    "snapshot cannot be combined with source/template/templates",
                ));
            }
            let snapshot: TemplateSnapshot = serde_json::from_value(snapshot.clone())?;
            check_snapshot(&snapshot)?;
            return Ok(snapshot);
        }
        let _group = nctool_assets::GroupGuard::acquire(&self.root).map_err(asset_error)?;
        let root_name = input
            .get("template")
            .and_then(Value::as_str)
            .unwrap_or("__inline__")
            .to_owned();
        let mut root_asset = None;
        let mut sources = BTreeMap::new();
        let mut loaded_bytes = 0;
        for name in self.templates.list().map_err(asset_error)? {
            let (asset, _) = self.templates.read(&name).map_err(asset_error)?;
            let source = asset.get("source").and_then(Value::as_str).ok_or_else(|| {
                PluginError::new("invalid_asset", format!("{name} has no source"))
            })?;
            loaded_bytes += source.len();
            if sources.len() >= 1024 || loaded_bytes > 16 * 1024 * 1024 {
                return Err(PluginError::new(
                    "template_limit",
                    "template collection exceeds limits",
                ));
            }
            sources.insert(name.clone(), source.into());
            if name == root_name {
                root_asset = Some(asset);
            }
        }
        if let Some(map) = input.get("templates").and_then(Value::as_object) {
            for (name, source) in map {
                sources.insert(
                    name.clone(),
                    source
                        .as_str()
                        .ok_or_else(|| {
                            PluginError::new("invalid_input", "templates values must be strings")
                        })?
                        .into(),
                );
            }
        }
        let name = root_name;
        // Source drafts replace only the source; the selected asset's constraints remain.
        if let Some(source) = input.get("source").and_then(Value::as_str) {
            sources.insert(name.clone(), source.into());
        }
        let snapshot = TemplateSnapshot {
            template: name,
            templates: sources,
            asset: root_asset,
        };
        check_snapshot(&snapshot)?;
        Ok(snapshot)
    }

    fn inspect(&self, input: &Value) -> PluginResult<Value> {
        let snapshot = self.sources(input)?;
        let name = &snapshot.template;
        let sources = &snapshot.templates;
        let mut queue = vec![name.clone()];
        let mut visited = BTreeSet::new();
        let mut variables: BTreeMap<String, Value> = BTreeMap::new();
        while let Some(name) = queue.pop() {
            if !visited.insert(name.clone()) {
                continue;
            }
            // Inspection freezes available sources, not the execution's control flow.
            // Jinja resolves required/optional references when rendering the snapshot.
            let Some(source) = sources.get(&name) else {
                continue;
            };
            let ast = parse(source, &name)?;
            for v in extract_undeclared(&ast) {
                let value = json!({"name":v.name,"optional":v.optional,"template":name,"line":v.line,"column":v.col});
                match variables.get(&v.name) {
                    Some(old) if old["optional"] == false => {}
                    _ => {
                        variables.insert(v.name, value);
                    }
                }
            }
            queue.extend(extract_template_refs(&ast));
        }
        let mut result = json!({"template":name,"variables":variables.into_values().collect::<Vec<_>>(),"references":visited,"source":sources[name]});
        if input
            .get("include_snapshot")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            result["snapshot"] = serde_json::to_value(snapshot)?;
        }
        Ok(result)
    }
    fn render(&self, input: Value, ctx: &Context) -> PluginResult<ActionResult> {
        let TemplateSnapshot {
            template: name,
            templates: sources,
            asset,
        } = self.sources(&input)?;
        let mut renderer = Renderer::new();
        if input
            .get("lenient")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            renderer = renderer.with_lenient();
        }
        let trim = input
            .get("trim_blocks")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let lstrip = input
            .get("lstrip_blocks")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        renderer = renderer.with_whitespace(trim, lstrip);
        let extensions: Vec<String> = input
            .get("extensions")
            .and_then(Value::as_array)
            .map(|v| {
                v.iter()
                    .map(|v| v.as_str().unwrap_or_default().into())
                    .collect()
            })
            .unwrap_or_default();
        ctx.extend(&mut renderer, &extensions)?;
        for (name, source) in sources {
            renderer.add_template(name, source)?;
        }
        let mut context = input.get("context").cloned().unwrap_or_else(|| json!({}));
        if let Some(defaults) = input
            .get("defaults")
            .or_else(|| asset.as_ref().and_then(|a| a.get("defaults")))
            .and_then(Value::as_object)
        {
            let map = context
                .as_object_mut()
                .ok_or_else(|| PluginError::new("invalid_input", "context must be an object"))?;
            for (key, value) in defaults {
                map.entry(key).or_insert_with(|| value.clone());
            }
        }
        if let Some(schema) = input
            .get("schema")
            .or_else(|| asset.as_ref().and_then(|a| a.get("schema")))
        {
            validate_schema(schema, &context, "validation")?;
        }
        ctx.cancellation.check()?;
        let text = renderer.render_template(&name, &nctool_tpl::Value::from_serialize(context))?;
        ctx.cancellation.check()?;
        Ok(ActionResult::text(
            &format!("{name}.txt"),
            "text/plain",
            text,
        ))
    }
}
fn check_snapshot(snapshot: &TemplateSnapshot) -> PluginResult<()> {
    if !snapshot.templates.contains_key(&snapshot.template) {
        return Err(PluginError::new("template_not_found", &snapshot.template));
    }
    if snapshot.templates.len() > 1024
        || snapshot.templates.values().map(String::len).sum::<usize>() > 16 * 1024 * 1024
    {
        return Err(PluginError::new(
            "template_limit",
            "template collection exceeds limits",
        ));
    }
    Ok(())
}
fn asset_error(error: WriteError) -> PluginError {
    let code = match error {
        WriteError::Conflict { .. } | WriteError::LockBusy { .. } => "conflict",
        WriteError::NotFound(_) => "not_found",
        WriteError::PathEscape { .. } => "path_escape",
        _ => "asset_error",
    };
    PluginError::new(code, error.to_string())
}
fn render_schema() -> Value {
    json!({"type":"object","properties":{
        "source":{"type":"string","maxLength":1048576},"template":{"type":"string"},
        "include_snapshot":{"type":"boolean"},
        "snapshot":{"type":"object","required":["template","templates"],"properties":{"template":{"type":"string"},"templates":{"type":"object","additionalProperties":{"type":"string"}},"asset":{"type":["object","null"]}},"additionalProperties":false},
        "context":{"type":"object"},"templates":{"type":"object","additionalProperties":{"type":"string"}},
        "lenient":{"type":"boolean"},"extensions":{"type":"array","items":{"type":"string"}},
        "schema":{"type":["object","boolean"]},"defaults":{"type":"object"},"trim_blocks":{"type":"boolean"},"lstrip_blocks":{"type":"boolean"}
    },"anyOf":[{"required":["source"]},{"required":["template"]},{"required":["snapshot"]}],"additionalProperties":false})
}
impl Plugin for TemplatePlugin {
    fn descriptor(&self) -> PluginDescriptor {
        Self::definition()
    }
    fn actions(&self) -> Vec<ActionDescriptor> {
        Self::action_definitions()
    }
    fn invoke(&self, action: &str, input: Value, ctx: &Context) -> PluginResult<ActionResult> {
        let _group = if ["template.render", "template.inspect"].contains(&action) {
            None
        } else {
            Some(nctool_assets::GroupGuard::acquire(&self.root).map_err(asset_error)?)
        };
        if action == "template.collection.export" {
            return nctool_assets::export_collection(&[
                ("templates", &self.templates),
                ("presets", &self.presets),
            ])
            .map(ActionResult::data)
            .map_err(asset_error);
        }
        if action == "template.collection.import" {
            let stores = [("templates", &self.templates), ("presets", &self.presets)];
            if let Some(receipt) = input.get("rollback") {
                nctool_assets::rollback_collection(&stores, receipt).map_err(asset_error)?;
                return Ok(ActionResult::data(json!({"rolled_back":true})));
            }
            let imported = nctool_assets::import_collection(
                &stores,
                &input["collection"],
                input["validate_only"].as_bool().unwrap_or(false),
                |group, name, asset| {
                    if group == "templates" {
                        let source = asset["source"]
                            .as_str()
                            .ok_or_else(|| WriteError::Corrupt("模板缺少 source".into()))?;
                        parse(source, name).map_err(|e| WriteError::Corrupt(e.to_string()))?;
                        if let Some(tags) = asset.get("tags") {
                            if !tags
                                .as_array()
                                .is_some_and(|v| v.iter().all(Value::is_string))
                            {
                                return Err(WriteError::Corrupt("tags must be strings".into()));
                            }
                        }
                        if let Some(schema) = asset.get("schema") {
                            compile_schema(schema)
                                .map_err(|e| WriteError::Corrupt(e.to_string()))?;
                        }
                    }
                    if group == "presets" && asset.get("context").is_none() {
                        return Err(WriteError::Corrupt("预设缺少 context".into()));
                    }
                    Ok(())
                },
            )
            .map_err(asset_error)?;
            return Ok(ActionResult::data(imported));
        }
        if action == "template.render" {
            return self.render(input, ctx);
        }
        let _guard = self.access.lock().unwrap();
        if action == "template.inspect" {
            return self.inspect(&input).map(ActionResult::data);
        }
        if action == "template.catalog" {
            let mut items = vec![];
            let mut diagnostics = vec![];
            for name in self.templates.list().map_err(asset_error)? {
                match self.templates.read(&name) {
                    Ok((asset,fingerprint))=>items.push(json!({"id":name,"title":asset["metadata"]["title"].as_str().unwrap_or(&name),"description":asset["metadata"]["description"].as_str().unwrap_or(""),"tags":asset.get("tags").cloned().unwrap_or_else(||json!([])),"fingerprint":fingerprint})),
                    Err(error)=>diagnostics.push(Diagnostic{level:"error".into(),code:"invalid_asset".into(),message:error.to_string(),path:Some(name)})
                }
            }
            let mut result = ActionResult::data(json!({"items":items}));
            result.diagnostics = diagnostics;
            return Ok(result);
        }
        let store = if action.starts_with("template.") {
            &self.templates
        } else {
            &self.presets
        };
        match action.rsplit('.').next().unwrap_or_default() {
            "list" => Ok(ActionResult::data(
                json!({"names":store.list().map_err(asset_error)?}),
            )),
            "read" => {
                let (asset, fingerprint) = store
                    .read(input["name"].as_str().unwrap())
                    .map_err(asset_error)?;
                Ok(ActionResult::data(
                    json!({"asset":asset,"fingerprint":fingerprint}),
                ))
            }
            "remove" => {
                let name = input["name"].as_str().unwrap();
                if action == "template.remove" {
                    // Static references must be repaired before deleting a shared template.
                    for other in self.templates.list().map_err(asset_error)? {
                        if other == name {
                            continue;
                        }
                        let (asset, _) = self.templates.read(&other).map_err(asset_error)?;
                        if let Some(source) = asset["source"].as_str() {
                            let ast = parse(source, &other)?;
                            if extract_template_refs(&ast)
                                .iter()
                                .any(|reference| reference == name)
                            {
                                return Err(PluginError::new(
                                    "asset_referenced",
                                    format!("模板 {other} 正在引用 {name}"),
                                ));
                            }
                        }
                    }
                }
                store
                    .remove(name, input["expected"].as_str().unwrap())
                    .map_err(asset_error)?;
                Ok(ActionResult::data(json!({"removed":name})))
            }
            "copy" => {
                let (mut asset, _) = store
                    .read(input["name"].as_str().unwrap())
                    .map_err(asset_error)?;
                if let Some(title) = input.get("title") {
                    if !asset.get("metadata").is_some_and(Value::is_object) {
                        asset["metadata"] = json!({});
                    }
                    asset["metadata"]["title"] = title.clone();
                }
                let fingerprint = store
                    .save(input["new_name"].as_str().unwrap(), &asset, None)
                    .map_err(asset_error)?;
                Ok(ActionResult::data(
                    json!({"asset":asset,"fingerprint":fingerprint}),
                ))
            }
            "save" => {
                let asset = &input["asset"];
                if action == "template.save" {
                    let source = asset.get("source").and_then(Value::as_str).ok_or_else(|| {
                        PluginError::new("invalid_asset", "template requires source")
                    })?;
                    parse(source, input["name"].as_str().unwrap())?;
                    if let Some(schema) = asset.get("schema") {
                        let validator = compile_schema(schema)
                            .map_err(|e| PluginError::new("invalid_schema", e.to_string()))?;
                        drop(validator);
                    }
                    if let Some(tags) = asset.get("tags") {
                        if !tags
                            .as_array()
                            .is_some_and(|v| v.iter().all(Value::is_string))
                        {
                            return Err(PluginError::new("invalid_asset", "tags must be strings"));
                        }
                    }
                }
                let fp = store
                    .save(
                        input["name"].as_str().unwrap(),
                        asset,
                        input.get("expected").and_then(Value::as_str),
                    )
                    .map_err(asset_error)?;
                Ok(ActionResult::data(json!({"fingerprint":fp})))
            }
            _ => Err(PluginError::new("action_not_found", action)),
        }
    }
}

impl TemplatePlugin {
    pub fn definition() -> PluginDescriptor {
        let mut d = PluginDescriptor::builtin("template");
        d.asset_collections = vec![AssetCollection {
            id: "template".into(),
            export_action: "template.collection.export".into(),
            import_action: "template.collection.import".into(),
        }];
        d.provides = ["template.render", "template.inspect", "template.read"]
            .iter()
            .map(|id| Service::new(id, id))
            .collect();
        d.panels = vec![
            Panel {
                id: "templates".into(),
                title: "模板工作区".into(),
                actions: vec![
                    "template.render".into(),
                    "template.inspect".into(),
                    "template.list".into(),
                    "template.read".into(),
                    "template.save".into(),
                ],
                view: "template-editor".into(),
            },
            Panel {
                id: "presets".into(),
                title: "参数预设".into(),
                actions: vec![
                    "preset.list".into(),
                    "preset.read".into(),
                    "preset.save".into(),
                ],
                view: "form".into(),
            },
        ];
        d
    }
}

impl TemplatePlugin {
    pub fn action_definitions() -> Vec<ActionDescriptor> {
        let mut render = ActionDescriptor::new("template.render", "渲染文本", render_schema());
        render.ui_schema = Some(
            json!({"document_input":{"title":"普通文本","default":true,"description":"通用 Jinja 渲染，不注入领域上下文。","bindings":{"source":"source","template":"id","context":"context","schema":"schema","defaults":"defaults"}}}),
        );
        render.example = json!({"source":"Hello {{ user.name }}!{% for item in items %}\n- {{ item }}{% endfor %}","context":{"user":{"name":"Ada"},"items":["templates","plugins"]}});
        let inspect = ActionDescriptor::new("template.inspect", "检查变量", render_schema());
        let read = json!({"type":"object","required":["name"],"properties":{"name":{"type":"string"}},"additionalProperties":false});
        let save = json!({"type":"object","required":["name","asset"],"properties":{"name":{"type":"string"},"asset":{"type":"object"},"expected":{"type":["string","null"]}},"additionalProperties":false});
        let mut actions = vec![
            ActionDescriptor::new(
                "template.collection.export",
                "导出模板与预设",
                object_schema(),
            ),
            ActionDescriptor::new(
                "template.collection.import",
                "导入模板与预设",
                json!({"type":"object","properties":{"collection":{"type":"object"},"validate_only":{"type":"boolean"},"rollback":{"type":"array"}},"anyOf":[{"required":["collection"]},{"required":["rollback"]}],"additionalProperties":false}),
            ),
            render,
            inspect,
            ActionDescriptor::new(
                "template.catalog",
                "模板目录",
                json!({"type":"object","additionalProperties":false}),
            ),
        ];
        for prefix in ["template", "preset"] {
            actions.push(ActionDescriptor::new(
                &format!("{prefix}.list"),
                "列出资产",
                json!({"type":"object","additionalProperties":false}),
            ));
            actions.push(ActionDescriptor::new(
                &format!("{prefix}.read"),
                "读取资产",
                read.clone(),
            ));
            let mut action =
                ActionDescriptor::new(&format!("{prefix}.save"), "保存资产", save.clone());
            action.example = if prefix == "template" {
                json!({"name":"hello","asset":{"source":"Hello {{ name }}","tags":["example"],"schema":{"type":"object","properties":{"name":{"type":"string"}},"required":["name"]},"defaults":{},"metadata":{}},"expected":null})
            } else {
                json!({"name":"hello","asset":{"context":{"name":"Ada"}},"expected":null})
            };
            actions.push(action);
            actions.push(ActionDescriptor::new(&format!("{prefix}.remove"),"删除资产",json!({"type":"object","required":["name","expected"],"properties":{"name":{"type":"string"},"expected":{"type":"string"}},"additionalProperties":false})));
            actions.push(ActionDescriptor::new(&format!("{prefix}.copy"),"复制资产",json!({"type":"object","required":["name","new_name"],"properties":{"name":{"type":"string"},"new_name":{"type":"string"},"title":{"type":"string"}},"additionalProperties":false})));
        }
        actions
    }
}
