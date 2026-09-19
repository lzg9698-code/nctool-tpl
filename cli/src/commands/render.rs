//! `render` / `generate` 子命令：渲染生成 G-code。

use std::path::{Path, PathBuf};
use std::rc::Rc;

use nctool_core::pipeline::{GCodeGenerator, GenerationOptions, OutputFormat};
use nctool_core::registry::{TemplateCategory, TemplateSource};

use crate::cli::RenderArgs;
use crate::context::{build_params, Ctx};
use crate::output::CliError;

/// `render` 命令：解析模板 → 校验 → 渲染 → 后处理 → 输出/写文件。
pub fn run(ctx: &Ctx, args: &RenderArgs) -> Result<(), CliError> {
    let (gen, name, template_source) = resolve_registry(ctx, &args.template)?;
    // 规格：`--param k=v` 的值要按规格归一（argv 没有类型信息，见 args::coerce_param_value）
    let specs = gen
        .registry()
        .get(&name)
        .map(|e| e.params.clone())
        .unwrap_or_default();
    let params = build_params(
        args.params.params_file.as_deref(),
        &args.params.param,
        &specs,
    )?;
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
        // 报告走 stderr：stdout 要留给 G-code（未指定 --out 时 G-code 写 stdout）。
        // 且**不能**把整份报告塞进 `CliError::message`——统一错误输出只给首行加
        // `error: ` 前缀，多行报告的首行会被当成错误摘要（提示行还可能排在最前，
        // 变成 `error: 提示 …` 这种自相矛盾的输出）。
        eprintln!("{}", report.summary());
        return Err(CliError::new(
            "validation",
            "参数校验未通过（详见上方报告）",
        ));
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
///
/// 走 [`Ctx::build_registry`] 的缓存注册表；仅当需要**就地注册临时文件模板**时
/// 退化为 [`Ctx::build_registry_fresh`] 重建一份独立注册表——临时模板不该
/// 污染共享缓存（否则后续调用会看到一个本不存在的模板名）。
pub fn resolve_registry(
    ctx: &Ctx,
    name_or_path: &str,
) -> Result<(Rc<GCodeGenerator>, String, Option<PathBuf>), CliError> {
    let gen = ctx.build_registry()?;

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
        if fname.is_empty() {
            return Err(CliError::new(
                "args",
                format!("无法从路径取文件名: {}", path.display()),
            ));
        }
        // 注册名：默认用文件名；**与注册表里另一个文件同名时**退化为完整路径。
        //
        // 此前这里只判断"同名的已存在"，然后直接 `return Ok((gen, fname, ...))` ——
        // 即命中冲突就改用注册表里那一份。于是
        // `nctool --template-dir templates render /tmp/other/a.j2`（模板目录里恰好也有
        // `a.j2`）会渲染出 `templates/a.j2` 的 G-code，用户以为渲染的是自己给的文件。
        // 不报错、不告警，只是产出另一份程序。
        //
        // 用路径作注册名而非报错：用户显式给了路径，诉求很明确，不该被挡回来；
        // 完整路径不可能与目录模板名冲突，且一眼能看出是个文件。
        let key = match gen.registry().get(&fname) {
            // 同名**且就是本文件**：复用注册表里那份 —— 用户显式写出模板目录内的
            // 路径时是最常见的情形，不必重复注册。
            Some(entry) => match &entry.source {
                TemplateSource::File(p) if same_path(p, &path) => {
                    return Ok((gen, fname, Some(path)));
                }
                _ => path.display().to_string(),
            },
            None => fname,
        };
        let mut fresh = ctx.build_registry_fresh()?;
        fresh.registry_mut().add_file(
            key.clone(),
            TemplateCategory::General,
            format!("文件模板: {}", path.display()),
            &path,
            vec![],
        )?;
        return Ok((Rc::new(fresh), key, Some(path)));
    }

    Err(CliError::new(
        "template_not_found",
        format!("模板不存在: {name_or_path}"),
    ))
}

#[cfg(test)]
mod tests {
    use super::{resolve_registry, same_path};
    use crate::context::Ctx;

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

    /// 回归（P1-7）：模板目录里已有 `a.j2` 时，`render <别的目录>/a.j2` 此前
    /// **静默渲染成模板目录里那一份** —— 用户以为渲染的是自己给的文件，
    /// 拿到的是另一份程序，且不报错、不告警。
    #[test]
    fn explicit_path_that_collides_with_registered_name_wins() {
        let base = std::env::temp_dir().join(format!("nctool_collide_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let tpl_dir = base.join("templates");
        let elsewhere = base.join("elsewhere");
        std::fs::create_dir_all(&tpl_dir).unwrap();
        std::fs::create_dir_all(&elsewhere).unwrap();
        // 两份同名文件的内容必须可区分，否则断言证明不了渲染的是哪一份
        std::fs::write(tpl_dir.join("a.j2"), "REGISTERED G0 X1\n").unwrap();
        let explicit = elsewhere.join("a.j2");
        std::fs::write(&explicit, "EXPLICIT G0 X2\n").unwrap();

        let mut ctx = Ctx::for_test();
        ctx.template_dir = Some(tpl_dir.clone());

        let (gen, name, src) = resolve_registry(&ctx, explicit.to_str().unwrap()).unwrap();
        assert_eq!(
            src.as_deref(),
            Some(explicit.as_path()),
            "源路径应是用户显式给的那个文件"
        );
        assert_ne!(name, "a.j2", "同名冲突时注册名应退化为路径: {name}");
        let entry = gen.registry().get(&name).expect("应能取到刚注册的模板");
        assert!(
            entry.source_text.contains("EXPLICIT"),
            "渲染的必须是用户指定的文件，而不是模板目录里的同名模板: {}",
            entry.source_text
        );

        // 反向：路径指向的**就是**模板目录里那个文件时，仍复用注册表条目，
        // 注册名保持可读的 `a.j2`（常见情形，不该被上面的退化牵连）
        let (gen2, name2, _) =
            resolve_registry(&ctx, tpl_dir.join("a.j2").to_str().unwrap()).unwrap();
        assert_eq!(name2, "a.j2", "同一文件不该改名");
        assert!(gen2
            .registry()
            .get("a.j2")
            .unwrap()
            .source_text
            .contains("REGISTERED"));

        let _ = std::fs::remove_dir_all(&base);
    }
}
