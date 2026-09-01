//! `config` 子命令：初始化示例配置 / 展示生效配置。

use std::path::PathBuf;

use crate::cli::{ConfigArgs, ConfigCommand};
use crate::config;
use crate::context::Ctx;
use crate::output::CliError;

/// `config` 命令分发。
pub fn run(ctx: &Ctx, args: &ConfigArgs) -> Result<(), CliError> {
    match &args.command {
        ConfigCommand::Init => init(ctx),
        ConfigCommand::Show => show(ctx),
    }
}

/// `config init`：生成示例 nctool.toml 到当前目录。
fn init(ctx: &Ctx) -> Result<(), CliError> {
    let path = PathBuf::from("nctool.toml");
    config::init_config(&path)?;
    let text = format!("已生成示例配置: {}\n", path.display());
    let data = serde_json::json!({ "path": path.display().to_string() });
    ctx.style.print_ok(&text, data);
    Ok(())
}

/// `config show`：展示全局 + 项目层叠合并后的生效配置。
fn show(ctx: &Ctx) -> Result<(), CliError> {
    let merged = config::merged_config()?;
    let text = merged_text(&merged);
    let data = serde_json::json!({
        "template_dir": merged.template_dir,
        "default_machine": merged.default_machine,
        "machine": merged.machine,
    });
    ctx.style.print_ok(&text, data);
    Ok(())
}

fn merged_text(cfg: &config::NctoolConfig) -> String {
    let mut s = String::from("生效配置:\n");
    s.push_str(&format!(
        "  模板目录: {}\n",
        cfg.template_dir
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "(未设置)".to_string())
    ));
    s.push_str(&format!(
        "  默认机床: {}\n",
        cfg.default_machine.as_deref().unwrap_or("generic")
    ));
    s.push_str(&format!("  自定义机床: {} 个\n", cfg.machine.len()));
    for (id, m) in &cfg.machine {
        s.push_str(&format!("    {id} ({} {})\n", m.vendor, m.model));
    }
    s
}
