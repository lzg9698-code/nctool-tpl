//! Controller-specific lowering, numeric serialization and rendered-output verification.
use crate::geometry::{Analysis, Project};
use nctool_plugin_sdk::*;
use std::collections::BTreeSet;

pub const TEMPLATE:&str="; FTU IDEAL CONTACT PATH - NO TOOL NOSE COMPENSATION\n; Caller selects tool, offset, spindle, G40 and positions at START.\n; START X{{ start.x }} Z{{ start.z }}\nG18 G90 G710 DIAMON\nG95\nG96 S{{ cutting.vc }} LIMS={{ cutting.rpm }} F{{ blocks[0].f }}\n{% for b in blocks %}\n{{ b.motion }} X{{ b.x }} Z{{ b.z }}{% if b.cr is not none %} CR={{ b.cr }}{% endif %} F{{ b.f }}\n{% endfor %}\nM17\n";
pub fn schema() -> Value {
    let number =
        json!({"type":"string","maxLength":32,"pattern":"^-?(0|[1-9][0-9]*)(\\.[0-9]{1,6})?$"});
    json!({"type":"object","required":["format_version","start","cutting","blocks"],"additionalProperties":false,"properties":{
        "format_version":{"const":1},
        "start":{"type":"object","required":["x","z"],"additionalProperties":false,"properties":{"x":number,"z":number}},
        "cutting":{"type":"object","required":["vc","rpm"],"additionalProperties":false,"properties":{"vc":number,"rpm":number}},
        "blocks":{"type":"array","minItems":1,"maxItems":2048,"items":{"type":"object","required":["id","motion","x","z","cr","f"],"additionalProperties":false,"properties":{"id":{"type":"string"},"motion":{"enum":["G1","G2","G3"]},"x":number,"z":number,"f":number,"cr":{"anyOf":[number,{"type":"null"}]}}}}
    }})
}
pub fn number(n: f64) -> PluginResult<String> {
    if !n.is_finite() || n.abs() > 1e9 {
        return Err(PluginError::new("numeric_range", "NC 数值超出范围"));
    }
    let rounded = (n * 1e6).round() / 1e6;
    if rounded == 0.0 {
        return Ok("0".into());
    }
    Ok(format!("{rounded:.6}")
        .trim_end_matches('0')
        .trim_end_matches('.')
        .into())
}
fn positive(n: f64) -> PluginResult<String> {
    let text = number(n)?;
    if n <= 0.0 || text == "0" {
        Err(PluginError::new(
            "nc_precision",
            "NC 正参数小于输出精度或非正数",
        ))
    } else {
        Ok(text)
    }
}
pub fn lower(p: &Project, a: &Analysis) -> PluginResult<Value> {
    if !a.valid || a.segments.is_empty() {
        return Err(PluginError::new(
            "invalid_geometry",
            "轨迹检查未通过，不能生成",
        ));
    }
    let mut blocks = vec![];
    let mut previous = a.segments[0].start;
    for s in &a.segments {
        if previous.sub(s.start).length() > 1e-6 {
            return Err(PluginError::new("nc_continuity", "轨迹不连续"));
        }
        let x = number(s.end.r * 2.0)?;
        let z = number(s.end.z)?;
        if x == number(s.start.r * 2.0)? && z == number(s.start.z)? {
            return Err(PluginError::new("nc_precision", "运动段在舍入后消失"));
        }
        let mut cr = Value::Null;
        let motion = if let Some(c) = s.center {
            let radius = s.start.sub(c).length();
            let sx = number(s.start.r * 2.0)?.parse::<f64>().unwrap() / 2.0;
            let sz = number(s.start.z)?.parse::<f64>().unwrap();
            let ex = x.parse::<f64>().unwrap() / 2.0;
            let ez = z.parse::<f64>().unwrap();
            let cx = number(c.r * 2.0)?.parse::<f64>().unwrap() / 2.0;
            let cz = number(c.z)?.parse::<f64>().unwrap();
            let quantized_radius = positive(radius)?.parse::<f64>().unwrap();
            if !s.sweep.is_finite()
                || s.sweep.abs() < 1e-8
                || s.sweep.abs() >= std::f64::consts::TAU
                || ((sx - cx).hypot(sz - cz) - quantized_radius).abs() > 1e-6
                || ((ex - cx).hypot(ez - cz) - quantized_radius).abs() > 1e-6
            {
                return Err(PluginError::new("nc_arc", "圆弧在输出精度下不一致"));
            }
            cr = json!(number(if s.sweep.abs() > std::f64::consts::PI {
                -radius
            } else {
                radius
            })?);
            if s.sweep > 0.0 {
                "G3"
            } else {
                "G2"
            }
        } else {
            "G1"
        };
        blocks.push(
            json!({"id":s.id,"motion":motion,"x":x,"z":z,"cr":cr,"f":positive(s.feed_per_rev)?}),
        );
        previous = s.end;
    }
    Ok(
        json!({"format_version":1,"start":{"x":number(a.segments[0].start.r*2.0)?,"z":number(a.segments[0].start.z)?},"cutting":{"vc":positive(p.cutting.surface_speed_m_per_min)?,"rpm":positive(p.cutting.max_spindle_rpm)?},"blocks":blocks}),
    )
}
fn canonical(line: &str) -> PluginResult<Vec<String>> {
    let mut seen = BTreeSet::new();
    let mut words = vec![];
    for (i, word) in line.split_whitespace().enumerate() {
        if i == 0
            && word.starts_with('N')
            && word.len() > 1
            && word[1..].bytes().all(|c| c.is_ascii_digit())
        {
            continue;
        }
        let normalized = if word.starts_with('X')
            || word.starts_with('Z')
            || word.starts_with('S')
            || word.starts_with('F')
            || word.starts_with("CR=")
            || word.starts_with("LIMS=")
        {
            let prefix = if word.starts_with("CR=") {
                3
            } else if word.starts_with("LIMS=") {
                5
            } else {
                1
            };
            let raw = &word[prefix..];
            if raw.is_empty()
                || !raw
                    .bytes()
                    .all(|c| c.is_ascii_digit() || c == b'-' || c == b'.')
            {
                return Err(PluginError::new("nc_output", "不支持的数值格式"));
            }
            let n = raw
                .parse::<f64>()
                .map_err(|_| PluginError::new("nc_output", "NC 数字无效"))?;
            let formatted = number(n)?;
            if formatted.parse::<f64>().unwrap() != n {
                return Err(PluginError::new(
                    "nc_precision",
                    "模板数值超过声明的六位小数精度",
                ));
            }
            format!("{}{}", &word[..prefix], formatted)
        } else {
            word.to_string()
        };
        let key = normalized
            .trim_start_matches('-')
            .split(['=', '.'])
            .next()
            .unwrap_or("")
            .chars()
            .take_while(|c| c.is_ascii_alphabetic())
            .collect::<String>();
        // Several distinct G modal words may share a block; other word addresses cannot repeat.
        let key = if key == "G" || key == "M" {
            normalized.clone()
        } else {
            key
        };
        if !seen.insert(key) {
            return Err(PluginError::new("nc_output", "NC 块包含重复地址"));
        }
        words.push(normalized);
    }
    words.sort();
    Ok(words)
}
pub fn verify(text: &str, model: &Value) -> PluginResult<String> {
    if !text.is_ascii()
        || text
            .bytes()
            .any(|b| b < 32 && !matches!(b, b'\r' | b'\n' | b'\t'))
    {
        return Err(PluginError::new("nc_encoding", "SPF 仅支持 ASCII 文本"));
    }
    let mut expected = vec![
        "G18 G90 G710 DIAMON".into(),
        "G95".into(),
        format!(
            "G96 S{} LIMS={} F{}",
            model["cutting"]["vc"].as_str().unwrap(),
            model["cutting"]["rpm"].as_str().unwrap(),
            model["blocks"][0]["f"].as_str().unwrap()
        ),
    ];
    for b in model["blocks"].as_array().unwrap() {
        expected.push(format!(
            "{} X{} Z{}{} F{}",
            b["motion"].as_str().unwrap(),
            b["x"].as_str().unwrap(),
            b["z"].as_str().unwrap(),
            b["cr"]
                .as_str()
                .map(|s| format!(" CR={s}"))
                .unwrap_or_default(),
            b["f"].as_str().unwrap()
        ));
    }
    expected.push("M17".into());
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let actual: Vec<_> = normalized
        .lines()
        .map(|s| s.split(';').next().unwrap().trim())
        .filter(|s| !s.is_empty())
        .map(canonical)
        .collect::<PluginResult<_>>()?;
    let wanted: Vec<_> = expected
        .iter()
        .map(|s: &String| canonical(s))
        .collect::<PluginResult<_>>()?;
    if actual != wanted {
        return Err(PluginError::new(
            "nc_output_mismatch",
            "模板输出改变了已校验的运动、进给或模态；不交付产物",
        ));
    }
    Ok((normalized.trim_end_matches('\n').to_string() + "\n").replace('\n', "\r\n"))
}
pub fn scene(a: &Analysis) -> Value {
    let layers = [("profile", &a.profile), ("trajectory", &a.segments)];
    let items:Vec<_>=layers.into_iter().flat_map(|(layer,segments)|segments.iter().map(move|s|json!({"id":format!("{layer}-{}",s.id),"entity":s.feature_id,"layer":layer,"kind":s.kind,"start":[s.start.z,s.start.r],"end":[s.end.z,s.end.r],"center":s.center.map(|p|[p.z,p.r]),"sweep":s.sweep,"length":s.length()}))).collect();
    json!({"format_version":1,"axes":{"x":"Z / mm","y":"D / mm","y_scale":2},"items":items,"path":a.segments.iter().map(|s|format!("trajectory-{}",s.id)).collect::<Vec<_>>()})
}
pub fn svg(scene: &Value) -> String {
    let mut points = vec![];
    for s in scene["items"].as_array().unwrap() {
        for key in ["start", "end"] {
            points.push((s[key][0].as_f64().unwrap(), s[key][1].as_f64().unwrap()));
        }
        if let Some(c) = s["center"].as_array() {
            let cx = c[0].as_f64().unwrap();
            let cy = c[1].as_f64().unwrap();
            let radius =
                (s["start"][0].as_f64().unwrap() - cx).hypot(s["start"][1].as_f64().unwrap() - cy);
            points.extend([(cx - radius, cy - radius), (cx + radius, cy + radius)]);
        }
    }
    let xmin = points.iter().map(|p| p.0).fold(f64::INFINITY, f64::min) - 2.0;
    let xmax = points.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max) + 2.0;
    let ymin = points.iter().map(|p| p.1).fold(f64::INFINITY, f64::min) - 2.0;
    let ymax = points.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max) + 2.0;
    let mut out=format!("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"{xmin} {} {} {}\"><title>FTU ideal contact path, no tool compensation</title>",-ymax,xmax-xmin,ymax-ymin);
    for s in scene["items"].as_array().unwrap() {
        let x = s["start"][0].as_f64().unwrap();
        let y = -s["start"][1].as_f64().unwrap();
        let ex = s["end"][0].as_f64().unwrap();
        let ey = -s["end"][1].as_f64().unwrap();
        let d = if s["kind"] == "arc" {
            let radius =
                (x - s["center"][0].as_f64().unwrap()).hypot(y + s["center"][1].as_f64().unwrap());
            let sweep = s["sweep"].as_f64().unwrap();
            format!(
                "M{x},{y} A{radius},{radius} 0 {} {} {ex},{ey}",
                u8::from(sweep.abs() > std::f64::consts::PI),
                u8::from(sweep < 0.0)
            )
        } else {
            format!("M{x},{y} L{ex},{ey}")
        };
        out += &format!(
            "<path d=\"{d}\" fill=\"none\" stroke=\"{}\" stroke-width=\"0.12\"/>",
            if s["layer"] == "profile" {
                "#8993a0"
            } else {
                "#315bea"
            }
        );
    }
    out + "</svg>"
}
pub fn csv(a: &Analysis) -> String {
    let mut out="segment,feature,role,kind,start_z,start_r,end_z,end_r,center_z,center_r,sweep,feed_mm_rev\r\n".to_string();
    for s in &a.segments {
        let escape = |text: &str| {
            let safe = if text.starts_with(['=', '+', '-', '@', '\t', '\r']) {
                format!("'{text}")
            } else {
                text.to_string()
            };
            format!("\"{}\"", safe.replace('"', "\"\""))
        };
        out += &format!(
            "{},{},{},{},{},{},{},{},{},{},{},{}\r\n",
            escape(&s.id),
            escape(&s.feature_id),
            escape(&s.role),
            escape(&s.kind),
            s.start.z,
            s.start.r,
            s.end.z,
            s.end.r,
            s.center.map(|p| p.z.to_string()).unwrap_or_default(),
            s.center.map(|p| p.r.to_string()).unwrap_or_default(),
            s.sweep,
            s.feed_per_rev
        );
    }
    out
}
