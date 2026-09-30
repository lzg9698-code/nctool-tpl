//! 校验 / 检查命令：参数校验和静态 lint 均复用共享 route。

use crate::commands::shared::{load_ctx, run_route, run_route_json, spawn_failed, CommandError};

/// 校验模板参数 → `data = {report:{...}}`。
///
/// `params_json` 以文本拼接进请求体，保留下溢守卫。
#[tauri::command]
pub async fn validate_template(
    template: String,
    params_json: String,
) -> Result<serde_json::Value, CommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        let ctx = load_ctx()?;
        let body = format!(
            "{{\"template\":{t},\"params\":{p}}}",
            t = serde_json::to_string(&template).expect("模板名序列化"),
            p = params_json,
        );
        run_route(&ctx, "POST", "/api/validate", "", body.as_bytes())
    })
    .await
    .map_err(spawn_failed)?
}

/// 对已保存模板执行静态 lint → `Vec<LintFinding>`，finding 不带错误级别。
#[tauri::command]
pub async fn lint_template(template: String) -> Result<serde_json::Value, CommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        let ctx = load_ctx()?;
        run_route_json(
            &ctx,
            "POST",
            "/api/lint",
            &serde_json::json!({"template": template}),
        )
    })
    .await
    .map_err(spawn_failed)?
}

#[cfg(test)]
mod tests {
    use super::*;
    use nctool_cli::config::LoadedConfig;
    use nctool_cli::context::Ctx;

    fn test_ctx() -> Ctx {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("templates");
        Ctx::for_embedded(Some(root), None, LoadedConfig::default())
    }

    #[test]
    fn lint_route_reports_findings_for_saved_template() {
        let ctx = test_ctx();
        let templates = run_route(&ctx, "GET", "/api/templates", "", b"").unwrap();
        let name = templates["templates"][0]["name"].as_str().unwrap();
        let findings = run_route_json(
            &ctx,
            "POST",
            "/api/lint",
            &serde_json::json!({"template": name}),
        )
        .unwrap();
        assert!(findings.is_array());
        for finding in findings.as_array().unwrap() {
            assert!(finding.get("line").is_some());
            assert!(finding.get("col").is_some());
            assert!(finding.get("filter").is_some());
            assert!(finding.get("suggestion").is_some());
            assert!(finding.get("message").is_some());
            assert!(finding.get("level").is_none());
        }
    }

    #[test]
    fn lint_route_preserves_missing_template_status() {
        let ctx = test_ctx();
        let err = run_route_json(
            &ctx,
            "POST",
            "/api/lint",
            &serde_json::json!({"template": "__missing_template__"}),
        )
        .unwrap_err();
        assert_eq!(err.kind, "template_not_found");
        assert_eq!(err.status, 404);
    }
}
