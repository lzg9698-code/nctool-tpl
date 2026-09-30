//! 机床命令：Tauri 只适配参数与返回字段，共享 HTTP route 的校验和写入用例。

use crate::commands::shared::{load_ctx, run_route, run_route_json, spawn_failed, CommandError};

#[tauri::command]
pub async fn list_machines() -> Result<serde_json::Value, CommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        let ctx = load_ctx()?;
        let mut data = run_route(&ctx, "GET", "/api/machines", "", b"")?;
        // 保留桌面前端既有字段名；HTTP 合约使用 configFingerprint。
        data["fileFingerprint"] = data
            .get("configFingerprint")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        Ok(data)
    })
    .await
    .map_err(spawn_failed)?
}

#[tauri::command]
pub async fn save_machine(
    id: String,
    vendor: String,
    model: String,
    config: std::collections::BTreeMap<String, String>,
    expect_hash: Option<String>,
) -> Result<serde_json::Value, CommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        let ctx = load_ctx()?;
        run_route_json(
            &ctx,
            "POST",
            "/api/machines",
            &serde_json::json!({
                "id": id,
                "vendor": vendor,
                "model": model,
                "config": config,
                "expectHash": expect_hash,
            }),
        )
    })
    .await
    .map_err(spawn_failed)?
}

#[tauri::command]
pub async fn delete_machine(
    id: String,
    expect_hash: Option<String>,
) -> Result<serde_json::Value, CommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        let ctx = load_ctx()?;
        run_route_json(
            &ctx,
            "POST",
            "/api/machines/delete",
            &serde_json::json!({"id": id, "expectHash": expect_hash}),
        )
    })
    .await
    .map_err(spawn_failed)?
}
