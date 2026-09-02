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

/// 校验问题的结构化类别。
///
/// 存在意义：调用方需要**按类别**而非按消息文本做决策。典型场景是
/// [`crate::pipeline::GCodeGenerator::generate_lenient`] —— 宽松模式放行
/// 绝大多数校验问题，但**必须**拦截 `NonFinite`（NaN/Inf 会写出非法坐标），
/// 靠 `message.contains("NaN")` 这样的文本匹配是脆弱且易失效的。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum IssueKind {
    /// 必选参数缺失
    Missing,
    /// 类型不匹配
    TypeMismatch,
    /// 数值非有限（NaN / Inf）
    NonFinite,
    /// 超出规格声明的取值区间（min / max）
    OutOfRange,
    /// 违反整数约束（规格要求整数，实际带小数）
    NotInteger,
    /// 参数集提供了模板未引用的参数
    Unused,
    /// 参数与系统注入变量同名
    ShadowedSystemVar,
    /// 模板解析失败
    ParseError,
    /// 其他 / 未分类
    Other,
}

/// 单条校验问题。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationIssue {
    /// 级别
    pub level: ValidationLevel,
    /// 结构化类别（调用方据此做程序化决策，勿依赖 `message` 文本）
    pub kind: IssueKind,
    /// 涉及的参数名（无则 `None`）
    pub param: Option<String>,
    /// 问题描述
    pub message: String,
}

impl ValidationIssue {
    /// 错误级问题（`kind` 必填：调用方按类别决策，不依赖消息文本）。
    fn error_kind(kind: IssueKind, param: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            level: ValidationLevel::Error,
            kind,
            param: Some(param.into()),
            message: message.into(),
        }
    }

    /// 警告级问题。
    fn warning_kind(kind: IssueKind, param: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            level: ValidationLevel::Warning,
            kind,
            param: Some(param.into()),
            message: message.into(),
        }
    }

    /// 全局问题（不归属具体参数）。
    fn global_kind(level: ValidationLevel, kind: IssueKind, message: impl Into<String>) -> Self {
        Self {
            level,
            kind,
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

    /// 是否包含指定类别的问题（任意级别）。
    ///
    /// 用于按类别做程序化决策，例如宽松模式拦截 NaN：
    /// `report.has_kind(IssueKind::NonFinite)`。
    pub fn has_kind(&self, kind: IssueKind) -> bool {
        self.issues.iter().any(|i| i.kind == kind)
    }

    /// 迭代指定类别的问题。
    pub fn of_kind(&self, kind: IssueKind) -> impl Iterator<Item = &ValidationIssue> {
        self.issues.iter().filter(move |i| i.kind == kind)
    }

    /// 把 Error 级问题降级为 Warning，**保留** `keep` 中列出的类别。
    ///
    /// 宽松生成（[`crate::pipeline::GCodeGenerator::generate_lenient`]）用它表达
    /// "除列出的类别外，其余问题不阻断生成、仅作提示"：报告级别与"是否阻断"
    /// 保持一致，调用方拿到报告即可直接展示，无需再自行判断。
    ///
    /// 典型用法：`report.downgrade_errors_except(&[IssueKind::NonFinite])`
    /// —— NaN/Inf 会让机床走到非法坐标，宽松模式也必须硬失败，故保留。
    pub fn downgrade_errors_except(&mut self, keep: &[IssueKind]) {
        for issue in &mut self.issues {
            if issue.level == ValidationLevel::Error && !keep.contains(&issue.kind) {
                issue.level = ValidationLevel::Warning;
            }
        }
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
                issues: vec![ValidationIssue::global_kind(
                    ValidationLevel::Error,
                    IssueKind::ParseError,
                    format!("模板解析失败：{err}"),
                )],
            }
        }
    };
    // 2. 共享校验核心
    check_vars(&vars, specs, params, system_vars, Some(template_name))
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
    check_vars(vars, specs, params, system_vars, None)
}

/// 校验共享核心：对照变量列表、规格与参数集逐项检查。
///
/// 检查规则：
/// - **缺失**：模板引用的必选变量（无 `default` 兜底）参数集未提供 → 错误
/// - **类型**：参数规格声明类型与参数集实际类型不匹配 → 错误
/// - **有限性**：数值参数为 NaN/Inf（会污染 G-code）→ 错误
/// - **区间**：数值超出规格声明的 `min`/`max`（含边界比较）→ 错误
/// - **整数性**：规格标记 `integer` 但值带小数（如 `5.5`）→ 错误
/// - **规格默认值自洽**：`spec.default` 自身违反类型/区间/整数约束 → 错误
/// - **冗余**：参数集提供了模板未引用的参数 → 警告
///
/// `system_vars` 由系统在渲染时注入，视为已提供，不参与缺失/冗余检查。
/// `template_name`：仅供错误消息定位（`validate_with_vars` 场景可为 `None`）。
fn check_vars(
    vars: &[nctool_tpl::Variable],
    specs: &[ParamSpec],
    params: &ParameterSet,
    system_vars: &[&str],
    template_name: Option<&str>,
) -> ValidationReport {
    // 规格索引：参数名 → ParamSpec
    let spec_map: std::collections::HashMap<&str, &ParamSpec> =
        specs.iter().map(|s| (s.name.as_str(), s)).collect();

    let mut report = ValidationReport::default();

    // 规格默认值自身的自洽性：default 写错（类型不符 / 越界 / 非整数）时，
    // 它会在渲染前被静默注入上下文，用户提供的合法值反而用不上。
    // 这类错误只源于模板作者，必须在校验阶段暴露。
    for spec in specs {
        if let Some(default) = &spec.default {
            check_value_constraints(spec, default, &mut report, "（规格默认值）");
        }
    }

    // 逐变量检查
    for var in vars {
        let name = var.name.as_str();
        let spec = spec_map.get(name).copied();

        match params.get(name) {
            Some(value) => {
                // 有限性检查：数值必须有限（NaN/Inf 会写入非法坐标）
                if let crate::model::ParamValue::Number(n) = value {
                    if !n.is_finite() {
                        report.issues.push(ValidationIssue::error_kind(
                            IssueKind::NonFinite,
                            name,
                            format!(
                                "数值参数为 NaN/Inf（非有限数），拒绝生成{}",
                                location_suffix(template_name, var)
                            ),
                        ));
                    }
                }
                // 类型检查：规格声明类型与实际提供类型必须匹配
                if let Some(spec) = spec {
                    if !spec.kind.matches(value) {
                        report.issues.push(ValidationIssue::error_kind(
                            IssueKind::TypeMismatch,
                            name,
                            format!(
                                "类型不匹配：规格要求 {}, 实际提供 {}{}",
                                spec.kind.label(),
                                value_kind_label(value),
                                location_suffix(template_name, var)
                            ),
                        ));
                    }
                    // 取值区间 / 整数约束（类型已不匹配时不再重复报错，
                    // 避免同一参数刷出多条噪声）
                    if spec.kind.matches(value) {
                        check_value_constraints(
                            spec,
                            value,
                            &mut report,
                            &location_suffix(template_name, var),
                        );
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
                    report.issues.push(ValidationIssue::error_kind(
                        IssueKind::Missing,
                        name,
                        format!(
                            "必选参数缺失（模板引用且无默认值兜底，参数集未提供）{}",
                            location_suffix(template_name, var)
                        ),
                    ));
                }
            }
        }
    }

    // 冗余参数检查：参数集提供了、但模板未引用的参数
    let referenced: BTreeSet<&str> = vars.iter().map(|v| v.name.as_str()).collect();
    for name in params.values.keys() {
        if system_vars.contains(&name.as_str()) {
            // 与系统注入变量同名：渲染时被系统值覆盖，用户提供的值无效
            report.issues.push(ValidationIssue::warning_kind(
                IssueKind::ShadowedSystemVar,
                name,
                "参数与系统注入变量（machine 等）同名，渲染时将被系统值覆盖（该参数无效）",
            ));
            continue;
        }
        if !referenced.contains(name.as_str()) {
            report.issues.push(ValidationIssue::warning_kind(
                IssueKind::Unused,
                name,
                "参数集提供了该参数，但模板未引用（可能是模板选错或参数名拼写错误）",
            ));
        }
    }

    report
}

/// 校验问题定位后缀（nctool-tpl 的 `Variable` 携带行列）：
/// `（模板 name 第 L 行第 C 列引用）`；模板名不可用时仅行列。
fn location_suffix(template_name: Option<&str>, v: &nctool_tpl::Variable) -> String {
    match template_name {
        Some(t) => format!("（模板 {t} 第 {} 行第 {} 列引用）", v.line, v.col),
        None => format!("（第 {} 行第 {} 列引用）", v.line, v.col),
    }
}

fn value_kind_label(value: &crate::model::ParamValue) -> &'static str {
    match value {
        crate::model::ParamValue::Number(_) => "数值",
        crate::model::ParamValue::Integer(_) => "整数",
        crate::model::ParamValue::String(_) => "字符串",
        crate::model::ParamValue::Bool(_) => "布尔",
    }
}

/// 校验单个参数值是否满足规格的**取值约束**（整数性 + 区间）。
///
/// 调用前置条件：`spec.kind.matches(value)` 已通过——类型不匹配时再报区间/整数
/// 问题只是噪声。规格默认值也走这里（此时 `suffix` 标明来源）。
///
/// 这些约束是 CNC 工艺安全的主要承载处：进给率必须为正、切削深度符号、
/// 主轴转速上界、程序号/刀具号必须为整数等，全部由 `ParamSpec` 的
/// `min` / `max` / `integer` 字段表达。
fn check_value_constraints(
    spec: &ParamSpec,
    value: &crate::model::ParamValue,
    report: &mut ValidationReport,
    suffix: &str,
) {
    let Some(n) = value.as_f64() else {
        return; // 非数值类型无区间/整数约束
    };

    // 整数约束：规格要求整数但值带小数。
    // 缺失此检查时 prog=1.7 → nc_pad 静默截断为 O0001、tool_num=5.5 → T5.5，
    // 两者都不报错，产出的 G-code 却是错的。
    if spec.integer && n.fract() != 0.0 {
        report.issues.push(ValidationIssue::error_kind(
            IssueKind::NotInteger,
            &spec.name,
            format!("参数要求整数值，实际为 {n}（小数部分会被静默丢弃或产出非法字址）{suffix}"),
        ));
    }

    // 区间约束（含边界）
    if let Some(min) = spec.min {
        if n < min {
            report.issues.push(ValidationIssue::error_kind(
                IssueKind::OutOfRange,
                &spec.name,
                format!("参数值 {n}{} 低于下界 {min}{suffix}", spec.unit_suffix()),
            ));
        }
    }
    if let Some(max) = spec.max {
        if n > max {
            report.issues.push(ValidationIssue::error_kind(
                IssueKind::OutOfRange,
                &spec.name,
                format!("参数值 {n}{} 超出上界 {max}{suffix}", spec.unit_suffix()),
            ));
        }
    }
}

/// 便捷：构造带默认值的规格（测试与内置模板用）。
///
/// 只填写基础字段，不带取值范围/整数约束。需要约束时用 [`ParamSpec::new`]
/// 配合 `with_min` / `with_max` / `require_integer` 等构造器。
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
        min: None,
        max: None,
        integer: false,
        unit: None,
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

    // -------------------------------------------------------------------
    // 结构化类别（IssueKind）：调用方按类别决策，不依赖消息文本
    // -------------------------------------------------------------------

    #[test]
    fn nan_issue_carries_nonfinite_kind() {
        // 宽松模式靠 has_kind(NonFinite) 硬失败，故类别必须可靠
        let mut ps = ParameterSet::new();
        ps.set_number("x", f64::NAN).set_number("z", 5.0);
        let report = validate_template(TPL, "t.j2", &[], &ps, &[]);
        assert!(
            report.has_kind(IssueKind::NonFinite),
            "NaN 应带 NonFinite 类别: {report:?}"
        );
    }

    #[test]
    fn missing_issue_carries_missing_kind() {
        let mut ps = ParameterSet::new();
        ps.set_number("x", 10.0);
        let report = validate_template(TPL, "t.j2", &[], &ps, &[]);
        assert!(report.has_kind(IssueKind::Missing));
    }

    #[test]
    fn unused_warning_carries_unused_kind() {
        let mut ps = ParameterSet::new();
        ps.set_number("x", 10.0)
            .set_number("z", 5.0)
            .set_number("extra", 1.0);
        let report = validate_template(TPL, "t.j2", &[], &ps, &[]);
        assert!(report.has_kind(IssueKind::Unused));
        // 冗余只是警告，不应带 Error 级别
        assert!(!report.has_errors());
    }

    #[test]
    fn downgrade_errors_except_keeps_listed_kind() {
        // 宽松模式语义：除 NonFinite 外全部降级为警告
        let mut ps = ParameterSet::new();
        ps.set_number("z", 5.0); // x 缺失 + NaN 无法共存演示，用缺失代替
        let mut report = validate_template(TPL, "t.j2", &[], &ps, &[]);
        assert!(report.has_errors());
        report.downgrade_errors_except(&[IssueKind::NonFinite]);
        assert!(
            !report.has_errors(),
            "Missing 被排除在 keep 之外，应降级: {}",
            report.summary()
        );
        assert!(report.has_warnings(), "降级后应变为警告");
    }

    // -------------------------------------------------------------------
    // 取值区间 / 整数约束
    // -------------------------------------------------------------------

    #[test]
    fn min_constraint_rejects_too_small() {
        // 进给率必须为正：CNC 里 F0 / F-5 是无意义甚至危险的
        let specs = [ParamSpec::new("x", ParamKind::Number, "进给率")
            .with_min(0.001)
            .with_unit("mm/min")];
        let mut ps = ParameterSet::new();
        ps.set_number("x", -5.0).set_number("z", 5.0);
        let report = validate_template(TPL, "t.j2", &specs, &ps, &[]);
        assert!(
            report.has_kind(IssueKind::OutOfRange),
            "{}",
            report.summary()
        );
    }

    #[test]
    fn max_constraint_rejects_too_large() {
        let specs = [ParamSpec::new("x", ParamKind::Number, "主轴转速")
            .with_max(6000.0)
            .with_unit("r/min")];
        let mut ps = ParameterSet::new();
        ps.set_number("x", 99999.0).set_number("z", 5.0);
        let report = validate_template(TPL, "t.j2", &specs, &ps, &[]);
        assert!(
            report.has_kind(IssueKind::OutOfRange),
            "{}",
            report.summary()
        );
    }

    #[test]
    fn range_boundaries_are_inclusive() {
        // 含边界：正好等于 min/max 应通过
        let specs = [ParamSpec::new("x", ParamKind::Number, "X").with_range(0.0, 100.0)];
        for v in [0.0, 100.0, 50.0] {
            let mut ps = ParameterSet::new();
            ps.set_number("x", v).set_number("z", 5.0);
            let report = validate_template(TPL, "t.j2", &specs, &ps, &[]);
            assert!(report.is_ok(), "{v} 在闭区间内应通过: {}", report.summary());
        }
    }

    #[test]
    fn integer_constraint_rejects_fractional_tool_number() {
        // 回归：tool_num=5.5 会输出非法字址 T5.5（此前静默通过）
        let specs = [ParamSpec::new("x", ParamKind::Number, "刀具号").require_integer()];
        let mut ps = ParameterSet::new();
        ps.set_number("x", 5.5).set_number("z", 5.0);
        let report = validate_template(TPL, "t.j2", &specs, &ps, &[]);
        assert!(
            report.has_kind(IssueKind::NotInteger),
            "{}",
            report.summary()
        );
    }

    #[test]
    fn integer_constraint_accepts_integral_value() {
        let specs = [ParamSpec::new("x", ParamKind::Number, "刀具号").require_integer()];
        let mut ps = ParameterSet::new();
        ps.set_number("x", 5.0).set_number("z", 5.0);
        let report = validate_template(TPL, "t.j2", &specs, &ps, &[]);
        assert!(report.is_ok(), "整值 5.0 应通过: {}", report.summary());
    }

    #[test]
    fn integer_kind_rejects_fractional() {
        let specs = [ParamSpec::new("x", ParamKind::Integer, "程序号")];
        let mut ps = ParameterSet::new();
        ps.set_number("x", 1.7).set_number("z", 5.0);
        let report = validate_template(TPL, "t.j2", &specs, &ps, &[]);
        assert!(report.has_errors(), "{:?}", report.issues);
        assert!(
            report.has_kind(IssueKind::TypeMismatch),
            "非整值对 Integer 类型应报类型不匹配: {}",
            report.summary()
        );
    }

    #[test]
    fn integer_value_passes_number_spec() {
        // 整数是数值的特例：Number 规格应接受 Integer 值
        let specs = [spec("x", ParamKind::Number, true, None, "X")];
        let mut ps = ParameterSet::new();
        ps.set_integer("x", 5).set_number("z", 5.0);
        let report = validate_template(TPL, "t.j2", &specs, &ps, &[]);
        assert!(report.is_ok(), "{}", report.summary());
    }

    #[test]
    fn spec_default_is_checked_against_constraints() {
        // 回归：spec.default 此前从不校验，写错会静默注入非法值。
        // 这里 default 越界（应 > 0，实际给 -1）必须被发现。
        let specs = [ParamSpec::new("x", ParamKind::Number, "进给率")
            .with_min(0.001)
            .with_default(ParamValue::Number(-1.0))];
        let mut ps = ParameterSet::new();
        ps.set_number("z", 5.0);
        let report = validate_template(TPL, "t.j2", &specs, &ps, &[]);
        assert!(
            report.has_kind(IssueKind::OutOfRange),
            "非法的规格默认值应被检出: {}",
            report.summary()
        );
    }

    #[test]
    fn integer_large_values_pass_i64_precision() {
        // i64 转 f64 在超大值会丢精度，确认常规 CNC 量级安全
        let specs = [ParamSpec::new("x", ParamKind::Integer, "程序号").with_max(99999.0)];
        let mut ps = ParameterSet::new();
        ps.set_integer("x", 99999).set_number("z", 5.0);
        let report = validate_template(TPL, "t.j2", &specs, &ps, &[]);
        assert!(report.is_ok(), "{}", report.summary());
    }
}
