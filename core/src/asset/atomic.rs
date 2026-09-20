//! 原子写原语：同目录临时文件 + `rename`。
//!
//! 为什么必须"同目录"：`rename` 只有在**同一文件系统**内才是原子的。把临时文件
//! 放到系统临时目录再 `rename`，跨分区时会退化成"复制 + 删除"，既非原子又慢。
//!
//! 为什么必须"临时文件"：直接 `open + truncate + write` 覆盖目标，写到一半时
//! 崩溃会留下**半成品**（比旧内容更糟）。先写临时文件再原子替换，目标任何时刻
//! 要么是旧内容、要么是新内容。

use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use super::{map_io, WriteError};

/// 临时文件名中的标记（含此标记者不得被任何资产发现逻辑收录）。
pub(crate) const TMP_INFIX: &str = ".nctool-tmp-";
/// 临时文件名后缀。
pub(crate) const TMP_SUFFIX: &str = ".tmp";

/// 生成临时文件名：`<目标文件名>.nctool-tmp-<pid>-<nanos>.tmp`。
///
/// `pid` 区分进程、`nanos` 区分同一进程内的多次写，避免并发写互相踩踏。
/// 后缀为 `.tmp`（**不是** `.j2`），因此模板注册表的 `*.j2` 扫描不会收录它。
pub(crate) fn temp_name(target: &Path) -> String {
    let base = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "asset".to_string());
    let pid = std::process::id();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{base}{TMP_INFIX}{pid}-{nanos}{TMP_SUFFIX}")
}

/// 原子写：写临时文件 → `flush`/`fsync` → `rename` 覆盖目标。
///
/// 任一步失败都清理临时文件，不留残留；目标父目录须已存在。
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), WriteError> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let tmp = parent.join(temp_name(path));

    if let Err(err) = write_tmp(&tmp, path, bytes) {
        let _ = std::fs::remove_file(&tmp);
        return Err(err);
    }
    if let Err(err) = std::fs::rename(&tmp, path) {
        // 目标只读 / 被占用时 rename 失败：删掉临时文件，保持目录干净。
        let _ = std::fs::remove_file(&tmp);
        return Err(map_io(err, path));
    }
    Ok(())
}

/// 写临时文件并落盘（`flush` + `fsync`），确保内容在 `rename` 前已持久化。
///
/// `target` 仅用于错误归类（把 `PermissionDenied` 报成对**目标**的只读，
/// 而不是临时文件），便于用户定位。
fn write_tmp(tmp: &Path, target: &Path, bytes: &[u8]) -> Result<(), WriteError> {
    let mut file = File::create(tmp).map_err(|e| map_io(e, target))?;
    file.write_all(bytes).map_err(|e| map_io(e, target))?;
    file.flush().map_err(|e| map_io(e, target))?;
    file.sync_all().map_err(|e| map_io(e, target))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temp_name_carries_marker_and_tmp_suffix() {
        let name = temp_name(Path::new("turning/demo.j2"));
        assert!(name.starts_with("demo.j2"), "应基于目标文件名: {name}");
        assert!(name.contains(TMP_INFIX), "应含标记: {name}");
        assert!(name.ends_with(TMP_SUFFIX), "应以 .tmp 结尾: {name}");
        assert!(
            !name.ends_with(".j2"),
            "不得以 .j2 结尾（否则会被模板注册表发现）: {name}"
        );
        assert!(
            name.contains(&std::process::id().to_string()),
            "应含 pid 以区分进程: {name}"
        );
    }

    #[test]
    fn temp_name_without_file_name_uses_fallback() {
        // 根路径没有 file_name，回退到 "asset"，但仍带标记与后缀。
        let name = temp_name(Path::new("/"));
        assert!(name.contains(TMP_INFIX), "{name}");
        assert!(name.ends_with(TMP_SUFFIX), "{name}");
    }
}
