//! 参数派生：由**规格声明**的规则在 Rust 侧算出派生参数，渲染前注入上下文。
//!
//! # 为什么需要它
//!
//! 项目核心原则是「**模板只做变量替换，计算在 Rust 侧完成**」。但迁移模板里仍有
//! 查表逻辑留在 Jinja 中——`machines/index_g420/dg_cal_ir9.j2` 用
//! `{% set tip_depth_map = {...} %}` + `map[k] | default(v)` 把顶尖型号换算成
//! 中心孔深度。这有两个问题：
//!
//! 1. **数据表写在模板里**：改表要改模板，且表与模板正文混在一起，无法单独 review；
//! 2. minijinja 的 map 没有任何方法（见项目约定 R6），只能靠下标 + `default` 绕，
//!    模板读起来是"为了绕开引擎限制"而不是"业务逻辑"。
//!
//! 本模块把这类**查表型派生**搬到规格里（[`crate::model::DeriveRule`]），由 Rust
//! 计算后注入。派生结果与 `machine` 同属**系统注入值**：
//!
//! - 调用方不必提供，校验也不要求提供（见 [`crate::validate`]）；
//! - 调用方若提供了同名参数，**派生值覆盖它**并给出提示（见下）。
//!
//! # 为什么"覆盖"而不是"让用户的值优先"
//!
//! 派生参数的值必须与源参数**一致**：表里说 `DM24 → 29.61`，用户却填 `10`，
//! 到底用哪个？允许用户覆盖就等于允许"型号与深度对不上"的 G-code —— 正是本项目
//! 零容忍的那类静默错误。因此派生值恒胜，用户的输入被忽略并**显式告警**。
//!
//! # 失败不静默
//!
//! 源参数缺失且规则没有 `fallback`、或取值未命中表项且没有 `fallback`，一律
//! **报错**而不是取 0 —— 中心孔深度取 0 会让 `I_R9[80]`（顶紧位置）算错，
//! 机床顶着工件走错位置。
//!
//! # 链式派生：源参数若本身也是派生参数，必须先算出它
//!
//! `A.derive.from = B` 而 `B` 自己也是派生参数时，`A` 的查表键必须是
//! **B 的派生结果**，而不是调用方传入的 B 值（那个值恒被派生结果覆盖），
//! 也不是 B 的规格默认值。三者一旦取错，A 会静默算出与型号不符的数值，
//! 且没有任何报错。因此 [`apply`] 按依赖顺序（重复扫描到不动点）计算，
//! 互相依赖成环时返回 [`DeriveError::Circular`]。

use crate::model::{DeriveRule, ParamSpec, ParamValue, ParameterSet};
use std::collections::BTreeSet;

/// 派生失败原因。
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum DeriveError {
    /// 源参数既未提供、规则也没有 `fallback`，无法计算
    MissingSource {
        /// 派生目标参数名
        target: String,
        /// 规则依赖的源参数名
        from: String,
    },
    /// 源参数取值未命中表项，且规则没有 `fallback`
    NoMatch {
        /// 派生目标参数名
        target: String,
        /// 规则依赖的源参数名
        from: String,
        /// 源参数的实际取值（已渲染为可读文本）
        value: String,
    },
    /// 源参数取值类型与表项键类型不兼容（如源是列表）
    UnusableSource {
        /// 派生目标参数名
        target: String,
        /// 规则依赖的源参数名
        from: String,
        /// 源参数的实际取值（已渲染为可读文本）
        value: String,
    },
    /// 派生参数互相依赖成环（或依赖了无法计算出的派生参数），无法按依赖顺序计算。
    ///
    /// 成环时没有任何"合理的"取值可用：取默认值会静默产出与型号不符的 G-code，
    /// 因此必须报错（见模块文档"链式派生"）。
    Circular {
        /// 本趟无法推进的派生参数名（规格声明顺序）
        targets: Vec<String>,
    },
}

impl DeriveError {
    /// 派生目标参数名（校验层据此把问题定位到具体参数）。
    pub fn target(&self) -> &str {
        match self {
            DeriveError::MissingSource { target, .. }
            | DeriveError::NoMatch { target, .. }
            | DeriveError::UnusableSource { target, .. } => target,
            DeriveError::Circular { targets } => {
                targets.first().map(String::as_str).unwrap_or("<派生参数>")
            }
        }
    }
}

impl std::fmt::Display for DeriveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeriveError::MissingSource { target, from } => write!(
                f,
                "派生参数 {target} 无法计算：源参数 {from} 未提供，且规则未声明 fallback"
            ),
            DeriveError::NoMatch { target, from, value } => write!(
                f,
                "派生参数 {target} 无法计算：源参数 {from} = {value} 未命中规则表项，且规则未声明 fallback"
            ),
            DeriveError::UnusableSource { target, from, value } => write!(
                f,
                "派生参数 {target} 无法计算：源参数 {from} = {value} 的类型不能用作查表键"
            ),
            DeriveError::Circular { targets } => write!(
                f,
                "派生参数 {} 互相依赖成环（或依赖了无法计算出的派生参数），无法确定计算顺序",
                targets.join(" ← ")
            ),
        }
    }
}

impl std::error::Error for DeriveError {}

/// 计算全部派生参数并注入参数集。
///
/// - 规格中**未声明 `derive`** 的参数原样保留；
/// - 声明了 `derive` 的参数**总是**以派生结果写入（覆盖调用方提供的同名值，
///   见模块文档"为什么覆盖"）；
/// - 返回值是新集合，入参不被修改（与 `crate::model::apply_spec_defaults` 一致）。
///
/// 与 `crate::model::apply_spec_defaults` 的调用顺序：**先派生、再兜底默认值**。
/// 派生依赖源参数的取值，而源参数可能是靠规格默认值兜底才存在的——因此
/// 本函数内部先对源参数应用一次规格默认值，避免"源参数有默认值却算不出派生值"。
///
/// **链式派生**（源参数本身也是派生参数）按依赖顺序计算：重复扫描直到不动点，
/// 每趟只计算"源参数已就绪"的参数。源参数若也是派生参数，取**已算出的派生值**，
/// 既不是调用方传入的同名值（它恒被派生值覆盖），也不是该参数的规格默认值。
/// 互相依赖成环时返回 [`DeriveError::Circular`]，不静默取任何值。
pub fn apply(specs: &[ParamSpec], params: &ParameterSet) -> Result<ParameterSet, DeriveError> {
    let derived: Vec<&ParamSpec> = specs.iter().filter(|s| s.derive.is_some()).collect();
    if derived.is_empty() {
        return Ok(params.clone());
    }
    // 源参数可能靠规格默认值兜底才存在；用兜底后的集合查表
    let with_defaults = crate::model::apply_spec_defaults(specs, params);
    // 同时具备"派生目标"身份的参数名：它们的取值必须等本函数算出后再用
    let derived_set: BTreeSet<&str> = derived.iter().map(|s| s.name.as_str()).collect();
    let mut out = params.clone();
    let mut computed: BTreeSet<String> = BTreeSet::new();
    let mut pending: Vec<&ParamSpec> = derived;

    // 反复扫描直到不动点：每趟至少算出一个"源参数已就绪"的参数，等价于拓扑排序。
    // 一整趟下来一个都算不出 = 剩余参数互相依赖（环）或依赖了永远算不出的派生参数。
    while !pending.is_empty() {
        let mut next = Vec::with_capacity(pending.len());
        let mut progressed = false;
        for spec in pending {
            let rule = spec.derive.as_ref().expect("已按 derive.is_some() 过滤");
            if derived_set.contains(rule.from.as_str()) && !computed.contains(&rule.from) {
                next.push(spec); // 源参数本身待派生：留到下一趟
                continue;
            }
            // 优先级：已算出的派生值 > 调用方提供的值 > 规格默认值
            let source = out
                .get(&rule.from)
                .or_else(|| with_defaults.get(&rule.from));
            let value = compute(spec, rule, source)?;
            out.values.insert(spec.name.clone(), value);
            computed.insert(spec.name.clone());
            progressed = true;
        }
        if !progressed {
            return Err(DeriveError::Circular {
                targets: next.iter().map(|s| s.name.clone()).collect(),
            });
        }
        pending = next;
    }
    Ok(out)
}

/// 单条派生规则的取值：缺失/未命中一律走 `fallback`，没有 `fallback` 就报错。
fn compute(
    spec: &ParamSpec,
    rule: &DeriveRule,
    source: Option<&ParamValue>,
) -> Result<ParamValue, DeriveError> {
    match source {
        None => rule
            .fallback
            .clone()
            .ok_or_else(|| DeriveError::MissingSource {
                target: spec.name.clone(),
                from: rule.from.clone(),
            }),
        Some(source_value) => {
            // 列表/布尔不能作查表键：直接判为不可用（而不是静默落到 fallback，
            // 那会把"传错了参数类型"变成"用了一个看似合理的默认值"）
            if matches!(source_value, ParamValue::List(_)) {
                return Err(DeriveError::UnusableSource {
                    target: spec.name.clone(),
                    from: rule.from.clone(),
                    value: render_value(source_value),
                });
            }
            match rule
                .table
                .iter()
                .find(|(key, _)| key.matches_option(source_value))
            {
                Some((_, v)) => Ok(v.clone()),
                None => rule.fallback.clone().ok_or_else(|| DeriveError::NoMatch {
                    target: spec.name.clone(),
                    from: rule.from.clone(),
                    value: render_value(source_value),
                }),
            }
        }
    }
}

/// 规格中声明了 `derive` 的参数名（供校验层识别"系统注入"参数）。
pub fn derived_names(specs: &[ParamSpec]) -> Vec<&str> {
    specs
        .iter()
        .filter(|s| s.derive.is_some())
        .map(|s| s.name.as_str())
        .collect()
}

/// 参数值的可读渲染（统一走 [`ParamValue::display`]，与错误提示口径一致）。
fn render_value(v: &ParamValue) -> String {
    v.display()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{DeriveRule, ParamKind};

    /// `tip_depth` 由 `tip_model` 查表派生，回退 DM24 的 29.61。
    fn tip_depth_spec() -> ParamSpec {
        ParamSpec::new("tip_depth", ParamKind::Number, "尾座中心孔深度").with_derive(DeriveRule {
            from: "tip_model".into(),
            table: vec![
                (ParamValue::String("B4".into()), ParamValue::Number(8.51)),
                (ParamValue::String("B10".into()), ParamValue::Number(20.32)),
                (ParamValue::String("DM24".into()), ParamValue::Number(29.61)),
            ],
            fallback: Some(ParamValue::Number(29.61)),
        })
    }

    fn params(pairs: &[(&str, ParamValue)]) -> ParameterSet {
        let mut ps = ParameterSet::new();
        for (k, v) in pairs {
            ps.values.insert((*k).to_string(), v.clone());
        }
        ps
    }

    #[test]
    fn derives_by_table_lookup() {
        let specs = [tip_depth_spec()];
        let ps = params(&[("tip_model", ParamValue::String("B10".into()))]);
        let out = apply(&specs, &ps).unwrap();
        assert_eq!(out.get("tip_depth"), Some(&ParamValue::Number(20.32)));
        // 源参数原样保留
        assert_eq!(
            out.get("tip_model"),
            Some(&ParamValue::String("B10".into()))
        );
    }

    #[test]
    fn falls_back_when_source_missing_or_unknown() {
        let specs = [tip_depth_spec()];
        // 源参数未提供 → fallback
        let out = apply(&specs, &params(&[])).unwrap();
        assert_eq!(out.get("tip_depth"), Some(&ParamValue::Number(29.61)));
        // 源参数未命中表项 → fallback
        let out = apply(
            &specs,
            &params(&[("tip_model", ParamValue::String("未知型号".into()))]),
        )
        .unwrap();
        assert_eq!(out.get("tip_depth"), Some(&ParamValue::Number(29.61)));
    }

    #[test]
    fn missing_source_without_fallback_errors() {
        // 不静默取 0：中心孔深度取 0 会让 I_R9[80]（顶紧位置）算错
        let mut spec = tip_depth_spec();
        spec.derive.as_mut().unwrap().fallback = None;
        let err = apply(&[spec], &params(&[])).unwrap_err();
        assert!(matches!(err, DeriveError::MissingSource { .. }), "{err:?}");
        assert!(err.to_string().contains("tip_depth"), "{err}");
        assert!(err.to_string().contains("tip_model"), "{err}");
    }

    #[test]
    fn no_match_without_fallback_errors() {
        let mut spec = tip_depth_spec();
        spec.derive.as_mut().unwrap().fallback = None;
        let err = apply(
            &[spec],
            &params(&[("tip_model", ParamValue::String("未知".into()))]),
        )
        .unwrap_err();
        match &err {
            DeriveError::NoMatch { value, .. } => assert_eq!(value, "\"未知\""),
            other => panic!("应为 NoMatch: {other:?}"),
        }
    }

    #[test]
    fn list_source_is_rejected_not_silently_fallen_back() {
        // 源参数是列表 → 明确报错，而不是"静默用 fallback"把传错类型掩盖过去
        let specs = [tip_depth_spec()];
        let ps = params(&[(
            "tip_model",
            ParamValue::List(vec![ParamValue::String("B4".into())]),
        )]);
        let err = apply(&specs, &ps).unwrap_err();
        assert!(matches!(err, DeriveError::UnusableSource { .. }), "{err:?}");
    }

    #[test]
    fn derived_value_overrides_user_supplied_value() {
        // 派生值恒胜：允许用户覆盖就等于允许"型号与深度对不上"的 G-code
        let specs = [tip_depth_spec()];
        let ps = params(&[
            ("tip_model", ParamValue::String("B4".into())),
            ("tip_depth", ParamValue::Number(999.0)),
        ]);
        let out = apply(&specs, &ps).unwrap();
        assert_eq!(out.get("tip_depth"), Some(&ParamValue::Number(8.51)));
    }

    #[test]
    fn source_may_come_from_spec_default() {
        // 源参数靠规格默认值兜底时才存在：派生必须先应用默认值再查表
        let specs = [
            ParamSpec::new("tip_model", ParamKind::String, "顶尖型号")
                .with_default(ParamValue::String("B10".into())),
            tip_depth_spec(),
        ];
        let out = apply(&specs, &params(&[])).unwrap();
        assert_eq!(out.get("tip_depth"), Some(&ParamValue::Number(20.32)));
    }

    #[test]
    fn numeric_table_keys_match_across_number_and_integer() {
        // 表键是整数、源参数是整值浮点（JSON 常把 8 解析成 8.0）时仍应命中
        let spec = ParamSpec::new("depth", ParamKind::Number, "深度").with_derive(DeriveRule {
            from: "size".into(),
            table: vec![
                (ParamValue::Integer(8), ParamValue::Number(1.0)),
                (ParamValue::Integer(12), ParamValue::Number(2.0)),
            ],
            fallback: None,
        });
        let out = apply(&[spec], &params(&[("size", ParamValue::Number(12.0))])).unwrap();
        assert_eq!(out.get("depth"), Some(&ParamValue::Number(2.0)));
    }

    /// `tip_z` 由 `tip_depth` 派生；`tip_z` 声明在 `tip_depth` **之前**，
    /// 用于验证计算顺序由依赖关系决定、而不是由规格声明顺序决定。
    fn chained_tip_z_spec() -> ParamSpec {
        ParamSpec::new("tip_z", ParamKind::Number, "顶尖 Z 偏置").with_derive(DeriveRule {
            from: "tip_depth".into(),
            table: vec![(ParamValue::Number(20.32), ParamValue::Number(1.0))],
            fallback: None,
        })
    }

    #[test]
    fn chained_derive_uses_computed_source_not_stale_user_value() {
        // 回归：此前 A 的查表键取的是 `with_defaults`（用户值/规格默认值），
        // 而不是 B 的派生结果。用户传了 B=999 时 A 用 999 查表 → 未命中 → 报错；
        // 更隐蔽的情形是 B 有 fallback/默认值，此时不报错，直接算出错误数值。
        let specs = [chained_tip_z_spec(), tip_depth_spec()]; // 故意把依赖方写在前面
        let ps = params(&[
            ("tip_model", ParamValue::String("B10".into())),
            ("tip_depth", ParamValue::Number(999.0)), // 将被派生值 20.32 覆盖
        ]);
        let out = apply(&specs, &ps).unwrap();
        assert_eq!(out.get("tip_depth"), Some(&ParamValue::Number(20.32)));
        assert_eq!(
            out.get("tip_z"),
            Some(&ParamValue::Number(1.0)),
            "tip_z 必须按 tip_depth 的派生结果查表，而不是用户传入的 999"
        );
    }

    #[test]
    fn chained_derive_uses_derived_value_not_spec_default() {
        // tip_depth 同时有「规格默认值 20.32」和「派生结果 29.61」（型号未命中表项
        // 走 fallback）：tip_z 必须取派生结果。此前取的是规格默认值，
        // 不报错但算出完全错误的数值。
        let mut tip_depth = tip_depth_spec();
        tip_depth.default = Some(ParamValue::Number(20.32));
        let tip_z_from_2961 = ParamSpec::new("tip_z", ParamKind::Number, "顶尖 Z 偏置")
            .with_derive(DeriveRule {
                from: "tip_depth".into(),
                // 只有 29.61 命中：若取了规格默认值 20.32，就会因无 fallback 而报错
                table: vec![(ParamValue::Number(29.61), ParamValue::Number(7.0))],
                fallback: None,
            });
        let specs = [
            tip_depth,
            tip_z_from_2961,
            ParamSpec::new("tip_model", ParamKind::String, "顶尖型号")
                .with_default(ParamValue::String("未知型号".into())),
        ];
        let out = apply(&specs, &params(&[])).unwrap();
        assert_eq!(out.get("tip_depth"), Some(&ParamValue::Number(29.61)));
        assert_eq!(
            out.get("tip_z"),
            Some(&ParamValue::Number(7.0)),
            "tip_z 必须按 tip_depth 的派生结果 29.61 查表，而不是其规格默认值 20.32"
        );
    }

    #[test]
    fn circular_derive_errors_instead_of_silently_using_stale_value() {
        // A ← B ← A：没有任何"合理"取值可用（取默认值等于静默产出错误 G-code）
        let specs = [
            ParamSpec::new("a", ParamKind::Number, "A").with_derive(DeriveRule {
                from: "b".into(),
                table: vec![(ParamValue::Number(1.0), ParamValue::Number(1.0))],
                fallback: Some(ParamValue::Number(0.0)),
            }),
            ParamSpec::new("b", ParamKind::Number, "B").with_derive(DeriveRule {
                from: "a".into(),
                table: vec![(ParamValue::Number(1.0), ParamValue::Number(1.0))],
                fallback: Some(ParamValue::Number(0.0)),
            }),
        ];
        let err = apply(&specs, &params(&[])).unwrap_err();
        match &err {
            DeriveError::Circular { targets } => {
                assert_eq!(targets.len(), 2, "{targets:?}");
                assert!(targets.contains(&"a".to_string()) && targets.contains(&"b".to_string()));
            }
            other => panic!("应为 Circular: {other:?}"),
        }
        assert!(err.to_string().contains("环"), "{err}");
    }

    #[test]
    fn specs_without_derive_are_untouched() {
        let specs = [ParamSpec::new("x", ParamKind::Number, "X")];
        let ps = params(&[("x", ParamValue::Number(1.0))]);
        let out = apply(&specs, &ps).unwrap();
        assert_eq!(out, ps);
        assert!(derived_names(&specs).is_empty());
        assert_eq!(derived_names(&[tip_depth_spec()]), vec!["tip_depth"]);
    }
}
