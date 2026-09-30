//! 命令层共享设施：错误类型、契约调用、`spawn_blocking` 错误映射、URL 工具。

use nctool_cli::context::Ctx;

/// 命令错误：映射自 HTTP 包络 `error:{kind,message}`（或内部错误），
/// 经 Tauri 的 Promise reject 回传前端，保留 `kind` 供前端分支。
#[derive(Debug, serde::Serialize)]
pub struct CommandError {
    /// 错误分类（与 HTTP 包络 `error.kind` 逐字对齐，如 `num_underflow` / `bad_request`）
    pub kind: String,
    /// 人类可读错误描述
    pub message: String,
    /// 对应 HTTP 状态码（内部错误统一 500）
    pub status: u16,
}

/// `spawn_blocking` 任务调度失败 → 统一内部错误。
pub(crate) fn spawn_failed(e: impl std::fmt::Display) -> CommandError {
    CommandError {
        kind: "internal".into(),
        message: format!("任务调度失败: {e}"),
        status: 500,
    }
}

/// 调用 cli 契约层：`route()` → `Result<data, CommandError>`。
///
/// - 成功（包络 `{ok:true,data}`）→ 原样转发 `data`（**不再投影成强类型**，
///   避免在 GUI 侧复制一份"契约影子"，新增字段被静默丢弃）。
/// - 失败（包络 `{ok:false,error:{kind,message}}`）→ `Err(CommandError{kind,message,status})`。
/// - `Resp::Html`（非 API 响应，理论不可达）→ 内部错误。
pub(crate) fn run_route(
    ctx: &nctool_cli::context::Ctx,
    method: &str,
    path: &str,
    query: &str,
    body: &[u8],
) -> Result<serde_json::Value, CommandError> {
    use nctool_cli::server::{route, Resp};
    match route(ctx, method, path, query, body) {
        Resp::Json(status, env) => {
            if env.get("ok").and_then(|v| v.as_bool()) == Some(true) {
                Ok(env.get("data").cloned().unwrap_or(serde_json::Value::Null))
            } else {
                let e = env.get("error");
                Err(CommandError {
                    kind: e
                        .and_then(|e| e.get("kind"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("internal")
                        .into(),
                    message: e
                        .and_then(|e| e.get("message"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("未知错误")
                        .into(),
                    status,
                })
            }
        }
        Resp::Html => Err(CommandError {
            kind: "internal".into(),
            message: "非 API 响应".into(),
            status: 500,
        }),
    }
}

/// 在桌面命令中读取最新的层叠配置并重建共享服务上下文。
///
/// 每个命令都重新加载层叠配置；Web 与桌面入口通过同一 `Ctx`/`route`
/// 路径执行核心用例。
pub(crate) fn load_ctx() -> Result<Ctx, CommandError> {
    let loaded = nctool_cli::config::load().map_err(|e| CommandError {
        kind: "config".into(),
        message: e.to_string(),
        status: 500,
    })?;
    Ok(Ctx::for_embedded(
        loaded.merged.template_dir.clone(),
        loaded.merged.default_machine.clone(),
        loaded,
    ))
}

/// 将结构化 JSON 请求体送进 HTTP/Tauri 共用的纯函数路由。
pub(crate) fn run_route_json(
    ctx: &Ctx,
    method: &str,
    path: &str,
    value: &serde_json::Value,
) -> Result<serde_json::Value, CommandError> {
    let body = serde_json::to_vec(value).map_err(|e| CommandError {
        kind: "internal".into(),
        message: format!("请求序列化失败: {e}"),
        status: 500,
    })?;
    run_route(ctx, method, path, "", &body)
}

/// 路径段 / query 值百分号编码（RFC 3986 unreserved 之外全部编码）。
///
/// **必须把 `/` 编成 `%2F`**：`route` 对 `GET /api/templates/{name}` 先
/// `strip_prefix("/api/templates/")` 再 `percent_decode`，若模板名里的 `/`
/// 不被编码，`turning/undercut.j2` 会被当作多段路径处理。非 ASCII（中文分类名）
/// 按 UTF-8 字节编码，`route` 侧 `percent_decode` 以 UTF-8 lossy 还原。
pub(crate) fn percent_encode_segment(s: &str) -> String {
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
    use super::*;

    #[test]
    fn percent_encode_keeps_unreserved_and_escapes_slash() {
        assert_eq!(percent_encode_segment("drill_cycle"), "drill_cycle");
        assert_eq!(
            percent_encode_segment("turning/undercut.j2"),
            "turning%2Fundercut.j2"
        );
        // 中文按 UTF-8 字节编码
        assert_eq!(percent_encode_segment("通用"), "%E9%80%9A%E7%94%A8");
    }

    #[test]
    fn percent_encode_escapes_space() {
        assert_eq!(percent_encode_segment("a b"), "a%20b");
    }
}
