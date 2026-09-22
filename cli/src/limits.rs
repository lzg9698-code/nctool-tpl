//! 本地文件读取的大小上限（第四轮审查 P1-10）。
//!
//! HTTP 侧对请求体有 1 MiB 上限（`server.rs::MAX_BODY_BYTES`），但 CLI 侧的
//! 文件读取此前**全部无上限**：`read_to_string` 会把 `--template-dir` 指向的
//! 网络盘 / 超大文件整体读进内存。本地工具定位下风险可控，但代价不对称
//! —— 加一道上限的成本极小，误读 GB 级文件却可能耗尽内存。
//!
//! 三处入口统一走 [`read_text_limited`]：
//! - `config.rs`：`nctool.toml`（配置文件）
//! - `args.rs`：`--params-file`（参数文件）
//! - `context.rs`：模板目录下的 `*.j2`（逐个模板源码）
//!
//! 上限选取：模板 / 参数 / 配置都远小于此。真实零件模板在几十 KB 量级，
//! 1 MiB 留了两个数量级余量，既能挡住误指大文件，又不会误伤正常用法。

use std::path::Path;

use crate::output::CliError;

/// 单个本地文本文件的上限：1 MiB（与 HTTP 请求体上限一致）。
pub const MAX_LOCAL_TEXT_BYTES: u64 = 1024 * 1024;

/// 带大小上限的文本读取。
///
/// `kind` 是 [`CliError`] 的错误类别（如 `"io"`）；`what` 是错误文案里的
/// 名词（如 `"配置文件"`、`"参数文件"`、`"模板"`），用于生成可读的错误消息。
///
/// 超限时返回 `io` 类别的错误，说明文件路径、实际大小与上限 —— 不静默截断，
/// 截断会产出语法不完整的内容而更难排查。
pub fn read_text_limited(path: &Path, kind: &'static str, what: &str) -> Result<String, CliError> {
    // 先看 metadata：能在读取前就拒绝超大文件，避免把内容读进内存再判断。
    // 目录 / 特殊文件没有可靠的 len()，交给 read_to_string 报错（保持原行为）。
    if let Ok(meta) = std::fs::metadata(path) {
        if meta.is_file() && meta.len() > MAX_LOCAL_TEXT_BYTES {
            return Err(CliError::new(
                kind,
                format!(
                    "{}过大 {}: {} 字节，超过 {} 字节上限\
                     （仅处理本地小文件；请确认路径未指向大文件或网络盘）",
                    what,
                    path.display(),
                    meta.len(),
                    MAX_LOCAL_TEXT_BYTES
                ),
            ));
        }
    }
    std::fs::read_to_string(path)
        .map_err(|e| CliError::new(kind, format!("读取{}失败 {}: {e}", what, path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_file(name: &str, contents: &[u8]) -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("nctool_limits_{}_{name}", std::process::id()));
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(contents).unwrap();
        path
    }

    #[test]
    fn reads_small_file() {
        let path = temp_file("small.txt", b"hello");
        let text = read_text_limited(&path, "io", "模板").unwrap();
        assert_eq!(text, "hello");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn rejects_file_over_limit() {
        // 造一个超过上限的文件：上限 + 1 字节。用稀疏内容也不必真写 1 MiB 的数据量，
        // 但 set_len 造空洞对 read_to_string 仍然有效（读出 NUL 字节）。
        let mut path = std::env::temp_dir();
        path.push(format!("nctool_limits_{}_big.txt", std::process::id()));
        let f = std::fs::File::create(&path).unwrap();
        f.set_len(MAX_LOCAL_TEXT_BYTES + 1).unwrap();
        drop(f);

        let err = read_text_limited(&path, "io", "参数文件").unwrap_err();
        assert_eq!(err.kind, "io");
        assert!(
            err.message.contains("过大") && err.message.contains("上限"),
            "错误应说明超限，实际: {}",
            err.message
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn exactly_at_limit_is_accepted() {
        let mut path = std::env::temp_dir();
        path.push(format!("nctool_limits_{}_exact.txt", std::process::id()));
        let f = std::fs::File::create(&path).unwrap();
        f.set_len(MAX_LOCAL_TEXT_BYTES).unwrap();
        drop(f);

        // 恰好等于上限应通过（边界包含）。
        let text = read_text_limited(&path, "io", "模板").unwrap();
        assert_eq!(text.len() as u64, MAX_LOCAL_TEXT_BYTES);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn missing_file_reports_io_error() {
        let path = std::env::temp_dir().join("nctool_limits_does_not_exist.txt");
        let err = read_text_limited(&path, "io", "配置文件").unwrap_err();
        assert_eq!(err.kind, "io");
        assert!(
            err.message.contains("读取配置文件失败"),
            "实际: {}",
            err.message
        );
    }
}
