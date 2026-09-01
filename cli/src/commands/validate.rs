//! `validate` 子命令：渲染前参数校验，输出结构化报告。

use nctool_core::validate::ValidationReport;

use crate::cli::ValidateArgs;
use crate::context::Ctx;
use crate::output::CliError;

use super::templates::resolve_source;

/// `validate <template>`：校验模板参数，报告错误/警告/提示。
pub fn run(ctx: &Ctx, args: &ValidateArgs) -> Result<(), CliError> {
    let (name, source, specs) = resolve_source(ctx, &args.template)?;
    let params =
        crate::context::build_params(args.params.params_file.as_deref(), &args.params.param)?;

    // 校验：注册表模板走 registry.validate（含系统变量 machine 豁免）；
    // 文件模板走 validate_template（specs 来自解析，若无规格传空）。
    let report: ValidationReport = match specs {
        Some(specs) => {
            nctool_core::validate::validate_template(&source, &name, &specs, &params, &["machine"])
        }
        None => {
            let gen = ctx.build_registry()?;
            gen.registry().validate(&name, &params)?
        }
    };

    let has_errors = report.has_errors();

    // 输出报告
    let data = report_json(&name, &report);
    let text = format!("模板: {name}\n{}", report.summary());
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
