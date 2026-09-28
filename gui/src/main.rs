//! nctool-gui —— nctool 桌面 GUI（Tauri 2 + React）。
//!
//! 进程内复用 `nctool-cli` 的服务契约层（`nctool_cli::server::route`）：每个
//! Tauri command 在工作线程内构造 `Ctx` → 拼 HTTP 请求体 → 调 `route`，把返回的
//! `{ok,data}` / `{ok,error}` 包络映射为 `Result<serde_json::Value, CommandError>`。
//!
//! **不新增端点、不改 `scripts/api_routes.json`** —— GUI 与 Web UI 走同一份契约。

// release 构建不弹控制台窗口（仅 Windows 生效；其它平台此属性不参与编译）。
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod commands;
mod state;

use state::AppState;

fn main() {
    // 启动即加载配置；失败时**回退默认值**（保证窗口能起来），把错误写进 warnings。
    let state = AppState::load(None, None).unwrap_or_else(|e| {
        eprintln!("warning: 加载配置失败，使用默认值: {e}");
        AppState {
            template_dir: None,
            default_machine: None,
            loaded: Default::default(),
            warnings: vec![e.to_string()],
        }
    });

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            commands::render::render_template,
            commands::templates::list_templates,
            commands::templates::get_template,
            commands::checks::validate_template,
            // Phase 2 第一批：检查页的静态检查（GUI 专有，不经 route()）。
            commands::checks::lint_template,
            commands::machines::list_machines,
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
