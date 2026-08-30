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

/// 多模板全链路：注册 include/extends 模板 + 变量提取 + 渲染。
#[test]
fn multi_template_full_pipeline() {
    let mut renderer = Renderer::new();
    renderer
        .add_template("header.j2", "O{{ prog }} ({{ name }})")
        .unwrap();
    renderer
        .add_template(
            "main.j2",
            "{% include \"header.j2\" %}\nG1 X{{ diameter / 2 }}",
        )
        .unwrap();

    // 主模板变量提取（include 的子模板变量不会进入主模板 AST）
    let ast = parse(
        "{% include \"header.j2\" %}\nG1 X{{ diameter / 2 }}",
        "main.j2",
    )
    .unwrap();
    let undeclared: Vec<String> = extract_undeclared(&ast)
        .into_iter()
        .map(|v| v.name)
        .collect();
    assert_eq!(undeclared, vec!["diameter".to_string()]);

    // 渲染：include 子模板的变量由上下文提供
    let ctx = minijinja::context! {
        prog => 1000,
        name => "DEMO",
        diameter => 42.0,
    };
    let out = renderer.render_template("main.j2", &ctx).unwrap();
    assert!(out.contains("O1000 (DEMO)"));
    assert!(out.contains("G1 X21"));
}

/// 从文件系统目录加载模板（path_loader）。
#[test]
fn path_loader_from_directory() {
    let dir = std::env::temp_dir().join(format!("nctool_tpl_test_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("sub.j2"), "SUB{{ v }}").unwrap();
    std::fs::write(dir.join("main.j2"), "{% include \"sub.j2\" %} END").unwrap();

    let mut renderer = Renderer::new();
    renderer.set_path_loader(&dir);
    let ctx = minijinja::context! { v => 7.0 };
    let out = renderer.render_template("main.j2", &ctx).unwrap();
    assert_eq!(out, "SUB7.0 END");

    // 清理
    let _ = std::fs::remove_dir_all(&dir);
}

/// 未注册模板渲染应报错，且错误信息可定位到模板名。
#[test]
fn render_missing_template_error() {
    let renderer = Renderer::new();
    let ctx = minijinja::context! {};
    let err = renderer.render_template("nope.j2", &ctx).unwrap_err();
    match err {
        TplError::Render { name, message } => {
            assert_eq!(name, "nope.j2");
            assert!(!message.is_empty());
        }
        _ => panic!("应为 Render 错误"),
    }
}
