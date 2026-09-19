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
/// **只列 minijinja 真正提供的全局**。`lipsum` / `cycler` / `joiner` 是 Jinja2
/// （Python）侧的东西，minijinja 不提供（本库也没有 `add_global` 注册它们）——
/// 把它们列进来会让**恰好叫这些名字的模板参数**被静默排除出必选与类型校验：
/// 参数写错了没人报，缺参也不拦。而真去写 `{{ lipsum() }}` 的模板在渲染期照样
/// 报"未定义"，名单一点忙也帮不上。
///
/// 注意：`debug` 是 minijinja 启用 `debug` feature 后才注入的全局。本库依赖
/// `debug` feature（用于解析错误的字节范围定位），故必须在此列出，否则
/// `{{ debug() }}` 会被误报为必选参数。
const BUILTIN_GLOBALS: &[&str] = &["range", "dict", "namespace", "debug"];

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
/// 变量提取只读这个结构；渲染路径（`renderer::Renderer::render`）另行把**同一份**
/// `source` 交给 minijinja 从头编译。两条路径共用同一份源码文本，不会出现
/// 「提取看的是新源码、渲染用的还是旧 AST」这类错位。
///
/// 不要在这里断言 minijinja 的编译缓存行为 —— 那是上游实现细节，本注释此前
/// 声称「内部自带 JIT 编译缓存」，与 `render` 每次都走一遍
/// `template_from_named_str` 的实际调用形式对不上，属无据的说法。
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
    ///
    /// **模板局部变量的引用同样不记「必选」**（见下）：局部变量由模板自己
    /// 赋值，不是外部参数，不参与必选判定。
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
        // 关键：只有「外部参数引用」才记必选，模板局部变量不记。
        //
        // 反例（本修复前的行为）：
        //   {% set R1 = R1|default(4000) %}   ← RHS 是兜底引用，本应可选
        //   {{ R1 }}                          ← 引用的是上一行 set 出来的局部量
        // 修复前第二行的局部引用会无条件写入 required_refs，把第一行判定的
        // 「可选」整体翻转为「必选」，导致 `{% set x = x|default(v) %}`
        // 这一标准兜底惯用法无法使用。
        //
        // 注意 `{% set total = total + x %}` 中 RHS 的 total 仍记必选：
        // set 的 RHS 先于目标声明求值（见 walk_stmt 的 Stmt::Set 分支），
        // 此刻 total 还不是局部变量。
        if !in_optional && !self.is_local(name) {
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
#[cfg(test)]
mod tests {
    use super::*;

    fn all_of(src: &str) -> Vec<Variable> {
        let ast = parse(src, "t.j2").expect("解析失败");
        extract_variables(&ast)
    }

    fn undeclared_of(src: &str) -> Vec<Variable> {
        let ast = parse(src, "t.j2").expect("解析失败");
        extract_undeclared(&ast)
    }

    fn names(vs: &[Variable]) -> Vec<&str> {
        vs.iter().map(|v| v.name.as_str()).collect()
    }

    /// 取变量可选性；缺失时 panic 并列出实际结果，便于定位。
    fn opt(vs: &[Variable], name: &str) -> bool {
        vs.iter()
            .find(|v| v.name == name)
            .unwrap_or_else(|| panic!("变量 {name} 未出现在结果中：{:?}", names(vs)))
            .optional
    }

    // ---- 兜底语义：只对「裸变量直接操作数」生效，不向下传播 ----

    /// 裸变量被 `default` 兜底 → 可选。
    #[test]
    fn default_covers_bare_var() {
        let vs = undeclared_of("{{ x | default(1) }}");
        assert!(opt(&vs, "x"));
    }

    /// `default` 兜不住运算：undefined 参与运算即报错，故 a、b 必选。
    #[test]
    fn default_does_not_cover_arithmetic_operand() {
        let vs = undeclared_of("{{ (a + b) | default(1) }}");
        assert!(!opt(&vs, "a"), "a 参与运算，default 兜不住 → 必选");
        assert!(!opt(&vs, "b"), "b 参与运算，default 兜不住 → 必选");
    }

    /// `default` 兜不住属性访问：需先对 undefined 取属性，同样报错。
    #[test]
    fn default_does_not_cover_getattr_operand() {
        let vs = undeclared_of("{{ a.b | default(1) }}");
        assert!(!opt(&vs, "a"), "a 需先取属性，default 兜不住 → 必选");
    }

    /// `is defined` 同理：只兜底裸变量，属性路径上的父变量仍必选。
    #[test]
    fn defined_test_only_covers_bare_var() {
        let bare = undeclared_of("{% if x is defined %}Y{% endif %}");
        assert!(opt(&bare, "x"), "裸变量 + defined → 可选");

        let attr = undeclared_of("{% if a.b is defined %}Y{% endif %}");
        assert!(!opt(&attr, "a"), "a.b 中的 a 需先取属性 → 必选");
    }

    /// 过滤器参数不受操作数兜底影响：`default(y)` 的 y 仍需外部提供。
    #[test]
    fn filter_arg_not_covered_by_operand_default() {
        let vs = undeclared_of("{{ x | default(y) }}");
        assert!(opt(&vs, "x"), "x 被 default 兜底 → 可选");
        assert!(!opt(&vs, "y"), "默认值表达式 y 仍需外部提供 → 必选");
    }

    // ---- set 惯用法：局部引用不得翻转兜底判定 ----

    /// `{% set x = x | default(v) %}` 是标准兜底惯用法，x 必须保持可选。
    ///
    /// 这是本模块修过的真实缺陷：早期实现里后续 `{{ R1 }}` 的**局部**引用会
    /// 无条件记「必选」，把首行判定的可选整体翻转，导致惯用法不可用。
    #[test]
    fn set_default_idiom_keeps_var_optional() {
        let vs = undeclared_of("{% set R1 = R1 | default(4000) %}{{ R1 }}");
        assert!(opt(&vs, "R1"), "R1 整体应可选，否则兜底惯用法失效");
    }

    /// 但 set 的 RHS 先于目标声明求值：`{% set total = total + x %}` 中
    /// 右侧 total 引用的仍是外层/上下文值 → 必选（避免漏报）。
    #[test]
    fn set_rhs_evaluated_before_target_declared() {
        let vs = undeclared_of("{% set total = total + x %}{{ total }}");
        assert!(!opt(&vs, "total"), "RHS 的 total 在绑定前求值 → 必选");
        assert!(!opt(&vs, "x"));
    }

    // ---- with 块：逐条交错「求值右值 → 绑定目标」----

    /// `{% with a = 1, b = a + 1 %}` 中 b 的右值看到本块已绑定的 a → a 不算未声明。
    #[test]
    fn with_block_later_binding_sees_earlier_one() {
        let vs = undeclared_of("{% with a = 1, b = a + 1 %}{{ b }}{% endwith %}");
        assert!(
            !names(&vs).contains(&"a"),
            "a 由本块绑定，不应进未声明集合：{:?}",
            names(&vs)
        );
    }

    /// 同一条赋值内右值先于绑定求值：`{% with y = y + 1 %}` 的 y 仍必选。
    #[test]
    fn with_block_rhs_before_its_own_binding() {
        let vs = undeclared_of("{% with y = y + 1 %}{{ y }}{% endwith %}");
        assert!(!opt(&vs, "y"), "右值 y 在绑定前求值 → 必选");
    }

    // ---- 作用域 ----

    /// for 循环变量是模板局部，不进未声明集合。
    #[test]
    fn for_target_is_local() {
        let vs = undeclared_of("{% for h in holes %}{{ h.x }}{% endfor %}");
        assert_eq!(names(&vs), vec!["holes"]);
    }

    /// block 体是独立作用域：块内 set 的名字不外泄，块外引用按未声明处理。
    #[test]
    fn block_body_has_own_scope() {
        let vs =
            undeclared_of("{% block b %}{% set inner = 1 %}{{ inner }}{% endblock %}{{ inner }}");
        assert!(
            names(&vs).contains(&"inner"),
            "块外引用 inner 应视为未声明：{:?}",
            names(&vs)
        );
        assert!(!opt(&vs, "inner"));
    }

    /// macro 名与参数均为局部（宏名在外层定义，参数在宏作用域内）。
    #[test]
    fn macro_name_and_args_are_local() {
        let vs = undeclared_of(
            "{% macro line(x, y) %}G1 X{{ x }} Y{{ y }}{% endmacro %}{{ line(10, 20) }}",
        );
        assert!(
            names(&vs).is_empty(),
            "宏名/参数均为局部，不应有未声明变量：{:?}",
            names(&vs)
        );
    }

    /// `{% set x %}...{% endset %}`：块体先于目标绑定求值，块体里的外部变量
    /// 仍计入未声明，而目标名本身是局部。
    #[test]
    fn set_block_body_evaluated_before_binding() {
        let vs = undeclared_of("{% set greeting %}hello {{ who }}{% endset %}{{ greeting }}");
        assert_eq!(
            names(&vs),
            vec!["who"],
            "块体中的 who 未声明；greeting 是局部"
        );
    }

    // ---- 保留名与内置全局 ----

    /// 引擎保留名不进变量集合。
    #[test]
    fn reserved_names_excluded() {
        let vs = all_of("{% for i in items %}{{ loop.index }}{{ i }}{% endfor %}");
        let n = names(&vs);
        assert!(n.contains(&"items"));
        assert!(n.contains(&"i"));
        assert!(!n.contains(&"loop"), "loop 是引擎内置：{n:?}");
    }

    /// Jinja 内置全局（range/dict/debug 等）不算「需要外部提供的参数」。
    #[test]
    fn builtin_globals_not_undeclared() {
        let vs = undeclared_of(
            "{% for i in range(3) %}{{ i }}{% endfor %}{{ dict(a=1) }}{% if debug %}{{ debug() }}{% endif %}",
        );
        assert!(
            names(&vs).is_empty(),
            "内置全局不应进未声明集合：{:?}",
            names(&vs)
        );
    }

    // ---- 静态模板引用 ----

    /// include / extends / import / from-import 的字面量模板名被收集且去重。
    #[test]
    fn template_refs_collected_and_deduped() {
        let ast = parse(
            r#"{% extends "base.j2" %}{% include "header.j2" %}{% include "header.j2" %}{% import "macros.j2" as m %}{% from "macros.j2" import helper %}"#,
            "t.j2",
        )
        .unwrap();
        assert_eq!(
            extract_template_refs(&ast),
            vec!["base.j2", "header.j2", "macros.j2"]
        );
    }

    /// 动态模板名无法静态确定，不收集。
    #[test]
    fn template_refs_ignores_dynamic_names() {
        let ast = parse(r#"{% include name %}"#, "t.j2").unwrap();
        assert!(
            extract_template_refs(&ast).is_empty(),
            "变量形式的模板名无法静态确定"
        );
    }

    /// 回归（P1-20）：`collect_template_refs_stmt` 的**嵌套语句体分支此前零覆盖**
    /// —— lcov 显示 for-else、if 的 else 体、with / set-block、autoescape /
    /// filter-block / block / macro / call-block 全部 `DA:...,0`；唯一被测到的
    /// 嵌套形态是 `{% for %}…{% include %}`。
    ///
    /// 这不是「多测一点」：该函数是**组合模板必选参数不漏检**的入口
    /// （`registry::extract_params` 靠它穿透 include 闭包）。漏掉一个分支 =
    /// 用那种语法写的子模板，其必选参数**完全不参与校验**，缺参一路静默到渲染
    /// 甚至产出缺参的 G-code。表驱动逐分支钉住，将来 minijinja 升级改了 AST
    /// 形态（如 elif 的展开方式）也会在这里先红。
    #[test]
    fn template_refs_traverse_every_nested_body() {
        let cases: &[(&str, &[&str])] = &[
            // 顶层并列（原有覆盖，留作对照）
            (r#"{% include "top.j2" %}"#, &["top.j2"]),
            // if 的 true 体与 false 体
            (
                r#"{% if a %}{% include "t.j2" %}{% else %}{% include "f.j2" %}{% endif %}"#,
                &["t.j2", "f.j2"],
            ),
            // elif 在 AST 里展开成嵌套 IfCond，三个分支都要走到
            (
                r#"{% if a %}{% include "t.j2" %}{% elif b %}{% include "e.j2" %}{% else %}{% include "f.j2" %}{% endif %}"#,
                &["t.j2", "e.j2", "f.j2"],
            ),
            // for 的循环体与 else 体
            (
                r#"{% for i in xs %}{% include "body.j2" %}{% else %}{% include "empty.j2" %}{% endfor %}"#,
                &["body.j2", "empty.j2"],
            ),
            // with / set 块
            (
                r#"{% with x = 1 %}{% include "w.j2" %}{% endwith %}"#,
                &["w.j2"],
            ),
            (r#"{% set x %}{% include "s.j2" %}{% endset %}"#, &["s.j2"]),
            // filter / autoescape / block
            (
                r#"{% filter upper %}{% include "filt.j2" %}{% endfilter %}"#,
                &["filt.j2"],
            ),
            (
                r#"{% autoescape true %}{% include "ae.j2" %}{% endautoescape %}"#,
                &["ae.j2"],
            ),
            (
                r#"{% block b %}{% include "blk.j2" %}{% endblock %}"#,
                &["blk.j2"],
            ),
            // macro 体 / call 块
            (
                r#"{% macro m() %}{% include "mac.j2" %}{% endmacro %}"#,
                &["mac.j2"],
            ),
            (
                r#"{% call m() %}{% include "cb.j2" %}{% endcall %}"#,
                &["cb.j2"],
            ),
        ];
        for (src, want) in cases {
            let ast = parse(src, "t.j2").unwrap_or_else(|e| panic!("解析失败 {src}: {e}"));
            assert_eq!(
                extract_template_refs(&ast),
                want.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
                "源码: {src}"
            );
        }
    }

    /// 回归（P2-7）：`BUILTIN_GLOBALS` 只应列 **minijinja 真正提供**的全局。
    /// 它此前含 `lipsum` / `cycler` / `joiner` —— 那是 Jinja2（Python）侧的东西，
    /// minijinja 不提供，本库也没有 `add_global` 注册它们。
    ///
    /// 后果不是"多排除了几个名字"，而是：**恰好叫这些名字的模板参数**被静默排除
    /// 出未声明集合，必选与类型校验一起失效（参数写错没人报、缺参也不拦）；
    /// 而真去写 `{{ lipsum() }}` 的模板在渲染期照样报未定义，名单一点忙没帮上。
    #[test]
    fn builtin_globals_are_exactly_what_minijinja_provides() {
        // 实证：minijinja 环境里这三个名字是未定义的
        let env = minijinja::Environment::new();
        for name in ["lipsum", "cycler", "joiner"] {
            let src = format!("{{{{ {name}() }}}}");
            assert!(
                env.render_str(&src, minijinja::context!()).is_err(),
                "minijinja 不该提供 {name}"
            );
            assert!(
                !BUILTIN_GLOBALS.contains(&name),
                "{name} 不是 minijinja 全局，不该在名单里"
            );
        }
        // 真实存在的全局仍在（删过头会让 {{ range(3) }} 被误报为必选参数）
        for name in ["range", "dict", "namespace", "debug"] {
            assert!(BUILTIN_GLOBALS.contains(&name), "{name} 是 minijinja 全局");
        }
    }

    // ---- 位置信息 ----

    /// 行/列/字节偏移准确，供上层把校验错误定位回源码。
    #[test]
    fn variable_position_is_accurate() {
        let src = "G0 X10\nG1 X{{ dia }}";
        let vs = undeclared_of(src);
        let v = &vs[0];
        assert_eq!(v.name, "dia");
        assert_eq!(v.line, 2);
        assert_eq!(&src[v.start..v.end], "dia", "字节偏移应精确覆盖变量名");
        // 列号按 lexer token 计，而非按字节：`{{` 整体占一列。
        // 故第 2 行 `G1 X{{ dia }}` 的列为：G(1) 1(2) ␠(3) X(4) `{{`(5) ␠(6) d(7)。
        assert_eq!(v.col, 7);
    }
}
