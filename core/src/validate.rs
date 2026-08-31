//! 参数校验引擎：渲染前发现参数问题。
//!
//! 核心思想：**渲染前可发现错误**。通过解析模板并提取其引用的变量
//! （基于 [`nctool_tpl::extract_undeclared`]），结合模板注册表提供的参数规格
//! 与调用方提供的参数集，在校验阶段就定位：
//!
//! - 必选参数缺失（模板引用了、无默认值兜底、但参数集未提供）
//! - 类型不匹配（参数规格声明数值，参数集提供字符串）
//! - 冗余参数（参数集提供了模板未引用的参数，可能是模板选错或参数名拼错）

use std::collections::BTreeSet;

use crate::model::{ParamKind, ParamSpec, ParameterSet};

/// 校验问题级别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationLevel {
    /// 错误：必须修复，否则生成结果不可用
    Error,
    /// 警告：不影响生成，但值得关注
    Warning,
    /// 信息：提示性说明
    Info,
}

impl ValidationLevel {
    fn label(&self) -> &'static str {
        match self {
            ValidationLevel::Error => "错误",
            ValidationLevel::Warning => "警告",
            ValidationLevel::Info => "提示",
        }
    }
}

/// 单条校验问题。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationIssue {
    /// 级别
    pub level: ValidationLevel,
    /// 涉及的参数名（无则 `None`）
    pub param: Option<String>,
    /// 问题描述
    pub message: String,
}

impl ValidationIssue {
    fn error(param: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            level: ValidationLevel::Error,
            param: Some(param.into()),
            message: message.into(),
        }
    }

    fn warning(param: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            level: ValidationLevel::Warning,
            param: Some(param.into()),
            message: message.into(),
        }
    }

    fn global(level: ValidationLevel, message: impl Into<String>) -> Self {
        Self {
            level,
            param: None,
            message: message.into(),
        }
    }
}

/// 校验报告：一组校验问题的集合。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ValidationReport {
    /// 全部问题（按出现顺序）
    pub issues: Vec<ValidationIssue>,
}

impl ValidationReport {
    /// 是否全部通过（无 Error 级别问题）。
    pub fn is_ok(&self) -> bool {
        !self
            .issues
            .iter()
            .any(|i| i.level == ValidationLevel::Error)
    }

    /// 是否有 Error 级别问题。
    pub fn has_errors(&self) -> bool {
        !self.is_ok()
    }

    /// 是否有 Warning 级别问题。
    pub fn has_warnings(&self) -> bool {
        self.issues
            .iter()
            .any(|i| i.level == ValidationLevel::Warning)
    }

    /// 迭代错误问题。
    pub fn errors(&self) -> impl Iterator<Item = &ValidationIssue> {
        self.issues
            .iter()
            .filter(|i| i.level == ValidationLevel::Error)
    }

    /// 迭代警告问题。
    pub fn warnings(&self) -> impl Iterator<Item = &ValidationIssue> {
        self.issues
            .iter()
            .filter(|i| i.level == ValidationLevel::Warning)
    }

    /// 迭代信息问题。
    pub fn infos(&self) -> impl Iterator<Item = &ValidationIssue> {
        self.issues
            .iter()
            .filter(|i| i.level == ValidationLevel::Info)
    }

    /// 人类可读的摘要（多行，每行一条）。
    pub fn summary(&self) -> String {
        if self.issues.is_empty() {
            return "校验通过：无问题".to_string();
        }
        let mut lines = Vec::new();
        for issue in &self.issues {
            let param = issue
                .param
                .as_ref()
                .map(|p| format!("[{}] ", p))
                .unwrap_or_default();
            lines.push(format!(
                "{} {}{}",
                issue.level.label(),
                param,
                issue.message
            ));
        }
        lines.join("\n")
    }
}

/// 校验结果：`Ok(())` 表示通过，`Err(ValidationReport)` 表示存在问题。
pub type ValidationResult = Result<(), ValidationReport>;

/// 完整校验：解析模板 → 提取变量 → 对照规格与参数集检查。
///
/// # 参数
/// - `template_source`：模板源码
/// - `template_name`：模板名（用于错误定位）
/// - `specs`：模板注册表提供的参数规格（可传空切片，表示无规格信息）
/// - `params`：调用方提供的参数集
/// - `system_vars`：由系统在渲染时注入的变量名（如 `machine`），校验时视为已提供，
///   不要求参数集提供，也不作为缺失报错
///
/// # 返回
/// [`ValidationReport`]，可能包含错误与警告，需调用方根据 [`ValidationReport::is_ok`]
/// 决定是否继续渲染。
pub fn validate_template(
    template_source: &str,
    template_name: &str,
    specs: &[ParamSpec],
    params: &ParameterSet,
    system_vars: &[&str],
) -> ValidationReport {
    // 1. 解析并提取模板引用的变量
    let vars = match nctool_tpl::parse(template_source, template_name) {
        Ok(ast) => nctool_tpl::extract_undeclared(&ast),
        Err(err) => {
            return ValidationReport {
                issues: vec![ValidationIssue::global(
                    ValidationLevel::Error,
                    format!("模板解析失败：{err}"),
                )],
            }
        }
    };
    // 2. 共享校验核心
    check_vars(&vars, specs, params, system_vars)
}

/// 从 nctool-tpl 的 `Variable` 列表直接校验（跳过重新解析）。
///
/// 适用于已解析过模板、想复用提取结果的场景。
pub fn validate_with_vars(
    vars: &[nctool_tpl::Variable],
    specs: &[ParamSpec],
    params: &ParameterSet,
    system_vars: &[&str],
) -> ValidationReport {
    check_vars(vars, specs, params, system_vars)
}

/// 校验共享核心：对照变量列表、规格与参数集逐项检查。
///
/// 检查规则：
/// - **缺失**：模板引用的必选变量（无 `default` 兜底）参数集未提供 → 错误
/// - **类型**：参数规格声明类型与参数集实际类型不匹配 → 错误
/// - **有限性**：数值参数为 NaN/Inf（会污染 G-code）→ 错误
/// - **冗余**：参数集提供了模板未引用的参数 → 警告
///
/// `system_vars` 由系统在渲染时注入，视为已提供，不参与缺失/冗余检查。
fn check_vars(
    vars: &[nctool_tpl::Variable],
    specs: &[ParamSpec],
    params: &ParameterSet,
    system_vars: &[&str],
) -> ValidationReport {
    // 规格索引：参数名 → ParamSpec
    let spec_map: std::collections::HashMap<&str, &ParamSpec> =
        specs.iter().map(|s| (s.name.as_str(), s)).collect();

    let mut report = ValidationReport::default();
    let mut provided: BTreeSet<String> = BTreeSet::new();
    provided.extend(params.values.keys().cloned());
    // 系统注入变量视为已提供（不参与缺失/冗余检查）
    provided.extend(system_vars.iter().map(|s| s.to_string()));

    // 逐变量检查
    for var in vars {
        let name = var.name.as_str();
        let spec = spec_map.get(name).copied();

        match params.get(name) {
            Some(value) => {
                // 有限性检查：数值必须有限（NaN/Inf 会写入非法坐标）
                if let crate::model::ParamValue::Number(n) = value {
                    if !n.is_finite() {
                        report.issues.push(ValidationIssue::error(
                            name,
                            "数值参数为 NaN/Inf（非有限数），拒绝生成",
                        ));
                    }
                }
                // 类型检查：规格声明类型与实际提供类型必须匹配
                if let Some(spec) = spec {
                    if !spec.kind.matches(value) {
                        report.issues.push(ValidationIssue::error(
                            name,
                            format!(
                                "类型不匹配：规格要求 {}, 实际提供 {}",
                                spec.kind.label(),
                                value_kind_label(value)
                            ),
                        ));
                    }
                }
            }
            None => {
                // 未提供：若为系统变量则跳过，否则判定是否可接受
                if system_vars.contains(&name) {
                    continue;
                }
                let has_default = var.optional || spec.and_then(|s| s.default.as_ref()).is_some();
                if !has_default {
                    report.issues.push(ValidationIssue::error(
                        name,
                        "必选参数缺失（模板引用且无默认值兜底，参数集未提供）",
                    ));
                }
            }
        }
    }

    // 冗余参数检查：参数集提供了、但模板未引用的参数
    let referenced: BTreeSet<&str> = vars.iter().map(|v| v.name.as_str()).collect();
    for name in &provided {
        if !referenced.contains(name.as_str()) && !system_vars.contains(&name.as_str()) {
            report.issues.push(ValidationIssue::warning(
                name,
                "参数集提供了该参数，但模板未引用（可能是模板选错或参数名拼写错误）",
            ));
        }
    }

    report
}

/// 校验结果是否为错误（含错误级问题）。
pub fn has_errors(report: &ValidationReport) -> bool {
    report.has_errors()
}
fn value_kind_label(value: &crate::model::ParamValue) -> &'static str {
    match value {
        crate::model::ParamValue::Number(_) => "数值",
        crate::model::ParamValue::String(_) => "字符串",
        crate::model::ParamValue::Bool(_) => "布尔",
    }
}

/// 便捷：构造带默认值的规格（测试与内置模板用）。
pub fn spec(
    name: &str,
    kind: ParamKind,
    required: bool,
    default: Option<crate::model::ParamValue>,
    description: &str,
) -> ParamSpec {
    ParamSpec {
        name: name.to_string(),
        kind,
        required,
        default,
        description: description.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ParamValue;

    const TPL: &str = "X{{ x }} Y{{ y | default(0) }} Z{{ z }}";

    #[test]
    fn all_params_provided_passes() {
        let mut ps = ParameterSet::new();
        ps.set_number("x", 10.0).set_number("z", 5.0);
        let report = validate_template(TPL, "t.j2", &[], &ps, &[]);
        assert!(report.is_ok(), "应通过，实际: {}", report.summary());
    }

    #[test]
    fn missing_required_reports_error() {
        let mut ps = ParameterSet::new();
        ps.set_number("x", 10.0); // z 缺失
        let report = validate_template(TPL, "t.j2", &[], &ps, &[]);
        assert!(report.has_errors());
        let errors: Vec<_> = report.errors().collect();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].param.as_deref(), Some("z"));
        assert!(errors[0].message.contains("必选参数缺失"));
    }

    #[test]
    fn optional_variable_without_value_ok() {
        // y 有 default(0) 兜底，缺失不报错
        let mut ps = ParameterSet::new();
        ps.set_number("x", 10.0).set_number("z", 5.0);
        let report = validate_template(TPL, "t.j2", &[], &ps, &[]);
        assert!(report.is_ok());
    }

    #[test]
    fn type_mismatch_reports_error() {
        let specs = [spec("x", ParamKind::Number, true, None, "X 坐标")];
        let mut ps = ParameterSet::new();
        ps.set_string("x", "abc").set_number("z", 5.0);
        let report = validate_template(TPL, "t.j2", &specs, &ps, &[]);
        assert!(report.has_errors());
        let errors: Vec<_> = report.errors().collect();
        let type_err = errors.iter().find(|e| e.message.contains("类型不匹配"));
        assert!(
            type_err.is_some(),
            "应有类型不匹配错误: {}",
            report.summary()
        );
    }

    #[test]
    fn unreferenced_param_is_warning() {
        let mut ps = ParameterSet::new();
        ps.set_number("x", 10.0)
            .set_number("z", 5.0)
            .set_number("extra", 1.0);
        let report = validate_template(TPL, "t.j2", &[], &ps, &[]);
        assert!(!report.has_errors(), "不应有错误: {}", report.summary());
        assert!(report.has_warnings());
        let warnings: Vec<_> = report.warnings().collect();
        assert!(warnings.iter().any(|w| w.param.as_deref() == Some("extra")));
    }

    #[test]
    fn parse_error_reported() {
        let report = validate_template("{{ unclosed", "bad.j2", &[], &ParameterSet::new(), &[]);
        assert!(report.has_errors());
        assert!(report.errors().next().unwrap().message.contains("解析失败"));
    }

    #[test]
    fn spec_default_covers_missing_required() {
        // 规格声明 z 必选但有默认值 → 缺失时仍可接受
        let specs = [
            spec("x", ParamKind::Number, true, None, "X"),
            spec(
                "z",
                ParamKind::Number,
                true,
                Some(ParamValue::Number(1.0)),
                "Z",
            ),
        ];
        let mut ps = ParameterSet::new();
        ps.set_number("x", 10.0);
        let report = validate_template(TPL, "t.j2", &specs, &ps, &[]);
        assert!(report.is_ok(), "规格默认值应兜底缺失: {}", report.summary());
    }

    #[test]
    fn validate_with_vars_agrees() {
        let ast = nctool_tpl::parse(TPL, "t.j2").unwrap();
        let vars = nctool_tpl::extract_undeclared(&ast);
        let mut ps = ParameterSet::new();
        ps.set_number("x", 10.0).set_number("z", 5.0);
        let r1 = validate_template(TPL, "t.j2", &[], &ps, &[]);
        let r2 = validate_with_vars(&vars, &[], &ps, &[]);
        assert_eq!(r1.issues.len(), r2.issues.len());
    }

    #[test]
    fn summary_format() {
        let ps = ParameterSet::new();
        let report = validate_template("{{ x }}", "t.j2", &[], &ps, &[]);
        let s = report.summary();
        assert!(s.contains("错误"));
        assert!(s.contains("x"));
    }

    #[test]
    fn nan_number_rejected() {
        // NaN 数值参数应被拒绝（避免污染 G-code），而非静默通过
        let mut ps = ParameterSet::new();
        ps.set_number("x", f64::NAN).set_number("z", 5.0);
        let report = validate_template(TPL, "t.j2", &[], &ps, &[]);
        assert!(report.has_errors(), "NaN 应报错: {}", report.summary());
        assert!(
            report.errors().any(|e| e.param.as_deref() == Some("x")),
            "NaN 错误应定位到 x"
        );
    }

    #[test]
    fn infinity_number_rejected() {
        let mut ps = ParameterSet::new();
        ps.set_number("x", f64::INFINITY).set_number("z", 5.0);
        let report = validate_template(TPL, "t.j2", &[], &ps, &[]);
        assert!(report.has_errors());
    }

    #[test]
    fn finite_numbers_accepted() {
        let mut ps = ParameterSet::new();
        ps.set_number("x", -10.5).set_number("z", 0.0);
        let report = validate_template(TPL, "t.j2", &[], &ps, &[]);
        assert!(report.is_ok(), "有限数应通过: {}", report.summary());
    }

    #[test]
    fn nan_via_with_vars_rejected() {
        // 经 validate_with_vars 路径也应拦截 NaN
        let ast = nctool_tpl::parse(TPL, "t.j2").unwrap();
        let vars = nctool_tpl::extract_undeclared(&ast);
        let mut ps = ParameterSet::new();
        ps.set_number("x", f64::NAN).set_number("z", 5.0);
        let report = validate_with_vars(&vars, &[], &ps, &[]);
        assert!(report.has_errors());
    }
}
