//! 资产写入内核：三大编辑模块（模板 / 机床 / 预设）的**唯一落盘通道**。
//!
//! 本模块把"写一个文件"这件危险的事收敛到一处，集中提供四项保证：
//!
//! 1. **原子写**：先写同目录临时文件，`flush`/`fsync` 后再 `rename` 覆盖，
//!    目标文件在任何时刻要么是旧内容、要么是新内容，不存在"半成品"。
//! 2. **乐观锁**：[`FileFingerprint`] 以 `(hash, len, mtime)` 三元组刻画写前快照，
//!    写前重读比对，任一不同即 [`WriteError::Conflict`]，绝不静默覆盖他人改动。
//! 3. **路径防护**：[`SafePath`] 以 canonicalize + 根包含校验拒绝路径穿越与
//!    符号链接逃逸；名称校验统一走 [`validate_asset_name`]（模板 / 机床 / 预设共用）。
//! 4. **单一入口**：[`WriteKernel`] 无状态，CLI 与 Web UI 共用同一组关联函数，
//!    任何"自己拼文件字节"的旁路都违背本设计的单一来源原则。
//!
//! 格式相关策略（模板 `templates.yaml` 文本编辑、机床 `nctool.toml` 的
//! `toml_edit` upsert、预设 serde 往返）在后续模块中实现，但**一律经由本内核落盘**。

mod atomic;
mod guard;
mod path;
mod spec_fingerprint;
pub mod template;

use std::path::{Path, PathBuf};

pub use guard::FileFingerprint;
pub use path::{validate_asset_name, SafePath};
pub use spec_fingerprint::SpecFingerprint;
pub use template::{
    build_derived_source, last_component, manifest_append_entry, manifest_contains_key,
    manifest_entry_body, manifest_is_parseable, manifest_rewrite_key, scan_stale_includes,
    sibling_rel_key, DeriveReport, ManifestOutcome, RenameReport, TemplateWriter,
};

/// 原子写 + 乐观锁 + 指纹读取的统一写内核。
///
/// 无状态：所有方法均为关联函数，调用方无需持有实例。整个 crate 只有这一处
/// 真正调用 `std::fs::rename`，以保证"单一写入口"。
pub struct WriteKernel;

impl WriteKernel {
    /// 原子写：把 `bytes` 写入 `path`，不存在则创建、存在则覆盖。
    ///
    /// 实现见 `atomic` 子模块：同目录临时文件 → `flush`/`fsync` → `rename`。
    /// **不做乐观锁比对**（调用方已确认可覆盖时使用）。
    pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), WriteError> {
        atomic::write_atomic(path, bytes)
    }

    /// 乐观锁写：先比对 `expect` 与实际指纹，一致才落盘。
    ///
    /// - `expect = Some(fp)`：要求写前文件指纹恰为 `fp`；
    /// - `expect = None`：要求写前**文件不存在**（"新建"语义）。
    ///
    /// 不一致返回 [`WriteError::Conflict`]，**不写入**。一致时：目标不存在 →
    /// [`WriteAction::Created`]；内容与 `bytes` 相同 → [`WriteAction::Unchanged`]
    /// （跳过落盘）；否则 [`WriteAction::Updated`]。
    pub fn write_guarded(
        path: &Path,
        bytes: &[u8],
        expect: Option<FileFingerprint>,
    ) -> Result<WriteOutcome, WriteError> {
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
        let fingerprint = Self::read_fingerprint(path)?.map(|fp| fp.as_string());
        Ok(WriteOutcome {
            path: path.to_path_buf(),
            action,
            fingerprint,
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
    /// 写入目标不可用（路径存在但不是普通文件，如目录占位）。
    ///
    /// 注意：**并非**"数据损坏"——通常是目标位置被同名目录占用，改名或移除即可。
    Corrupt(String),
}

impl std::fmt::Display for WriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WriteError::Conflict { path, .. } => {
                write!(f, "写入冲突：{} 已被外部修改（乐观锁失败）", path.display())
            }
            WriteError::PathEscape { rel, reason } => {
                write!(f, "路径越界被拒绝：{rel}（{reason}）")
            }
            WriteError::ReadOnly { path } => {
                write!(f, "目标只读或无写入权限：{}", path.display())
            }
            WriteError::Io(err) => write!(f, "IO 错误：{err}"),
            WriteError::Corrupt(msg) => write!(f, "写入目标不可用：{msg}"),
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
    use std::time::UNIX_EPOCH;

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
        ];
        let texts: Vec<String> = cases.iter().map(|e| format!("{e}")).collect();
        assert!(texts[0].contains("写入冲突"), "{}", texts[0]);
        assert!(texts[1].contains("路径越界"), "{}", texts[1]);
        assert!(texts[2].contains("只读"), "{}", texts[2]);
        assert!(texts[3].contains("IO 错误"), "{}", texts[3]);
        assert!(texts[4].contains("写入目标不可用"), "{}", texts[4]);
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
        };
        let json = serde_json::to_value(&out).unwrap();
        assert_eq!(json["action"], "created");
        assert_eq!(json["path"], "a.j2");
        assert_eq!(json["fingerprint"], "fnv1a64:0000000000000000");
    }
}
