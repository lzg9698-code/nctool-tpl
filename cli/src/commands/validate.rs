//! `validate` 子命令：渲染前参数校验，输出结构化报告。

use nctool_core::validate::ValidationReport;

use crate::cli::ValidateArgs;
use crate::context::Ctx;
use crate::output::{CliError, OutputStyle};

use super::render::resolve_registry;

/// `validate <template>`：校验模板参数，报告错误/警告/提示。
///
/// - 模板解析与 `render` 完全一致（内置/目录模板名或文件路径均可）；
/// - 报告始终输出；存在错误时退出码 1；
/// - `--format json` 失败时输出**单个**对象（`ok:false` + 完整报告），
///   避免与统一错误输出产生两段 JSON。
pub fn run(ctx: &Ctx, args: &ValidateArgs) -> Result<(), CliError> {
    let (gen, name, _) = resolve_registry(ctx, &args.template)?;
    // 规格：`--param k=v` 的值要按规格归一（argv 没有类型信息，见 args::coerce_param_value）
    let specs = gen
        .registry()
        .get(&name)
        .map(|e| e.params.clone())
        .unwrap_or_default();
    let params = crate::context::build_params(
        args.params.params_file.as_deref(),
        &args.params.param,
        &specs,
    )?;

    // 校验：内置模板与目录模板都带规格——目录模板的规格来自头部
    // `{# PARAMS: #}` 参数表与清单 `params` 覆盖层（见 core::manifest）。
    let report: ValidationReport = gen.registry().validate(&name, &params)?;

    let has_errors = report.has_errors();
    let data = report_json(&name, &report);
    let text = format!("模板: {name}\n{}", report.summary());

    if has_errors && ctx.style == OutputStyle::Json {
        // 单对象输出：统一失败结构 {ok:false, error, data}（report 附于 data），
        // 避免 print_error 的第二个 JSON 对象破坏"单对象"契约
        let obj = serde_json::json!({
            "ok": false,
            "data": data,
            "error": { "kind": "validation", "message": report.summary() },
        });
        println!("{}", serde_json::to_string_pretty(&obj).unwrap_or_default());
        return Err(CliError::new("validation", "参数校验未通过").silent());
    }

    ctx.style.print_ok(&text, data);

    if has_errors {
        return Err(CliError::new(
            "validation",
            "参数校验未通过（详见上方报告）",
        ));
    }
    Ok(())
}

/// 报告转 JSON。
fn report_json(template: &str, report: &ValidationReport) -> serde_json::Value {
    let issues: Vec<serde_json::Value> = report
        .issues
        .iter()
        .map(|i| {
            let level = match i.level {
                nctool_core::validate::ValidationLevel::Error => "error",
                nctool_core::validate::ValidationLevel::Warning => "warning",
                nctool_core::validate::ValidationLevel::Info => "info",
            };
            serde_json::json!({
                "level": level,
                "param": i.param,
                "message": i.message,
            })
        })
        .collect();
    serde_json::json!({
        "template": template,
        "ok": report.is_ok(),
        "errors": report.errors().count(),
        "warnings": report.warnings().count(),
        "issues": issues,
    })
}
