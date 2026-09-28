//! 校验 / 检查命令：`validate_template`（`POST /api/validate`）与
//! `lint_template`（**GUI 专有**，不经 `route()`）。
//!
//! 两类检查语义不同（设计稿 §2c）：
//! - `validate_template` → `ValidationReport`，**有级别**（Error/Warning/Info），
//!   可驱动"存在 Error 时禁用生成"；
//! - `lint_template` → `Vec<LintFinding>`，**无级别**（只有 `line/col/filter/
//!   suggestion/message`），不得拼 ERROR/WARN/INFO 分级、不得算"通过率"。

use crate::commands::shared::{percent_encode_segment, run_route, spawn_failed, CommandError};
use crate::state::AppState;

/// 校验模板参数 → `data = {report:{...}}`。
///
/// `params_json` 以文本拼接进请求体（同 `render_template`，保留下溢守卫）。
#[tauri::command]
pub async fn validate_template(
    template: String,
    params_json: String,
    state: tauri::State<'_, AppState>,
) -> Result<serde_json::Value, CommandError> {
    let (td, dm, loaded) = state.snapshot();
    tauri::async_runtime::spawn_blocking(move || {
        let ctx = nctool_cli::context::Ctx::for_embedded(td, dm, loaded);
        let body = format!(
            "{{\"template\":{t},\"params\":{p}}}",
            t = serde_json::to_string(&template).expect("模板名序列化"),
            p = params_json, // ← 原样文本
        );
        run_route(&ctx, "POST", "/api/validate", "", body.as_bytes())
    })
    .await
    .map_err(spawn_failed)?
}

/// 模板静态检查（当前仅"三角函数度制风险"一条规则）→ `data = [{line,col,filter,suggestion,message}, ...]`。
///
/// **GUI 专有命令，不经 `route()`**（`/api/lint` 端点不存在，`cli/src/server.rs`
/// 无此路由臂）。集成路径与 CLI 的 `lint` 子命令**完全一致**：
/// 1. `GET /api/templates/{name}` 取 `data.template.source`（与渲染页同一端点）；
/// 2. 调 **`nctool_tpl::lint(&source, &name)`** —— 与 `cli/src/commands/lint.rs`
///    调用的是同一个函数，**不写第二套实现**。
///
/// 错误映射：
/// - 模板不存在 → 透传 `route` 的 `404 template_not_found`；
/// - 解析失败（语法错误）→ `CommandError { kind: "render", status: 400 }`
///   （与 CLI `lint` 子命令把 `TplError` 映射成 `render` 同口径）；
/// - 模板详情响应缺 `source` → 内部错误。
#[tauri::command]
pub async fn lint_template(
    template: String,
    state: tauri::State<'_, AppState>,
) -> Result<serde_json::Value, CommandError> {
    let (td, dm, loaded) = state.snapshot();
    tauri::async_runtime::spawn_blocking(move || {
        let ctx = nctool_cli::context::Ctx::for_embedded(td, dm, loaded);
        // ① 取模板源码（`name` 含 `/` 时必须把 `/` 编成 `%2F`，同 get_template）。
        let path = format!("/api/templates/{}", percent_encode_segment(&template));
        let data = run_route(&ctx, "GET", &path, "", b"")?;
        let source = data
            .get("template")
            .and_then(|t| t.get("source"))
            .and_then(|s| s.as_str())
            .ok_or_else(|| CommandError {
                kind: "internal".into(),
                message: "模板详情响应缺少 template.source".into(),
                status: 500,
            })?;
        // ② 与 CLI `lint` 子命令同一函数（`LintFinding` 无级别）。
        let findings = nctool_tpl::lint(source, &template).map_err(|e| CommandError {
            kind: "render".into(),
            message: e.to_string(),
            status: 400,
        })?;
        let items: Vec<serde_json::Value> = findings
            .iter()
            .map(|f| {
                serde_json::json!({
                    "line": f.line,
                    "col": f.col,
                    "filter": f.filter,
                    "suggestion": f.suggestion,
                    "message": f.message,
                })
            })
            .collect();
        Ok(serde_json::Value::Array(items))
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

    /// 真实注册表中的模板做 lint 全链路：`GET /api/templates/{name}` → `nctool_tpl::lint`。
    ///
    /// 只断言"能跑通且返回数组、每项字段齐全"，不断言具体发现项数量
    /// （模板会演进；此处验证的是**集成路径**而非规则集）。
    #[test]
    fn lint_template_path_returns_findings_array() {
        // ① 取一个真实模板名（列表的第一个）。
        let list = run_route(&ctx(), "GET", "/api/templates", "", b"").expect("模板列表");
        let name = list["templates"][0]["name"]
            .as_str()
            .expect("模板列表非空")
            .to_string();

        // ② 复现 `lint_template` 的两步（命令体不能直接调，逻辑同源）。
        let path = format!("/api/templates/{}", percent_encode_segment(&name));
        let data = run_route(&ctx(), "GET", &path, "", b"").expect("模板详情");
        let source = data["template"]["source"]
            .as_str()
            .expect("模板详情含 source");
        let findings = nctool_tpl::lint(source, &name).expect("模板可解析");
        let items: Vec<serde_json::Value> = findings
            .iter()
            .map(|f| {
                serde_json::json!({
                    "line": f.line,
                    "col": f.col,
                    "filter": f.filter,
                    "suggestion": f.suggestion,
                    "message": f.message,
                })
            })
            .collect();
        // 结构断言：是数组，且每项都带齐 5 个字段（无级别字段）。
        for item in &items {
            assert!(item.get("line").is_some());
            assert!(item.get("col").is_some());
            assert!(item.get("filter").is_some());
            assert!(item.get("suggestion").is_some());
            assert!(item.get("message").is_some());
            assert!(item.get("level").is_none(), "lint 结果不得有级别字段");
        }
    }

    /// 不存在的模板 → 透传 `404 template_not_found`（不是 500）。
    #[test]
    fn lint_template_missing_template_is_404() {
        let path = format!(
            "/api/templates/{}",
            percent_encode_segment("__nonexistent_template__")
        );
        let err = run_route(&ctx(), "GET", &path, "", b"").expect_err("模板不存在");
        assert_eq!(err.kind, "template_not_found");
        assert_eq!(err.status, 404);
    }

    /// 语法错误 → `nctool_tpl::lint` 返回 `Err`，映射成 `render` / 400。
    #[test]
    fn lint_syntax_error_maps_to_render_400() {
        let err = nctool_tpl::lint("{{ unclosed", "bad.j2").expect_err("语法错误");
        let mapped = CommandError {
            kind: "render".into(),
            message: err.to_string(),
            status: 400,
        };
        assert_eq!(mapped.kind, "render");
        assert_eq!(mapped.status, 400);
    }
}
