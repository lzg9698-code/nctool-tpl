//! E4.2 大程序（万行级）生成的耗时与内存复核。
//!
//! 关注点（ROADMAP E4.2）：行号位宽已夹紧，但**夹紧是否真的挡住了内存放大**
//! 需要实测。`line_number_digits` 来自用户可编辑的机床配置，若缺失上界，
//! 配一个天文数字就会让**每一行**都去分配超大缓冲 —— 而 Rust 的分配失败是
//! 进程 abort，**不可捕获**。
//!
//! 因此这里的两条 `#[ignore]` 用例同时盯两件事：
//! 1. 万行程序在可接受的耗时内完成；
//! 2. 即便配置恶意位宽，输出体积仍被夹在可控范围（不会 GB 级膨胀）。
//!
//! `#[ignore]` 是因为万行渲染在 debug 下要跑数秒，不适合进常规 CI 门禁；
//! 实测请跑：`cargo test --release -p nctool-core --test large_program -- --ignored --nocapture`

use std::time::Instant;

use nctool_core::machine::MachinePreset;
use nctool_core::pipeline::{GCodeGenerator, GenerationOptions};
use nctool_core::registry::TemplateCategory;

/// 万行级规模。真实模具/叶轮程序常见数千至数万行，取 10000 作代表规模。
const BIG_LINES: usize = 10_000;

/// 构造 `n` 行线性插补段的模板（不依赖参数集：`range()` 在模板内驱动）。
fn line_template(lines: usize) -> String {
    format!("{{% for i in range({lines}) %}}\nG1 X{{{{ i }}}} Y{{{{ i }}}} F100\n{{% endfor %}}")
}

/// 注册模板并返回生成器。
fn generator_with(template_src: &str) -> GCodeGenerator {
    let mut g = GCodeGenerator::new();
    g.registry_mut()
        .add_memory(
            "big",
            TemplateCategory::General,
            "万行级压力测试模板",
            template_src,
            vec![],
        )
        .expect("注册模板失败");
    g
}

/// 开行号、清空行的选项。
fn numbered_opts() -> GenerationOptions {
    GenerationOptions {
        line_numbers: true,
        strip_blank_lines: true,
        ..GenerationOptions::default()
    }
}

// ---------------------------------------------------------------------------
// 常规用例：行号边界与位宽夹紧（快，进 CI）
// ---------------------------------------------------------------------------

#[test]
fn line_numbers_stop_at_max_line_number() {
    // step=10、上限 50 ⇒ 只应编出 N0010..N0050 共 5 行，之后不再编号。
    // 这是"行号达到 max 后不再递增"契约的直接验证。
    let g = generator_with(&line_template(20));
    let opts = GenerationOptions {
        line_number_step: 10,
        max_line_number: 50,
        ..numbered_opts()
    };
    let out = g
        .generate(
            "big",
            &Default::default(),
            &MachinePreset::Generic.config(),
            &opts,
        )
        .unwrap();

    let numbered: Vec<&str> = out.lines().filter(|l| l.starts_with('N')).collect();
    assert_eq!(
        numbered.len(),
        5,
        "上限 50 / 步进 10 应恰好编号 5 行，实际 {numbered:?}"
    );
    assert_eq!(numbered[0], "N0010 G1 X0 Y0 F100");
    assert_eq!(numbered[4], "N0050 G1 X4 Y4 F100");
    // 第 6 行起不再带前缀
    assert!(
        out.lines().nth(5).unwrap().starts_with("G1"),
        "超出上限的行不应再编号: {:?}",
        out.lines().nth(5)
    );
}

#[test]
fn hostile_line_number_digits_is_clamped() {
    // 恶意配置：位宽 10 亿。若未夹紧，单行就要分配 GB 级缓冲并 abort。
    // 夹紧上界为 32（见 pipeline::MAX_LINE_NUMBER_DIGITS）。
    let mut machine = MachinePreset::Generic.config();
    machine
        .config
        .insert("line_number_digits".to_string(), "1000000000".to_string());

    let g = generator_with(&line_template(3));
    let out = g
        .generate("big", &Default::default(), &machine, &numbered_opts())
        .unwrap();

    let first = out.lines().next().unwrap();
    // 形如 `N` + 32 位数字 + 一个空格
    let digits = first
        .strip_prefix('N')
        .and_then(|s| s.split(' ').next())
        .expect("首行应带 N 前缀");
    assert_eq!(
        digits.len(),
        32,
        "位宽应被夹到 32，实际 {}（整行: {first:?}）",
        digits.len()
    );
    assert!(
        digits.trim_start_matches('0').parse::<u64>().is_ok(),
        "夹紧后仍应是合法数字: {digits}"
    );
}

#[test]
fn step_zero_is_treated_as_one() {
    // step=0 若不当作 1，会产出重复的 N0000 行
    let g = generator_with(&line_template(3));
    let opts = GenerationOptions {
        line_number_step: 0,
        ..numbered_opts()
    };
    let out = g
        .generate(
            "big",
            &Default::default(),
            &MachinePreset::Generic.config(),
            &opts,
        )
        .unwrap();
    let numbered: Vec<&str> = out.lines().filter(|l| l.starts_with('N')).collect();
    assert_eq!(numbered.len(), 3, "step=0 应每行编号: {numbered:?}");
    assert_eq!(numbered[0], "N0001 G1 X0 Y0 F100");
    assert_eq!(numbered[2], "N0003 G1 X2 Y2 F100");
}

// ---------------------------------------------------------------------------
// #[ignore] 实测用例：万行级规模
// ---------------------------------------------------------------------------

#[test]
#[ignore = "万行级实测，跑 `cargo test --release -- --ignored --nocapture`"]
fn large_program_10k_lines_is_fast_and_bounded() {
    let g = generator_with(&line_template(BIG_LINES));

    let start = Instant::now();
    let out = g
        .generate(
            "big",
            &Default::default(),
            &MachinePreset::Generic.config(),
            &numbered_opts(),
        )
        .unwrap();
    let elapsed = start.elapsed();

    let lines = out.lines().count();
    println!(
        "[10k] 耗时 {:?}｜输出 {} 行 / {} 字节",
        elapsed,
        lines,
        out.len()
    );

    assert_eq!(lines, BIG_LINES, "清理空行后应恰好 {BIG_LINES} 行");
    // 4 位行号 + 空格 ≈ 6 字节/行前缀，10k 行合计 ~0.3 MB；给 4 MB 冗余
    assert!(
        out.len() < 4 * 1024 * 1024,
        "输出体积失控: {} 字节（位宽夹紧可能失效）",
        out.len()
    );
    // 阈值放宽到 10s：CI 机器性能差异大，此用例只拦"数量级退化"，不做微基准
    assert!(
        elapsed.as_secs() < 10,
        "万行生成耗时 {elapsed:?} 超出预期（应远小于 10s）"
    );
}

#[test]
#[ignore = "恶意位宽 × 万行的内存实测，跑 `cargo test --release -- --ignored --nocapture`"]
fn large_program_with_hostile_digits_stays_bounded() {
    // 组合最坏情况：万行 + 位宽 10 亿。夹紧若失效，这里会 OOM/abort 而非断言失败。
    let mut machine = MachinePreset::Generic.config();
    machine
        .config
        .insert("line_number_digits".to_string(), "1000000000".to_string());

    let g = generator_with(&line_template(BIG_LINES));
    let start = Instant::now();
    let out = g
        .generate("big", &Default::default(), &machine, &numbered_opts())
        .unwrap();
    let elapsed = start.elapsed();

    println!(
        "[10k + 位宽 1e9] 耗时 {:?}｜输出 {} 字节（夹紧后每行前缀 33 字节）",
        elapsed,
        out.len()
    );

    // 夹紧到 32 位后：10k 行 × (1 + 32 + 1) ≈ 340 KB，加内容余量给 8 MB
    assert!(
        out.len() < 8 * 1024 * 1024,
        "恶意位宽下输出体积失控: {} 字节",
        out.len()
    );
    assert!(elapsed.as_secs() < 10, "耗时 {elapsed:?} 超出预期");
}
