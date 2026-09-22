//! `lint` 子命令：模板静态检查，在渲染前发现会导致错误 G-code 的常见笔误。
//!
//! 首个（也是目前唯一）检查项是**三角函数度制风险**：minijinja 的 `sin`/`cos`/
//! `tan` 以**弧度**为单位，而 G-code 角度几乎总是**度**。写 `{{ 30 | sin }}`
//! 不会报错，只会静默产出错误坐标（撞刀级）。本命令对这类用法给警告，建议
//! 改用度制变体 `sin_d` 等。
//!
//! - 有发现项时退出码 1（与 `validate` 一致：静态检查有结论即非零，便于 CI 门禁）；
//! - 无发现项退出 0；
//! - 语法错误退出 6（与渲染入口一致）；
//! - `--format json` 输出结构化发现项（失败时单对象 `{ok:false,error,data}`）。

use crate::cli::LintArgs;
use crate::context::Ctx;
use crate::output::{write_stdout_quiet, CliError, OutputStyle};

use super::templates::resolve_source;

/// `lint <template>`：对模板做静态检查。
pub fn run(ctx: &Ctx, args: &LintArgs) -> Result<(), CliError> {
    let (name, source, _, _) = resolve_source(ctx, &args.template)?;

    // 解析错误按渲染入口的语义转为 CliError（kind=render → 退出码 6）
    let findings =
        nctool_tpl::lint(&source, &name).map_err(|e| CliError::new("render", e.to_string()))?;

    let has_findings = !findings.is_empty();
    let summary = format!("模板 {name} 有 {} 处静态检查发现", findings.len());

    if ctx.style == OutputStyle::Json {
        let items: Vec<serde_json::Value> = findings
            .iter()
            .map(|f| {
                serde_json::json!({
                    "line": f.line,
                    "col": f.col,
                    "filter": f.filter,
                    "suggestion": f.suggestion,
                    "message": f.message,
                })
            })
            .collect();
        let data = serde_json::json!({
            "template": name,
            "findings": items,
            "finding_count": findings.len(),
        });

        // 单对象失败结构：与 validate 的契约一致（ok:false + error + data），
        // 避免 print_error 的第二个 JSON 对象破坏"单对象"契约。走 silent()。
        if has_findings {
            let obj = serde_json::json!({
                "ok": false,
                "data": data,
                "error": { "kind": "validation", "message": summary },
            });
            write_stdout_quiet(&format!(
                "{}\n",
                serde_json::to_string_pretty(&obj).unwrap_or_default()
            ));
            return Err(CliError::new("validation", summary).silent());
        }
        return ack_json(&data);
    }

    // 文本输出
    let mut text = format!("模板: {name}\n");
    if findings.is_empty() {
        text.push_str("静态检查通过：未发现问题\n");
    } else {
        for f in &findings {
            text.push_str(&format!(
                "警告 [{}:{}] 过滤器 `{}`：{}\n",
                f.line, f.col, f.filter, f.message
            ));
        }
        text.push_str(&format!("\n共 {} 处发现。\n", findings.len()));
    }

    if has_findings {
        // 正文走 stdout（与 validate 一致：报告在 stdout、错误摘要在 stderr）
        write_stdout_quiet(&text);
        return Err(CliError::new("validation", summary));
    }
    write_stdout_quiet(&text);
    Ok(())
}

/// 无发现项时的 JSON 成功输出（单对象）。
fn ack_json(data: &serde_json::Value) -> Result<(), CliError> {
    let obj = serde_json::json!({ "ok": true, "data": data });
    write_stdout_quiet(&format!(
        "{}\n",
        serde_json::to_string_pretty(&obj).unwrap_or_default()
    ));
    Ok(())
}
