//! Native preparation of the V3 business inputs. No Python runtime or UI state.
use nctool_plugin_sdk::*;

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
fn text<'a>(input: &'a Value, field: &str) -> PluginResult<&'a str> {
    input[field]
        .as_str()
        .ok_or_else(|| error(field, "必须是字符串"))
}
pub fn calculate(input: &Value, standard: &Value) -> PluginResult<Value> {
    let gtype = text(input, "gtype")?.to_uppercase();
    let side = text(input, "side")?;
    if !["ES", "FS"].contains(&gtype.as_str()) || !["Right", "Left"].contains(&side) {
        return Err(error("gtype/side", "无效槽类型或方向"));
    }
    let key = text(input, "std_key")?;
    let std = &standard["tables"][&gtype][key];
    if !std.is_object() {
        return Err(error("std_key", "未知的工艺标准"));
    }
    let raw = positive(input, "raw_diameter")?;
    let safety = positive(input, "safety_dis")?;
    let prefix = if gtype == "ES" { "ES" } else { "FS" };
    let d1 = positive(input, &format!("{prefix}_d1"))?;
    let z = n(input, &format!("{prefix}_z"))?;
    let radius = positive(std, "r")?;
    let t1 = positive(std, "t1")?;
    let f = positive(std, "f")?;
    let angle = positive(std, if gtype == "ES" { "ES_ang" } else { "FS_ang_1" })?;
    if d1 >= raw || d1 <= 2. * t1 || angle >= 90. {
        return Err(error("d1", "槽底直径或槽侧角无效"));
    }
    let offset = f + safety / angle.to_radians().tan();
    let right = side == "Right";
    let mut c = json!({"STD_KEY":key,"groove_type":gtype,"side":side,"X_START":raw+2.*safety,"R":radius,"Z_START":if right{z+offset}else{z-offset},"D1_CUT":d1-2.*t1,"ANG_1":if right{angle-180.}else{-angle},"D1_COMP_START":d1+2.*safety});
    // Supply typed inactive branch fields for the legacy static variable checks.
    for field in [
        "ES_Z",
        "ES_D2",
        "D2_COMP_CUT",
        "Z_MID",
        "FS_Z_PLUS1",
        "FS_Z_MINUS1",
        "ANG_2",
    ] {
        c[field] = json!(0.);
    }
    if gtype == "ES" {
        let d2 = positive(input, "ES_d2")?;
        let chamfer = n(input, "ES_c")?;
        let a = n(input, "ES_c_ang")?;
        if chamfer < 0. || !(0. ..90.).contains(&a) {
            return Err(error("ES_c", "倒角尺寸或角度无效"));
        }
        let cut = d2 - 2. * chamfer * a.to_radians().tan() - 0.5;
        if cut <= 0. {
            return Err(error("ES_d2", "倒角补偿后的切削直径必须大于零"));
        }
        c.as_object_mut().unwrap().extend(
            json!({"ES_Z":z,"ES_D2":d2,"D2_COMP_CUT":cut})
                .as_object()
                .unwrap()
                .clone(),
        );
    } else {
        let a2 = positive(std, "FS_ang_2")?;
        let t2 = positive(std, "t2")?;
        if a2 >= 90. {
            return Err(error("FS_ang_2", "无效标准角度"));
        }
        let offset = t2 - radius + radius / (45. - a2 / 2.).to_radians().tan();
        c.as_object_mut().unwrap().extend(json!({"Z_MID":if right{z-offset}else{z+offset},"FS_Z_PLUS1":z+1.,"FS_Z_MINUS1":z-1.,"ANG_2":if right{90.-a2}else{90.+a2}}).as_object().unwrap().clone());
    }
    Ok(json!({"contexts":[c],"warnings":[]}))
}
