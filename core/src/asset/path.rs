//! 路径穿越防护：安全根内的相对名称解析与名称合法性校验。

use std::path::{Component, Path, PathBuf};

use super::{map_io, WriteError};

/// 校验资产名称（模板名 / 机床 id / 预设名共用）。
///
/// 合法名称的**唯一形态**：恰好一个 [`Component::Normal`]。由此一并拒绝：
/// 空串、`.`（`CurDir`）、`..`（`ParentDir`）、含 `/` 的多段路径、
/// 绝对路径（`RootDir`）、Windows 盘符相对前缀（`Prefix`，如 `Z:`）、
/// UNC 前缀（`\\server\share`）；另显式拒绝 `\` 与控制字符。
///
/// 返回 `Err(原因)` 供上层（如 CLI）转成各自的错误类型与退出码。
pub fn validate_asset_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("名称不能为空".to_string());
    }
    // 控制字符（含 NUL / 换行 / 制表）出现在文件名里永远不是本意，且会污染
    // 后续日志与清单文本。
    if name.chars().any(char::is_control) {
        return Err(format!("名称不能包含控制字符: {name:?}"));
    }
    if name == "." || name == ".." {
        return Err(format!("非法名称: {name}"));
    }
    // 反斜杠：Linux/macOS 上它不是路径分隔符，`Path::components()` 不会拦截，
    // 但 Windows 上是；跨平台统一显式拒绝。
    if name.contains('\\') {
        return Err(format!("名称不能包含路径分隔符: {name}"));
    }
    // 盘符相对前缀（`C:`、`Z:foo`）：Windows 上它是 `Component::Prefix`，
    // 但 Linux/macOS 上 `Path::components()` 会把整段当作普通文件名而放行，
    // 导致行为随平台漂移。跨平台统一按"ASCII 字母 + 冒号"开头识别并拒绝。
    if is_drive_relative(name) {
        return Err(format!("名称不能包含盘符前缀: {name}"));
    }
    // 关键：不仅要求"组件数为 1"，还要求该组件是 **`Normal`**。
    // 只数组件会放行 `Z:`（Windows 上恰为 1 个 `Prefix` 组件），而
    // `root.join("Z:")` 因 RHS 带前缀会**整体替换**路径 → 逃出安全根。
    let mut components = Path::new(name).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(_)), None) => Ok(()),
        _ => Err(format!("名称不能包含路径分隔符: {name}")),
    }
}

/// 识别 Windows 盘符相对前缀：首个字符为 ASCII 字母、第二个字符为 `:`
/// （如 `C:`、`z:foo`）。该形态在 Windows 上是路径 `Prefix`，在类 Unix 上
/// 只是普通文件名，因此需跨平台统一判定，而不是交给 `Path::components()`。
fn is_drive_relative(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(
        (chars.next(), chars.next()),
        (Some(drive), Some(':')) if drive.is_ascii_alphabetic()
    )
}

/// 安全根：所有相对资产名称都相对它解析，且解析结果必须落在其内。
///
/// 构造时对根做 canonicalize（消除 `..` 与符号链接），解析时对**已存在的目标**
/// 再次 canonicalize 并校验根包含，从而拒绝符号链接逃逸（沿用设计 D14 双层校验）。
#[derive(Debug, Clone)]
pub struct SafePath {
    root: PathBuf,
}

impl SafePath {
    /// 以 `root` 为安全根；`root` 必须存在**且为目录**（否则返回错误）。
    pub fn from_root(root: &Path) -> Result<Self, WriteError> {
        let canon = root.canonicalize().map_err(|e| map_io(e, root))?;
        if !canon.is_dir() {
            return Err(WriteError::Corrupt(format!(
                "安全根必须是目录: {}",
                canon.display()
            )));
        }
        Ok(SafePath { root: canon })
    }

    /// 安全根（canonicalize 后）。
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// 解析相对名称 `rel` 为绝对路径。
    ///
    /// 先做名称校验（唯一 `Normal` 组件），再对 **`candidate` 自身**做根包含校验
    /// （D14 双层）：目标已存在时用 `canonicalize` 后的真实路径（识破符号链接 /
    /// junction 逃逸）；目标不存在时归一**父目录**后拼回文件名。任一步发现逃逸 →
    /// [`WriteError::PathEscape`]。
    ///
    /// 第二层校验不可省：即便名称校验放行了某个"看起来正常"的名字，也必须在
    /// **结果路径**上再断言一次落在根内，避免 `Path::join` 的前缀替换语义等
    /// 意外绕过。
    ///
    /// # 返回的是归一后的路径（P1-6，2026-09-27 修）
    ///
    /// 此前"校验对象"与"返回对象"是**两条不同的路径**：校验 `checked`、返回
    /// `candidate`。调用方随后把 `candidate` 交给 `File::create` / `rename`，
    /// 而 `File::create` 会**跟随符号链接** —— 时间窗内目标被换成链接时，
    /// 第一次写入就写穿了根外，第二层校验形同虚设。现在两者是同一条路径。
    ///
    /// 同时把 `exists()`（跟随链接）换成 `symlink_metadata()`（不跟随）：
    /// 目标是符号链接时**直接拒绝** —— 创建资产本来就不该落到链接上。
    /// 这同时消掉了 `exists()` → `canonicalize()` 之间的 TOCTOU 窗口。
    pub fn resolve(&self, rel: &str) -> Result<PathBuf, WriteError> {
        if let Err(reason) = validate_asset_name(rel) {
            return Err(WriteError::PathEscape {
                rel: rel.to_string(),
                reason,
            });
        }
        let candidate = self.root.join(rel);
        // ① 不跟随链接地探测目标：是链接即拒绝（不给 `File::create` 跟随的机会）。
        match std::fs::symlink_metadata(&candidate) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(WriteError::PathEscape {
                    rel: rel.to_string(),
                    reason: format!(
                        "目标是指向别处的符号链接，拒绝写入: {}",
                        candidate.display()
                    ),
                });
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(map_io(e, &candidate)),
        }
        // ② 归一：存在 → 目标自身；不存在（保存前校验的常态）→ 父目录 + 文件名。
        let checked = match std::fs::canonicalize(&candidate) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let parent = candidate.parent().unwrap_or(&self.root);
                let base = std::fs::canonicalize(parent).unwrap_or_else(|_| parent.to_path_buf());
                match candidate.file_name() {
                    Some(name) => base.join(name),
                    None => base,
                }
            }
            Err(e) => return Err(map_io(e, &candidate)),
        };
        if !checked.starts_with(&self.root) {
            return Err(WriteError::PathEscape {
                rel: rel.to_string(),
                reason: format!("解析结果越出安全根: {}", checked.display()),
            });
        }
        Ok(checked)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_asset_name_accepts_plain_names() {
        for ok in ["my_op", "my.op", "钻_孔循环", "a", "a-b_c.1", "wfl_m65"] {
            assert!(validate_asset_name(ok).is_ok(), "{ok} 应合法");
        }
    }

    #[test]
    fn validate_asset_name_rejects_traversal() {
        // 跨平台一律拒绝的形态（裸盘符前缀见 validate_asset_name_rejects_drive_prefix）
        for bad in [
            "",
            ".",
            "..",
            "a/b",
            "../evil",
            "a//b",
            r"..\evil",
            "sub\\evil",
            "/abs",
            r"\\server\share",
            r"C:\Windows",
            "C:/Windows",
            "sub/../..",
            // 控制字符
            "a\u{0}b",
            "a\nb",
            "a\tb",
            "a\rb",
        ] {
            assert!(validate_asset_name(bad).is_err(), "{bad:?} 应被拒绝");
        }
    }

    /// 盘符相对前缀（`Z:`、`C:foo`）是最易被"只数组件数"放行的形态：
    /// Windows 上它是 `Prefix` 组件，类 Unix 上则是普通文件名。
    /// 现已跨平台统一拒绝，故断言不设 `#[cfg(windows)]`。
    #[test]
    fn validate_asset_name_rejects_drive_prefix() {
        for bad in ["Z:", "C:", "Q:", "c:foo", "D:bar"] {
            assert!(
                validate_asset_name(bad).is_err(),
                "{bad} 含盘符前缀，应被拒绝"
            );
        }
    }

    /// 纵深防御（跨平台）：即便名称校验放行，`resolve` 也必须保证结果落在根内。
    /// 这是 P1 缺陷的**直接**断言——不依赖名称校验是否拦住 `Z:`。
    #[test]
    fn resolve_never_returns_path_outside_root() {
        let sp = SafePath::from_root(&std::env::temp_dir()).unwrap();
        for bad in ["Z:", "C:", "Q:", "/abs", r"\\server\share", "../x", "a/b"] {
            if let Ok(p) = sp.resolve(bad) {
                assert!(
                    p.starts_with(sp.root()),
                    "{bad:?} 解析出根外路径: {}",
                    p.display()
                );
            }
        }
    }

    /// 安全根必须是**已存在的目录**；普通文件不算合法根。
    #[test]
    fn from_root_requires_directory() {
        let file = std::env::temp_dir().join(format!("nctool_root_file_{}", std::process::id()));
        std::fs::write(&file, b"x").unwrap();
        assert!(SafePath::from_root(&file).is_err(), "根是普通文件时应报错");
        let _ = std::fs::remove_file(&file);
    }

    #[test]
    fn resolve_within_root_ok() {
        let sp = SafePath::from_root(&std::env::temp_dir()).unwrap();
        let p = sp.resolve("nctool_asset_probe").unwrap();
        assert!(
            p.starts_with(sp.root()),
            "解析结果应在根内: {}",
            p.display()
        );
    }

    /// 目标已存在时走"canonicalize 目标自身"分支（`candidate.exists()` 为真）。
    #[test]
    fn resolve_existing_file_within_root_ok() {
        let root = std::env::temp_dir();
        let name = format!("nctool_asset_existing_{}", std::process::id());
        let file = root.join(&name);
        std::fs::write(&file, b"x").unwrap();

        let sp = SafePath::from_root(&root).unwrap();
        let resolved = sp.resolve(&name).unwrap();
        assert!(resolved.starts_with(sp.root()));
        assert_eq!(std::fs::read(&resolved).unwrap(), b"x");

        let _ = std::fs::remove_file(&file);
    }

    #[test]
    fn resolve_rejects_escape_names() {
        let sp = SafePath::from_root(&std::env::temp_dir()).unwrap();
        for bad in ["../x", "a/b", ".", ""] {
            assert!(
                matches!(sp.resolve(bad), Err(WriteError::PathEscape { .. })),
                "{bad} 应判为路径越界"
            );
        }
    }

    #[test]
    fn from_root_missing_errors() {
        let missing = std::env::temp_dir().join("nctool_missing_root_xyz_9f3a");
        assert!(SafePath::from_root(&missing).is_err(), "根不存在应报错");
    }
}
