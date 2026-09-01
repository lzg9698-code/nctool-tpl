//! `inspect` 子命令：变量提取（必选/可选 + 行列定位）。

use crate::cli::InspectArgs;
use crate::context::Ctx;
use crate::output::CliError;

use super::templates::{extract_variables, resolve_source};

/// `inspect <template>`：解析模板并提取其引用的外部变量。
pub fn run(ctx: &Ctx, args: &InspectArgs) -> Result<(), CliError> {
    let (name, source, _, system_vars) = resolve_source(ctx, &args.template)?;
    let vars = extract_variables(&source, &name, &system_vars)?;

    let required: Vec<_> = vars.iter().filter(|v| !v.optional).collect();
    let optional: Vec<_> = vars.iter().filter(|v| v.optional).collect();

    // JSON 输出
    let data = serde_json::json!({
        "template": name,
        "required": required.iter().map(|v| serde_json::json!({
            "name": v.name, "line": v.line, "col": v.col,
        })).collect::<Vec<_>>(),
        "optional": optional.iter().map(|v| serde_json::json!({
            "name": v.name, "line": v.line, "col": v.col,
        })).collect::<Vec<_>>(),
    });

    // 文本输出
    let mut text = format!("模板: {name}\n");
    text.push_str(&format!("必选参数（{}）:\n", required.len()));
    for v in &required {
        text.push_str(&format!("  {}  行 {} 列 {}\n", v.name, v.line, v.col));
    }
    text.push_str(&format!("可选参数（{}）:\n", optional.len()));
    for v in &optional {
        text.push_str(&format!("  {}  行 {} 列 {}\n", v.name, v.line, v.col));
    }
    if vars.is_empty() {
        text.push_str("  （无外部变量引用）\n");
    }

    ctx.style.print_ok(&text, data);
    Ok(())
}
