//! Typed NC path plans (Siemens-style output) and the V3-compatible undercut
//! strategy. Ported from the retired `plugins/nc/src/trajectory.rs` and the
//! `plan()` routine in `plugins/undercut/src/lib.rs`.
use crate::rpc::{PluginError, PluginResult};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

fn invalid(message: &str) -> PluginError {
    PluginError::new("trajectory_validation", message)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Word {
    pub value: f64,
    pub precision: usize,
    pub trim: bool,
}
impl Word {
    pub fn fixed(value: f64, precision: usize) -> Self {
        Self {
            value,
            precision,
            trim: false,
        }
    }
    pub fn short(value: f64) -> Self {
        Self {
            value,
            precision: 6,
            trim: true,
        }
    }
    pub fn text(&self) -> PluginResult<String> {
        if !self.value.is_finite() || self.value.abs() > 1e12 || self.precision > 6 {
            return Err(invalid("轨迹数值或精度越界"));
        }
        let text = format!("{:.*}", self.precision, self.value);
        Ok(if self.trim {
            text.trim_end_matches('0').trim_end_matches('.').to_string()
        } else {
            text
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum Block {
    Motion {
        stage: String,
        rapid: bool,
        compensation: Option<u8>,
        x: Option<Word>,
        z: Option<Word>,
        angle: Option<Word>,
        radius: Option<Word>,
        feed: Option<Word>,
    },
    Mode {
        relative: bool,
    },
    Sync {
        event: String,
        number: u64,
    },
    Release,
    Comment {
        text: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub format_version: u32,
    pub side: String,
    pub channel: u8,
    pub blocks: Vec<Block>,
}

impl Plan {
    pub fn new(side: &str, channel: u8) -> Self {
        Self {
            format_version: 1,
            side: side.into(),
            channel,
            blocks: vec![],
        }
    }
    pub fn motion(
        &mut self,
        stage: &str,
        rapid: bool,
        x: Option<f64>,
        z: Option<f64>,
        feed: Option<Word>,
    ) {
        self.blocks.push(Block::Motion {
            stage: stage.into(),
            rapid,
            compensation: None,
            x: x.map(|v| Word::fixed(v, 3)),
            z: z.map(|v| Word::fixed(v, 3)),
            angle: None,
            radius: None,
            feed,
        });
    }
    pub fn comment(&mut self, text: String) {
        self.blocks.push(Block::Comment { text });
    }
    pub fn last_motion(&mut self) -> (&mut Option<u8>, &mut Option<Word>, &mut Option<Word>) {
        match self.blocks.last_mut().unwrap() {
            Block::Motion {
                compensation,
                angle,
                radius,
                ..
            } => (compensation, angle, radius),
            _ => unreachable!(),
        }
    }
    pub fn render(&self) -> PluginResult<String> {
        if self.format_version != 1
            || !["left", "right"].contains(&self.side.as_str())
            || !(1..=2).contains(&self.channel)
            || self.blocks.len() > 160_005
        {
            return Err(invalid("轨迹计划版本、侧别、通道或长度无效"));
        }
        let mut lines = vec![];
        let mut relative = false;
        let mut compensation = 0;
        let mut last_sync = 0;
        for block in &self.blocks {
            lines.push(match block {
                Block::Motion {
                    stage,
                    rapid,
                    compensation: change,
                    x,
                    z,
                    angle,
                    radius,
                    feed,
                } => {
                    if !["approach", "entry", "cut", "retract", "tail"].contains(&stage.as_str())
                        || (x.is_none() && z.is_none())
                    {
                        return Err(invalid("无效运动阶段或缺少坐标"));
                    }
                    let mut words = vec![if *rapid { "G0".into() } else { "G1".into() }];
                    if let Some(c) = change {
                        if ![40, 41, 42].contains(c)
                            || (*c != 40 && compensation != 0)
                            || (*c == 40 && compensation == 0)
                        {
                            return Err(invalid("非法刀补状态转换"));
                        }
                        compensation = if *c == 40 { 0 } else { *c };
                        words.push(format!("G{c}"));
                    }
                    if let Some(v) = x {
                        words.push(format!("X{}", v.text()?));
                    }
                    if let Some(v) = z {
                        words.push(format!("Z{}", v.text()?));
                    }
                    if let Some(v) = angle {
                        words.push(format!("ANG={}", v.text()?));
                    }
                    if let Some(v) = radius {
                        if v.value <= 0. {
                            return Err(invalid("圆角必须大于零"));
                        }
                        words.push(format!("RND={}", v.text()?));
                    }
                    if let Some(v) = feed {
                        if *rapid || v.value <= 0. {
                            return Err(invalid("进给模式或数值无效"));
                        }
                        words.push(format!("F{}", v.text()?));
                    }
                    words.join(" ")
                }
                Block::Mode { relative: next } => {
                    relative = *next;
                    if relative {
                        "G91".into()
                    } else {
                        "G90".into()
                    }
                }
                Block::Sync { number, .. } => {
                    if *number <= last_sync || *number > 1 << 53 {
                        return Err(invalid("同步号必须精确且递增"));
                    }
                    last_sync = *number;
                    format!("WAITM({number},1,2)")
                }
                Block::Release => "M4=97".into(),
                Block::Comment { text } => {
                    if !text.starts_with(';') || text.contains(['\n', '\r']) {
                        return Err(invalid("无效注释"));
                    }
                    text.clone()
                }
            });
        }
        if relative || compensation != 0 {
            return Err(invalid("轨迹结束时坐标模式或刀补未恢复"));
        }
        Ok(format!("{}\n", lines.join("\n")))
    }
    /// Nominal points use the emitted precision. ANG/RND remain marked as schematic.
    pub fn preview(&self) -> PluginResult<Value> {
        self.render()?;
        let mut x = None;
        let mut z = None;
        let mut relative = false;
        let mut points = vec![];
        let mut approximate = false;
        for block in &self.blocks {
            match block {
                Block::Mode { relative: r } => relative = *r,
                Block::Motion {
                    x: nx,
                    z: nz,
                    rapid,
                    stage,
                    angle,
                    radius,
                    ..
                } => {
                    if angle.is_some() || radius.is_some() {
                        approximate = true;
                    }
                    for (target, word) in [(&mut x, nx), (&mut z, nz)] {
                        if let Some(w) = word {
                            let v = w
                                .text()?
                                .parse::<f64>()
                                .map_err(|_| invalid("预览数值无效"))?;
                            *target = Some(if relative {
                                target.unwrap_or(0.) + v
                            } else {
                                v
                            });
                        }
                    }
                    if let (Some(x), Some(z)) = (x, z) {
                        points.push(json!({"x":x,"z":z,"rapid":rapid,"stage":stage}));
                    }
                }
                _ => {}
            }
        }
        Ok(
            json!({"points":points,"nominal":true,"approximate":approximate,"label":if approximate {"计划坐标示意（ANG/RND、刀补未仿真）"} else {"计划坐标示意（刀补未仿真）"}}),
        )
    }
}

/// The V3-compatible trajectory preset, now a fixed strategy inside the plugin
/// rather than a workspace asset.
pub fn builtin() -> Value {
    json!({"format_version":1,"id":"v3-compatible","title":"V3 兼容轨迹","plugin":"undercut","strategy":"v3-standard","strategy_version":1,"source":"NCtool V3","parameters":{"approach_extra":0.,"cut_feed":0.2,"fs_exit_extension":1.}})
}
pub fn parameter_schema() -> Value {
    json!({"type":"object","properties":{"approach_extra":{"type":"number","minimum":0,"maximum":1000},"cut_feed":{"type":"number","exclusiveMinimum":0,"maximum":10000},"fs_exit_extension":{"type":"number","exclusiveMinimum":0,"maximum":1000}},"required":["approach_extra","cut_feed","fs_exit_extension"],"additionalProperties":false})
}

/// Build the trajectory plan for one computed groove context with strategy
/// parameters `p` (see `parameter_schema`). Identical to the retired builtin
/// `plan()`: same motion order, compensation transitions and word precision.
pub fn plan(c: &mut Value, p: &Value) -> PluginResult<Plan> {
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
    plan.blocks.push(Block::Release);
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn renders_v3_es_right() {
        let mut c = json!({"STD_KEY":"DIN509-E 0,2 x 0,1","groove_type":"ES","side":"Right",
            "X_START":104.,"R":0.2,"Z_START":31.,"D1_CUT":79.8,"ANG_1":-165.,"D1_COMP_START":84.,
            "ES_Z":30.,"ES_D2":85.,"D2_COMP_CUT":84.,});
        let text = plan(&mut c, &builtin()["parameters"])
            .unwrap()
            .render()
            .unwrap();
        assert!(text.contains("G1 X79.800 ANG=-165 RND=0.2 F0.2"));
        assert!(text.contains("G41") == false); // right side uses G42
        assert!(text.contains("G42"));
        assert!(text.ends_with("; --------------------------------------\n"));
    }
    #[test]
    fn compensation_must_close() {
        let mut c = json!({"STD_KEY":"x","groove_type":"FS","side":"Left",
            "X_START":104.,"R":0.2,"Z_START":31.,"D1_CUT":79.8,"ANG_1":-15.,"D1_COMP_START":84.,
            "Z_MID":30.,"FS_Z_PLUS1":31.,"FS_Z_MINUS1":29.,"ANG_2":78.,});
        let p = plan(&mut c, &builtin()["parameters"]).unwrap();
        let text = p.render().unwrap();
        assert!(text.contains("G41"));
        assert!(text.contains("G40"));
    }
}
