//! `part` 子命令：零件级批量生成（多工序一次生成）。
//!
//! 编排逻辑在 [`nctool_core::part`]；本层只负责 I/O 与呈现：
//! 读零件定义文件 → 调编排 → 按 `--out` 写文件或输出到 stdout。
//!
//! 三处 E5 走查局限（`docs/REAL_PART_WALKTHROUGH.md` §5）的落点：
//! - **5.1 行号续编** → `--line-numbers` 时由 core 逐工序传递游标
//! - **5.2 错误聚合** → core 跑完全部工序再决定成败；本层把失败清单展示给用户，
//!   且**绝不写出半成品文件**
//! - **5.3 参数继承** → 零件定义顶层的 `params` 对所有工序可见

use std::path::Path;

use nctool_core::part::{PartError, PartOptions, PartSpec};

use crate::args::read_text_capped;
use crate::cli::PartGenerateArgs;
use crate::context::Ctx;
use crate::output::{json_underflow_error, CliError};

/// `nctool part generate`：零件级批量生成。
pub fn run(ctx: &Ctx, args: &PartGenerateArgs) -> Result<(), CliError> {
    let text = read_text_capped(&args.part, "零件定义文件")?;
    // ERR-NUM-UNDERFLOW：在 serde_json 解析**之前**拦下下溢字面量（硬失败）。
    if let Some(err) = json_underflow_error(&text, &format!("零件定义文件 {}", args.part.display()))
    {
        return Err(err);
    }
    let spec: PartSpec = serde_json::from_str(&text).map_err(|e| {
        CliError::new(
            "args",
            format!("解析零件定义失败 {}: {e}", args.part.display()),
        )
    })?;

    let gen = ctx.build_registry()?;
    let opts = PartOptions {
        line_numbers: args.line_numbers,
        add_header_comment: args.header,
        strip_blank_lines: args.strip_blank,
        ascii_only: args.ascii,
        lenient: args.lenient,
    };
    // 兜底机床走 Ctx：它能解析配置里的**自定义机床**，而 core 的
    // `resolve_machine` 只认内建预设。工序级/程序级若显式指定了内建标识，
    // core 侧已解析；这里只在两者都未给时生效。
    let outcome = spec.generate(&gen, ctx.default_machine.as_deref(), &opts);

    match outcome {
        Ok(result) => {
            let data = serde_json::json!({
                "part": spec.name,
                "op_count": result.ops.len(),
                "line_count": result.line_count(),
                // 逐工序结果：**每道工序自己的正文也带上**。少了它，调用方只能拿到
                // 拼接后的整份程序，想知道「第 2 道工序从哪一行开始」就得靠
                // `end_line_number` 反推 —— 而未开行号时它恒为 0，反推无从谈起。
                // 正文与行号同时在手，切分才是可验证的。
                "ops": result.ops.iter().map(|o| serde_json::json!({
                    "index": o.index,
                    "template": o.template,
                    "output": o.output,
                    "end_line_number": o.end_line_number,
                })).collect::<Vec<_>>(),
            });
            match &args.out {
                Some(path) => {
                    // 事务语义的最后一环：**全部工序成功后**才落盘，
                    // 且只写一次。失败路径上根本走不到这里。
                    write_out_file(path, &result.program)?;
                    let mut text = format!(
                        "已写入: {}\n  工序 {} 道，非空行 {}\n",
                        path.display(),
                        result.ops.len(),
                        result.line_count()
                    );
                    if opts.line_numbers {
                        text.push_str(&format!(
                            "  行号已跨工序续编（末行号 {}）\n",
                            result.ops.last().map(|o| o.end_line_number).unwrap_or(0)
                        ));
                    }
                    let mut data = data;
                    data["output_file"] = serde_json::json!(path.display().to_string());
                    ctx.style.print_ok(&text, data);
                }
                None => {
                    // stdout 要留给 G-code，故摘要信息不进 stdout
                    // （与 `render` 的约定一致）。JSON 模式下 print_ok 输出结构化载荷。
                    let mut data = data;
                    data["output"] = serde_json::json!(result.program);
                    ctx.style.print_ok(&result.program, data);
                }
            }
            Ok(())
        }
        Err(err) => Err(part_error_to_cli(err, &args.part)),
    }
}

/// 把编排错误转成 CLI 错误：**分类要准**，用户才知道该改哪里。
///
/// - 零件定义形状不合法（`ops` 空、`template` 空）→ `args`（用户输入问题）
/// - 工序失败 → `validation`（参数/模板/渲染层面的问题，与 `render` 同类）
///
/// 错误正文含**全部**失败工序（core 已聚合），一次修完不必反复试。
fn part_error_to_cli(err: PartError, path: &Path) -> CliError {
    match err {
        PartError::InvalidSpec(msg) => {
            CliError::new("args", format!("零件定义不合法 {}: {msg}", path.display()))
        }
        PartError::OperationsFailed { failures } => {
            // 多行正文：统一错误输出只给首行加 `error: ` 前缀，故把
            // "哪几道失败"逐行列出，首行是概括，读日志时一眼能看到全貌。
            let mut msg = format!("{} 道工序失败（未写出任何文件）", failures.len());
            for f in &failures {
                msg.push_str(&format!(
                    "\n  工序{}（{}）: {}",
                    f.index + 1,
                    f.name,
                    f.error
                ));
            }
            CliError::new("validation", msg)
        }
    }
}

/// 写输出文件（父目录缺失时创建，与 `templates new` / `render` 的策略一致）。
///
/// 与 `render::write_out_file` 的区别：零件生成的输出是**拼接产物**，不可能
/// 与源模板同路径，故不需要"拒绝写入源模板自身"的检查。
fn write_out_file(path: &Path, out: &str) -> Result<(), CliError> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_spec_maps_to_args_kind() {
        let err = part_error_to_cli(
            PartError::InvalidSpec("ops 为空".to_string()),
            Path::new("p.json"),
        );
        assert_eq!(err.kind, "args", "定义形状问题是用户输入问题");
        assert!(err.message.contains("ops 为空"), "{}", err.message);
        assert!(
            err.message.contains("p.json"),
            "应指出是哪个文件: {}",
            err.message
        );
    }

    #[test]
    fn op_failures_map_to_validation_kind_and_list_every_op() {
        use nctool_core::part::OpFailure;
        let err = part_error_to_cli(
            PartError::OperationsFailed {
                failures: vec![
                    OpFailure {
                        index: 0,
                        name: "hdr".to_string(),
                        error: "模板不存在: hdr".to_string(),
                    },
                    OpFailure {
                        index: 2,
                        name: "drill".to_string(),
                        error: "参数校验未通过：缺 x".to_string(),
                    },
                ],
            },
            Path::new("p.json"),
        );
        assert_eq!(err.kind, "validation", "工序列问题与 render 同类");
        let m = &err.message;
        assert!(m.starts_with("2 道工序失败"), "首行应是概括: {m}");
        assert!(m.contains("未写出任何文件"), "应明确告知没有半成品: {m}");
        assert!(m.contains("工序1（hdr）"), "应列出第一道: {m}");
        assert!(m.contains("工序3（drill）"), "下标 2 应展示为工序3: {m}");
        // 两道都要出现 —— 聚合语义在呈现层不能丢
        assert!(m.contains("模板不存在: hdr") && m.contains("缺 x"), "{m}");
    }

    #[test]
    fn error_message_lines_are_indented_for_multiline_output() {
        use nctool_core::part::OpFailure;
        let err = part_error_to_cli(
            PartError::OperationsFailed {
                failures: vec![OpFailure {
                    index: 0,
                    name: "a".to_string(),
                    error: "boom".to_string(),
                }],
            },
            Path::new("p.json"),
        );
        let lines: Vec<&str> = err.message.lines().collect();
        assert_eq!(lines.len(), 2, "概括 + 一条失败明细: {lines:?}");
        assert!(
            lines[1].starts_with("  "),
            "明细行应有缩进，多行输出才读得出层次: {:?}",
            lines[1]
        );
    }

    #[test]
    fn write_out_file_creates_missing_parent_dirs() {
        let dir = std::env::temp_dir().join(format!("nctool_part_cli_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let target = dir.join("nested").join("out.nc");
        write_out_file(&target, "G0 X1\n").unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "G0 X1\n");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
