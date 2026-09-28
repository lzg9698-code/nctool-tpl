//! nctool-cli —— nctool 命令行工具的**库形态**。
//!
//! 7 个模块中仅 5 个对外公开：GUI 复用 `server::route` 契约层、
//! `context::Ctx`、`config` 配置层、`output::CliError`/下溢守卫，
//! 以及 `cli`（`Cli` / `GlobalArgs` / `Command`，供 bin 入口解析）。
//! `args` / `commands` 仅供 crate 内部（含 bin）使用。
//!
//! **可见性依据**：
//! - `main.rs`（bin target）需要 `cli::Cli`、`output::OutputStyle` → `cli`、`output` 必须 `pub`。
//! - GUI 需要 `server::route`、`context::Ctx`、`config::LoadedConfig`、
//!   `output::CliError` → 这 4 个 `pub`。
//! - `args` / `commands` 无需对外：`Command::run` 是 `pub` 方法、`Command` 是 `pub`
//!   类型，**inherent impl 的方法可见性只取决于方法自身与类型的可达性，与 impl
//!   所在模块无关** —— 因此 `mod commands;` 保持私有，`main.rs` 仍可
//!   `cli.command.run(...)`。
//!
//! **不要**给本文件加 `#![warn(missing_docs)]`（现有 bin 没有；加上会引入大量
//! doc warning，`cargo doc -D warnings` 会红）。新公开项（`Ctx::for_embedded`）
//! 仍按惯例写 doc 注释。

pub mod cli; // Cli / GlobalArgs / Command / CategoryArg / FormatArg —— main.rs 用
pub mod config; // LoadedConfig / load / init_config
pub mod context; // Ctx / Ctx::for_embedded
pub mod output; // CliError / OutputStyle / json_underflow_error / report_json
pub mod server; // route / Resp —— GUI 复用的契约层

mod args; // parameter_set_from_json / read_text_capped —— 仅 crate 内部
mod commands; // Command::run 的落地 —— 仅 crate 内部
