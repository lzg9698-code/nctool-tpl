//! 模板命令：`list_templates` / `get_template`。

use crate::commands::shared::{percent_encode_segment, run_route, spawn_failed, CommandError};
use crate::state::AppState;

/// 列出模板 → `data = {templates:[{name,category,description}]}`。
///
/// `category` 为空/仅空白 → 不筛选（发空 query）。
#[tauri::command]
pub async fn list_templates(
    category: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<serde_json::Value, CommandError> {
    let (td, dm, loaded) = state.snapshot();
    tauri::async_runtime::spawn_blocking(move || {
        let ctx = nctool_cli::context::Ctx::for_embedded(td, dm, loaded);
        let query = match category.filter(|c| !c.trim().is_empty()) {
            Some(c) => format!("category={}", percent_encode_segment(&c)),
            None => String::new(),
        };
        run_route(&ctx, "GET", "/api/templates", &query, b"")
    })
    .await
    .map_err(spawn_failed)?
}

/// 模板详情 → `data = {template:{name,category,description,builtin,source,params,variables}}`。
///
/// **渲染页表单的数据源**（`params` 齐备）。不用 `/api/inspect` —— 它不返回
/// `params`（勘误 E3）。模板名含 `/` 时必须把 `/` 编成 `%2F`（见 `percent_encode_segment`）。
#[tauri::command]
pub async fn get_template(
    name: String,
    state: tauri::State<'_, AppState>,
) -> Result<serde_json::Value, CommandError> {
    let (td, dm, loaded) = state.snapshot();
    tauri::async_runtime::spawn_blocking(move || {
        let ctx = nctool_cli::context::Ctx::for_embedded(td, dm, loaded);
        let path = format!("/api/templates/{}", percent_encode_segment(&name));
        run_route(&ctx, "GET", &path, "", b"")
    })
    .await
    .map_err(spawn_failed)?
}
