//! 可运行演示：解析 `templates/demo_gcode.j2`，
//! 打印模板用到的变量 / 未声明变量，再用给定上下文渲染出 G-code。
//!
//! 运行：`cargo run --example demo`

use nctool_tpl::{extract_undeclared, extract_variables, parse, Renderer};

fn main() {
    // 读取示例模板（相对 crate 根目录）
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/templates/demo_gcode.j2");
    let source = std::fs::read_to_string(path).expect("读取模板失败");
    let name = "demo_gcode.j2";

    // 1) 语法检查 + AST
    let ast = parse(&source, name).expect("模板语法错误");
    println!("== 模板已解析: {name} ==");

    // 2) 变量提取
    let all = extract_variables(&ast);
    println!("\n== 模板引用到的全部变量 ({}) ==", all.len());
    for v in &all {
        println!("  {:<20} @ 行 {} 列 {}", v.name, v.line, v.col);
    }

    let undeclared = extract_undeclared(&ast);
    println!("\n== 未声明变量（需外部提供，共 {}) ==", undeclared.len());
    for v in &undeclared {
        println!("  {:<20} @ 行 {} 列 {}", v.name, v.line, v.col);
    }

    // 3) 渲染
    let renderer = Renderer::new();
    let ctx = minijinja::context! {
        program_number => 1000,
        program_name => "RELIEF_GROOVE_DEMO",
        tool_number => 3,
        start_x => 60.0,
        start_z => 2.0,
        safety_z => 5.0,
        diameter => 42.0,
        depth => -3.5,
        passes => vec![
            minijinja::context! { x => 41.0, z => -1.0 },
            minijinja::context! { x => 40.5, z => -2.0 },
            minijinja::context! { x => 40.0, z => -3.0 },
        ],
    };
    println!("\n== 渲染结果 ==");
    let gcode = renderer.render(&source, name, &ctx).expect("渲染失败");
    print!("{gcode}");
}
