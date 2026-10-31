//! nc-undercut action implementations.
use crate::calculation;
use crate::rpc::{PluginError, PluginResult, Server};
use crate::trajectory;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn invalid(message: &str) -> PluginError {
    PluginError::new("undercut_validation", message)
}

pub struct Plugin {
    root: PathBuf,
}

fn fingerprint(value: &Value) -> String {
    use sha2::{Digest, Sha256};
    let bytes = serde_json::to_vec(value).unwrap_or_default();
    let digest = Sha256::digest(&bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// Batch request contract shared by prepare/generate (and the UI form).
pub fn batch_schema() -> Value {
    let row = json!({"type":"object","required":["id"],"properties":{"id":{"type":"string","minLength":1},"gtype":{"type":"string","enum":["ES","FS"]},"std_key":{"type":"string"},"d1":{"type":"number"},"z":{"type":"number"},"d2":{"type":"number"},"c":{"type":"number"},"angle":{"type":"number"}},"additionalProperties":false});
    let side = json!({"type":"object","required":["rows"],"properties":{"rows":{"type":"array","items":row,"maxItems":256},"trajectory":{"type":"object","properties":{"preset_id":{"type":"string"},"overrides":{"type":"object"}},"additionalProperties":false}},"additionalProperties":false});
    json!({"type":"object","properties":{"raw_diameter":{"type":"number"},"safety_dis":{"type":"number"},"target":{"type":"string","enum":["right","left","all"]},"sides":{"type":"object","properties":{"right":side,"left":side},"additionalProperties":false},"standard_id":{"type":"string"},"template":{"type":"string"},"options":{"type":"object"}},"required":["raw_diameter","safety_dis","sides"],"additionalProperties":false})
}

impl Plugin {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn builtin_standard(&self) -> PluginResult<Value> {
        let text = std::fs::read_to_string(self.root.join("standards/v3-grooves.json"))?;
        Ok(serde_json::from_str(&text)?)
    }

    fn custom_standard(&self) -> PluginResult<Option<Value>> {
        let path = self.root.join("data/custom-standards.json");
        if !path.exists() {
            return Ok(None);
        }
        Ok(Some(serde_json::from_str(&std::fs::read_to_string(path)?)?))
    }

    /// Builtin tables merged with imported custom entries; a custom key
    /// overrides the same-named builtin entry.
    pub fn merged_standard(&self) -> PluginResult<Value> {
        let mut standard = self.builtin_standard()?;
        let mut custom_count = 0usize;
        if let Some(custom) = self.custom_standard()? {
            for gtype in ["ES", "FS"] {
                if let Some(entries) = custom["tables"][gtype].as_object() {
                    for (key, entry) in entries {
                        standard["tables"][gtype]
                            .as_object_mut()
                            .ok_or_else(|| invalid("内置标准表结构无效"))?
                            .insert(key.clone(), entry.clone());
                        custom_count += 1;
                    }
                }
            }
        }
        standard["custom_count"] = json!(custom_count);
        Ok(standard)
    }

    /// Resolve the trajectory preset: v1 ships only the V3-compatible strategy;
    /// `overrides` may retune its three parameters within the declared bounds.
    fn trajectory_parameters(&self, choice: &Value) -> PluginResult<Value> {
        let preset = choice["preset_id"].as_str().unwrap_or("v3-compatible");
        if preset != "v3-compatible" {
            return Err(invalid("未知的轨迹预设，仅内置 v3-compatible 可用"));
        }
        let mut parameters = trajectory::builtin()["parameters"].clone();
        if let Some(overrides) = choice.get("overrides").and_then(Value::as_object) {
            for (key, value) in overrides {
                let v = value
                    .as_f64()
                    .filter(|v| v.is_finite())
                    .ok_or_else(|| invalid("轨迹参数必须是有限数值"))?;
                let ok = match key.as_str() {
                    "approach_extra" => (0. ..=1000.).contains(&v),
                    "cut_feed" => (0. ..=10000.).contains(&v) && v > 0.,
                    "fs_exit_extension" => (0. ..=1000.).contains(&v) && v > 0.,
                    _ => {
                        return Err(invalid(
                            "未知轨迹参数，仅 approach_extra/cut_feed/fs_exit_extension 可调",
                        ))
                    }
                };
                if !ok {
                    return Err(invalid("轨迹参数超出允许范围"));
                }
                parameters[key] = json!(v);
            }
        }
        Ok(parameters)
    }

    fn prepare(
        &self,
        input: &Value,
        server: &Arc<Server>,
        request_id: &str,
    ) -> PluginResult<Value> {
        let standard = self.merged_standard()?;
        let target = input["target"].as_str().unwrap_or("all");
        if !["right", "left", "all"].contains(&target) {
            return Err(invalid("target 必须是 right/left/all"));
        }
        if !input["sides"].is_object() {
            return Err(invalid("sides 必须是对象"));
        }
        let mut sides = json!({});
        let mut total = 0usize;
        for side in ["right", "left"] {
            if target != "all" && target != side {
                continue;
            }
            let Some(settings) = input["sides"].get(side) else {
                if target != "all" {
                    return Err(invalid("所选侧别缺少输入"));
                }
                continue;
            };
            let parameters = self.trajectory_parameters(&settings["trajectory"])?;
            let rows_input = settings["rows"]
                .as_array()
                .ok_or_else(|| invalid("rows 必须是数组"))?;
            let mut rows = vec![];
            let mut texts = vec![];
            let mut seen = std::collections::BTreeSet::new();
            for (index, row) in rows_input.iter().enumerate() {
                server.check_cancelled(request_id)?;
                let row_id = row["id"].as_str().unwrap_or_default();
                if !seen.insert(row_id.to_string()) {
                    return Err(invalid("行 ID 重复"));
                }
                if row.as_object().map(|m| m.len() <= 1).unwrap_or(true) {
                    continue; // id-only rows stay draft
                }
                let es = row["gtype"] == "ES";
                if row["gtype"] != "ES" && row["gtype"] != "FS" {
                    return Err(invalid("gtype 必须是 ES 或 FS"));
                }
                if !es && ["d2", "c", "angle"].iter().any(|k| row.get(*k).is_some()) {
                    return Err(invalid("FS 不接受 ES 专用字段"));
                }
                let prefix = if es { "ES" } else { "FS" };
                let mut raw = json!({
                    "gtype": row["gtype"], "std_key": row["std_key"],
                    "side": if side == "right" { "Right" } else { "Left" },
                    "raw_diameter": input["raw_diameter"], "safety_dis": input["safety_dis"],
                });
                raw[format!("{prefix}_d1")] = row["d1"].clone();
                raw[format!("{prefix}_z")] = row["z"].clone();
                if es {
                    raw["ES_d2"] = row["d2"].clone();
                    raw["ES_c"] = row["c"].clone();
                    raw["ES_c_ang"] = row["angle"].clone();
                }
                let result = (|| -> PluginResult<Value> {
                    let mut prepared = calculation::calculate(&raw, &standard)?;
                    let path = trajectory::plan(&mut prepared["contexts"][0], &parameters)?;
                    let program = path.render()?;
                    prepared["previews"] = json!([path.preview()?]);
                    prepared["trajectory_plans"] = json!([path]);
                    prepared["program"] = json!(program);
                    prepared["operation"] = json!({"id":"groove","version":1});
                    prepared["inputs"] = raw.clone();
                    prepared["algorithm_version"] = json!(1);
                    Ok(prepared)
                })();
                let prepared = result.map_err(|mut e| {
                    e.message = format!("{side} 第 {} 行 [{}]：{}", index + 1, row_id, e.message);
                    for d in &mut e.diagnostics {
                        let path = d
                            .get("path")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string();
                        d["path"] = json!(format!("/sides/{side}/rows/{index}/{path}"));
                    }
                    e
                })?;
                texts.push(prepared["program"].as_str().unwrap_or_default().to_string());
                rows.push(json!({"id": row_id, "prepared": prepared}));
                total += 1;
            }
            if rows.is_empty() {
                if target != "all" {
                    return Err(invalid("至少填写一个完整槽"));
                }
                continue;
            }
            sides[side] = json!({
                "rows": rows,
                "channels": [texts.join("\n")],
                "trajectory": {"preset_id":"v3-compatible","parameters":parameters},
                "confirmed_for": fingerprint(&parameters),
            });
        }
        if total == 0 {
            return Err(invalid("至少填写一个完整槽"));
        }
        Ok(json!({
            "sides": sides,
            "standard_id": standard["id"],
            "input_fingerprint": fingerprint(input),
            "all_or_nothing": true,
        }))
    }

    fn template_source(&self, config: &Value) -> PluginResult<String> {
        let name = config["template"].as_str().unwrap_or("program.j2");
        if name.contains(['/', '\\']) || name.is_empty() {
            return Err(invalid("config.template 必须是插件目录内文件名"));
        }
        std::fs::read_to_string(self.root.join("templates").join(name))
            .map_err(|e| PluginError::new("template_missing", format!("模板 {name} 不可读: {e}")))
    }

    fn generate(
        &self,
        input: &Value,
        server: &Arc<Server>,
        request_id: &str,
    ) -> PluginResult<Value> {
        let prepared = self.prepare(input, server, request_id)?;
        let source = self.template_source(&server.config())?;
        let mut artifacts = vec![];
        let mut diagnostics = vec![];
        for side in ["right", "left"] {
            let Some(s) = prepared["sides"].get(side) else {
                continue;
            };
            server.check_cancelled(request_id)?;
            let context = json!({
                "name": format!("undercut-{side}"),
                "side": side,
                "program": s["channels"][0],
                "rows": s["rows"].as_array().map(|r| r.len()).unwrap_or(0),
                "standard_id": prepared["standard_id"],
                "generated_by": "nc-undercut",
            });
            let result = server.call(
                request_id,
                "template.render",
                json!({"source": source, "context": context}),
            )?;
            let text = result["data"]["text"]
                .as_str()
                .ok_or_else(|| PluginError::new("render_invalid", "模板未返回正文"))?;
            if let Some(items) = result["diagnostics"].as_array() {
                diagnostics.extend(items.iter().cloned());
            }
            artifacts.push(json!({
                "name": format!("undercut-{side}.SPF"),
                "media_type": "text/plain",
                "text": text,
            }));
        }
        Ok(json!({
            "data": {
                "prepared": prepared,
                "replay_input": input,
                "process_id": std::process::id(),
            },
            "artifacts": artifacts,
            "diagnostics": diagnostics,
        }))
    }

    fn export_collection(&self) -> PluginResult<Value> {
        Ok(json!({"data": self.custom_standard()?.unwrap_or_else(
            || json!({"format_version":1,"tables":{"ES":{},"FS":{}}})
        )}))
    }

    fn validate_collection(collection: &Value) -> PluginResult<Value> {
        if collection["format_version"].as_u64() != Some(1) {
            return Err(invalid("集合 format_version 必须是 1"));
        }
        let tables = collection["tables"]
            .as_object()
            .ok_or_else(|| invalid("集合缺少 tables"))?;
        let mut count = 0usize;
        let mut mapping = json!({});
        for gtype in ["ES", "FS"] {
            let mut keys = vec![];
            if let Some(entries) = tables.get(gtype).and_then(Value::as_object) {
                for (key, entry) in entries {
                    let required: &[&str] = if gtype == "ES" {
                        &["r", "t1", "f", "ES_ang"]
                    } else {
                        &["r", "t1", "f", "t2", "FS_ang_1", "FS_ang_2"]
                    };
                    for field in required {
                        if !entry[field].is_number() {
                            return Err(invalid(&format!("标准 {key} 缺少数值字段 {field}")));
                        }
                    }
                    keys.push(key.clone());
                    count += 1;
                }
            }
            mapping[gtype] = json!(keys);
        }
        if count > 256 {
            return Err(invalid("自定义标准不能超过 256 项"));
        }
        Ok(json!({"count": count, "mapping": mapping, "valid": true}))
    }

    fn import_collection(&self, input: &Value) -> PluginResult<Value> {
        let path = self.root.join("data/custom-standards.json");
        if let Some(receipt) = input.get("rollback") {
            if receipt["previous"].is_null() {
                if path.exists() {
                    std::fs::remove_file(&path)?;
                }
            } else {
                let tmp = self.root.join("data/.custom-standards.tmp");
                std::fs::write(&tmp, serde_json::to_vec_pretty(&receipt["previous"])?)?;
                std::fs::rename(&tmp, &path)?;
            }
            return Ok(json!({"data": {"rolled_back": true}}));
        }
        let collection = &input["collection"];
        let report = Self::validate_collection(collection)?;
        if input["validate_only"].as_bool().unwrap_or(false) {
            return Ok(json!({"data": report}));
        }
        let previous = self.custom_standard()?.unwrap_or(Value::Null);
        let tmp = self.root.join("data/.custom-standards.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(collection)?)?;
        std::fs::rename(&tmp, &path)?;
        Ok(
            json!({"data": {"receipt": {"collection": "nc-undercut.standards", "previous": previous}}}),
        )
    }

    fn wait(input: &Value, server: &Arc<Server>, request_id: &str) -> PluginResult<Value> {
        let ms = input["milliseconds"].as_u64().unwrap_or(0).min(30_000);
        let ignore = input["ignore_cancel"].as_bool().unwrap_or(false);
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(ms);
        while std::time::Instant::now() < deadline {
            if !ignore && server.is_cancelled(request_id) {
                return Err(PluginError::new("cancelled", "fixture cancelled"));
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        Ok(json!({"data": {"process_id": std::process::id()}}))
    }

    pub fn invoke(
        &self,
        action: &str,
        input: &Value,
        server: &Arc<Server>,
        request_id: &str,
    ) -> PluginResult<Value> {
        match action {
            "nc-undercut.describe" => {
                let standard = self.merged_standard()?;
                let keys = |g: &str| {
                    standard["tables"][g]
                        .as_object()
                        .map(|t| t.keys().cloned().collect::<Vec<_>>())
                        .unwrap_or_default()
                };
                Ok(json!({"data": {
                    "schema": batch_schema(),
                    "trajectory_parameter_schema": trajectory::parameter_schema(),
                    "builtin_trajectory": trajectory::builtin(),
                    "units": {"diameter": "mm (直径)", "z": "mm", "angle": "deg",
                              "feed": "按机床程序进给模式"},
                    "standards": {"id": standard["id"], "builtin_id": "v3-grooves",
                                  "es": keys("ES"), "fs": keys("FS"),
                                  "custom_count": standard["custom_count"]},
                    "process_id": std::process::id(),
                }}))
            }
            "nc-undercut.standards" => {
                let standard = self.merged_standard()?;
                Ok(json!({"data": {
                    "standard": standard,
                    "fingerprint": fingerprint(&standard),
                }}))
            }
            "nc-undercut.prepare" => Ok(json!({"data": self.prepare(input, server, request_id)?})),
            "nc-undercut.generate" => self.generate(input, server, request_id),
            "nc-undercut.standards.export" => self.export_collection(),
            "nc-undercut.standards.import" => self.import_collection(input),
            "nc-undercut.wait" => Self::wait(input, server, request_id),
            _ => Err(PluginError::new("action_not_found", action)),
        }
    }
}

/// Input for `plugins-external` install validation is ordinary repo data.
pub fn plugin(root: &Path) -> Plugin {
    Plugin::new(root.to_path_buf())
}
