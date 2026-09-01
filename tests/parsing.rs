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

/// path_loader 模板名不逃逸根目录：盘符前缀 / 绝对路径 / `..` 段一律视为
/// 模板不存在（前两者由本库校验拦截，`..` 由引擎 safe_join 拦截）。
#[test]
fn path_loader_rejects_escape_names() {
    let dir = std::env::temp_dir().join(format!("nctool_tpl_esc_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("in.j2"), "IN").unwrap();

    let mut renderer = Renderer::new();
    renderer.set_path_loader(&dir);
    let ctx = minijinja::context! {};
    for name in [
        "C:/evil",
        "C:\\evil",
        "/etc/passwd",
        "\\evil",
        "../in.j2",
        "",
    ] {
        let err = renderer.render_template(name, &ctx).unwrap_err();
        assert!(
            matches!(err, TplError::TemplateNotFound { .. }),
            "{name:?} 应视为模板不存在: {err:?}"
        );
    }
    // 目录内正常加载不受影响
    let out = renderer.render_template("in.j2", &ctx).unwrap();
    assert_eq!(out, "IN");

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
        TplError::TemplateNotFound { name, .. } => {
            assert_eq!(name, "nope.j2");
        }
        _ => panic!("应为 TemplateNotFound 错误"),
    }
}

// -----------------------------------------------------------------------
// 渲染边界 case
// -----------------------------------------------------------------------

/// for 循环 + loop.index 渲染。
#[test]
fn render_for_loop_with_index() {
    let src = "{% for i in items %}{{ loop.index }}:{{ i }} {% endfor %}";
    let r = Renderer::new();
    let ctx = minijinja::context! { items => vec!["a", "b", "c"] };
    let out = r.render(src, "for.j2", &ctx).unwrap();
    assert_eq!(out, "1:a 2:b 3:c ");
}

/// if / elif / else 分支渲染。
#[test]
fn render_if_elif_else() {
    let src = "{% if x > 10 %}big{% elif x > 5 %}medium{% else %}small{% endif %}";
    let r = Renderer::new();
    let ctx1 = minijinja::context! { x => 20 };
    assert_eq!(r.render(src, "if.j2", &ctx1).unwrap(), "big");
    let ctx2 = minijinja::context! { x => 7 };
    assert_eq!(r.render(src, "if.j2", &ctx2).unwrap(), "medium");
    let ctx3 = minijinja::context! { x => 1 };
    assert_eq!(r.render(src, "if.j2", &ctx3).unwrap(), "small");
}

/// 宏定义与调用渲染。
#[test]
fn render_macro_call() {
    let src = "{% macro line(x, y) %}G1 X{{ x }} Y{{ y }}{% endmacro %}{{ line(10, 20) }}";
    let r = Renderer::new();
    let ctx = minijinja::context! {};
    let out = r.render(src, "macro.j2", &ctx).unwrap();
    assert_eq!(out, "G1 X10 Y20");
}

/// loop_controls：continue 跳过偶数，break 在第 3 项停止。
#[test]
fn render_loop_controls_continue_and_break() {
    let src = "{% for i in range(1, 6) %}{% if i % 2 == 0 %}{% continue %}{% endif %}{{ i }}{% if i == 3 %}{% break %}{% endif %}{% endfor %}";
    let r = Renderer::new();
    let ctx = minijinja::context! {};
    let out = r.render(src, "loop.j2", &ctx).unwrap();
    // 1（奇数，输出），2（continue 跳过），3（奇数，输出后 break）
    assert_eq!(out, "13");
}

/// 空白控制 `{%-` / `-%}` 渲染。
#[test]
fn render_whitespace_control() {
    let src = "A\n{%- set x = 1 -%}\nB\n{{- x -}}\nC";
    let r = Renderer::new();
    let ctx = minijinja::context! {};
    let out = r.render(src, "ws.j2", &ctx).unwrap();
    // lstrip 吃掉 A 后的换行，trim 吃掉 set 后的换行
    assert_eq!(out, "AB1C");
}

/// 多行模板语法错误应定位到正确行。
#[test]
fn parse_error_multiline_line_number() {
    let src = "G1 X10\nG1 Y20\n{{ (1 + 2 }}\nM3";
    let err = parse(src, "multi.j2").unwrap_err();
    match err {
        TplError::Parse { line, .. } => {
            assert_eq!(line, 3, "应定位到第 3 行，实际 {line}");
        }
        _ => panic!("应为 Parse 错误"),
    }
}

/// 所有数学过滤器在正常值下可渲染。
#[test]
fn all_math_filters_render() {
    let src = "{{ 4 | sqrt }} {{ 2 | exp }} {{ 10 | ln }} {{ 100 | log10 }} {{ 2 | pow(3) }} {{ 1.5 | floor }} {{ 1.5 | ceil }} {{ 0 | sin }} {{ 0 | cos }}";
    let r = Renderer::new();
    let ctx = minijinja::context! {};
    let out = r.render(src, "math.j2", &ctx).unwrap();
    assert!(out.contains("2")); // sqrt(4)
    assert!(out.contains("8")); // pow(2,3)
    assert!(out.contains("1")); // floor(1.5)
    assert!(out.contains("2")); // ceil(1.5)
}
