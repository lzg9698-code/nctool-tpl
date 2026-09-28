//! NC 文件保存命令：`save_nc_file`。
//!
//! 安全模型：webview 里的 `path` 由前端 `@tauri-apps/plugin-dialog` 的 `save()`
//! 产生，但**后端把它当不可信输入**——webview 一旦被注入，攻击者可以传任意
//! 字符串。本模块在写盘前做「形状校验」，把可写面收敛到：
//!
//! 1. 绝对路径；
//! 2. 不含控制字符（换行 / NUL / ANSI 序列）；
//! 3. 扩展名在 NC 程序白名单内（与前端 `save()` 的 filters 逐字对齐）——
//!    因此**不可能**覆盖 `~/.bashrc`、`nctool.toml`、`.cargo/config.toml`
//!    这类无扩展名 / 非 NC 扩展名的启动面文件；
//! 4. 文件名不含 `:`（拒绝 NTFS ADS：`out.nc:stream`）；
//! 5. 父目录已存在且是目录。
//!
//! 保存动作经 `spawn_blocking` 下沉到工作线程（与 `render.rs` 一致），
//! 不阻塞 Tauri 异步运行时。

use crate::commands::shared::{spawn_failed, CommandError};

/// 允许写入的扩展名（与 `RenderPage.tsx` 的 `save()` filters 逐字对齐）。
const ALLOWED_EXTS: [&str; 5] = ["nc", "mpf", "spf", "tap", "txt"];

/// 校验保存路径，返回规范化后的 `PathBuf`。
///
/// 拒绝时 `kind = "bad_request"`、`status = 400`（与 `route()` 的输入类
/// 错误口径一致，前端可按 kind 分支）。
fn validate_save_path(path: &str) -> Result<std::path::PathBuf, CommandError> {
    let bad = |m: &str| CommandError {
        kind: "bad_request".into(),
        message: m.into(),
        status: 400,
    };

    if path.is_empty() {
        return Err(bad("保存路径为空"));
    }
    if path.chars().any(|c| c.is_control()) {
        return Err(bad("保存路径含控制字符"));
    }
    let p = std::path::Path::new(path);
    if !p.is_absolute() {
        return Err(bad("保存路径必须是绝对路径"));
    }
    let name = p
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| bad("保存路径缺少合法文件名"))?;
    if name == "." || name == ".." {
        return Err(bad("保存路径文件名非法"));
    }
    if name.contains(':') {
        return Err(bad("保存路径文件名含 NTFS 流分隔符"));
    }
    let ext = p
        .extension()
        .and_then(|s| s.to_str())
        .ok_or_else(|| bad("保存文件缺少扩展名"))?;
    if !ALLOWED_EXTS.iter().any(|e| ext.eq_ignore_ascii_case(e)) {
        return Err(bad(&format!(
            "仅允许保存为 NC 程序扩展名：{ALLOWED_EXTS:?}，收到 .{ext}"
        )));
    }
    let parent = p
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .ok_or_else(|| bad("保存路径缺少父目录"))?;
    match std::fs::metadata(parent) {
        Ok(m) if m.is_dir() => {}
        Ok(_) => {
            return Err(bad(&format!(
                "父目录不存在或不是目录：{}",
                parent.display()
            )))
        }
        Err(e) => return Err(bad(&format!("父目录不可访问：{}（{e}）", parent.display()))),
    }
    Ok(p.to_path_buf())
}

/// 校验通过后的实际写入（在线程池中执行）。
fn write_nc(path: &std::path::Path, content: &str) -> Result<(), CommandError> {
    std::fs::write(path, content.as_bytes()).map_err(|e| CommandError {
        kind: "io".into(),
        message: format!("写入失败 {}：{e}", path.display()),
        status: 500,
    })
}

/// 把渲染结果写入**用户选择**的路径。
///
/// 见模块文档的五条形状约束；写入经 `spawn_blocking` 不阻塞运行时。
#[tauri::command]
pub async fn save_nc_file(path: String, content: String) -> Result<(), CommandError> {
    let path = validate_save_path(&path)?;
    tauri::async_runtime::spawn_blocking(move || write_nc(&path, &content))
        .await
        .map_err(spawn_failed)?
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 系统临时目录（Windows 下形如 `C:\Users\...\Temp`，绝对路径）。
    fn tmp() -> std::path::PathBuf {
        std::env::temp_dir()
    }

    fn tmp_str() -> String {
        tmp().to_string_lossy().into_owned()
    }

    #[test]
    fn accepts_absolute_nc_path_in_existing_dir() {
        let p = format!("{}/ok_out.nc", tmp_str());
        assert!(
            validate_save_path(&p).is_ok(),
            "白名单扩展名 + 已存在父目录应放行"
        );
    }

    #[test]
    fn accepts_all_allowlisted_extensions() {
        for ext in ALLOWED_EXTS {
            let p = format!("{}/out.{ext}", tmp_str());
            assert!(validate_save_path(&p).is_ok(), ".{ext} 应在白名单内");
        }
        // 大小写不敏感
        assert!(validate_save_path(&format!("{}/OUT.NC", tmp_str())).is_ok());
    }

    #[test]
    fn rejects_relative_path() {
        let e = validate_save_path("relative/out.nc").expect_err("相对路径必须拒绝");
        assert_eq!(e.kind, "bad_request");
        assert_eq!(e.status, 400);
    }

    #[test]
    fn rejects_startup_surface_files() {
        // 无扩展名：.bashrc / nctool.toml 之类启动面文件
        assert!(validate_save_path("/home/user/.bashrc").is_err());
        assert!(validate_save_path(&format!("{}/nctool.toml", tmp_str())).is_err());
        // 非 NC 扩展名
        assert!(validate_save_path(&format!("{}/evil.exe", tmp_str())).is_err());
        assert!(validate_save_path(&format!("{}/a.sh", tmp_str())).is_err());
        assert!(validate_save_path(&format!("{}/config.toml", tmp_str())).is_err());
    }

    #[test]
    fn rejects_control_chars_including_newline_and_nul() {
        assert!(validate_save_path(&format!("{}/out\u{0}.nc", tmp_str())).is_err());
        assert!(validate_save_path(&format!("{}/ou\u{7}t.nc", tmp_str())).is_err());
        // 通过 CRLF 拼接的注入尝试
        assert!(validate_save_path(&format!("{}/out\r\n.nc", tmp_str())).is_err());
    }

    #[test]
    fn rejects_ntfs_ads_separator() {
        assert!(validate_save_path(&format!("{}/out.nc:evil", tmp_str())).is_err());
    }

    #[test]
    fn rejects_missing_parent_dir() {
        assert!(validate_save_path("C:\\nctool\\no\\such\\dir\\out.nc").is_err());
    }

    #[test]
    fn rejects_missing_extension() {
        assert!(validate_save_path(&format!("{}/out", tmp_str())).is_err());
    }

    #[test]
    fn write_then_read_back() {
        let dir = tmp();
        let p = dir.join(format!("nctool_nc_save_test_{}.nc", std::process::id()));
        let path = p.to_string_lossy().into_owned();
        let path = validate_save_path(&path).expect("校验通过");
        write_nc(&path, "G90 G0 X0 Y0;\n").expect("写入成功");
        let back = std::fs::read_to_string(&path).expect("读回");
        assert_eq!(back, "G90 G0 X0 Y0;\n");
        let _ = std::fs::remove_file(&path);
    }
}
