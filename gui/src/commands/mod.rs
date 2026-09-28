//! Tauri commands：GUI → `nctool-cli` 契约层（`nctool_cli::server::route`）的映射。
//!
//! 三条硬规则（贯穿全部子模块）：
//! 1. 参数入口是 `params_json: String`，请求体**文本拼接**（绝不 `from_str`→`to_string`，
//!    否则 JSON 下溢守卫失效）；
//! 2. 集成点统一是 `nctool_cli::server::route(&ctx, method, path, query, body)`，
//!    成功取包络 `data`、失败取 `error:{kind,message}`；
//! 3. 命令是 `async fn` + `spawn_blocking`，`Ctx` **只在工作线程内构造**
//!    （`Ctx` 是 `!Send`，跨 `await` 会编译不过 —— 这是刻意的护栏）。
//!
//! **不使用 `pub use` 重导出命令**：`tauri::generate_handler!` 会把给定路径的
//! 末段替换为 `__cmd__<name>` / `__tauri_command_name_<name>` 两个宏（由
//! `#[tauri::command]` 生成，`pub fn` 时带 `#[macro_export]`）。这两个宏**不会被
//! `pub use 模块::函数名` 一并重导出**，故 `main.rs` 必须用**完整模块路径**
//! （`commands::render::render_template`）注册。

pub mod checks;
pub mod config;
pub mod machines;
pub mod nc;
pub mod presets;
pub mod render;
pub mod shared;
pub mod templates;
