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

#![warn(missing_docs)]

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
/// `#[non_exhaustive]`：未来可能新增错误变体，外部 match 应保留通配分支。
///
/// 细分变体让上层可精准处理：例如 `UndefinedVariable` 可触发"参数缺失"提示，
/// `TemplateNotFound` 可触发模板路径检查，而不必解析 message 字符串。
#[non_exhaustive]
#[derive(Debug)]
pub enum TplError {
    /// 语法错误，带模板名与行列号。
    ///
    /// `col` 为 minijinja 停止解析位置的最佳近似（来自其错误携带的字节范围），
    /// 在无法取得字节范围时回退为 `1`。
    Parse {
        /// 触发错误的模板名
        name: String,
        /// 完整错误信息（含 minijinja 原始描述）
        message: String,
        /// 错误所在行（1 起始）
        line: usize,
        /// 错误所在列（1 起始，最佳近似）
        col: usize,
    },
    /// 模板未找到（`{% include %}` / `{% extends %}` / `get_template` 引用了不存在的模板）。
    TemplateNotFound {
        /// 触发错误的模板名
        name: String,
        /// 被引用但不存在的模板名（从 minijinja 错误详情中提取，可能为空）。
        template: String,
        /// 完整错误信息
        message: String,
    },
    /// 未定义变量（严格模式下引用了不存在的变量）。
    UndefinedVariable {
        /// 触发错误的模板名
        name: String,
        /// 变量名（尽力从源码错误位置恢复；无法定位时为空）。
        variable: String,
        /// 完整错误信息
        message: String,
    },
    /// 未知过滤器（模板使用了未注册的过滤器）。
    UnknownFilter {
        /// 触发错误的模板名
        name: String,
        /// 过滤器名（从错误详情中提取，可能为空）。
        filter: String,
        /// 完整错误信息
        message: String,
    },
    /// 未知测试（模板使用了未注册的测试）。
    UnknownTest {
        /// 触发错误的模板名
        name: String,
        /// 测试名（从错误详情中提取，可能为空）。
        test: String,
        /// 完整错误信息
        message: String,
    },
    /// 其他渲染错误（无效操作、参数错误、序列化失败等兜底）。
    Render {
        /// 触发错误的模板名
        name: String,
        /// 完整错误信息
        message: String,
    },
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
            TplError::TemplateNotFound {
                name,
                template,
                message,
            } => {
                if template.is_empty() {
                    write!(f, "{name}: 模板未找到: {message}")
                } else {
                    write!(f, "{name}: 模板未找到 '{template}': {message}")
                }
            }
            TplError::UndefinedVariable {
                name,
                variable,
                message,
            } => {
                if variable.is_empty() {
                    write!(f, "{name}: 未定义变量: {message}")
                } else {
                    write!(f, "{name}: 未定义变量 '{variable}': {message}")
                }
            }
            TplError::UnknownFilter {
                name,
                filter,
                message,
            } => {
                if filter.is_empty() {
                    write!(f, "{name}: 未知过滤器: {message}")
                } else {
                    write!(f, "{name}: 未知过滤器 '{filter}': {message}")
                }
            }
            TplError::UnknownTest {
                name,
                test,
                message,
            } => {
                if test.is_empty() {
                    write!(f, "{name}: 未知测试: {message}")
                } else {
                    write!(f, "{name}: 未知测试 '{test}': {message}")
                }
            }
            TplError::Render { name, message } => {
                write!(f, "{name}: 渲染错误: {message}")
            }
        }
    }
}

impl std::error::Error for TplError {}

/// 从错误详情字符串中提取第一个引号（单引号或双引号）内的内容。
///
/// minijinja 的错误详情通常形如 `unknown filter 'foo'` 或
/// `variable 'x' is undefined`，此函数提取其中的名称。
fn extract_quoted(s: &str) -> Option<String> {
    let start = s.find('\'').or_else(|| s.find('"'))?;
    let quote = s.as_bytes()[start];
    let rest = &s[start + 1..];
    let end = rest.find(quote as char)?;
    Some(rest[..end].to_string())
}

/// 从 `"prefix name rest"` 格式的详情中提取 `name`（第一个空白分隔的词）。
///
/// 用于 minijinja 的 `"filter badfilter is unknown"` / `"test badtest is unknown"`
/// 这类无引号格式。
fn extract_after_prefix<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    s.strip_prefix(prefix)?.split_whitespace().next()
}

/// 从源码 `offset` 处尽力提取一个标识符（变量名）。
///
/// 用于从 minijinja 运行时错误的字节范围中恢复未定义变量名：
/// debug feature 下错误携带字节范围，通常指向出错的表达式起点
/// （如 `{{ missing }}` 的 `missing`）。无法定位或该处不是标识符时返回 `None`。
fn extract_identifier_at(source: &str, offset: usize) -> Option<String> {
    // 对齐到 UTF-8 字符边界（错误范围可能落在多字节字符中间）
    let mut off = offset.min(source.len());
    while off > 0 && !source.is_char_boundary(off) {
        off -= 1;
    }
    let rest = &source[off..];
    let mut chars = rest.chars();
    let first = chars.next()?;
    if !(first == '_' || first.is_alphabetic()) {
        return None;
    }
    let len = first.len_utf8()
        + chars
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .map(char::len_utf8)
            .sum::<usize>();
    Some(rest[..len].to_string())
}

/// 从源码与错误字节范围中尽力恢复未定义变量的名字。
///
/// minijinja 的 `UndefinedError` 不携带变量名，但 debug feature 下错误携带
/// 字节范围。范围通常覆盖整个出错的表达式（如 `x.missing_attr` 整条属性链），
/// 仅当范围起点恰好是一个**裸标识符**（后面不紧跟 `.` / `[` 等访问符）时才
/// 认定其为缺失变量名 —— 属性链中无法确定缺失的是基础名还是某个属性，
/// 此时返回 `None`（宁缺毋错，避免误导）。
fn extract_undefined_var_name(source: &str, range: std::ops::Range<usize>) -> Option<String> {
    let rest = source.get(range.start..)?;
    let id = extract_identifier_at(rest, 0)?;
    let after = rest[id.len()..].trim_start();
    if after.starts_with('.') || after.starts_with('[') {
        return None;
    }
    Some(id)
}

/// 将 minijinja 错误转换为细分的 [`TplError`]。
///
/// `fallback_name`：当 minijinja 错误未携带模板名时使用的名称。
/// `source`：模板源码，用于语法错误的列号换算（可为 None，此时 col 回退为 1）。
fn from_minijinja_error(
    err: minijinja::Error,
    fallback_name: &str,
    source: Option<&str>,
) -> TplError {
    use minijinja::ErrorKind;
    let name = err.name().unwrap_or(fallback_name).to_string();
    let message = err.to_string();
    let detail = err.detail().unwrap_or("");

    match err.kind() {
        ErrorKind::SyntaxError => {
            let (line, col) = err
                .range()
                .and_then(|range| source.map(|s| line_col_at(s, range.start)))
                .unwrap_or_else(|| (err.line().unwrap_or(1), 1));
            TplError::Parse {
                name,
                message,
                line,
                col,
            }
        }
        ErrorKind::TemplateNotFound => TplError::TemplateNotFound {
            name,
            template: extract_quoted(detail).unwrap_or_default(),
            message,
        },
        ErrorKind::UndefinedError => TplError::UndefinedVariable {
            name,
            // minijinja 的 UndefinedError 不直接携带变量名；debug feature 下
            // 错误携带字节范围（指向出错表达式），尽力从源码恢复变量名。
            variable: source
                .and_then(|src| {
                    err.range()
                        .and_then(|range| extract_undefined_var_name(src, range))
                })
                .unwrap_or_default(),
            message,
        },
        ErrorKind::UnknownFilter => TplError::UnknownFilter {
            name,
            filter: extract_quoted(detail)
                .or_else(|| extract_after_prefix(detail, "filter ").map(str::to_string))
                .unwrap_or_default(),
            message,
        },
        ErrorKind::UnknownTest => TplError::UnknownTest {
            name,
            test: extract_quoted(detail)
                .or_else(|| extract_after_prefix(detail, "test ").map(str::to_string))
                .unwrap_or_default(),
            message,
        },
        _ => TplError::Render { name, message },
    }
}

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
    /// 作用域栈：`scopes[0]` 为模板顶层，`for`/`with`/`macro` 各推入独立作用域
    /// （与 Jinja2 语义一致：`if`/`block` 不创建作用域）。
    /// 每层存放该作用域内声明的名字（set 目标 / for 目标 / with 赋值 / macro 参数 / import 别名）。
    scopes: Vec<HashSet<&'a str>>,
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
            scopes: vec![HashSet::new()],
            all: Vec::new(),
            all_seen: HashSet::new(),
            undeclared: Vec::new(),
            undeclared_seen: HashSet::new(),
            required_refs: HashSet::new(),
        }
    }

    /// 推入新作用域（for 循环体 / with 块 / macro 体）。
    fn push_scope(&mut self) {
        self.scopes.push(HashSet::new());
    }

    /// 弹出当前作用域。
    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    /// 在当前（栈顶）作用域声明一个名字。
    fn declare(&mut self, name: &'a str) {
        self.scopes
            .last_mut()
            .expect("作用域栈不应为空")
            .insert(name);
    }

    /// 名字是否在任意可见作用域中已声明。
    fn is_local(&self, name: &str) -> bool {
        self.scopes.iter().any(|scope| scope.contains(name))
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
        if !self.is_local(name)
            && !BUILTIN_GLOBALS.contains(&name)
            && self.undeclared_seen.insert(var.name.clone())
        {
            self.undeclared.push(var);
        }
    }
}

/// 把赋值目标（Var 或解构的 List）里的名字登记为当前作用域的模板局部变量。
fn declare_locals<'a>(expr: &Expr<'a>, c: &mut Collector<'a>) {
    match expr {
        Expr::Var(s) => {
            c.declare(s.id);
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
            // 迭代表达式在外层作用域求值（循环变量此时还不存在）
            walk_expr(&s.iter, c, opt);
            c.push_scope();
            declare_locals(&s.target, c);
            c.declare("loop");
            // 过滤表达式与循环体可引用循环变量（Jinja2 语义）
            if let Some(f) = &s.filter_expr {
                walk_expr(f, c, opt);
            }
            for child in &s.body {
                walk_stmt(child, c, opt);
            }
            c.pop_scope();
            // else 体在循环变量不可见的外层作用域执行
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
            // 赋值表达式先在外层作用域求值（与 Jinja2 语义一致，
            // 避免 {% with y = y + 1 %} 把右侧 y 误当作新局部）
            for (_target, value) in &s.assignments {
                walk_expr(value, c, opt);
            }
            c.push_scope();
            for (target, _) in &s.assignments {
                declare_locals(target, c);
            }
            for child in &s.body {
                walk_stmt(child, c, opt);
            }
            c.pop_scope();
        }
        Stmt::Set(s) => {
            // RHS 先在外层作用域求值，再声明目标：
            // {% set total = total + x %} 中右侧 total 引用的是外层/上下文值，
            // 若先声明会把必选变量 total 误判为模板局部，导致校验漏报。
            walk_expr(&s.expr, c, opt);
            declare_locals(&s.target, c);
        }
        Stmt::SetBlock(s) => {
            // 块体与过滤表达式先在外层作用域求值（块体中对目标名的引用
            // 指向外层同名变量），求值完毕后再绑定目标。
            if let Some(f) = &s.filter {
                walk_expr(f, c, opt);
            }
            for child in &s.body {
                walk_stmt(child, c, opt);
            }
            declare_locals(&s.target, c);
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
            // 宏名在外层作用域定义，引用它不算“未声明变量”
            c.declare(s.name);
            c.push_scope();
            for arg in &s.args {
                declare_locals(arg, c);
            }
            // 默认值在宏作用域内求值（调用时绑定，可引用更早声明的参数名）
            for d in &s.defaults {
                walk_expr(d, c, opt);
            }
            for child in &s.body {
                walk_stmt(child, c, opt);
            }
            c.pop_scope();
        }
        Stmt::CallBlock(s) => {
            walk_call(&s.call, c, opt);
            c.push_scope();
            for arg in &s.macro_decl.args {
                declare_locals(arg, c);
            }
            for d in &s.macro_decl.defaults {
                walk_expr(d, c, opt);
            }
            for child in &s.macro_decl.body {
                walk_stmt(child, c, opt);
            }
            c.pop_scope();
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

/// 渲染器。内部持有 minijinja `Environment`，并注册一组数学过滤器和 NC 数值格式化过滤器。
///
/// 数学过滤器集（全部基于 Rust 标准库 `f64`，零额外依赖）：
/// `sin` `cos` `tan` `asin` `acos` `atan` `sqrt` `exp` `ln` `log10` `pow` `floor` `ceil`
///
/// NC 数值格式化过滤器（G-code 专用）：
/// - `nc_fixed(N)`：固定小数位，`{{ x | nc_fixed(3) }}` → `21.000`
/// - `nc_strip`：去尾零，`{{ x | nc_strip }}` → `21`（输入 21.0）
/// - `nc_pad(N)`：前导零填充，`{{ n | nc_pad(4) }}` → `0001`（程序号/行号用）
///
/// 所有数学过滤器和 NC 过滤器对结果做**有限性校验**：一旦产生 `NaN`/`Inf`（如 `sqrt(-1)`、
/// `asin(2)`、`ln(0)`），渲染立即失败并报 [`TplError::Render`]，避免非法坐标静默写入 G-code。
#[derive(Debug)]
pub struct Renderer {
    env: Environment<'static>,
    /// 宽松模式下未定义变量渲染为空字符串（而非报错）。
    lenient: bool,
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

// ---------------------------------------------------------------------------
// NC 数值格式化过滤器（G-code 专用）
// ---------------------------------------------------------------------------

/// 固定小数位：`{{ x | nc_fixed(3) }}` → `21.000`。
///
/// 用于需要固定精度的坐标值（如 `X21.000 Y15.500`）。非有限数（NaN/Inf）报错。
fn filter_nc_fixed(value: f64, decimals: usize) -> Result<String, minijinja::Error> {
    if !value.is_finite() {
        return Err(minijinja::Error::new(
            minijinja::ErrorKind::InvalidOperation,
            "nc_fixed: 输入非有限数（NaN/Inf）",
        ));
    }
    Ok(format!("{:.*}", decimals, value))
}

/// 去尾零：`{{ x | nc_strip }}` → `21`（输入 21.0）或 `21.5`（输入 21.50）。
///
/// 用于不需要固定精度的数值，避免输出 `X21.0` 而期望 `X21`。非有限数报错。
fn filter_nc_strip(value: f64) -> Result<String, minijinja::Error> {
    if !value.is_finite() {
        return Err(minijinja::Error::new(
            minijinja::ErrorKind::InvalidOperation,
            "nc_strip: 输入非有限数（NaN/Inf）",
        ));
    }
    // Rust f64 Display 已自动去尾零：21.0 → "21"，21.50 → "21.5"
    Ok(format!("{}", value))
}

/// 前导零填充：`{{ n | nc_pad(4) }}` → `0001`（输入 1）。
///
/// 用于程序号（`O0001`）、行号（`N0010`）等需要固定宽度的**非负**整数。
/// 输入为浮点数时截断小数部分取整。负数或非有限数报错
/// （负数会拼出 `O-001` 这类非法 G-code）。
fn filter_nc_pad(value: f64, width: usize) -> Result<String, minijinja::Error> {
    if !value.is_finite() {
        return Err(minijinja::Error::new(
            minijinja::ErrorKind::InvalidOperation,
            "nc_pad: 输入非有限数（NaN/Inf）",
        ));
    }
    if value < 0.0 {
        return Err(minijinja::Error::new(
            minijinja::ErrorKind::InvalidOperation,
            "nc_pad: 输入为负数（程序号/行号不可为负）",
        ));
    }
    if width == 0 {
        return Err(minijinja::Error::new(
            minijinja::ErrorKind::InvalidOperation,
            "nc_pad: 宽度不能为 0",
        ));
    }
    let int_val = value.trunc() as i64;
    Ok(format!("{:0>width$}", int_val, width = width))
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
        // NC 数值格式化过滤器（G-code 专用）
        env.add_filter("nc_fixed", filter_nc_fixed);
        env.add_filter("nc_strip", filter_nc_strip);
        env.add_filter("nc_pad", filter_nc_pad);
        Self {
            env,
            lenient: false,
        }
    }

    /// 切换为**宽松模式**：模板中未定义变量渲染为空字符串，而非报错。
    ///
    /// 消费式 builder 方法，便于链式构造：`Renderer::new().with_lenient()`。
    /// 默认构造已是严格模式，此方法用于需要宽松渲染的场景（如先渲染、后由
    /// [`extract_undeclared`] 校验必选参数的流程）。
    pub fn with_lenient(mut self) -> Self {
        self.env
            .set_undefined_behavior(minijinja::UndefinedBehavior::Lenient);
        self.lenient = true;
        self
    }

    /// 显式切换为**严格模式**（默认）：模板中未定义变量渲染报错。
    ///
    /// 消费式 builder 方法，便于链式构造：`Renderer::new().with_strict()`。
    pub fn with_strict(mut self) -> Self {
        self.env
            .set_undefined_behavior(minijinja::UndefinedBehavior::Strict);
        self.lenient = false;
        self
    }

    /// 当前是否为宽松模式（未定义变量渲染为空而非报错）。
    pub fn is_lenient(&self) -> bool {
        self.lenient
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
            .map_err(|err| from_minijinja_error(err, name, Some(source)))?;
        tmpl.render(context)
            .map_err(|err| from_minijinja_error(err, name, Some(source)))
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
        let source = source.into();
        self.env
            .add_template_owned(name.clone(), source.clone())
            .map_err(|err| from_minijinja_error(err, &name, Some(&source)))
    }

    /// 从文件系统目录动态加载模板。
    ///
    /// 目录下的文件按**文件名（含扩展名）**作为模板名引用，例如
    /// `templates/sub.gcode` 可被 `{% include "sub.gcode" %}` 引用。
    /// 模板按需加载并缓存，同一名称只加载一次。
    ///
    /// # 安全性
    ///
    /// 模板内容视为**可信输入**：模板名直接与目录拼接解析路径，
    /// `{% include "../x" %}` 这类相对路径可以加载目录之外的文件。
    /// 请勿将不受信任来源的模板交给本加载器。
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
            .map_err(|err| from_minijinja_error(err, name, None))?;
        tmpl.render(context)
            .map_err(|err| from_minijinja_error(err, name, Some(tmpl.source())))
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
            TplError::TemplateNotFound { name, .. } => {
                assert_eq!(name, "missing.j2");
            }
            _ => panic!("应为 TemplateNotFound 错误"),
        }
    }

    #[test]
    fn add_template_syntax_error() {
        let mut r = Renderer::new();
        let err = r.add_template("bad.j2", "{{ oops ").unwrap_err();
        match err {
            TplError::Parse { message, .. } => assert!(message.contains("syntax")),
            _ => panic!("应为 Parse 错误（注册时语法检查失败）"),
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

    // -----------------------------------------------------------------------
    // 错误细分验证
    // -----------------------------------------------------------------------

    #[test]
    fn error_unknown_filter_is_subdivided() {
        let r = Renderer::new();
        let ctx = minijinja::context! { x => 1.0 };
        let err = r
            .render("{{ x | nonexistent_filter }}", "f.j2", &ctx)
            .unwrap_err();
        match err {
            TplError::UnknownFilter { filter, .. } => {
                assert_eq!(filter, "nonexistent_filter");
            }
            _ => panic!("应为 UnknownFilter，实际: {err:?}"),
        }
    }

    #[test]
    fn error_unknown_test_is_subdivided() {
        let r = Renderer::new();
        let ctx = minijinja::context! { x => 1.0 };
        let err = r
            .render("{% if x is nonexistent_test %}yes{% endif %}", "t.j2", &ctx)
            .unwrap_err();
        match err {
            TplError::UnknownTest { test, .. } => {
                assert_eq!(test, "nonexistent_test");
            }
            _ => panic!("应为 UnknownTest，实际: {err:?}"),
        }
    }

    #[test]
    fn error_undefined_variable_is_subdivided() {
        let r = Renderer::new();
        let ctx = minijinja::context! {};
        let err = r.render("{{ missing }}", "u.j2", &ctx).unwrap_err();
        match err {
            // 变量名从源码错误位置尽力恢复
            TplError::UndefinedVariable { variable, .. } => {
                assert_eq!(variable, "missing", "应恢复出变量名");
            }
            _ => panic!("应为 UndefinedVariable，实际: {err:?}"),
        }
    }

    #[test]
    fn undefined_variable_attr_chain_leaves_empty() {
        // 属性链缺失时无法确定缺失的是基础名还是属性 → variable 为空（宁缺毋错）
        let r = Renderer::new();
        let ctx = minijinja::context! { x => 42.0 };
        let err = r.render("{{ x.missing_attr }}", "a.j2", &ctx).unwrap_err();
        match err {
            TplError::UndefinedVariable { variable, .. } => {
                assert!(variable.is_empty(), "属性链场景不应给出误导性名字");
            }
            _ => panic!("应为 UndefinedVariable，实际: {err:?}"),
        }
    }

    #[test]
    fn undefined_variable_recovered_in_registered_template() {
        // render_template 路径（env 内模板）同样恢复变量名
        let mut r = Renderer::new();
        r.add_template("t.j2", "V={{ missing2 }}").unwrap();
        let ctx = minijinja::context! {};
        let err = r.render_template("t.j2", &ctx).unwrap_err();
        match err {
            TplError::UndefinedVariable { variable, .. } => assert_eq!(variable, "missing2"),
            _ => panic!("应为 UndefinedVariable，实际: {err:?}"),
        }
    }

    #[test]
    fn extract_identifier_and_var_name_work() {
        assert_eq!(
            extract_identifier_at("{{ missing }}", 3),
            Some("missing".to_string())
        );
        assert_eq!(
            extract_identifier_at("G1 X{{ m1 }}", 7),
            Some("m1".to_string())
        );
        assert_eq!(extract_identifier_at("(( x", 0), None);
        // 裸标识符 → 恢复；属性/下标链 → 宁缺毋错
        assert_eq!(
            extract_undefined_var_name("{{ missing }}", 3..10),
            Some("missing".to_string())
        );
        assert_eq!(
            extract_undefined_var_name("{{ x.missing_attr }}", 3..17),
            None
        );
        assert_eq!(extract_undefined_var_name("{{ table[key] }}", 3..13), None);
    }

    #[test]
    fn error_display_includes_subdivision() {
        let err = TplError::UndefinedVariable {
            name: "t.j2".to_string(),
            variable: "x".to_string(),
            message: "variable 'x' is undefined".to_string(),
        };
        let display = format!("{err}");
        assert!(display.contains("未定义变量"));
        assert!(display.contains("'x'"));
        assert!(display.contains("t.j2"));
    }

    // -----------------------------------------------------------------------
    // 严格 / 宽松模式切换
    // -----------------------------------------------------------------------

    #[test]
    fn default_is_strict_mode() {
        let r = Renderer::new();
        assert!(!r.is_lenient(), "默认应为严格模式");
        let ctx = minijinja::context! {};
        let err = r.render("{{ missing }}", "s.j2", &ctx).unwrap_err();
        match err {
            TplError::UndefinedVariable { .. } => {}
            _ => panic!("严格模式下未定义变量应报错，实际: {err:?}"),
        }
    }

    #[test]
    fn with_lenient_renders_undefined_as_empty() {
        let r = Renderer::new().with_lenient();
        assert!(r.is_lenient());
        let ctx = minijinja::context! { x => 42 };
        let out = r.render("X{{ x }} {{ missing }}", "l.j2", &ctx).unwrap();
        assert_eq!(out, "X42 ", "宽松模式下未定义变量渲染为空字符串");
    }

    #[test]
    fn with_strict_switches_back_to_strict() {
        let r = Renderer::new().with_lenient().with_strict();
        assert!(!r.is_lenient());
        let ctx = minijinja::context! {};
        let err = r.render("{{ missing }}", "s2.j2", &ctx).unwrap_err();
        match err {
            TplError::UndefinedVariable { .. } => {}
            _ => panic!("切回严格模式后未定义变量应报错"),
        }
    }

    #[test]
    fn lenient_mode_still_extracts_required_variables() {
        // 宽松模式只影响渲染行为，不影响 extract_undeclared 的必选判定
        let _r = Renderer::new().with_lenient();
        let ast = parse("X{{ x }} Y{{ y | default(1) }}", "e.j2").unwrap();
        let vars = extract_undeclared(&ast);
        let names: Vec<&str> = vars.iter().map(|v| v.name.as_str()).collect();
        assert!(names.contains(&"x"), "无 default 的 x 应仍为必选");
        let x = vars.iter().find(|v| v.name == "x").unwrap();
        assert!(!x.optional, "宽松模式不影响 optional 判定");
    }

    #[test]
    fn strict_mode_lenient_mode_render_consistency() {
        // 提供完整参数时，严格与宽松模式输出应一致
        let r_strict = Renderer::new();
        let r_lenient = Renderer::new().with_lenient();
        let ctx = minijinja::context! { x => 21.0, y => 15.5 };
        let src = "X{{ x | nc_fixed(3) }} Y{{ y | nc_fixed(3) }}";
        let a = r_strict.render(src, "c.j2", &ctx).unwrap();
        let b = r_lenient.render(src, "c.j2", &ctx).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn extract_quoted_works() {
        assert_eq!(
            extract_quoted("unknown filter 'foo'"),
            Some("foo".to_string())
        );
        assert_eq!(
            extract_quoted("variable \"x\" is undefined"),
            Some("x".to_string())
        );
        assert_eq!(extract_quoted("no quotes here"), None);
    }

    // -----------------------------------------------------------------------
    // 边界 case：空模板 / 纯文本 / 注释 / 保留名
    // -----------------------------------------------------------------------

    #[test]
    fn empty_template_parses_and_has_no_vars() {
        let ast = parse("", "empty.j2").unwrap();
        assert!(extract_variables(&ast).is_empty());
        assert!(extract_undeclared(&ast).is_empty());
    }

    #[test]
    fn pure_text_has_no_vars() {
        let ast = parse("G1 X10 Y20\nM3 S1000", "text.j2").unwrap();
        assert!(extract_variables(&ast).is_empty());
        assert!(extract_undeclared(&ast).is_empty());
    }

    #[test]
    fn comments_do_not_produce_vars() {
        let src = "{# this is a comment with x y z #}G1 X{{ actual }}";
        let ast = parse(src, "comment.j2").unwrap();
        let undeclared = extract_undeclared(&ast);
        let names: Vec<&str> = undeclared.iter().map(|v| v.name.as_str()).collect();
        assert_eq!(names, vec!["actual"]);
    }

    #[test]
    fn reserved_names_not_undeclared() {
        // loop / self / super / caller 是引擎内置，不算未声明
        let src = "{% for item in items %}{{ loop.index }} {{ self }} {{ super }} {{ caller }}{% endfor %}";
        let ast = parse(src, "reserved.j2").unwrap();
        let undeclared = extract_undeclared(&ast);
        let names: Vec<&str> = undeclared.iter().map(|v| v.name.as_str()).collect();
        assert_eq!(names, vec!["items"]);
    }

    // -----------------------------------------------------------------------
    // 边界 case：作用域（for / macro / set）
    // -----------------------------------------------------------------------

    #[test]
    fn for_loop_var_not_undeclared() {
        let src = "{% for x in xs %}{{ x }}{% endfor %}";
        let ast = parse(src, "for.j2").unwrap();
        let undeclared = extract_undeclared(&ast);
        let names: Vec<&str> = undeclared.iter().map(|v| v.name.as_str()).collect();
        assert_eq!(names, vec!["xs"]);
    }

    #[test]
    fn macro_params_not_undeclared() {
        let src = "{% macro greet(name, greeting) %}{{ greeting }} {{ name }}{% endmacro %}{{ greet(\"world\", \"Hi\") }}";
        let ast = parse(src, "macro.j2").unwrap();
        // 宏参数 name/greeting 不算未声明；greet 是宏调用也不算
        assert!(extract_undeclared(&ast).is_empty());
    }

    #[test]
    fn set_declared_var_not_undeclared() {
        let src = "{% set x = 1 %}{% set y = x + 2 %}{{ x }} {{ y }} {{ z }}";
        let ast = parse(src, "set.j2").unwrap();
        let undeclared = extract_undeclared(&ast);
        let names: Vec<&str> = undeclared.iter().map(|v| v.name.as_str()).collect();
        assert_eq!(names, vec!["z"]);
    }

    // -----------------------------------------------------------------------
    // 作用域语义：set/with/for/macro 的作用域正确性
    // -----------------------------------------------------------------------

    #[test]
    fn self_referential_set_reports_undeclared() {
        // {% set total = total + price %}：右侧 total 引用外层（上下文）值，
        // 必须出现在未声明集合中，否则校验漏报、严格渲染才报错
        let src = "{% set total = total + price %}T{{ total }}";
        let ast = parse(src, "selfset.j2").unwrap();
        let undeclared = extract_undeclared(&ast);
        let names: Vec<&str> = undeclared.iter().map(|v| v.name.as_str()).collect();
        assert_eq!(names, vec!["total", "price"]);
        assert!(
            undeclared.iter().all(|v| !v.optional),
            "自引用 set 引用的变量应为必选"
        );
    }

    #[test]
    fn set_inside_for_does_not_leak() {
        // for 是独立作用域（Jinja2 语义）：循环内 set 的名字在循环外不可见，
        // 循环后引用 hx 应视为未声明（渲染时缺失会报错）
        let src = "{% for h in holes %}{% set hx = h.x %}{{ hx }}{% endfor %}{{ hx }}";
        let ast = parse(src, "leak.j2").unwrap();
        let undeclared = extract_undeclared(&ast);
        let names: Vec<&str> = undeclared.iter().map(|v| v.name.as_str()).collect();
        assert_eq!(names, vec!["holes", "hx"]);
    }

    #[test]
    fn for_var_not_visible_after_loop() {
        let src = "{% for x in items %}{{ x }}{% endfor %}{{ x }}";
        let ast = parse(src, "forleak.j2").unwrap();
        let undeclared = extract_undeclared(&ast);
        let names: Vec<&str> = undeclared.iter().map(|v| v.name.as_str()).collect();
        assert_eq!(names, vec!["items", "x"]);
    }

    #[test]
    fn with_var_not_visible_after_block() {
        let src = "{% with y = 1 %}{{ y }}{% endwith %}{{ y }}";
        let ast = parse(src, "withleak.j2").unwrap();
        let undeclared = extract_undeclared(&ast);
        let names: Vec<&str> = undeclared.iter().map(|v| v.name.as_str()).collect();
        assert_eq!(names, vec!["y"]);
    }

    #[test]
    fn set_inside_if_persists_to_template_scope() {
        // if 不创建作用域（Jinja2 语义）：if 内 set 的名字在其后可见
        let src = "{% if cond %}{% set tmp = 1 %}{% endif %}{{ tmp }}";
        let ast = parse(src, "ifset.j2").unwrap();
        let undeclared = extract_undeclared(&ast);
        let names: Vec<&str> = undeclared.iter().map(|v| v.name.as_str()).collect();
        assert_eq!(names, vec!["cond"]);
    }

    #[test]
    fn for_filter_expr_sees_loop_var() {
        // for 的 if 过滤表达式可引用循环变量（Jinja2 语义）
        let src = "{% for x in items if x > 0 %}{{ x }}{% endfor %}";
        let ast = parse(src, "forfilter.j2").unwrap();
        let undeclared = extract_undeclared(&ast);
        let names: Vec<&str> = undeclared.iter().map(|v| v.name.as_str()).collect();
        assert_eq!(names, vec!["items"]);
    }

    #[test]
    fn macro_body_scoped() {
        // 宏体内 set 的名字不泄漏到外层
        let src = "{% macro m() %}{% set inner = 1 %}{{ inner }}{% endmacro %}{{ m() }}{{ inner }}";
        let ast = parse(src, "macrosc.j2").unwrap();
        let undeclared = extract_undeclared(&ast);
        let names: Vec<&str> = undeclared.iter().map(|v| v.name.as_str()).collect();
        assert_eq!(names, vec!["inner"]);
    }

    #[test]
    fn with_self_referential_value_reports_outer_var() {
        // {% with y = y + 1 %}：右侧 y 引用外层/上下文值，不应被误判为局部
        let src = "{% with y = y + base %}{{ y }}{% endwith %}";
        let ast = parse(src, "withself.j2").unwrap();
        let undeclared = extract_undeclared(&ast);
        let names: Vec<&str> = undeclared.iter().map(|v| v.name.as_str()).collect();
        assert_eq!(names, vec!["y", "base"]);
    }

    #[test]
    fn nested_default_all_optional() {
        // {{ a | default(b | default(1)) }}：a 和 b 都在兜底上下文
        let src = "{{ a | default(b | default(1)) }}";
        let ast = parse(src, "nest.j2").unwrap();
        let vars = extract_variables(&ast);
        for v in &vars {
            assert!(v.optional, "{} 应标记为可选", v.name);
        }
        assert_eq!(vars.len(), 2);
    }

    #[test]
    fn filter_chain_extracts_vars() {
        let src = "{{ x | abs | round(2) }}";
        let ast = parse(src, "chain.j2").unwrap();
        let undeclared = extract_undeclared(&ast);
        let names: Vec<&str> = undeclared.iter().map(|v| v.name.as_str()).collect();
        assert_eq!(names, vec!["x"]);
    }

    #[test]
    fn complex_expression_vars() {
        let src = "G1 X{{ (diameter / 2) + offset | round(2) }} F{{ feed * 1.5 }}";
        let ast = parse(src, "complex.j2").unwrap();
        let undeclared = extract_undeclared(&ast);
        let names: Vec<&str> = undeclared.iter().map(|v| v.name.as_str()).collect();
        assert_eq!(names, vec!["diameter", "offset", "feed"]);
    }

    #[test]
    fn string_and_list_literals_no_vars() {
        let src = r#"{{ "hello" }} {{ [1, 2, 3] }} {{ {"a": 1} }}"#;
        let ast = parse(src, "literal.j2").unwrap();
        assert!(extract_undeclared(&ast).is_empty());
    }

    #[test]
    fn whitespace_control_parses() {
        let src = "{%- set x = 1 -%}\n{{- x -}}\n";
        let ast = parse(src, "ws.j2").unwrap();
        assert!(extract_undeclared(&ast).is_empty());
    }

    #[test]
    fn variable_name_with_underscores_and_digits() {
        let src = "{{ my_var_1 }} {{ _private }} {{ x2 }}";
        let ast = parse(src, "names.j2").unwrap();
        let undeclared = extract_undeclared(&ast);
        let names: Vec<&str> = undeclared.iter().map(|v| v.name.as_str()).collect();
        assert_eq!(names, vec!["my_var_1", "_private", "x2"]);
    }

    // -----------------------------------------------------------------------
    // 极端输入：Unicode / 特殊字符 / 超长 / 深嵌套
    // -----------------------------------------------------------------------

    #[test]
    fn unicode_content_renders() {
        // 模板内容（非变量名）含中文/emoji，应正常解析和渲染
        let src = "G1 X{{ x }} (中文注释 ✅)";
        let r = Renderer::new();
        let ctx = minijinja::context! { x => 10.0 };
        let out = r.render(src, "unicode.j2", &ctx).unwrap();
        assert!(out.contains("中文注释 ✅"));
        assert!(out.contains("X10"));
    }

    #[test]
    fn special_characters_in_text() {
        // 反斜杠、引号、控制字符在纯文本中应正常透传
        let src = r#"path: C:\temp\file "quoted" tab:	here"#;
        let ast = parse(src, "special.j2").unwrap();
        assert!(extract_undeclared(&ast).is_empty());
        let r = Renderer::new();
        let ctx = minijinja::context! {};
        let out = r.render(src, "special.j2", &ctx).unwrap();
        assert!(out.contains(r"C:\temp\file"));
        assert!(out.contains("\"quoted\""));
    }

    #[test]
    fn very_long_variable_name() {
        // 256 字符变量名
        let long_name = "x".repeat(256);
        let src = format!("{{{{ {long_name} }}}}");
        let ast = parse(&src, "long.j2").unwrap();
        let undeclared = extract_undeclared(&ast);
        assert_eq!(undeclared.len(), 1);
        assert_eq!(undeclared[0].name.len(), 256);
    }

    #[test]
    fn many_distinct_variables() {
        // 1000 个不同变量，验证去重和性能
        let mut src = String::new();
        for i in 0..1000 {
            src.push_str(&format!("{{{{ var_{i} }}}} "));
        }
        let ast = parse(&src, "many.j2").unwrap();
        let undeclared = extract_undeclared(&ast);
        assert_eq!(undeclared.len(), 1000);
        let all = extract_variables(&ast);
        assert_eq!(all.len(), 1000);
    }

    #[test]
    fn deeply_nested_ifs() {
        // 50 层嵌套 if
        let mut src = String::new();
        for i in 0..50 {
            src.push_str(&format!("{{% if v{i} > 0 %}}"));
        }
        src.push_str("DEEP");
        for _ in 0..50 {
            src.push_str("{% endif %}");
        }
        let ast = parse(&src, "deep.j2").unwrap();
        let undeclared = extract_undeclared(&ast);
        assert_eq!(undeclared.len(), 50);
    }

    #[test]
    fn deeply_nested_defaults() {
        // 50 层嵌套 default：{{ a | default(b | default(c | ... | default(0))) }}
        let mut expr = String::from("0");
        for i in (0..50).rev() {
            expr = format!("v{i} | default({expr})");
        }
        let src = format!("{{{{ {expr} }}}}");
        let ast = parse(&src, "nestdef.j2").unwrap();
        let vars = extract_variables(&ast);
        assert_eq!(vars.len(), 50);
        // 所有变量都在 default 兜底链中，应全为可选
        for v in &vars {
            assert!(v.optional, "{} 应标记为可选", v.name);
        }
    }

    #[test]
    fn mixed_optional_required_in_chain() {
        // default 链中混入非兜底引用：{{ a | default(b) }} {{ c }}
        // a 可选（在 default 操作数位置），b 必选（default 的参数位置），c 必选
        let src = "{{ a | default(b) }} {{ c }}";
        let ast = parse(src, "mixed.j2").unwrap();
        let vars = extract_variables(&ast);
        let get = |n: &str| vars.iter().find(|v| v.name == n).unwrap();
        assert!(get("a").optional, "a 应可选");
        assert!(!get("b").optional, "b 应必选（default 参数）");
        assert!(!get("c").optional, "c 应必选");
    }

    #[test]
    fn comment_with_special_chars() {
        // 注释中含模板语法字符，不应被解析
        let src = "{# {{ not_a_var }} {% if x %} #}{{ real_var }}";
        let ast = parse(src, "comment.j2").unwrap();
        let undeclared = extract_undeclared(&ast);
        let names: Vec<&str> = undeclared.iter().map(|v| v.name.as_str()).collect();
        assert_eq!(names, vec!["real_var"]);
    }

    #[test]
    fn raw_block_ignores_template_syntax() {
        // raw 块内的模板语法不应被解析
        let src = "{% raw %}{{ not_var }} {% if x %}{% endraw %}{{ real }}";
        let ast = parse(src, "raw.j2").unwrap();
        let undeclared = extract_undeclared(&ast);
        let names: Vec<&str> = undeclared.iter().map(|v| v.name.as_str()).collect();
        assert_eq!(names, vec!["real"]);
    }

    #[test]
    fn empty_for_loop_body() {
        let src = "{% for x in items %}{% endfor %}";
        let ast = parse(src, "emptyfor.j2").unwrap();
        let undeclared = extract_undeclared(&ast);
        let names: Vec<&str> = undeclared.iter().map(|v| v.name.as_str()).collect();
        assert_eq!(names, vec!["items"]);
    }

    #[test]
    fn macro_with_no_args() {
        let src = "{% macro say_hi() %}HI{% endmacro %}{{ say_hi() }}";
        let ast = parse(src, "macro0.j2").unwrap();
        assert!(extract_undeclared(&ast).is_empty());
    }

    #[test]
    fn variable_starting_with_digit_is_syntax_error() {
        // Jinja2 变量名不能以数字开头
        let result = parse("{{ 1bad }}", "baddigit.j2");
        assert!(result.is_err());
    }

    #[test]
    fn render_with_nan_in_context_rejects() {
        // 上下文中传入 NaN，渲染时应报错（数学过滤器或直接输出）
        let r = Renderer::new();
        let ctx = minijinja::context! { x => f64::NAN };
        // 直接输出 NaN 可能不报错（minijinja 允许），但通过数学过滤器应报错
        let err = r.render("{{ x | sqrt }}", "nan.j2", &ctx).unwrap_err();
        match err {
            TplError::Render { .. } => {}
            _ => panic!("NaN 通过数学过滤器应报错"),
        }
    }

    // -----------------------------------------------------------------------
    // NC 数值格式化过滤器（nc_fixed / nc_strip / nc_pad）
    // -----------------------------------------------------------------------

    #[test]
    fn nc_fixed_decimal_places() {
        let r = Renderer::new();
        let ctx = minijinja::context! { x => 21.0, y => 15.5 };
        // 固定 3 位小数
        let out = r
            .render(
                "X{{ x | nc_fixed(3) }} Y{{ y | nc_fixed(3) }}",
                "f.j2",
                &ctx,
            )
            .unwrap();
        assert_eq!(out, "X21.000 Y15.500");
        // 固定 0 位小数（取整）
        let out = r.render("X{{ x | nc_fixed(0) }}", "f0.j2", &ctx).unwrap();
        assert_eq!(out, "X21");
    }

    #[test]
    fn nc_strip_trailing_zeros() {
        let r = Renderer::new();
        let ctx = minijinja::context! { x => 21.0, y => 15.50, z => 0.0 };
        let out = r
            .render(
                "X{{ x | nc_strip }} Y{{ y | nc_strip }} Z{{ z | nc_strip }}",
                "s.j2",
                &ctx,
            )
            .unwrap();
        assert_eq!(out, "X21 Y15.5 Z0");
    }

    #[test]
    fn nc_pad_leading_zeros() {
        let r = Renderer::new();
        let ctx = minijinja::context! { n => 1, line => 10, big => 12345 };
        // 程序号 O0001
        let out = r
            .render("O{{ n | nc_pad(4) }} N{{ line | nc_pad(4) }}", "p.j2", &ctx)
            .unwrap();
        assert_eq!(out, "O0001 N0010");
        // 数值超过宽度时不截断
        let out = r.render("{{ big | nc_pad(3) }}", "pbig.j2", &ctx).unwrap();
        assert_eq!(out, "12345");
    }

    #[test]
    fn nc_filters_accept_integer_input() {
        // 整数字面量应能被 f64 参数的过滤器接受
        let r = Renderer::new();
        let ctx = minijinja::context! {};
        let out = r
            .render(
                "{{ 42 | nc_fixed(2) }} {{ 7 | nc_strip }} {{ 5 | nc_pad(4) }}",
                "int.j2",
                &ctx,
            )
            .unwrap();
        assert_eq!(out, "42.00 7 0005");
    }

    #[test]
    fn nc_filters_negative_values() {
        let r = Renderer::new();
        let ctx = minijinja::context! { x => -21.5 };
        let out = r
            .render("X{{ x | nc_fixed(3) }} X{{ x | nc_strip }}", "neg.j2", &ctx)
            .unwrap();
        assert_eq!(out, "X-21.500 X-21.5");
    }

    #[test]
    fn nc_filters_reject_non_finite() {
        let r = Renderer::new();
        // NaN
        let ctx_nan = minijinja::context! { x => f64::NAN };
        let err = r
            .render("{{ x | nc_fixed(2) }}", "nan.j2", &ctx_nan)
            .unwrap_err();
        match err {
            TplError::Render { .. } => {}
            _ => panic!("NaN 应报错"),
        }
        // Inf
        let ctx_inf = minijinja::context! { x => f64::INFINITY };
        let err = r
            .render("{{ x | nc_strip }}", "inf.j2", &ctx_inf)
            .unwrap_err();
        match err {
            TplError::Render { .. } => {}
            _ => panic!("Inf 应报错"),
        }
    }

    #[test]
    fn nc_pad_zero_width_rejects() {
        let r = Renderer::new();
        let ctx = minijinja::context! { n => 1 };
        let err = r
            .render("{{ n | nc_pad(0) }}", "pad0.j2", &ctx)
            .unwrap_err();
        match err {
            TplError::Render { message, .. } => assert!(message.contains("宽度不能为 0")),
            _ => panic!("nc_pad(0) 应报错"),
        }
    }

    #[test]
    fn nc_pad_negative_rejects() {
        // 负数会拼出 O-001 这类非法 G-code，应报错
        let r = Renderer::new();
        let ctx = minijinja::context! { n => -1.0 };
        let err = r
            .render("O{{ n | nc_pad(4) }}", "padneg.j2", &ctx)
            .unwrap_err();
        match err {
            TplError::Render { message, .. } => assert!(message.contains("负数")),
            _ => panic!("nc_pad 负数应报错"),
        }
    }

    #[test]
    fn nc_filters_combined_in_gcode() {
        // 模拟真实 G-code 场景：程序号 + 坐标 + 行号
        let r = Renderer::new();
        let ctx = minijinja::context! {
            prog => 1,
            x => 21.0,
            y => 15.5,
            feed => 0.150,
            line => 10,
        };
        let src = "O{{ prog | nc_pad(4) }}\nN{{ line | nc_pad(4) }} G1 X{{ x | nc_fixed(3) }} Y{{ y | nc_fixed(3) }} F{{ feed | nc_strip }}";
        let out = r.render(src, "gcode.j2", &ctx).unwrap();
        assert_eq!(out, "O0001\nN0010 G1 X21.000 Y15.500 F0.15");
    }

    // -----------------------------------------------------------------------
    // 并发安全：Send + Sync 编译时断言 + 多线程渲染
    // -----------------------------------------------------------------------

    #[test]
    fn types_are_send_and_sync() {
        // 编译时断言：核心类型可跨线程共享和移动
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Renderer>();
        assert_send_sync::<Variable>();
        assert_send_sync::<TplError>();
        // Ast 带生命周期，用 'static 验证
        assert_send_sync::<Ast<'static>>();
    }

    #[test]
    fn multi_thread_render_shared_renderer() {
        // 多个线程共享同一个 Renderer（&self），同时渲染不同模板
        use std::sync::Arc;
        use std::thread;

        let renderer = Arc::new(Renderer::new());
        let mut handles = vec![];

        for i in 0..8 {
            let r = Arc::clone(&renderer);
            handles.push(thread::spawn(move || {
                let src = format!("G1 X{{{{ x }}}} F{{{{ feed }}}} ; thread {i}");
                let ctx = minijinja::context! { x => i as f64 * 10.0, feed => 0.15 };
                let out = r.render(&src, &format!("t{i}.j2"), &ctx).unwrap();
                assert!(out.contains(&format!("X{}", i * 10)));
                assert!(out.contains("F0.15"));
                out
            }));
        }

        for h in handles {
            h.join().unwrap();
        }
    }

    #[test]
    fn multi_thread_parse_and_extract() {
        // 多个线程同时解析和提取变量
        use std::thread;

        let mut handles = vec![];
        for i in 0..8 {
            handles.push(thread::spawn(move || {
                let src = format!("{{{{ var_{i} }}}} {{{{ common }}}}");
                let name = format!("t{i}.j2");
                let ast = parse(&src, &name).unwrap();
                let undeclared = extract_undeclared(&ast);
                assert_eq!(undeclared.len(), 2);
                undeclared
            }));
        }
        for h in handles {
            let result = h.join().unwrap();
            assert_eq!(result.len(), 2);
        }
    }

    // -----------------------------------------------------------------------
    // Fuzz 测试：随机模板输入不 panic
    // -----------------------------------------------------------------------

    /// 简单 LCG 伪随机数生成器（无需额外依赖）。
    struct SimpleRng {
        state: u64,
    }

    impl SimpleRng {
        fn new(seed: u64) -> Self {
            SimpleRng { state: seed }
        }
        fn next_u64(&mut self) -> u64 {
            // LCG 参数（Numerical Recipes）
            self.state = self
                .state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            self.state
        }
        fn next_usize(&mut self, max: usize) -> usize {
            (self.next_u64() as usize) % max
        }
    }

    #[test]
    fn fuzz_random_templates_no_panic() {
        // 5000 次随机模板输入，验证 parse/extract 绝不 panic
        let charset: Vec<char> = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 \t\n{}%#|/-=!<>()[].,;:\"'&".chars().collect();
        let keywords = [
            "if",
            "for",
            "set",
            "macro",
            "default",
            "defined",
            "end",
            "else",
            "elif",
            "include",
            "extends",
            "import",
            "from",
            "as",
            "in",
            "not",
            "and",
            "or",
            "is",
            "{{",
            "}}",
            "{%",
            "%}",
            "{#",
            "#}",
            "|",
            "default(",
            "is defined",
            "is undefined",
        ];

        let mut rng = SimpleRng::new(20260830);

        for iteration in 0..5000 {
            // 随机生成长度 0-200 的字符串
            let len = rng.next_usize(201);
            let mut s = String::with_capacity(len);
            for _ in 0..len {
                if rng.next_usize(10) < 3 {
                    // 30% 概率插入关键字片段
                    let kw = keywords[rng.next_usize(keywords.len())];
                    s.push_str(kw);
                } else {
                    // 70% 概率插入随机字符
                    s.push(charset[rng.next_usize(charset.len())]);
                }
            }

            let name = format!("fuzz_{iteration}.j2");
            // 核心断言：parse 和 extract 绝不 panic
            if let Ok(ast) = parse(&s, &name) {
                let _ = extract_variables(&ast);
                let _ = extract_undeclared(&ast);
            }
        }
        // 如果到达这里，说明 5000 次迭代均无 panic
    }

    #[test]
    fn fuzz_random_render_no_panic() {
        // 1000 次随机渲染输入，验证 render 绝不 panic
        let charset: Vec<char> = "abcdefghijklmnopqrstuvwxyz0123456789 \t\n{}%|/-=!<>()[].,;:"
            .chars()
            .collect();
        let mut rng = SimpleRng::new(42);
        let renderer = Renderer::new();

        for iteration in 0..1000 {
            let len = rng.next_usize(101);
            let mut s = String::with_capacity(len);
            for _ in 0..len {
                s.push(charset[rng.next_usize(charset.len())]);
            }

            let ctx = minijinja::context! { x => 1.0, y => "test", z => vec![1, 2, 3] };
            // render 返回 Err 是正常的（语法错误），但绝不 panic
            let _ = renderer.render(&s, &format!("r{iteration}.j2"), &ctx);
        }
        // 如果到达这里，说明 1000 次迭代均无 panic
    }

    // -----------------------------------------------------------------------
    // 内存/性能：大模板、深嵌套、无 O(n²)
    // -----------------------------------------------------------------------

    #[test]
    fn large_template_1mb_parses_and_extracts() {
        // 生成约 1MB 的模板（重复 G-code 行，每行含变量）
        let line = "G1 X{{ diameter / 2 }} Y{{ y_pos }} F{{ feed }} S{{ speed }}\n";
        let repeats = 1_000_000 / line.len();
        let src: String = line.repeat(repeats);
        assert!(
            src.len() >= 900_000,
            "模板应接近 1MB，实际 {} 字节",
            src.len()
        );

        let ast = parse(&src, "large.j2").unwrap();
        let undeclared = extract_undeclared(&ast);
        // 只有 4 个不同变量（diameter, y_pos, feed, speed）
        assert_eq!(undeclared.len(), 4);
        let all = extract_variables(&ast);
        assert_eq!(all.len(), 4);
    }

    #[test]
    fn large_template_render_performance() {
        // 100KB 模板渲染应在合理时间内完成
        let line = "G1 X{{ x }} Y{{ y }} F{{ feed }}\n";
        let repeats = 100_000 / line.len();
        let src: String = line.repeat(repeats);
        assert!(
            src.len() >= 90_000,
            "模板源应接近 100KB，实际 {} 字节",
            src.len()
        );

        let r = Renderer::new();
        let ctx = minijinja::context! { x => 10.0, y => 20.0, feed => 0.15 };
        let start = std::time::Instant::now();
        let out = r.render(&src, "large_render.j2", &ctx).unwrap();
        let elapsed = start.elapsed();
        // 渲染后输出应非空且包含预期内容（不精确卡长度，因 f64 格式和换行处理可能变化）
        assert!(!out.is_empty());
        assert!(out.contains("G1 X10.0 Y20.0 F0.15"));
        // 输出行数应与 repeats 一致
        let line_count = out.lines().count();
        assert_eq!(line_count, repeats, "输出行数应与 repeats 一致");
        assert!(
            elapsed.as_millis() < 2000,
            "渲染 100KB 应 < 2s，实际 {:?}",
            elapsed
        );
    }

    #[test]
    fn deeply_nested_100_levels_no_stack_overflow() {
        // 100 层嵌套 if（minijinja 解析器有递归深度限制，应在 parse 阶段报错而非栈溢出）
        let mut src = String::new();
        for i in 0..100 {
            src.push_str(&format!("{{% if v{i} > 0 %}}"));
        }
        src.push_str("DEEP");
        for _ in 0..100 {
            src.push_str("{% endif %}");
        }
        // 无论成功还是语法错误，都不应 panic 或栈溢出
        let result = parse(&src, "deep100.j2");
        match result {
            Ok(ast) => {
                // 如果解析成功（minijinja 允许 100 层），变量提取也不应栈溢出
                let _ = extract_variables(&ast);
                let _ = extract_undeclared(&ast);
            }
            Err(_) => {
                // 解析失败是正常的（递归深度限制），不是 bug
            }
        }
    }

    #[test]
    fn many_duplicate_references_efficient() {
        // 同一变量被引用 10000 次，去重后应只有 1 个，且不 O(n²)
        let src = "{{ x }}".repeat(10000);
        let ast = parse(&src, "dup.j2").unwrap();
        let all = extract_variables(&ast);
        let undeclared = extract_undeclared(&ast);
        assert_eq!(all.len(), 1);
        assert_eq!(undeclared.len(), 1);
        assert_eq!(all[0].name, "x");
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
            TplError::UndefinedVariable {
                variable, message, ..
            } => {
                // 变量名从源码错误位置尽力恢复
                assert_eq!(variable, "missing_var", "应恢复出变量名");
                assert!(
                    message.contains("undefined"),
                    "Strict 模式应报未定义值错误: {message}"
                );
            }
            _ => panic!("应为 UndefinedVariable 错误"),
        }
    }
}
