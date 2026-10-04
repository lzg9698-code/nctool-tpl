//! 原子写原语：同目录临时文件 + `rename`。
//!
//! 为什么必须"同目录"：`rename` 只有在**同一文件系统**内才是原子的。把临时文件
//! 放到系统临时目录再 `rename`，跨分区时会退化成"复制 + 删除"，既非原子又慢。
//!
//! 为什么必须"临时文件"：直接 `open + truncate + write` 覆盖目标，写到一半时
//! 崩溃会留下**半成品**（比旧内容更糟）。先写临时文件再原子替换，目标任何时刻
//! 要么是旧内容、要么是新内容。
//!
//! # 保证的强度（P1-5 起写清）
//!
//! **原子可见 + 尽力持久**，不是"保证持久"：
//! - 数据本身 `flush` + `sync_all` 后才 `rename`（见 [`write_tmp`]），
//! - `rename` 之后再对**父目录**做一次 fsync（见 [`sync_parent_dir`]），
//!   让目录项本身落盘 —— POSIX 语义下目录项只进页缓存，掉电可能整个 rename 丢失，
//!   表现为"旧文件还在、新文件不存在"，对机床参数是危险方向。
//!
//! 父目录 fsync 是**尽力而为**：拿不到目录句柄（只读卷 / 权限受限）时静默跳过，
//! 因为数据已落盘、为此让整次写入失败是反向交易。

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
///
/// 写盘经 [`with_retry`] 包装：对**瞬时权限拒绝**做有限重试（成因与边界见该函数文档）。
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), WriteError> {
    // 退避基数 10ms：线性退避 10/20/30/40ms，累计 ≤100ms，足以跨越一次扫描窗口。
    with_retry(std::time::Duration::from_millis(10), || {
        write_atomic_once(path, bytes)
    })
}

/// 删除文件目录项，并在支持的平台上同步父目录。
///
/// 调用方必须先在 `WriteKernel` 中持有对应路径锁并校验指纹。
pub(crate) fn remove_file(path: &Path) -> Result<(), WriteError> {
    with_retry(std::time::Duration::from_millis(10), || {
        std::fs::remove_file(path).map_err(|e| map_io(e, path))
    })?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    sync_parent_dir(parent);
    Ok(())
}

/// 有限重试：仅对「权限被拒」重试，其余错误立即返回（重试无意义）。
///
/// # 为什么要重试「权限被拒」
///
/// Windows 上杀毒 / Defender 的**按访问扫描**会在文件刚创建或被替换的瞬间持有句柄，
/// 令 `File::create` / `rename` 返回 `ERROR_ACCESS_DENIED` / `ERROR_SHARING_VIOLATION`
/// （Rust 归一为 `PermissionDenied` → [`WriteError::ReadOnly`]）。这是**瞬时**失败，
/// 重试即可自愈；而**真正的**只读目标会持续失败，重试耗尽后仍如实返回错误 ——
/// **重试不掩盖问题**。高并发落盘（如 E2E 里数十个 `nctool.exe` 进程同时写盘）会显著
/// 抬高命中概率，故必须有此重试。
///
/// `base_delay` 与 `attempt` 都**可注入**：生产传 10ms + 真实写盘闭包；单测传
/// [`std::time::Duration::ZERO`] + 计数闭包，从而**确定性地**覆盖重试的各个分支，
/// 既不依赖真实等待，也不依赖 Defender（见本模块单测）。
fn with_retry<F>(base_delay: std::time::Duration, mut attempt: F) -> Result<(), WriteError>
where
    F: FnMut() -> Result<(), WriteError>,
{
    // 总尝试次数（首次 + 重试）。5 次 × 线性退避 ≈ 100ms 上限，足以跨越一次扫描窗口。
    const MAX_ATTEMPTS: u32 = 5;

    let mut n: u32 = 0;
    loop {
        match attempt() {
            Ok(()) => return Ok(()),
            Err(err) => {
                n += 1;
                // 只对「权限被拒」重试：磁盘满 / 路径不存在等其它 IO 错误重试无意义，
                // 直接返回以免无谓延迟。
                if !matches!(err, WriteError::ReadOnly { .. }) || n >= MAX_ATTEMPTS {
                    return Err(err);
                }
                // 线性退避（n=1..4 → base×1..base×4）：给扫描器让出时间窗。
                std::thread::sleep(base_delay * n);
            }
        }
    }
}

/// 单次原子写尝试（由 [`write_atomic`] 包装以支持瞬时失败重试）。
///
/// 每次尝试都用**新**的临时文件名（`temp_name` 含纳秒），故重试不会踩上一次的残留。
fn write_atomic_once(path: &Path, bytes: &[u8]) -> Result<(), WriteError> {
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
    // 目录项本身也要落盘（P1-5）：否则掉电后 rename 可能整体丢失 ——
    // 用户拿到"修改前"的旧参数而不自知。
    sync_parent_dir(parent);
    Ok(())
}

/// 尽力让**父目录**的目录项落盘（`rename` 的持久性那一半）。
///
/// 失败**不阻断**、也不上报：数据已 `sync_all` 过，这里只是把"目录项更新"
/// 也推进磁盘；只读卷 / 权限受限目录上拿不到句柄是正常情形。
///
/// # 平台差异（2026-09-27 本机实测，勿凭直觉写）
///
/// - **Unix**：`File::open(dir)` + `sync_all()` 即 `fsync(dirfd)`，正常可用。
/// - **Windows**：`File::open(dir)` **直接失败** `PermissionDenied`（os error 5）
///   —— 打不开目录。必须加 `FILE_FLAG_BACKUP_SEMANTICS` 才能拿到目录句柄。
///   且 `FlushFileBuffers` 要求句柄带**写权限**：只读句柄即使拿到了句柄，
///   `sync_all()` 仍报 os error 5。故 Windows 上必须 `write(true)`。
///
/// 也就是说"Windows 上同样有效"这句要成立，前提是**按下面这样打开**；
/// 照抄 Unix 的 `File::open` 在 Windows 上是彻底的空操作。
fn sync_parent_dir(parent: &Path) {
    if let Ok(dir) = open_dir_for_sync(parent) {
        let _ = dir.sync_all();
    }
}

/// 以"可 fsync"的方式打开目录（平台差异见 [`sync_parent_dir`]）。
#[cfg(windows)]
fn open_dir_for_sync(dir: &Path) -> std::io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    /// 允许对目录取句柄（缺此标志 `open` 报 `PermissionDenied`）。
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    std::fs::OpenOptions::new()
        .read(true)
        // `FlushFileBuffers` 要求 GENERIC_WRITE：只读句柄报 os error 5
        .write(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(dir)
}

/// 以"可 fsync"的方式打开目录（平台差异见 [`sync_parent_dir`]）。
#[cfg(not(windows))]
fn open_dir_for_sync(dir: &Path) -> std::io::Result<File> {
    File::open(dir)
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

    // --- with_retry：确定性覆盖重试各分支（Duration::ZERO + 计数闭包，不碰文件系统） ---

    /// 瞬时「权限被拒」应被重试**自愈**（模拟 Defender 扫描窗口结束后的成功）。
    #[test]
    fn retry_recovers_from_transient_readonly() {
        let mut calls = 0u32;
        let res = with_retry(std::time::Duration::ZERO, || {
            calls += 1;
            if calls < 3 {
                Err(WriteError::ReadOnly {
                    path: std::path::PathBuf::from("transient.toml"),
                })
            } else {
                Ok(())
            }
        });
        assert!(res.is_ok(), "瞬时只读应被重试自愈：{res:?}");
        assert_eq!(calls, 3, "应恰好尝试 3 次（2 次失败 + 1 次成功）");
    }

    /// 真实只读应**如实失败**（不被重试掩盖），且尝试次数恰为上限 5。
    #[test]
    fn retry_exhausts_and_surfaces_last_error() {
        let mut calls = 0u32;
        let res = with_retry(std::time::Duration::ZERO, || {
            calls += 1;
            Err(WriteError::ReadOnly {
                path: std::path::PathBuf::from("really_readonly.toml"),
            })
        });
        assert!(
            matches!(res, Err(WriteError::ReadOnly { .. })),
            "真实只读应返回 ReadOnly 错误：{res:?}"
        );
        assert_eq!(calls, 5, "重试耗尽应恰好尝试 5 次（上限）");
    }

    /// 非权限类错误（如 IO）**不得**被重试，应立即返回（不给磁盘满等错误加无谓延迟）。
    #[test]
    fn non_readonly_error_is_not_retried() {
        let mut calls = 0u32;
        let res = with_retry(std::time::Duration::ZERO, || {
            calls += 1;
            Err(WriteError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "boom",
            )))
        });
        assert!(
            matches!(res, Err(WriteError::Io(_))),
            "非权限类错误应原样返回：{res:?}"
        );
        assert_eq!(calls, 1, "非权限类错误不得重试");
    }

    /// **P1-5 的非空转守卫**：父目录 fsync 必须**真的**能拿到可 fsync 的目录句柄。
    ///
    /// 这条不能省：`sync_parent_dir` 吞掉全部错误（这是有意的，只读卷上拿不到
    /// 句柄属正常），于是一旦平台实现写错，它会**静默退化成空操作** ——
    /// 从外部完全看不出来，测试全绿而掉电丢 rename 的风险照旧。
    /// 本机实测：Windows 上照抄 Unix 的 `File::open(dir)` 正是这种空操作
    /// （`PermissionDenied`），必须 `FILE_FLAG_BACKUP_SEMANTICS` + 写权限。
    #[test]
    fn parent_dir_can_actually_be_opened_for_fsync() {
        let dir = std::env::temp_dir().join(format!(
            "nctool_dirfsync_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let handle = open_dir_for_sync(&dir).unwrap_or_else(|e| {
            panic!("必须能打开目录句柄（{}）：{e}", dir.display());
        });
        handle.sync_all().unwrap_or_else(|e| {
            panic!(
                "目录句柄必须支持 fsync（{}/{}）：{e}",
                std::env::consts::OS,
                dir.display()
            );
        });

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 端到端：`write_atomic` 走完整条路径（写临时 → fsync → rename → 父目录 fsync）
    /// 之后内容与预期一致，且不残留临时文件。
    #[test]
    fn write_atomic_with_parent_fsync_is_transparent() {
        let dir = std::env::temp_dir().join(format!("nctool_atomic_fsync_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("x.yaml");

        write_atomic(&target, b"first").unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"first");
        write_atomic(&target, b"second").unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"second");

        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.contains(TMP_INFIX))
            .collect();
        assert!(leftovers.is_empty(), "不应残留临时文件：{leftovers:?}");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
