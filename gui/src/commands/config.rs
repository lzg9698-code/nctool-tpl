//! 配置命令：复用服务契约中的 `GET /api/config` 只读视图。

use crate::commands::shared::{load_ctx, run_route, spawn_failed, CommandError};

#[tauri::command]
pub async fn get_config() -> Result<serde_json::Value, CommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        let ctx = load_ctx()?;
        run_route(&ctx, "GET", "/api/config", "", b"")
    })
    .await
    .map_err(spawn_failed)?
}
