//! 机床命令：`list_machines`（`GET /api/machines`）。

use crate::commands::shared::{run_route, spawn_failed, CommandError};
use crate::state::AppState;

/// 机床列表 → `data = {machines:[...], schema:[...]}`（内置预设 + 配置自定义机床）。
#[tauri::command]
pub async fn list_machines(
    state: tauri::State<'_, AppState>,
) -> Result<serde_json::Value, CommandError> {
    let (td, dm, loaded) = state.snapshot();
    tauri::async_runtime::spawn_blocking(move || {
        let ctx = nctool_cli::context::Ctx::for_embedded(td, dm, loaded);
        run_route(&ctx, "GET", "/api/machines", "", b"")
    })
    .await
    .map_err(spawn_failed)?
}
