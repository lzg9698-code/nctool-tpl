//! # nctool-tpl —— NCtool 模板解析核心
//!
//! 基于 [minijinja]（Jinja2 的 Rust 实现，近乎零依赖、渲染最快）提供四个核心能力：
//!
//! 1. [`parse`]：语法检查 + 生成 AST（带行列定位）
//! 2. [`extract_variables`]：提取模板中**引用过**的所有变量名（含模板内部声明的）
//! 3. [`extract_undeclared`]：提取模板中引用、但**未在模板内部声明**的变量
//!    （即运行时要由外部上下文提供的参数），并区分**可选 / 必选**（见 [`Variable::optional`]）
//!    —— 对标 Python `jinja2.meta.find_undeclared_variables`
//! 4. [`Renderer`]：用上下文渲染出最终文本（G-code），内置数学过滤器集
//!
//! # 示例
//!
//! ```
//! use nctool_tpl::{parse, extract_undeclared, Variable};
//!
//! let source = r#"{% set feed = 0.15 %}G1 X{{ diameter / 2 }} F{{ feed }}"#;
//! let ast = parse(source, "demo.j2").unwrap();
//!
//! let undeclared: Vec<Variable> = extract_undeclared(&ast);
//! assert_eq!(undeclared.len(), 1);
//! assert_eq!(undeclared[0].name, "diameter");
//! assert!(!undeclared[0].optional); // 必选
//! ```
//!
//! # 稳定性
//!
//! 公共 API 为 [`parse`] / [`extract_variables`] / [`extract_undeclared`] /
//! [`Renderer`] / [`Variable`] / [`TplError`] / [`Ast`]。[`Ast`] 内部字段已私有化
//! （通过方法访问），[`TplError`] 标注 `#[non_exhaustive]`，以便未来扩展而不破坏
//! 下游。v0.x 阶段 API 仍可能调整，建议在 `Cargo.toml` 中锁定 minor 版本。

use std::collections::HashSet;
use std::fmt;
use std::path::Path;

use minijinja::machinery::ast::{self, Expr, Spanned, Stmt};
use minijinja::machinery::WhitespaceConfig;
use minijinja::syntax::SyntaxConfig;
use minijinja::Environment;

/// 引擎内置、不由上下文提供的名字（出现在模板里也不算“未声明变量”）。
const RESERVED_NAMES: &[&str] = &["loop", "self", "super", "caller"];

/// Jinja 自动注入的内置全局（函数/构造器），同样不算“需要外部提供的参数”。
/// 与 `jinja2.meta` 一致：无参数使用的这些全局名不进入未声明集合。
const BUILTIN_GLOBALS: &[&str] = &["range", "dict", "lipsum", "cycler", "joiner", "namespace"];

/// 模板中出现的一个变量及其在源码中的位置。
///
/// `line` / `col` 均为 1 起始，`start` / `end` 为源码字节偏移。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Variable {
    /// 变量名
    pub name: String,
    /// 起始行（1 起始）
    pub line: usize,
    /// 起始列（1 起始）
    pub col: usize,
    /// 起始字节偏移
    pub start: usize,
    /// 结束字节偏移（不含）
    pub end: usize,
    /// 该变量的**所有**引用是否都处于「兜底上下文」。
    ///
    /// 兜底上下文指 `default`/`d` 过滤器（`{{ x | default(0.15) }}`）或
    /// `is defined`/`is undefined` 测试（`{% if x is defined %}`）——
    /// 这些位置上的引用在变量缺失时模板仍可安全渲染。
    ///
    /// 对 [`extract_undeclared`] 的语义：`true` = 可选参数（缺失时可兜底），
    /// `false` = 必选参数（渲染时必须由外部上下文提供）。
    pub optional: bool,
}

/// 解析结果：持有模板 AST，同时保留源码与文件名引用。
///
/// 变量提取只读这个结构；渲染可复用同源文本（minijinja 内部自带 JIT 编译缓存）。
///
/// 字段均为私有，通过 [`name`](Self::name) / [`source`](Self::source) 访问，
/// 以便未来改变内部存储而不破坏公共 API。
#[derive(Debug)]
pub struct Ast<'a> {
    name: &'a str,
    source: &'a str,
    pub(crate) stmt: Stmt<'a>,
}

impl<'a> Ast<'a> {
    /// 模板名（用于错误信息）。
    pub fn name(&self) -> &str {
        self.name
    }

    /// 模板源码。
    pub fn source(&self) -> &str {
        self.source
    }
}

/// 模板解析/渲染错误。
///
/// `#[non_exhaustive]`：未来可能新增错误变体（如 `UndefinedVariable`、
/// `TemplateNotFound`），外部 match 应保留通配分支。
#[non_exhaustive]
#[derive(Debug)]
pub enum TplError {
    /// 语法错误，带模板名与行列号。
    ///
    /// `col` 为 minijinja 停止解析位置的最佳近似（来自其错误携带的字节范围），
    /// 在无法取得字节范围时回退为 `1`。
    Parse {
        name: String,
        message: String,
        line: usize,
        col: usize,
    },
    /// 渲染错误（未定义变量、过滤器不存在等）
    Render { name: String, message: String },
}

impl fmt::Display for TplError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TplError::Parse {
                name,
                message,
                line,
                ..
            } => {
                write!(f, "{name}:{line}: 模板语法错误: {message}")
            }
            TplError::Render { name, message } => {
                write!(f, "{name}: 渲染错误: {message}")
            }
        }
    }
}

impl std::error::Error for TplError {}

/// 由源码字节偏移换算 (行, 列)，均 1 起始；列以**字节**计（与 minijinja AST
/// span 的 `start_col` 口径一致）。`\n` 视为行分隔符，`\r\n` 中 `\r` 归入行尾。
fn line_col_at(source: &str, byte_offset: usize) -> (usize, usize) {
    let bytes = source.as_bytes();
    let off = byte_offset.min(bytes.len());
    let mut line = 1usize;
    let mut line_start = 0usize;
    for (i, &b) in bytes.iter().enumerate() {
        if i >= off {
            break;
        }
        if b == b'\n' {
            line += 1;
            line_start = i + 1;
        }
    }
    (line, off - line_start + 1)
}

/// 语法检查并生成 AST。
///
/// 解析失败时返回带行列定位的 [`TplError::Parse`]。列号取自 minijinja 错误携带的
/// 字节范围（需启用 `debug` feature）——它指向解析器停止处的 token，是对错误位置
/// 的最佳近似；无法取得字节范围时回退为 `col = 1`。
pub fn parse<'a>(source: &'a str, name: &'a str) -> Result<Ast<'a>, TplError> {
    let stmt = minijinja::machinery::parse(source, name, SyntaxConfig, WhitespaceConfig::default())
        .map_err(|err| {
            let (line, col) = err
                .range()
                .map(|range| line_col_at(source, range.start))
                .unwrap_or_else(|| (err.line().unwrap_or(1), 1));
            TplError::Parse {
                name: name.to_string(),
                message: err.to_string(),
                line,
                col,
            }
        })?;
    Ok(Ast { name, source, stmt })
}

/// 提取模板中**引用过**的所有变量名（含模板内部用 `set`/`for` 声明的名字）。
///
/// 结果按首次出现顺序去重，排除引擎内置名（`loop`/`self`/`super`/`caller`）。
/// 每个变量的 [`Variable::optional`] 表示其全部引用是否都处于兜底上下文。
pub fn extract_variables<'a>(ast: &Ast<'a>) -> Vec<Variable> {
    let mut c = Collector::new(ast.source());
    walk_stmt(&ast.stmt, &mut c, false);
    c.finalize();
    c.all
}

/// 提取模板中引用、但**未在模板内部声明**的变量 —— 即渲染时必须由外部上下文提供。
///
/// 对标 Python `jinja2.meta.find_undeclared_variables(ast)`。结果按首次出现顺序去重。
/// 每个变量的 [`Variable::optional`]：`true` = 可选参数（全部引用均有 `default`/`defined`
/// 兜底，缺失时模板仍可渲染）；`false` = 必选参数。
pub fn extract_undeclared<'a>(ast: &Ast<'a>) -> Vec<Variable> {
    let mut c = Collector::new(ast.source());
    walk_stmt(&ast.stmt, &mut c, false);
    c.finalize();
    c.undeclared
}

// ---------------------------------------------------------------------------
// 变量提取：AST 遍历器
// ---------------------------------------------------------------------------

struct Collector<'a> {
    /// 模板内部已声明的名字（set 目标 / for 目标 / with 赋值 / macro 参数 / import 别名）
    locals: HashSet<&'a str>,
    all: Vec<Variable>,
    all_seen: HashSet<String>,
    undeclared: Vec<Variable>,
    undeclared_seen: HashSet<String>,
    /// 出现过「非兜底引用」的变量名 —— 用于最终计算 `optional`。
    required_refs: HashSet<String>,
}

impl<'a> Collector<'a> {
    fn new(_src: &'a str) -> Self {
        Collector {
            locals: HashSet::new(),
            all: Vec::new(),
            all_seen: HashSet::new(),
            undeclared: Vec::new(),
            undeclared_seen: HashSet::new(),
            required_refs: HashSet::new(),
        }
    }

    /// 按「是否出现过非兜底引用」回填所有变量的 `optional` 字段。
    fn finalize(&mut self) {
        for v in &mut self.all {
            v.optional = !self.required_refs.contains(&v.name);
        }
        for v in &mut self.undeclared {
            v.optional = !self.required_refs.contains(&v.name);
        }
    }

    /// 记录一次变量引用：进入 `all`；若未在模板内声明则进入 `undeclared`。
    ///
    /// `in_optional` 为 `true` 表示本次引用处于兜底上下文（`default` 过滤器 /
    /// `defined` 测试的操作数），此时不把该变量记为「必选」。
    fn record(&mut self, v: &Spanned<ast::Var<'a>>, in_optional: bool) {
        let name = v.id;
        if RESERVED_NAMES.contains(&name) {
            return;
        }
        let span = v.span();
        let var = Variable {
            name: name.to_string(),
            line: span.start_line as usize,
            col: span.start_col as usize,
            start: span.start_offset as usize,
            end: span.end_offset as usize,
            optional: false, // 在 finalize() 中按 required_refs 统一回填
        };
        if !in_optional {
            self.required_refs.insert(var.name.clone());
        }
        if self.all_seen.insert(var.name.clone()) {
            self.all.push(var.clone());
        }
        if !self.locals.contains(name)
            && !BUILTIN_GLOBALS.contains(&name)
            && self.undeclared_seen.insert(var.name.clone())
        {
            self.undeclared.push(var);
        }
    }
}

/// 把赋值目标（Var 或解构的 List）里的名字登记为模板局部变量。
fn declare_locals<'a>(expr: &Expr<'a>, c: &mut Collector<'a>) {
    match expr {
        Expr::Var(s) => {
            c.locals.insert(s.id);
        }
        Expr::List(s) => {
            for item in &s.items {
                declare_locals(item, c);
            }
        }
        _ => {}
    }
}

fn walk_stmt<'a>(stmt: &Stmt<'a>, c: &mut Collector<'a>, opt: bool) {
    match stmt {
        Stmt::Template(s) => {
            for child in &s.children {
                walk_stmt(child, c, opt);
            }
        }
        Stmt::EmitExpr(s) => walk_expr(&s.expr, c, opt),
        Stmt::EmitRaw(_) => {}
        Stmt::ForLoop(s) => {
            declare_locals(&s.target, c);
            walk_expr(&s.iter, c, opt);
            if let Some(f) = &s.filter_expr {
                walk_expr(f, c, opt);
            }
            c.locals.insert("loop");
            for child in &s.body {
                walk_stmt(child, c, opt);
            }
            for child in &s.else_body {
                walk_stmt(child, c, opt);
            }
        }
        Stmt::IfCond(s) => {
            walk_expr(&s.expr, c, opt);
            for child in &s.true_body {
                walk_stmt(child, c, opt);
            }
            for child in &s.false_body {
                walk_stmt(child, c, opt);
            }
        }
        Stmt::WithBlock(s) => {
            for (target, value) in &s.assignments {
                declare_locals(target, c);
                walk_expr(value, c, opt);
            }
            for child in &s.body {
                walk_stmt(child, c, opt);
            }
        }
        Stmt::Set(s) => {
            declare_locals(&s.target, c);
            walk_expr(&s.expr, c, opt);
        }
        Stmt::SetBlock(s) => {
            declare_locals(&s.target, c);
            if let Some(f) = &s.filter {
                walk_expr(f, c, opt);
            }
            for child in &s.body {
                walk_stmt(child, c, opt);
            }
        }
        Stmt::AutoEscape(s) => {
            walk_expr(&s.enabled, c, opt);
            for child in &s.body {
                walk_stmt(child, c, opt);
            }
        }
        Stmt::FilterBlock(s) => {
            walk_expr(&s.filter, c, opt);
            for child in &s.body {
                walk_stmt(child, c, opt);
            }
        }
        Stmt::Block(s) => {
            for child in &s.body {
                walk_stmt(child, c, opt);
            }
        }
        Stmt::Import(s) => {
            walk_expr(&s.expr, c, opt);
            declare_locals(&s.name, c);
        }
        Stmt::FromImport(s) => {
            walk_expr(&s.expr, c, opt);
            for (alias, orig) in &s.names {
                declare_locals(alias, c);
                if let Some(o) = orig {
                    walk_expr(o, c, opt);
                }
            }
        }
        Stmt::Extends(s) => walk_expr(&s.name, c, opt),
        Stmt::Include(s) => walk_expr(&s.name, c, opt),
        Stmt::Macro(s) => {
            // 宏名在模板内已定义，引用它不算“未声明变量”
            c.locals.insert(s.name);
            for arg in &s.args {
                declare_locals(arg, c);
            }
            for d in &s.defaults {
                walk_expr(d, c, opt);
            }
            for child in &s.body {
                walk_stmt(child, c, opt);
            }
        }
        Stmt::CallBlock(s) => {
            walk_call(&s.call, c, opt);
            for arg in &s.macro_decl.args {
                declare_locals(arg, c);
            }
            for d in &s.macro_decl.defaults {
                walk_expr(d, c, opt);
            }
            for child in &s.macro_decl.body {
                walk_stmt(child, c, opt);
            }
        }
        Stmt::Continue(_) | Stmt::Break(_) => {}
        Stmt::Do(s) => walk_call(&s.call, c, opt),
    }
}

fn walk_expr<'a>(expr: &Expr<'a>, c: &mut Collector<'a>, opt: bool) {
    match expr {
        Expr::Var(s) => c.record(s, opt),
        Expr::Const(_) => {}
        Expr::Slice(s) => {
            walk_expr(&s.expr, c, opt);
            if let Some(e) = &s.start {
                walk_expr(e, c, opt);
            }
            if let Some(e) = &s.stop {
                walk_expr(e, c, opt);
            }
            if let Some(e) = &s.step {
                walk_expr(e, c, opt);
            }
        }
        Expr::UnaryOp(s) => walk_expr(&s.expr, c, opt),
        Expr::BinOp(s) => {
            walk_expr(&s.left, c, opt);
            walk_expr(&s.right, c, opt);
        }
        Expr::Compare(s) => {
            walk_expr(&s.expr, c, opt);
            for op in &s.ops {
                walk_expr(&op.expr, c, opt);
            }
        }
        Expr::IfExpr(s) => {
            walk_expr(&s.test_expr, c, opt);
            walk_expr(&s.true_expr, c, opt);
            if let Some(f) = &s.false_expr {
                walk_expr(f, c, opt);
            }
        }
        Expr::Filter(s) => {
            // default / d：被过滤的操作数在变量缺失时由默认值兜底 → 进入兜底上下文
            let is_default = matches!(s.name, "default" | "d");
            if let Some(e) = &s.expr {
                walk_expr(e, c, opt || is_default);
            }
            // 过滤器参数（含默认值表达式）仍需正常求值 → 透传当前 opt
            for arg in &s.args {
                walk_call_arg(arg, c, opt);
            }
        }
        Expr::Test(s) => {
            // defined / undefined：被测试的表达式允许缺失 → 进入兜底上下文
            let is_defined = matches!(s.name, "defined" | "undefined");
            walk_expr(&s.expr, c, opt || is_defined);
            for arg in &s.args {
                walk_call_arg(arg, c, opt);
            }
        }
        Expr::GetAttr(s) => walk_expr(&s.expr, c, opt),
        Expr::GetItem(s) => {
            walk_expr(&s.expr, c, opt);
            walk_expr(&s.subscript_expr, c, opt);
        }
        Expr::Call(s) => walk_call(s, c, opt),
        Expr::List(s) => {
            for item in &s.items {
                walk_expr(item, c, opt);
            }
        }
        Expr::Map(s) => {
            for k in &s.keys {
                walk_expr(k, c, opt);
            }
            for v in &s.values {
                walk_expr(v, c, opt);
            }
        }
    }
}

fn walk_call<'a>(call: &Spanned<ast::Call<'a>>, c: &mut Collector<'a>, opt: bool) {
    walk_expr(&call.expr, c, opt);
    for arg in &call.args {
        walk_call_arg(arg, c, opt);
    }
}

fn walk_call_arg<'a>(arg: &ast::CallArg<'a>, c: &mut Collector<'a>, opt: bool) {
    match arg {
        ast::CallArg::Pos(e) | ast::CallArg::PosSplat(e) | ast::CallArg::KwargSplat(e) => {
            walk_expr(e, c, opt)
        }
        ast::CallArg::Kwarg(_, e) => walk_expr(e, c, opt),
    }
}

// ---------------------------------------------------------------------------
// 渲染器：minijinja Environment + 数学过滤器集
// ---------------------------------------------------------------------------

/// 渲染器。内部持有 minijinja `Environment`，并注册一组数学过滤器。
///
/// 过滤器集（全部基于 Rust 标准库 `f64`，零额外依赖）：
/// `sin` `cos` `tan` `asin` `acos` `atan` `sqrt` `exp` `ln` `log10` `pow` `floor` `ceil`
///
/// 所有数学过滤器对结果做**有限性校验**：一旦产生 `NaN`/`Inf`（如 `sqrt(-1)`、`asin(2)`、
/// `ln(0)`），渲染立即失败并报 [`TplError::Render`]，避免非法坐标静默写入 G-code。
#[derive(Debug)]
pub struct Renderer {
    env: Environment<'static>,
}

/// 数学过滤器结果校验：`NaN`/`Inf` 一律转为渲染错误，防止非法数值进入 G-code。
fn checked_math(value: f64, filter: &'static str) -> Result<f64, minijinja::Error> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(minijinja::Error::new(
            minijinja::ErrorKind::InvalidOperation,
            format!("数学过滤器 `{filter}` 输出非有限数（NaN/Inf），拒绝渲染"),
        ))
    }
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

impl Renderer {
    /// 新建渲染器（带数学过滤器集）。
    ///
    /// 默认使用 **Strict** 未定义变量策略：模板引用缺失变量时直接渲染失败并报错，
    /// 避免静默输出不完整 G-code（与 `jinja2.meta` + `StrictUndefined` 的做法一致）。
    pub fn new() -> Self {
        let mut env = Environment::new();
        env.set_undefined_behavior(minijinja::UndefinedBehavior::Strict);
        env.add_filter("sin", |v: f64| checked_math(v.sin(), "sin"));
        env.add_filter("cos", |v: f64| checked_math(v.cos(), "cos"));
        env.add_filter("tan", |v: f64| checked_math(v.tan(), "tan"));
        env.add_filter("asin", |v: f64| checked_math(v.asin(), "asin"));
        env.add_filter("acos", |v: f64| checked_math(v.acos(), "acos"));
        env.add_filter("atan", |v: f64| checked_math(v.atan(), "atan"));
        env.add_filter("sqrt", |v: f64| checked_math(v.sqrt(), "sqrt"));
        env.add_filter("exp", |v: f64| checked_math(v.exp(), "exp"));
        env.add_filter("ln", |v: f64| checked_math(v.ln(), "ln"));
        env.add_filter("log10", |v: f64| checked_math(v.log10(), "log10"));
        env.add_filter("pow", |v: f64, e: f64| checked_math(v.powf(e), "pow"));
        env.add_filter("floor", |v: f64| checked_math(v.floor(), "floor"));
        env.add_filter("ceil", |v: f64| checked_math(v.ceil(), "ceil"));
        Self { env }
    }

    /// 渲染模板。`context` 用 `minijinja::context!` 宏或 `Value::from_serialize` 构造。
    ///
    /// 此方法渲染**单段字符串**模板，不涉及模板间引用。如需 `{% include %}` /
    /// `{% extends %}` / `{% import %}`，请先用 [`add_template`](Self::add_template)
    /// 或 [`set_path_loader`](Self::set_path_loader) 注册模板，再调用
    /// [`render_template`](Self::render_template)。
    pub fn render(
        &self,
        source: &str,
        name: &str,
        context: &minijinja::Value,
    ) -> Result<String, TplError> {
        let tmpl = self
            .env
            .template_from_named_str(name, source)
            .map_err(|err| TplError::Render {
                name: name.to_string(),
                message: err.to_string(),
            })?;
        tmpl.render(context).map_err(|err| TplError::Render {
            name: name.to_string(),
            message: err.to_string(),
        })
    }

    /// 注册一个内存模板（owned 字符串，无生命周期约束）。
    ///
    /// 注册后可通过 [`render_template`](Self::render_template) 按名称渲染，
    /// 且模板内的 `{% include "name" %}` / `{% extends "name" %}` /
    /// `{% import "name" %}` 能正确解析到已注册的模板。
    pub fn add_template(
        &mut self,
        name: impl Into<String>,
        source: impl Into<String>,
    ) -> Result<(), TplError> {
        let name = name.into();
        self.env
            .add_template_owned(name.clone(), source.into())
            .map_err(|err| TplError::Render {
                name,
                message: err.to_string(),
            })
    }

    /// 从文件系统目录动态加载模板。
    ///
    /// 目录下的文件按**文件名（含扩展名）**作为模板名引用，例如
    /// `templates/sub.gcode` 可被 `{% include "sub.gcode" %}` 引用。
    /// 模板按需加载并缓存，同一名称只加载一次。
    pub fn set_path_loader(&mut self, dir: impl AsRef<Path>) {
        let dir = dir.as_ref().to_path_buf();
        self.env.set_loader(minijinja::path_loader(dir));
    }

    /// 渲染已注册或已加载的模板（支持 `include` / `extends` / `import`）。
    ///
    /// 模板需先通过 [`add_template`](Self::add_template) 注册，或通过
    /// [`set_path_loader`](Self::set_path_loader) 配置目录加载。
    pub fn render_template(
        &self,
        name: &str,
        context: &minijinja::Value,
    ) -> Result<String, TplError> {
        let tmpl = self
            .env
            .get_template(name)
            .map_err(|err| TplError::Render {
                name: name.to_string(),
                message: err.to_string(),
            })?;
        tmpl.render(context).map_err(|err| TplError::Render {
            name: name.to_string(),
            message: err.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(vars: &[Variable]) -> Vec<String> {
        vars.iter().map(|v| v.name.clone()).collect()
    }

    #[test]
    fn extract_basic() {
        let src = r#"{% set feed = 0.15 %}
O1000 ({{ program_name }})
G0 X{{ start_x }} Z{{ start_z }}
G1 X{{ diameter / 2 }} F{{ feed * 1.2 | round(2) }}
{% for hole in holes %}
  G81 X{{ hole.x }} Y{{ hole.y }}
{% endfor %}"#;
        let ast = parse(src, "demo.j2").unwrap();

        // 引用过的所有变量（feed/hole 是模板内部声明的，但仍被引用）
        let all = names(&extract_variables(&ast));
        assert!(all.contains(&"program_name".to_string()));
        assert!(all.contains(&"start_x".to_string()));
        assert!(all.contains(&"diameter".to_string()));
        assert!(all.contains(&"holes".to_string()));
        assert!(all.contains(&"feed".to_string()));
        assert!(all.contains(&"hole".to_string()));
        assert!(!all.contains(&"loop".to_string()));

        // 未声明变量 = 需外部提供的参数；feed/hole/loop 都不应出现
        let undeclared = names(&extract_undeclared(&ast));
        assert_eq!(
            undeclared,
            vec!["program_name", "start_x", "start_z", "diameter", "holes"]
        );
    }

    #[test]
    fn extract_dedup_and_order() {
        let src = "{{ a }} {{ b }} {{ a }} {{ c }}";
        let ast = parse(src, "t.j2").unwrap();
        let all = names(&extract_variables(&ast));
        assert_eq!(all, vec!["a", "b", "c"]);
    }

    #[test]
    fn extract_with_local_and_macro() {
        let src = r#"{% macro round2(x) %}{{ x * 2 }}{% endmacro %}
{{ round2(value) }}
{% set ns = namespace(foo=1) %}
{% with y = ns.foo %}{{ y }}{% endwith %}"#;
        let ast = parse(src, "t.j2").unwrap();
        let undeclared = names(&extract_undeclared(&ast));
        // 只需外部提供 value；x 是宏参数、round2 是宏名、ns/y 是 set/with 局部，
        // namespace 是内置全局、foo 是 kwarg 名字/属性名（非变量引用）
        assert_eq!(undeclared, vec!["value".to_string()]);
    }

    #[test]
    fn parse_syntax_error_line() {
        let src = "G0 X10\n{{ name \nG1 Z5";
        let err = parse(src, "bad.j2").unwrap_err();
        match err {
            TplError::Parse { line, .. } => assert!(line >= 2),
            _ => panic!("expected parse error"),
        }
    }

    #[test]
    fn parse_error_column_is_located() {
        // 未闭合括号：minijinja 报 `unexpected }`，列号应精确指向 `}`（非恒 1）
        let src = "{{ (1 + 2 }}";
        let err = parse(src, "bad.j2").unwrap_err();
        match err {
            TplError::Parse { line, col, .. } => {
                assert_eq!(line, 1);
                assert_eq!(col, 11, "应定位到 }} 所在列，实际 col={col}");
            }
            _ => panic!("expected parse error"),
        }
    }

    #[test]
    fn parse_error_column_on_multiline() {
        // 多行模板：行列均来自字节偏移换算，不应退化到 col=1
        let src = "G0 X0\nG1 Z5\n  {{ x | }}\n";
        let err = parse(src, "bad.j2").unwrap_err();
        match err {
            TplError::Parse { line, col, .. } => {
                assert_eq!(line, 3);
                assert!(col >= 6, "错误应在第 3 行 `{{ x | }}` 附近，实际 col={col}");
            }
            _ => panic!("expected parse error"),
        }
    }

    #[test]
    fn extract_undeclared_required_by_default() {
        let src = "G1 X{{ diameter }}";
        let ast = parse(src, "t.j2").unwrap();
        let v = extract_undeclared(&ast);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].name, "diameter");
        assert!(!v[0].optional, "无兜底引用的变量应为必选");
    }

    #[test]
    fn extract_undeclared_default_chain_optional() {
        // 直接 default 过滤器：变量缺失时由默认值兜底 → 可选
        let src = "G1 F{{ feed | default(0.15) }}";
        let ast = parse(src, "t.j2").unwrap();
        let v = extract_undeclared(&ast);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].name, "feed");
        assert!(v[0].optional, "default 兜底的变量应为可选");

        // 别名 d 同样生效
        let src = "G1 F{{ feed | d(0.15) }}";
        let ast = parse(src, "t.j2").unwrap();
        let v = extract_undeclared(&ast);
        assert_eq!(v[0].name, "feed");
        assert!(v[0].optional, "d 别名也应视为兜底");
    }

    #[test]
    fn extract_undeclared_set_with_default_optional() {
        // README 示例：default_feed 有 default 兜底 → 可选；feed 为模板局部
        let src = "{% set feed = default_feed | default(0.15) %}G1 F{{ feed }}";
        let ast = parse(src, "t.j2").unwrap();
        let v = extract_undeclared(&ast);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].name, "default_feed");
        assert!(v[0].optional, "default_feed 应有 default 兜底 → 可选");
    }

    #[test]
    fn extract_undeclared_mixed_reference_is_required() {
        // 同一变量既出现在兜底上下文、又出现在必选上下文 → 整体视为必选
        let src = "G1 F{{ feed | default(0.15) }} X{{ feed }}";
        let ast = parse(src, "t.j2").unwrap();
        let v = extract_undeclared(&ast);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].name, "feed");
        assert!(!v[0].optional, "存在非兜底引用时仍应视为必选");
    }

    #[test]
    fn extract_undeclared_defined_test_optional() {
        // defined 测试：被检查变量缺失时模板仍可安全执行 → 可选
        let src = "{% if radius is defined %}{{ 'ok' }}{% else %}{{ 'missing' }}{% endif %}";
        let ast = parse(src, "t.j2").unwrap();
        let v = extract_undeclared(&ast);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].name, "radius");
        assert!(v[0].optional, "defined 测试保护的变量应为可选");
    }

    #[test]
    fn extract_variables_carries_optional() {
        // extract_variables 同样携带 optional（表示该变量所有引用是否均在兜底上下文）
        let src = "{{ a }}{{ b | default(1) }}";
        let ast = parse(src, "t.j2").unwrap();
        let all = extract_variables(&ast);
        let a = all.iter().find(|v| v.name == "a").unwrap();
        let b = all.iter().find(|v| v.name == "b").unwrap();
        assert!(!a.optional);
        assert!(b.optional);
    }

    // -----------------------------------------------------------------------
    // 多模板渲染（include / extends / import）
    // -----------------------------------------------------------------------

    #[test]
    fn render_template_include() {
        let mut r = Renderer::new();
        r.add_template("sub.j2", "X{{ x }}").unwrap();
        r.add_template("main.j2", "G0 {% include \"sub.j2\" %} Z{{ z }}")
            .unwrap();
        let ctx = minijinja::context! { x => 1.0, z => 2.0 };
        let out = r.render_template("main.j2", &ctx).unwrap();
        assert_eq!(out, "G0 X1.0 Z2.0");
    }

    #[test]
    fn render_template_extends() {
        let mut r = Renderer::new();
        r.add_template(
            "base.j2",
            "HEAD {% block content %}default{% endblock %} TAIL",
        )
        .unwrap();
        r.add_template(
            "child.j2",
            "{% extends \"base.j2\" %}{% block content %}GCODE{% endblock %}",
        )
        .unwrap();
        let ctx = minijinja::context! {};
        let out = r.render_template("child.j2", &ctx).unwrap();
        assert_eq!(out, "HEAD GCODE TAIL");
    }

    #[test]
    fn render_template_import_macro() {
        let mut r = Renderer::new();
        r.add_template("macros.j2", "{% macro greet(n) %}Hi {{ n }}{% endmacro %}")
            .unwrap();
        r.add_template(
            "main.j2",
            "{% from \"macros.j2\" import greet %}{{ greet(\"world\") }}",
        )
        .unwrap();
        let ctx = minijinja::context! {};
        let out = r.render_template("main.j2", &ctx).unwrap();
        assert_eq!(out, "Hi world");
    }

    #[test]
    fn render_template_not_found_errors() {
        let r = Renderer::new();
        let ctx = minijinja::context! {};
        let err = r.render_template("missing.j2", &ctx).unwrap_err();
        match err {
            TplError::Render { message, .. } => {
                assert!(
                    message.contains("template") || message.contains("not found"),
                    "应报模板未找到错误: {message}"
                );
            }
            _ => panic!("应为渲染错误"),
        }
    }

    #[test]
    fn add_template_syntax_error() {
        let mut r = Renderer::new();
        let err = r.add_template("bad.j2", "{{ oops ").unwrap_err();
        match err {
            TplError::Render { message, .. } => assert!(message.contains("syntax")),
            _ => panic!("应为渲染错误（注册时语法检查失败）"),
        }
    }

    #[test]
    fn render_single_string_still_works() {
        // 向后兼容：无模板注册时，render() 单字符串渲染不受影响
        let r = Renderer::new();
        let ctx = minijinja::context! { x => 42.0 };
        let out = r.render("X{{ x }}", "s.j2", &ctx).unwrap();
        assert_eq!(out, "X42.0");
    }

    #[test]
    fn render_with_math_filters() {
        // 注意 Jinja 过滤器优先级高于算术：必须用括号把整体括起来再取整
        let src = "G1 X{{ (diameter / 2) | round(2) }} F{{ feed }} S{{ (2000 * 1.5) | ceil }}";
        let renderer = Renderer::new();
        let ctx = minijinja::context! { diameter => 42.0, feed => 0.15 };
        let out = renderer.render(src, "gcode.j2", &ctx).unwrap();
        assert!(out.contains("X21.0"));
        assert!(out.contains("F0.15"));
        assert!(out.contains("S3000"));
    }

    #[test]
    fn render_error_on_undefined() {
        // Strict 模式下，缺失变量必须报错而不是静默输出空值
        let src = "G1 X{{ missing_var }}";
        let renderer = Renderer::new();
        let ctx = minijinja::context! {};
        let err = renderer.render(src, "gcode.j2", &ctx).unwrap_err();
        match err {
            TplError::Render { message, .. } => {
                assert!(
                    message.contains("undefined"),
                    "Strict 模式应报未定义值错误: {message}"
                )
            }
            _ => panic!("expected render error"),
        }
    }
}
