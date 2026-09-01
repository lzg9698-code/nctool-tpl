//! `machine` 子命令：机床预设列表 / 查看配置。

use nctool_core::machine::MachinePreset;

use crate::cli::{MachineArgs, MachineCommand, MachineShowArgs};
use crate::config;
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
    let mut presets: Vec<serde_json::Value> = Vec::new();
    let mut text = String::from("机床预设:\n");
    for p in MachinePreset::all() {
        let cfg = p.config();
        presets.push(serde_json::json!({
            "id": p.id(),
            "vendor": cfg.vendor,
            "model": cfg.model,
        }));
        text.push_str(&format!("  {:<12} {} {}\n", p.id(), cfg.vendor, cfg.model));
    }
    // 追加配置文件中的自定义机床
    let cfg = config::merged_config()?;
    for (id, m) in &cfg.machine {
        if MachinePreset::from_id(id).is_none() {
            presets.push(serde_json::json!({
                "id": id,
                "vendor": m.vendor,
                "model": m.model,
            }));
            text.push_str(&format!("  {:<12} {} {} (自定义)\n", id, m.vendor, m.model));
        }
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
    let data = serde_json::json!({
        "id": m.id,
        "vendor": m.vendor,
        "model": m.model,
        "config": m.config,
    });
    ctx.style.print_ok(&text, data);
    Ok(())
}
