//! `machine` 子命令：机床预设列表 / 查看配置。

use nctool_core::machine::MachinePreset;

use crate::cli::{MachineArgs, MachineCommand, MachineShowArgs};
use crate::context::Ctx;
use crate::output::CliError;

/// `machine` 命令分发。
pub fn run(ctx: &Ctx, args: &MachineArgs) -> Result<(), CliError> {
    match &args.command {
        MachineCommand::List => list(ctx),
        MachineCommand::Show(a) => show(ctx, a),
    }
}

fn list(ctx: &Ctx) -> Result<(), CliError> {
    // 枚举规则来自 core 的单一来源（见 `MachinePreset::entries`）；
    // 文本行由 `MachineEntry::display_line` 提供，与 HTTP 侧同源。
    let entries = MachinePreset::entries(&ctx.loaded.merged.machine);
    let mut text = String::from("机床预设:\n");
    let mut presets: Vec<serde_json::Value> = Vec::new();
    for m in &entries {
        // CLI 列表有意不带完整 config（那是 `machine show` 的职责）
        presets.push(serde_json::json!({
            "id": m.id,
            "vendor": m.vendor,
            "model": m.model,
            "builtin": m.builtin,
        }));
        text.push_str(&m.display_line());
        text.push('\n');
    }
    let data = serde_json::json!({ "machines": presets });
    ctx.style.print_ok(&text, data);
    Ok(())
}

fn show(ctx: &Ctx, args: &MachineShowArgs) -> Result<(), CliError> {
    let m = ctx.resolve_machine(Some(&args.id))?;
    let mut text = format!("机床: {}\n", m.id);
    text.push_str(&format!("  厂商: {}\n  型号: {}\n", m.vendor, m.model));
    text.push_str("  配置:\n");
    for (k, v) in &m.config {
        text.push_str(&format!("    {:<24} {}\n", k, v));
    }
    // schema 告警（A4）：未知键 / 非法值提示，不阻断命令成功
    let warnings = nctool_core::machine::validate_config_keys(&m);
    if !warnings.is_empty() {
        text.push_str("  配置告警:\n");
        for w in &warnings {
            text.push_str(&format!("    ⚠ {w}\n"));
        }
    }
    let data = serde_json::json!({
        "id": m.id,
        "vendor": m.vendor,
        "model": m.model,
        "config": m.config,
        "warnings": warnings,
    });
    ctx.style.print_ok(&text, data);
    Ok(())
}
