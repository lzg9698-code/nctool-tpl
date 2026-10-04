//! Shared atomic writes, advisory locks, fingerprints and safe asset paths.
mod atomic;
mod guard;
pub mod json_num;
mod lock;
mod path;

use std::path::{Path, PathBuf};
use std::time::Duration;

pub use guard::FileFingerprint;
pub use path::{validate_asset_name, SafePath};

/// CLI 通道取锁的有界等待（`nctool` 子命令：可短等，2 秒）。
///
/// 服务侧（`nctool ui` / GUI 内嵌 `route`）**不**用本值，传 `Duration::ZERO`
/// 走 try-lock —— `serve_requests` 是单线程顺序循环，阻塞等锁会钉死整个 UI。
pub const CLI_LOCK_WAIT: Duration = Duration::from_secs(2);

/// 原子写 + 跨进程互斥锁 + 乐观锁 + 指纹读取的统一写内核。
///
/// 无状态：所有方法均为关联函数，调用方无需持有实例。整个 crate 只有这一处
/// 真正调用 `std::fs::rename`，以保证"单一写入口"。
pub struct WriteKernel;

impl WriteKernel {
    /// 原子写：把 `bytes` 写入 `path`，不存在则创建、存在则覆盖。
    ///
    /// 实现见 `atomic` 子模块：同目录临时文件 → `flush`/`fsync` → `rename`。
    /// **不做乐观锁比对**（调用方已确认可覆盖时使用）。
    /// **不取互斥锁** —— 需要"比对后再写"的调用方走 [`Self::write_guarded`]。
    pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), WriteError> {
        atomic::write_atomic(path, bytes)
    }

    /// 乐观锁写（CLI 默认等待 [`CLI_LOCK_WAIT`]）：先取锁，再比对 `expect`
    /// 与实际指纹，一致才落盘。服务侧请用 [`Self::write_guarded_with_wait`]
    /// 传 `Duration::ZERO`（try-lock，拿不到即 [`WriteError::LockBusy`]）。
    ///
    /// - `expect = Some(fp)`：要求写前文件指纹恰为 `fp`；
    /// - `expect = None`：要求写前**文件不存在**（"新建"语义）。
    ///
    /// 不一致返回 [`WriteError::Conflict`]，**不写入**。一致时：目标不存在 →
    /// [`WriteAction::Created`]；内容与 `bytes` 相同 → [`WriteAction::Unchanged`]
    /// （跳过落盘）；否则 [`WriteAction::Updated`]。
    ///
    /// 流程（P0-1）：**取锁 → 读快照 → 比对 → 写 → 释放**。锁把并发写者串行
    /// 引入临界区：后到者拿到的快照已是前者写完的状态，`expect` 过期即
    /// [`WriteError::Conflict`] —— 修复前的 check-then-write 会让两者都通过
    /// 比对、后 `rename` 者胜、先写者的一整次编辑被静默丢弃。
    pub fn write_guarded(
        path: &Path,
        bytes: &[u8],
        expect: Option<FileFingerprint>,
    ) -> Result<WriteOutcome, WriteError> {
        Self::write_guarded_with_wait(path, bytes, expect, CLI_LOCK_WAIT)
    }

    /// 与 [`Self::write_guarded`] 相同，但锁等待时长由调用方注入：
    /// CLI 传 [`CLI_LOCK_WAIT`]（或直接用 `write_guarded`）；HTTP/UI 服务侧
    /// 传 `Duration::ZERO` —— `serve_requests` 单线程顺序循环，阻塞等锁会
    /// 钉死整个 UI（tiny_http 亦无读超时），故服务侧绝不等待。
    pub fn write_guarded_with_wait(
        path: &Path,
        bytes: &[u8],
        expect: Option<FileFingerprint>,
        wait: Duration,
    ) -> Result<WriteOutcome, WriteError> {
        // 取锁：`Drop` 兜底释放，panic / 早退路径也不会留死锁。
        let _lock = lock::DirLock::acquire(path, wait)?;
        let snapshot = guard::read_snapshot(path)?;
        let actual = snapshot.as_ref().map(|(_, fp)| *fp);
        if actual != expect {
            return Err(WriteError::Conflict {
                path: path.to_path_buf(),
                expected: expect,
                actual,
            });
        }
        let action = match &snapshot {
            None => WriteAction::Created,
            Some((old, _)) if old.as_slice() == bytes => WriteAction::Unchanged,
            Some(_) => WriteAction::Updated,
        };
        if action != WriteAction::Unchanged {
            atomic::write_atomic(path, bytes)?;
        }
        let revision = Self::read_fingerprint(path)?;
        let fingerprint = revision.map(|fp| fp.as_string());
        Ok(WriteOutcome {
            path: path.to_path_buf(),
            action,
            fingerprint,
            revision,
        })
    }

    /// 带指纹保护的删除：先持有目标锁，再核对内容快照，避免重命名时删除
    /// 另一个编辑器刚写入的源文件。
    pub fn remove_guarded(
        path: &Path,
        expect: Option<FileFingerprint>,
    ) -> Result<WriteOutcome, WriteError> {
        let _lock = lock::DirLock::acquire(path, CLI_LOCK_WAIT)?;
        let snapshot = guard::read_snapshot(path)?;
        let actual = snapshot.as_ref().map(|(_, fp)| *fp);
        if actual != expect {
            return Err(WriteError::Conflict {
                path: path.to_path_buf(),
                expected: expect,
                actual,
            });
        }
        if snapshot.is_none() {
            return Err(WriteError::NotFound(format!(
                "删除目标不存在: {}",
                path.display()
            )));
        }
        atomic::remove_file(path)?;
        Ok(WriteOutcome {
            path: path.to_path_buf(),
            action: WriteAction::Deleted,
            fingerprint: None,
            revision: None,
        })
    }

    /// 读取文件指纹；文件不存在返回 `None`。
    ///
    /// 供调用方在编辑前取快照，编辑后作为 [`Self::write_guarded`] 的 `expect`。
    pub fn read_fingerprint(path: &Path) -> Result<Option<FileFingerprint>, WriteError> {
        Ok(guard::read_snapshot(path)?.map(|(_, fp)| fp))
    }
}

/// 写入动作类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum WriteAction {
    /// 目标文件此前不存在，本次新建。
    Created,
    /// 目标文件存在且内容被更新。
    Updated,
    /// 目标文件内容与新内容一致，未改动。
    Unchanged,
    /// 目标条目被移除（文件内容因此变化）。
    ///
    /// # 为什么要独立于 [`WriteAction::Updated`]
    ///
    /// `remove()` 内部走的是"改完内容再落盘"，若直接透传 `save` 的动作，
    /// 调用方拿到的就是 `updated` —— 一个**删除**被报成**更新**。
    /// CLI 与 HTTP 都把这个值直接给了消费方，于是"按 action 分支"的逻辑
    /// 永远走不到删除分支。这是静默的语义错误（值合法、不报错、只是错）。
    Deleted,
}

/// 一次写操作的结果。
#[derive(Debug, Clone, serde::Serialize)]
pub struct WriteOutcome {
    /// 写入的目标路径。
    pub path: PathBuf,
    /// 本次执行的动作。
    pub action: WriteAction,
    /// 写后文件指纹字符串（`fnv1a64:<16 hex>`）；目标不存在时为 `None`。
    pub fingerprint: Option<String>,
    /// Complete revision captured while still holding the write lock.
    pub revision: Option<FileFingerprint>,
}

/// 写内核统一错误。
///
/// `#[non_exhaustive]`：与 `TplError` / `RegistryError` 一致，为后续扩展留空间，
/// 下游 `match` 必须带 `_` 兜底分支。
#[derive(Debug)]
#[non_exhaustive]
pub enum WriteError {
    /// 乐观锁冲突：实际指纹与 `expect` 不一致（文件被外部改动或与预期不符）。
    Conflict {
        /// 目标路径。
        path: PathBuf,
        /// 调用方期望的指纹（`None` = 期望文件不存在）。
        expected: Option<FileFingerprint>,
        /// 写前实测指纹（`None` = 文件不存在）。
        actual: Option<FileFingerprint>,
    },
    /// 锁争用（P0-1）：另一 `nctool` 进程正在写同一文件，本次**未做任何改动**。
    ///
    /// 与 [`WriteError::Conflict`] 的分工（两者都可重试，CLI 同归
    /// `write_conflict` / 退出码 6，HTTP 同为 409 `write_conflict`）：
    ///
    /// - `Conflict` = 内容**已被改**（乐观锁失败）→ 以新内容为基线重试；
    /// - `LockBusy` = 另一进程**正在写**（互斥锁未释放）→ 稍后重试同一操作。
    ///
    /// 路径仅供 CLI/stderr 消息使用；**HTTP 响应体不得回显路径**（见 `lock`
    /// 子模块文档的脱敏约束，P1-17 口径）。
    LockBusy {
        /// 被锁写入目标的路径（锁文件为同目录 `<stem>.nctool.lock`）。
        path: PathBuf,
    },
    /// 路径越界：名称非法或解析结果逃出安全根。
    PathEscape {
        /// 被拒绝的相对名称。
        rel: String,
        /// 拒绝原因。
        reason: String,
    },
    /// 目标只读 / 无写入权限。
    ReadOnly {
        /// 目标路径。
        path: PathBuf,
    },
    /// 其余 IO 失败。
    Io(std::io::Error),
    /// 操作的目标条目不存在（如 `rename` / `remove` 指定的预设名不在文件里）。
    ///
    /// # 为什么要独立于 [`WriteError::Corrupt`]
    ///
    /// 早期把"预设不存在"塞进 `Corrupt`，于是下游只能靠**匹配消息文本**里有没有
    /// `"不存在"` 来分类（CLI 与 HTTP 各写一遍）。那是 D7 明令禁止的做法：
    /// 文案一改，分类就静默错位——HTTP 侧会把"删一个不存在的预设"报成
    /// **500 内部错误**（本该是 4xx 的调用方问题）。结构化字段是唯一可靠判据。
    NotFound(String),
    /// 写入目标不可用（路径存在但不是普通文件，如目录占位）。
    ///
    /// 注意：**并非**"数据损坏"——通常是目标位置被同名目录占用，改名或移除即可。
    Corrupt(String),
    /// 文件中的数值字面量下溢（`|真值| < 2^-1000` 且被 YAML 解析器静默归零）。
    ///
    /// **为什么是错误而非警告**：被静默归零的值（如预设参数 `value: 1e-400` → `0.0`）
    /// 会直接参与渲染，产出错误坐标（撞刀）。故这类**硬失败**，且**不**参与
    /// "损坏文件降级"策略（ERR-NUM-UNDERFLOW，设计 §4.1 / 附录 D6）。
    ///
    /// 定位信息（行/列 + 字面量）用于让用户直接改到那一行。
    NumRange {
        literal: String,
        line: usize,
        column: usize,
    },
    NumUnderflow {
        /// 文件路径。
        path: PathBuf,
        /// 触发下溢的原始字面量文本。
        literal: String,
        /// 字面量所在行（1 起）。
        line: usize,
        /// 字面量所在列（1 起，字节计）。
        column: usize,
    },
}

impl std::fmt::Display for WriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WriteError::Conflict { path, .. } => {
                write!(f, "写入冲突：{} 已被外部修改（乐观锁失败）", path.display())
            }
            WriteError::LockBusy { path } => {
                write!(
                    f,
                    "文件正被另一个 nctool 进程写入：{}（未改动，稍后重试）",
                    path.display()
                )
            }
            WriteError::PathEscape { rel, reason } => {
                write!(f, "路径越界被拒绝：{rel}（{reason}）")
            }
            WriteError::ReadOnly { path } => {
                write!(f, "目标只读或无写入权限：{}", path.display())
            }
            WriteError::Io(err) => write!(f, "IO 错误：{err}"),
            WriteError::NotFound(msg) => write!(f, "{msg}"),
            WriteError::Corrupt(msg) => write!(f, "写入目标不可用：{msg}"),
            WriteError::NumRange { literal, line, column } => write!(f, "整数超出 JSON 支持范围 (i64/u64): {literal} at {line}:{column}"),
            WriteError::NumUnderflow {
                path,
                literal,
                line,
                column,
            } => write!(
                f,
                "JSON {} 第 {line} 行第 {column} 列：非零数值 {literal} 在解析时下溢为零。请使用可表示的数值。",
                path.display()
            ),
        }
    }
}

impl std::error::Error for WriteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            WriteError::Io(err) => Some(err),
            _ => None,
        }
    }
}

impl From<std::io::Error> for WriteError {
    fn from(err: std::io::Error) -> Self {
        WriteError::Io(err)
    }
}

/// 把 IO 错误按类别归一：`PermissionDenied` → [`WriteError::ReadOnly`]，
/// 其余 → [`WriteError::Io`]。`path` 用于错误信息（一般为写目标）。
pub(crate) fn map_io(err: std::io::Error, path: &Path) -> WriteError {
    if err.kind() == std::io::ErrorKind::PermissionDenied {
        WriteError::ReadOnly {
            path: path.to_path_buf(),
        }
    } else {
        WriteError::Io(err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Error, ErrorKind};
    use std::sync::{mpsc, Arc, Barrier};
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    fn conflict() -> WriteError {
        WriteError::Conflict {
            path: PathBuf::from("a.j2"),
            expected: None,
            actual: Some(FileFingerprint::of_bytes(b"x", UNIX_EPOCH)),
        }
    }

    #[test]
    fn display_covers_every_variant() {
        let cases = [
            conflict(),
            WriteError::PathEscape {
                rel: "../x".into(),
                reason: "越界".into(),
            },
            WriteError::ReadOnly {
                path: PathBuf::from("b.j2"),
            },
            WriteError::Io(Error::new(ErrorKind::NotFound, "boom")),
            WriteError::Corrupt("同名目录占位".into()),
            WriteError::LockBusy {
                path: PathBuf::from("p.yaml"),
            },
        ];
        let texts: Vec<String> = cases.iter().map(|e| format!("{e}")).collect();
        assert!(texts[0].contains("写入冲突"), "{}", texts[0]);
        assert!(texts[1].contains("路径越界"), "{}", texts[1]);
        assert!(texts[2].contains("只读"), "{}", texts[2]);
        assert!(texts[3].contains("IO 错误"), "{}", texts[3]);
        assert!(texts[4].contains("写入目标不可用"), "{}", texts[4]);
        // LockBusy 文案必须与 Conflict 区分（锁争用 ≠ 内容被改），且带路径供 CLI 定位
        assert!(texts[5].contains("另一个 nctool 进程"), "{}", texts[5]);
        assert!(texts[5].contains("p.yaml"), "{}", texts[5]);
        assert!(
            !texts[5].contains("外部修改"),
            "锁争用不得复用 Conflict 文案: {}",
            texts[5]
        );
        assert!(texts.iter().all(|t| !t.is_empty()));
    }

    #[test]
    fn error_source_is_some_only_for_io() {
        let io = WriteError::Io(Error::new(ErrorKind::NotFound, "nope"));
        assert!(std::error::Error::source(&io).is_some());
        assert!(std::error::Error::source(&conflict()).is_none());
    }

    #[test]
    fn from_io_error_maps_to_io_variant() {
        let err: WriteError = Error::new(ErrorKind::NotFound, "nope").into();
        assert!(matches!(err, WriteError::Io(_)));
    }

    #[test]
    fn map_io_classifies_permission_denied_as_readonly() {
        let denied = map_io(
            Error::new(ErrorKind::PermissionDenied, "no"),
            Path::new("t"),
        );
        assert!(matches!(denied, WriteError::ReadOnly { .. }));
        let other = map_io(Error::new(ErrorKind::NotFound, "no"), Path::new("t"));
        assert!(matches!(other, WriteError::Io(_)));
    }

    #[test]
    fn outcome_serializes_with_lowercase_action() {
        let out = WriteOutcome {
            path: PathBuf::from("a.j2"),
            action: WriteAction::Created,
            fingerprint: Some("fnv1a64:0000000000000000".into()),
            revision: None,
        };
        let json = serde_json::to_value(&out).unwrap();
        assert_eq!(json["action"], "created");
        assert_eq!(json["path"], "a.j2");
        assert_eq!(json["fingerprint"], "fnv1a64:0000000000000000");
    }

    /// 测试临时目录：`pid + 纳秒时间戳` 保证并行测试互不踩踏。
    fn tmpdir(tag: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("nctool_lock_{tag}_{}_{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("应能创建临时目录");
        dir
    }

    /// P0-1（SUMMARY §8-6）：两个写者并发写同一路径、各自以**同一份旧指纹**
    /// 为 `expect`（经典"读-改-写"丢失更新场景）。
    ///
    /// 修复前（check-then-write 零互斥）：两者都通过比对、后 `rename` 者胜、
    /// 先写者的一整次编辑被静默丢弃（双方都收到 `Ok`）—— 本测试必须红。
    /// 修复后：锁把临界区串行化，恰好一个 `Ok`、另一个 `Conflict`（等锁后读到
    /// 的已是新指纹），文件内容恰为胜者全文、无任何交错。
    #[test]
    fn concurrent_writes_to_same_path_never_corrupt() {
        let dir = tmpdir("concurrent");
        let path = dir.join("data.yaml");
        std::fs::write(&path, b"original").unwrap();
        let fp = WriteKernel::read_fingerprint(&path)
            .unwrap()
            .expect("文件存在，应有指纹");

        let spawn = |bytes: &'static [u8]| {
            let path = path.clone();
            std::thread::spawn(move || WriteKernel::write_guarded(&path, bytes, Some(fp)))
        };
        let a = spawn(b"AAAAAAAAAAAAAAAAAAAA");
        let b = spawn(b"BBBBBBBBBBBBBBBBBBBB");
        let ra = a.join().unwrap();
        let rb = b.join().unwrap();

        let oks = [&ra, &rb].iter().filter(|r| r.is_ok()).count();
        assert_eq!(oks, 1, "必须恰好一个写者成功：ra={ra:?} rb={rb:?}");
        let loser = if ra.is_err() { &ra } else { &rb };
        assert!(
            matches!(loser, Err(WriteError::Conflict { .. })),
            "落败者应为 Conflict（等锁进入临界区后发现指纹已变）：{loser:?}"
        );
        let winner_bytes: &[u8] = if ra.is_ok() {
            b"AAAAAAAAAAAAAAAAAAAA"
        } else {
            b"BBBBBBBBBBBBBBBBBBBB"
        };
        assert_eq!(
            std::fs::read(&path).unwrap(),
            winner_bytes,
            "文件必须恰为胜者全文，绝无交错/半成品"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// P0-1 回归守卫（2026-09-27）：**首次创建**锁文件时的 Windows 竞态
    /// 不得被误报成「文件只读」。
    ///
    /// Windows 上两个线程并发对**尚不存在**的路径 `CreateFile(OPEN_ALWAYS,
    /// GENERIC_WRITE)`，落败者拿到的是 `ERROR_ACCESS_DENIED(5)`（实测 200/200，
    /// 见 `examples/probe_create_race.rs`），`map_io` 会把它归类为
    /// [`WriteError::ReadOnly`] —— 于是上面那条并发写测试的落败者得到
    /// `ReadOnly` 而不是语义正确的 [`WriteError::Conflict`]，测试在 Windows 上
    /// 恒红。修法见 `lock::open_lock_file`（`create_new` 两步）。
    ///
    /// 本测试在 `try_open` 层面直接钉住归因：并发首次取锁只允许出现
    /// 「拿到锁」或「`LockBusy`」，**绝不允许 `ReadOnly`**。
    #[test]
    fn concurrent_first_lock_file_creation_is_not_misreported_as_readonly() {
        let dir = tmpdir("lockcreate");
        let target = dir.join("presets.yaml");
        std::fs::write(&target, b"v1").unwrap();
        let lock_path = lock::lock_path_for(&target);
        assert!(
            !lock_path.exists(),
            "锁文件必须尚未存在 —— 该竞态只在「首次创建」发生"
        );

        let barrier = Arc::new(Barrier::new(2));
        let handles: Vec<_> = (0..2)
            .map(|_| {
                let target = target.clone();
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    // 两个线程尽量同一瞬间进入 open，最大化复现竞态的概率。
                    barrier.wait();
                    match lock::DirLock::try_lock(&target) {
                        Ok(_held) => "held",
                        Err(WriteError::LockBusy { .. }) => "busy",
                        Err(other) => {
                            panic!("并发首次取锁不得报 {other:?}（只允许 held / LockBusy）")
                        }
                    }
                })
            })
            .collect();
        let outcomes: Vec<&str> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert!(
            outcomes.contains(&"held"),
            "必须至少有一个线程拿到锁：{outcomes:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 且不改动文件；持锁者释放后同一 `expect` 重试成功（可重试语义）。
    #[test]
    fn try_lock_reports_busy_then_succeeds_after_release() {
        let dir = tmpdir("trylock");
        let path = dir.join("presets.yaml");
        std::fs::write(&path, b"v1").unwrap();
        let fp = WriteKernel::read_fingerprint(&path)
            .unwrap()
            .expect("文件存在，应有指纹");

        let (held_tx, held_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let holder_path = path.clone();
        let holder = std::thread::spawn(move || {
            let _lock = lock::DirLock::acquire(&holder_path, Duration::from_secs(10))
                .expect("无争用，应拿到锁");
            held_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            // _lock 离开作用域时 Drop 解锁
        });
        held_rx.recv().expect("持锁线程应已拿锁");

        let start = Instant::now();
        let err = WriteKernel::write_guarded_with_wait(&path, b"v2", Some(fp), Duration::ZERO)
            .expect_err("锁被持有时 try-lock 必须失败");
        assert!(matches!(err, WriteError::LockBusy { .. }), "{err:?}");
        assert!(
            start.elapsed() < Duration::from_secs(1),
            "等待 0 不得阻塞：{:?}",
            start.elapsed()
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"v1", "被拒绝时文件不得改动");

        release_tx.send(()).unwrap();
        holder.join().unwrap();
        WriteKernel::write_guarded_with_wait(&path, b"v2", Some(fp), Duration::ZERO)
            .expect("锁释放后重试应成功");
        assert_eq!(std::fs::read(&path).unwrap(), b"v2");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// P0-1 实现细节：锁文件 = `目标.with_extension("nctool.lock")`（与目标同
    /// 目录）；**只 unlock 不 unlink** —— 删被锁文件会让别人已打开的 fd 失效、
    /// 互斥被打破；进程死亡由 OS 自动解锁，磁盘留空锁文件是有意代价。
    #[test]
    fn lock_file_is_named_stem_nctool_lock_and_survives_release() {
        let dir = tmpdir("lockname");
        let path = dir.join("presets.yaml");
        std::fs::write(&path, b"x").unwrap();

        let lock_path = lock::lock_path_for(&path);
        assert_eq!(lock_path, dir.join("presets.nctool.lock"));
        {
            let _l = lock::DirLock::acquire(&path, Duration::ZERO).expect("无争用应成功");
            assert!(lock_path.exists(), "取锁时锁文件应已创建");
        }
        assert!(
            lock_path.exists(),
            "解锁后不得删除锁文件（unlink 会让他人已持有的锁失效）"
        );
        lock::DirLock::acquire(&path, Duration::ZERO).expect("锁释放后应可再次获取，无陈旧锁");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// JSON assets share the write kernel; no domain-specific manifest or machine rules.
#[derive(Clone)]
pub struct JsonStore {
    root: SafePath,
}
impl JsonStore {
    pub fn new(root: &Path) -> Result<Self, WriteError> {
        std::fs::create_dir_all(root)?;
        Ok(Self {
            root: SafePath::from_root(root)?,
        })
    }
    fn path(&self, name: &str) -> Result<PathBuf, WriteError> {
        validate_asset_name(name).map_err(|reason| WriteError::PathEscape {
            rel: name.into(),
            reason,
        })?;
        self.root.resolve(&format!("{name}.json"))
    }
    pub fn list(&self) -> Result<Vec<String>, WriteError> {
        let mut names = Vec::new();
        for entry in std::fs::read_dir(self.root.root())? {
            let entry = entry?;
            if entry.file_type()?.is_file() {
                if let Some(s) = entry
                    .file_name()
                    .to_str()
                    .and_then(|s| s.strip_suffix(".json"))
                {
                    names.push(s.into());
                }
            }
        }
        names.sort();
        Ok(names)
    }
    pub fn read(&self, name: &str) -> Result<(serde_json::Value, String), WriteError> {
        let (bytes, fp) = guard::read_snapshot(&self.path(name)?)?
            .ok_or_else(|| WriteError::NotFound(name.into()))?;
        let value = parse_json(&bytes)?;
        let token = serde_json::to_string(&fp).map_err(|e| WriteError::Corrupt(e.to_string()))?;
        Ok((value, token))
    }
    /// Delete requires the complete revision supplied by the read operation.
    pub fn remove(&self, name: &str, expected: &str) -> Result<(), WriteError> {
        let fingerprint =
            serde_json::from_str(expected).map_err(|e| WriteError::Corrupt(e.to_string()))?;
        WriteKernel::remove_guarded(&self.path(name)?, Some(fingerprint))?;
        Ok(())
    }
    /// None means create-only. Existing assets always require the previous opaque token.
    pub fn save(
        &self,
        name: &str,
        value: &serde_json::Value,
        expected: Option<&str>,
    ) -> Result<String, WriteError> {
        let bytes =
            serde_json::to_vec_pretty(value).map_err(|e| WriteError::Corrupt(e.to_string()))?;
        if bytes.len() > 1024 * 1024 {
            return Err(WriteError::Corrupt("asset exceeds 1 MiB".into()));
        }
        let fp = expected
            .map(serde_json::from_str::<FileFingerprint>)
            .transpose()
            .map_err(|e| WriteError::Corrupt(e.to_string()))?;
        let path = self.path(name)?;
        let outcome = WriteKernel::write_guarded_with_wait(&path, &bytes, fp, Duration::ZERO)?;
        serde_json::to_string(
            &outcome
                .revision
                .ok_or_else(|| WriteError::Corrupt("missing write revision".into()))?,
        )
        .map_err(|e| WriteError::Corrupt(e.to_string()))
    }
}

/// Parse JSON while rejecting silent integer overflow and nonzero decimal underflow.
pub fn parse_json(bytes: &[u8]) -> Result<serde_json::Value, WriteError> {
    let value = serde_json::from_slice(bytes).map_err(|e| WriteError::Corrupt(e.to_string()))?;
    let text = std::str::from_utf8(bytes).map_err(|e| WriteError::Corrupt(e.to_string()))?;
    if let Some(token) = json_num::scan_json_integer_overflows(text)
        .into_iter()
        .next()
    {
        return Err(WriteError::NumRange {
            literal: token.literal,
            line: token.line,
            column: token.column,
        });
    }
    for candidate in json_num::scan_underflow_candidates(text) {
        if serde_json::from_str::<f64>(&candidate.literal).ok() == Some(0.) {
            return Err(WriteError::NumUnderflow {
                path: PathBuf::from("<json>"),
                literal: candidate.literal,
                line: candidate.line,
                column: candidate.column,
            });
        }
    }
    Ok(value)
}

/// Shared lock for coherent collection snapshots and cross-process asset mutations.
pub struct GroupGuard {
    _guard: lock::DirLock,
}
impl GroupGuard {
    pub fn acquire(root: &Path) -> Result<Self, WriteError> {
        Ok(Self {
            _guard: lock::DirLock::acquire(&root.join(".asset-group"), Duration::from_secs(2))?,
        })
    }
}
/// Export every record in the named stores with stable identifiers.
pub fn export_collection(stores: &[(&str, &JsonStore)]) -> Result<serde_json::Value, WriteError> {
    let mut records = vec![];
    let mut total = 0usize;
    for (group, store) in stores {
        for name in store.list()? {
            let (asset, _) = store.read(&name)?;
            total += serde_json::to_vec(&asset)
                .map_err(|e| WriteError::Corrupt(e.to_string()))?
                .len();
            if total > 64 * 1024 * 1024 {
                return Err(WriteError::Corrupt("资产集合超过 64 MiB".into()));
            }
            records.push(serde_json::json!({"group":group,"name":name,"asset":asset}));
        }
    }
    Ok(serde_json::json!({"version":1,"records":records}))
}
/// Preflight avoids overwrite; apply returns revisions usable for guarded rollback.
pub fn import_collection(
    stores: &[(&str, &JsonStore)],
    data: &serde_json::Value,
    validate_only: bool,
    validate: impl Fn(&str, &str, &serde_json::Value) -> Result<(), WriteError>,
) -> Result<serde_json::Value, WriteError> {
    if data["version"].as_u64() != Some(1) {
        return Err(WriteError::Corrupt("不支持的资产集合版本".into()));
    }
    let records = data["records"]
        .as_array()
        .ok_or_else(|| WriteError::Corrupt("缺少资产列表".into()))?;
    if records.len() > 2048 {
        return Err(WriteError::Corrupt("集合最多包含 2048 个资产".into()));
    }
    let mut seen = std::collections::BTreeSet::new();
    for record in records {
        let group = record["group"]
            .as_str()
            .ok_or_else(|| WriteError::Corrupt("缺少集合名称".into()))?;
        let name = record["name"]
            .as_str()
            .ok_or_else(|| WriteError::Corrupt("缺少资产标识".into()))?;
        let store = stores
            .iter()
            .find(|(key, _)| *key == group)
            .map(|(_, store)| *store)
            .ok_or_else(|| WriteError::Corrupt(format!("未知资产类型: {group}")))?;
        validate_asset_name(name).map_err(WriteError::Corrupt)?;
        if !seen.insert((group, name)) {
            return Err(WriteError::Corrupt(format!("集合中有重复标识: {name}")));
        }
        if WriteKernel::read_fingerprint(&store.path(name)?)?.is_some() {
            return Err(WriteError::Conflict {
                path: store.path(name)?,
                expected: None,
                actual: WriteKernel::read_fingerprint(&store.path(name)?)?,
            });
        }
        if !record["asset"].is_object() {
            return Err(WriteError::Corrupt("资产必须是对象".into()));
        }
        if serde_json::to_vec(&record["asset"])
            .map_err(|e| WriteError::Corrupt(e.to_string()))?
            .len()
            > 1024 * 1024
        {
            return Err(WriteError::Corrupt("单个资产超过 1 MiB".into()));
        }
        validate(group, name, &record["asset"])?;
    }
    if validate_only {
        return Ok(
            serde_json::json!({"count":records.len(),"mapping":records.iter().map(|record|serde_json::json!({"group":record["group"],"from":record["name"],"to":record["name"]})).collect::<Vec<_>>()}),
        );
    }
    let mut receipt = vec![];
    for record in records {
        let group = record["group"].as_str().unwrap();
        let name = record["name"].as_str().unwrap();
        let store = stores.iter().find(|(key, _)| *key == group).unwrap().1;
        match store.save(name, &record["asset"], None) {
            Ok(fingerprint) => receipt
                .push(serde_json::json!({"group":group,"name":name,"fingerprint":fingerprint})),
            Err(error) => {
                let rollback = rollback_collection(stores, &serde_json::json!(receipt));
                return if rollback.is_ok() {
                    Err(error)
                } else {
                    Err(WriteError::Corrupt(format!(
                        "导入失败，回滚受冲突阻止: {error}"
                    )))
                };
            }
        }
    }
    Ok(serde_json::json!({"count":records.len(),"receipt":receipt}))
}
pub fn rollback_collection(
    stores: &[(&str, &JsonStore)],
    receipt: &serde_json::Value,
) -> Result<(), WriteError> {
    let records = receipt
        .as_array()
        .ok_or_else(|| WriteError::Corrupt("缺少回滚凭证".into()))?;
    for record in records.iter().rev() {
        let group = record["group"]
            .as_str()
            .ok_or_else(|| WriteError::Corrupt("无效回滚凭证".into()))?;
        let store = stores
            .iter()
            .find(|(key, _)| *key == group)
            .ok_or_else(|| WriteError::Corrupt("未知资产类型".into()))?
            .1;
        store.remove(
            record["name"]
                .as_str()
                .ok_or_else(|| WriteError::Corrupt("缺少资产名称".into()))?,
            record["fingerprint"]
                .as_str()
                .ok_or_else(|| WriteError::Corrupt("缺少版本指纹".into()))?,
        )?;
    }
    Ok(())
}
