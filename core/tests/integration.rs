//! nctool-core 集成测试：通过公共 API 验证端到端能力。

use nctool_core::machine::MachinePreset;
use nctool_core::pipeline::{GCodeGenerator, GenerationOptions, PipelineError};
use nctool_core::registry::{TemplateCategory, TemplateRegistry};
use nctool_core::{ParamKind, ParamValue, ParameterSet};

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
    // 字节级 golden：坐标系/进给模式直接输出配置值（不重复 G 前缀）
    assert_eq!(
        out,
        "O0042\n( GEAR_SHAFT )\n(  )\nG21 (metric)\nG54\nG94\nM5\nM9\n"
    );
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
