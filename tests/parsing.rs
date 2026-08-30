//! 集成测试：从 crate 外部使用公开 API，验证「解析 → 变量提取 → 渲染」全链路。

use nctool_tpl::{extract_undeclared, extract_variables, parse, Renderer, TplError};

#[test]
fn full_pipeline_on_gcode_template() {
    let src = r#"{% set feed = 0.15 %}
O1000 ( {{ program_name }} )
G0 X{{ start_x }}
G1 X{{ diameter / 2 }} F{{ feed | round(2) }}
{% for hole in holes %}
  G81 X{{ hole.x }} Y{{ hole.y }}
{% endfor %}"#;

    let ast = parse(src, "pipe.j2").expect("解析失败");

    let all: Vec<String> = extract_variables(&ast)
        .into_iter()
        .map(|v| v.name)
        .collect();
    assert!(all.contains(&"diameter".to_string()));
    assert!(all.contains(&"holes".to_string()));
    assert!(all.contains(&"feed".to_string()));

    let undeclared: Vec<String> = extract_undeclared(&ast)
        .into_iter()
        .map(|v| v.name)
        .collect();
    assert_eq!(
        undeclared,
        vec![
            "program_name".to_string(),
            "start_x".to_string(),
            "diameter".to_string(),
            "holes".to_string()
        ]
    );

    let renderer = Renderer::new();
    let ctx = minijinja::context! {
        program_name => "PIPE",
        start_x => 10.0,
        diameter => 42.0,
        holes => vec![minijinja::context! { x => 1.0, y => 2.0 }],
    };
    let out = renderer.render(src, "pipe.j2", &ctx).expect("渲染失败");
    assert!(out.contains("O1000 ( PIPE )"));
    assert!(out.contains("X21.0"));
    assert!(out.contains("G81 X1.0 Y2.0"));
}

#[test]
fn syntax_error_surfaces() {
    let err = parse("G0 X10\n{{ oops \nG1 Z5", "bad.j2").unwrap_err();
    match err {
        TplError::Parse { line, .. } => assert!(line >= 2, "应定位到第 2 行，实际 {line}"),
        _ => panic!("应为解析错误"),
    }
}

/// 覆盖更多 AST 分支：macro 默认值 / filter block / set block / with / slice /
/// if / 三元表达式 / import / from-import。
#[test]
fn extract_covers_more_branches() {
    let src = r#"{% macro box(w = default_w) %}{{ w * 2 }}{% endmacro %}
{{ box(10) }}
{% filter upper %}{{ greeting }}{% endfilter %}
{% set block %}set-{{ name }}{% endset %}
{% with x = items[0] %}{{ x }}{% endwith %}
{{ items[1:3] | length }}
{% if cond %}Y{% else %}N{% endif %}
{{ "a" if flag else "b" }}
{% import "macros.j2" as m %}{{ m.fn() }}
{% from "macros.j2" import helper %}{{ helper() }}"#;
    let ast = parse(src, "branches.j2").unwrap();
    let undeclared: Vec<String> = extract_undeclared(&ast)
        .into_iter()
        .map(|v| v.name)
        .collect();
    // 期望：仅外部需提供的变量；w/box/m/helper 为模板局部，upper/length 为过滤器名，
    // "a"/"b"、"macros.j2" 为字面量，均不应出现。
    assert_eq!(
        undeclared,
        vec![
            "default_w".to_string(),
            "greeting".to_string(),
            "name".to_string(),
            "items".to_string(),
            "cond".to_string(),
            "flag".to_string()
        ]
    );
}

/// 数学过滤器对 NaN/Inf 必须报渲染错误，而不是把非法值写入 G-code。
#[test]
fn render_rejects_nonfinite_math() {
    let renderer = Renderer::new();
    let ctx = minijinja::context! {};

    // sqrt(-1) -> NaN
    let err = renderer
        .render("G1 X{{ -1 | sqrt }}", "gcode.j2", &ctx)
        .unwrap_err();
    match err {
        TplError::Render { message, .. } => {
            assert!(
                message.contains("非有限数") || message.contains("NaN"),
                "NaN 应触发渲染错误: {message}"
            );
        }
        _ => panic!("应为渲染错误"),
    }

    // ln(0) -> -Inf
    let err = renderer
        .render("G1 X{{ 0 | ln }}", "gcode.j2", &ctx)
        .unwrap_err();
    match err {
        TplError::Render { message, .. } => {
            assert!(
                message.contains("非有限数") || message.contains("NaN"),
                "Inf 应触发渲染错误: {message}"
            );
        }
        _ => panic!("应为渲染错误"),
    }
}

/// 解析错误应带真实列号定位，而非恒为 1 的占位值。
#[test]
fn parse_error_locates_column() {
    let err = parse("G0 X10\n{{ (1 + 2 }}", "bad.j2").unwrap_err();
    match err {
        TplError::Parse { line, col, .. } => {
            assert_eq!(line, 2, "应定位到第 2 行，实际 {line}");
            assert!(col > 3, "应定位到 `}}` 附近列号，实际 col={col}");
        }
        _ => panic!("应为解析错误"),
    }
}

/// 未声明变量区分「可选 / 必选」：有 default 兜底的为可选，否则为必选。
#[test]
fn undeclared_distinguishes_optional_required() {
    let src = r#"{% set feed = default_feed | default(0.15) %}
G1 X{{ diameter / 2 }} F{{ feed }}
G1 Z{{ depth | default(-5) }}
{% if coolant is defined %}M8{% endif %}"#;
    let ast = parse(src, "opt.j2").unwrap();
    let vars = extract_undeclared(&ast);
    let opt = |n: &str| {
        vars.iter()
            .find(|v| v.name == n)
            .unwrap_or_else(|| panic!("缺少变量 {n}"))
            .optional
    };

    // 有 default 兜底 / 仅 defined 检查 → 可选
    assert!(opt("default_feed"), "default_feed 应可选");
    assert!(opt("depth"), "depth 应可选（default 兜底）");
    assert!(opt("coolant"), "coolant 应可选（defined 检查）");
    // 无任何兜底 → 必选
    assert!(!opt("diameter"), "diameter 应必选");
}

/// extract_variables 与 extract_undeclared 中 optional 语义一致。
#[test]
fn variables_and_undeclared_share_optional() {
    let src = "{{ required_x }}{{ optional_y | default(1) }}";
    let ast = parse(src, "opt2.j2").unwrap();
    let all = extract_variables(&ast);
    let undeclared = extract_undeclared(&ast);

    let all_opt = |n: &str| all.iter().find(|v| v.name == n).unwrap().optional;
    let un_opt = |n: &str| undeclared.iter().find(|v| v.name == n).unwrap().optional;

    assert!(!all_opt("required_x") && !un_opt("required_x"));
    assert!(all_opt("optional_y") && un_opt("optional_y"));
}
