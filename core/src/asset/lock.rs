//! 跨进程写互斥锁（OS advisory lock）：`write_guarded` 流程
//! 「**取锁** → 读快照 → 比对 → 写 → 释放」的第一环。
//!
//! # 分层口径（写死，勿混用）
//!
//! - **本锁**解决「写-写交叠」（跨进程）：两个进程同时写同一文件，后 `rename`
//!   者会静默覆盖先写者 —— 用 OS 级排他锁把临界区（读快照 → 比对 → 写）串行化。
//! - **乐观锁**（[`super::FileFingerprint`] + `expect`）解决「读-改-写」的 ABA，
//!   含跨会话场景（用户昨天取的指纹今天才提交）。
//! - **两层都保留：锁不替代乐观锁。** 锁只保证临界区互斥；拿着过期 `expect`
//!   进临界区，仍会在比对处拿到 [`super::WriteError::Conflict`]。
//!
//! # 为什么是 OS advisory lock，而不是"锁文件存在即锁定"
//!
//! 进程被杀时 OS **自动释放** advisory lock —— 不会留下陈旧锁，无需 TTL /
//! stale 检测 / 砸锁脚本。锁文件因此**只 unlock、不 unlink**（Unix 上删除被
//! flock 的文件会让别人手里已打开的 fd 失效，造成互斥破洞）；磁盘上会留下一个
//! 空的 `<stem>.nctool.lock`，这是有意的代价。
//!
//! # 等待时长必须可注入：CLI 短等，HTTP 等待 0
//!
//! [`DirLock::acquire`] 带有界等待（CLI 传 [`super::CLI_LOCK_WAIT`] = 2s）；
//! [`DirLock::try_lock`] 等待 0，拿不到立即 [`super::WriteError::LockBusy`]。
//! `nctool ui` 的 `serve_requests` 是单线程顺序循环、tiny_http 无读超时：
//! 请求若在 `route()` 内阻塞等锁，整个 UI 会停顿 —— **服务侧一律 try-lock**。
//!
//! # HTTP 文案脱敏约束
//!
//! [`super::WriteError::LockBusy`] 的 `Display` 带绝对路径（CLI/stderr 通道
//! 可用）；**HTTP 响应体不得照抄** —— 服务侧消息固定为
//! "预设文件正被另一个 nctool 进程写入，请稍后重试"（409 + `write_conflict`），
//! 路径只进 stderr。绝对路径只允许出现在 CLI / stderr 通道（P1-17 口径）。
//!
//! 实现：std 的 `File::try_lock` / `File::unlock`（Rust 1.89 稳定 —— MSRV
//! 因此为 1.89），零新增依赖；拿不到锁时 10ms 轮询，临界区是毫秒级，成本可忽略。
//!
//! # 打开锁文件必须走 `create_new` 两步（Windows 硬要求）
//!
//! 见 [`open_lock_file`]：并发**首次创建**同一文件时 Windows 会给落败者
//! `ERROR_ACCESS_DENIED(5)`，被 [`map_io`] 误判成"只读"。别把两步改回
//! 一步 `create(true).open()`。

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::{map_io, WriteError};

/// 持有即持有锁的 RAII 守卫：`acquire` / `try_lock` 成功返回的同时已持有该
/// 目标的 OS 级排他锁；`Drop` 解锁（**不**删除锁文件）。
pub(crate) struct DirLock {
    file: File,
}

impl DirLock {
    /// 锁定 `target` 对应的锁文件，最多等 `wait`；超时返回
    /// [`WriteError::LockBusy`]（`wait = Duration::ZERO` 即非阻塞 try-lock）。
    ///
    /// 锁文件 = `target.with_extension("nctool.lock")`，与 `target` 同目录
    /// （如 `presets.yaml` → `presets.nctool.lock`）。
    pub(crate) fn acquire(target: &Path, wait: Duration) -> Result<Self, WriteError> {
        // 等待 0 就是 try-lock：语义上等价于 [`Self::try_lock`]，直接走同一入口。
        if wait.is_zero() {
            return Self::try_lock(target);
        }
        let deadline = Instant::now() + wait;
        loop {
            match Self::try_open(target)? {
                Some(lock) => return Ok(lock),
                None => {
                    if Instant::now() >= deadline {
                        return Err(WriteError::LockBusy {
                            path: target.to_path_buf(),
                        });
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
        }
    }

    /// 非阻塞取锁（等待 0）：拿不到立即 [`WriteError::LockBusy`]。服务侧专用
    /// （HTTP/UI 通道不得在 `route()` 内等锁）。
    pub(crate) fn try_lock(target: &Path) -> Result<Self, WriteError> {
        Self::try_open(target)?.ok_or_else(|| WriteError::LockBusy {
            path: target.to_path_buf(),
        })
    }

    /// 打开锁文件并试取一次排他锁：拿到返回守卫，被占返回 `None`，
    /// IO 失败返回 [`WriteError::Io`] / [`WriteError::ReadOnly`]。
    fn try_open(target: &Path) -> Result<Option<Self>, WriteError> {
        let lock_path = lock_path_for(target);
        let file = open_lock_file(&lock_path)?;
        match file.try_lock() {
            Ok(()) => Ok(Some(DirLock { file })),
            Err(std::fs::TryLockError::WouldBlock) => Ok(None),
            Err(std::fs::TryLockError::Error(e)) => Err(map_io(e, &lock_path)),
        }
    }
}

/// 打开（必要时创建）锁文件，返回**尚未加锁**的句柄。
///
/// # 为什么不能一步 `create(true).write(true).open()`
///
/// Windows 上「两个线程/进程并发**首次创建**同一个尚不存在的文件」时，落败者的
/// `CreateFile(OPEN_ALWAYS, GENERIC_WRITE)` **既不返回** `ERROR_SHARING_VIOLATION(32)`
/// **也不返回** `ERROR_FILE_EXISTS(80)`，而是 `ERROR_ACCESS_DENIED(5)`。
/// 本机实测（2026-09-27，`examples/probe_create_race.rs`）：文件不存在时 200 轮
/// **200/200** 出现恰好一个 `os error 5`；同一文件已存在时 400/400 全部成功。
///
/// 而 [`map_io`] 把 `PermissionDenied` 归类为 [`WriteError::ReadOnly`]，于是
/// **并发写的落败者被报成"文件只读"** —— 归因错误，还盖掉了它本该得到的
/// [`WriteError::Conflict`]（`concurrent_writes_to_same_path_never_corrupt`
/// 在 Windows 上因此恒红，属 P0-1 修复自身引入的缺陷）。
///
/// 修法是**消掉"并发创建"这个动作本身**，而不是给它加时序假设（重试/睡眠）：
/// 先 `create_new`（`CREATE_NEW` / `O_CREAT|O_EXCL`），落败者拿到
/// `AlreadyExists` 后改走普通 open 打开既有文件 —— 该路径实测 400/400 无失败。
/// 真·只读锁文件仍会（在回退 open 处）拿到 `PermissionDenied` → `ReadOnly`，
/// 语义不变；锁文件从不 unlink，故"回退 open 时文件已消失"的窗口不存在。
fn open_lock_file(lock_path: &Path) -> Result<File, WriteError> {
    match OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(lock_path)
    {
        Ok(file) => Ok(file),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => OpenOptions::new()
            .create(false)
            .truncate(false)
            .write(true)
            .open(lock_path)
            .map_err(|e| map_io(e, lock_path)),
        Err(e) => Err(map_io(e, lock_path)),
    }
}

/// 只 unlock、**不** unlink（原因见模块文档）。
impl Drop for DirLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

/// `target` 的锁文件路径：`presets.yaml` → `presets.nctool.lock`。
///
/// 同目录不同 stem 各自一把锁（`a.yaml` 与 `b.json` 互不相干）；
/// `foo.yaml` 与 `foo.json` 会共享 `foo.nctool.lock` —— 过度锁定是安全方向。
pub(crate) fn lock_path_for(target: &Path) -> PathBuf {
    target.with_extension("nctool.lock")
}
