//! nctool-tpl 性能基准：解析 / 变量提取 / 渲染三个核心场景。
//!
//! 运行：`cargo bench`
//! 报告：`target/criterion/report/index.html`

use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use nctool_tpl::{extract_undeclared, parse, Renderer};

/// 中等复杂度的 G-code 模板：含 set/default、数学过滤器、循环、条件。
const TEMPLATE: &str = r#"{% set feed = default_feed | default(0.15) %}
O{{ program_number }} ({{ part_name }})
G90 G54 G17
M3 S{{ spindle_speed }}
{% for hole in holes %}
G0 X{{ hole.x }} Y{{ hole.y }}
G1 Z{{ hole.depth }} F{{ feed }}
G0 Z5.0
{% endfor %}
{% if coolant %}M8{% endif %}
M5
M30
"#;

fn bench_parse(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse");
    group.throughput(Throughput::Bytes(TEMPLATE.len() as u64));
    group.bench_function("gcode_template", |b| {
        b.iter(|| parse(TEMPLATE, "bench.j2").unwrap());
    });
    group.finish();
}

fn bench_extract_undeclared(c: &mut Criterion) {
    let ast = parse(TEMPLATE, "bench.j2").unwrap();
    let mut group = c.benchmark_group("extract_undeclared");
    group.bench_function("gcode_template", |b| {
        b.iter(|| extract_undeclared(&ast));
    });
    group.finish();
}

fn bench_render(c: &mut Criterion) {
    let renderer = Renderer::new();
    let context = minijinja::context! {
        default_feed => 0.15,
        program_number => 1000,
        part_name => "DEMO_PART",
        spindle_speed => 2000,
        coolant => true,
        holes => vec![
            minijinja::context! { x => 10.0, y => 20.0, depth => -5.0 },
            minijinja::context! { x => 30.0, y => 40.0, depth => -10.0 },
            minijinja::context! { x => 50.0, y => 60.0, depth => -15.0 },
        ],
    };
    let mut group = c.benchmark_group("render");
    group.throughput(Throughput::Bytes(TEMPLATE.len() as u64));
    group.bench_function("gcode_template", |b| {
        b.iter(|| renderer.render(TEMPLATE, "bench.j2", &context).unwrap());
    });
    group.finish();
}

criterion_group!(benches, bench_parse, bench_extract_undeclared, bench_render);
criterion_main!(benches);
