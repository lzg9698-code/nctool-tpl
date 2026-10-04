use nctool_tpl::{Renderer, TplError};
fn renderer() -> Renderer {
    let mut r = Renderer::new();
    nctool_plugin_math::configure(&mut r).unwrap();
    nctool_plugin_nc::configure(&mut r).unwrap();
    r
}
#[test]
fn nested_include_error_reports_root_cause() {
    // 回归：此前嵌套 include 的错误消息只有外层包装
    // `could not render include: error in "sub.j2" (in main.j2:2)`，
    // 根因（子模板第几行、什么错）被丢弃，实测无法定位问题。
    let mut r = renderer();
    r.add_template("sub.j2", "G1 X{{ x | nc_fixed(3) }}\n")
        .unwrap();
    r.add_template("main.j2", "A\n{% include \"sub.j2\" %}\nB\n")
        .unwrap();
    // 字符串喂给 nc_fixed → 渲染期错误，且发生在子模板内
    let err = r
        .render_template("main.j2", &minijinja::context! { x => "abc" })
        .unwrap_err();
    let message = err.to_string();
    assert!(message.contains(" ← "), "消息应包含根因链: {message}");
    // 根因段必须能定位到子模板（模板名 + 行号）
    assert!(
        message.contains("sub.j2"),
        "根因段应定位到子模板: {message}"
    );
    // 最外层分类与模板名保持不变（现有调用方按此判断）
    match err {
        TplError::Render { name, .. } => assert_eq!(name, "main.j2"),
        other => panic!("应为 Render: {other:?}"),
    }
}

#[test]
fn shallow_error_has_no_chain_suffix() {
    // 无嵌套时不应画蛇添足地追加 ` ← `（保持既有消息格式不变）
    let mut r = renderer();
    r.add_template("solo.j2", "G1 X{{ x | nc_fixed(3) }}\n")
        .unwrap();
    let err = r
        .render_template("solo.j2", &minijinja::context! { x => "abc" })
        .unwrap_err();
    assert!(
        !err.to_string().contains(" ← "),
        "单层错误不应带根因链: {err}"
    );
}

#[test]
fn nc_pad_huge_value_rejects_overflow() {
    // 饱和转换（1e300 → i64::MAX）应报错，而非静默输出错误数字
    let r = renderer();
    let ctx = minijinja::context! { x => 1e300 };
    let err = r.render("{{ x | nc_pad(8) }}", "p.j2", &ctx).unwrap_err();
    assert!(err.to_string().contains("整数范围"), "应报溢出错误: {err}");
}

#[test]
fn nc_fixed_decimals_overflow_rejected() {
    // 超大小数位会触发巨量分配/进程 abort：应报错
    let r = renderer();
    let ctx = minijinja::context! { x => 21.0 };
    let err = r
        .render("{{ x | nc_fixed(999999999) }}", "f.j2", &ctx)
        .unwrap_err();
    assert!(err.to_string().contains("小数位"), "应报上限错误: {err}");
}

#[test]
fn nc_pad_width_overflow_rejected() {
    let r = renderer();
    let ctx = minijinja::context! { n => 1.0 };
    let err = r
        .render("{{ n | nc_pad(999999999) }}", "p2.j2", &ctx)
        .unwrap_err();
    assert!(err.to_string().contains("宽度"), "应报上限错误: {err}");
}

#[test]
fn strict_mode_lenient_mode_render_consistency() {
    // 提供完整参数时，严格与宽松模式输出应一致
    let r_strict = renderer();
    let r_lenient = renderer().with_lenient();
    let ctx = minijinja::context! { x => 21.0, y => 15.5 };
    let src = "X{{ x | nc_fixed(3) }} Y{{ y | nc_fixed(3) }}";
    let a = r_strict.render(src, "c.j2", &ctx).unwrap();
    let b = r_lenient.render(src, "c.j2", &ctx).unwrap();
    assert_eq!(a, b);
}

#[test]
fn render_with_nan_in_context_rejects() {
    // 上下文中传入 NaN，渲染时应报错（数学过滤器或直接输出）
    let r = renderer();
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
    let r = renderer();
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
    let r = renderer();
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
    let r = renderer();
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
    let r = renderer();
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
    let r = renderer();
    let ctx = minijinja::context! { x => -21.5 };
    let out = r
        .render("X{{ x | nc_fixed(3) }} X{{ x | nc_strip }}", "neg.j2", &ctx)
        .unwrap();
    assert_eq!(out, "X-21.500 X-21.5");
}

/// `nc_signed`：正数与零都带 `+`，负数带 `-`（对应源项目的 `fmt_coord`）。
#[test]
fn nc_signed_forces_explicit_plus() {
    let r = renderer();
    let ctx = minijinja::context! { p => 21.0, n => -4.5, z => 0.0 };
    let out = r
        .render(
            "{{ p | nc_signed(3) }} {{ n | nc_signed(3) }} {{ z | nc_signed(3) }}",
            "s.j2",
            &ctx,
        )
        .unwrap();
    assert_eq!(out, "+21.000 -4.500 +0.000");
}

/// 负零归一到 `+0.000`：控制器对 `-0.000` 处理不一致，且图纸上无意义。
#[test]
fn nc_signed_normalizes_negative_zero() {
    let r = renderer();
    let ctx = minijinja::context! { z => -0.0 };
    let out = r.render("{{ z | nc_signed(3) }}", "nz.j2", &ctx).unwrap();
    assert_eq!(out, "+0.000");
}

/// `nc_signed` 与 `nc_fixed` 的唯一差别就是正号——回归守护两者混淆。
#[test]
fn nc_signed_differs_from_nc_fixed_only_by_sign() {
    let r = renderer();
    let ctx = minijinja::context! { p => 21.0 };
    let signed = r.render("{{ p | nc_signed(3) }}", "a.j2", &ctx).unwrap();
    let fixed = r.render("{{ p | nc_fixed(3) }}", "b.j2", &ctx).unwrap();
    assert_eq!(signed, "+21.000");
    assert_eq!(fixed, "21.000");
}

#[test]
fn nc_signed_rejects_non_finite() {
    let r = renderer();
    let ctx = minijinja::context! { x => f64::NAN };
    assert!(r.render("{{ x | nc_signed(3) }}", "n.j2", &ctx).is_err());
}

#[test]
fn nc_signed_decimal_limit() {
    let r = renderer();
    let ctx = minijinja::context! { x => 1.0 };
    assert!(r.render("{{ x | nc_signed(33) }}", "d.j2", &ctx).is_err());
}

#[test]
fn nc_filters_reject_non_finite() {
    let r = renderer();
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
    let r = renderer();
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
    let r = renderer();
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
fn nc_pad_fractional_rejects() {
    // 回归：1.7 经 trunc() 会静默变成 O0001 —— 程序号写错却不报错，
    // 是"渲染成功但结果错误"中最危险的一类。必须拒绝而非截断。
    let r = renderer();
    let ctx = minijinja::context! { n => 1.7 };
    let err = r
        .render("O{{ n | nc_pad(4) }}", "padfrac.j2", &ctx)
        .unwrap_err();
    match err {
        TplError::Render { message, .. } => assert!(
            message.contains("不是整数"),
            "小数输入应被拒绝，实际: {message}"
        ),
        _ => panic!("nc_pad 小数输入应报错"),
    }
}

#[test]
fn nc_pad_accepts_integral_float() {
    // 整值浮点（1.0）应正常通过：JSON/CLI 常把 1 解析成 1.0，
    // 不能因小数拒绝而误伤这种常见表示。
    let r = renderer();
    let ctx = minijinja::context! { n => 1.0 };
    assert_eq!(
        r.render("O{{ n | nc_pad(4) }}", "ok.j2", &ctx).unwrap(),
        "O0001"
    );
}

#[test]
fn nc_pad_explicit_round_then_pad() {
    // 需要取整时显式转换仍可用（报错信息指引的路径必须真的走得通）
    let r = renderer();
    let ctx = minijinja::context! { n => 1.7 };
    assert_eq!(
        r.render("O{{ n | round | nc_pad(4) }}", "r.j2", &ctx)
            .unwrap(),
        "O0002"
    );
}

#[test]
fn nc_filters_combined_in_gcode() {
    // 模拟真实 G-code 场景：程序号 + 坐标 + 行号
    let r = renderer();
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
fn render_with_math_filters() {
    // 注意 Jinja 过滤器优先级高于算术：必须用括号把整体括起来再取整
    let src = "G1 X{{ (diameter / 2) | round(2) }} F{{ feed }} S{{ (2000 * 1.5) | ceil }}";
    let renderer = renderer();
    let ctx = minijinja::context! { diameter => 42.0, feed => 0.15 };
    let out = renderer.render(src, "gcode.j2", &ctx).unwrap();
    assert!(out.contains("X21.0"));
    assert!(out.contains("F0.15"));
    assert!(out.contains("S3000"));
}

// -----------------------------------------------------------------------
// 角度制三角函数（sin_d / cos_d / tan_d / asin_d / acos_d / atan_d）
// -----------------------------------------------------------------------

/// 度制过滤器必须与弧度制裸过滤器**结果不同**——这是防撞刀的核心断言。
#[test]
fn degree_trig_matches_expected_values() {
    let r = renderer();
    let ctx = minijinja::context! { a => 30.0, b => 60.0, c => 45.0 };
    let out = r
        .render(
            "{{ a | sin_d | round(6) }} {{ b | cos_d | round(6) }} {{ c | tan_d | round(6) }}",
            "d.j2",
            &ctx,
        )
        .unwrap();
    assert_eq!(out, "0.5 0.5 1.0");
}

/// 反三角以**度**输出（而非弧度）：`asin_d(0.5)` → `30`，不是 `0.5236`。
#[test]
fn inverse_degree_trig_outputs_degrees() {
    let r = renderer();
    let ctx = minijinja::context! { h => 0.5, one => 1.0, zero => 0.0 };
    let out = r
        .render(
            "{{ h | asin_d | round(6) }} {{ h | acos_d | round(6) }} \
                 {{ one | atan_d | round(6) }} {{ zero | atan_d | round(6) }}",
            "i.j2",
            &ctx,
        )
        .unwrap();
    assert_eq!(out, "30.0 60.0 45.0 0.0");
}

/// 回归守护：30 度与 30 弧度结果迥异。若有人把 `sin_d` 误实现为
/// `v.sin()`（漏掉 `to_radians`），此断言会失败。
#[test]
fn degree_and_radian_trig_are_distinguishable() {
    let r = renderer();
    let ctx = minijinja::context! { a => 30.0 };
    let deg = r.render("{{ a | sin_d }}", "deg.j2", &ctx).unwrap();
    let rad = r.render("{{ a | sin }}", "rad.j2", &ctx).unwrap();
    assert_ne!(deg, rad, "度制与弧度制必须产生不同结果");
    let deg_v: f64 = deg.parse().unwrap();
    assert!((deg_v - 0.5).abs() < 1e-9, "sin_d(30) 应约等于 0.5");
}

/// `_d` 过滤器沿用有限性防线：定义域外输入必须报错而非写入 NaN。
#[test]
fn degree_trig_rejects_non_finite() {
    let r = renderer();
    // asin 定义域为 [-1, 1]，2.0 产出 NaN
    let ctx = minijinja::context! { x => 2.0 };
    assert!(r.render("{{ x | asin_d }}", "bad.j2", &ctx).is_err());
    // tan_d(90) 在 f64 下是有限的大数（非 Inf），应可渲染——
    // 这里断言的是"不 panic"，实际工艺应在参数校验层拦下 90 度。
    let ctx90 = minijinja::context! { x => 90.0 };
    assert!(r.render("{{ x | tan_d }}", "t90.j2", &ctx90).is_ok());
}

#[test]
fn render_rejects_nonfinite_math() {
    let renderer = renderer();
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
fn all_math_filters_render() {
    let r = renderer();
    let ctx = minijinja::context! {};

    // 统一按**数值**比较：这些过滤器返回的是浮点（`sqrt(4)` 渲染成 `"2.0"`），
    // 逐字符比对会把平台相关的浮点格式化文本写进断言。
    for (src, want) in [
        ("{{ 4 | sqrt }}", 2.0),
        ("{{ 100 | log10 }}", 2.0),
        ("{{ 2 | pow(3) }}", 8.0),
        ("{{ 1.5 | floor }}", 1.0),
        ("{{ 1.5 | ceil }}", 2.0),
        ("{{ 0 | sin }}", 0.0),
        ("{{ 0 | cos }}", 1.0),
        ("{{ 2 | exp }}", std::f64::consts::E.powi(2)),
        ("{{ 10 | ln }}", 10f64.ln()),
    ] {
        let out = r.render(src, "math.j2", &ctx).unwrap();
        let got: f64 = out
            .trim()
            .parse()
            .unwrap_or_else(|e| panic!("{src} 输出不是数值 {out:?}: {e}"));
        assert!((got - want).abs() < 1e-9, "{src}: got {got}, want {want}");
    }
}
