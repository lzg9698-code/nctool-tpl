//! 变量提取核心：AST 遍历，区分可选/必选引用。
//!
//! 对标 Python `jinja2.meta`：解析 [`Ast`]、收集引用的变量、区分模板内声明
//! 与外部未声明变量（可选/必选判定），以及静态模板引用提取。

use std::collections::HashSet;

use minijinja::machinery::ast::{self, Expr, Spanned, Stmt};
use minijinja::machinery::WhitespaceConfig;
use minijinja::syntax::SyntaxConfig;

use crate::error::{line_col_at, TplError};

/// 引擎内置、不由上下文提供的名字（出现在模板里也不算“未声明变量”）。
const RESERVED_NAMES: &[&str] = &["loop", "self", "super", "caller"];

/// Jinja 自动注入的内置全局（函数/构造器），同样不算“需要外部提供的参数”。
/// 与 `jinja2.meta` 一致：无参数使用的这些全局名不进入未声明集合。
///
/// 注意：`debug` 是 minijinja 启用 `debug` feature 后才注入的全局。本库依赖
/// `debug` feature（用于解析错误的字节范围定位），故必须在此列出，否则
/// `{{ debug() }}` 会被误报为必选参数。
const BUILTIN_GLOBALS: &[&str] = &[
    "range",
    "dict",
    "lipsum",
    "cycler",
    "joiner",
    "namespace",
    "debug",
];

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
    /// 兜底上下文指：作为 `default`/`d` 过滤器或 `is defined`/`is undefined`
    /// 测试的**直接裸变量操作数**（如 `{{ x | default(0.15) }}`、
    /// `{% if x is defined %}`）——这些位置上的引用在变量缺失时模板仍可安全渲染。
    ///
    /// **兜底不向下传播**：`{{ (a+b) | default(1) }}`、`{{ a.b | default(1) }}`、
    /// `{% if a.b is defined %}` 中的 `a`/`b` **不**算兜底引用——minijinja 会先
    /// 求值子表达式（运算 / 取属性），undefined 参与即报错，`default` 无法兜底。
    /// 这类变量记为必选，避免上层校验放行后严格模式渲染失败。
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

/// 提取模板通过 `{% include %}` / `{% extends %}` / `{% import %}` / `{% from %}`
/// 静态引用的模板名（字符串字面量形式），按出现顺序去重。
///
/// 动态引用（如 `{% include name %}`，`name` 为变量）无法静态确定，不在结果中。
/// 上层（如注册表校验）可据此递归检查被引用模板的参数，避免组合模板的
/// 必选参数漏检。
pub fn extract_template_refs(ast: &Ast) -> Vec<String> {
    let mut refs = Vec::new();
    collect_template_refs_stmt(&ast.stmt, &mut refs);
    refs
}

/// 遍历语句树收集静态模板引用名（只需覆盖所有携带语句体的分支）。
fn collect_template_refs_stmt<'a>(stmt: &Stmt<'a>, refs: &mut Vec<String>) {
    match stmt {
        Stmt::Template(s) => {
            for child in &s.children {
                collect_template_refs_stmt(child, refs);
            }
        }
        Stmt::ForLoop(s) => {
            for child in &s.body {
                collect_template_refs_stmt(child, refs);
            }
            for child in &s.else_body {
                collect_template_refs_stmt(child, refs);
            }
        }
        Stmt::IfCond(s) => {
            for child in &s.true_body {
                collect_template_refs_stmt(child, refs);
            }
            for child in &s.false_body {
                collect_template_refs_stmt(child, refs);
            }
        }
        Stmt::WithBlock(s) => {
            for child in &s.body {
                collect_template_refs_stmt(child, refs);
            }
        }
        Stmt::SetBlock(s) => {
            for child in &s.body {
                collect_template_refs_stmt(child, refs);
            }
        }
        Stmt::AutoEscape(s) => {
            for child in &s.body {
                collect_template_refs_stmt(child, refs);
            }
        }
        Stmt::FilterBlock(s) => {
            for child in &s.body {
                collect_template_refs_stmt(child, refs);
            }
        }
        Stmt::Block(s) => {
            for child in &s.body {
                collect_template_refs_stmt(child, refs);
            }
        }
        Stmt::Macro(s) => {
            for child in &s.body {
                collect_template_refs_stmt(child, refs);
            }
        }
        Stmt::CallBlock(s) => {
            for child in &s.macro_decl.body {
                collect_template_refs_stmt(child, refs);
            }
        }
        Stmt::Import(s) => collect_ref_name(&s.expr, refs),
        Stmt::FromImport(s) => collect_ref_name(&s.expr, refs),
        Stmt::Extends(s) => collect_ref_name(&s.name, refs),
        Stmt::Include(s) => collect_ref_name(&s.name, refs),
        Stmt::EmitExpr(_)
        | Stmt::EmitRaw(_)
        | Stmt::Set(_)
        | Stmt::Continue(_)
        | Stmt::Break(_)
        | Stmt::Do(_) => {}
    }
}

/// 记录静态（字符串字面量）模板引用名；变量等动态引用无法静态确定，忽略。
fn collect_ref_name(expr: &Expr, refs: &mut Vec<String>) {
    if let Expr::Const(s) = expr {
        if let Some(name) = s.value.as_str() {
            if !name.is_empty() && !refs.iter().any(|r| r == name) {
                refs.push(name.to_string());
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 变量提取：AST 遍历器
// ---------------------------------------------------------------------------

struct Collector<'a> {
    /// 作用域栈：`scopes[0]` 为模板顶层，`for`/`with`/`macro`/`block` 各推入独立作用域
    /// （对齐 Jinja2/minijinja VM 语义：`if` 不创建作用域，`block` 体按独立帧求值）。
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
            // minijinja 按「求值右值 → 绑定目标」**逐条交错**执行（实测：
            // {% with a=1, b=a+1 %}{{ b }}{% endwith %} 输出 2，即便外部传入
            // a=99 也不影响 —— b 的初值看到的是本块绑定的 a）。
            //
            // 因此必须推入作用域后逐条交替处理，两种语义都才能判对：
            //   - {% with a=1, b=a+1 %} → b 的 a 命中本块局部，不进未声明集合（避免误报）
            //   - {% with y = y + 1 %}  → 右值在 y 绑定**前**求值，y 仍记必选（避免漏报）
            c.push_scope();
            for (target, value) in &s.assignments {
                walk_expr(value, c, opt);
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
            // block 体在 VM 中是独立作用域（call_block 推入新帧，set 写入栈顶）：
            // 块内 set 的名字不外泄，块外引用按未声明处理（super 由 RESERVED_NAMES 覆盖）
            c.push_scope();
            for child in &s.body {
                walk_stmt(child, c, opt);
            }
            c.pop_scope();
        }
        Stmt::Import(s) => {
            walk_expr(&s.expr, c, opt);
            declare_locals(&s.name, c);
        }
        Stmt::FromImport(s) => {
            walk_expr(&s.expr, c, opt);
            // minijinja 语义：names 为 (导入名, Option<别名>)，绑定时取别名
            // （无别名则取导入名）。绑定名是局部声明而非外部变量引用，
            // 导入名指向被导入模板的导出，同样不算外部引用。
            for (name, alias) in &s.names {
                declare_locals(alias.as_ref().unwrap_or(name), c);
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

/// 表达式是否为「裸变量引用」（单个 `Expr::Var`，无属性/下标/运算包裹）。
///
/// 只有裸变量才能被 `default` 过滤器 / `defined` 测试安全兜底：minijinja 对
/// undefined 值取属性、下标或参与运算都会直接报错，不会走到兜底逻辑。
fn is_bare_var(expr: &Expr) -> bool {
    matches!(expr, Expr::Var(_))
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
            // default / d：被过滤的操作数在变量缺失时由默认值兜底。
            //
            // 关键：兜底**只对直接操作数生效，且操作数必须是裸变量**。
            // minijinja 先求值操作数再套用过滤器，因此：
            //   - `{{ x | default(1) }}`            操作数即 x，undefined 被兜底 → 可选
            //   - `{{ (a+b) | default(1) }}`        先算 a+b，undefined 参与运算即报错，
            //                                      default 救不了 → a、b 必选
            //   - `{{ a.b | default(1) }}`          先取属性，undefined 父值报错 → a 必选
            // 若把兜底标记传播进子树，会把上述后两类误判为可选，导致上层
            // 校验放行、严格模式渲染却失败 —— 产出不完整 G-code 的最坏失败模式。
            let is_default = matches!(s.name, "default" | "d");
            if let Some(e) = &s.expr {
                walk_expr(e, c, opt || (is_default && is_bare_var(e)));
            }
            // 过滤器参数（含默认值表达式）仍需正常求值 → 透传当前 opt
            for arg in &s.args {
                walk_call_arg(arg, c, opt);
            }
        }
        Expr::Test(s) => {
            // defined / undefined：同样**只对裸变量直接操作数**兜底。
            // `{% if a.b is defined %}` 会先对 undefined 的 a 取属性并报错，
            // 故 a 必选；只有 `{% if x is defined %}` 这类裸变量才记可选。
            let is_defined = matches!(s.name, "defined" | "undefined");
            walk_expr(&s.expr, c, opt || (is_defined && is_bare_var(&s.expr)));
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
