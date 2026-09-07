//! nctool-core 性能基线：生成管线的端到端与后处理开销。
//!
//! 与根 crate `benches/bench.rs` 的分工：
//! - 根 crate：解析（`parse`）、变量提取（`extract_undeclared`）、渲染（`render`）
//! - 本文件：**后处理**（行号 / 空行清理 / ASCII 清洗）与**端到端生成**
//!
//! 后处理单独测的原因：行号是"每行的固定开销"，在万行级程序上会被放大
//! 一万倍；单独出基线才能在规模增长时定位到是渲染慢还是后处理慢。
//! 相关实测见 `core/tests/large_program.rs`（E4.2，万行级约 1.8 ms）。
//!
//! 运行：`cargo bench -p nctool-core`
//! 报告：`target/criterion/report/index.html`

use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use nctool_core::machine::MachinePreset;
use nctool_core::pipeline::{GCodeGenerator, GenerationOptions};
use nctool_core::registry::TemplateCategory;
use nctool_core::ParameterSet;

/// 中等规模模板：3000 行线性插补段，用于放大后处理的每行开销。
const MID_LINES: usize = 3_000;

/// `n` 行线性插补段模板（`range()` 在模板内驱动，不依赖参数集）。
fn line_template(lines: usize) -> String {
    format!("{{% for i in range({lines}) %}}\nG1 X{{{{ i }}}} Y{{{{ i }}}} F100\n{{% endfor %}}")
}

fn generator_with(template_src: &str) -> GCodeGenerator {
    let mut g = GCodeGenerator::new();
    g.registry_mut()
        .add_memory(
            "bench_big",
            TemplateCategory::General,
            "性能基准用模板",
            template_src,
            vec![],
        )
        .expect("注册模板失败");
    g
}

/// 端到端：内置 `drill_cycle`（真实小模板，反映典型交互路径）。
fn bench_generate_end_to_end(c: &mut Criterion) {
    let g = GCodeGenerator::new();
    let mut ps = ParameterSet::new();
    ps.set_number("x", 21.0)
        .set_number("y", 15.0)
        .set_number("r_plane", 5.0)
        .set_number("depth", -10.0)
        .set_number("feed", 100.0);
    let machine = MachinePreset::Generic.config();

    c.bench_function("generate_drill_cycle", |b| {
        b.iter(|| {
            g.generate(
                black_box("drill_cycle"),
                &ps,
                &machine,
                &GenerationOptions::default(),
            )
            .unwrap()
        })
    });
}

/// 后处理：同一渲染结果下，对比"不编号"与"编号"的开销差。
///
/// 差值即行号前缀本身每行引入的成本；乘以目标行数可外推任意规模。
fn bench_postprocess_line_numbers(c: &mut Criterion) {
    let src = line_template(MID_LINES);
    let g = generator_with(&src);
    let params = ParameterSet::new();
    let machine = MachinePreset::Generic.config();

    let plain = GenerationOptions {
        strip_blank_lines: true,
        ..GenerationOptions::default()
    };
    let numbered = GenerationOptions {
        line_numbers: true,
        strip_blank_lines: true,
        ..GenerationOptions::default()
    };

    // 先跑一次拿到渲染规模，作为 throughput 基准
    let sample = g
        .generate("bench_big", &params, &machine, &plain)
        .expect("基准模板应可生成");

    let mut group = c.benchmark_group("postprocess");
    group.throughput(Throughput::Bytes(sample.len() as u64));

    group.bench_function("plain", |b| {
        b.iter(|| g.generate("bench_big", &params, &machine, &plain).unwrap())
    });
    group.bench_function("line_numbers", |b| {
        b.iter(|| {
            g.generate("bench_big", &params, &machine, &numbered)
                .unwrap()
        })
    });
    group.finish();
}

/// 后处理：ASCII 清洗（逐字符扫描，万行级同样是放大项）。
fn bench_postprocess_ascii(c: &mut Criterion) {
    // 含中文注释的模板，触发 sanitize_ascii 的逐字符路径
    let src = "{% for i in range(3000) %}\nG1 X{{ i }} (进给 切削)\n{% endfor %}";
    let g = generator_with(src);
    let params = ParameterSet::new();
    let machine = MachinePreset::Generic.config();

    let opts = GenerationOptions {
        ascii_only: true,
        strip_blank_lines: true,
        ..GenerationOptions::default()
    };

    c.bench_function("postprocess_ascii_only", |b| {
        b.iter(|| g.generate("bench_big", &params, &machine, &opts).unwrap())
    });
}

criterion_group!(
    benches,
    bench_generate_end_to_end,
    bench_postprocess_line_numbers,
    bench_postprocess_ascii
);
criterion_main!(benches);
