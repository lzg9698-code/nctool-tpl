//! 命令行参数解析：`--param k=v` 类型推断 + 参数文件加载。

use std::path::Path;

use nctool_core::{ParamValue, ParameterSet};

use crate::output::CliError;

/// 解析单个 `k=v` 参数，并按值推断类型：
/// - `k:s=v` / `k:n=v` / `k:b=v` 强制字符串/数值/布尔（消除歧义的通道）
/// - `true`/`false`（不区分大小写）→ 布尔
/// - 可解析为 f64 → 数值（含整数 `21`、科学计数 `1e3`）；前导零纯数字（如
///   `007`）保持字符串（数值会丢前导零）
/// - 其余 → 字符串（如 `D12`、`轴`）
pub fn parse_kv(s: &str) -> Result<(String, ParamValue), CliError> {
    let (k, v) = s
        .split_once('=')
        .ok_or_else(|| CliError::new("args", format!("参数格式应为 k=v，得到: {s}")))?;
    // 类型后缀：k:s=v / k:n=v / k:b=v（后缀必须是 s/n/b 才解析为强制类型）
    let (raw_key, forced) = match k.trim().rsplit_once(':') {
        Some((name, ty)) if matches!(ty, "s" | "n" | "b") => (name.trim(), Some(ty)),
        _ => (k.trim(), None),
    };
    if raw_key.is_empty() {
        return Err(CliError::new("args", "参数名不能为空"));
    }
    let value = match forced {
        Some("s") => ParamValue::String(v.trim().to_string()),
        Some("n") => match v.trim().parse::<f64>() {
            Ok(n) if n.is_finite() => ParamValue::Number(n),
            _ => {
                return Err(CliError::new(
                    "args",
                    format!("强制数值参数 {raw_key}={} 无法解析为有限数", v.trim()),
                ))
            }
        },
        Some("b") => {
            if v.trim().eq_ignore_ascii_case("true") {
                ParamValue::Bool(true)
            } else if v.trim().eq_ignore_ascii_case("false") {
                ParamValue::Bool(false)
            } else {
                return Err(CliError::new(
                    "args",
                    format!("强制布尔参数 {raw_key} 应为 true/false，得到: {}", v.trim()),
                ));
            }
        }
        _ => infer_param_value(v.trim()),
    };
    Ok((raw_key.to_string(), value))
}

/// 按值文本推断参数类型。
///
/// 注意：`NaN` / `inf` / `Infinity` 等非有限数**不**判为数值（落到字符串），
/// 避免把"NaN"这类文本误判为数值传入渲染上下文；前导零纯数字（`007`/`00`）
/// 同样保持字符串（数值类型会静默丢前导零，如 `T007`→`T7`）。
pub fn infer_param_value(v: &str) -> ParamValue {
    if v.eq_ignore_ascii_case("true") {
        return ParamValue::Bool(true);
    }
    if v.eq_ignore_ascii_case("false") {
        return ParamValue::Bool(false);
    }
    if has_leading_zero(v) {
        return ParamValue::String(v.to_string());
    }
    if let Ok(n) = v.parse::<f64>() {
        if n.is_finite() {
            return ParamValue::Number(n);
        }
    }
    ParamValue::String(v.to_string())
}

/// 前导零纯数字（`007`/`00`）：数值化会丢前导零，保持字符串。
fn has_leading_zero(v: &str) -> bool {
    let bytes = v.as_bytes();
    bytes.len() > 1 && bytes[0] == b'0' && v.chars().all(|c| c.is_ascii_digit())
}

/// 从 JSON 对象构造参数集：`{"x": 21.0, "tool": "D12", "coolant": true}`。
///
/// 数值 → Number，字符串 → String，布尔 → Bool；其他类型报错。
pub fn load_params_file(path: &Path) -> Result<ParameterSet, CliError> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| CliError::new("io", format!("读取参数文件失败 {}: {e}", path.display())))?;
    let value: serde_json::Value = serde_json::from_str(&text).map_err(|e| {
        CliError::new(
            "args",
            format!("参数文件不是合法 JSON {}: {e}", path.display()),
        )
    })?;
    let obj = value.as_object().ok_or_else(|| {
        CliError::new(
            "args",
            format!("参数文件应为 JSON 对象（键值对），得到: {}", path.display()),
        )
    })?;
    let mut set = ParameterSet::new();
    for (k, v) in obj {
        match v {
            serde_json::Value::Number(n) => {
                let f = n
                    .as_f64()
                    .ok_or_else(|| CliError::new("args", format!("参数 {k} 数值无法解析为 f64")))?;
                // 与 --param 推断路径对齐：非有限数拒绝进入参数集
                // （JSON 字面量 1e999 会被解析为 f64::INFINITY）
                if !f.is_finite() {
                    return Err(CliError::new(
                        "args",
                        format!("参数 {k} 为非有限数（NaN/Inf），拒绝生成"),
                    ));
                }
                set.set_number(k.clone(), f);
            }
            serde_json::Value::String(s) => {
                set.set_string(k.clone(), s.clone());
            }
            serde_json::Value::Bool(b) => {
                set.set_bool(k.clone(), *b);
            }
            other => {
                return Err(CliError::new(
                    "args",
                    format!("参数 {k} 类型不支持（仅支持数值/字符串/布尔）: {other}"),
                ));
            }
        }
    }
    Ok(set)
}

/// 合并参数输入：先加载 `--params-file`，再用 `--param` 覆盖（显式参数优先）。
pub fn build_parameter_set(
    params_file: Option<&Path>,
    params: &[String],
) -> Result<ParameterSet, CliError> {
    let mut set = ParameterSet::new();
    if let Some(path) = params_file {
        set.merge(&load_params_file(path)?);
    }
    for kv in params {
        let (k, v) = parse_kv(kv)?;
        set.values.insert(k, v);
    }
    Ok(set)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infer_bool() {
        assert_eq!(infer_param_value("true"), ParamValue::Bool(true));
        assert_eq!(infer_param_value("TRUE"), ParamValue::Bool(true));
        assert_eq!(infer_param_value("false"), ParamValue::Bool(false));
    }

    #[test]
    fn infer_number() {
        assert_eq!(infer_param_value("21.0"), ParamValue::Number(21.0));
        assert_eq!(infer_param_value("21"), ParamValue::Number(21.0));
        assert_eq!(infer_param_value("-10.5"), ParamValue::Number(-10.5));
        assert_eq!(infer_param_value("1e3"), ParamValue::Number(1000.0));
    }

    #[test]
    fn infer_string() {
        assert_eq!(
            infer_param_value("D12"),
            ParamValue::String("D12".to_string())
        );
        assert_eq!(
            infer_param_value("轴"),
            ParamValue::String("轴".to_string())
        );
        // 注意：带字母的坐标串（如 X21）不会被误判为数值
        assert_eq!(
            infer_param_value("X21"),
            ParamValue::String("X21".to_string())
        );
    }

    #[test]
    fn infer_non_finite_is_string() {
        // NaN / inf / Infinity 不判为数值，避免污染渲染上下文
        assert_eq!(
            infer_param_value("NaN"),
            ParamValue::String("NaN".to_string())
        );
        assert_eq!(
            infer_param_value("inf"),
            ParamValue::String("inf".to_string())
        );
        assert_eq!(
            infer_param_value("Infinity"),
            ParamValue::String("Infinity".to_string())
        );
    }

    #[test]
    fn parse_kv_formats() {
        let (k, v) = parse_kv("x=21.0").unwrap();
        assert_eq!(k, "x");
        assert_eq!(v, ParamValue::Number(21.0));
        let (k, v) = parse_kv("tool=D12").unwrap();
        assert_eq!(k, "tool");
        assert_eq!(v, ParamValue::String("D12".to_string()));
    }

    #[test]
    fn parse_kv_missing_equals() {
        assert!(parse_kv("nokey").is_err());
        assert!(parse_kv("=1").is_err());
    }

    #[test]
    fn infer_leading_zero_stays_string() {
        // 前导零纯数字保持字符串（数值会丢前导零：T007 → T7）
        assert_eq!(infer_param_value("007"), ParamValue::String("007".into()));
        assert_eq!(infer_param_value("00"), ParamValue::String("00".into()));
        assert_eq!(infer_param_value("0"), ParamValue::Number(0.0));
        assert_eq!(infer_param_value("0.5"), ParamValue::Number(0.5));
        assert_eq!(infer_param_value("10"), ParamValue::Number(10.0));
    }

    #[test]
    fn parse_kv_type_suffix() {
        let (k, v) = parse_kv("tool:s=D12").unwrap();
        assert_eq!(k, "tool");
        assert_eq!(v, ParamValue::String("D12".into()));
        let (_, v) = parse_kv("n:n=21").unwrap();
        assert_eq!(v, ParamValue::Number(21.0));
        let (_, v) = parse_kv("flag:b=TRUE").unwrap();
        assert_eq!(v, ParamValue::Bool(true));
        // true/false 文本经 :s 可强制为字符串
        let (_, v) = parse_kv("note:s=true").unwrap();
        assert_eq!(v, ParamValue::String("true".into()));
        // 强制类型失败 → 报错
        assert!(parse_kv("x:n=abc").is_err());
        assert!(parse_kv("x:b=yes").is_err());
    }

    #[test]
    fn params_file_non_finite_rejected() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("nctool_test_inf_{}.json", std::process::id()));
        std::fs::write(&path, r#"{"x": 1e999}"#).unwrap();
        let err = load_params_file(&path).unwrap_err();
        std::fs::remove_file(&path).ok();
        assert!(
            err.message.contains("非有限数") || err.message.contains("合法 JSON"),
            "1e999 应被拒绝（非有限数或解析错误）: {}",
            err.message
        );
    }

    #[test]
    fn build_set_from_params() {
        let set =
            build_parameter_set(None, &["x=1.5".to_string(), "tool=D12".to_string()]).unwrap();
        assert_eq!(set.get("x"), Some(&ParamValue::Number(1.5)));
        assert_eq!(
            set.get("tool"),
            Some(&ParamValue::String("D12".to_string()))
        );
    }

    #[test]
    fn load_params_file_json() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("nctool_test_params_{}.json", std::process::id()));
        std::fs::write(&path, r#"{"x": 21.0, "tool": "D12", "coolant": true}"#).unwrap();
        let set = load_params_file(&path).unwrap();
        std::fs::remove_file(&path).ok();
        assert_eq!(set.get("x"), Some(&ParamValue::Number(21.0)));
        assert_eq!(
            set.get("tool"),
            Some(&ParamValue::String("D12".to_string()))
        );
        assert_eq!(set.get("coolant"), Some(&ParamValue::Bool(true)));
    }

    #[test]
    fn params_file_overridden_by_kv() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("nctool_test_override_{}.json", std::process::id()));
        std::fs::write(&path, r#"{"x": 1.0}"#).unwrap();
        let set = build_parameter_set(Some(&path), &["x=99.0".to_string()]).unwrap();
        std::fs::remove_file(&path).ok();
        assert_eq!(set.get("x"), Some(&ParamValue::Number(99.0)));
    }

    #[test]
    fn load_params_file_bad_type() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("nctool_test_bad_{}.json", std::process::id()));
        std::fs::write(&path, r#"{"x": [1,2]}"#).unwrap();
        let err = load_params_file(&path).unwrap_err();
        std::fs::remove_file(&path).ok();
        assert!(err.message.contains("不支持"));
    }
}
