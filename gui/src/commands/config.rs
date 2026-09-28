//! 配置命令：`get_config` —— **无对应 HTTP 端点**，直接从 `state.loaded` 投影只读视图。

use crate::commands::shared::CommandError;
use crate::state::AppState;

/// 生效配置只读视图（供状态栏 / 设置页展示）。
#[tauri::command]
pub async fn get_config(
    state: tauri::State<'_, AppState>,
) -> Result<serde_json::Value, CommandError> {
    Ok(serde_json::json!({
        "globalPath":     state.loaded.global_path,
        "projectPath":    state.loaded.project_path,
        "templateDir":    state.loaded.merged.template_dir,
        "defaultMachine": state.loaded.merged.default_machine,
        "customMachines": state.loaded.merged.machine.keys().collect::<Vec<_>>(),
        "warnings":       state.warnings,
    }))
}
