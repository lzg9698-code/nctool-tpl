//! nctool-gui —— nctool 桌面 GUI（Tauri 2 + React）。
//!
//! 进程内复用 `nctool-cli` 的服务契约层（`nctool_cli::server::route`）：每个
//! Tauri command 在工作线程内构造 `Ctx` → 拼 HTTP 请求体 → 调 `route`，把返回的
//! `{ok,data}` / `{ok,error}` 包络映射为 `Result<serde_json::Value, CommandError>`。
//!
//! Tauri 命令在工作线程内适配参数后调用共享 `route()`；HTTP API 与桌面 GUI 的契约
//! 在 `scripts/api_routes.json` 中对拍，业务校验和资产写入不再由两侧分别编排。

// release 构建不弹控制台窗口（仅 Windows 生效；其它平台此属性不参与编译）。
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod commands;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            commands::render::render_template,
            commands::templates::list_templates,
            commands::templates::get_template,
            commands::templates::create_template,
            commands::templates::save_template,
            commands::templates::derive_template,
            commands::templates::rename_template,
            commands::checks::validate_template,
            // 检查页通过共享 POST /api/lint 契约执行静态检查。
            commands::checks::lint_template,
            commands::machines::list_machines,
            commands::machines::save_machine,
            commands::machines::delete_machine,
            commands::config::get_config,
            commands::nc::save_nc_file,
            // Phase 2 第一批：参数预设页（复用 GET/POST /api/presets* 三条端点）。
            commands::presets::list_presets,
            commands::presets::save_preset,
            commands::presets::delete_preset,
        ])
        .run(tauri::generate_context!())
        .expect("运行 nctool-gui 失败");
}
