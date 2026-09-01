//! `render` / `generate` 子命令：渲染生成 G-code。

use nctool_core::pipeline::{GCodeGenerator, GenerationOptions, OutputFormat};
use nctool_core::registry::TemplateCategory;

use crate::cli::RenderArgs;
use crate::context::{build_params, render_lenient, Ctx};
use crate::output::CliError;

/// `render` 命令：解析模板 → 校验 → 渲染 → 后处理 → 输出/写文件。
pub fn run(ctx: &Ctx, args: &RenderArgs) -> Result<(), CliError> {
    let (gen, name) = resolve_registry(ctx, &args.template)?;
    let params = build_params(args.params.params_file.as_deref(), &args.params.param)?;
    let machine = ctx.resolve_machine(None)?;

    let opts = GenerationOptions {
        format: OutputFormat::Gcode,
        line_numbers: args.line_numbers,
        add_header_comment: args.header,
        strip_blank_lines: args.strip_blank,
        ascii_only: args.ascii,
        ..Default::default()
    };

    // 渲染前校验（宽松模式不阻断，仅提示）
    let report = gen.registry().validate(&name, &params)?;
    if report.has_errors() && !args.lenient {
        return Err(CliError::new("validation", report.summary()));
    }
    if (report.has_warnings() || report.has_errors()) && ctx.verbose {
        eprintln!("note: 校验报告\n{}", report.summary());
    }

    // 渲染：宽松模式未定义变量渲染为空；否则走核心生成管线（校验/兜底/后处理）
    let out = if args.lenient {
        render_lenient(&gen, &name, &params, &machine)?
    } else {
        gen.generate(&name, &params, &machine, &opts)?
    };

    // 输出：--out 写文件；否则写 stdout
    match &args.out {
        Some(path) => {
            std::fs::write(path, &out).map_err(|e| {
                CliError::new("io", format!("写入输出文件失败 {}: {e}", path.display()))
            })?;
            let text = format!("已写入: {}\n", path.display());
            let data = serde_json::json!({
                "output_file": path.display().to_string(),
                "template": name,
            });
            ctx.style.print_ok(&text, data);
        }
        None => {
            let data = serde_json::json!({ "output": out, "template": name });
            ctx.style.print_ok(&out, data);
        }
    }
    Ok(())
}

/// 解析模板引用为 (注册表, 模板名)。
///
/// 优先级：已注册模板名（内置/目录）→ 文件路径（注册进注册表后用文件名引用）。
pub fn resolve_registry(
    ctx: &Ctx,
    name_or_path: &str,
) -> Result<(GCodeGenerator, String), CliError> {
    let mut gen = ctx.build_registry()?;

    // 1) 已注册模板名（内置 / 目录）优先
    if gen.registry().get(name_or_path).is_some() {
        return Ok((gen, name_or_path.to_string()));
    }

    // 2) 文件路径 → 注册（模板名 = 文件名，含扩展名）
    if let Some(path) = ctx.find_template_file(name_or_path) {
        let fname = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        if !fname.is_empty() && gen.registry().get(&fname).is_none() {
            gen.registry_mut().add_file(
                fname.clone(),
                TemplateCategory::General,
                format!("文件模板: {}", path.display()),
                &path,
                vec![],
            )?;
        }
        return Ok((gen, fname));
    }

    Err(CliError::new(
        "template_not_found",
        format!("模板不存在: {name_or_path}"),
    ))
}
