//! # nctool-tpl —— NCtool 模板解析核心
//!
//! 基于 [minijinja]（Jinja2 的 Rust 实现，近乎零依赖、渲染最快）提供四个核心能力：
//!
//! 1. [`parse`]：语法检查 + 生成 AST（带行列定位）
//! 2. [`extract_variables`]：提取模板中**引用过**的所有变量名（含模板内部声明的）
//! 3. [`extract_undeclared`]：提取模板中引用、但**未在模板内部声明**的变量
//!    （即运行时要由外部上下文提供的参数）—— 对标 Python `jinja2.meta.find_undeclared_variables`
//! 4. [`Renderer`]：用上下文渲染出最终文本（G-code），内置数学过滤器集
//!
//! # 示例
//!
//! ```
//! use nctool_tpl::{parse, extract_undeclared};
//!
//! let source = r#"{% set feed = 0.15 %}G1 X{{ diameter / 2 }} F{{ feed }}"#;
//! let ast = parse(source, "demo.j2").unwrap();
//!
//! let undeclared: Vec<String> = extract_undeclared(&ast)
//!     .into_iter()
//!     .map(|v| v.name)
//!     .collect();
//! assert_eq!(undeclared, vec!["diameter".to_string()]);
//! ```

use std::collections::HashSet;
use std::fmt;

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
}

/// 解析结果：持有模板 AST，同时保留源码与文件名引用。
///
/// 变量提取只读这个结构；渲染可复用同源文本（minijinja 内部自带 JIT 编译缓存）。
#[derive(Debug)]
pub struct Ast<'a> {
    /// 模板名（用于错误信息）
    pub name: &'a str,
    /// 模板源码
    pub source: &'a str,
    pub(crate) stmt: Stmt<'a>,
}

/// 模板解析/渲染错误。
#[derive(Debug)]
pub enum TplError {
    /// 语法错误，带模板名与行号。
    ///
    /// 注意：`col` 字段为占位值（恒为 `1`）——minijinja 的错误对象仅暴露
    /// 行号（[`minijinja::Error::line`]），未暴露列号；列号细节一般包含在
    /// `message` 的文本描述中，请勿将 `col` 当作准确的列定位使用。
    Parse {
        name: String,
        message: String,
        line: usize,
        /// 占位列号（当前恒为 1，见枚举文档说明）
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

/// 语法检查并生成 AST。
///
/// 解析失败时返回带行列定位的 [`TplError::Parse`]。
pub fn parse<'a>(source: &'a str, name: &'a str) -> Result<Ast<'a>, TplError> {
    let stmt = minijinja::machinery::parse(source, name, SyntaxConfig, WhitespaceConfig::default())
        .map_err(|err| {
            let line = err.line().unwrap_or(1);
            TplError::Parse {
                name: name.to_string(),
                message: err.to_string(),
                line,
                col: 1,
            }
        })?;
    Ok(Ast { name, source, stmt })
}

/// 提取模板中**引用过**的所有变量名（含模板内部用 `set`/`for` 声明的名字）。
///
/// 结果按首次出现顺序去重，排除引擎内置名（`loop`/`self`/`super`/`caller`）。
pub fn extract_variables<'a>(ast: &Ast<'a>) -> Vec<Variable> {
    let mut c = Collector::new(ast.source);
    walk_stmt(&ast.stmt, &mut c);
    c.all
}

/// 提取模板中引用、但**未在模板内部声明**的变量 —— 即渲染时必须由外部上下文提供。
///
/// 对标 Python `jinja2.meta.find_undeclared_variables(ast)`。结果按首次出现顺序去重。
pub fn extract_undeclared<'a>(ast: &Ast<'a>) -> Vec<Variable> {
    let mut c = Collector::new(ast.source);
    walk_stmt(&ast.stmt, &mut c);
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
}

impl<'a> Collector<'a> {
    fn new(_src: &'a str) -> Self {
        Collector {
            locals: HashSet::new(),
            all: Vec::new(),
            all_seen: HashSet::new(),
            undeclared: Vec::new(),
            undeclared_seen: HashSet::new(),
        }
    }

    /// 记录一次变量引用：进入 `all`；若未在模板内声明则进入 `undeclared`。
    fn record(&mut self, v: &Spanned<ast::Var<'a>>) {
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
        };
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

fn walk_stmt<'a>(stmt: &Stmt<'a>, c: &mut Collector<'a>) {
    match stmt {
        Stmt::Template(s) => {
            for child in &s.children {
                walk_stmt(child, c);
            }
        }
        Stmt::EmitExpr(s) => walk_expr(&s.expr, c),
        Stmt::EmitRaw(_) => {}
        Stmt::ForLoop(s) => {
            declare_locals(&s.target, c);
            walk_expr(&s.iter, c);
            if let Some(f) = &s.filter_expr {
                walk_expr(f, c);
            }
            c.locals.insert("loop");
            for child in &s.body {
                walk_stmt(child, c);
            }
            for child in &s.else_body {
                walk_stmt(child, c);
            }
        }
        Stmt::IfCond(s) => {
            walk_expr(&s.expr, c);
            for child in &s.true_body {
                walk_stmt(child, c);
            }
            for child in &s.false_body {
                walk_stmt(child, c);
            }
        }
        Stmt::WithBlock(s) => {
            for (target, value) in &s.assignments {
                declare_locals(target, c);
                walk_expr(value, c);
            }
            for child in &s.body {
                walk_stmt(child, c);
            }
        }
        Stmt::Set(s) => {
            declare_locals(&s.target, c);
            walk_expr(&s.expr, c);
        }
        Stmt::SetBlock(s) => {
            declare_locals(&s.target, c);
            if let Some(f) = &s.filter {
                walk_expr(f, c);
            }
            for child in &s.body {
                walk_stmt(child, c);
            }
        }
        Stmt::AutoEscape(s) => {
            walk_expr(&s.enabled, c);
            for child in &s.body {
                walk_stmt(child, c);
            }
        }
        Stmt::FilterBlock(s) => {
            walk_expr(&s.filter, c);
            for child in &s.body {
                walk_stmt(child, c);
            }
        }
        Stmt::Block(s) => {
            for child in &s.body {
                walk_stmt(child, c);
            }
        }
        Stmt::Import(s) => {
            walk_expr(&s.expr, c);
            declare_locals(&s.name, c);
        }
        Stmt::FromImport(s) => {
            walk_expr(&s.expr, c);
            for (alias, orig) in &s.names {
                declare_locals(alias, c);
                if let Some(o) = orig {
                    walk_expr(o, c);
                }
            }
        }
        Stmt::Extends(s) => walk_expr(&s.name, c),
        Stmt::Include(s) => walk_expr(&s.name, c),
        Stmt::Macro(s) => {
            // 宏名在模板内已定义，引用它不算“未声明变量”
            c.locals.insert(s.name);
            for arg in &s.args {
                declare_locals(arg, c);
            }
            for d in &s.defaults {
                walk_expr(d, c);
            }
            for child in &s.body {
                walk_stmt(child, c);
            }
        }
        Stmt::CallBlock(s) => {
            walk_call(&s.call, c);
            for arg in &s.macro_decl.args {
                declare_locals(arg, c);
            }
            for d in &s.macro_decl.defaults {
                walk_expr(d, c);
            }
            for child in &s.macro_decl.body {
                walk_stmt(child, c);
            }
        }
        Stmt::Continue(_) | Stmt::Break(_) => {}
        Stmt::Do(s) => walk_call(&s.call, c),
    }
}

fn walk_expr<'a>(expr: &Expr<'a>, c: &mut Collector<'a>) {
    match expr {
        Expr::Var(s) => c.record(s),
        Expr::Const(_) => {}
        Expr::Slice(s) => {
            walk_expr(&s.expr, c);
            if let Some(e) = &s.start {
                walk_expr(e, c);
            }
            if let Some(e) = &s.stop {
                walk_expr(e, c);
            }
            if let Some(e) = &s.step {
                walk_expr(e, c);
            }
        }
        Expr::UnaryOp(s) => walk_expr(&s.expr, c),
        Expr::BinOp(s) => {
            walk_expr(&s.left, c);
            walk_expr(&s.right, c);
        }
        Expr::Compare(s) => {
            walk_expr(&s.expr, c);
            for op in &s.ops {
                walk_expr(&op.expr, c);
            }
        }
        Expr::IfExpr(s) => {
            walk_expr(&s.test_expr, c);
            walk_expr(&s.true_expr, c);
            if let Some(f) = &s.false_expr {
                walk_expr(f, c);
            }
        }
        Expr::Filter(s) => {
            if let Some(e) = &s.expr {
                walk_expr(e, c);
            }
            for arg in &s.args {
                walk_call_arg(arg, c);
            }
        }
        Expr::Test(s) => {
            walk_expr(&s.expr, c);
            for arg in &s.args {
                walk_call_arg(arg, c);
            }
        }
        Expr::GetAttr(s) => walk_expr(&s.expr, c),
        Expr::GetItem(s) => {
            walk_expr(&s.expr, c);
            walk_expr(&s.subscript_expr, c);
        }
        Expr::Call(s) => walk_call(s, c),
        Expr::List(s) => {
            for item in &s.items {
                walk_expr(item, c);
            }
        }
        Expr::Map(s) => {
            for k in &s.keys {
                walk_expr(k, c);
            }
            for v in &s.values {
                walk_expr(v, c);
            }
        }
    }
}

fn walk_call<'a>(call: &Spanned<ast::Call<'a>>, c: &mut Collector<'a>) {
    walk_expr(&call.expr, c);
    for arg in &call.args {
        walk_call_arg(arg, c);
    }
}

fn walk_call_arg<'a>(arg: &ast::CallArg<'a>, c: &mut Collector<'a>) {
    match arg {
        ast::CallArg::Pos(e) | ast::CallArg::PosSplat(e) | ast::CallArg::KwargSplat(e) => {
            walk_expr(e, c)
        }
        ast::CallArg::Kwarg(_, e) => walk_expr(e, c),
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
