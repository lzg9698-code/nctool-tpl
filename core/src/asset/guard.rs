//! 乐观锁指纹：以 `(hash, len, mtime)` 三元组刻画文件写前快照。
//!
//! 指纹算法为 64 位 FNV-1a（本模块自带实现，零新增依赖）。**只用于变更检测，
//! 不是安全用途**：不做防篡改承诺，也不需要抗碰撞强度。

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use super::{map_io, WriteError};

/// 文件指纹：内容哈希 + 字节长度 + 修改时间。
///
/// 三者**任一不同**即视为文件被改动（见 [`FileFingerprint::matches`]）。
/// 有意**不**采用"纯 mtime 比对"：Windows 的文件时间分辨率较粗，同一秒内
/// 连续两次修改会得到相同 mtime，从而漏检覆盖冲突；内容哈希不受此影响。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileFingerprint {
    /// 内容哈希（FNV-1a 64 位）。
    pub hash: u64,
    /// 内容字节长度。
    pub len: u64,
    /// 文件修改时间。
    pub mtime: SystemTime,
}

impl FileFingerprint {
    /// 由内容字节与修改时间构造指纹。
    pub fn of_bytes(bytes: &[u8], mtime: SystemTime) -> Self {
        Self {
            hash: fnv1a64(bytes),
            len: bytes.len() as u64,
            mtime,
        }
    }

    /// 两个指纹是否等价（`hash`、`len`、`mtime` 三者全等）。
    pub fn matches(&self, other: &Self) -> bool {
        self.hash == other.hash && self.len == other.len && self.mtime == other.mtime
    }

    /// 指纹的可读字符串形式：`fnv1a64:<16 位十六进制>`。
    ///
    /// 仅展示内容哈希（长度与时间不参与），供 CLI `--expect-hash` 提示与日志用。
    pub fn as_string(&self) -> String {
        format!("fnv1a64:{:016x}", self.hash)
    }
}

/// FNV-1a 64 位哈希（零依赖实现）。
///
/// 选它而非 `sha2` / `blake3`：指纹只用于变更检测，不需要抗碰撞强度；
/// 引入加密哈希会新增 crate，违背本设计的零新增依赖约束。
pub(crate) fn fnv1a64(bytes: &[u8]) -> u64 {
    const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = OFFSET_BASIS;
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

/// 读取文件快照：`(内容字节, 指纹)`；文件不存在返回 `None`。
///
/// 路径存在但不是普通文件（目录等）→ [`WriteError::Corrupt`]。
pub(crate) fn read_snapshot(path: &Path) -> Result<Option<(Vec<u8>, FileFingerprint)>, WriteError> {
    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(map_io(e, path)),
    };
    if !meta.is_file() {
        return Err(WriteError::Corrupt(format!(
            "目标位置已被占用（存在同名目录或非普通文件），请改名或先移除该目录: {}",
            path.display()
        )));
    }
    let bytes = std::fs::read(path).map_err(|e| map_io(e, path))?;
    // 取不到修改时间时回退 `UNIX_EPOCH`：hash/len 仍能区分内容变更，
    // 只是失去"同内容但时间变化"这一维（不影响正确性，只是弱化一点）。
    let mtime = meta.modified().unwrap_or(UNIX_EPOCH);
    let fingerprint = FileFingerprint::of_bytes(&bytes, mtime);
    Ok(Some((bytes, fingerprint)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// FNV-1a 64 的公开标准向量：钉住实现，防止误改成 FNV-1 或乘错质数。
    #[test]
    fn fnv1a64_matches_published_vectors() {
        assert_eq!(
            fnv1a64(b""),
            0xcbf2_9ce4_8422_2325,
            "空输入应等于 offset basis"
        );
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a64(b"foobar"), 0x8594_4171_f739_67e8);
    }

    #[test]
    fn fingerprint_is_deterministic_and_sensitive() {
        let t = UNIX_EPOCH;
        let a = FileFingerprint::of_bytes(b"abc", t);
        let b = FileFingerprint::of_bytes(b"abc", t);
        assert!(a.matches(&b), "同内容同时间应等价");
        assert_eq!(a.len, 3);
        assert_eq!(a.as_string(), format!("fnv1a64:{:016x}", a.hash));
        assert_eq!(a.as_string().len(), "fnv1a64:".len() + 16);

        // 内容变化 → hash 变化
        let c = FileFingerprint::of_bytes(b"abd", t);
        assert_ne!(a.hash, c.hash);
        assert!(!a.matches(&c));

        // 长度变化（hash 也随之变）→ 不等价
        let longer = FileFingerprint::of_bytes(b"abcd", t);
        assert!(!a.matches(&longer));

        // 仅 mtime 变化 → 不等价（三要素任一不同即冲突）
        let later = FileFingerprint::of_bytes(b"abc", t + Duration::from_secs(1));
        assert_eq!(a.hash, later.hash);
        assert_eq!(a.len, later.len);
        assert!(!a.matches(&later), "mtime 不同必须判为改动");
    }
}
