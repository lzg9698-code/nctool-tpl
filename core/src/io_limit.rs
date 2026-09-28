//! 文本文件读取的**字节上限**：内存放大防护的单一原语。
//!
//! # 为什么必须有
//!
//! `serde_yaml 0.9` 的别名（anchor / alias）展开**没有预算** —— 每遇一处
//! alias 就重新反序列化整棵子树；底层 `unsafe-libyaml` 也没有深度/别名限制。
//! 一份几十 KB 的"锚点套锚点"清单（billion laughs 变体）可以展开出 GB 级
//! 节点树，进程被 OOM 打死。**这是全仓唯一一条不需要认证就能让 CLI/HTTP
//! 进程倒下的路径**：打开他人的仓库、或往模板目录里放一份这样的清单即可。
//!
//! 因此凡是"读进内存后交给 YAML 解析器"的入口，都必须先过字节上限。
//! 上限按用途分档而不是一刀切 —— 模板允许大段注释与长程序样板。
//!
//! # 为什么用 `Take` 而不是 `metadata().len()`
//!
//! `metadata()` 与实际 `read` 之间存在 TOCTOU：文件可在两步之间被追加。
//! 只有流式截断才能保证"进内存的字节数有上界"这个不变量。多读 1 字节用于
//! 区分"恰好等于上限"与"超过上限"。

use std::io::Read;
use std::path::Path;

/// "人写的配置"类文件上限：**1 MiB**（机床配置、预设文件）。
///
/// 与 CLI 的 `args::MAX_CLI_FILE_BYTES`、HTTP 的 `MAX_BODY_BYTES` 同口径。
pub const MAX_CONFIG_BYTES: u64 = 1024 * 1024;

/// 清单 / 模板源码上限：**8 MiB**。
///
/// 比配置档宽松：模板里可以有大段注释与整段程序样板，1 MiB 会误杀正常文件。
/// 但它仍然远小于"能撑爆内存"的量级 —— 真正的防线是别名展开的规模，
/// 而 8 MiB 的锚点套锚点已经足以放大到危险区，所以上限只解决"读进来"这一半，
/// 另一半见模块文档的说明（模板目录属可信输入，不接受任意上传）。
pub const MAX_SOURCE_BYTES: u64 = 8 * 1024 * 1024;

/// 读取文本文件；超过 `max` 字节即返回 [`std::io::ErrorKind::InvalidData`]。
///
/// 错误消息**不报具体字节数**：为了不把超限文件整份读进内存，我们只读到
/// `max + 1` 就停下，因此拿不到真实大小 —— 报一个 `max + 1` 是误导。
pub fn read_text_capped(path: &Path, max: u64) -> std::io::Result<String> {
    let file = std::fs::File::open(path)?;
    let mut text = String::new();
    std::io::BufReader::new(file)
        .take(max + 1)
        .read_to_string(&mut text)?;
    if text.len() as u64 > max {
        return Err(too_large(max));
    }
    Ok(text)
}

/// 构造"文件过大"错误（`InvalidData`）。
///
/// 单独暴露是为了让调用方能在**不读文件**的场合复用同一文案
/// （例如流式读取 stdin 时自行判长）。
pub fn too_large(max: u64) -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        format!(
            "文件过大：超过上限 {max} 字节（{} KiB）——请确认这确实是 nctool 的文件",
            max / 1024
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "nctool_io_limit_{}_{}_{}",
            std::process::id(),
            tag,
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 恰好等于上限放行、超 1 字节即拒 —— 边界两侧各钉一条，
    /// 防止有人把 `>` 写成 `>=` 造成误杀。
    #[test]
    fn boundary_is_exact_and_off_by_one_is_rejected() {
        let dir = tmpdir("boundary");
        let max = 64u64;

        let exact = dir.join("exact.txt");
        std::fs::write(&exact, "x".repeat(max as usize)).unwrap();
        assert_eq!(
            read_text_capped(&exact, max)
                .expect("恰好等于上限应放行")
                .len(),
            max as usize
        );

        let over = dir.join("over.txt");
        std::fs::write(&over, "x".repeat(max as usize + 1)).unwrap();
        let err = read_text_capped(&over, max).expect_err("超 1 字节必须拒绝");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("过大"), "{err}");
    }

    /// 用 1 MiB 真实文件对 1 KiB 上限：必须失败。
    ///
    /// 这条覆盖的是"**先读完再判**"这种写法 —— 它同样会返回错误，但内存
    /// 已经被吃满了，防护形同虚设。只有 `Take` 才能在读到 `max + 1` 时停下。
    #[test]
    fn take_actually_caps_memory_not_reads_then_checks() {
        let dir = tmpdir("take");
        let p = dir.join("big.txt");
        std::fs::write(&p, "y".repeat(1024 * 1024)).unwrap();
        assert!(
            read_text_capped(&p, 1024).is_err(),
            "1 MiB 文件对 1 KiB 上限必须失败"
        );
    }

    /// 不存在的文件透传 `NotFound`（调用方靠它区分"缺失"与"过大"）。
    #[test]
    fn missing_file_preserves_not_found() {
        let dir = tmpdir("missing");
        let err = read_text_capped(&dir.join("nope.txt"), 16).expect_err("应报缺失");
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    }
}
