//! 命令行参数解析：`--param k=v` 类型推断 + 参数文件加载。

use std::path::Path;

use nctool_core::{ParamValue, ParameterSet};

use crate::output::CliError;

/// 解析单个 `k=v` 参数，并按值推断类型：
/// - `true`/`false`（不区分大小写）→ 布尔
/// - 可解析为 f64 → 数值（含整数 `21`、科学计数 `1e3`）
/// - 其余 → 字符串（如 `D12`、`轴`）
pub fn parse_kv(s: &str) -> Result<(String, ParamValue), CliError> {
    let (k, v) = s
        .split_once('=')
        .ok_or_else(|| CliError::new("args", format!("参数格式应为 k=v，得到: {s}")))?;
    let key = k.trim();
    if key.is_empty() {
        return Err(CliError::new("args", "参数名不能为空"));
    }
    Ok((key.to_string(), infer_param_value(v.trim())))
}

/// 按值文本推断参数类型。
pub fn infer_param_value(v: &str) -> ParamValue {
    if v.eq_ignore_ascii_case("true") {
        return ParamValue::Bool(true);
    }
    if v.eq_ignore_ascii_case("false") {
        return ParamValue::Bool(false);
    }
    if let Ok(n) = v.parse::<f64>() {
        return ParamValue::Number(n);
    }
    ParamValue::String(v.to_string())
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
