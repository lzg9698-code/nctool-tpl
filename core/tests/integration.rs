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

/// 断言输出与 golden 文件一致；设置环境变量 `NCTOOL_UPDATE_GOLDEN=1` 时重新
/// 写入 golden 文件（**仅用于人工确认后的基线刷新，切勿在 CI 更新**）。
fn assert_golden(name: &str, actual: &str) {
    if std::env::var_os("NCTOOL_UPDATE_GOLDEN").is_some() {
        let path = golden_path(name);
        std::fs::create_dir_all(path.parent().expect("golden 路径应有父目录"))
            .expect("创建 golden 目录失败");
        std::fs::write(&path, actual).expect("写入 golden 文件失败");
        return;
    }
    let path = golden_path(name);
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("读取 golden 文件失败 {}: {err}", path.display()));
    assert_eq!(actual, expected, "golden 不匹配: {}", path.display());
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

/// golden 基线矩阵：5 内置模板 × 3 机床预设 = 15 组。
///
/// 每组返回：模板名、文件茎名（`<模板>_<机床id>`）、机床预设、参数集。
/// 参数集与机床无关（模板引用的都是加工参数 + machine 系统变量）。
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
        ps.set_integer("tool_num", 5).set_integer("spindle_speed", 3000);
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
    }
    cases
}

#[test]
fn builtin_templates_match_golden_matrix() {
    // 15 组基线：每组固化渲染输出（.nc）与校验报告（.report.txt）。
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
        assert_golden(&format!("{stem}.report.txt"), &format!("{}\n", report.summary()));
    }
}

#[test]
fn golden_matrix_covers_all_builtin_templates() {
    // 防漏项：矩阵必须覆盖全部 5 个内置模板 × 全部 3 个预设
    let g = GCodeGenerator::new();
    let builtin: Vec<String> = g
        .registry()
        .list(None)
        .iter()
        .filter(|e| matches!(&e.source, nctool_core::registry::TemplateSource::Builtin))
        .map(|e| e.name.clone())
        .collect();
    assert_eq!(builtin.len(), 5, "内置模板数不应漂移: {builtin:?}");
    let mut covered: Vec<String> = golden_cases()
        .iter()
        .map(|(t, ..)| t.to_string())
        .collect();
    covered.sort();
    covered.dedup();
    assert_eq!(covered, builtin, "golden 矩阵未覆盖全部内置模板");
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
