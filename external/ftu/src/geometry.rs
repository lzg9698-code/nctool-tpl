//! Independent analytical finishing geometry; coordinates are z / true radius, mm.
use nctool_plugin_sdk::{Cancellation, PluginError, PluginResult};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    f64::consts::{PI, TAU},
};

pub const EPS: f64 = 1e-8;
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Point {
    pub z: f64,
    pub r: f64,
}
impl Point {
    pub fn add(self, p: Self) -> Self {
        Self {
            z: self.z + p.z,
            r: self.r + p.r,
        }
    }
    pub fn sub(self, p: Self) -> Self {
        Self {
            z: self.z - p.z,
            r: self.r - p.r,
        }
    }
    pub fn scale(self, n: f64) -> Self {
        Self {
            z: self.z * n,
            r: self.r * n,
        }
    }
    pub fn dot(self, p: Self) -> f64 {
        self.z * p.z + self.r * p.r
    }
    pub fn cross(self, p: Self) -> f64 {
        self.z * p.r - self.r * p.z
    }
    pub fn length(self) -> f64 {
        self.z.hypot(self.r)
    }
    fn unit(self) -> Self {
        self.scale(1.0 / self.length())
    }
    fn left(self) -> Self {
        Self {
            z: -self.r,
            r: self.z,
        }
    }
    fn angle(self) -> f64 {
        self.r.atan2(self.z)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Corner {
    pub r#type: String,
    pub value: f64,
    pub feed_per_rev: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Feature {
    pub id: String,
    pub r#type: String,
    pub length: Option<f64>,
    pub diameter: Option<f64>,
    pub feed_per_rev: f64,
    pub corner: Corner,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Lead {
    pub r#type: String,
    pub length: f64,
    pub radius: f64,
    pub angle: f64,
    pub clearance: f64,
    pub feed_per_rev: f64,
    pub clearance_feed_per_rev: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Cutting {
    pub mode: String,
    pub surface_speed_m_per_min: f64,
    pub max_spindle_rpm: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Zone {
    pub z_min: f64,
    pub z_max: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Stock {
    pub z_min: f64,
    pub z_max: f64,
    pub outer_diameter: f64,
    pub inner_diameter: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Project {
    pub version: u32,
    pub mode: String,
    pub start_z: f64,
    pub start_diameter: f64,
    pub direction: i32,
    pub reverse: bool,
    pub cutting: Cutting,
    pub features: Vec<Feature>,
    pub entry: Lead,
    pub exit: Lead,
    pub stock: Option<Stock>,
    pub forbidden: Vec<Zone>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Segment {
    pub id: String,
    pub feature_id: String,
    pub role: String,
    pub kind: String,
    pub start: Point,
    pub end: Point,
    pub center: Option<Point>,
    pub sweep: f64,
    pub feed_per_rev: f64,
}
impl Segment {
    fn line(a: Point, b: Point, id: &str, role: &str, feed: f64) -> Self {
        Self {
            id: String::new(),
            feature_id: id.into(),
            role: role.into(),
            kind: "line".into(),
            start: a,
            end: b,
            center: None,
            sweep: 0.0,
            feed_per_rev: feed,
        }
    }
    pub fn length(&self) -> f64 {
        if let Some(c) = self.center {
            self.start.sub(c).length() * self.sweep.abs()
        } else {
            self.end.sub(self.start).length()
        }
    }
    pub fn at(&self, t: f64) -> Point {
        if let Some(c) = self.center {
            let a = self.start.sub(c).angle() + self.sweep * t;
            let r = self.start.sub(c).length();
            c.add(Point {
                z: r * a.cos(),
                r: r * a.sin(),
            })
        } else {
            self.start.add(self.end.sub(self.start).scale(t))
        }
    }
    fn tangent(&self, t: f64) -> Point {
        if let Some(c) = self.center {
            self.at(t).sub(c).left().unit().scale(self.sweep.signum())
        } else {
            self.end.sub(self.start).unit()
        }
    }
    fn parameter(&self, p: Point) -> Option<f64> {
        let t = if let Some(c) = self.center {
            if (p.sub(c).length() - self.start.sub(c).length()).abs() > EPS {
                return None;
            }
            let mut a = ((p.sub(c).angle() - self.start.sub(c).angle()) * self.sweep.signum())
                .rem_euclid(TAU);
            if (TAU - a).abs() < EPS {
                a = 0.0;
            }
            a / self.sweep.abs()
        } else {
            let v = self.end.sub(self.start);
            if v.cross(p.sub(self.start)).abs() > EPS * v.length().max(1.0) {
                return None;
            }
            p.sub(self.start).dot(v) / v.dot(v)
        };
        if (-EPS..=1.0 + EPS).contains(&t) {
            Some(t.clamp(0.0, 1.0))
        } else {
            None
        }
    }
    fn axis(&self, radial: bool, value: f64) -> Vec<f64> {
        if let Some(c) = self.center {
            let radius = self.start.sub(c).length();
            let q = (value - if radial { c.r } else { c.z }) / radius;
            if q.abs() > 1.0 + EPS {
                return vec![];
            }
            let q = q.clamp(-1.0, 1.0);
            let a = if radial { q.asin() } else { q.acos() };
            [a, if radial { PI - a } else { -a }]
                .iter()
                .filter_map(|a| {
                    self.parameter(c.add(Point {
                        z: radius * a.cos(),
                        r: radius * a.sin(),
                    }))
                })
                .collect()
        } else {
            let delta = if radial {
                self.end.r - self.start.r
            } else {
                self.end.z - self.start.z
            };
            if delta.abs() < EPS {
                vec![]
            } else {
                let t = (value - if radial { self.start.r } else { self.start.z }) / delta;
                if (0.0..=1.0).contains(&t) {
                    vec![t]
                } else {
                    vec![]
                }
            }
        }
    }
    pub fn extrema(&self) -> Vec<Point> {
        let mut p = vec![self.start, self.end];
        if let Some(c) = self.center {
            for a in [0.0, PI / 2.0, PI, PI * 1.5] {
                let radius = self.start.sub(c).length();
                let q = c.add(Point {
                    z: radius * a.cos(),
                    r: radius * a.sin(),
                });
                if self.parameter(q).is_some() {
                    p.push(q);
                }
            }
        }
        p
    }
    fn reversed(&self) -> Self {
        let mut s = self.clone();
        std::mem::swap(&mut s.start, &mut s.end);
        s.sweep = -s.sweep;
        s
    }
}
pub fn intersections(a: &Segment, b: &Segment) -> Vec<Point> {
    let mut points = vec![];
    match (a.center, b.center) {
        (None, None) => {
            let u = a.end.sub(a.start);
            let v = b.end.sub(b.start);
            let d = u.cross(v);
            if d.abs() > EPS {
                points.push(a.start.add(u.scale(b.start.sub(a.start).cross(v) / d)));
            } else if b.start.sub(a.start).cross(u).abs() < EPS {
                points.extend([a.start, a.end, b.start, b.end]);
            }
        }
        (Some(c), Some(d)) => {
            let v = d.sub(c);
            let l = v.length();
            let r = a.start.sub(c).length();
            let s = b.start.sub(d).length();
            if l < EPS {
                if (r - s).abs() < EPS {
                    points.extend([a.start, a.end, b.start, b.end]);
                }
            } else if l <= r + s + EPS && l >= (r - s).abs() - EPS {
                let x = (r * r - s * s + l * l) / (2.0 * l);
                let h = (r * r - x * x).max(0.0).sqrt();
                let mid = c.add(v.scale(x / l));
                let off = v.left().scale(h / l);
                points.extend([mid.add(off), mid.sub(off)]);
            }
        }
        _ => {
            let (line, arc) = if a.center.is_none() { (a, b) } else { (b, a) };
            let c = arc.center.unwrap();
            let u = line.end.sub(line.start).unit();
            let mid = line.start.add(u.scale(c.sub(line.start).dot(u)));
            let dist = mid.sub(c).length();
            let radius = arc.start.sub(c).length();
            if dist <= radius + EPS {
                let h = (radius * radius - dist * dist).max(0.0).sqrt();
                points.extend([mid.add(u.scale(h)), mid.sub(u.scale(h))]);
            }
        }
    }
    points
        .into_iter()
        .filter(|p| a.parameter(*p).is_some() && b.parameter(*p).is_some())
        .collect()
}
#[derive(Clone, Debug, Serialize)]
pub struct Issue {
    pub code: String,
    pub message: String,
    pub path: String,
}
#[derive(Clone, Debug, Serialize)]
pub struct Analysis {
    pub valid: bool,
    pub profile: Vec<Segment>,
    pub segments: Vec<Segment>,
    pub length: f64,
    pub diagnostics: Vec<Issue>,
}
fn fail(msg: &str) -> PluginError {
    PluginError::new("invalid_project", msg)
}
fn positive(v: f64) -> bool {
    v.is_finite() && v > 0.0 && v <= 1e9
}
pub fn analyze(p: &Project, cancel: &Cancellation) -> PluginResult<Analysis> {
    let mut a = Analysis {
        valid: false,
        profile: vec![],
        segments: vec![],
        length: 0.0,
        diagnostics: vec![],
    };
    match calculate(p, cancel) {
        Ok((profile, segments, issues)) => {
            a.length = segments.iter().map(Segment::length).sum();
            a.profile = profile;
            a.segments = segments;
            a.diagnostics = issues;
            a.valid = a.diagnostics.is_empty();
        }
        Err(e) if e.code == "cancelled" => return Err(e),
        Err(e) => a.diagnostics.push(Issue {
            code: e.code,
            message: e.message,
            path: "/project".into(),
        }),
    };
    Ok(a)
}
fn calculate(
    p: &Project,
    cancel: &Cancellation,
) -> PluginResult<(Vec<Segment>, Vec<Segment>, Vec<Issue>)> {
    cancel.check()?;
    if p.version != 1
        || !["outside", "inside"].contains(&p.mode.as_str())
        || ![-1, 1].contains(&p.direction)
        || !p.start_z.is_finite()
        || p.start_z.abs() > 1e9
        || !positive(p.start_diameter)
        || p.features.is_empty()
        || p.features.len() > 500
        || p.forbidden.len() > 100
    {
        return Err(fail("项目版本、坐标或特征数量无效"));
    }
    if p.cutting.mode != "constant-surface-speed"
        || !positive(p.cutting.surface_speed_m_per_min)
        || !positive(p.cutting.max_spindle_rpm)
    {
        return Err(fail("恒线速度及最高转速必须为有限正数"));
    }
    let mut ids = BTreeSet::new();
    let mut raw = vec![];
    let mut position = Point {
        z: p.start_z,
        r: p.start_diameter / 2.0,
    };
    let mut prior = 0.0;
    for f in &p.features {
        cancel.check()?;
        if f.id.is_empty()
            || !ids.insert(&f.id)
            || !["cylinder", "cone", "face"].contains(&f.r#type.as_str())
            || !["none", "chamfer", "fillet"].contains(&f.corner.r#type.as_str())
            || !positive(f.feed_per_rev)
            || !positive(f.corner.feed_per_rev)
        {
            return Err(fail("特征标识、类型或进给无效"));
        }
        let dz = if f.r#type == "face" {
            0.0
        } else {
            let l = f.length.ok_or_else(|| fail("缺少轴向长度"))?;
            if !positive(l) {
                return Err(fail("长度必须为正数"));
            }
            l * p.direction as f64
        };
        let r = if f.r#type == "cylinder" {
            position.r
        } else {
            let d = f.diameter.ok_or_else(|| fail("缺少终点直径"))?;
            if !positive(d) {
                return Err(fail("直径必须为正数"));
            }
            d / 2.0
        };
        let next = Point {
            z: position.z + dz,
            r,
        };
        if next.sub(position).length() < EPS {
            return Err(fail("轮廓存在零长度段"));
        }
        let dr = next.r - position.r;
        if dr.abs() > EPS {
            let sign = dr.signum() * if p.mode == "outside" { 1.0 } else { -1.0 };
            if prior < 0.0 && sign > 0.0 {
                return Err(fail("不支持槽形回勾，请拆分轮廓"));
            }
            prior = sign;
        }
        raw.push(Segment::line(
            position,
            next,
            &f.id,
            "finish",
            f.feed_per_rev,
        ));
        position = next;
    }
    if p.features.last().unwrap().corner.r#type != "none" {
        return Err(fail("末段不能设置连接修饰"));
    }
    let mut trims = vec![(0.0, 0.0); raw.len()];
    let mut joins = vec![None; raw.len()];
    for i in 0..raw.len() - 1 {
        let f = &p.features[i];
        if f.corner.r#type == "none" {
            continue;
        }
        if !positive(f.corner.value) {
            return Err(fail("连接修饰尺寸必须为正数"));
        }
        let u = raw[i].tangent(1.0);
        let v = raw[i + 1].tangent(0.0);
        let turn = u.cross(v).atan2(u.dot(v));
        if turn.abs() < EPS || turn.abs() >= PI - EPS {
            return Err(fail("共线或折返不能设置连接修饰"));
        }
        let d = if f.corner.r#type == "chamfer" {
            f.corner.value
        } else {
            f.corner.value * (turn.abs() / 2.0).tan()
        };
        trims[i].1 = d;
        trims[i + 1].0 = d;
        let start = raw[i].end.sub(u.scale(d));
        let end = raw[i + 1].start.add(v.scale(d));
        let mut s = Segment::line(start, end, &f.id, "finish", f.corner.feed_per_rev);
        if f.corner.r#type == "fillet" {
            s.kind = "arc".into();
            s.center = Some(start.add(u.left().scale(turn.signum() * f.corner.value)));
            s.sweep = turn;
        }
        joins[i] = Some(s);
    }
    let mut profile = vec![];
    for (i, s) in raw.iter().enumerate() {
        let (start, end) = trims[i];
        if start + end > s.length() + EPS {
            return Err(fail("连接修饰占用超过可用长度"));
        }
        if s.length() - start - end > EPS {
            let mut trimmed = s.clone();
            let u = s.tangent(0.0);
            trimmed.start = s.start.add(u.scale(start));
            trimmed.end = s.end.sub(u.scale(end));
            profile.push(trimmed);
        }
        if let Some(s) = joins[i].take() {
            profile.push(s);
        }
    }
    if profile.is_empty() {
        return Err(fail("轮廓为空"));
    }
    for i in 0..profile.len() {
        cancel.check()?;
        for j in i + 2..profile.len() {
            if !intersections(&profile[i], &profile[j]).is_empty() {
                return Err(fail("轮廓存在自交或非相邻接触"));
            }
        }
    }
    let ordered: Vec<_> = if p.reverse {
        profile.iter().rev().map(Segment::reversed).collect()
    } else {
        profile.clone()
    };
    let mut segments = leads(&p.entry, true, &ordered, p)?;
    segments.extend(ordered.clone());
    segments.extend(leads(&p.exit, false, &ordered, p)?);
    let points: Vec<_> = profile.iter().flat_map(Segment::extrema).collect();
    let zmin = points.iter().map(|p| p.z).fold(f64::INFINITY, f64::min);
    let zmax = points.iter().map(|p| p.z).fold(f64::NEG_INFINITY, f64::max);
    for zone in &p.forbidden {
        if !zone.z_min.is_finite() || !zone.z_max.is_finite() || zone.z_min >= zone.z_max {
            return Err(fail("禁入区范围无效"));
        }
    }
    if let Some(stock) = &p.stock {
        if !stock.z_min.is_finite()
            || !stock.z_max.is_finite()
            || stock.z_min >= stock.z_max
            || !positive(stock.outer_diameter)
            || !stock.inner_diameter.is_finite()
            || stock.inner_diameter < 0.0
            || stock.inner_diameter >= stock.outer_diameter
        {
            return Err(fail("毛坯包络无效"));
        }
        if points.iter().any(|q| {
            q.z < stock.z_min - EPS
                || q.z > stock.z_max + EPS
                || q.r < stock.inner_diameter / 2.0 - EPS
                || q.r > stock.outer_diameter / 2.0 + EPS
        }) {
            return Err(fail("成品轮廓超出毛坯包络"));
        }
    }
    let mut issues = vec![];
    for (index, s) in segments.iter_mut().enumerate() {
        cancel.check()?;
        s.id = format!("s{}", index + 1);
        let mut ts = vec![0.0, 1.0];
        for q in s.extrema() {
            if let Some(t) = s.parameter(q) {
                ts.push(t);
            }
        }
        for b in &profile {
            for q in intersections(s, b) {
                if let Some(t) = s.parameter(q) {
                    ts.push(t);
                }
            }
        }
        let mut zz = vec![zmin, zmax];
        for f in &p.forbidden {
            zz.extend([f.z_min, f.z_max]);
        }
        if let Some(b) = &p.stock {
            zz.extend([b.z_min, b.z_max]);
            for r in [b.inner_diameter / 2.0, b.outer_diameter / 2.0] {
                ts.extend(s.axis(true, r));
            }
        }
        for z in zz {
            ts.extend(s.axis(false, z));
        }
        ts.extend(s.axis(true, 0.0));
        ts.sort_by(f64::total_cmp);
        ts.dedup_by(|a, b| (*a - *b).abs() < EPS);
        let mut samples: Vec<_> = ts.iter().map(|t| s.at(*t)).collect();
        samples.extend(ts.windows(2).map(|w| s.at((w[0] + w[1]) / 2.0)));
        let path = format!(
            "/project/features/{}",
            p.features
                .iter()
                .position(|f| f.id == s.feature_id)
                .unwrap_or(0)
        );
        let mut issue = |code: &str, msg: &str| {
            if !issues
                .iter()
                .any(|d: &Issue| d.code == code && d.path == path)
            {
                issues.push(Issue {
                    code: code.into(),
                    message: msg.into(),
                    path: path.clone(),
                });
            }
        };
        if samples.iter().any(|q| q.r <= EPS) {
            issue("axis", "轨迹接触或穿过旋转轴");
        }
        if !s.length().is_finite()
            || samples
                .iter()
                .any(|q| !q.z.is_finite() || !q.r.is_finite() || q.z.abs() > 1e9 || q.r.abs() > 1e9)
        {
            issue("numeric", "轨迹超出可计算范围");
        }
        if samples.iter().any(|q| {
            p.forbidden
                .iter()
                .any(|f| q.z >= f.z_min - EPS && q.z <= f.z_max + EPS)
        }) {
            issue("forbidden", "轨迹进入轴向禁入区");
        }
        if s.role != "finish" {
            if samples.iter().any(|q| {
                if q.z <= zmin + EPS || q.z >= zmax - EPS {
                    return false;
                }
                let rr: Vec<_> = profile
                    .iter()
                    .flat_map(|b| b.axis(false, q.z).into_iter().map(|t| b.at(t).r))
                    .collect();
                !rr.is_empty()
                    && if p.mode == "outside" {
                        q.r < rr.into_iter().fold(f64::NEG_INFINITY, f64::max) - EPS
                    } else {
                        q.r > rr.into_iter().fold(f64::INFINITY, f64::min) + EPS
                    }
            }) {
                issue("gouge", "进退刀或间隙段侵入成品材料侧");
            }
            let attach = if s.role == "entry" {
                Some(ordered[0].start)
            } else if s.role == "exit" {
                Some(ordered.last().unwrap().end)
            } else {
                None
            };
            if profile.iter().any(|b| {
                intersections(s, b)
                    .iter()
                    .any(|q| attach.is_none_or(|a| q.sub(a).length() > EPS))
            }) {
                issue("profile_cross", "过渡与轮廓发生非连接接触");
            }
        }
        if let Some(b) = &p.stock {
            if (s.role == "approach" || s.role == "depart")
                && samples.iter().any(|q| {
                    q.z >= b.z_min - EPS
                        && q.z <= b.z_max + EPS
                        && q.r >= b.inner_diameter / 2.0 - EPS
                        && q.r <= b.outer_diameter / 2.0 + EPS
                })
            {
                issue("stock", "接近或离开段接触毛坯");
            }
        }
    }
    for (i, s) in profile.iter_mut().enumerate() {
        s.id = format!("p{}", i + 1);
    }
    Ok((profile, segments, issues))
}
fn leads(cfg: &Lead, enter: bool, profile: &[Segment], p: &Project) -> PluginResult<Vec<Segment>> {
    if !positive(cfg.clearance)
        || !positive(cfg.feed_per_rev)
        || !positive(cfg.clearance_feed_per_rev)
        || !(["line", "arc"].contains(&cfg.r#type.as_str()))
        || if cfg.r#type == "line" {
            !positive(cfg.length)
        } else {
            !positive(cfg.radius) || !positive(cfg.angle) || cfg.angle > 180.0
        }
    {
        return Err(fail("进退刀长度、半径、间隙及进给无效"));
    }
    let edge = if enter {
        &profile[0]
    } else {
        profile.last().unwrap()
    };
    let point = if enter { edge.start } else { edge.end };
    let tangent = edge.tangent(if enter { 0.0 } else { 1.0 });
    let free = if p.mode == "outside" { 1.0 } else { -1.0 };
    let normal = tangent
        .left()
        .scale(free * p.direction as f64 * if p.reverse { -1.0 } else { 1.0 });
    let mut s = if cfg.r#type == "line" {
        let q = point.add(tangent.scale(if enter { -cfg.length } else { cfg.length }));
        Segment::line(
            if enter { q } else { point },
            if enter { point } else { q },
            &edge.feature_id,
            if enter { "entry" } else { "exit" },
            cfg.feed_per_rev,
        )
    } else {
        let c = point.add(normal.scale(cfg.radius));
        let sweep = tangent.cross(normal).signum() * cfg.angle.to_radians();
        let angle = point.sub(c).angle() + if enter { -sweep } else { sweep };
        let q = c.add(Point {
            z: cfg.radius * angle.cos(),
            r: cfg.radius * angle.sin(),
        });
        let mut s = Segment::line(
            if enter { q } else { point },
            if enter { point } else { q },
            &edge.feature_id,
            if enter { "entry" } else { "exit" },
            cfg.feed_per_rev,
        );
        s.kind = "arc".into();
        s.center = Some(c);
        s.sweep = sweep;
        s
    };
    s.feed_per_rev = cfg.feed_per_rev;
    let q = if enter { s.start } else { s.end };
    let clear = q.add(Point {
        z: 0.0,
        r: free * cfg.clearance,
    });
    let gap = Segment::line(
        if enter { clear } else { q },
        if enter { q } else { clear },
        &edge.feature_id,
        if enter { "approach" } else { "depart" },
        cfg.clearance_feed_per_rev,
    );
    Ok(if enter { vec![gap, s] } else { vec![s, gap] })
}
