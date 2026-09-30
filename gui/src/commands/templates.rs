//! 模板命令：Tauri 只做参数适配，实际用例统一走 `nctool_cli::server::route`。

use crate::commands::shared::{load_ctx, run_route, run_route_json, spawn_failed, CommandError};

#[tauri::command]
pub async fn list_templates(category: Option<String>) -> Result<serde_json::Value, CommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        let ctx = load_ctx()?;
        let query = match category.filter(|c| !c.trim().is_empty()) {
            Some(c) => format!("category={}", percent_encode_segment(&c)),
            None => String::new(),
        };
        run_route(&ctx, "GET", "/api/templates", &query, b"")
    })
    .await
    .map_err(spawn_failed)?
}

#[tauri::command]
pub async fn get_template(name: String) -> Result<serde_json::Value, CommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        let ctx = load_ctx()?;
        let path = format!("/api/templates/{}", percent_encode_segment(&name));
        run_route(&ctx, "GET", &path, "", b"")
    })
    .await
    .map_err(spawn_failed)?
}

#[tauri::command]
pub async fn create_template(
    name: String,
    category: String,
) -> Result<serde_json::Value, CommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        let ctx = load_ctx()?;
        run_route_json(
            &ctx,
            "POST",
            "/api/templates/create",
            &serde_json::json!({"name": name, "category": category}),
        )
    })
    .await
    .map_err(spawn_failed)?
}

#[tauri::command]
pub async fn save_template(
    name: String,
    source: String,
    expect_hash: String,
) -> Result<serde_json::Value, CommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        let ctx = load_ctx()?;
        run_route_json(
            &ctx,
            "POST",
            "/api/templates/save",
            &serde_json::json!({"name": name, "source": source, "expectHash": expect_hash}),
        )
    })
    .await
    .map_err(spawn_failed)?
}

#[tauri::command]
pub async fn derive_template(
    source_name: String,
    new_name: String,
) -> Result<serde_json::Value, CommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        let ctx = load_ctx()?;
        run_route_json(
            &ctx,
            "POST",
            "/api/templates/derive",
            &serde_json::json!({"sourceName": source_name, "newName": new_name}),
        )
    })
    .await
    .map_err(spawn_failed)?
}

#[tauri::command]
pub async fn rename_template(
    old_name: String,
    new_name: String,
) -> Result<serde_json::Value, CommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        let ctx = load_ctx()?;
        run_route_json(
            &ctx,
            "POST",
            "/api/templates/rename",
            &serde_json::json!({"oldName": old_name, "newName": new_name}),
        )
    })
    .await
    .map_err(spawn_failed)?
}

/// 路径段编码遵循 RFC 3986；`/` 必须编码，避免模板逻辑名被拆成多段。
fn percent_encode_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::percent_encode_segment;

    #[test]
    fn percent_encode_segment_escapes_slash_and_utf8() {
        assert_eq!(
            percent_encode_segment("turning/demo.j2"),
            "turning%2Fdemo.j2"
        );
        assert_eq!(percent_encode_segment("通用"), "%E9%80%9A%E7%94%A8");
    }
}
