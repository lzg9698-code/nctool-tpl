//! `render` / `generate` 子命令：渲染生成 G-code。

use std::path::{Path, PathBuf};

use nctool_core::pipeline::{GCodeGenerator, GenerationOptions, OutputFormat};
use nctool_core::registry::{TemplateCategory, TemplateSource};

use crate::cli::RenderArgs;
use crate::context::{build_params, Ctx};
use crate::output::CliError;

/// `render` 命令：解析模板 → 校验 → 渲染 → 后处理 → 输出/写文件。
pub fn run(ctx: &Ctx, args: &RenderArgs) -> Result<(), CliError> {
    let (gen, name, template_source) = resolve_registry(ctx, &args.template)?;
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
    // 警告并入 JSON 成功输出（text 通道仅在 --verbose 时展示）
    let warnings: Vec<String> = report.warnings().map(|w| w.message.clone()).collect();

    // 渲染：严格走核心生成管线；宽松走核心宽松管线（规格默认值兜底与
    // 后处理与严格模式完全一致，仅未定义变量留空、校验不阻断）
    let out = if args.lenient {
        gen.generate_lenient(&name, &params, &machine, &opts)?
    } else {
        gen.generate(&name, &params, &machine, &opts)?
    };

    // 输出：--out 写文件；否则写 stdout
    match &args.out {
        Some(path) => {
            write_out_file(path, &out, template_source.as_deref())?;
            let text = format!("已写入: {}\n", path.display());
            let data = serde_json::json!({
                "output_file": path.display().to_string(),
                "template": name,
                "warnings": &warnings,
            });
            ctx.style.print_ok(&text, data);
        }
        None => {
            let data = serde_json::json!({
                "output": out,
                "template": name,
                "warnings": &warnings,
            });
            ctx.style.print_ok(&out, data);
        }
    }
    Ok(())
}

/// 写输出文件：拒绝写入源模板自身（会销毁模板源码）；父目录缺失时创建
/// （与 `templates new` 的目录策略一致）。
fn write_out_file(path: &Path, out: &str, template_source: Option<&Path>) -> Result<(), CliError> {
    if let Some(src) = template_source {
        if same_path(src, path) {
            return Err(CliError::new(
                "args",
                format!("输出文件与模板源文件相同，拒绝写入: {}", path.display()),
            ));
        }
    }
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            std::fs::create_dir_all(parent).map_err(|e| {
                CliError::new("io", format!("创建输出目录失败 {}: {e}", parent.display()))
            })?;
        }
    }
    std::fs::write(path, out)
        .map_err(|e| CliError::new("io", format!("写入输出文件失败 {}: {e}", path.display())))
}

/// 判断两个路径是否指向同一文件（规范化比较；目标文件可能尚不存在）。
fn same_path(a: &Path, b: &Path) -> bool {
    fn norm(p: &Path) -> Option<PathBuf> {
        if p.exists() {
            std::fs::canonicalize(p).ok()
        } else {
            // 目标尚未存在：规范化其父目录后拼回文件名
            let parent = match p.parent() {
                Some(par) if !par.as_os_str().is_empty() => std::fs::canonicalize(par).ok()?,
                _ => std::env::current_dir().ok()?,
            };
            Some(parent.join(p.file_name()?))
        }
    }
    match (norm(a), norm(b)) {
        (Some(x), Some(y)) => {
            // Windows 文件系统大小写不敏感：规范化后仍可能仅大小写不同
            x == y
                || x.to_string_lossy()
                    .eq_ignore_ascii_case(&y.to_string_lossy())
        }
        _ => false,
    }
}

/// 解析模板引用为 (生成器, 模板名, 源文件路径)。
///
/// 优先级：已注册模板名（内置/目录）→ 文件路径（注册进注册表后用文件名引用）。
/// 源文件路径用于 `--out` 同路径检测（内置模板无源路径 → `None`）。
pub fn resolve_registry(
    ctx: &Ctx,
    name_or_path: &str,
) -> Result<(GCodeGenerator, String, Option<PathBuf>), CliError> {
    let mut gen = ctx.build_registry()?;

    // 1) 已注册模板名（内置 / 目录）优先
    if let Some(entry) = gen.registry().get(name_or_path) {
        let source = match &entry.source {
            TemplateSource::File(p) => Some(p.clone()),
            _ => None,
        };
        return Ok((gen, name_or_path.to_string(), source));
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
        return Ok((gen, fname, Some(path)));
    }

    Err(CliError::new(
        "template_not_found",
        format!("模板不存在: {name_or_path}"),
    ))
}

#[cfg(test)]
mod tests {
    use super::same_path;

    #[test]
    fn same_path_detects_self_and_case_insensitive() {
        let dir = std::env::temp_dir().join(format!("nctool_same_path_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("t.j2");
        std::fs::write(&src, "X{{ x }}").unwrap();

        // 存在的源 vs 尚不存在的同路径输出（相对/绝对、大小写变体）
        assert!(same_path(&src, &dir.join("t.j2")));
        assert!(same_path(&src, &dir.join("T.J2")));

        // 不同文件
        assert!(!same_path(&src, &dir.join("other.j2")));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
