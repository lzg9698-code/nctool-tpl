//! Typed NC path plans. Presets choose strategies; no executable code is stored.
use nctool_plugin_sdk::*;
use serde::{Deserialize, Serialize};

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
pub fn invalid(message: &str) -> PluginError {
    PluginError::new("trajectory_validation", message)
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
    pub fn sync(&mut self, number: u64) {
        self.blocks.push(Block::Sync {
            event: format!("barrier-{number}"),
            number,
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
pub fn validate_pair(plans: &[Plan]) -> PluginResult<()> {
    if plans.len() != 2
        || plans[0].channel != 1
        || plans[1].channel != 2
        || plans[0].side != plans[1].side
    {
        return Err(invalid("需要同侧的 CH1/CH2 配对计划"));
    }
    let events = |p: &Plan| {
        p.blocks
            .iter()
            .filter_map(|b| match b {
                Block::Sync { event, number } => Some((event.clone(), *number)),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    if events(&plans[0]) != events(&plans[1]) {
        return Err(invalid("两个通道同步事件不一致"));
    }
    for p in plans {
        p.render()?;
    }
    Ok(())
}
