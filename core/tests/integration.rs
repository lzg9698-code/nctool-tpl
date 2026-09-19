//! nctool-core 集成测试：通过公共 API 验证端到端能力。

use std::path::Path;

use nctool_core::machine::MachinePreset;
use nctool_core::pipeline::{GCodeGenerator, GenerationOptions, PipelineError};
use nctool_core::registry::{TemplateCategory, TemplateRegistry};
use nctool_core::{ParamKind, ParamValue, ParameterSet};

/// golden 文件路径（workspace 根 `tests/golden/`，与 CLI golden 同目录）。
fn golden_path(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("tests")
        .join("golden")
        .join(name)
}

/// 把 CRLF / 孤立 CR 统一折算成 LF。
///
/// golden 基线在仓库中恒为 LF（见根 `.gitattributes` 的 `* text=auto eol=lf`），
/// 但检出配置异常、手工编辑，或 `NCTOOL_UPDATE_GOLDEN=1` 在 Windows 下刷新，
/// 都可能把 CRLF 写进基线。比较前统一口径，让 golden 断言与平台、git 配置无关
/// （ROADMAP E2.4）。
fn normalize_newlines(s: &str) -> String {
    s.replace("\r\n", "\n").replace('\r', "\n")
}

/// 断言输出与 golden 文件一致；设置环境变量 `NCTOOL_UPDATE_GOLDEN=1` 时重新
/// 写入 golden 文件（**仅用于人工确认后的基线刷新，切勿在 CI 更新**）。
///
/// 比较与刷新两侧都过 [`normalize_newlines`]，保证基线永远是 LF。
fn assert_golden(name: &str, actual: &str) {
    let actual = normalize_newlines(actual);
    let path = golden_path(name);
    if std::env::var_os("NCTOOL_UPDATE_GOLDEN").is_some() {
        // CI 下硬拦：刷新分支会写文件并 `return`，跳过本函数的**全部**断言，
        // 21 组 golden 就静默退化成「跑得通即通过」。当前 workflow 没有设这个
        // 变量，但 `env:`、`.cargo/config.toml` 或某个 runner 的默认环境都可能
        // 把它带进来 —— 一旦带进来，没有任何东西会报错，只会从此全是绿的。
        assert!(
            std::env::var_os("CI").is_none(),
            "CI 环境禁止刷新 golden 基线（检测到 NCTOOL_UPDATE_GOLDEN）：\
             刷新会跳过全部 golden 断言。如需更新基线，请在本地跑并人工 diff 复核后再提交"
        );
        std::fs::create_dir_all(path.parent().expect("golden 路径应有父目录"))
            .expect("创建 golden 目录失败");
        // 刷新时同样落 LF，避免把平台行尾固化进仓库
        std::fs::write(&path, actual.as_bytes()).expect("写入 golden 文件失败");
        return;
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("读取 golden 文件失败 {}: {err}", path.display()));
    let expected = normalize_newlines(&expected);
    assert_eq!(actual, expected, "golden 不匹配: {}", path.display());
}

/// golden 基线不得含 CRLF/CR（ROADMAP E2.4 / B4.2）。
///
/// 归一化让比较本身不受行尾影响，但基线文件仍应保持纯 LF：否则 diff 噪声、
/// 跨平台检出不一致等问题会卷土重来。此用例在文件落盘层面把住关口。
#[test]
fn golden_files_are_lf_only() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("tests")
        .join("golden");
    let mut checked = 0;
    for entry in std::fs::read_dir(&dir).expect("读取 golden 目录失败") {
        let path = entry.expect("遍历 golden 目录失败").path();
        if !path.is_file() {
            continue;
        }
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("读取 {path:?} 失败: {e}"));
        assert!(
            !bytes.contains(&b'\r'),
            "golden 基线必须纯 LF，发现 CR: {path:?}（请用 LF 重新保存，或设 .gitattributes 后重新检出）"
        );
        checked += 1;
    }
    // 21 组正向（`.nc` + `.report.txt` 各 21）+ 3 份负向报告 = 45
    assert!(
        checked >= 45,
        "golden 目录应有 45 个文件（21 正向 ×2 + 3 负向），实际 {checked} 个"
    );
}

#[test]
fn end_to_end_custom_template_generation() {
    let mut g = GCodeGenerator::new();
    g.registry_mut()
        .add_memory(
            "face_drill",
            TemplateCategory::Drilling,
            "面钻孔",
            "X{{ x | nc_fixed(3) }} Y{{ y | nc_fixed(3) }} Z{{ depth | nc_fixed(3) }}",
            vec![
                nctool_core::validate::spec("x", ParamKind::Number, true, None, "X"),
                nctool_core::validate::spec("y", ParamKind::Number, true, None, "Y"),
                nctool_core::validate::spec("depth", ParamKind::Number, true, None, "Z"),
            ],
        )
        .unwrap();

    let mut ps = ParameterSet::new();
    ps.set_number("x", 10.0)
        .set_number("y", 20.0)
        .set_number("depth", -5.0);
    let machine = MachinePreset::Generic.config();
    let out = g
        .generate("face_drill", &ps, &machine, &GenerationOptions::default())
        .unwrap();
    assert_eq!(out.trim(), "X10.000 Y20.000 Z-5.000");
}

#[test]
fn builtin_program_header_with_wfl_machine() {
    // 内置模板 + WFL 机床配置
    let g = GCodeGenerator::new();
    let mut ps = ParameterSet::new();
    ps.set_number("prog", 42.0)
        .set_string("part_name", "GEAR_SHAFT");
    let wfl = MachinePreset::WflM65.config();
    let out = g
        .generate("program_header", &ps, &wfl, &GenerationOptions::default())
        .unwrap();
    // WFL 当前只覆盖通用模板键；至少验证预设身份与可生成性，不把 generic
    // 字节输出误当作 WFL 专属适配已经生效。
    assert_eq!(wfl.vendor, "WFL");
    assert!(out.contains("O0042"));
}

/// golden 基线矩阵：7 内置模板 × 3 机床预设 = 21 组。
///
/// 每组返回：模板名、文件茎名（`<模板>_<机床id>`）、机床预设、参数集。
/// 参数集与机床无关（模板引用的都是加工参数 + machine 系统变量）。
///
/// **机床维度当前是"平的"**：三个预设只在 `max_spindle_rpm` / `machine_type` /
/// `axes` / `vendor` / `model` 上有差异，而这些键**没有任何内置模板引用**；
/// 模板真正用到的 `program_prefix` / `units` / `coordinate_system` / `feed_mode`
/// 等来自共享的 `generic_config()`，三个预设完全一致。因此 21 份 `.nc` 实际只有
/// 7 份不同内容 —— 这不是测试写错，而是该维度在现有模板集下不含信息。
/// 保留 3 份的理由：任一预设改动**模板可见**的键时，对应的 `_wfl` / `_index`
/// 基线会立刻红。该性质由 `machine_dimension_is_currently_flat` 显式守住。
fn golden_cases() -> Vec<(&'static str, String, MachinePreset, ParameterSet)> {
    let presets = [
        MachinePreset::Generic,
        MachinePreset::WflM65,
        MachinePreset::IndexMs40,
    ];
    let mut cases = Vec::new();
    for preset in presets {
        let id = preset.id();

        let mut ps = ParameterSet::new();
        ps.set_integer("prog", 1).set_string("part_name", "DEMO");
        cases.push(("program_header", format!("program_header_{id}"), preset, ps));

        cases.push((
            "program_footer",
            format!("program_footer_{id}"),
            preset,
            ParameterSet::new(),
        ));

        let mut ps = ParameterSet::new();
        ps.set_integer("tool_num", 5)
            .set_integer("spindle_speed", 3000);
        cases.push(("tool_change", format!("tool_change_{id}"), preset, ps));

        let mut ps = ParameterSet::new();
        ps.set_number("x", 10.0).set_number("y", 20.0);
        cases.push(("safe_move", format!("safe_move_{id}"), preset, ps));

        let mut ps = ParameterSet::new();
        ps.set_number("x", 21.0)
            .set_number("y", 15.0)
            .set_number("depth", -10.0)
            .set_number("feed", 100.0);
        cases.push(("drill_cycle", format!("drill_cycle_{id}"), preset, ps));

        let mut ps = ParameterSet::new();
        ps.set_number("x0", 0.0)
            .set_number("y0", 0.0)
            .set_number("length", 50.0)
            .set_number("width", 25.0)
            .set_number("depth", -1.0)
            .set_number("feed", 200.0);
        cases.push(("facing", format!("facing_{id}"), preset, ps));

        let mut ps = ParameterSet::new();
        ps.set_number("x0", 0.0)
            .set_number("y0", 0.0)
            .set_number("length", 50.0)
            .set_number("depth", -5.0)
            .set_number("feed", 200.0);
        cases.push(("slot_milling", format!("slot_milling_{id}"), preset, ps));
    }
    cases
}

#[test]
fn builtin_templates_match_golden_matrix() {
    // 21 组基线：每组固化渲染输出（.nc）与校验报告（.report.txt）。
    // 这是唯一回归防线：任何改动导致输出/校验漂移都会在此失败。
    let g = GCodeGenerator::new();
    for (template, stem, preset, params) in golden_cases() {
        let machine = preset.config();
        let out = g
            .generate(template, &params, &machine, &GenerationOptions::default())
            .unwrap();
        assert_golden(&format!("{stem}.nc"), &out);

        // 校验报告同样冻结：预期所有组合都"校验通过：无问题"
        let report = g.registry().validate(template, &params).unwrap();
        assert!(
            report.is_ok(),
            "golden 用例 {template} 校验应通过: {}",
            report.summary()
        );
        assert_golden(
            &format!("{stem}.report.txt"),
            &format!("{}\n", report.summary()),
        );
    }
}

#[test]
fn golden_matrix_covers_all_builtin_templates() {
    // 防漏项：矩阵必须覆盖全部 7 个内置模板 × 全部 3 个预设
    let g = GCodeGenerator::new();
    let builtin: Vec<String> = g
        .registry()
        .list(None)
        .iter()
        .filter(|e| matches!(&e.source, nctool_core::registry::TemplateSource::Builtin))
        .map(|e| e.name.clone())
        .collect();
    assert_eq!(builtin.len(), 7, "内置模板数不应漂移: {builtin:?}");
    let mut covered: Vec<String> = golden_cases().iter().map(|(t, ..)| t.to_string()).collect();
    covered.sort();
    covered.dedup();
    assert_eq!(covered, builtin, "golden 矩阵未覆盖全部内置模板");
}

/// 守卫（第四轮批次 B）：机床维度当前**不产生差异**，这一点必须是显式事实。
///
/// 三个预设只在 `max_spindle_rpm` / `machine_type` / `axes` / `vendor` / `model`
/// 上不同，而内置模板只引用 `generic_config()` 里的共享键 —— 于是同一模板在三个
/// 预设下逐字节相同（实测 21 份 `.nc` 只有 7 份内容）。本用例把这份"偶然的重复"
/// 变成受守的断言：
///
/// - 若某预设改了**模板可见**的键（如 INDEX 的 `coordinate_system` 改成 `G55`），
///   本用例会红并提示"机床维度开始分化"—— 而不是让 21 份文件里悄悄多出差异、
///   无人复核；
/// - 它也拦住"为了消重而删掉 `_wfl` / `_index` 基线"这种改法：删了就没有东西
///   能拦住预设的模板可见改动。
#[test]
fn machine_dimension_is_currently_flat() {
    let g = GCodeGenerator::new();
    let presets = [
        ("Generic", MachinePreset::Generic),
        ("WFL", MachinePreset::WflM65),
        ("INDEX", MachinePreset::IndexMs40),
    ];
    let mut seen = std::collections::BTreeSet::new();

    for (template, _, _, params) in golden_cases() {
        if !seen.insert(template) {
            continue; // 每个模板只验一次（golden_cases 里出现 3 次）
        }
        let outputs: Vec<(&str, String)> = presets
            .iter()
            .map(|(label, preset)| {
                let out = g
                    .generate(
                        template,
                        &params,
                        &preset.config(),
                        &GenerationOptions::default(),
                    )
                    .expect("golden 用例应能生成");
                (*label, out)
            })
            .collect();
        for (label, out) in &outputs[1..] {
            assert_eq!(
                &outputs[0].1, out,
                "{template}: Generic 与 {label} 的输出开始分化。\
                 这说明机床维度已含信息 —— 请把 golden 矩阵改成真正按机床分维，\
                 人工复核 `_wfl` / `_index` 基线后删除本断言"
            );
        }
    }
}

/// 负向 golden 用例：故意让校验失败，冻结**失败路径的报告文本**。
///
/// 21 组正向基线的报告恒为「校验通过：无问题」，报告维度只有 1 份信息量；失败
/// 路径的级别 / 参数名 / 文案此前**完全没有基线** —— 把 `Missing` 误标成警告、
/// 或消息里的参数名写错，不会有任何测试发现。
///
/// 返回：(文件茎名, 模板名, 参数集)；期望报告见 `tests/golden/<茎名>.report.txt`。
fn negative_cases() -> Vec<(&'static str, &'static str, ParameterSet)> {
    // 1) 必选参数缺失：drill_cycle 需要 x / y / depth / feed
    let mut missing = ParameterSet::new();
    missing.set_number("x", 21.0);

    // 2) 类型不符：tool_num 声明为整数，传字符串
    let mut wrong_type = ParameterSet::new();
    wrong_type
        .set_string("tool_num", "T5")
        .set_integer("spindle_speed", 3000);

    // 3) 越界：program_header 的 prog 声明区间 [1, 9999]
    let mut out_of_range = ParameterSet::new();
    out_of_range.set_integer("prog", 99999);

    vec![
        ("neg_missing_required", "drill_cycle", missing),
        ("neg_type_mismatch", "tool_change", wrong_type),
        ("neg_out_of_range", "program_header", out_of_range),
    ]
}

#[test]
fn negative_cases_freeze_failure_reports() {
    let g = GCodeGenerator::new();
    for (stem, template, params) in negative_cases() {
        let report = g
            .registry()
            .validate(template, &params)
            .unwrap_or_else(|e| panic!("{stem}: 校验本身不应失败: {e:?}"));
        assert!(
            report.has_errors(),
            "{stem}: 负向用例必须报 Error，否则这份基线没有意义: {}",
            report.summary()
        );
        assert_golden(
            &format!("{stem}.report.txt"),
            &format!("{}\n", report.summary()),
        );
    }
}

#[test]
fn template_include_between_registered_templates() {
    // 模板间 include：子模板复用内置子程序
    let mut g = GCodeGenerator::new();
    g.registry_mut()
        .add_memory(
            "my_program",
            TemplateCategory::General,
            "",
            "{% include \"program_header\" %}\n( 主体工序 )\n{% include \"program_footer\" %}",
            vec![nctool_core::validate::spec(
                "prog",
                ParamKind::Number,
                true,
                None,
                "程序号",
            )],
        )
        .unwrap();

    let mut ps = ParameterSet::new();
    ps.set_number("prog", 7.0);
    let machine = MachinePreset::Generic.config();
    let out = g
        .generate("my_program", &ps, &machine, &GenerationOptions::default())
        .unwrap();
    assert!(out.contains("O0007"));
    assert!(out.contains("( 主体工序 )"));
    assert!(out.contains("M30"), "应包含程序尾");
}

#[test]
fn validation_report_is_structured() {
    let g = GCodeGenerator::new();
    // drill_cycle 缺必选参数
    let err = g
        .generate(
            "drill_cycle",
            &ParameterSet::new(),
            &MachinePreset::Generic.config(),
            &GenerationOptions::default(),
        )
        .unwrap_err();
    match err {
        PipelineError::Validation(report) => {
            let params: Vec<&str> = report.errors().filter_map(|e| e.param.as_deref()).collect();
            assert!(params.contains(&"x"));
            assert!(params.contains(&"depth"));
        }
        other => panic!("应为校验错误: {other}"),
    }
}

#[test]
fn machine_config_can_be_customized() {
    // 自定义机床配置并覆盖系统变量
    let g = GCodeGenerator::new();
    let mut machine = MachinePreset::Generic.config();
    machine
        .config
        .insert("coordinate_system".into(), "G55".into());
    machine.config.insert("program_end".into(), "M99".into());

    let mut ps = ParameterSet::new();
    ps.set_number("prog", 1.0);
    let out = g
        .generate(
            "program_header",
            &ps,
            &machine,
            &GenerationOptions::default(),
        )
        .unwrap();
    assert!(out.contains("G55"));
    let footer = g
        .generate(
            "program_footer",
            &ParameterSet::new(),
            &machine,
            &GenerationOptions::default(),
        )
        .unwrap();
    assert!(footer.contains("M99"), "自定义 program_end 应生效");
}

#[test]
fn registry_list_and_filter() {
    let r = TemplateRegistry::new();
    let general = r.list(Some(TemplateCategory::General));
    assert!(general.iter().any(|e| e.name == "program_header"));
    let drilling = r.list(Some(TemplateCategory::Drilling));
    assert!(drilling.iter().any(|e| e.name == "drill_cycle"));
    // 分类互斥
    assert!(!general.iter().any(|e| e.name == "drill_cycle"));
}

#[test]
fn parameter_set_serde_roundtrip() {
    // 参数集 JSON 序列化往返（CLI 配置文件场景）
    let mut ps = ParameterSet::new();
    ps.set_number("x", 21.0)
        .set_string("tool", "D12")
        .set_bool("coolant", true);
    let json = serde_json::to_string(&ps).unwrap();
    let back: ParameterSet = serde_json::from_str(&json).unwrap();
    assert_eq!(back, ps);
    // 参数以裸值注入上下文
    let v = back.to_minijinja_value();
    let x = v.get_attr("x").unwrap();
    assert!(x.is_number());
}

#[test]
fn spec_default_used_in_generation() {
    // 规格默认值应在渲染前应用（校验与渲染一致）
    let mut g = GCodeGenerator::new();
    g.registry_mut()
        .add_memory(
            "with_default",
            TemplateCategory::General,
            "",
            "F{{ feed | nc_fixed(3) }} S{{ rpm }}",
            vec![
                nctool_core::validate::spec(
                    "feed",
                    ParamKind::Number,
                    false,
                    Some(ParamValue::Number(0.15)),
                    "进给",
                ),
                nctool_core::validate::spec(
                    "rpm",
                    ParamKind::Number,
                    false,
                    Some(ParamValue::Number(1200.0)),
                    "转速",
                ),
            ],
        )
        .unwrap();
    // 不提供 feed/rpm，应被规格默认值兜底
    let out = g
        .generate(
            "with_default",
            &ParameterSet::new(),
            &MachinePreset::Generic.config(),
            &GenerationOptions::default(),
        )
        .unwrap();
    assert!(out.contains("F0.150"));
    assert!(out.contains("S1200"));
}
