//! Dedicated dual-channel rough turning: raw input, recommendation and typed trajectories.
mod calculation;
use nctool_plugin_nc::{
    business::{self, BusinessAssets},
    trajectory::{self, Block, Plan, Word},
};
use nctool_plugin_sdk::*;
use std::path::Path;
pub struct SyncTurningPlugin {
    assets: BusinessAssets,
}
fn invalid(message: &str) -> PluginError {
    PluginError::new("sync_validation", message)
}
pub fn builtin() -> Value {
    json!({"format_version":1,"id":"v3-compatible","title":"V3 兼容轨迹","plugin":"sync-turning","strategy":"v3-paired","strategy_version":1,"source":"NCtool V3","parameters":{"clearance_x":10.,"retract_extra":4.,"left_entry_angle":25.,"left_entry_feed":0.2,"left_retract_feed":40.,"right_entry_feed":400.,"right_retract_feed":0.3}})
}
pub fn parameter_schema() -> Value {
    let mut properties = json!({});
    for k in [
        "clearance_x",
        "retract_extra",
        "left_entry_feed",
        "left_retract_feed",
        "right_entry_feed",
        "right_retract_feed",
    ] {
        properties[k] = json!({"type":"number","exclusiveMinimum":0,"maximum":10000});
    }
    properties["left_entry_angle"] = json!({"type":"number","minimum":1,"maximum":80});
    json!({"type":"object","properties":properties,"required":["clearance_x","retract_extra","left_entry_angle","left_entry_feed","left_retract_feed","right_entry_feed","right_retract_feed"],"additionalProperties":false})
}
fn validate_asset(group: &str, id: &str, a: &Value) -> PluginResult<()> {
    if a["format_version"] != 1 || a["id"] != id || !a["title"].is_string() {
        return Err(invalid("资产版本、ID 或名称无效"));
    }
    if group == "trajectories" {
        if id == "v3-compatible"
            || a["plugin"] != "sync-turning"
            || a["strategy"] != "v3-paired"
            || a["strategy_version"] != 1
        {
            return Err(invalid("内置轨迹须复制为新 ID；不支持此轨迹策略"));
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
fn trajectory_choice_schema() -> Value {
    json!({"type":"object","properties":{"preset_id":{"type":"string"},"overrides":{"type":"object"},"confirmed_for":{"type":"string"}},"additionalProperties":false})
}
pub fn batch_schema() -> Value {
    let row = json!({"type":"object","properties":{"id":{"type":"string","minLength":1},"sta_x":{"type":"number"},"sta_z":{"type":"number"},"end_x":{"type":"number"},"end_z":{"type":"number"},"ap_2":{"type":"number"}},"required":["id"],"additionalProperties":false});
    let side = json!({"type":"object","required":["ap_1","start_sync_num","rows"],"properties":{"ap_1":{"type":"number"},"ap2_min":{"type":"number"},"ap2_max":{"type":"number"},"start_sync_num":{"type":"integer","minimum":1,"maximum":9007199254740992u64},"rows":{"type":"array","maxItems":256,"items":row},"template":{"type":"string"},"trajectory":trajectory_choice_schema()},"additionalProperties":false});
    json!({"type":"object","properties":{"target":{"type":"string","enum":["right","left","all"]},"sync_feed":{"type":"number"},"sides":{"type":"object","properties":{"right":side,"left":side},"additionalProperties":false},"machine_id":{"type":"string"},"machine":{"type":"object"},"options":{"type":"object"},"job_snapshot":{"type":"object"}},"anyOf":[{"required":["sync_feed","sides"]},{"required":["job_snapshot"]}],"additionalProperties":false})
}
pub fn recommend(input: &Value) -> PluginResult<Value> {
    let v = |key| calculation::number(&input[key], key);
    let (start, end, ap, min, max) = (
        v("sta_x")?,
        v("end_x")?,
        v("ap_1")?,
        v("ap2_min")?,
        v("ap2_max")?,
    );
    if start <= 0. || end <= 0. || ap <= 0. || min < 0. || max <= 0. || min > max {
        return Err(invalid("推荐范围或直径无效"));
    }
    for n in 1..5000 {
        let average = (start - end) / n as f64;
        if average > ap + max + 1e-9 {
            continue;
        }
        if average < ap + min - 1e-9 {
            break;
        }
        let ap2 = average - ap;
        if ap2 >= min - 1e-9 && ap2 <= max + 1e-9 {
            let adopted = format!("{ap2:.2}").parse::<f64>().unwrap();
            let ratio = (start - end) / (ap + adopted);
            let near = ratio.round_ties_even();
            let actual = if near > 0. && (ratio - near).abs() <= 1e-8 {
                near
            } else {
                ratio.ceil()
            };
            let valid = adopted > 0. && adopted >= min && adopted <= max && actual == n as f64;
            return Ok(
                json!({"success":true,"cut_num":n,"ap_2":ap2,"adopted_ap_2":adopted,"generatable":valid,"diagnostic":if valid {""} else {"两位采用值为零、越界或改变刀次；请调整范围或手动输入"}}),
            );
        }
    }
    Ok(
        json!({"success":false,"generatable":false,"diagnostic":"没有匹配刀次；请调整 ap2 范围或手动输入"}),
    )
}
fn plans(prepared: &mut Value, p: &Value) -> PluginResult<Vec<Plan>> {
    let mut plans = vec![];
    let g = |k: &str| calculation::number(&p[k], k);
    let clearance = g("clearance_x")?;
    let extra = g("retract_extra")?;
    let angle = g("left_entry_angle")?;
    for c in prepared["contexts"].as_array_mut().unwrap() {
        let v = |k: &str| c[k].as_f64().unwrap();
        let (left, channel) = (c["side"] == "Left", c["channel"].as_u64().unwrap() as u8);
        let (start, end, sta_z, end_z, ap1, ap2, count, sync) = (
            v("actual_sta_x"),
            v("end_x"),
            v("sta_z"),
            v("end_z"),
            v("ap_1"),
            v("ap_2"),
            c["cut_num"].as_u64().unwrap(),
            c["sync_num_start"].as_u64().unwrap(),
        );
        let actual_z = sta_z + ((start - v("sta_x")) / 2.) / angle.to_radians().tan() - 2.;
        if left {
            c["actual_sta_z"] = json!(actual_z);
        }
        let mut plan = Plan::new(if left { "left" } else { "right" }, channel);
        plan.comment(if left {
            format!(";---SYT-Left--D{end:.1}---start---")
        } else {
            format!(";---SYT-Right-D{end:.1}---start---")
        });
        plan.motion(
            "approach",
            true,
            None,
            Some(if left { actual_z } else { sta_z + 4. }),
            None,
        );
        plan.motion("approach", true, Some(start + clearance), None, None);
        if !left {
            for w in c["warnings"].as_array().unwrap() {
                plan.comment(w.as_str().unwrap().into());
            }
        }
        plan.sync(sync);
        for i in 1..=count {
            let x = if channel == 1 {
                start - ap1 - (i - 1) as f64 * (ap1 + ap2)
            } else {
                start - i as f64 * (ap1 + ap2)
            };
            if left {
                let base =
                    actual_z - (i - 1) as f64 * (ap1 + ap2) / (2. * angle.to_radians().tan());
                let dis = if channel == 1 { 4. } else { 8. };
                plan.motion("approach", true, None, Some(base + dis / 2.), None);
                plan.motion(
                    "approach",
                    true,
                    Some(x + ap1 + if channel == 1 { dis } else { ap2 + dis }),
                    None,
                    None,
                );
                plan.sync(sync + i);
                let feed = Some(Word::short(g("left_entry_feed")?));
                if channel == 2 {
                    plan.motion("entry", false, None, Some(base + 2.), feed.clone());
                    *plan.last_motion().1 = Some(Word::short(225.));
                }
                plan.motion("entry", false, None, Some(base), feed.clone());
                *plan.last_motion().1 = Some(Word::short(225.));
                plan.motion("entry", false, Some(x), None, feed);
                *plan.last_motion().1 = Some(Word::short(180. + angle));
            } else {
                plan.motion(
                    "approach",
                    true,
                    None,
                    Some(sta_z + if channel == 1 { 2. } else { 4. }),
                    None,
                );
                plan.motion(
                    "entry",
                    false,
                    Some(x),
                    None,
                    Some(Word::short(g("right_entry_feed")?)),
                );
                plan.sync(sync + i);
            }
            let mut feed = Word::fixed(c["sync_feed"].as_f64().unwrap(), 2);
            feed.trim = left;
            plan.motion("cut", false, None, Some(end_z), Some(feed));
            plan.blocks.push(Block::Mode { relative: true });
            plan.motion(
                "retract",
                false,
                Some(ap1 + ap2 + extra),
                None,
                Some(Word::short(g(if left {
                    "left_retract_feed"
                } else {
                    "right_retract_feed"
                })?)),
            );
            if !left {
                plan.motion("retract", false, Some(2.), Some(1.), None);
                if let Block::Motion { x, z, .. } = plan.blocks.last_mut().unwrap() {
                    *x = Some(Word::fixed(2., 0));
                    *z = Some(Word::fixed(1., 0));
                }
            }
            plan.blocks.push(Block::Mode { relative: false });
        }
        plan.motion("tail", true, Some(start + clearance), None, None);
        plan.motion(
            "tail",
            true,
            None,
            Some(end_z + if left { 0. } else { 4. }),
            None,
        );
        plan.comment(";---End---".into());
        plans.push(plan);
    }
    if !plans.is_empty() {
        trajectory::validate_pair(&plans)?;
    }
    Ok(plans)
}
impl SyncTurningPlugin {
    pub fn new(root: &Path) -> PluginResult<Self> {
        Ok(Self {
            assets: BusinessAssets::new(root, "sync-turning")?,
        })
    }
    pub fn definition() -> PluginDescriptor {
        let mut d = PluginDescriptor::builtin("sync-turning");
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
            "recommend",
            "prepare",
            "generate",
            "check",
            "operation.prepare",
        ]
        .iter()
        .map(|v| {
            let s = format!("sync-turning.{v}");
            Service::new(&s, &s)
        })
        .collect();
        d.panels = vec![Panel {
            id: "sync-turning".into(),
            title: "同步粗车".into(),
            view: "sync-turning.workbench".into(),
            actions: Self::action_definitions()
                .iter()
                .map(|a| a.id.clone())
                .collect(),
        }];
        BusinessAssets::contribute(&mut d, "sync-turning");
        d
    }
    pub fn action_definitions() -> Vec<ActionDescriptor> {
        let mut actions = BusinessAssets::actions("sync-turning");
        for name in ["freeze", "prepare", "generate"] {
            actions.push(ActionDescriptor::new(
                &format!("sync-turning.{name}"),
                "同步粗车",
                batch_schema(),
            ));
        }
        actions.push(ActionDescriptor::new(
            "sync-turning.describe",
            "同步粗车能力",
            json!({"type":"object","additionalProperties":false}),
        ));
        actions.push(ActionDescriptor::new("sync-turning.check","核对同步粗车依赖",json!({"type":"object","required":["job_snapshot"],"properties":{"job_snapshot":{"type":"object"}},"additionalProperties":false})));
        let mut fields = json!({});
        for k in ["sta_x", "end_x", "ap_1", "ap2_min", "ap2_max"] {
            fields[k] = json!({"type":"number"});
        }
        actions.push(ActionDescriptor::new("sync-turning.recommend","推荐 ap2",json!({"type":"object","properties":fields,"required":["sta_x","end_x","ap_1","ap2_min","ap2_max"],"additionalProperties":false})));
        actions.push(ActionDescriptor::new("sync-turning.operation.prepare","兼容单条同步粗车",json!({"type":"object","properties":{"operation":{"type":"object"},"inputs":{"type":"object"},"standard":{},"cutting":{}},"required":["operation","inputs"],"additionalProperties":false})));
        actions
    }
    fn prepare(&self, saved: &Value, confirmed: bool, ctx: &Context) -> PluginResult<Value> {
        let input = &saved["input"];
        validate_schema(&batch_schema(), input, "sync_validation")?;
        let target = input["target"].as_str().unwrap_or("all");
        let mut sides = json!({});
        let mut total = 0;
        let mut total_cuts = 0u64;
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
            let trajectory_assets = business::trajectory_resources(saved, side, &json!({}));
            let asset = if confirmed {
                business::preset(&trajectory_assets, &choice)?
            } else {
                business::preview_preset(&trajectory_assets, &choice)?
            };
            if asset["strategy"] != "v3-paired" || asset["strategy_version"] != 1 {
                return Err(invalid("轨迹策略不可用"));
            }
            validate_schema(
                &parameter_schema(),
                &asset["parameters"],
                "trajectory_validation",
            )?;
            let mut cursor = settings["start_sync_num"].clone();
            let mut rows = vec![];
            let mut channels = [vec![], vec![]];
            let mut seen = std::collections::BTreeSet::new();
            for (index, row) in settings["rows"].as_array().unwrap().iter().enumerate() {
                ctx.cancellation.check()?;
                if !seen.insert(row["id"].as_str().unwrap()) {
                    return Err(invalid("行 ID 重复"));
                }
                if row.as_object().unwrap().len() == 1 {
                    continue;
                }
                let mut raw = row.clone();
                raw.as_object_mut().unwrap().remove("id");
                raw["ap_1"] = settings["ap_1"].clone();
                raw["sync_feed"] = input["sync_feed"].clone();
                raw["current_sync_num"] = cursor;
                let result = (|| {
                    validate_schema(
                        &nctool_plugin_nc::operation::input_schema(&format!("sync.{side}")),
                        &raw,
                        "operation_validation",
                    )?;
                    let mut p = business::prepare_metadata(
                        calculation::calculate(side, &raw)?,
                        &json!({"id":format!("sync.{side}"),"version":1}),
                        &raw,
                    )?;
                    total_cuts += p["contexts"][0]["cut_num"].as_u64().unwrap_or(0);
                    if total_cuts > 20_000 {
                        return Err(invalid("单次批量切削刀次合计不能超过 20000，请拆分任务"));
                    }
                    let paths = plans(&mut p, &asset["parameters"])?;
                    if let Some(skip) = p["skip_text"].as_str() {
                        for c in &mut channels {
                            c.push(skip.to_string());
                        }
                    } else {
                        for (i, path) in paths.iter().enumerate() {
                            channels[i].push(business::render_trajectory(
                                side,
                                &p["contexts"][i],
                                &path.render()?,
                                saved,
                                ctx,
                            )?);
                        }
                    }
                    p["trajectory_plans"] = serde_json::to_value(&paths)?;
                    p["previews"] = json!(paths
                        .iter()
                        .map(Plan::preview)
                        .collect::<PluginResult<Vec<_>>>()?);
                    Ok::<_, PluginError>(p)
                })();
                let p = result.map_err(|mut e| {
                    for d in &mut e.diagnostics {
                        d.path = Some(format!(
                            "/sides/{side}/rows/{index}/{}",
                            d.path.clone().unwrap_or_default()
                        ));
                    }
                    e.message =
                        format!("{side} 第 {} 行 [{}]：{}", index + 1, row["id"], e.message);
                    e
                })?;
                cursor = p["next_sync_num"].clone();
                rows.push(json!({"id":row["id"],"prepared":p}));
                total += 1;
            }
            if rows.is_empty() {
                if target != "all" {
                    return Err(invalid("至少填写一条完整台阶"));
                }
                continue;
            }
            sides[side] = json!({"rows":rows,"channels":channels.map(|c|c.join("\n")),"next_sync_num":cursor,"trajectory":asset,"confirmed_for":business::confirmation(&asset,&trajectory_assets)});
        }
        if total == 0 {
            return Err(invalid("至少填写一条完整台阶"));
        }
        Ok(
            json!({"sides":sides,"input_fingerprint":business::fingerprint(input),"all_or_nothing":true}),
        )
    }
}
impl Plugin for SyncTurningPlugin {
    fn descriptor(&self) -> PluginDescriptor {
        Self::definition()
    }
    fn actions(&self) -> Vec<ActionDescriptor> {
        Self::action_definitions()
    }
    fn invoke(&self, action: &str, input: Value, ctx: &Context) -> PluginResult<ActionResult> {
        match action {
            "sync-turning.freeze" => {
                business::snapshot(&input, &self.assets, builtin(), ctx).map(ActionResult::data)
            }
            "sync-turning.describe" => Ok(ActionResult::data(
                json!({"schema":batch_schema(),"trajectory_parameter_schema":parameter_schema(),"builtin_trajectory":builtin(),"units":{"x":"mm (直径)","z":"mm","ap":"mm (直径减量)","feed":"按机床程序进给模式"}}),
            )),
            "sync-turning.recommend" => recommend(&input).map(ActionResult::data),
            "sync-turning.operation.prepare" => {
                let kind = input["operation"]["id"].as_str().unwrap_or("");
                if !["sync.left", "sync.right"].contains(&kind) {
                    return Err(invalid("工艺标识无效"));
                }
                validate_schema(
                    &nctool_plugin_nc::operation::input_schema(kind),
                    &input["inputs"],
                    "operation_validation",
                )?;
                let mut prepared = calculation::calculate(
                    if kind == "sync.left" { "left" } else { "right" },
                    &input["inputs"],
                )?;
                prepared =
                    business::prepare_metadata(prepared, &input["operation"], &input["inputs"])?;
                let paths = plans(&mut prepared, &builtin()["parameters"])?;
                for (index, path) in paths.iter().enumerate() {
                    prepared["contexts"][index]["trajectory_program"] = json!(path.render()?);
                }
                business::prepare_metadata(prepared, &input["operation"], &input["inputs"])
                    .map(ActionResult::data)
            }

            "sync-turning.check" => {
                business::check(&input["job_snapshot"], &self.assets, ctx).map(ActionResult::data)
            }
            "sync-turning.prepare" | "sync-turning.generate" => {
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
                        for (i, text) in s["channels"].as_array().unwrap().iter().enumerate() {
                            ctx.cancellation.check()?;
                            let result = business::finalize(
                                &format!("sync-{side}-CH{}.SPF", i + 1),
                                text.as_str().unwrap(),
                                &saved,
                                ctx,
                            )?;
                            end_line_number = end_line_number
                                .max(result.data["end_line_number"].as_u64().unwrap_or(0));
                            artifacts.extend(result.artifacts);
                            diagnostics.extend(result.diagnostics);
                        }
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
