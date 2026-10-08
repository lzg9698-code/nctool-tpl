//! Native preparation of the V3 business inputs. No Python runtime or UI state.
use nctool_plugin_sdk::*;
use std::f64::consts::PI;

fn error(field: &str, message: &str) -> PluginError {
    PluginError {
        code: "operation_validation".into(),
        message: format!("{field}: {message}"),
        diagnostics: vec![Diagnostic {
            level: "error".into(),
            code: "operation_validation".into(),
            message: message.into(),
            path: Some(field.into()),
        }],
    }
}
pub fn number(v: &Value, field: &str) -> PluginResult<f64> {
    if let Some(i) = v.as_i64() {
        if i.unsigned_abs() > 1 << 53 {
            return Err(error(field, "整数无法精确参与计算"));
        }
    }
    if let Some(i) = v.as_u64() {
        if i > 1 << 53 {
            return Err(error(field, "整数无法精确参与计算"));
        }
    }
    v.as_f64()
        .filter(|v| v.is_finite())
        .ok_or_else(|| error(field, "必须是有限数值"))
}
fn n(input: &Value, field: &str) -> PluginResult<f64> {
    number(&input[field], field)
}
fn positive(input: &Value, field: &str) -> PluginResult<f64> {
    let v = n(input, field)?;
    if v <= 0. {
        Err(error(field, "必须大于零"))
    } else {
        Ok(v)
    }
}
fn integer(v: f64, field: &str) -> PluginResult<u64> {
    if v > 0. && v.fract() == 0. && v <= 9_007_199_254_740_992. {
        Ok(v as u64)
    } else {
        Err(error(field, "必须是可精确表示的正整数"))
    }
}
fn text<'a>(input: &'a Value, field: &str) -> PluginResult<&'a str> {
    input[field]
        .as_str()
        .ok_or_else(|| error(field, "必须是字符串"))
}
fn id(input: &Value, field: &str, offset: bool) -> PluginResult<String> {
    let value = text(input, field)?;
    let valid = if offset {
        value.starts_with('D') && value[1..].parse::<u64>().is_ok_and(|i| i > 0)
    } else {
        !value.is_empty()
            && value.len() <= 64
            && value.as_bytes()[0].is_ascii_alphanumeric()
            && value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
    };
    if !valid {
        return Err(error(field, "无效的 NC 标识"));
    }
    Ok(value.into())
}
fn rounded(v: f64, field: &str) -> PluginResult<f64> {
    let v = v.round_ties_even();
    integer(v, field)?;
    Ok(v)
}
fn tool(tool: &Value, cutting: Option<&Value>) -> PluginResult<(f64, f64)> {
    id(tool, "tool_num", false)?;
    id(tool, "tool_offset", true)?;
    let diameter = positive(tool, "diameter")?;
    if let Some(cutting) = cutting {
        if cutting["operation"] != "milling" || cutting["feed_mode"] != "per_minute" {
            return Err(error("tool", "键槽加工预置必须使用铣削和每分钟进给"));
        }
        return Ok((
            rounded(positive(&cutting["values"], "rpm")?, "rpm")?,
            rounded(positive(&cutting["values"], "feed")?, "feed")?,
        ));
    }
    let teeth = positive(tool, "teeth")?;
    integer(teeth, "teeth")?;
    let rpm = rounded(
        positive(tool, "cutting_speed")? * 1000. / (PI * diameter),
        "spindle_speed",
    )?;
    let feed = rounded(rpm * teeth * positive(tool, "feed_per_tooth")?, "feed_rate")?;
    Ok((rpm, feed))
}
/// Exact decimal conversion to 0.001 mm ticks, including scientific notation.
fn ticks(value: &Value, field: &str) -> PluginResult<i64> {
    decimal_ticks(value, field, 3)
}
pub fn decimal_ticks(value: &Value, field: &str, scale: i32) -> PluginResult<i64> {
    let raw = value.to_string();
    let (mantissa, exp) = if let Some((m, e)) = raw.split_once(['e', 'E']) {
        (
            m,
            e.parse::<i32>().map_err(|_| error(field, "无效十进制值"))?,
        )
    } else {
        (raw.as_str(), 0)
    };
    let decimals = mantissa
        .split_once('.')
        .map(|(_, f)| f.len() as i32)
        .unwrap_or(0);
    let digits = mantissa.replace('.', "");
    let mut integer = digits
        .parse::<i64>()
        .map_err(|_| error(field, "切深超出精确计算范围"))?;
    let power = exp + scale - decimals;
    if power.abs() > 18 {
        return Err(error(field, "切深必须是 0.001 mm 的正整数倍"));
    }
    let factor = 10_i64.pow(power.unsigned_abs());
    if power < 0 {
        if integer % factor != 0 {
            return Err(error(field, "切深必须是 0.001 mm 的正整数倍"));
        }
        integer /= factor;
    } else {
        integer = integer
            .checked_mul(factor)
            .ok_or_else(|| error(field, "切深溢出"))?;
    }
    if integer <= 0 {
        return Err(error(field, "切深必须大于零"));
    }
    Ok(integer)
}
pub fn depth_layers(total: &Value, step: &Value) -> PluginResult<(Vec<f64>, Vec<f64>)> {
    let total = ticks(total, "depth_c")?;
    let step = ticks(step, "max_depth_per_pass")?;
    let count = total / step + i64::from(total % step != 0);
    if count > 10_000 {
        return Err(error("max_depth_per_pass", "分层数量不能超过 10000"));
    }
    let mut depths = vec![];
    let mut targets = vec![];
    let mut previous = 0_i64;
    for _ in 0..count {
        let target = previous.checked_add(step).unwrap_or(total).min(total);
        depths.push((target - previous) as f64 / 1000.);
        targets.push(target as f64 / 1000.);
        previous = target;
    }
    Ok((depths, targets))
}
fn keyway(stage: &str, input: &Value, cutting: &Value) -> PluginResult<Value> {
    let geo = &input["geometry"];
    let proc = &input["process"];
    let rough = &input["rough_tool"];
    let finish = &input["finish_tool"];
    let cham = &input["chamfer"];
    let a = positive(geo, "length_a")?;
    let b = positive(geo, "width_b")?;
    let c = positive(geo, "depth_c")?;
    let d = positive(geo, "shaft_d")?;
    let z = n(geo, "position_z")?;
    let q = positive(geo, "corner_radius_q")?;
    let form = text(geo, "form")?;
    if !["闭口", "左开口", "右开口"].contains(&form) {
        return Err(error("form", "未知的键槽形式"));
    }
    let param_id = text(geo, "param_id")?;
    if !(param_id.starts_with('[')
        && param_id.ends_with(']')
        && param_id.len() > 2
        && param_id[1..param_id.len() - 1]
            .bytes()
            .all(|b| b.is_ascii_digit()))
    {
        return Err(error("param_id", "必须为 [数字]"));
    }
    if d <= b || c >= d / 2. || q > b / 2. {
        return Err(error("geometry", "轴径、槽宽、槽深或圆角关系无效"));
    }
    let angle = positive(proc, "ramp_angle")?;
    let factor = positive(proc, "corner_feed_factor")?;
    let margin = n(proc, "rough_margin")?;
    let semi_margin = n(proc, "semi_finish_margin")?;
    let semi = n(proc, "semi_finish_feed")?;
    if angle >= 90.
        || factor > 1.
        || semi < 0.
        || !(0. <= semi_margin && semi_margin <= margin && margin < b / 2.)
    {
        return Err(error("process", "角度、降速或加工余量无效"));
    }
    let (rough_n, rough_vf) = tool(rough, cutting["slots"].get("rough"))?;
    let (finish_n, finish_vf) = tool(finish, cutting["slots"].get("finish"))?;
    let rough_d = positive(rough, "diameter")?;
    let finish_d = positive(finish, "diameter")?;
    if rough_d > b - 2. * margin || finish_d > b || finish_d / 2. > q {
        return Err(error("tool.diameter", "刀具无法实现槽宽或圆角"));
    }
    let (depths, targets) = depth_layers(&geo["depth_c"], &proc["max_depth_per_pass"])?;
    let tan = angle.to_radians().tan();
    if form == "闭口" {
        let ramp_end = q - margin - rough_d;
        let entry = ramp_end - c / tan;
        let minimum = -(a + q) + rough_d / 2. + margin;
        let maximum = q - rough_d / 2. - margin;
        if entry < minimum || ramp_end > maximum {
            return Err(error("ramp_angle", "闭口槽斜向入刀超出允许范围"));
        }
    }
    let enabled = input["enable_chamfer"]
        .as_bool()
        .ok_or_else(|| error("enable_chamfer", "必须是布尔值"))?;
    if enabled {
        id(cham, "tool_num", false)?;
        id(cham, "tool_offset", true)?;
        integer(positive(cham, "spindle_speed")?, "chamfer.spindle_speed")?;
        integer(positive(cham, "feed")?, "chamfer.feed")?;
        positive(cham, "depth")?;
        let cd = positive(cham, "tool_diameter")?;
        let plunge = positive(cham, "plunge_depth")?;
        positive(cham, "protrusion")?;
        let edge = n(cham, "chisel_edge_width")?;
        if plunge >= q || edge < 0. || edge >= cd {
            return Err(error("chamfer", "倒角下降深度或横刃宽度无效"));
        }
    } else if stage == "chamfer" {
        return Err(error("enable_chamfer", "倒角未启用"));
    }
    let is_round = (q - b / 2.).abs() < 0.001 && q > 0.;
    let path = if form == "右开口" {
        "right_open"
    } else if form == "左开口" {
        "left_open"
    } else if stage == "rough" {
        if is_round {
            "round_2pass"
        } else if rough_d / 2. >= q - margin - 0.001 {
            "closed_2pass"
        } else {
            "closed_3pass"
        }
    } else if is_round {
        "closed_round"
    } else {
        "closed_non_round"
    };
    let mut context = json!({"U_A":a,"U_B":b,"U_C":c,"U_D":d,"U_Z":z,"U_Q":q,"U_FX":form,"U_ID":param_id,"is_left_open":form=="左开口","path_type":path});
    let selected = if stage == "rough" {
        rough
    } else if stage == "finish" {
        finish
    } else {
        cham
    };
    context["tool_num"] = selected["tool_num"].clone();
    context["tool_offset"] = selected["tool_offset"].clone();
    context["next_tool_num"] = if stage == "rough" {
        finish["tool_num"].clone()
    } else if stage == "finish" && enabled {
        cham["tool_num"].clone()
    } else {
        json!("")
    };
    if stage == "rough" {
        let semi_vf = rounded(if semi > 0. { semi } else { finish_vf }, "semi_feed")?;
        rounded(semi_vf / 2., "semi_feed")?;
        let extra = json!({"U_RT":rough["tool_num"],"U_RTB":rough["tool_offset"],"U_RTD":rough_d,"U_RC":proc["max_depth_per_pass"],"U_ANG":angle,"U_ALBR":margin,"U_ALBHF":semi_margin,"rough_n":rough_n,"rough_vf":rough_vf,"rough_vf_corner":rounded(rough_vf*factor,"rough_vf_corner")?,"semi_vf":semi_vf,"layer_count":depths.len(),"depths":depths,"target_depths":targets,"spindle_speed":rough_n});
        context
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
    } else if stage == "finish" {
        context.as_object_mut().unwrap().extend(json!({"U_FT":finish["tool_num"],"U_FTB":finish["tool_offset"],"U_FTD":finish_d,"finish_n":finish_n,"finish_vf":finish_vf,"spindle_speed":finish_n}).as_object().unwrap().clone());
    } else {
        context.as_object_mut().unwrap().extend(json!({"U_CT":cham["tool_num"],"U_CTB":cham["tool_offset"],"U_CHAMFER_DEPTH":cham["depth"],"R1":cham["spindle_speed"],"R2":cham["feed"],"R3":cham["tool_diameter"],"R4":cham["plunge_depth"],"R5":cham["chisel_edge_width"],"R6":cham["protrusion"],"spindle_speed":cham["spindle_speed"]}).as_object().unwrap().clone());
    }
    let warnings = if stage == "rough" && depths_len(&context) > 5 {
        vec![format!(
            "键槽将分为 {} 层加工，请确认切深设置",
            depths_len(&context)
        )]
    } else {
        vec![]
    };
    Ok(json!({"contexts":[context],"warnings":warnings}))
}
fn depths_len(v: &Value) -> u64 {
    v["layer_count"].as_u64().unwrap_or(0)
}
/// Fixed arithmetic grammar for legacy numeric entry. No variables or calls.
struct Arithmetic<'a> {
    bytes: &'a [u8],
    pos: usize,
    nodes: usize,
}
impl Arithmetic<'_> {
    fn literal(&mut self) -> PluginResult<f64> {
        let start = self.pos;
        let base = if self.bytes.get(start) == Some(&b'0') {
            match self.bytes.get(start + 1) {
                Some(b'x' | b'X') => Some(16),
                Some(b'o' | b'O') => Some(8),
                Some(b'b' | b'B') => Some(2),
                _ => None,
            }
        } else {
            None
        };
        if let Some(base) = base {
            self.pos += 2;
            let digits = self.pos;
            while self
                .bytes
                .get(self.pos)
                .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
            {
                self.pos += 1;
            }
            let token = &self.bytes[digits..self.pos];
            for (i, byte) in token.iter().enumerate() {
                if *byte == b'_'
                    && ((i > 0 && char::from(token[i - 1]).to_digit(base).is_none())
                        || token
                            .get(i + 1)
                            .is_none_or(|b| char::from(*b).to_digit(base).is_none()))
                {
                    return Err(error("expression", "无效数值分隔符"));
                }
            }
            let clean = std::str::from_utf8(token).unwrap_or("").replace('_', "");
            return u64::from_str_radix(&clean, base)
                .map(|v| v as f64)
                .map_err(|_| error("expression", "无效或超范围的整数"));
        }
        while self
            .bytes
            .get(self.pos)
            .is_some_and(|b| b.is_ascii_digit() || matches!(*b, b'.' | b'_'))
        {
            self.pos += 1;
        }
        if self
            .bytes
            .get(self.pos)
            .is_some_and(|b| matches!(*b, b'e' | b'E'))
        {
            self.pos += 1;
            if self
                .bytes
                .get(self.pos)
                .is_some_and(|b| matches!(*b, b'+' | b'-'))
            {
                self.pos += 1;
            }
            while self
                .bytes
                .get(self.pos)
                .is_some_and(|b| b.is_ascii_digit() || *b == b'_')
            {
                self.pos += 1;
            }
        }
        let token = &self.bytes[start..self.pos];
        for (i, byte) in token.iter().enumerate() {
            if *byte == b'_'
                && (i == 0
                    || !token[i - 1].is_ascii_digit()
                    || token.get(i + 1).is_none_or(|b| !b.is_ascii_digit()))
            {
                return Err(error("expression", "无效数值分隔符"));
            }
        }
        let clean = std::str::from_utf8(token).unwrap_or("").replace('_', "");
        if !clean.contains(['.', 'e', 'E'])
            && clean.starts_with('0')
            && clean.bytes().any(|b| b != b'0')
        {
            return Err(error("expression", "十进制整数不能包含多余前导零"));
        }
        clean
            .parse()
            .map_err(|_| error("expression", "仅允许数值和算术表达式"))
    }
    fn whitespace(&mut self) {
        while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }
    fn expr(&mut self, min: u8) -> PluginResult<f64> {
        if self.nodes > 64 {
            return Err(error("expression", "表达式过于复杂"));
        }
        self.whitespace();
        let mut left = match self.bytes.get(self.pos) {
            Some(b'+') | Some(b'-') => {
                self.nodes += 2;
                let sign = self.bytes[self.pos];
                self.pos += 1;
                let v = self.expr(3)?;
                if sign == b'-' {
                    -v
                } else {
                    v
                }
            }
            Some(b'(') => {
                self.pos += 1;
                let v = self.expr(0)?;
                self.whitespace();
                if self.bytes.get(self.pos) != Some(&b')') {
                    return Err(error("expression", "括号不匹配"));
                }
                self.pos += 1;
                v
            }
            _ => {
                self.nodes += 1;
                self.literal()?
            }
        };
        if self.nodes > 64 {
            return Err(error("expression", "表达式过于复杂"));
        }
        check_arithmetic(left)?;
        loop {
            self.whitespace();
            let Some(op) = self.bytes.get(self.pos).copied() else {
                break;
            };
            let pow = op == b'*' && self.bytes.get(self.pos + 1) == Some(&b'*');
            let precedence = if pow {
                4
            } else if op == b'*' || op == b'/' {
                2
            } else if op == b'+' || op == b'-' {
                1
            } else {
                break;
            };
            if precedence < min {
                break;
            }
            self.pos += if pow { 2 } else { 1 };
            let right = self.expr(if pow { precedence } else { precedence + 1 })?;
            self.nodes += 2;
            if self.nodes > 64 {
                return Err(error("expression", "表达式过于复杂"));
            }
            left = if pow {
                if right.abs() > 8. {
                    return Err(error("expression", "幂指数绝对值不能超过 8"));
                }
                left.powf(right)
            } else {
                match op {
                    b'+' => left + right,
                    b'-' => left - right,
                    b'*' => left * right,
                    b'/' => left / right,
                    _ => unreachable!(),
                }
            };
            check_arithmetic(left)?;
        }
        Ok(left)
    }
}
fn check_arithmetic(value: f64) -> PluginResult<()> {
    if !value.is_finite() || value.abs() > 1e9 {
        Err(error("expression", "算术值非有限、除以零或幅值过大"))
    } else {
        Ok(())
    }
}
// Decimal formatting rounds the actual binary float, as Python round(x, 3) does.
// Multiplication by 1000 followed by rounding can move a tie to the wrong side.
fn python_round3(value: f64) -> f64 {
    format!("{value:.3}")
        .parse()
        .expect("finite formatted float")
}
pub fn arithmetic(raw: &str) -> PluginResult<f64> {
    let raw = raw.trim();
    if raw.is_empty() || raw.len() > 256 {
        return Err(error("expression", "表达式不能为空或超过 256 字符"));
    }
    let mut parser = Arithmetic {
        bytes: raw.as_bytes(),
        pos: 0,
        nodes: 1,
    };
    let value = parser.expr(0)?;
    parser.whitespace();
    if parser.pos != parser.bytes.len() {
        return Err(error("expression", "不支持的算术表达式"));
    }
    Ok(value)
}
fn circlip(input: &Value) -> PluginResult<Value> {
    let mut positions = vec![];
    if let Some(array) = input["z_positions"].as_array() {
        for v in array {
            positions.push(number(v, "z_positions")?);
        }
    } else if let Some(raw) = input["z_positions"].as_str() {
        if raw.len() > 2_560_000 {
            return Err(error("z_positions", "槽位置文本过长"));
        }
        for (index, line) in raw.replace(',', "\n").lines().enumerate() {
            if !line.trim().is_empty() {
                positions.push(arithmetic(line).map_err(|mut e| {
                    e.message = format!("z_positions[{}]: {}", index + 1, e.message);
                    e
                })?);
            }
        }
    }
    if positions.is_empty() || positions.len() > 10_000 {
        return Err(error("z_positions", "槽位置数量必须处于 1..10000"));
    }
    let mut context =
        json!({"z_offsets":positions.iter().map(|v|python_round3(*v)).collect::<Vec<_>>()});
    for field in [
        "X_RAW_D",
        "X_TIP_D",
        "X_DOWN_D",
        "WIDTH_CIRCLIP",
        "MAX_CUTDIPTH",
        "WIDTH_CUTER",
        "FEEDRATE",
    ] {
        let value = if let Some(raw) = input[field].as_str() {
            arithmetic(raw)?
        } else {
            number(&input[field], field)?
        };
        let value = python_round3(value);
        if value <= 0. {
            return Err(error(field, "必须大于零"));
        }
        context[field] = json!(value);
    }
    if !(n(&context, "X_RAW_D")? >= n(&context, "X_TIP_D")?
        && n(&context, "X_TIP_D")? > n(&context, "X_DOWN_D")?)
        || n(&context, "WIDTH_CUTER")? > n(&context, "WIDTH_CIRCLIP")?
    {
        return Err(error("geometry", "直径关系或刀宽无效"));
    }
    Ok(json!({"contexts":[context],"warnings":[]}))
}
pub fn prepare(operation: &Value, input: &Value, standard: &Value) -> PluginResult<Value> {
    prepare_with_cutting(operation, input, standard, &json!({}))
}
pub fn prepare_with_cutting(
    operation: &Value,
    input: &Value,
    _standard: &Value,
    cutting: &Value,
) -> PluginResult<Value> {
    if operation["version"] != 1 {
        return Err(error("operation.version", "不支持的工艺算法版本"));
    }
    let kind = text(operation, "id")?;
    if kind != "direct" {
        validate_schema(&input_schema(kind), input, "operation_validation")?;
    }
    let mut prepared = match kind {
        "keyway.rough" => keyway("rough", input, cutting)?,
        "keyway.finish" => keyway("finish", input, cutting)?,
        "keyway.chamfer" => keyway("chamfer", input, cutting)?,
        "sync.left" | "sync.right" | "groove" => {
            return Err(error("operation.id", "请通过专用插件服务准备此工艺"))
        }
        "circlip" => circlip(input)?,
        "direct" => json!({"contexts":[input],"warnings":[]}),
        _ => return Err(error("operation.id", "未知工艺处理器")),
    };
    for context in prepared["contexts"].as_array().unwrap() {
        let _: crate::model::ParameterSet = serde_json::from_value(context.clone())?;
    }
    prepared["derived_fields"] = json!(prepared["contexts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|context| context
            .as_object()
            .unwrap()
            .iter()
            .filter(|(key, value)| input.get(*key) != Some(*value))
            .map(|(key, _)| format!("/{key}"))
            .collect::<Vec<_>>())
        .collect::<Vec<_>>());
    let channel_count = if kind.starts_with("sync.") { 2 } else { 1 };
    prepared["output_plan"] =
        json!({"channels":(1..=channel_count).collect::<Vec<_>>(),"all_or_nothing":true});
    prepared["operation"] = operation.clone();
    prepared["inputs"] = input.clone();
    prepared["algorithm_version"] = json!(1);
    Ok(prepared)
}
pub fn input_schema(kind: &str) -> Value {
    let numeric = |fields: &[&str]| {
        let properties = fields
            .iter()
            .map(|f| (f.to_string(), json!({"type":"number","title":f})))
            .collect::<serde_json::Map<_, _>>();
        json!({"type":"object","properties":properties,"required":fields,"additionalProperties":false})
    };
    if kind.starts_with("keyway.") {
        let mut geometry = numeric(&[
            "length_a",
            "width_b",
            "depth_c",
            "shaft_d",
            "position_z",
            "corner_radius_q",
        ]);
        geometry["properties"]["form"] = json!({"type":"string","enum":["闭口","左开口","右开口"]});
        geometry["properties"]["param_id"] = json!({"type":"string","pattern":"^\\[[0-9]+\\]$"});
        geometry["required"]
            .as_array_mut()
            .unwrap()
            .extend([json!("form"), json!("param_id")]);
        let mut tool = numeric(&["diameter", "teeth", "cutting_speed", "feed_per_tooth"]);
        tool["required"] = json!(["diameter"]);
        tool["properties"]["teeth"]["type"] = json!("integer");
        for f in ["tool_num", "tool_offset"] {
            tool["properties"][f] = json!({"type":"string"});
            tool["required"].as_array_mut().unwrap().push(json!(f));
        }
        let mut chamfer = numeric(&[
            "depth",
            "spindle_speed",
            "feed",
            "tool_diameter",
            "plunge_depth",
            "chisel_edge_width",
            "protrusion",
        ]);
        for f in ["tool_num", "tool_offset"] {
            chamfer["properties"][f] = json!({"type":"string"});
            chamfer["required"].as_array_mut().unwrap().push(json!(f));
        }
        return json!({"type":"object","properties":{"geometry":geometry,"rough_tool":tool,"finish_tool":tool,"process":numeric(&["max_depth_per_pass","ramp_angle","rough_margin","semi_finish_margin","semi_finish_feed","corner_feed_factor"]),"chamfer":chamfer,"enable_chamfer":{"type":"boolean"}},"required":["geometry","rough_tool","finish_tool","process","enable_chamfer"],"additionalProperties":false});
    }
    if kind.starts_with("sync.") {
        let mut s = numeric(&[
            "sta_x",
            "sta_z",
            "end_x",
            "end_z",
            "ap_1",
            "ap_2",
            "sync_feed",
            "current_sync_num",
        ]);
        s["properties"]["current_sync_num"]["type"] = json!("integer");
        return s;
    }
    if kind == "groove" {
        let mut s = numeric(&["raw_diameter", "safety_dis"]);
        for f in [
            "ES_d1", "ES_z", "ES_d2", "ES_c", "ES_c_ang", "FS_d1", "FS_z",
        ] {
            s["properties"][f] = json!({"type":"number"});
        }
        for (f, field) in [
            ("gtype", json!({"type":"string","enum":["ES","FS"]})),
            ("side", json!({"type":"string","enum":["Left","Right"]})),
            ("std_key", json!({"type":"string"})),
        ] {
            s["properties"][f] = field;
            s["required"].as_array_mut().unwrap().push(json!(f));
        }
        return s;
    }
    if kind == "circlip" {
        let mut s = numeric(&[
            "X_RAW_D",
            "X_TIP_D",
            "X_DOWN_D",
            "WIDTH_CIRCLIP",
            "MAX_CUTDIPTH",
            "WIDTH_CUTER",
            "FEEDRATE",
        ]);
        for field in s["properties"].as_object_mut().unwrap().values_mut() {
            field["type"] = json!(["number", "string"]);
        }
        s["properties"]["z_positions"] =
            json!({"type":["string","array"],"items":{"type":"number"},"format":"multiline"});
        s["required"]
            .as_array_mut()
            .unwrap()
            .push(json!("z_positions"));
        return s;
    }
    json!({"type":"object"})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn precise_layers() {
        assert_eq!(
            depth_layers(&json!(13.1), &json!(5)).unwrap(),
            (vec![5., 5., 3.1], vec![5., 10., 13.1])
        );
        assert!(depth_layers(&json!(0.0009), &json!(1)).is_err());
        assert!(depth_layers(&json!(20), &json!(0.001)).is_err());
    }
    #[test]
    fn numeric_grammar() {
        assert_eq!(arithmetic("-2**2 + 10 / 2").unwrap(), 1.);
        assert_eq!(arithmetic("0x10 + 0o10 + 0b10 + 1_0.2_5").unwrap(), 36.25);
        assert!(arithmetic(&["1"; 21].join("+")).is_ok());
        assert!(arithmetic(&["1"; 22].join("+")).is_err());
        for raw in [
            "True",
            "f(1)",
            "__import__('os')",
            "1/0",
            "2**9",
            "1e999",
            "01",
            "1__0",
            "0x__1",
            "1_",
            "1%2",
            "1//2",
            "(-1)**0.5",
        ] {
            assert!(arithmetic(raw).is_err(), "{raw}");
        }
    }
}
