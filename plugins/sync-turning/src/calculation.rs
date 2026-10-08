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
fn integer(v: f64, field: &str) -> PluginResult<u64> {
    if v > 0. && v.fract() == 0. && v <= 9_007_199_254_740_992. {
        Ok(v as u64)
    } else {
        Err(error(field, "必须是可精确表示的正整数"))
    }
}
pub fn calculate(side: &str, input: &Value) -> PluginResult<Value> {
    let start = positive(input, "sta_x")?;
    let end = positive(input, "end_x")?;
    let sta_z = n(input, "sta_z")?;
    let end_z = n(input, "end_z")?;
    let ap1 = positive(input, "ap_1")?;
    let ap2 = positive(input, "ap_2")?;
    let feed = positive(input, "sync_feed")?;
    let sync_num = integer(positive(input, "current_sync_num")?, "current_sync_num")?;
    nctool_plugin_nc::operation::decimal_ticks(&input["sync_feed"], "sync_feed", 2)
        .map_err(|_| error("sync_feed", "同步进给必须是 0.01 的正整数倍"))?;
    if start < end {
        return Err(error("end_x", "终止直径不能大于起始直径"));
    }
    if start == end {
        return Ok(
            json!({"contexts":[],"next_sync_num":sync_num,"skip_text":if side=="left"{";No cut"}else{";(No cutting required)"}}),
        );
    }
    let ratio = (start - end) / (ap1 + ap2);
    if !ratio.is_finite() || ratio > 10_000. {
        return Err(error("cut_count", "切削刀次不能超过 10000"));
    }
    let near = ratio.round_ties_even();
    let count = if near > 0. && (ratio - near).abs() <= 1e-8 {
        near
    } else {
        ratio.ceil()
    };
    let actual = end + count * (ap1 + ap2);
    let actual_z = sta_z + ((actual - start) / 2.) / 25_f64.to_radians().tan() - 2.;
    let mut warnings = vec![];
    if side == "right" {
        for (field, value, max) in [
            ("ap_1", ap1, 5.),
            ("ap_2", ap2, 8.),
            ("sync_feed", feed, 0.5),
        ] {
            if value > max {
                let label = if field == "sync_feed" {
                    "同步进给 sync_feed"
                } else {
                    field
                };
                warnings.push(format!(";警告: {label}={} 超出上限{max}", input[field]));
            }
        }
    }
    let base = json!({"side":if side=="left"{"Left"}else{"Right"},"end_x":end,"sta_x":start,"sta_z":sta_z,"actual_sta_x":actual,"actual_sta_z":actual_z,"end_z":end_z,"sync_feed":feed,"ap_1":ap1,"ap_2":ap2,"cut_num":count as u64,"sync_num_start":sync_num,"warnings":warnings});
    let mut channels = vec![];
    for channel in [1, 2] {
        let mut c = base.clone();
        c["channel"] = json!(channel);
        channels.push(c);
    }
    let next = sync_num
        .checked_add(count as u64 + 1)
        .filter(|v| *v <= 9_007_199_254_740_992)
        .ok_or_else(|| error("current_sync_num", "同步编号溢出"))?;
    Ok(json!({"contexts":channels,"next_sync_num":next,"warnings":warnings}))
}
