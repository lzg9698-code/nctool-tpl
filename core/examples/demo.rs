//! 端到端示例：从参数规格到 G-code 生成。
//!
//! 展示 nctool-core 的完整流程：
//! 1. 注册一个自定义钻孔工序模板（带参数规格）
//! 2. 用规格默认值兜底 + 参数集渲染
//! 3. 切换到 WFL M65 机床配置
//! 4. 演示参数校验失败时的结构化错误
//!
//! 运行：`cargo run -p nctool-core --example demo`

use nctool_core::machine::MachinePreset;
use nctool_core::pipeline::{GCodeGenerator, GenerationOptions, PipelineError};
use nctool_core::registry::TemplateCategory;
use nctool_core::{ParamKind, ParamValue, ParameterSet};

fn main() {
    let mut g = GCodeGenerator::new();

    // 1. 注册自定义模板：带参数规格（x/y 必选，depth 必选，feed 可选默认 0.15）
    g.registry_mut()
        .add_memory(
            "face_drill",
            TemplateCategory::Drilling,
            "面钻孔循环",
            concat!(
                "{{ machine.rapid }} X{{ x | nc_fixed(3) }} Y{{ y | nc_fixed(3) }}\n",
                "{{ machine.linear }} G98 G81 R{{ r_plane | nc_fixed(3) }} ",
                "Z{{ depth | nc_fixed(3) }} F{{ feed | nc_fixed(3) }}\n",
                "G80\n",
            ),
            vec![
                nctool_core::validate::spec("x", ParamKind::Number, true, None, "孔 X 坐标"),
                nctool_core::validate::spec("y", ParamKind::Number, true, None, "孔 Y 坐标"),
                nctool_core::validate::spec("depth", ParamKind::Number, true, None, "钻孔深度"),
                nctool_core::validate::spec(
                    "r_plane",
                    ParamKind::Number,
                    false,
                    Some(ParamValue::Number(5.0)),
                    "R 平面（默认 5.0）",
                ),
                nctool_core::validate::spec(
                    "feed",
                    ParamKind::Number,
                    false,
                    Some(ParamValue::Number(0.15)),
                    "进给（默认 0.15）",
                ),
            ],
        )
        .expect("注册模板失败");

    // 2. 构造参数集（x/y/depth 必选，r_plane/feed 由规格默认值兜底）
    let mut params = ParameterSet::new();
    params
        .set_number("x", 21.0)
        .set_number("y", 15.0)
        .set_number("depth", -10.0);

    // 3. 用通用机床生成
    let generic = MachinePreset::Generic.config();
    let opts = GenerationOptions {
        line_numbers: true,
        ..Default::default()
    };
    let out = g
        .generate("face_drill", &params, &generic, &opts)
        .expect("生成失败");
    println!("=== 通用机床 ===\n{out}");

    // 4. 切换到 WFL M65 机床（模板自动引用不同 machine 配置）
    let wfl = MachinePreset::WflM65.config();
    let out_wfl = g
        .generate("face_drill", &params, &wfl, &GenerationOptions::default())
        .expect("WFL 生成失败");
    println!("=== WFL M65 ===\n{out_wfl}");

    // 5. 演示校验失败：缺少必选参数 x
    let bad = ParameterSet::new();
    match g.generate("face_drill", &bad, &generic, &GenerationOptions::default()) {
        Err(PipelineError::Validation(report)) => {
            println!("=== 校验失败（结构化错误） ===");
            println!("{}", report.summary());
        }
        other => panic!("预期校验失败，实际: {other:?}"),
    }
}
