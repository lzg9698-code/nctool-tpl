//! 命令行参数解析：`--param k=v` 类型推断 + 参数文件加载。

use std::path::Path;

use nctool_core::{ParamKind, ParamSpec, ParamValue, ParameterSet};

use crate::output::CliError;

/// 解析单个 `k=v` 参数：按值推断类型，并**按规格归一取值**（见 [`coerce_param_value`]）。
///
/// - `k:s=v` / `k:n=v` / `k:b=v` 强制字符串/数值/布尔（消除歧义的通道）
/// - `true`/`false`（不区分大小写）→ 布尔
/// - 可解析为 f64 → 数值（含整数 `21`、科学计数 `1e3`）；前导零纯数字（如
///   `007`）保持字符串（数值会丢前导零）
/// - 其余 → 字符串（如 `D12`、`轴`）
///
/// `specs` 传空切片即退化为纯启发式推断（无规格信息时的行为）。
/// 显式类型后缀（`k:s=` / `k:n=` / `k:b=`）**优先于**规格归一——
/// 那是用户明确表达意图的通道。
pub fn parse_kv_with_specs(s: &str, specs: &[ParamSpec]) -> Result<(String, ParamValue), CliError> {
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
        // 无显式后缀 → 按规格归一（无规格信息时退化为启发式推断）
        _ => coerce_param_value(v.trim(), specs.iter().find(|sp| sp.name == raw_key)),
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

/// 按规格归一 `--param` 的取值。
///
/// argv 里没有类型信息，此前只按"像不像数字"推断（`5010` → 数值）。但字符串型
/// 参数若值恰好形如数字（`U_CTB` 的 `1631`、`U_ID` 的 `[42]`）会被推断成数值 →
/// 类型不匹配，用户只能改用 `--params-file` 传 JSON 字符串或加 `k:s=` 后缀。
///
/// 规则（**先白名单、后类型**）：
/// 1. 规格声明了候选值 → **优先取能命中白名单的那种解释**：`U_CTB` 的候选项混有
///    `1631` 与 `"DECKEL"`，两种解释各命中一半，按值语义比较即可；
/// 2. 候选值都没命中 → 保持启发式结果，交给校验层报"不在候选项内"
///    （错误信息里的值更贴近用户输入）；
/// 3. 规格没声明候选值 → 按声明的类型：`String` 保持字符串，其余沿用启发式推断；
/// 4. 无规格（文件路径模板 / 未注册模板）→ 沿用启发式推断。
fn coerce_param_value(raw: &str, spec: Option<&ParamSpec>) -> ParamValue {
    let heuristic = infer_param_value(raw);
    let Some(spec) = spec else {
        return heuristic;
    };
    if let Some(accepted) = spec.accepts_option(&heuristic) {
        if accepted {
            return heuristic;
        }
        // 启发式解释不在白名单里，试"文本"解释（`1631` → `"1631"`）
        let as_text = ParamValue::String(raw.to_string());
        if spec.accepts_option(&as_text) == Some(true) {
            return as_text;
        }
        return heuristic;
    }
    match spec.kind {
        ParamKind::String => ParamValue::String(raw.to_string()),
        _ => heuristic,
    }
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
    let text = crate::limits::read_text_limited(path, "io", "参数文件")?;
    let value: serde_json::Value = serde_json::from_str(&text).map_err(|e| {
        CliError::new(
            "args",
            format!("参数文件不是合法 JSON {}: {e}", path.display()),
        )
    })?;
    parameter_set_from_json(&value).map_err(|e| {
        if e.kind == "args" {
            CliError::new("args", format!("{}: {}", e.message, path.display()))
        } else {
            e
        }
    })
}

/// 从 JSON 对象构造参数集，供 CLI 参数文件和 Web API 共用。
///
/// 支持的类型：数值 / 字符串 / 布尔 / 数组（→ 列表参数，驱动模板循环）。
/// 数组元素递归解析，因此支持嵌套数组与混合类型。
pub fn parameter_set_from_json(value: &serde_json::Value) -> Result<ParameterSet, CliError> {
    let obj = value
        .as_object()
        .ok_or_else(|| CliError::new("args", "参数应为 JSON 对象（键值对）"))?;
    let mut set = ParameterSet::new();
    for (k, v) in obj {
        set.values.insert(k.clone(), json_to_param_value(k, v)?);
    }
    Ok(set)
}

/// JSON 值 → 参数值（递归；`path` 用于错误定位）。
fn json_to_param_value(path: &str, v: &serde_json::Value) -> Result<ParamValue, CliError> {
    match v {
        serde_json::Value::Number(n) => {
            let f = n
                .as_f64()
                .ok_or_else(|| CliError::new("args", format!("参数 {path} 数值无法解析为 f64")))?;
            if !f.is_finite() {
                return Err(CliError::new(
                    "args",
                    format!("参数 {path} 为非有限数（NaN/Inf），拒绝生成"),
                ));
            }
            Ok(ParamValue::Number(f))
        }
        serde_json::Value::String(s) => Ok(ParamValue::String(s.clone())),
        serde_json::Value::Bool(b) => Ok(ParamValue::Bool(*b)),
        serde_json::Value::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for (i, item) in items.iter().enumerate() {
                // 错误信息带上标号（`passes[2].x`），否则嵌套列表里无法定位
                out.push(json_to_param_value(&format!("{path}[{i}]"), item)?);
            }
            Ok(ParamValue::List(out))
        }
        // 对象类型不直接作为参数值：模板对结构体的字段访问（`p.x`）需要的是
        // 具名结构，而 ParamValue 是扁平值模型。需要结构时建模为「平行列表」
        // （如 `xs` + `zs`）或改用模板内置的列表推导。
        serde_json::Value::Object(_) => Err(CliError::new(
            "args",
            format!(
                "参数 {path} 不支持对象类型：请改用扁平值或平行列表。\
                 若模板需要结构体，请在模板内用列表元素字段组合表达"
            ),
        )),
        serde_json::Value::Null => Err(CliError::new(
            "args",
            format!("参数 {path} 为 null：请省略该键以使用默认值，或显式提供值"),
        )),
    }
}

/// 合并参数输入：先加载 `--params-file`，再用 `--param` 覆盖（显式参数优先）。
pub fn build_parameter_set(
    params_file: Option<&Path>,
    params: &[String],
    specs: &[ParamSpec],
) -> Result<ParameterSet, CliError> {
    let mut set = ParameterSet::new();
    if let Some(path) = params_file {
        set.merge(&load_params_file(path)?);
    }
    for kv in params {
        let (k, v) = parse_kv_with_specs(kv, specs)?;
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
        let (k, v) = parse_kv_with_specs("x=21.0", &[]).unwrap();
        assert_eq!(k, "x");
        assert_eq!(v, ParamValue::Number(21.0));
        let (k, v) = parse_kv_with_specs("tool=D12", &[]).unwrap();
        assert_eq!(k, "tool");
        assert_eq!(v, ParamValue::String("D12".to_string()));
    }

    #[test]
    fn parse_kv_missing_equals() {
        assert!(parse_kv_with_specs("nokey", &[]).is_err());
        assert!(parse_kv_with_specs("=1", &[]).is_err());
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
        let (k, v) = parse_kv_with_specs("tool:s=D12", &[]).unwrap();
        assert_eq!(k, "tool");
        assert_eq!(v, ParamValue::String("D12".into()));
        let (_, v) = parse_kv_with_specs("n:n=21", &[]).unwrap();
        assert_eq!(v, ParamValue::Number(21.0));
        let (_, v) = parse_kv_with_specs("flag:b=TRUE", &[]).unwrap();
        assert_eq!(v, ParamValue::Bool(true));
        // true/false 文本经 :s 可强制为字符串
        let (_, v) = parse_kv_with_specs("note:s=true", &[]).unwrap();
        assert_eq!(v, ParamValue::String("true".into()));
        // 强制类型失败 → 报错
        assert!(parse_kv_with_specs("x:n=abc", &[]).is_err());
        assert!(parse_kv_with_specs("x:b=yes", &[]).is_err());
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
            build_parameter_set(None, &["x=1.5".to_string(), "tool=D12".to_string()], &[]).unwrap();
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
        let set = build_parameter_set(Some(&path), &["x=99.0".to_string()], &[]).unwrap();
        std::fs::remove_file(&path).ok();
        assert_eq!(set.get("x"), Some(&ParamValue::Number(99.0)));
    }

    /// 数组现在被解析为列表参数（`ParamValue::List`），不再是坏类型。
    #[test]
    fn load_params_file_array_as_list() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("nctool_test_arr_{}.json", std::process::id()));
        std::fs::write(&path, r#"{"z_offsets": [261, 463.75]}"#).unwrap();
        let set = load_params_file(&path).unwrap();
        std::fs::remove_file(&path).ok();
        assert_eq!(
            set.get("z_offsets"),
            Some(&ParamValue::List(vec![
                ParamValue::Number(261.0),
                ParamValue::Number(463.75),
            ]))
        );
    }

    /// 嵌套数组同样支持，且错误信息带下标路径。
    #[test]
    fn load_params_file_nested_and_bad_element() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("nctool_test_bad_{}.json", std::process::id()));
        // 嵌套数组中的对象元素 → 报错，且路径应定位到 passes[1][0]
        std::fs::write(&path, r#"{"passes": [[1, 2], [{"x": 1}]]}"#).unwrap();
        let err = load_params_file(&path).unwrap_err();
        std::fs::remove_file(&path).ok();
        assert!(
            err.message.contains("passes[1][0]"),
            "实际: {}",
            err.message
        );

        // 顶层对象同样拒绝
        let path2 = dir.join(format!("nctool_test_obj_{}.json", std::process::id()));
        std::fs::write(&path2, r#"{"p": {"x": 1}}"#).unwrap();
        let err2 = load_params_file(&path2).unwrap_err();
        std::fs::remove_file(&path2).ok();
        assert!(
            err2.message.contains("不支持对象类型"),
            "实际: {}",
            err2.message
        );
    }

    #[test]
    fn param_value_coerced_by_spec_whitelist() {
        // 值形如数字的字符串型参数：按白名单命中哪种解释就用哪种。
        // `U_CTB` 的候选项混有 1631 与 "DECKEL"，两种输入都应能命中。
        let specs = [
            ParamSpec::new("U_CTB", ParamKind::Any, "倒角后备刀").with_options([
                ParamValue::Integer(1631),
                ParamValue::String("DECKEL".into()),
            ]),
        ];
        let (_, v) = parse_kv_with_specs("U_CTB=1631", &specs).unwrap();
        assert_eq!(v, ParamValue::Number(1631.0), "数值解释命中 Integer(1631)");
        let (_, v) = parse_kv_with_specs("U_CTB=DECKEL", &specs).unwrap();
        assert_eq!(v, ParamValue::String("DECKEL".into()));
    }

    #[test]
    fn param_value_coerced_by_declared_kind() {
        // 无白名单时按声明的类型：String 型参数即使值形如数字也保持字符串
        // （此前会被推断成数值 → 类型不匹配，只能用 --params-file 或 k:s= 绕过）
        let specs = [ParamSpec::new("tool_name", ParamKind::String, "刀具名")];
        let (_, v) = parse_kv_with_specs("tool_name=1631", &specs).unwrap();
        assert_eq!(v, ParamValue::String("1631".into()));
        // 数值型参数照旧按启发式推断
        let specs2 = [ParamSpec::new("x", ParamKind::Number, "X 坐标")];
        let (_, v) = parse_kv_with_specs("x=21", &specs2).unwrap();
        assert_eq!(v, ParamValue::Number(21.0));
    }

    #[test]
    fn forced_type_suffix_beats_spec_coercion() {
        // `k:s=` 等显式后缀是用户明确表达意图的通道，优先于规格归一
        let specs = [ParamSpec::new("x", ParamKind::Number, "X 坐标")];
        let (_, v) = parse_kv_with_specs("x:s=1631", &specs).unwrap();
        assert_eq!(v, ParamValue::String("1631".into()));
    }

    #[test]
    fn no_spec_falls_back_to_heuristic() {
        // 无规格（文件路径模板 / 未注册模板）→ 沿用启发式推断，行为不变
        let (_, v) = parse_kv_with_specs("x=21.5", &[]).unwrap();
        assert_eq!(v, ParamValue::Number(21.5));
        let (_, v) = parse_kv_with_specs("t=D12", &[]).unwrap();
        assert_eq!(v, ParamValue::String("D12".into()));
    }

    /// `--param` 取值归一的规则有两份实现：本文件的 `coerce_param_value` 与前端
    /// `ui/index.html` 的 `coerceParamValue`（`cli/ui/index.html` 是它的副本）。
    /// 两份各自漂移，CLI 与 Web UI 就会对同一输入产出不同 G-code —— 典型的静默错误。
    ///
    /// 故用例与期望值集中在 `scripts/param_parity_cases.json`：本测试消费它，
    /// 前端由 `scripts/check_param_parity.mjs`（CI 硬门禁）消费同一份。
    /// 改任一侧都必须同步改 fixture，否则另一侧立刻失败。
    #[test]
    fn param_coercion_matches_shared_fixture() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../scripts/param_parity_cases.json"))
                .expect("param_parity_cases.json 必须是合法 JSON");
        let cases = fixture["cases"]
            .as_array()
            .expect("fixture 顶层必须有 cases 数组");
        assert!(!cases.is_empty(), "fixture 不应为空");

        for (i, case) in cases.iter().enumerate() {
            let raw = case["input"]
                .as_str()
                .unwrap_or_else(|| panic!("case #{i}: input 缺失"));
            let spec = fixture_spec(&case["spec"], i);
            // 与 `parse_kv_with_specs` 一致：归一前先 trim
            let got = coerce_param_value(raw.trim(), Some(&spec));
            let want = fixture_value(&case["expect"], i);
            assert_eq!(
                got, want,
                "case #{i}: input={raw:?} kind={:?} options={:?}",
                spec.kind, spec.options
            );
        }
    }

    /// fixture 里的 `kind` 用**前端收到的形态**（首字母大写，见
    /// `server::spec_json`），因为它同时被 `check_param_parity.mjs` 直接喂给 JS。
    /// 注意与 `ParamValue` 的 serde 形态（小写）不是一套大小写，别混用。
    fn fixture_kind(s: &str, i: usize) -> ParamKind {
        match s {
            "Number" => ParamKind::Number,
            "Integer" => ParamKind::Integer,
            "String" => ParamKind::String,
            "Bool" => ParamKind::Bool,
            "Choice" => ParamKind::Choice,
            "Any" => ParamKind::Any,
            "List" => ParamKind::List,
            other => panic!("case #{i}: 未知 kind {other:?}（应取 spec_json 的形态）"),
        }
    }

    fn fixture_spec(v: &serde_json::Value, i: usize) -> ParamSpec {
        let kind = fixture_kind(v["kind"].as_str().unwrap_or("Any"), i);
        let spec = ParamSpec::new("p", kind, "");
        match v["options"].as_array() {
            Some(opts) => spec.with_options(
                opts.iter()
                    .map(|o| fixture_value(o, i))
                    .collect::<Vec<ParamValue>>(),
            ),
            None => spec,
        }
    }

    /// 带标签形式 `{"type": ..., "value": ...}` —— 与后端序列化给 UI 的形状一致，
    /// 前端因此也走同一条 `bareValue` 解析路径。
    fn fixture_value(v: &serde_json::Value, i: usize) -> ParamValue {
        let t = v["type"]
            .as_str()
            .unwrap_or_else(|| panic!("case #{i}: type 缺失"));
        match t {
            "number" => ParamValue::Number(v["value"].as_f64().expect("number 需要数值")),
            "integer" => ParamValue::Integer(v["value"].as_i64().expect("integer 需要整数")),
            "string" => ParamValue::String(v["value"].as_str().expect("string 需要文本").into()),
            "bool" => ParamValue::Bool(v["value"].as_bool().expect("bool 需要布尔")),
            other => panic!("case #{i}: 未知 type {other:?}"),
        }
    }
}
