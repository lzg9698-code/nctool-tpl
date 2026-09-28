//! 参数预设命令：`list_presets` / `save_preset` / `delete_preset`。
//!
//! 三条命令全部复用 cli 契约层的**已有端点**（见 `scripts/api_routes.json`）：
//! - `list_presets`   → `GET  /api/presets`（`?template=<名>` 过滤）
//! - `save_preset`    → `POST /api/presets`（体 `{name,template,params,force}`）
//! - `delete_preset`  → `POST /api/presets/delete`（体 `{name}`）
//!
//! ★ 参数通道安全：`save_preset` 的 `params_json` 以**文本**拼接进请求体，
//! 绝不 `from_str`→`to_string` —— `presets_save` 在 `serde_json::from_slice`
//! **之前**先做 `json_underflow_resp`（`cli/src/server.rs`），若 GUI 侧先把
//! `1e-400` 解析再重序列化，下溢字面量会被静默归零，守卫永久失效（勘误 E10）。
//! 故 body 构造抽成纯函数 [`build_save_preset_body`] 以便单测。

use crate::commands::shared::{percent_encode_segment, run_route, spawn_failed, CommandError};
use crate::state::AppState;

/// `GET /api/presets` → `data = {path, pathRedacted, presets:[...], warnings?}`。
///
/// `path` 为 `~/...` **脱敏形态**（P1-9，不含用户名绝对路径），`pathRedacted` 恒为 `true`。
///
/// `template` 为空 / 仅空白 → 不筛选（发空 query）。
/// 列表项含结构化陈旧字段（`resolvable` / `stale` / `staleParams` / `missingRequired`）；
/// 损坏文件降级为警告而非报错（D13）。
#[tauri::command]
pub async fn list_presets(
    template: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<serde_json::Value, CommandError> {
    let (td, dm, loaded) = state.snapshot();
    tauri::async_runtime::spawn_blocking(move || {
        let ctx = nctool_cli::context::Ctx::for_embedded(td, dm, loaded);
        let query = match template.filter(|t| !t.trim().is_empty()) {
            Some(t) => format!("template={}", percent_encode_segment(&t)),
            None => String::new(),
        };
        run_route(&ctx, "GET", "/api/presets", &query, b"")
    })
    .await
    .map_err(spawn_failed)?
}

/// ★ 关键：`params_json` 以**文本**拼接进请求体，绝不 `from_str`→`to_string`。
///
/// 输出形状（键序固定，便于逐字断言）：
/// `{"name":<n>,"template":<t>,"params":<原样文本>,"force":<bool>}`
///
/// 若把 `params_json` 先 `serde_json::from_str` 再 `to_string`，下溢字面量
/// （如 `1e-400`）会被静默归零成 `0.0`，`presets_save` 的文本层下溢守卫随之失效。
pub(crate) fn build_save_preset_body(
    name: &str,
    template: &str,
    params_json: &str,
    force: bool,
) -> String {
    format!(
        "{{\"name\":{n},\"template\":{t},\"params\":{p},\"force\":{f}}}",
        n = serde_json::to_string(name).expect("预设名序列化"),
        t = serde_json::to_string(template).expect("模板名序列化"),
        p = params_json, // ← 原样文本
        f = force,
    )
}

/// 保存 / 覆盖预设 →
/// `data = {name,template,paramCount,path,pathRedacted,action,specFingerprint,fileFingerprint}`
///（`path` 为 `~/...` 脱敏形态，P1-9）。
///
/// 同名默认拒绝（`409 name_conflict`）；前端确认覆盖后带 `force=true` 重试。
/// `force` 用 `Option<bool>`：前端可省略，缺省即 `false`。
#[tauri::command]
pub async fn save_preset(
    name: String,
    template: String,
    params_json: String,
    force: Option<bool>,
    state: tauri::State<'_, AppState>,
) -> Result<serde_json::Value, CommandError> {
    let (td, dm, loaded) = state.snapshot();
    tauri::async_runtime::spawn_blocking(move || {
        let ctx = nctool_cli::context::Ctx::for_embedded(td, dm, loaded);
        let body = build_save_preset_body(&name, &template, &params_json, force.unwrap_or(false));
        run_route(&ctx, "POST", "/api/presets", "", body.as_bytes())
    })
    .await
    .map_err(spawn_failed)?
}

/// 删除预设 → `data = {name,path,pathRedacted,action,fileFingerprint}`
///（`path` 为 `~/...` 脱敏形态，P1-9）。
///
/// 走 POST（`/api/presets/delete`）而非 `DELETE` + 路径段：预设名可含 `.`/`-`，
/// 放路径段需额外编码；删除属**有副作用**操作，必须经 `PresetStore`
/// （乐观锁 + 原子写），不能直接删文件。预设不存在 → `404 preset_not_found`。
#[tauri::command]
pub async fn delete_preset(
    name: String,
    state: tauri::State<'_, AppState>,
) -> Result<serde_json::Value, CommandError> {
    let (td, dm, loaded) = state.snapshot();
    tauri::async_runtime::spawn_blocking(move || {
        let ctx = nctool_cli::context::Ctx::for_embedded(td, dm, loaded);
        let body = format!(
            "{{\"name\":{}}}",
            serde_json::to_string(&name).expect("预设名序列化")
        );
        run_route(&ctx, "POST", "/api/presets/delete", "", body.as_bytes())
    })
    .await
    .map_err(spawn_failed)?
}

#[cfg(test)]
mod tests {
    use super::*;
    use nctool_cli::config::LoadedConfig;
    use nctool_cli::context::Ctx;

    /// 仓库根下的 `templates/`（`CARGO_MANIFEST_DIR` = `.../rustjinja/gui`）。
    fn repo_templates() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("gui 的上级目录 = 仓库根")
            .join("templates")
    }

    fn ctx() -> Ctx {
        Ctx::for_embedded(Some(repo_templates()), None, LoadedConfig::default())
    }

    /// ① 请求体构造：`params` 文本拼接、`force` 布尔、键序固定（逐字相等）。
    #[test]
    fn build_save_preset_body_shape() {
        let body = build_save_preset_body(
            "粗加工-1",
            "drill_cycle",
            r#"{"x":21,"y":15,"depth":-10,"feed":100}"#,
            false,
        );
        assert_eq!(
            body,
            r#"{"name":"粗加工-1","template":"drill_cycle","params":{"x":21,"y":15,"depth":-10,"feed":100},"force":false}"#
        );
    }

    /// ①b `force=true`（覆盖流程）同样逐字相等。
    #[test]
    fn build_save_preset_body_force_true() {
        let body = build_save_preset_body("p", "t", r#"{"a":1}"#, true);
        assert_eq!(
            body,
            r#"{"name":"p","template":"t","params":{"a":1},"force":true}"#
        );
    }

    /// ② 门槛项：JSON 下溢必须**硬失败**（`num_underflow` / 400），且**不落盘**。
    ///
    /// 刻意用**不存在的模板名**：反向验证（把文本拼接改成 `from_str`→`to_string`）
    /// 时守卫失效，流程会先撞上 `template_not_found`（而非真的把预设写进用户的
    /// 真实文件 `%APPDATA%\nctool\presets.yaml`）—— 测试因此 FAILED，且无副作用。
    #[test]
    fn save_preset_rejects_underflow_without_touching_file() {
        let preset_file = nctool_core::asset::default_preset_path();
        let before = std::fs::read(&preset_file).ok();

        let body = build_save_preset_body(
            "adv_underflow",
            "__nonexistent_template_for_guard__",
            r#"{"x":1e-400}"#,
            false,
        );
        let err = run_route(&ctx(), "POST", "/api/presets", "", body.as_bytes())
            .expect_err("下溢字面量必须硬失败，不得写盘");
        assert_eq!(err.kind, "num_underflow");
        assert_eq!(err.status, 400);

        let after = std::fs::read(&preset_file).ok();
        assert_eq!(
            before,
            after,
            "下溢必须在写盘前拦下：预设文件 {} 不得被改动",
            preset_file.display()
        );
    }
}
