//! Optional NC service: domain validation and postprocessing consume generic rendering.
pub mod derive;
mod filters;
pub mod lint;
pub mod model;
mod output_validation;
use output_validation::non_finite_nc;
pub mod postprocess;
pub mod validate;
use model::{MachineConfig, ParamSpec, ParameterSet};
use nctool_assets::JsonStore;
use nctool_plugin_sdk::*;
use postprocess::{GenerationOptions, OutputFormat};
use std::path::Path;

pub fn configure(renderer: &mut Renderer) -> Result<(), nctool_tpl::TplError> {
    renderer.add_filter("nc_fixed", filters::filter_nc_fixed)?;
    renderer.add_filter("nc_signed", filters::filter_nc_signed)?;
    renderer.add_filter("nc_strip", filters::filter_nc_strip)?;
    renderer.add_filter("nc_pad", filters::filter_nc_pad)?;
    Ok(())
}
pub struct NcPlugin {
    machines: JsonStore,
    root: std::path::PathBuf,
}
impl NcPlugin {
    pub fn new(workspace: &Path) -> PluginResult<Self> {
        Ok(Self {
            root: workspace.to_path_buf(),
            machines: JsonStore::new(&workspace.join("machines")).map_err(asset_error)?,
        })
    }
    fn machine(&self, input: &Value) -> PluginResult<MachineConfig> {
        if let Some(machine) = input.get("machine") {
            return validate_machine_content(machine);
        }
        if let Some(id) = input.get("machine_id").and_then(Value::as_str) {
            return validate_machine(id, &self.machines.read(id).map_err(asset_error)?.0);
        }
        Ok(MachineConfig {
            id: "generic".into(),
            vendor: "Example".into(),
            model: "Virtual".into(),
            config: Default::default(),
        })
    }
    fn generate(&self, input: Value, ctx: &Context) -> PluginResult<ActionResult> {
        if input.get("machine").is_some() && input.get("machine_id").is_some() {
            return Err(PluginError::new(
                "invalid_input",
                "choose machine or machine_id",
            ));
        }
        let machine = self.machine(&input)?;
        let template = input
            .get("template")
            .and_then(Value::as_str)
            .or_else(|| input["snapshot"]["template"].as_str())
            .unwrap_or("__inline__");
        let mut render_input = json!({"template":template});
        if let Some(snapshot) = input.get("snapshot") {
            render_input = json!({"snapshot":snapshot});
        }
        for key in [
            "source",
            "templates",
            "lenient",
            "trim_blocks",
            "lstrip_blocks",
        ] {
            if let Some(value) = input.get(key) {
                render_input[key] = value.clone();
            }
        }
        render_input["include_snapshot"] = json!(true);
        let inspection = ctx.call("template.inspect", 1, render_input.clone())?;
        let snapshot = inspection.data.get("snapshot").cloned().ok_or_else(|| {
            PluginError::new(
                "invalid_output",
                "template.inspect must return the requested snapshot",
            )
        })?;
        let defaults = input
            .get("defaults")
            .or_else(|| snapshot["asset"].get("defaults"));
        let mut raw_params = defaults
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        if let Some(provided) = input.get("params").and_then(Value::as_object) {
            raw_params.extend(provided.clone());
        }
        let params: ParameterSet = serde_json::from_value(json!(raw_params))?;
        // Domain assets may choose whitespace behavior; generic rendering stays explicit.
        for key in ["trim_blocks", "lstrip_blocks"] {
            if input.get(key).is_none() {
                if let Some(value) = snapshot["asset"]["metadata"]["nc"]["render_options"].get(key)
                {
                    if !value.is_boolean() {
                        return Err(PluginError::new(
                            "invalid_asset",
                            format!("NC render option {key} must be boolean"),
                        ));
                    }
                    render_input[key] = value.clone();
                }
            }
        }
        let mut specs = input.get("specs").cloned();
        if specs.is_none() {
            specs = snapshot["asset"]["metadata"]["nc"]["specs"]
                .as_array()
                .map(|v| json!(v));
        }
        let specs: Vec<ParamSpec> = serde_json::from_value(specs.unwrap_or_else(|| json!([])))?;
        for key in ["include_snapshot", "source", "template", "templates"] {
            render_input.as_object_mut().unwrap().remove(key);
        }
        render_input["snapshot"] = snapshot.clone();
        let variables = inspection.data["variables"].as_array().ok_or_else(|| {
            PluginError::new("invalid_output", "template.inspect must return variables")
        })?;
        let vars: Vec<nctool_tpl::Variable> = variables
            .iter()
            .map(|v| nctool_tpl::Variable {
                name: v["name"].as_str().unwrap_or_default().into(),
                optional: v["optional"].as_bool().unwrap_or(false),
                line: v["line"].as_u64().unwrap_or(1) as usize,
                col: v["column"].as_u64().unwrap_or(1) as usize,
                start: 0,
                end: 0,
            })
            .collect();
        let mut report = validate::validate_with_vars_with_machine(
            &vars,
            &specs,
            &params,
            &["machine"],
            Some(&machine),
        );
        let effective = derive::apply(&specs, &params).unwrap_or_else(|_| params.clone());
        let effective = model::apply_spec_defaults(&specs, &effective);
        let value_report =
            validate::check_param_values_with_machine(&specs, &effective, Some(&machine));
        for issue in value_report.issues {
            if !report
                .issues
                .iter()
                .any(|old| old.kind == issue.kind && old.param == issue.param)
            {
                report.issues.push(issue);
            }
        }
        let diagnostics: Vec<Diagnostic> = report
            .issues
            .iter()
            .map(|i| Diagnostic {
                level: i.level.as_str().into(),
                code: i.kind.as_str().into(),
                message: i.message.clone(),
                path: i.param.clone(),
            })
            .collect();
        if report.has_errors() {
            return Err(PluginError {
                code: "nc_validation".into(),
                message: "NC parameters failed validation".into(),
                diagnostics,
            });
        }
        // NC always renders required fields strictly; explicit Jinja defaults remain available.
        render_input["lenient"] = json!(false);
        let context = serde_json::to_value(model::build_render_context(&effective, &machine))?;
        let mut parameter_context = context.clone();
        parameter_context.as_object_mut().unwrap().remove("machine");
        if let Some(schema) = input
            .get("schema")
            .or_else(|| snapshot["asset"].get("schema"))
        {
            validate_schema(schema, &parameter_context, "nc_validation")?;
        }
        // Domain validation uses effective parameters, before injecting system variables.
        // The template service must not revalidate that Schema against the injected machine.
        render_input["schema"] = json!(true);
        render_input["defaults"] = json!({});
        render_input["context"] = context;
        render_input["extensions"] = json!(["math", "nc"]);
        let rendered = ctx.call("template.render", 1, render_input)?;
        let text = rendered.data["text"].as_str().ok_or_else(|| {
            PluginError::new("invalid_output", "template.render must return text")
        })?;
        let options: GenerationOptions =
            serde_json::from_value(input.get("options").cloned().unwrap_or_else(|| json!({})))?;
        if options.format != OutputFormat::Gcode {
            return Err(PluginError::new(
                "invalid_input",
                "use template.render for text output",
            ));
        }
        let outcome = postprocess::postprocess(text, template, &options, &machine);
        if non_finite_nc(&outcome.text) {
            return Err(PluginError::new(
                "nc_non_finite",
                "rendered NC contains non-finite numeric output",
            ));
        }
        let mut result = ActionResult::text("program.nc", "text/plain", outcome.text);
        result.data["end_line_number"] = json!(outcome.end_line_number);
        result.data["replay_input"] = json!({"snapshot":snapshot,"params":input.get("params").cloned().unwrap_or_else(||json!({})),"specs":specs,"machine":machine,"options":options});
        for key in [
            "lenient",
            "trim_blocks",
            "lstrip_blocks",
            "schema",
            "defaults",
        ] {
            if let Some(value) = input
                .get(key)
                .or_else(|| {
                    result.data["replay_input"]["snapshot"]["asset"]["metadata"]["nc"]
                        ["render_options"]
                        .get(key)
                })
                .cloned()
            {
                result.data["replay_input"][key] = value;
            }
        }
        result.diagnostics = diagnostics;
        result.diagnostics.extend(
            outcome
                .warnings
                .into_iter()
                .map(|w| Diagnostic::warning("line_number", w)),
        );
        if let Some(source) = inspection.data.get("source").and_then(Value::as_str) {
            for finding in lint::lint(source, template)? {
                result.diagnostics.push(Diagnostic {
                    level: "warning".into(),
                    code: "angle_unit".into(),
                    message: finding.message,
                    path: Some(format!("{template}:{}:{}", finding.line, finding.col)),
                });
            }
        }
        Ok(result)
    }
}
fn validate_machine(name: &str, asset: &Value) -> PluginResult<MachineConfig> {
    let machine: MachineConfig = serde_json::from_value(asset.clone())?;
    if machine.id != name {
        return Err(PluginError::new(
            "invalid_machine",
            "machine ID must match asset name",
        ));
    }
    validate_machine_content(asset)
}
fn validate_machine_content(asset: &Value) -> PluginResult<MachineConfig> {
    let machine: MachineConfig = serde_json::from_value(asset.clone())?;
    for (key, value) in &machine.config {
        if ["line_number_prefix", "program_prefix"].contains(&key.as_str())
            && (value.trim().is_empty() || value.chars().any(char::is_control))
        {
            return Err(PluginError::new(
                "invalid_machine",
                "prefixes must be nonempty and contain no control characters",
            ));
        }
    }
    Ok(machine)
}
fn asset_error(error: nctool_assets::WriteError) -> PluginError {
    let code = match error {
        nctool_assets::WriteError::Conflict { .. } | nctool_assets::WriteError::LockBusy { .. } => {
            "conflict"
        }
        nctool_assets::WriteError::NotFound(_) => "not_found",
        _ => "asset_error",
    };
    PluginError::new(code, error.to_string())
}
impl Plugin for NcPlugin {
    fn descriptor(&self) -> PluginDescriptor {
        Self::definition()
    }
    fn extensions(&self) -> Vec<RenderExtension> {
        vec![RenderExtension::new("nc", configure)]
    }
    fn actions(&self) -> Vec<ActionDescriptor> {
        Self::action_definitions()
    }
    fn invoke(&self, action: &str, input: Value, ctx: &Context) -> PluginResult<ActionResult> {
        let _group = if action.starts_with("machine.") {
            Some(nctool_assets::GroupGuard::acquire(&self.root).map_err(asset_error)?)
        } else {
            None
        };
        if action == "machine.collection.export" {
            return nctool_assets::export_collection(&[("machines", &self.machines)])
                .map(ActionResult::data)
                .map_err(asset_error);
        }
        if action == "machine.collection.import" {
            let stores = [("machines", &self.machines)];
            if let Some(receipt) = input.get("rollback") {
                nctool_assets::rollback_collection(&stores, receipt).map_err(asset_error)?;
                return Ok(ActionResult::data(json!({"rolled_back":true})));
            }
            return nctool_assets::import_collection(
                &stores,
                &input["collection"],
                input["validate_only"].as_bool().unwrap_or(false),
                |_, name, asset| {
                    validate_machine(name, asset)
                        .map_err(|e| nctool_assets::WriteError::Corrupt(e.to_string()))?;
                    Ok(())
                },
            )
            .map(ActionResult::data)
            .map_err(asset_error);
        }
        match action {
            "nc.generate" => self.generate(input, ctx),
            "nc.lint" => Ok(ActionResult::data(
                json!({"findings":lint::lint(input["source"].as_str().unwrap(),"inline")?.into_iter().map(|f|json!({"line":f.line,"column":f.col,"message":f.message,"suggestion":f.suggestion})).collect::<Vec<_>>()}),
            )),
            "machine.list" => Ok(ActionResult::data(
                json!({"names":self.machines.list().map_err(asset_error)?}),
            )),
            "machine.read" => {
                let (machine, fingerprint) = self
                    .machines
                    .read(input["name"].as_str().unwrap())
                    .map_err(asset_error)?;
                Ok(ActionResult::data(
                    json!({"machine":machine,"fingerprint":fingerprint}),
                ))
            }
            "machine.remove" => {
                self.machines
                    .remove(
                        input["name"].as_str().unwrap(),
                        input["expected"].as_str().unwrap(),
                    )
                    .map_err(asset_error)?;
                Ok(ActionResult::data(json!({"removed":input["name"]})))
            }
            "machine.save" => {
                let machine = validate_machine(input["name"].as_str().unwrap(), &input["machine"])?;
                let fp = self
                    .machines
                    .save(
                        &machine.id,
                        &input["machine"],
                        input.get("expected").and_then(Value::as_str),
                    )
                    .map_err(asset_error)?;
                Ok(ActionResult::data(json!({"fingerprint":fp})))
            }
            _ => Err(PluginError::new("action_not_found", action)),
        }
    }
}

impl NcPlugin {
    pub fn definition() -> PluginDescriptor {
        let mut d = PluginDescriptor::builtin("nc");
        d.requires = ["template.render", "template.inspect", "math.ready"]
            .iter()
            .map(|s| Requirement::new(s))
            .collect();
        d.asset_collections = vec![AssetCollection {
            id: "nc".into(),
            export_action: "machine.collection.export".into(),
            import_action: "machine.collection.import".into(),
        }];
        d.provides = vec![Service::new("nc.generate", "nc.generate")];
        d.panels = vec![
            Panel {
                id: "nc".into(),
                title: "G 代码生成".into(),
                actions: vec!["nc.generate".into(), "nc.lint".into()],
                view: "nc.generate".into(),
            },
            Panel {
                id: "machines".into(),
                title: "机床配置".into(),
                actions: vec![
                    "machine.list".into(),
                    "machine.read".into(),
                    "machine.save".into(),
                ],
                view: "nc.machines".into(),
            },
        ];
        d
    }
}

impl NcPlugin {
    pub fn action_definitions() -> Vec<ActionDescriptor> {
        let mut generate = ActionDescriptor::new(
            "nc.generate",
            "生成 NC 程序",
            json!({"type":"object","properties":{
            "snapshot":{"type":"object"},"source":{"type":"string"},"template":{"type":"string"},"templates":{"type":"object","additionalProperties":{"type":"string"}},
            "params":{"type":"object"},"specs":{"type":"array","items":{"type":"object"}},"schema":{"type":["object","boolean"]},"defaults":{"type":"object"},
            "machine":{"type":"object"},"machine_id":{"type":"string"},"options":{"type":"object"},"lenient":{"type":"boolean"},"trim_blocks":{"type":"boolean"},"lstrip_blocks":{"type":"boolean"}
        },"anyOf":[{"required":["source"]},{"required":["template"]},{"required":["snapshot"]}],"additionalProperties":false}),
        );
        generate.ui_schema = Some(
            json!({"document_input":{"title":"NC 程序","match_metadata":"nc","description":"执行 NC 参数校验、刀具联动和格式化。机床与行号选项可在 G 代码生成页面设置。","bindings":{"source":"source","template":"id","params":"effective_context","schema":"schema","defaults":"defaults","specs":"metadata.nc.specs","trim_blocks":"metadata.nc.render_options.trim_blocks","lstrip_blocks":"metadata.nc.render_options.lstrip_blocks","options":"metadata.nc.options","machine_id":"metadata.nc.machine_id"}}}),
        );
        generate.example = json!({"source":"O{{ program | nc_pad(4) }}\nG1 X{{ x | nc_fixed(3) }}\nM30", "params":{"program":1,"x":10.5},"options":{"line_numbers":true}});
        let lint = ActionDescriptor::new(
            "nc.lint",
            "检查角度单位",
            json!({"type":"object","required":["source"],"properties":{"source":{"type":"string"}},"additionalProperties":false}),
        );
        let list = ActionDescriptor::new(
            "machine.list",
            "机床列表",
            json!({"type":"object","additionalProperties":false}),
        );
        let read = ActionDescriptor::new(
            "machine.read",
            "读取机床",
            json!({"type":"object","required":["name"],"properties":{"name":{"type":"string"}},"additionalProperties":false}),
        );
        let mut save = ActionDescriptor::new(
            "machine.save",
            "保存机床",
            json!({"type":"object","required":["name","machine"],"properties":{"name":{"type":"string"},"machine":{"type":"object"},"expected":{"type":["string","null"]}},"additionalProperties":false}),
        );
        save.example = json!({"name":"example","machine":{"id":"example","vendor":"Example","model":"Virtual","config":{"line_number_digits":"4"}},"expected":null});
        generate.ui_schema.as_mut().unwrap()["properties"] = json!({"params":{"title":"加工参数"},"source":{"title":"临时源码","format":"multiline"}});
        vec![
            generate,
            lint,
            list,
            read,
            save,
            ActionDescriptor::new("machine.collection.export", "导出机床集合", object_schema()),
            ActionDescriptor::new(
                "machine.collection.import",
                "导入机床集合",
                json!({"type":"object","properties":{"collection":{"type":"object"},"validate_only":{"type":"boolean"},"rollback":{"type":"array"}},"anyOf":[{"required":["collection"]},{"required":["rollback"]}],"additionalProperties":false}),
            ),
            ActionDescriptor::new(
                "machine.remove",
                "删除机床",
                json!({"type":"object","required":["name","expected"],"properties":{"name":{"type":"string"},"expected":{"type":"string"}},"additionalProperties":false}),
            ),
        ]
    }
}
