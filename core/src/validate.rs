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
    /// 取值不在规格声明的候选项白名单（`options`）内
    NotInOptions,
    /// 条件必选（`required_if`）未命中：参数按分支不可达而跳过（提示级，不阻断生成）
    ///
    /// 与 [`IssueKind::Missing`] **必须区分**：调用方按类别决策时，"缺参"意味着
    /// 生成结果不完整（宽松模式也要提示），而"条件跳过"是**正常的**——该分支在
    /// 本次参数下根本不可达。两者混用会让"缺参"统计虚高。
    ConditionalSkipped,
    /// 派生参数无法计算（源参数缺失/未命中表项，且规则无回退值）
    ///
    /// 归为 Error 而非警告：派生值会进入 G-code，算不出来就不该继续。
    DeriveFailed,
    /// 参数集提供了模板未引用的参数
    Unused,
    /// 规格中存在**永不生效**的声明（配置侧的静默失效）
    ///
    /// 两种成因，处置相同（都是"去改配置"）：
    /// - **参数名模板未引用**：规格写了个不存在的参数（名字拼错，或模板改名后
    ///   头部/变量库/清单未同步）；
    /// - **约束与声明的类型不匹配**：`min`/`max`/`integer` 只对数值类型生效，
    ///   声明在 `String`/`Bool`/`List` 上时永不执行。
    ///
    /// 与 [`IssueKind::Unused`] **必须区分**：后者是"用户多传了参数"（无副作用），
    /// 本类别是配置侧的问题。混用会让两者都失去意义。
    SpecInert,
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

    /// 信息级问题（不影响生成，仅说明）。
    fn info_kind(kind: IssueKind, param: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            level: ValidationLevel::Info,
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
/// - **派生**：声明了 `derive` 的参数由 [`crate::derive::apply`] 算出后参与后续
///   检查（不要求调用方提供；调用方提供了则提示会被覆盖；算不出来报 `DeriveFailed`）
/// - **缺失**：模板引用的必选变量（无 `default` 兜底）参数集未提供 → 错误；
///   若规格声明了 `required_if`，则按控制参数取值判定（条件未命中 → 不报缺失，
///   仅留一条 Info 说明该分支不可达）
/// - **类型**：参数规格声明类型与参数集实际类型不匹配 → 错误
/// - **有限性**：数值参数为 NaN/Inf（会污染 G-code）→ 错误
/// - **白名单**：规格声明了 `options` 而值不在其中 → 错误
/// - **区间**：数值超出规格声明的 `min`/`max`（含边界比较）→ 错误
/// - **整数性**：规格标记 `integer` 但值带小数（如 `5.5`）→ 错误
/// - **规格默认值自洽**：`spec.default` 自身违反类型/区间/整数/白名单约束 → 错误
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
    // 模板实际引用的变量集合：规格一致性检查与冗余检查共用
    let referenced: BTreeSet<&str> = vars.iter().map(|v| v.name.as_str()).collect();

    let mut report = ValidationReport::default();

    check_derive_shadowed(specs, params, &mut report);

    // 派生：把派生参数算出来，**后续检查针对派生后的集合**——这样派生参数
    // 既不会被误报"缺失"，其派生值也会照常过类型/白名单/区间检查。
    // 派生失败（源参数缺失且无回退值）报 Error：派生值会进入 G-code，算不出来
    // 就不该继续；同时退回原始集合，让其余参数的问题照常报出。
    //
    // 这段只能留在本函数：派生集合要么新建（`derived`）、要么退回入参，
    // 返回的引用可能指向二者之一，借用关系无法封装进一个函数的返回值。
    let derived;
    let params = match crate::derive::apply(specs, params) {
        Ok(effective) => {
            derived = effective;
            &derived
        }
        Err(err) => {
            report.issues.push(ValidationIssue::error_kind(
                IssueKind::DeriveFailed,
                err.target(),
                err.to_string(),
            ));
            params
        }
    };

    check_spec_defaults(specs, &mut report);
    check_spec_declarations(specs, &referenced, &mut report);
    check_var_values(vars, &spec_map, params, template_name, &mut report);
    check_missing(
        vars,
        &spec_map,
        params,
        system_vars,
        template_name,
        &mut report,
    );
    check_unused(params, &referenced, system_vars, &mut report);

    report
}

/// 派生参数被显式提供 → 提示：派生值恒胜，调用方给的值会被覆盖。
///
/// 归为 `ShadowedSystemVar`（与"与 machine 同名"同类）：都是"系统注入值覆盖了
/// 用户提供的值"，调用方的处置也相同（该参数无效）。
fn check_derive_shadowed(
    specs: &[ParamSpec],
    params: &ParameterSet,
    report: &mut ValidationReport,
) {
    for spec in specs {
        if let Some(rule) = &spec.derive {
            if params.contains(&spec.name) {
                report.issues.push(ValidationIssue::warning_kind(
                    IssueKind::ShadowedSystemVar,
                    &spec.name,
                    format!(
                        "该参数由派生规则计算（{}），调用方提供的值会被覆盖（该参数无效）",
                        rule.display()
                    ),
                ));
            }
        }
    }
}

/// 规格默认值自身的自洽性：`default` 写错（类型不符 / 越界 / 非整数 / 不在
/// 候选项内）时，它会在渲染前被静默注入上下文，用户提供的合法值反而用不上。
/// 这类错误只源于模板作者，必须在校验阶段暴露。
fn check_spec_defaults(specs: &[ParamSpec], report: &mut ValidationReport) {
    for spec in specs {
        if let Some(default) = &spec.default {
            // 类型必须单独把门：`check_value_constraints` 对非数值类型在
            // `as_f64()` 处提前返回，因此 `Number` 规格配 `String` 默认值
            // 会一路静默通过，直到渲染时把字符串塞进 `nc_fixed` 才炸。
            if !spec.kind.matches(default) {
                report.issues.push(ValidationIssue::error_kind(
                    IssueKind::TypeMismatch,
                    &spec.name,
                    format!(
                        "规格默认值类型不匹配：规格要求 {}, 默认值为 {}（规格默认值）",
                        spec.kind.label(),
                        default.type_name()
                    ),
                ));
                // 类型都不对时不再叠区间/白名单错误，避免同一参数刷出噪声
                continue;
            }
            check_value_options(spec, default, report, "（规格默认值）");
            check_value_constraints(spec, default, report, "（规格默认值）");
        }
    }
}

/// 规格声明**自身**的两类静默失效（与取值无关，只看规格与模板的对应关系）。
///
/// 1. 声明了模板未引用的参数 → 该规格永远不会被执行（类型/区间/白名单全部
///    静默失效）。典型成因是参数名拼错，或模板改名后头部/变量库/清单未同步。
/// 2. 区间 / 整数约束声明在 String/Bool/List 上 → 永不执行
///    （`check_value_constraints` 对非数值提前返回）。注意 `Any` 不算：
///    它的值可能恰好是数值，约束会按值生效。
///
/// 两者都只报警告：模板本身仍可用。
fn check_spec_declarations(
    specs: &[ParamSpec],
    referenced: &BTreeSet<&str>,
    report: &mut ValidationReport,
) {
    for spec in specs {
        if !referenced.contains(spec.name.as_str()) {
            report.issues.push(ValidationIssue::warning_kind(
                IssueKind::SpecInert,
                &spec.name,
                "规格声明了该参数，但模板未引用（参数名可能拼错，或模板已改名而规格未同步）；该规格不会生效",
            ));
        }
    }

    for spec in specs {
        let non_numeric = matches!(
            spec.kind,
            ParamKind::String | ParamKind::Bool | ParamKind::List
        );
        if !non_numeric {
            continue;
        }
        let mut what: Vec<&str> = Vec::new();
        if spec.min.is_some() || spec.max.is_some() {
            what.push("区间");
        }
        if spec.integer {
            what.push("整数");
        }
        if what.is_empty() {
            continue;
        }
        report.issues.push(ValidationIssue::warning_kind(
            IssueKind::SpecInert,
            &spec.name,
            format!(
                "规格声明了{}约束，但类型是{}——数值约束对非数值类型永不生效",
                what.join("、"),
                spec.kind.label()
            ),
        ));
    }
}

/// 逐变量检查**已提供**的取值：有限性 → 类型 → 白名单 → 区间/整数。
///
/// 白名单与区间/整数各自已有专职函数（[`check_value_options`] /
/// [`check_value_constraints`]），这里只负责顺序与"类型不匹配就不再往下查"的
/// 短路——同一参数刷多条噪声反而掩盖真正要改的那一项。
fn check_var_values(
    vars: &[nctool_tpl::Variable],
    spec_map: &std::collections::HashMap<&str, &ParamSpec>,
    params: &ParameterSet,
    template_name: Option<&str>,
    report: &mut ValidationReport,
) {
    for var in vars {
        let name = var.name.as_str();
        let Some(value) = params.get(name) else {
            continue;
        };
        let Some(spec) = spec_map.get(name).copied() else {
            // 无规格：只做有限性检查（NaN/Inf 会写入非法坐标），其余无从判断
            check_finite(name, value, template_name, var, report);
            continue;
        };

        check_finite(name, value, template_name, var, report);

        // 类型检查：规格声明类型与实际提供类型必须匹配
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
            continue;
        }
        // 白名单必须先于区间/整数检查：后两者对字符串枚举会在 `as_f64()`
        // 处提前返回，放在它们之后等于永不执行。
        let at = location_suffix(template_name, var);
        check_value_options(spec, value, report, &at);
        check_value_constraints(spec, value, report, &at);
    }
}

/// 有限性：数值参数必须有限（NaN/Inf 会写入非法坐标）。
fn check_finite(
    name: &str,
    value: &crate::model::ParamValue,
    template_name: Option<&str>,
    var: &nctool_tpl::Variable,
    report: &mut ValidationReport,
) {
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
}

/// 逐变量检查**未提供**的情形：系统变量 / 派生 / 有兜底 → 跳过；
/// 条件必选按控制参数取值判定；否则报 `Missing`。
fn check_missing(
    vars: &[nctool_tpl::Variable],
    spec_map: &std::collections::HashMap<&str, &ParamSpec>,
    params: &ParameterSet,
    system_vars: &[&str],
    template_name: Option<&str>,
    report: &mut ValidationReport,
) {
    for var in vars {
        let name = var.name.as_str();
        if params.get(name).is_some() {
            continue;
        }
        let spec = spec_map.get(name).copied();

        if system_vars.contains(&name) {
            continue;
        }
        // 派生参数由系统注入：不要求调用方提供。派生失败时已单独报
        // `DeriveFailed`，这里不再叠一条"缺失"造成双重报错。
        if spec.and_then(|s| s.derive.as_ref()).is_some() {
            continue;
        }
        let has_default = var.optional || spec.and_then(|s| s.default.as_ref()).is_some();
        if has_default {
            continue;
        }
        // 无兜底 → 缺参。若规格声明了条件必选，先按控制参数的取值判定：
        // 条件未命中意味着该分支不可达（模板里的互斥分支），此时不报缺失。
        let decision = spec.map(|s| required_if_decision(s, spec_map, params));
        if let Some(RequiredDecision::NotRequired { condition }) = &decision {
            report.issues.push(ValidationIssue::info_kind(
                IssueKind::ConditionalSkipped,
                name,
                format!(
                    "未提供；条件必选要求 {condition} 才必选，当前不满足（该分支不可达），故不报缺失{}",
                    location_suffix(template_name, var)
                ),
            ));
        } else {
            let condition = match decision {
                Some(RequiredDecision::RequiredIf { condition }) => {
                    format!("；条件必选 {condition} 成立")
                }
                _ => String::new(),
            };
            report.issues.push(ValidationIssue::error_kind(
                IssueKind::Missing,
                name,
                format!(
                    "必选参数缺失（模板引用且无默认值兜底，参数集未提供）{condition}{}",
                    location_suffix(template_name, var)
                ),
            ));
        }
    }
}

/// 冗余参数：参数集提供了、但模板未引用；或与系统注入变量同名。
///
/// 前者多半是模板选错或参数名拼错；后者会在渲染时被系统值覆盖，
/// 两种情况用户提供的参数都是无效的，故都提示。
fn check_unused(
    params: &ParameterSet,
    referenced: &BTreeSet<&str>,
    system_vars: &[&str],
    report: &mut ValidationReport,
) {
    for name in params.values.keys() {
        if system_vars.contains(&name.as_str()) {
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
    value.type_name()
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

/// 缺失参数的必选性判定结果（见 [`required_if_decision`]）。
#[derive(Debug, Clone, PartialEq, Eq)]
enum RequiredDecision {
    /// 必选：无 `required_if` 声明，或条件命中，或条件不可判定（保守）
    Required,
    /// 条件命中而必选（携带条件描述，用于错误提示）
    RequiredIf {
        /// 条件的人类可读形式（如 `side = "Right"`）
        condition: String,
    },
    /// 条件明确未命中 → 该分支不可达，可缺失
    NotRequired {
        /// 条件的人类可读形式
        condition: String,
    },
}

/// 按 [`ParamSpec::required_if`] 判定一个**缺失**参数是否仍属必选。
///
/// 控制参数的**生效取值**按「用户提供值 > 规格默认值」取值，与
/// [`crate::model::apply_spec_defaults`] 的渲染期口径一致。
///
/// **不可判定时保守判必选**：控制参数既未提供、规格也没有默认值，说明它的取值
/// 要到渲染期才知道（也可能压根缺失）。此时宁可多要一个参数，也不能放过缺失——
/// 若分支真的被走到，渲染会以"未定义变量"失败，那已经是渲染期而非校验期，
/// 违背"渲染前可发现错误"。
///
/// **已知边界**：控制参数若靠**模板内联** `{{ side | default("Right") }}` 兜底
/// （即提取器标记 `optional`，规格里没有 `default`），其取值无法静态求得，
/// 同样落到"保守判必选"。要给条件必选一个可判定的控制参数，请在规格里声明
/// `default`，或在调用时显式提供该参数。
fn required_if_decision(
    spec: &ParamSpec,
    spec_map: &std::collections::HashMap<&str, &ParamSpec>,
    params: &ParameterSet,
) -> RequiredDecision {
    let Some(rif) = &spec.required_if else {
        return RequiredDecision::Required;
    };
    let condition = rif.display();
    let controlling = params.get(&rif.param).cloned().or_else(|| {
        spec_map
            .get(rif.param.as_str())
            .and_then(|s| s.default.clone())
    });
    match controlling {
        Some(value) if rif.triggered_by(&value) => RequiredDecision::RequiredIf { condition },
        Some(_) => RequiredDecision::NotRequired { condition },
        None => RequiredDecision::Required,
    }
}

/// 校验单个参数值是否落在规格声明的候选项白名单（`options`）内。
///
/// **必须与 [`check_value_constraints`] 并列调用，不能塞进它内部**：
/// 后者对非数值类型在 `as_f64()` 处直接 `return`，而字符串枚举
/// （`闭口 / 左开口 / 右开口`）恰恰是白名单的主要用途，放进去等于永不检查。
///
/// 前置条件：类型检查已通过（`spec.kind.matches(value)`）——类型都不匹配时
/// 再报"不在候选项内"只是噪声，且会掩盖真正的错误原因。
///
/// 语义：
/// - `options` 为 `None` 或空 → 不约束，直接返回（旧规格行为不变）；
/// - 非空 → 值必须与某个候选项等价（见 [`ParamValue::matches_option`]）；
/// - 非法值**不降级、不替换**，一律报 `Error`：静默改成默认值会产出与图纸
///   不符的 G-code，这正是白名单要堵住的问题。
fn check_value_options(
    spec: &ParamSpec,
    value: &crate::model::ParamValue,
    report: &mut ValidationReport,
    suffix: &str,
) {
    let Some(accepted) = spec.accepts_option(value) else {
        return; // 未声明有效白名单，不约束
    };
    if accepted {
        return;
    }
    let candidates = spec.options_display().unwrap_or_default();
    report.issues.push(ValidationIssue::error_kind(
        IssueKind::NotInOptions,
        &spec.name,
        format!(
            "取值 {} 不在候选项内，可选：{candidates}{}{suffix}",
            render_value(value),
            spec.unit_suffix()
        ),
    ));
}

/// 参数值的可读渲染（错误提示用）；统一走 [`crate::model::ParamValue::display`]，
/// 与候选值渲染口径一致，便于用户直接对照。
fn render_value(value: &crate::model::ParamValue) -> String {
    value.display()
}

/// 便捷：构造带默认值的规格（测试与内置模板用）。
///
/// 只填写基础字段，不带取值范围/整数约束/候选项。需要约束时用 [`ParamSpec::new`]
/// 配合 `with_min` / `with_max` / `require_integer` / `with_options` 等构造器。
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
        options: None,
        required_if: None,
        derive: None,
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

    // -------------------------------------------------------------------
    // 候选项白名单（options / NotInOptions）
    // -------------------------------------------------------------------

    /// 越程槽形式：源项目 `U_FX` 的候选集合。
    fn u_fx_spec() -> ParamSpec {
        ParamSpec::new("x", ParamKind::Choice, "越程槽形式").with_options([
            ParamValue::String("闭口".into()),
            ParamValue::String("左开口".into()),
            ParamValue::String("右开口".into()),
        ])
    }

    #[test]
    fn choice_valid_option_passes() {
        let specs = [u_fx_spec()];
        for ok in ["闭口", "左开口", "右开口"] {
            let mut ps = ParameterSet::new();
            ps.set_string("x", ok).set_number("z", 5.0);
            let report = validate_template(TPL, "t.j2", &specs, &ps, &[]);
            assert!(report.is_ok(), "{ok} 是合法候选: {}", report.summary());
        }
    }

    #[test]
    fn choice_invalid_option_reports_not_in_options() {
        // 回归：白名单此前只能写在模板注释里，非法选项会一路渲染成错误 G-code
        let specs = [u_fx_spec()];
        let mut ps = ParameterSet::new();
        ps.set_string("x", "上开口").set_number("z", 5.0);
        let report = validate_template(TPL, "t.j2", &specs, &ps, &[]);
        assert!(
            report.has_kind(IssueKind::NotInOptions),
            "非法选项应报 NotInOptions: {}",
            report.summary()
        );
        let issue = report.of_kind(IssueKind::NotInOptions).next().unwrap();
        assert_eq!(issue.level, ValidationLevel::Error);
        assert_eq!(issue.param.as_deref(), Some("x"));
        // 报错里必须列出候选值，否则用户无从修正
        assert!(issue.message.contains("左开口"), "{}", issue.message);
    }

    #[test]
    fn choice_without_options_is_unconstrained() {
        // 未声明白名单 → 不做成员检查（保证旧规格与半成品规格不被误杀）
        let specs = [ParamSpec::new("x", ParamKind::Choice, "任意文本")];
        let mut ps = ParameterSet::new();
        ps.set_string("x", "随便什么").set_number("z", 5.0);
        let report = validate_template(TPL, "t.j2", &specs, &ps, &[]);
        assert!(report.is_ok(), "{}", report.summary());
        assert!(!report.has_kind(IssueKind::NotInOptions));
    }

    #[test]
    fn options_on_number_kind_also_enforced() {
        // options 不只对 Choice 生效：数值枚举同样受约束
        let specs = [ParamSpec::new("x", ParamKind::Number, "卡簧槽宽")
            .with_options([ParamValue::Number(8.0), ParamValue::Number(12.5)])];
        let mut ps = ParameterSet::new();
        ps.set_number("x", 9.0).set_number("z", 5.0);
        let report = validate_template(TPL, "t.j2", &specs, &ps, &[]);
        assert!(
            report.has_kind(IssueKind::NotInOptions),
            "{}",
            report.summary()
        );
    }

    #[test]
    fn numeric_option_accepts_equivalent_number_and_integer() {
        // JSON/CLI 把 8 解析成 8.0 时不应假拒绝（8.0 == 8 数值恒等）
        let specs = [ParamSpec::new("x", ParamKind::Choice, "槽宽系列")
            .with_options([ParamValue::Integer(0), ParamValue::Integer(8)])];
        for v in [8.0_f64, 0.0] {
            let mut ps = ParameterSet::new();
            ps.set_number("x", v).set_number("z", 5.0);
            let report = validate_template(TPL, "t.j2", &specs, &ps, &[]);
            assert!(report.is_ok(), "{v} 应命中整数候选: {}", report.summary());
        }
        let mut ps = ParameterSet::new();
        ps.set_integer("x", 8).set_number("z", 5.0);
        let report = validate_template(TPL, "t.j2", &specs, &ps, &[]);
        assert!(report.is_ok(), "{}", report.summary());
    }

    #[test]
    fn choice_scalar_value_fails_whitelist_not_type() {
        // Choice 有意放宽类型匹配：数值/字符串/布尔都先过类型关，由白名单定夺。
        // 因此"填了数值但候选是文本"报的是 NotInOptions 而非 TypeMismatch ——
        // 用户填错的是工艺选项，不是数据类型，提示要贴近真实原因。
        let specs = [u_fx_spec()];
        let mut ps = ParameterSet::new();
        ps.set_number("x", 1.0).set_number("z", 5.0);
        let report = validate_template(TPL, "t.j2", &specs, &ps, &[]);
        assert!(
            report.has_kind(IssueKind::NotInOptions),
            "{}",
            report.summary()
        );
        assert!(
            !report.has_kind(IssueKind::TypeMismatch),
            "标量不该被判类型不匹配: {}",
            report.summary()
        );
    }

    #[test]
    fn type_mismatch_suppresses_options_noise() {
        // 类型都不匹配时，只报类型错，不再叠一条"不在候选项内"的噪声
        // （1）列表值对 Choice 属类型不匹配：列表不可能是扁平白名单的成员
        let specs = [u_fx_spec()];
        let mut ps = ParameterSet::new();
        ps.values.insert(
            "x".into(),
            ParamValue::List(vec![ParamValue::String("闭口".into())]),
        );
        ps.set_number("z", 5.0);
        let report = validate_template(TPL, "t.j2", &specs, &ps, &[]);
        assert!(
            report.has_kind(IssueKind::TypeMismatch),
            "{}",
            report.summary()
        );
        assert!(
            !report.has_kind(IssueKind::NotInOptions),
            "{}",
            report.summary()
        );

        // （2）数值规格 + 字符串值：同样只报类型错
        let specs =
            [ParamSpec::new("x", ParamKind::Number, "槽宽")
                .with_options([ParamValue::Number(8.0)])];
        let mut ps = ParameterSet::new();
        ps.set_string("x", "8").set_number("z", 5.0);
        let report = validate_template(TPL, "t.j2", &specs, &ps, &[]);
        assert!(
            report.has_kind(IssueKind::TypeMismatch),
            "{}",
            report.summary()
        );
        assert!(
            !report.has_kind(IssueKind::NotInOptions),
            "类型不匹配时不应重复报白名单错误: {}",
            report.summary()
        );
    }

    #[test]
    fn default_not_in_options_is_rejected() {
        // 回归：规格默认值此前只查区间/整数。默认值不在候选项内会被静默注入，
        // 用户提供的合法值反而用不上 —— 必须在校验阶段暴露。
        let specs = [u_fx_spec().with_default(ParamValue::String("上开口".into()))];
        let mut ps = ParameterSet::new();
        ps.set_string("x", "闭口").set_number("z", 5.0);
        let report = validate_template(TPL, "t.j2", &specs, &ps, &[]);
        assert!(
            report.has_kind(IssueKind::NotInOptions),
            "非法的规格默认值应被检出: {}",
            report.summary()
        );
        let issue = report.of_kind(IssueKind::NotInOptions).next().unwrap();
        assert!(
            issue.message.contains("规格默认值"),
            "默认值问题应标明来源: {}",
            issue.message
        );
    }

    #[test]
    fn valid_default_in_options_passes() {
        let specs = [u_fx_spec().with_default(ParamValue::String("闭口".into()))];
        let mut ps = ParameterSet::new();
        ps.set_number("z", 5.0); // x 缺失 → 用规格默认值
        let report = validate_template(TPL, "t.j2", &specs, &ps, &[]);
        assert!(report.is_ok(), "{}", report.summary());
    }

    #[test]
    fn options_round_trip_serialization() {
        // YAML（模板清单/变量库）与 JSON（HTTP API）两条路径都必须能承载 options，
        // 且往返后判定行为不变。
        let original = ParamSpec::new("U_Q", ParamKind::Choice, "槽宽系列")
            .with_options([
                ParamValue::Integer(0),
                ParamValue::Integer(8),
                ParamValue::Number(12.5),
            ])
            .with_unit("mm");

        let yaml = serde_yaml::to_string(&original).unwrap();
        assert!(yaml.contains("options"), "YAML 应写出 options: {yaml}");
        let back: ParamSpec = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(back, original);

        let json = serde_json::to_string(&original).unwrap();
        let back_json: ParamSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(back_json, original);
        assert_eq!(
            back_json.accepts_option(&ParamValue::Number(8.0)),
            Some(true)
        );
        assert_eq!(
            back_json.accepts_option(&ParamValue::Number(9.0)),
            Some(false)
        );
    }

    #[test]
    fn options_absent_in_legacy_payload_deserializes_to_none() {
        // 向后兼容：旧规格 JSON 没有 options 字段，必须仍能反序列化
        let legacy = r#"{
            "name": "x",
            "kind": "number",
            "required": true,
            "default": null,
            "min": null,
            "max": null,
            "integer": false,
            "unit": null,
            "description": "X 坐标"
        }"#;
        let spec: ParamSpec = serde_json::from_str(legacy).unwrap();
        assert_eq!(spec.options, None);
        assert_eq!(spec.accepts_option(&ParamValue::Number(1.0)), None);
        // 未声明 options 时序列化输出中不应出现该字段
        let out = serde_json::to_string(&spec).unwrap();
        assert!(!out.contains("options"), "{out}");
    }

    #[test]
    fn spec_default_type_mismatch_is_rejected() {
        // 回归：规格默认值此前从不查类型（`check_value_constraints` 对非数值
        // 提前返回），`Number` 规格配 `String` 默认值会静默注入上下文。
        let specs = [ParamSpec::new("x", ParamKind::Number, "X 坐标")
            .with_default(ParamValue::String("abc".into()))];
        let mut ps = ParameterSet::new();
        ps.set_number("x", 10.0).set_number("z", 5.0);
        let report = validate_template(TPL, "t.j2", &specs, &ps, &[]);
        assert!(
            report.has_kind(IssueKind::TypeMismatch),
            "默认值类型不符应被检出: {}",
            report.summary()
        );
        let issue = report.of_kind(IssueKind::TypeMismatch).next().unwrap();
        assert!(
            issue.message.contains("规格默认值"),
            "应标明问题来源: {}",
            issue.message
        );
        // 类型都不对时不应再叠区间/白名单噪声
        assert_eq!(report.errors().count(), 1, "{}", report.summary());
    }

    #[test]
    fn spec_default_integer_kind_accepts_integral_number() {
        // `Integer` 规格配整值浮点默认值（YAML 常把 5 解析成 5.0）不应误报
        let specs =
            [ParamSpec::new("x", ParamKind::Integer, "程序号")
                .with_default(ParamValue::Number(5.0))];
        let mut ps = ParameterSet::new();
        ps.set_integer("x", 7).set_number("z", 5.0);
        let report = validate_template(TPL, "t.j2", &specs, &ps, &[]);
        assert!(report.is_ok(), "{}", report.summary());
    }

    #[test]
    fn not_in_options_kind_is_not_downgraded_silently() {
        // 白名单是工艺约束：宽松模式（只保留 NonFinite）会把 Error 降级为警告，
        // 这里确认降级是显式的、可被调用方观察到的行为，而非静默通过。
        let specs = [u_fx_spec()];
        let mut ps = ParameterSet::new();
        ps.set_string("x", "上开口").set_number("z", 5.0);
        let mut report = validate_template(TPL, "t.j2", &specs, &ps, &[]);
        assert!(report.has_errors());
        report.downgrade_errors_except(&[IssueKind::NonFinite]);
        assert!(report.has_kind(IssueKind::NotInOptions), "类别信息应保留");
        assert!(report.has_warnings());
    }

    // -------------------------------------------------------------------
    // 条件必选（required_if）
    // -------------------------------------------------------------------

    /// `x` 仅当 `z = 5` 时必选 —— 对应 `undercut_fs.j2` 里
    /// `FS_Z_PLUS1`（side=Right 用）/ `FS_Z_MINUS1`（side=Left 用）的互斥关系。
    fn conditional_x_spec() -> ParamSpec {
        ParamSpec::new("x", ParamKind::Number, "仅 z=5 时使用的坐标")
            .required_when("z", [ParamValue::Number(5.0)])
    }

    #[test]
    fn required_if_skips_untriggered_branch() {
        // 回归：互斥分支参数此前一律报"必选缺失"，用户被迫填根本用不到的参数
        let specs = [conditional_x_spec()];
        let mut ps = ParameterSet::new();
        ps.set_number("z", 1.0); // 条件未命中 → x 可缺失
        let report = validate_template(TPL, "t.j2", &specs, &ps, &[]);
        assert!(
            report.is_ok(),
            "条件未命中时不应报缺参: {}",
            report.summary()
        );
        // 类别必须与"真缺参"区分：混用会让缺参统计虚高
        assert!(!report.has_kind(IssueKind::Missing));
        assert!(report.has_kind(IssueKind::ConditionalSkipped));
        // 但要留下说明，避免用户困惑"为什么不报缺参"
        let info: Vec<_> = report.infos().collect();
        assert_eq!(info.len(), 1, "{}", report.summary());
        assert_eq!(info[0].param.as_deref(), Some("x"));
        assert!(info[0].message.contains("z = 5"), "{}", info[0].message);
    }

    #[test]
    fn required_if_enforces_triggered_branch() {
        let specs = [conditional_x_spec()];
        let mut ps = ParameterSet::new();
        ps.set_number("z", 5.0); // 条件命中 → x 必选
        let report = validate_template(TPL, "t.j2", &specs, &ps, &[]);
        assert!(report.has_errors(), "{}", report.summary());
        let issue = report.of_kind(IssueKind::Missing).next().unwrap();
        assert_eq!(issue.param.as_deref(), Some("x"));
        // 错误消息要说清"为什么这次必选"，否则用户会以为是误报
        assert!(
            issue.message.contains("条件必选") && issue.message.contains("z = 5"),
            "{}",
            issue.message
        );
    }

    #[test]
    fn required_if_undecidable_control_is_required() {
        // 控制参数既未提供、规格也无默认值 → 不可判定 → 保守判必选。
        // 分支可能被走到，宁可多要一个参数，也不能放过缺失（渲染期才发现就晚了）。
        let specs = [conditional_x_spec()];
        let ps = ParameterSet::new(); // x、z 都缺
        let report = validate_template(TPL, "t.j2", &specs, &ps, &[]);
        assert!(
            report.errors().any(|e| e.param.as_deref() == Some("x")),
            "控制参数不可判定时应保守判必选: {}",
            report.summary()
        );
    }

    #[test]
    fn required_if_reads_control_spec_default() {
        // 控制参数的生效取值遵循「用户提供值 > 规格默认值」，与渲染期口径一致
        let specs = [
            ParamSpec::new("z", ParamKind::Number, "控制参数")
                .with_default(ParamValue::Number(5.0)),
            conditional_x_spec(),
        ];
        let ps = ParameterSet::new(); // 都不提供：z 由规格默认值兜底 = 5 → x 必选
        let report = validate_template(TPL, "t.j2", &specs, &ps, &[]);
        assert!(
            report.errors().any(|e| e.param.as_deref() == Some("x")),
            "规格默认值应参与条件判定: {}",
            report.summary()
        );

        // 用户提供的值优先于规格默认值：z=1 → 条件不命中 → x 可缺失
        let mut ps2 = ParameterSet::new();
        ps2.set_number("z", 1.0);
        let report2 = validate_template(TPL, "t.j2", &specs, &ps2, &[]);
        assert!(report2.is_ok(), "{}", report2.summary());
    }

    #[test]
    fn required_if_absent_keeps_unconditional_semantics() {
        // 未声明 required_if 时行为完全不变（旧规格不受影响）
        let specs = [ParamSpec::new("x", ParamKind::Number, "X")];
        let mut ps = ParameterSet::new();
        ps.set_number("z", 1.0);
        let report = validate_template(TPL, "t.j2", &specs, &ps, &[]);
        assert!(report.has_errors(), "{}", report.summary());
        assert!(!report.has_kind(IssueKind::NotInOptions));
        assert!(report.infos().next().is_none(), "不应凭空产生 Info");
    }

    #[test]
    fn required_if_string_trigger_matches_exactly() {
        // 字符串触发值严格匹配（"Right" 不因大小写或近似而命中）
        let specs = [ParamSpec::new("x", ParamKind::Number, "X")
            .required_when("y", [ParamValue::String("Right".into())])];
        let mut ps = ParameterSet::new();
        ps.set_string("y", "Left").set_number("z", 5.0);
        let report = validate_template(TPL, "t.j2", &specs, &ps, &[]);
        assert!(report.is_ok(), "{}", report.summary());

        let mut ps2 = ParameterSet::new();
        ps2.set_string("y", "Right").set_number("z", 5.0);
        let report2 = validate_template(TPL, "t.j2", &specs, &ps2, &[]);
        assert!(
            report2.errors().any(|e| e.param.as_deref() == Some("x")),
            "{}",
            report2.summary()
        );
    }

    // -------------------------------------------------------------------
    // 规格自身的一致性：声明了模板未引用的参数
    // -------------------------------------------------------------------

    #[test]
    fn spec_for_unreferenced_param_is_warning() {
        // 回归：规格写了个模板根本没引用的参数（参数名拼错，或模板改名后
        // 头部/变量库/清单未同步）→ 该规格的类型/区间/白名单**永不执行**，
        // 是配置侧的静默失效，必须提示。
        let specs = [spec("nope", ParamKind::Number, true, None, "拼错的参数名")];
        let mut ps = ParameterSet::new();
        ps.set_number("x", 10.0).set_number("z", 5.0);
        let report = validate_template(TPL, "t.j2", &specs, &ps, &[]);
        assert!(
            !report.has_errors(),
            "只是警告，模板仍可用: {}",
            report.summary()
        );
        assert!(report.has_kind(IssueKind::SpecInert));
        let issue = report.of_kind(IssueKind::SpecInert).next().unwrap();
        assert_eq!(issue.param.as_deref(), Some("nope"));
        assert!(
            issue.message.contains("不会生效"),
            "要说清后果: {}",
            issue.message
        );
    }

    #[test]
    fn spec_unused_is_distinct_from_unused_params() {
        // 「规格多写了参数」与「用户多传了参数」是两回事，类别必须分开：
        // 前者是配置静默失效，后者只是用户多给了个值。
        let specs = [spec("nope", ParamKind::Number, true, None, "拼错的参数名")];
        let mut ps = ParameterSet::new();
        ps.set_number("x", 10.0)
            .set_number("z", 5.0)
            .set_number("extra", 1.0);
        let report = validate_template(TPL, "t.j2", &specs, &ps, &[]);
        assert!(report.has_kind(IssueKind::SpecInert));
        assert!(report.has_kind(IssueKind::Unused));
        let spec_issue = report.of_kind(IssueKind::SpecInert).next().unwrap();
        let param_issue = report.of_kind(IssueKind::Unused).next().unwrap();
        assert_eq!(spec_issue.param.as_deref(), Some("nope"));
        assert_eq!(param_issue.param.as_deref(), Some("extra"));
    }

    #[test]
    fn matching_specs_do_not_trigger_spec_unused() {
        let specs = [spec("x", ParamKind::Number, true, None, "X")];
        let mut ps = ParameterSet::new();
        ps.set_number("x", 10.0).set_number("z", 5.0);
        let report = validate_template(TPL, "t.j2", &specs, &ps, &[]);
        assert!(
            !report.has_kind(IssueKind::SpecInert),
            "{}",
            report.summary()
        );
        assert!(report.is_ok(), "{}", report.summary());
    }

    // -------------------------------------------------------------------
    // 派生参数（derive）：系统注入、派生值恒胜、失败不静默
    // -------------------------------------------------------------------

    /// `x` 由 `z` 查表派生（表项 1 → 10，回退 99）。
    fn derived_x_spec() -> ParamSpec {
        ParamSpec::new("x", ParamKind::Number, "派生值").with_derive(crate::model::DeriveRule {
            from: "z".into(),
            table: vec![(ParamValue::Number(1.0), ParamValue::Number(10.0))],
            fallback: Some(ParamValue::Number(99.0)),
        })
    }

    #[test]
    fn derived_param_is_not_reported_missing() {
        // 派生参数由系统注入，不要求调用方提供——此前会报"必选参数缺失"
        let specs = [derived_x_spec()];
        let mut ps = ParameterSet::new();
        ps.set_number("z", 1.0);
        let report = validate_template(TPL, "t.j2", &specs, &ps, &[]);
        assert!(
            report.is_ok(),
            "派生参数不应被要求提供: {}",
            report.summary()
        );
        assert!(!report.has_kind(IssueKind::Missing));
    }

    #[test]
    fn user_supplied_derived_param_warns_about_override() {
        // 派生值恒胜：用户提供的值会被覆盖，必须显式提示（不是错误——调用方
        // 可能不知道某参数已改为派生）
        let specs = [derived_x_spec()];
        let mut ps = ParameterSet::new();
        ps.set_number("z", 1.0).set_number("x", 999.0);
        let report = validate_template(TPL, "t.j2", &specs, &ps, &[]);
        assert!(!report.has_errors(), "只是提示: {}", report.summary());
        assert!(report.has_kind(IssueKind::ShadowedSystemVar));
        let issue = report.of_kind(IssueKind::ShadowedSystemVar).next().unwrap();
        assert_eq!(issue.param.as_deref(), Some("x"));
        assert!(issue.message.contains("派生"), "{}", issue.message);
        assert!(issue.message.contains("覆盖"), "{}", issue.message);
    }

    #[test]
    fn derive_failure_is_error_and_not_double_reported_as_missing() {
        // 源参数缺失且规则无回退值 → DeriveFailed（Error）；同时不再叠一条
        // "缺失"，否则同一参数刷出两条互相矛盾的问题
        let mut spec = derived_x_spec();
        spec.derive.as_mut().unwrap().fallback = None;
        // 源参数 z 也不提供 → 派生失败（注意：若给了 z=1.0 会命中表项，派生反而成功）
        let ps = ParameterSet::new();
        let report = validate_template(TPL, "t.j2", &[spec], &ps, &[]);
        assert!(
            report.has_kind(IssueKind::DeriveFailed),
            "{}",
            report.summary()
        );
        // 只断言**同一参数**不再报缺失：源参数 z 自身缺参仍应照报（那是另一个问题）
        assert!(
            !report
                .of_kind(IssueKind::Missing)
                .any(|i| i.param.as_deref() == Some("x")),
            "派生失败不应再对 x 报缺失: {}",
            report.summary()
        );
        let issue = report.of_kind(IssueKind::DeriveFailed).next().unwrap();
        assert_eq!(issue.level, ValidationLevel::Error);
        assert_eq!(issue.param.as_deref(), Some("x"));
    }

    #[test]
    fn derived_value_itself_is_checked_against_spec() {
        // 派生值也要过规格检查：表里写错了越界值必须被拦下（否则派生成了
        // 绕过校验的后门）
        let spec = ParamSpec::new("x", ParamKind::Number, "派生值")
            .with_range(0.0, 5.0)
            .with_derive(crate::model::DeriveRule {
                from: "z".into(),
                table: vec![(ParamValue::Number(1.0), ParamValue::Number(50.0))],
                fallback: None,
            });
        let mut ps = ParameterSet::new();
        ps.set_number("z", 1.0);
        let report = validate_template(TPL, "t.j2", &[spec], &ps, &[]);
        assert!(
            report.has_kind(IssueKind::OutOfRange),
            "派生值越界应被检出: {}",
            report.summary()
        );
    }

    #[test]
    fn numeric_constraint_on_non_numeric_kind_is_reported_inert() {
        // 回归：`min`/`max`/`integer` 只对数值生效，声明在 String/Bool/List 上
        // 时 `check_value_constraints` 会在 `as_f64()` 处提前返回——约束永不执行，
        // 是配置侧的静默失效（与"规格写了个不存在的参数"同类）。
        let specs = [ParamSpec::new("x", ParamKind::String, "文本参数").with_range(0.0, 10.0)];
        let mut ps = ParameterSet::new();
        ps.set_string("x", "abc").set_number("z", 5.0);
        let report = validate_template(TPL, "t.j2", &specs, &ps, &[]);
        assert!(
            report.has_kind(IssueKind::SpecInert),
            "非数值类型上的数值约束应被提示: {}",
            report.summary()
        );
        assert!(!report.has_errors(), "只是警告: {}", report.summary());
        let issue = report.of_kind(IssueKind::SpecInert).next().unwrap();
        assert_eq!(issue.param.as_deref(), Some("x"));
        assert!(issue.message.contains("永不生效"), "{}", issue.message);
    }

    #[test]
    fn numeric_constraint_on_any_kind_is_not_flagged() {
        // `Any` 的值可能恰好是数值，约束会按值生效，不该报警
        let specs = [ParamSpec::new("x", ParamKind::Any, "未标注类型").with_min(0.0)];
        let mut ps = ParameterSet::new();
        ps.set_number("x", 1.0).set_number("z", 5.0);
        let report = validate_template(TPL, "t.j2", &specs, &ps, &[]);
        assert!(
            !report.has_kind(IssueKind::SpecInert),
            "Any 类型不应被判定为约束失效: {}",
            report.summary()
        );
    }
}
