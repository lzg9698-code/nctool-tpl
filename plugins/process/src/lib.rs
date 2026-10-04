//! Sequential operations consume the versioned NC service, never its concrete Rust types.
use nctool_plugin_sdk::*;
pub struct ProcessPlugin;
impl Plugin for ProcessPlugin {
    fn descriptor(&self) -> PluginDescriptor {
        Self::definition()
    }
    fn actions(&self) -> Vec<ActionDescriptor> {
        Self::action_definitions()
    }
    fn invoke(&self, _action: &str, input: Value, ctx: &Context) -> PluginResult<ActionResult> {
        if input.get("machine").is_some() && input.get("machine_id").is_some() {
            return Err(PluginError::new(
                "invalid_input",
                "choose machine or machine_id",
            ));
        }
        let mut segments = Vec::new();
        let mut summaries = Vec::new();
        let mut replay_ops = Vec::new();
        let mut diagnostics = Vec::new();
        let mut failures = Vec::new();
        let mut cursor = input
            .get("options")
            .and_then(|v| v.get("line_number_start"))
            .and_then(Value::as_u64)
            .unwrap_or(0);
        for (index, op) in input["ops"].as_array().unwrap().iter().enumerate() {
            ctx.cancellation.check()?;
            let mut request = op.clone();
            let mut params = input
                .get("params")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            if let Some(overrides) = op.get("params").and_then(Value::as_object) {
                params.extend(overrides.clone());
            }
            request["params"] = json!(params);
            let mut options = input
                .get("options")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            if let Some(overrides) = op.get("options").and_then(Value::as_object) {
                options.extend(overrides.clone());
            }
            // The cursor is orchestrator-owned; operations cannot reset numbering.
            options.insert("line_number_start".into(), json!(cursor));
            request["options"] = json!(options);
            if op.get("machine").is_none() && op.get("machine_id").is_none() {
                for key in ["machine", "machine_id"] {
                    if let Some(value) = input.get(key) {
                        request[key] = value.clone();
                    }
                }
            }
            let replay_fallback = request.clone();
            match ctx.call("nc.generate", 1, request) {
                Ok(result) => {
                    replay_ops.push(
                        result
                            .data
                            .get("replay_input")
                            .cloned()
                            .unwrap_or(replay_fallback),
                    );
                    let text = result.data["text"]
                        .as_str()
                        .ok_or_else(|| {
                            PluginError::new("invalid_output", "NC provider must return text")
                        })?
                        .to_string();
                    cursor = result.data["end_line_number"].as_u64().ok_or_else(|| {
                        PluginError::new(
                            "invalid_output",
                            "NC provider must return end_line_number",
                        )
                    })?;
                    segments.push(text);
                    diagnostics.extend(result.diagnostics);
                    summaries.push(json!({"index":index,"ok":true}));
                }
                Err(error) => {
                    if error.code == "cancelled" {
                        return Err(error);
                    }
                    failures.push(Diagnostic {
                        level: "error".into(),
                        code: error.code,
                        message: error.message,
                        path: Some(format!("ops[{index}]")),
                    });
                    for mut diagnostic in error.diagnostics {
                        diagnostic.path = Some(format!(
                            "ops[{index}]{}",
                            diagnostic.path.map(|p| format!(".{p}")).unwrap_or_default()
                        ));
                        failures.push(diagnostic);
                    }
                    summaries.push(json!({"index":index,"ok":false}));
                }
            }
        }
        if !failures.is_empty() {
            failures.extend(diagnostics);
            return Err(PluginError {
                code: "operations_failed".into(),
                message: "No program delivered: one or more operations failed".into(),
                diagnostics: failures,
            });
        }
        // Delimit segments even if a swappable provider omits its final newline.
        let text = segments
            .into_iter()
            .map(|s| {
                if s.ends_with('\n') {
                    s
                } else {
                    format!("{s}\n")
                }
            })
            .collect::<String>();
        let name = input
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("program");
        let mut result = ActionResult::text(&format!("{name}.nc"), "text/plain", text);
        result.data["operations"] = json!(summaries);
        result.data["replay_input"] = json!({"name":name,"ops":replay_ops,"options":input.get("options").cloned().unwrap_or_else(||json!({}))});
        result.data["end_line_number"] = json!(cursor);
        result.diagnostics = diagnostics;
        Ok(result)
    }
}

impl ProcessPlugin {
    pub fn definition() -> PluginDescriptor {
        let mut d = PluginDescriptor::builtin("process");
        d.requires = vec![Requirement::new("nc.generate")];
        d.provides = vec![Service::new("process.generate", "process.generate")];
        d.panels = vec![Panel {
            id: "process".into(),
            title: "多工序程序".into(),
            actions: vec!["process.generate".into()],
            view: "nc.process".into(),
        }];
        d
    }
}

impl ProcessPlugin {
    pub fn action_definitions() -> Vec<ActionDescriptor> {
        let mut action = ActionDescriptor::new(
            "process.generate",
            "生成多工序程序",
            json!({"type":"object","required":["ops"],"properties":{
            "name":{"type":"string"},"params":{"type":"object"},"machine":{"type":"object"},"machine_id":{"type":"string"},"options":{"type":"object"},
            "ops":{"type":"array","minItems":1,"maxItems":1024,"items":{"type":"object","properties":{"snapshot":{"type":"object"},"source":{"type":"string"},"template":{"type":"string"},"templates":{"type":"object","additionalProperties":{"type":"string"}},"params":{"type":"object"},"specs":{"type":"array"},"schema":{"type":["object","boolean"]},"defaults":{"type":"object"},"machine":{"type":"object"},"machine_id":{"type":"string"},"options":{"type":"object"},"lenient":{"type":"boolean"},"trim_blocks":{"type":"boolean"},"lstrip_blocks":{"type":"boolean"}},"anyOf":[{"required":["source"]},{"required":["template"]},{"required":["snapshot"]}],"additionalProperties":false}}
        },"additionalProperties":false}),
        );
        action.example = json!({"name":"example","params":{"x":10},"options":{"line_numbers":true},"ops":[{"source":"G0 X{{ x | nc_fixed(3) }}"},{"source":"G1 X{{ x | nc_fixed(3) }}","params":{"x":20}}]});
        vec![action]
    }
}
