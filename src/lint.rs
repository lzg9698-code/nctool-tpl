//! 模板静态检查（`nctool lint` 的核心）：在**渲染之前**发现会导致错误 G-code
//! 的常见笔误。
//!
//! 首个检查项是**三角函数度制风险**（Backlog：`nctool lint`）。G-code 场景里
//! 角度几乎总是**度**，但 minijinja 的标准三角函数以**弧度**为单位：
//!
//! - `sin` / `cos` / `tan`：输入按**弧度**解释
//! - `asin` / `acos` / `atan`：输出为**弧度**
//!
//! 本项目为此提供了度制变体 `sin_d` / `cos_d` / `tan_d` / `asin_d` / `acos_d` /
//! `atan_d`。模板作者若写 `{{ 30 | sin }}` 以为是度制，实际会得到 `sin(30 rad)`
//! ≈ `-0.988` —— 这个错误**不会报错**，只会静默产出错误坐标（撞刀级）。
//!
//! 因此 lint 对标准三角函数给出**警告**（不是错误：弧度也可能是刻意为之），
//! 建议改用 `_d` 变体。

use minijinja::machinery::ast::{self, CallArg, Expr, Spanned, Stmt};
use minijinja::machinery::Span;

use crate::{parse, TplError};

/// 一条 lint 发现项。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LintFinding {
    /// 1 起始的行号
    pub line: usize,
    /// 1 起始的列号
    pub col: usize,
    /// 触发的过滤器名（如 `sin`）
    pub filter: String,
    /// 建议替换（如 `sin_d`；反三角为 `asin_d`）
    pub suggestion: String,
    /// 人类可读说明
    pub message: String,
}

/// 需要提示的「弧度制」三角函数及其度制替代。
const RADIAN_TRIG: &[(&str, &str)] = &[
    ("sin", "sin_d"),
    ("cos", "cos_d"),
    ("tan", "tan_d"),
    ("asin", "asin_d"),
    ("acos", "acos_d"),
    ("atan", "atan_d"),
];

/// 对模板源码做 lint，返回全部发现项（可能为空）。
///
/// 解析失败时返回 [`TplError`]（与渲染入口一致：语法错误就是语法错误），
/// 调用方决定怎么呈现。`name` 仅用于错误消息。
pub fn lint(source: &str, name: &str) -> Result<Vec<LintFinding>, TplError> {
    let ast = parse(source, name)?;
    let mut collector = LintCollector {
        findings: Vec::new(),
    };
    collector.walk_stmt(&ast.stmt);
    Ok(collector.findings)
}

struct LintCollector {
    findings: Vec<LintFinding>,
}

impl LintCollector {
    fn report_filter(&mut self, filter: &str, span: Span) {
        if let Some((radian, degree)) = RADIAN_TRIG.iter().find(|(r, _)| *r == filter) {
            let is_inverse = matches!(*radian, "asin" | "acos" | "atan");
            let detail = if is_inverse {
                format!("`{radian}` 的输出是**弧度**")
            } else {
                format!("`{radian}` 的输入按**弧度**解释")
            };
            self.findings.push(LintFinding {
                line: span.start_line as usize,
                col: span.start_col as usize,
                filter: (*radian).to_string(),
                suggestion: (*degree).to_string(),
                message: format!(
                    "{detail}；G-code 角度场景通常用**度**，若本意是度请改用 `{degree}`"
                ),
            });
        }
    }

    fn walk_stmt(&mut self, stmt: &Stmt<'_>) {
        match stmt {
            Stmt::Template(s) => {
                for child in &s.children {
                    self.walk_stmt(child);
                }
            }
            Stmt::EmitExpr(s) => self.walk_expr(&s.expr),
            Stmt::EmitRaw(_) => {}
            Stmt::ForLoop(s) => {
                self.walk_expr(&s.iter);
                if let Some(f) = &s.filter_expr {
                    self.walk_expr(f);
                }
                for child in &s.body {
                    self.walk_stmt(child);
                }
                for child in &s.else_body {
                    self.walk_stmt(child);
                }
            }
            Stmt::IfCond(s) => {
                self.walk_expr(&s.expr);
                for child in &s.true_body {
                    self.walk_stmt(child);
                }
                for child in &s.false_body {
                    self.walk_stmt(child);
                }
            }
            Stmt::WithBlock(s) => {
                for (_, value) in &s.assignments {
                    self.walk_expr(value);
                }
                for child in &s.body {
                    self.walk_stmt(child);
                }
            }
            Stmt::Set(s) => self.walk_expr(&s.expr),
            Stmt::SetBlock(s) => {
                if let Some(f) = &s.filter {
                    self.walk_expr(f);
                }
                for child in &s.body {
                    self.walk_stmt(child);
                }
            }
            Stmt::AutoEscape(s) => {
                self.walk_expr(&s.enabled);
                for child in &s.body {
                    self.walk_stmt(child);
                }
            }
            Stmt::FilterBlock(s) => {
                self.walk_expr(&s.filter);
                for child in &s.body {
                    self.walk_stmt(child);
                }
            }
            Stmt::Block(s) => {
                for child in &s.body {
                    self.walk_stmt(child);
                }
            }
            Stmt::Import(s) => self.walk_expr(&s.expr),
            Stmt::FromImport(s) => self.walk_expr(&s.expr),
            Stmt::Extends(s) => self.walk_expr(&s.name),
            Stmt::Include(s) => self.walk_expr(&s.name),
            Stmt::Macro(s) => {
                for d in &s.defaults {
                    self.walk_expr(d);
                }
                for child in &s.body {
                    self.walk_stmt(child);
                }
            }
            Stmt::CallBlock(s) => {
                self.walk_call(&s.call);
                for d in &s.macro_decl.defaults {
                    self.walk_expr(d);
                }
                for child in &s.macro_decl.body {
                    self.walk_stmt(child);
                }
            }
            Stmt::Continue(_) | Stmt::Break(_) => {}
            Stmt::Do(s) => self.walk_call(&s.call),
        }
    }

    fn walk_expr(&mut self, expr: &Expr<'_>) {
        match expr {
            Expr::Var(_) | Expr::Const(_) => {}
            Expr::Slice(s) => {
                self.walk_expr(&s.expr);
                if let Some(e) = &s.start {
                    self.walk_expr(e);
                }
                if let Some(e) = &s.stop {
                    self.walk_expr(e);
                }
                if let Some(e) = &s.step {
                    self.walk_expr(e);
                }
            }
            Expr::UnaryOp(s) => self.walk_expr(&s.expr),
            Expr::BinOp(s) => {
                self.walk_expr(&s.left);
                self.walk_expr(&s.right);
            }
            Expr::Compare(s) => {
                self.walk_expr(&s.expr);
                for op in &s.ops {
                    self.walk_expr(&op.expr);
                }
            }
            Expr::IfExpr(s) => {
                self.walk_expr(&s.test_expr);
                self.walk_expr(&s.true_expr);
                if let Some(f) = &s.false_expr {
                    self.walk_expr(f);
                }
            }
            Expr::Filter(s) => {
                self.report_filter(s.name, s.span());
                if let Some(e) = &s.expr {
                    self.walk_expr(e);
                }
                for arg in &s.args {
                    self.walk_call_arg(arg);
                }
            }
            Expr::Test(s) => {
                self.walk_expr(&s.expr);
                for arg in &s.args {
                    self.walk_call_arg(arg);
                }
            }
            Expr::GetAttr(s) => self.walk_expr(&s.expr),
            Expr::GetItem(s) => {
                self.walk_expr(&s.expr);
                self.walk_expr(&s.subscript_expr);
            }
            Expr::Call(s) => self.walk_call(s),
            Expr::List(s) => {
                for item in &s.items {
                    self.walk_expr(item);
                }
            }
            Expr::Map(s) => {
                for k in &s.keys {
                    self.walk_expr(k);
                }
                for v in &s.values {
                    self.walk_expr(v);
                }
            }
        }
    }

    fn walk_call(&mut self, call: &Spanned<ast::Call<'_>>) {
        self.walk_expr(&call.expr);
        for arg in &call.args {
            self.walk_call_arg(arg);
        }
    }

    fn walk_call_arg(&mut self, arg: &CallArg<'_>) {
        match arg {
            CallArg::Pos(e) | CallArg::PosSplat(e) | CallArg::KwargSplat(e) => self.walk_expr(e),
            CallArg::Kwarg(_, e) => self.walk_expr(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(fs: &[LintFinding]) -> Vec<&str> {
        fs.iter().map(|f| f.filter.as_str()).collect()
    }

    #[test]
    fn flags_radian_trig_filters() {
        let src = "{{ 30 | sin }}\n{{ x | cos }}\n{{ ratio | asin }}\n";
        let fs = lint(src, "t.j2").unwrap();
        assert_eq!(names(&fs), vec!["sin", "cos", "asin"]);
        assert_eq!(fs[0].suggestion, "sin_d");
        assert_eq!(fs[2].suggestion, "asin_d");
        assert_eq!(fs[0].line, 1, "行列应定位到过滤器");
        assert!(fs[0].message.contains("弧度"), "{}", fs[0].message);
    }

    #[test]
    fn degree_variants_are_clean() {
        let src = "{{ 30 | sin_d }}\n{{ x | cos_d }}\n{{ r | atan_d }}";
        assert!(lint(src, "t.j2").unwrap().is_empty());
    }

    #[test]
    fn non_trig_filters_are_clean() {
        let src = "{{ x | nc_fixed(3) }}\n{{ y | round(2) }}\n{{ z | abs }}";
        assert!(lint(src, "t.j2").unwrap().is_empty());
    }

    #[test]
    fn finds_trig_nested_in_expressions_and_statements() {
        let src = concat!(
            "{% set a = (x | sin) * 2 %}\n",
            "{% if y | cos > 0 %}{{ z }}{% endif %}\n",
            "{% for i in items %}{{ i | tan }}{% endfor %}\n",
        );
        let fs = lint(src, "t.j2").unwrap();
        assert_eq!(names(&fs), vec!["sin", "cos", "tan"]);
    }

    #[test]
    fn finds_trig_in_filter_arguments() {
        // `{{ x | nc_fixed(sin_y | round) }}` —— 过滤器参数里的 trig 也要查
        let src = "{{ 1 | default(2 | sin) }}";
        let fs = lint(src, "t.j2").unwrap();
        assert_eq!(names(&fs), vec!["sin"]);
    }

    #[test]
    fn syntax_error_surfaces() {
        assert!(lint("{{ unclosed", "bad.j2").is_err());
    }

    /// 遍历完整性：把 `| sin` 放进每一种语句/表达式形态，确保 walker 不漏踩。
    /// 这些用例也守护「新增表达式变体时必须同步 walk_expr」——漏踩会让 lint 静默失明。
    #[test]
    fn walks_every_statement_form() {
        let src = concat!(
            "{% set a = 1 %}\n",
            "{% set b %}x{{ p | sin }}{% endset %}\n",
            "{% with c = q | sin %}{{ c }}{% endwith %}\n",
            "{% if (r | sin) > 0 %}A{% else %}B{% endif %}\n",
            "{% for i in xs if (i | sin) > 0 %}{{ i }}{% else %}none{% endfor %}\n",
            "{% for j in ys %}{% continue %}{% break %}{% endfor %}\n",
            "{% autoescape true %}{{ s | sin }}{% endautoescape %}\n",
            "{% filter upper %}{{ t | cos }}{% endfilter %}\n",
            "{% block blk %}{{ u | tan }}{% endblock %}\n",
            "{% macro m(x = v | sin) %}{{ w | cos }}{% endmacro %}\n",
            "{% call m() %}{{ y | sin }}{% endcall %}\n",
            "{% do f(z | sin) %}\n",
            "{% include \"x.j2\" %}\n",
        );
        let fs = lint(src, "t.j2").unwrap();
        // set-block / with / if / for-filter / autoescape / filter / block / macro-default /
        // macro-body / call-block / do —— 共多个 sin/cos/tan
        assert!(
            fs.len() >= 10,
            "应踩到各语句体，实际 {}: {:?}",
            fs.len(),
            names(&fs)
        );
    }

    #[test]
    fn walks_every_expression_form() {
        let src = concat!(
            "{{ (a | sin) + (b | cos) }}\n",
            "{{ -(a | sin) }}\n",
            "{{ not (a | sin) }}\n",
            "{{ (a | sin) > 0 }}\n",
            "{{ (a | sin) if c else d }}\n",
            "{{ (a | sin) is defined }}\n",
            "{{ (lst | sin)[0] }}\n",
            "{{ (lst | sin)[1:2] }}\n",
            "{{ (obj | sin).attr }}\n",
            "{{ [a | sin, b | cos] }}\n",
            "{{ {'k': a | sin} }}\n",
            "{{ fn(a | sin) }}\n",
            // 链式比较才产生 `Expr::Compare`（单次比较是 BinOp，见 minijinja parser）
            "{{ 0 < (c | sin) < 1 }}\n",
        );
        let fs = lint(src, "t.j2").unwrap();
        assert!(
            fs.len() >= 12,
            "应踩到各表达式形态，实际 {}: {:?}",
            fs.len(),
            names(&fs)
        );
    }

    #[test]
    fn walks_remaining_forms() {
        // 补齐 walker 的剩余分支：SetBlock 过滤器、CallBlock 宏默认值、
        // slice 步长、Test 参数、kwarg 调用参数。
        let src = concat!(
            "{% set f | upper %}{{ p | sin }}{% endset %}\n",
            "{% call mm(a = v | sin) %}{{ y | cos }}{% endcall %}\n",
            "{{ (xs | sin)[1:2:3] }}\n",
            "{{ q is divisibleby(z | tan) }}\n",
            "{{ fn(a = b | sin) }}\n",
            // 行内宏签名的默认值：`{% call(a = ...) mm() %}`
            "{% call(x = q | sin) mm() %}{{ r }}{% endcall %}\n",
        );
        let fs = lint(src, "t.j2").unwrap();
        assert!(fs.len() >= 5, "实际 {}: {:?}", fs.len(), names(&fs));
    }

    #[test]
    fn walks_extends_and_import_names() {
        // extends / import / from-import 的名称表达式（此处为常量字符串，不应误报）
        let src = "{% extends \"base.j2\" %}{% import \"m.j2\" as m %}{% from \"m.j2\" import x %}";
        assert!(lint(src, "t.j2").unwrap().is_empty());
    }

    #[test]
    fn raw_block_is_skipped() {
        // raw 块内的 `| sin` 是字面文本，不得误报
        let src = "{% raw %}{{ x | sin }}{% endraw %}";
        assert!(lint(src, "t.j2").unwrap().is_empty());
    }
}
