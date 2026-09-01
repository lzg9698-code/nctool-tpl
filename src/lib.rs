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

mod error;
mod extract;
mod filters;
mod renderer;

// 公共 API 再导出
pub use error::TplError;
pub use extract::{
    extract_template_refs, extract_undeclared, extract_variables, parse, Ast, Variable,
};
pub use renderer::Renderer;

#[cfg(test)]
use error::{extract_identifier_at, extract_quoted, extract_undefined_var_name};

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
    fn from_import_alias_binds_alias_not_original() {
        // {% from "m" import helper as h %}：绑定的是别名 h（对齐 minijinja 语义），
        // helper 与 h 都不应记为未声明变量
        let src = r#"{% from "macros.j2" import helper as h %}{{ h() }}"#;
        let vars = extract_undeclared(&parse(src, "alias.j2").unwrap());
        assert!(
            vars.iter().all(|v| v.name != "h" && v.name != "helper"),
            "导入绑定不应记为未声明: {vars:?}"
        );
        // 渲染端对齐：别名可调用（严格模式不报 undefined）
        let mut r = Renderer::new();
        r.add_template("macros.j2", "{% macro helper() %}H{% endmacro %}")
            .unwrap();
        let out = r.render(src, "alias.j2", &minijinja::context! {}).unwrap();
        assert_eq!(out, "H");
    }

    #[test]
    fn set_inside_block_does_not_leak() {
        // block 体是独立作用域（对齐 minijinja VM 的帧语义）：
        // 块内 set 的名字块外不可见，块外引用按未声明处理
        let src = "{% block prep %}{% set feed = 0.1 %}{% endblock %}G1 F{{ feed }}";
        let vars = extract_undeclared(&parse(src, "blk.j2").unwrap());
        assert!(
            vars.iter().any(|v| v.name == "feed" && !v.optional),
            "块内 set 不应消除块外引用的未声明性: {vars:?}"
        );
        // 渲染端对齐：严格模式报未定义；宽松模式留空（G1 F，而非 G1 F0.1）
        let mut strict = Renderer::new();
        strict.add_template("blk.j2", src).unwrap();
        assert!(matches!(
            strict.render_template("blk.j2", &minijinja::context! {}),
            Err(TplError::UndefinedVariable { .. })
        ));
        let mut lenient = Renderer::new().with_lenient();
        lenient.add_template("blk.j2", src).unwrap();
        let out = lenient
            .render_template("blk.j2", &minijinja::context! {})
            .unwrap();
        assert_eq!(out, "G1 F");
    }

    #[test]
    fn undefined_var_in_included_template_not_recovered_from_main_source() {
        // extends 的父模板块内未定义变量：错误可能未包装直接冒泡（携带父模板
        // 名与字节范围），此时不能用主（子）模板源码恢复变量名（范围错位会
        // 得到无关标识符，宁缺毋错）
        let mut r = Renderer::new();
        r.add_template("base.j2", "{% block c %}{{ missing }}{% endblock %}")
            .unwrap();
        r.add_template("child.j2", "{% extends \"base.j2\" %}")
            .unwrap();
        let err = r
            .render_template("child.j2", &minijinja::context! {})
            .unwrap_err();
        match err {
            TplError::UndefinedVariable { name, variable, .. } => {
                assert_eq!(name, "base.j2");
                assert_eq!(variable, "", "不应从子模板源码错误恢复变量名");
            }
            TplError::Render { .. } => {
                // minijinja 将子模板错误包为 BadInclude/EvalBlock 时，顶层即
                // Render 变体、不触发变量名恢复——同样符合"不错位恢复"
            }
            other => panic!("应为 UndefinedVariable/Render: {other:?}"),
        }
        // include 路径：子模板错误被包为 BadInclude，消息可定位子模板名
        let mut r2 = Renderer::new();
        r2.add_template("sub.j2", "{{ missing }}").unwrap();
        r2.add_template("main.j2", "{% include \"sub.j2\" %}")
            .unwrap();
        let err = r2
            .render_template("main.j2", &minijinja::context! {})
            .unwrap_err();
        match err {
            TplError::Render { name, message } => {
                assert_eq!(name, "main.j2");
                assert!(message.contains("sub.j2"), "消息应可定位子模板: {message}");
            }
            other => panic!("应为 Render（BadInclude 包装）: {other:?}"),
        }
    }

    #[test]
    fn parse_error_column_counts_chars_not_bytes() {
        // 列号按字符计（与 Variable.col 的 minijinja span 口径一致）：
        // 两个仅前缀字节构成不同、字符构成相同的模板，错误列应相同
        let a = parse("( xx ) {{ 1 + }}", "a.j2").unwrap_err();
        let b = parse("( 中文 ) {{ 1 + }}", "b.j2").unwrap_err();
        let (la, ca) = match a {
            TplError::Parse { line, col, .. } => (line, col),
            other => panic!("应为 Parse 错误: {other:?}"),
        };
        let (lb, cb) = match b {
            TplError::Parse { line, col, .. } => (line, col),
            other => panic!("应为 Parse 错误: {other:?}"),
        };
        assert_eq!(la, lb);
        assert!(ca > 1, "应携带字节范围（col 非回退值 1）");
        assert_eq!(
            ca, cb,
            "等字符数前缀（'( xx ) ' 与 '( 中文 ) '）后的同一语法错误，列号应按字符口径一致"
        );
    }

    #[test]
    fn extract_template_refs_static_names() {
        // 静态（字符串字面量）引用按出现顺序去重；动态引用（变量名）忽略
        let ast = parse(
            "{% include \"a.j2\" %}{% include \"a.j2\" %}{% include name %}",
            "r.j2",
        )
        .unwrap();
        assert_eq!(extract_template_refs(&ast), vec!["a.j2".to_string()]);

        // 嵌套语句体内与 import/from 同样收集
        let ast = parse(
            "{% for i in items %}{% include \"b.j2\" %}{% endfor %}",
            "r2.j2",
        )
        .unwrap();
        assert_eq!(extract_template_refs(&ast), vec!["b.j2".to_string()]);

        let ast = parse("{% from \"d.j2\" import x %}", "r3.j2").unwrap();
        assert_eq!(extract_template_refs(&ast), vec!["d.j2".to_string()]);
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

    #[test]
    fn nc_pad_huge_value_rejects_overflow() {
        // 饱和转换（1e300 → i64::MAX）应报错，而非静默输出错误数字
        let r = Renderer::new();
        let ctx = minijinja::context! { x => 1e300 };
        let err = r.render("{{ x | nc_pad(8) }}", "p.j2", &ctx).unwrap_err();
        assert!(err.to_string().contains("整数范围"), "应报溢出错误: {err}");
    }

    #[test]
    fn nc_fixed_decimals_overflow_rejected() {
        // 超大小数位会触发巨量分配/进程 abort：应报错
        let r = Renderer::new();
        let ctx = minijinja::context! { x => 21.0 };
        let err = r
            .render("{{ x | nc_fixed(999999999) }}", "f.j2", &ctx)
            .unwrap_err();
        assert!(err.to_string().contains("小数位"), "应报上限错误: {err}");
    }

    #[test]
    fn nc_pad_width_overflow_rejected() {
        let r = Renderer::new();
        let ctx = minijinja::context! { n => 1.0 };
        let err = r
            .render("{{ n | nc_pad(999999999) }}", "p2.j2", &ctx)
            .unwrap_err();
        assert!(err.to_string().contains("宽度"), "应报上限错误: {err}");
    }

    #[test]
    fn add_template_same_name_replaces_silently() {
        // 同名注册静默替换（后者覆盖前者）
        let mut r = Renderer::new();
        r.add_template("dup.j2", "ONE").unwrap();
        r.add_template("dup.j2", "TWO {{ x }}").unwrap();
        let out = r
            .render_template("dup.j2", &minijinja::context! { x => 2 })
            .unwrap();
        assert_eq!(out, "TWO 2");
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
