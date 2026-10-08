//! Dedicated ES/FS undercut workflow with frozen standards and typed paths.
mod calculation;
use nctool_plugin_nc::{
    business::{self, BusinessAssets},
    trajectory::{Plan, Word},
};
use nctool_plugin_sdk::*;
use std::path::Path;
pub struct UndercutPlugin {
    assets: BusinessAssets,
}
fn invalid(message: &str) -> PluginError {
    PluginError::new("undercut_validation", message)
}
pub fn builtin() -> Value {
    json!({"format_version":1,"id":"v3-compatible","title":"V3 兼容轨迹","plugin":"undercut","strategy":"v3-standard","strategy_version":1,"source":"NCtool V3","parameters":{"approach_extra":0.,"cut_feed":0.2,"fs_exit_extension":1.}})
}
pub fn parameter_schema() -> Value {
    json!({"type":"object","properties":{"approach_extra":{"type":"number","minimum":0,"maximum":1000},"cut_feed":{"type":"number","exclusiveMinimum":0,"maximum":10000},"fs_exit_extension":{"type":"number","exclusiveMinimum":0,"maximum":1000}},"required":["approach_extra","cut_feed","fs_exit_extension"],"additionalProperties":false})
}
fn validate_asset(group: &str, id: &str, a: &Value) -> PluginResult<()> {
    if a["format_version"] != 1 || a["id"] != id || !a["title"].is_string() {
        return Err(invalid("资产版本、ID 或名称无效"));
    }
    if group == "trajectories" {
        if id == "v3-compatible"
            || a["plugin"] != "undercut"
            || a["strategy"] != "v3-standard"
            || a["strategy_version"] != 1
        {
            return Err(invalid("内置轨迹须复制为新 ID；不支持此策略"));
        }
        validate_schema(
            &parameter_schema(),
            &a["parameters"],
            "trajectory_validation",
        )?;
    } else {
        validate_schema(&batch_schema(), &a["input"], "session_validation")?;
    }
    Ok(())
}
pub fn batch_schema() -> Value {
    let row = json!({"type":"object","required":["id"],"properties":{"id":{"type":"string","minLength":1},"gtype":{"type":"string","enum":["ES","FS"]},"std_key":{"type":"string"},"d1":{"type":"number"},"z":{"type":"number"},"d2":{"type":"number"},"c":{"type":"number"},"angle":{"type":"number"}},"additionalProperties":false});
    let side = json!({"type":"object","required":["rows"],"properties":{"rows":{"type":"array","items":row,"maxItems":256},"template":{"type":"string"},"trajectory":{"type":"object","properties":{"preset_id":{"type":"string"},"overrides":{"type":"object"},"confirmed_for":{"type":"string"}},"additionalProperties":false}},"additionalProperties":false});
    json!({"type":"object","properties":{"raw_diameter":{"type":"number"},"safety_dis":{"type":"number"},"target":{"type":"string","enum":["right","left","all"]},"sides":{"type":"object","properties":{"right":side,"left":side},"additionalProperties":false},"standard_id":{"type":"string"},"machine":{"type":"object"},"machine_id":{"type":"string"},"options":{"type":"object"},"job_snapshot":{"type":"object"}},"anyOf":[{"required":["raw_diameter","safety_dis","sides"]},{"required":["job_snapshot"]}],"additionalProperties":false})
}
fn plan(c: &mut Value, p: &Value) -> PluginResult<Plan> {
    c["X_START"] = json!(c["X_START"].as_f64().unwrap() + p["approach_extra"].as_f64().unwrap());
    if c["groove_type"] == "FS" {
        c["FS_Z_PLUS1"] = json!(
            c["FS_Z_PLUS1"].as_f64().unwrap() - 1. + p["fs_exit_extension"].as_f64().unwrap()
        );
        c["FS_Z_MINUS1"] = json!(
            c["FS_Z_MINUS1"].as_f64().unwrap() + 1. - p["fs_exit_extension"].as_f64().unwrap()
        );
    }
    let right = c["side"] == "Right";
    let es = c["groove_type"] == "ES";
    let v = |k: &str| c[k].as_f64().unwrap();
    let mut plan = Plan::new(if right { "right" } else { "left" }, 1);
    plan.comment(format!(
        "; --- {}_{} ({}) ---",
        if es { "ES" } else { "FS" },
        if right { "Right" } else { "Left" },
        c["STD_KEY"].as_str().unwrap()
    ));
    plan.motion("approach", true, None, Some(v("Z_START")), None);
    plan.motion("approach", true, Some(v("X_START")), None, None);
    plan.blocks
        .push(nctool_plugin_nc::trajectory::Block::Release);
    plan.motion("entry", true, Some(v("D1_COMP_START")), None, None);
    *plan.last_motion().0 = Some(if right { 42 } else { 41 });
    plan.motion(
        "cut",
        false,
        Some(v("D1_CUT")),
        None,
        Some(Word::short(p["cut_feed"].as_f64().unwrap())),
    );
    *plan.last_motion().1 = Some(Word::fixed(v("ANG_1"), 0));
    *plan.last_motion().2 = Some(Word::fixed(v("R"), 1));
    plan.motion(
        "cut",
        false,
        None,
        Some(v(if es { "ES_Z" } else { "Z_MID" })),
        None,
    );
    *plan.last_motion().2 = Some(Word::fixed(v("R"), 1));
    if es {
        plan.motion("cut", false, Some(v("D2_COMP_CUT")), None, None);
        plan.motion("cut", false, Some(v("ES_D2")), None, None);
        *plan.last_motion().1 = Some(Word::fixed(if right { 135. } else { 45. }, 0));
    } else {
        plan.motion(
            "cut",
            false,
            None,
            Some(v(if right { "FS_Z_PLUS1" } else { "FS_Z_MINUS1" })),
            None,
        );
        *plan.last_motion().1 = Some(Word::fixed(v("ANG_2"), 0));
    }
    plan.motion("retract", true, Some(v("X_START")), None, None);
    *plan.last_motion().0 = Some(40);
    plan.comment("; --------------------------------------".into());
    plan.render()?;
    Ok(plan)
}
impl UndercutPlugin {
    pub fn new(root: &Path) -> PluginResult<Self> {
        Ok(Self {
            assets: BusinessAssets::new(root, "undercut")?,
        })
    }
    pub fn definition() -> PluginDescriptor {
        let mut d = PluginDescriptor::builtin("undercut");
        d.requires = [
            "template.inspect",
            "nc.generate",
            "nc.foundation.freeze",
            "nc.foundation.check",
            "nc.output.finalize",
        ]
        .iter()
        .map(|s| Requirement::new(s))
        .collect();
        d.provides = [
            "freeze",
            "describe",
            "standards",
            "prepare",
            "generate",
            "check",
            "operation.prepare",
        ]
        .iter()
        .map(|v| {
            let s = format!("undercut.{v}");
            Service::new(&s, &s)
        })
        .collect();
        d.panels = vec![Panel {
            id: "undercut".into(),
            title: "越程槽".into(),
            view: "undercut.workbench".into(),
            actions: Self::action_definitions()
                .iter()
                .map(|a| a.id.clone())
                .collect(),
        }];
        BusinessAssets::contribute(&mut d, "undercut");
        d
    }
    pub fn action_definitions() -> Vec<ActionDescriptor> {
        let mut actions = BusinessAssets::actions("undercut");
        for name in ["freeze", "prepare", "generate"] {
            actions.push(ActionDescriptor::new(
                &format!("undercut.{name}"),
                "越程槽",
                batch_schema(),
            ));
        }
        actions.push(ActionDescriptor::new(
            "undercut.describe",
            "越程槽能力",
            json!({"type":"object","additionalProperties":false}),
        ));
        actions.push(ActionDescriptor::new("undercut.standards","查询 ES/FS 标准",json!({"type":"object","properties":{"standard_id":{"type":"string"}},"additionalProperties":false})));
        actions.push(ActionDescriptor::new("undercut.check","核对越程槽依赖",json!({"type":"object","required":["job_snapshot"],"properties":{"job_snapshot":{"type":"object"}},"additionalProperties":false})));
        actions.push(ActionDescriptor::new("undercut.operation.prepare","兼容单条越程槽",json!({"type":"object","required":["operation","inputs"],"properties":{"operation":{"type":"object"},"inputs":{"type":"object"},"standard":{},"cutting":{}},"additionalProperties":false})));
        actions
    }
    fn prepare(&self, saved: &Value, confirmed: bool, ctx: &Context) -> PluginResult<Value> {
        let input = &saved["input"];
        validate_schema(&batch_schema(), input, "undercut_validation")?;
        let standard = nctool_plugin_parameters::record(
            &saved["nc"]["nc_snapshot"],
            "standards",
            input["standard_id"].as_str().unwrap_or("v3-grooves"),
        )?;
        let target = input["target"].as_str().unwrap_or("all");
        let mut sides = json!({});
        let mut total = 0;
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
            let choice = settings.get("trajectory").cloned().unwrap_or(json!({}));
            let trajectory_assets = business::trajectory_resources(saved, side, standard);
            let asset = if confirmed {
                business::preset(&trajectory_assets, &choice)?
            } else {
                business::preview_preset(&trajectory_assets, &choice)?
            };
            if asset["strategy"] != "v3-standard" || asset["strategy_version"] != 1 {
                return Err(invalid("轨迹策略不可用"));
            }
            validate_schema(
                &parameter_schema(),
                &asset["parameters"],
                "trajectory_validation",
            )?;
            let mut rows = vec![];
            let mut texts = vec![];
            let mut seen = std::collections::BTreeSet::new();
            for (index, row) in settings["rows"].as_array().unwrap().iter().enumerate() {
                ctx.cancellation.check()?;
                if !seen.insert(row["id"].as_str().unwrap()) {
                    return Err(invalid("行 ID 重复"));
                }
                if row.as_object().unwrap().len() == 1 {
                    continue;
                }
                let es = row["gtype"] == "ES";
                if !es && ["d2", "c", "angle"].iter().any(|k| row.get(*k).is_some()) {
                    return Err(invalid("FS 不接受 ES 专用字段"));
                }
                let prefix = if es { "ES" } else { "FS" };
                let mut raw = json!({"gtype":row["gtype"],"std_key":row["std_key"],"side":if side=="right"{"Right"}else{"Left"},"raw_diameter":input["raw_diameter"],"safety_dis":input["safety_dis"]});
                raw[format!("{prefix}_d1")] = row["d1"].clone();
                raw[format!("{prefix}_z")] = row["z"].clone();
                if es {
                    raw["ES_d2"] = row["d2"].clone();
                    raw["ES_c"] = row["c"].clone();
                    raw["ES_c_ang"] = row["angle"].clone();
                }
                let result = (|| {
                    validate_schema(
                        &nctool_plugin_nc::operation::input_schema("groove"),
                        &raw,
                        "operation_validation",
                    )?;
                    let mut prepared = business::prepare_metadata(
                        calculation::calculate(&raw, standard)?,
                        &json!({"id":"groove","version":1}),
                        &raw,
                    )?;
                    let path = plan(&mut prepared["contexts"][0], &asset["parameters"])?;
                    texts.push(business::render_trajectory(
                        side,
                        &prepared["contexts"][0],
                        &path.render()?,
                        saved,
                        ctx,
                    )?);
                    prepared["previews"] = json!([path.preview()?]);
                    prepared["trajectory_plans"] = json!([path]);
                    Ok::<_, PluginError>(prepared)
                })();
                let prepared = result.map_err(|mut e| {
                    e.message =
                        format!("{side} 第 {} 行 [{}]：{}", index + 1, row["id"], e.message);
                    for d in &mut e.diagnostics {
                        d.path = Some(format!(
                            "/sides/{side}/rows/{index}/{}",
                            d.path.clone().unwrap_or_default()
                        ));
                    }
                    e
                })?;
                rows.push(json!({"id":row["id"],"prepared":prepared}));
                total += 1;
            }
            if rows.is_empty() {
                if target != "all" {
                    return Err(invalid("至少填写一个完整槽"));
                }
                continue;
            }
            sides[side] = json!({"rows":rows,"channels":[texts.join("\n")],"trajectory":asset,"confirmed_for":business::confirmation(&asset,&trajectory_assets)});
        }
        if total == 0 {
            return Err(invalid("至少填写一个完整槽"));
        }
        Ok(
            json!({"sides":sides,"standard":standard,"input_fingerprint":business::fingerprint(input),"all_or_nothing":true}),
        )
    }
}
impl Plugin for UndercutPlugin {
    fn descriptor(&self) -> PluginDescriptor {
        Self::definition()
    }
    fn actions(&self) -> Vec<ActionDescriptor> {
        Self::action_definitions()
    }
    fn invoke(&self, action: &str, input: Value, ctx: &Context) -> PluginResult<ActionResult> {
        match action {
            "undercut.freeze" => {
                business::snapshot(&input, &self.assets, builtin(), ctx).map(ActionResult::data)
            }
            "undercut.describe" => Ok(ActionResult::data(
                json!({"schema":batch_schema(),"trajectory_parameter_schema":parameter_schema(),"builtin_trajectory":builtin(),"units":{"diameter":"mm (直径)","z":"mm","angle":"deg","feed":"按机床程序进给模式"}}),
            )),
            "undercut.standards" => {
                let saved = business::snapshot(&json!({}), &self.assets, builtin(), ctx)?;
                let standard = nctool_plugin_parameters::record(
                    &saved["nc"]["nc_snapshot"],
                    "standards",
                    input["standard_id"].as_str().unwrap_or("v3-grooves"),
                )?;
                Ok(ActionResult::data(
                    json!({"standard":standard,"fingerprint":business::fingerprint(standard)}),
                ))
            }
            "undercut.operation.prepare" => {
                if input["operation"]["id"] != "groove" {
                    return Err(invalid("工艺标识无效"));
                }
                validate_schema(
                    &nctool_plugin_nc::operation::input_schema("groove"),
                    &input["inputs"],
                    "operation_validation",
                )?;
                let standard = input
                    .get("standard")
                    .cloned()
                    .unwrap_or(serde_json::from_str(include_str!(
                        "../../nc/src/operation_standards.json"
                    ))?);
                let mut prepared = calculation::calculate(&input["inputs"], &standard)?;
                prepared =
                    business::prepare_metadata(prepared, &input["operation"], &input["inputs"])?;
                let path = plan(&mut prepared["contexts"][0], &builtin()["parameters"])?;
                prepared["contexts"][0]["trajectory_program"] = json!(path.render()?);
                business::prepare_metadata(prepared, &input["operation"], &input["inputs"])
                    .map(ActionResult::data)
            }

            "undercut.check" => {
                business::check(&input["job_snapshot"], &self.assets, ctx).map(ActionResult::data)
            }
            "undercut.prepare" | "undercut.generate" => {
                let mut saved = business::snapshot(&input, &self.assets, builtin(), ctx)?;
                let prepared = self.prepare(&saved, action.ends_with("generate"), ctx)?;
                if action.ends_with("prepare") {
                    return Ok(ActionResult::data(prepared));
                }
                let mut artifacts = vec![];
                let mut diagnostics = vec![];
                let mut end_line_number = 0;
                for side in ["right", "left"] {
                    if let Some(s) = prepared["sides"].get(side) {
                        ctx.cancellation.check()?;
                        let result = business::finalize(
                            &format!("undercut-{side}.SPF"),
                            s["channels"][0].as_str().unwrap(),
                            &saved,
                            ctx,
                        )?;
                        end_line_number = end_line_number
                            .max(result.data["end_line_number"].as_u64().unwrap_or(0));
                        artifacts.extend(result.artifacts);
                        diagnostics.extend(result.diagnostics);
                    }
                }
                saved["prepared"] = prepared.clone();
                Ok(ActionResult {
                    data: json!({"prepared":prepared,"end_line_number":end_line_number,"replay_input":{"job_snapshot":saved}}),
                    artifacts,
                    diagnostics,
                })
            }
            _ => self.assets.library.invoke(
                action.rsplit('.').next().unwrap_or(""),
                &input,
                validate_asset,
            ),
        }
    }
}
