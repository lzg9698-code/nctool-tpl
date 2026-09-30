//! 模板写操作策略：`TemplateWriter`（create / save / derive / rename）
//! 与 `templates.yaml` 的**定点文本编辑**。
//!
//! # 为什么是"定点文本编辑"而不是 serde 往返
//!
//! `templates/templates.yaml` 的头部有 24 行**语义注释**，条目内还散布行内注释
//! （如 `status: unreviewed # 迁移自 NCTool_V3`）。`serde_yaml` 往返会把这些注释
//! **全部删除**——把用户的文档变成噪声，且不可逆。因此本模块对清单只做两种
//! **行级、可控**的编辑：
//!
//! 1. [`manifest_append_entry`]：在 `templates:` 映射末尾追加一条顶层条目
//!    （`new` / `derive` 用）；
//! 2. [`manifest_rewrite_key`]：只替换某条已存在条目的**键行文本**（`rename` 用）。
//!
//! 两条操作都保证**除目标处外其余字节完全不变**（有测试断言）。
//!
//! # 降级哲学（沿用 D13）
//!
//! 清单是**可选**文件，且模板名 = 相对路径，**即使不写清单条目，模板也能被目录
//! 扫描发现**。因此清单缺失 / 解析失败 / 无 `templates:` 块时，一律
//! **降级为警告 + 提示手补**（[`ManifestOutcome::Degraded`]），**不阻断**模板
//! 文件本身的写入。
//!
//! # 写路径
//!
//! 所有变更都经 [`WriteKernel`]（原子写 + 乐观锁）；`rename` 的旧文件删除也
//! 持有同路径互斥锁并核对源指纹。

use std::path::{Path, PathBuf};

use crate::manifest::MANIFEST_FILE;

use super::{FileFingerprint, SafePath, WriteError, WriteKernel, WriteOutcome};

/// `templates.yaml` 的顶层映射键（`templates:`，位于第 0 列）。
const MANIFEST_TOP_KEY: &str = "templates:";
/// 清单条目**键行**的缩进（2 空格，与既有文件一致）。
///
/// 字段行缩进是"相对键行"的（见 [`manifest_append_entry`]），故此处只需这一个常量。
const ENTRY_INDENT: &str = "  ";

// ---------------------------------------------------------------------------
// 请求 / 报告类型
// ---------------------------------------------------------------------------

/// 清单处理结果。
///
/// 与设计 §3.2 的 `RenameReport.manifest: WriteOutcome` 相比，这里用枚举表达
/// "未改动清单"的两种成因（无条目 / 降级），比 `Option<WriteOutcome>` 更能传达
/// 降级原因，故为**有意偏差**。
#[derive(Debug, Clone)]
pub enum ManifestOutcome {
    /// 清单已成功写入（附写结果）。
    Written(WriteOutcome),
    /// 清单中不存在该模板的条目 → 未改动清单（不凭空造条目）。
    NoEntry,
    /// 清单缺失 / 解析失败 / 无 `templates:` 块 / 写入失败 → 降级，附原因。
    ///
    /// 携带的原因供调用方提示用户"手动补录清单"。
    Degraded(String),
}

/// `derive` 的结果：新模板文件 + 清单处理结果。
#[derive(Debug, Clone)]
pub struct DeriveReport {
    /// 新模板文件的写结果。
    pub file: WriteOutcome,
    /// 清单条目复制结果。
    pub manifest: ManifestOutcome,
}

/// `rename` 的结果：新文件 + 清单处理结果。
///
/// 待修复的 `include` 引用清单**不在本结构体内**：扫描需要注册表（属于交付层），
/// 由 CLI 调用 [`scan_stale_includes`] 单独计算。这是对设计 §3.2
/// `RenameReport.stale_includes` 的**有意偏差**——写内核不依赖注册表。
#[derive(Debug, Clone)]
pub struct RenameReport {
    /// 重命名后新文件的写结果。
    pub file: WriteOutcome,
    /// 清单键改写结果。
    pub manifest: ManifestOutcome,
}

// ---------------------------------------------------------------------------
// 模板写操作
// ---------------------------------------------------------------------------

/// 模板写操作（新建 / 保存 / 派生 / 重命名）的唯一入口。
///
/// 无状态：所有方法均为关联函数，内部一律经 [`WriteKernel`] 落盘。
pub struct TemplateWriter;

impl TemplateWriter {
    /// 在 `dir`（安全根）内新建 `rel_key`（`/` 分隔的相对键）模板文件。
    ///
    /// 乐观锁 `expect = None` 要求写前文件**不存在**；已存在 → [`WriteError::Conflict`]，
    /// 调用方映射为 `template_duplicate`(6)。
    ///
    /// 重名原子性（P0-1）：`write_guarded` 在**跨进程互斥锁内**做
    /// `read_snapshot` 比对 —— 并发同名 `new` 被锁串行化，后到者读到已存在
    /// 的文件 → `Conflict`，**不会**双双通过、更不会静默覆盖。初稿建议的
    /// `OpenOptions::create_new`（`O_EXCL`）已撤回：持锁后冗余，且 O_EXCL
    /// 直接写目标文件、崩在半路会留下残缺文件（架构 §3.1 定稿说明）。
    ///
    /// 相比旧的 `path.exists()` + `std::fs::write`，本函数的改善是：**写原子性**
    /// （tmp + rename，任何时刻要么旧内容要么新内容，无半成品）、**不跟随符号
    /// 链接**（`rename` 替换目录项本身；悬空链接向量下写不会落到根外），以及
    /// **重名检查的原子性**（锁内比对，见上）。
    pub fn create(dir: &Path, rel_key: &str, source: &str) -> Result<WriteOutcome, WriteError> {
        let sp = SafePath::from_root(dir)?;
        let path = resolve_rel(&sp, rel_key)?;
        WriteKernel::write_guarded(&path, source.as_bytes(), None)
    }

    /// 新建模板后，将名称、分类和待复核状态追加到模板清单。
    ///
    /// 清单属于可选元数据：模板文件创建成功后，清单写入失败返回
    /// [`ManifestOutcome::Degraded`]，由适配层把部分成功状态明确告知用户。
    pub fn append_manifest_entry(
        root: &Path,
        rel_key: &str,
        display_name: &str,
        category: &str,
    ) -> ManifestOutcome {
        let path = root.join(MANIFEST_FILE);
        let expected = match WriteKernel::read_fingerprint(&path) {
            Ok(Some(fp)) => fp,
            Ok(None) => {
                return ManifestOutcome::Degraded(format!(
                    "清单文件不存在（{}），未写入分类元数据，请手动补录",
                    path.display()
                ))
            }
            Err(e) => return ManifestOutcome::Degraded(format!("清单读取失败：{e}")),
        };
        let text = match crate::io_limit::read_text_capped(&path, crate::io_limit::MAX_SOURCE_BYTES)
        {
            Ok(t) => t,
            Err(e) => return ManifestOutcome::Degraded(format!("清单读取失败：{e}")),
        };
        if !manifest_is_parseable(&text) {
            return ManifestOutcome::Degraded(format!(
                "清单解析失败（{}），未改动清单；请手动补录",
                path.display()
            ));
        }
        let body = vec![
            format!(
                "  name: {}",
                serde_yaml::to_string(display_name)
                    .unwrap_or_default()
                    .trim_end()
            ),
            format!("  category: {category}"),
            "  status: unreviewed".to_string(),
        ];
        let Some(updated) = manifest_append_entry(&text, rel_key, &body) else {
            return ManifestOutcome::Degraded(
                "清单缺少顶层 `templates:` 块，未写入分类元数据".to_string(),
            );
        };
        write_manifest(&path, &updated, Some(expected))
    }

    /// 保存已有模板：`root/<rel_key>`，乐观锁 `expect` 为打开时的快照指纹。
    ///
    /// `rel_key` 用 `/` 分隔，可含子目录（如 `turning/demo_gcode.j2`）；
    /// 每一段都经 [`SafePath`] 校验，任何一段逃逸即 [`WriteError::PathEscape`]。
    pub fn save(
        root: &Path,
        rel_key: &str,
        new_source: &str,
        expect: Option<FileFingerprint>,
    ) -> Result<WriteOutcome, WriteError> {
        let sp = SafePath::from_root(root)?;
        let path = resolve_rel(&sp, rel_key)?;
        WriteKernel::write_guarded(&path, new_source.as_bytes(), expect)
    }

    /// 派生：写新模板文件（要求不存在），并**复制源模板在清单中的条目**
    /// （含 `params` 覆盖层）到新键下。
    ///
    /// 源模板在清单中**没有条目**时，只写文件不动清单（[`ManifestOutcome::NoEntry`]），
    /// 不凭空造条目。派生**不自动同步**源模板后续变更（避免隐式耦合）。
    pub fn derive(
        root: &Path,
        src_rel: &str,
        dst_rel: &str,
        new_source: &str,
    ) -> Result<DeriveReport, WriteError> {
        let sp = SafePath::from_root(root)?;
        let dst_path = resolve_rel(&sp, dst_rel)?;
        let file = WriteKernel::write_guarded(&dst_path, new_source.as_bytes(), None)?;
        let manifest = clone_manifest_entry(&sp, src_rel, dst_rel);
        Ok(DeriveReport { file, manifest })
    }

    /// 重命名：读旧文件 → 写新文件（要求不存在）→ 删旧文件 → 改写清单键。
    ///
    /// 先写新、后按源指纹删除旧：若另一个编辑器同时改写源文件，删除会冲突，
    /// 最坏留下两份，不会丢弃任一方的内容。
    pub fn rename(root: &Path, old_rel: &str, new_rel: &str) -> Result<RenameReport, WriteError> {
        let sp = SafePath::from_root(root)?;
        let old_path = resolve_rel(&sp, old_rel)?;
        let new_path = resolve_rel(&sp, new_rel)?;
        let (bytes, source_fingerprint) = super::guard::read_snapshot(&old_path)?
            .ok_or_else(|| WriteError::NotFound(format!("模板不存在：{}", old_path.display())))?;
        // 新文件必须不存在（expect = None）：重名即 Conflict → name_conflict(6)
        let file = WriteKernel::write_guarded(&new_path, &bytes, None)?;
        WriteKernel::remove_guarded(&old_path, Some(source_fingerprint))?;
        let manifest = rewrite_manifest_key(&sp, old_rel, new_rel);
        Ok(RenameReport { file, manifest })
    }
}

/// 解析 `/` 分隔的相对键为绝对路径（每一段都过 [`SafePath`] 校验）。
///
/// [`SafePath::resolve`] 只接受**单组件**名称（[`super::validate_asset_name`]
/// 拒绝 `/`），而模板键是"相对模板根的路径"（可含子目录）。本函数把键拆成
/// 组件、**逐段**用 [`SafePath`] 解析：中间目录用 `from_root` canonicalize 后
/// 作为下一段的根，因此符号链接逃逸在每一层都被识破，最后一段沿用
/// [`SafePath::resolve`] 的"存在则 canonicalize、不存在则拼接并断言根包含"。
///
/// 中间目录必须**已存在**（编辑/重命名既有模板时恒成立）；不存在的中间目录
/// 会由 `from_root` 报错，不会静默放行。
fn resolve_rel(root: &SafePath, rel: &str) -> Result<PathBuf, WriteError> {
    // `str::split` **至少产生一个元素**（空串 → `[""]`），故 `pop()` 必有值；
    // `rel=""` 时最后一段是空串，由 `resolve` 的名称校验拒绝（不静默放行）。
    let mut parts: Vec<&str> = rel.split('/').collect();
    let last = parts.pop().unwrap_or("");
    let mut cur = root.clone();
    for d in parts {
        let p = cur.resolve(d)?;
        cur = SafePath::from_root(&p)?;
    }
    cur.resolve(last)
}

// ---------------------------------------------------------------------------
// 清单：复制条目（derive）
// ---------------------------------------------------------------------------

/// 读取 `root/templates.yaml`，把源条目的字段块复制到新键下。
///
/// 清单缺失 / 解析失败 / 无 `templates:` 块 / 写入失败 → 降级；
/// 源模板无条目 → [`ManifestOutcome::NoEntry`]。
fn clone_manifest_entry(sp: &SafePath, src_rel: &str, dst_rel: &str) -> ManifestOutcome {
    let path = sp.root().join(MANIFEST_FILE);
    let expected = match WriteKernel::read_fingerprint(&path) {
        Ok(Some(fp)) => fp,
        Ok(None) => {
            return ManifestOutcome::Degraded(format!(
                "清单文件不存在（{}），未复制条目，请手动补录",
                path.display()
            ))
        }
        Err(e) => return ManifestOutcome::Degraded(format!("清单读取失败：{e}")),
    };
    let text = match crate::io_limit::read_text_capped(&path, crate::io_limit::MAX_SOURCE_BYTES) {
        Ok(t) => t,
        Err(e) => return ManifestOutcome::Degraded(format!("清单读取失败：{e}")),
    };
    // 改写前先试解析：损坏清单**不得静默改写**（一个字节都不改），降级为警告。
    if !manifest_is_parseable(&text) {
        return ManifestOutcome::Degraded(format!(
            "清单解析失败（{}），未复制条目，且未改动清单；请手动补录",
            path.display()
        ));
    }
    let Some(body) = manifest_entry_body(&text, src_rel) else {
        return ManifestOutcome::NoEntry;
    };
    let Some(new_text) = manifest_append_entry(&text, dst_rel, &body) else {
        return ManifestOutcome::Degraded("清单缺少顶层 `templates:` 块，未复制条目".to_string());
    };
    write_manifest(&path, &new_text, Some(expected))
}

/// 读取 `root/templates.yaml`，把旧键行改写为新键。
fn rewrite_manifest_key(sp: &SafePath, old_rel: &str, new_rel: &str) -> ManifestOutcome {
    let path = sp.root().join(MANIFEST_FILE);
    let expected = match WriteKernel::read_fingerprint(&path) {
        Ok(Some(fp)) => fp,
        Ok(None) => {
            return ManifestOutcome::Degraded(format!(
                "清单文件不存在（{}），未同步清单键，请手动改名",
                path.display()
            ))
        }
        Err(e) => return ManifestOutcome::Degraded(format!("清单读取失败：{e}")),
    };
    let text = match crate::io_limit::read_text_capped(&path, crate::io_limit::MAX_SOURCE_BYTES) {
        Ok(t) => t,
        Err(e) => return ManifestOutcome::Degraded(format!("清单读取失败：{e}")),
    };
    // 改写前先试解析：损坏清单**不得静默改写**（一个字节都不改），降级为警告。
    if !manifest_is_parseable(&text) {
        return ManifestOutcome::Degraded(format!(
            "清单解析失败（{}），未同步清单键，且未改动清单；请手动改名",
            path.display()
        ));
    }
    match manifest_rewrite_key(&text, old_rel, new_rel) {
        Some(new_text) => write_manifest(&path, &new_text, Some(expected)),
        None => ManifestOutcome::NoEntry,
    }
}

/// 经 [`WriteKernel`] 落盘清单文本；失败降级（不阻断调用方）。
fn write_manifest(path: &Path, text: &str, expect: Option<FileFingerprint>) -> ManifestOutcome {
    match WriteKernel::write_guarded(path, text.as_bytes(), expect) {
        Ok(out) => ManifestOutcome::Written(out),
        Err(e) => ManifestOutcome::Degraded(format!("清单写入失败（模板文件已写入）：{e}")),
    }
}

// ---------------------------------------------------------------------------
// 清单定点文本编辑（纯函数，可单测）
// ---------------------------------------------------------------------------

/// 追加一条顶层条目到 `templates:` 映射末尾，保留其余字节不变。
///
/// `body` 为**相对键行**的字段行（**不含键行**，缩进相对键行计算，如
/// `"  name: ..."`、`"    - name: X"`）；追加时统一加 `ENTRY_INDENT` 前缀。
/// 键行加 `ENTRY_INDENT` 缩进并加引号。
///
/// 找不到顶层 `templates:` 行时返回 `None`（调用方据此降级）。
pub fn manifest_append_entry(text: &str, key: &str, body: &[String]) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    let sep = line_sep(text);
    // 顶层 `templates:`（第 0 列、无缩进）
    let top = lines
        .iter()
        .position(|l| !l.starts_with([' ', '\t']) && l.trim_end() == MANIFEST_TOP_KEY)?;
    // 映射末尾：最后一个"非空且带缩进"的行；映射为空时落在 `templates:` 行本身
    let mut insert_at = top;
    for (i, l) in lines.iter().enumerate().skip(top + 1) {
        if !l.is_empty() && l.starts_with([' ', '\t']) {
            insert_at = i;
        } else if !l.is_empty() {
            break; // 顶层的下一个键，映射结束
        }
    }

    let mut out: Vec<String> = lines[..=insert_at].iter().map(|s| s.to_string()).collect();
    out.push(String::new()); // 与上一条目空一行分隔
    out.push(format!("{ENTRY_INDENT}{}:", yaml_quote(key)));
    for b in body {
        out.push(format!("{ENTRY_INDENT}{b}"));
    }
    out.extend(lines[insert_at + 1..].iter().map(|s| s.to_string()));

    let mut result = out.join(sep);
    if text.ends_with('\n') {
        result.push_str(sep);
    }
    Some(result)
}

/// 提取某条目**键行之后**的字段行，缩进**相对键行**归一（用于 `derive` 复制条目）。
///
/// 归一规则：把源键行的缩进从每个字段行前缀中减去，因此返回的行可直接交给
/// [`manifest_append_entry`]（后者会再加 `ENTRY_INDENT`）。这保留了 `params:`
/// 等嵌套结构的**相对缩进**，避免复制后 YAML 层级被压平。
///
/// **停止条件**：遇到空行、或**缩进不严格大于键行**的行（即兄弟条目键行 /
/// 上级键 / 顶格行）即停止。后者是关键——若只在空行 / 顶格行停，会把**相邻
/// 兄弟条目**当成源条目的字段吞并，使 `derive` 复制出**重复键**、整份清单被
/// `serde_yaml` 判 `duplicate entry` 而**静默失效**。条目不存在返回 `None`。
pub fn manifest_entry_body(text: &str, key: &str) -> Option<Vec<String>> {
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.iter().position(|l| key_line_matches(l, key))?;
    let key_indent = indent_of(lines[start]);
    let mut body = Vec::new();
    for l in lines.iter().skip(start + 1) {
        // 空行 = 条目结束；缩进不严格大于键行 = 兄弟条目键行 / 上级键 / 顶格行
        // → 边界，停止收集（不得把相邻兄弟条目当字段吞并）。
        if l.is_empty() || indent_of(l) <= key_indent {
            break;
        }
        // 此处必然 `indent_of(l) > key_indent`，故首 `key_indent` 字节是 ASCII
        // 空白、切片落在合法 char 边界上；`get` 兜底（畸形缩进不 panic，P1-3）。
        let Some(rest) = l.get(key_indent..) else {
            break;
        };
        body.push(rest.to_string());
    }
    Some(body)
}

/// 行首 **ASCII 空白**（空格 / 制表符）的**字节数**。
///
/// 只认 ASCII：若用 `trim_start()`（按 `char::is_whitespace` 裁剪，含 U+00A0 NBSP、
/// U+2028 等**多字节空白**），键行与字段行的空白前缀构成不同会让
/// `l[key_indent..]` 的下标落进多字节字符**内部** → 字节切片 panic。该 API 是
/// **pub**（经 `lib.rs` 再导出），库内 panic 调用方无从 catch（P1-3 / Q-?）。
fn indent_of(line: &str) -> usize {
    line.bytes()
        .take_while(|b| *b == b' ' || *b == b'\t')
        .count()
}

/// 改写某条已存在条目的键行文本，保留其余字节不变。
///
/// 找不到该条目时返回 `None`（调用方据此提示"清单无此条目"）。
pub fn manifest_rewrite_key(text: &str, old_key: &str, new_key: &str) -> Option<String> {
    let sep = line_sep(text);
    let mut out: Vec<String> = Vec::new();
    let mut replaced = false;
    for line in text.lines() {
        if !replaced && key_line_matches(line, old_key) {
            let indent = &line[..line.len() - line.trim_start().len()];
            out.push(format!("{indent}{}:", yaml_quote(new_key)));
            replaced = true;
        } else {
            out.push(line.to_string());
        }
    }
    if !replaced {
        return None;
    }
    let mut result = out.join(sep);
    if text.ends_with('\n') {
        result.push_str(sep);
    }
    Some(result)
}

/// 清单中是否存在某条目的键行。
pub fn manifest_contains_key(text: &str, key: &str) -> bool {
    text.lines().any(|l| key_line_matches(l, key))
}

/// 清单文本是否为**可解析的 YAML**。
///
/// 定点文本编辑是纯文本操作、从不解析整份清单；若不先试解析，**损坏清单会被
/// 静默改写**（追加 / 改名后仍是坏的），而文档承诺的是"清单解析失败 → 降级为
/// 警告 + 提示手补"。所有清单改写前都必须先过这一关：失败即降级，且**一个
/// 字节都不改**。
pub fn manifest_is_parseable(text: &str) -> bool {
    serde_yaml::from_str::<serde_yaml::Value>(text).is_ok()
}

/// 某行是否为条目 `key` 的键行（带缩进、形如 `"key":` 或 `key:`）。
fn key_line_matches(line: &str, key: &str) -> bool {
    if line.trim_start().len() == line.len() {
        return false; // 顶格行不是条目键行
    }
    let t = line.trim();
    t == format!("{}:", yaml_quote(key)) || t == format!("{key}:")
}

/// YAML 双引号标量。资产名允许引号字符（例如 Unix 文件名），必须转义后才能
/// 安全用作清单键；控制字符已由 `validate_asset_name` 拦截。
fn yaml_quote(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

/// 文本使用的换行分隔符（保留 CRLF 以免往返改字节）。
fn line_sep(text: &str) -> &'static str {
    if text.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

// ---------------------------------------------------------------------------
// 派生 / 重命名的辅助纯函数
// ---------------------------------------------------------------------------

/// 由源键与新名构造**同目录**的新键（自动补 `.j2` 扩展名）。
///
/// 例：`sibling_rel_key("turning/a.j2", "b") == "turning/b.j2"`。
pub fn sibling_rel_key(src_key: &str, new_name: &str) -> String {
    let fname = if new_name.to_lowercase().ends_with(".j2") {
        new_name.to_string()
    } else {
        format!("{new_name}.j2")
    };
    match src_key.rsplit_once('/') {
        Some((dir, _)) => format!("{dir}/{fname}"),
        None => fname,
    }
}

/// 构造派生模板源码：更新头部 `{# NAME #}`，并（默认）写入一行"派生自 X"注释。
///
/// - 头部前 10 行内存在 `{# NAME: ... #}` → 整行替换为新名；
/// - 不存在 → 在文件最前补一行 `{# NAME: <new> #}`；
/// - `note = true` 时在最前写 `{# 派生自 <src_key> #}`（G2 可追溯）。
///
/// 派生**不自动同步**源模板后续变更——这是有意的语义（避免隐式耦合）。
pub fn build_derived_source(src: &str, new_name: &str, src_key: &str, note: bool) -> String {
    let ends_nl = src.ends_with('\n');
    let mut lines: Vec<String> = src.lines().map(|s| s.to_string()).collect();
    let mut replaced = false;
    for line in lines.iter_mut().take(10) {
        if line.contains("{# NAME:") {
            *line = format!("{{# NAME: {new_name} #}}");
            replaced = true;
            break;
        }
    }
    let mut out = String::new();
    if note {
        out.push_str(&format!("{{# 派生自 {src_key} #}}\n"));
    }
    if !replaced {
        out.push_str(&format!("{{# NAME: {new_name} #}}\n"));
    }
    out.push_str(&lines.join("\n"));
    if ends_nl && !lines.is_empty() {
        out.push('\n');
    }
    out
}

/// 扫描引用了旧模板键（或其文件名）的模板，返回待修复的模板键清单。
///
/// **只报告、不自动改写**——`{% include %}` 是显式全路径，自动改易误伤
/// （设计 §9 Q10 已确认）。`templates` 为 `(模板键, 源码)` 列表，按输入顺序
/// 去重返回命中的键。
pub fn scan_stale_includes(
    templates: &[(String, String)],
    old_key: &str,
    old_name: &str,
) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (key, src) in templates {
        let Ok(ast) = nctool_tpl::parse(src, key) else {
            continue; // 语法坏掉的模板不参与引用扫描（自身另有校验负责）
        };
        let refs = nctool_tpl::extract_template_refs(&ast);
        if refs.iter().any(|r| r == old_key || r == old_name) && !out.iter().any(|k| k == key) {
            out.push(key.clone());
        }
    }
    out
}

/// 取路径键的最后一段（文件名）。
pub fn last_component(key: &str) -> &str {
    key.rsplit('/').next().unwrap_or(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- 清单定点文本编辑 ----

    const SAMPLE: &str = "# 头部注释\n# 第二行\ntemplates:\n  \"a/one.j2\":\n    name: \"一\"\n    params:\n      - name: X\n        kind: number\n\n  \"b/two.j2\":\n    name: \"二\"\n";

    #[test]
    fn append_entry_preserves_every_existing_line() {
        let body = vec![
            "  name: \"三\"".to_string(),
            "  status: unreviewed".to_string(),
        ];
        let out = manifest_append_entry(SAMPLE, "c/three.j2", &body).expect("应能追加");
        // 非目标行集合不变（原文件每一行仍逐字存在，且顺序不变）
        for line in SAMPLE.lines() {
            assert!(out.contains(line), "原有行丢失: {line:?}\n---\n{out}");
        }
        assert!(out.contains("  \"c/three.j2\":"));
        assert!(out.contains("    name: \"三\""));
        // 原文件（SAMPLE）不被改动：编辑只发生在返回的新串上
        assert!(SAMPLE.contains("# 头部注释"));
        assert!(out.starts_with("# 头部注释"));
        assert!(out.ends_with('\n'));
    }

    #[test]
    fn append_entry_without_top_key_returns_none() {
        assert!(manifest_append_entry("# 只有注释\n", "x.j2", &[]).is_none());
    }

    #[test]
    fn manifest_keys_with_quotes_remain_valid_yaml() {
        let old = "turning/a\"b.j2";
        let new = "turning/c\"d.j2";
        let appended = manifest_append_entry("templates:\n", old, &[]).unwrap();
        assert!(
            manifest_is_parseable(&appended),
            "追加键必须保持 YAML 合法：{appended}"
        );
        let renamed = manifest_rewrite_key(&appended, old, new).unwrap();
        assert!(
            manifest_is_parseable(&renamed),
            "重命名键必须保持 YAML 合法：{renamed}"
        );
        assert!(manifest_contains_key(&renamed, new));
        assert!(!manifest_contains_key(&renamed, old));
    }

    /// `templates:` 映射之后还有**其它顶层键**时，新条目必须插在映射末尾、
    /// 不得越过该顶层键（否则会污染后续顶层块）。
    #[test]
    fn append_entry_stops_before_next_top_level_key() {
        let text = "templates:\n  \"a.j2\":\n    name: \"A\"\n\nvariables:\n  foo: 1\n";
        let out = manifest_append_entry(text, "b.j2", &["  name: \"B\"".to_string()]).unwrap();
        let new_pos = out.find("\"b.j2\":").expect("应有新条目");
        let var_pos = out.find("variables:").expect("应保留 variables 块");
        assert!(new_pos < var_pos, "新条目必须插在 variables 之前:\n{out}");
        // 顶层块原样保留
        assert!(out.contains("variables:\n  foo: 1"));
    }

    #[test]
    fn append_entry_on_empty_mapping() {
        let text = "templates:\n";
        let out = manifest_append_entry(text, "a.j2", &["  name: \"A\"".to_string()]).unwrap();
        assert!(out.contains("  \"a.j2\":"));
        assert!(out.contains("    name: \"A\""));
    }

    #[test]
    fn rewrite_key_changes_only_target_line() {
        let out = manifest_rewrite_key(SAMPLE, "a/one.j2", "a/renamed.j2").expect("应能改写");
        let before: Vec<&str> = SAMPLE.lines().collect();
        let after: Vec<&str> = out.lines().collect();
        assert_eq!(before.len(), after.len(), "行数不变");
        let mut diffs = 0;
        for (b, a) in before.iter().zip(after.iter()) {
            if b != a {
                diffs += 1;
                assert_eq!(*b, "  \"a/one.j2\":");
                assert_eq!(*a, "  \"a/renamed.j2\":");
            }
        }
        assert_eq!(diffs, 1, "只允许键行一处变化");
        // 其它条目原样保留
        assert!(out.contains("  \"b/two.j2\":"));
    }

    #[test]
    fn rewrite_missing_key_returns_none() {
        assert!(manifest_rewrite_key(SAMPLE, "nope.j2", "x.j2").is_none());
    }

    #[test]
    fn entry_body_captures_indented_fields_relative_to_key() {
        let body = manifest_entry_body(SAMPLE, "a/one.j2").expect("应找到条目");
        assert_eq!(
            body,
            vec![
                "  name: \"一\"".to_string(),
                "  params:".to_string(),
                "    - name: X".to_string(),
                "      kind: number".to_string(),
            ]
        );
    }

    /// 复制条目必须保留 `params:` 等嵌套结构的**相对缩进**，否则 YAML 层级被压平、
    /// 参数覆盖层静默失效（`options` / `required_if` 全都不生效）。
    #[test]
    fn clone_entry_preserves_nested_indentation() {
        let text = "templates:\n  \"a.j2\":\n    name: \"A\"\n    params:\n      - name: X\n        kind: choice\n        options: [\"P\", \"Q\"]\n";
        let body = manifest_entry_body(text, "a.j2").unwrap();
        let out = manifest_append_entry(text, "b.j2", &body).unwrap();
        let parsed: serde_yaml::Value = serde_yaml::from_str(&out).expect("应是合法 YAML");
        let b = &parsed["templates"]["b.j2"];
        assert_eq!(b["name"], serde_yaml::Value::String("A".into()));
        let params = b["params"].as_sequence().expect("params 应是序列");
        assert_eq!(params.len(), 1, "params 只应有一项: {out}");
        assert_eq!(params[0]["name"], serde_yaml::Value::String("X".into()));
        assert_eq!(
            params[0]["kind"],
            serde_yaml::Value::String("choice".into())
        );
        assert!(
            params[0]["options"].is_sequence(),
            "嵌套 options 不得被压平: {out}"
        );
    }

    #[test]
    fn entry_body_absent_returns_none() {
        assert!(manifest_entry_body(SAMPLE, "missing.j2").is_none());
    }

    /// 字段行缩进**不严格大于**键行 → 视为**兄弟条目边界**，停止收集。
    ///
    /// （旧断言曾把"更浅缩进也吞"当作预期行为——等于把 P1 缺陷固化成规格；
    /// 该错误断言已随 P1 修复一并纠正为正确语义。）
    #[test]
    fn entry_body_stops_when_indent_not_deeper_than_key() {
        let text = "templates:\n    \"a.j2\":\n  name: \"A\"\n";
        let body = manifest_entry_body(text, "a.j2").unwrap();
        assert!(body.is_empty(), "更浅缩进不得被当作字段吞入: {body:?}");
    }

    /// **P1-3 回归**：多字节空白（NBSP）缩进**不得 panic**。
    ///
    /// 旧 `indent_of` 用 `trim_start()`（`char::is_whitespace` 裁剪，NBSP 计入），
    /// 与键行的 ASCII 缩进混合后 `l[key_indent..]` 的下标落进 NBSP 字节**内部**
    /// → 字节切片 panic（pub API 库内 panic，调用方无从 catch）。
    /// 现只认 ASCII 空白：NBSP 行缩进计 0 → 不满足"严格大于键行" → 停止收集。
    #[test]
    fn entry_body_multibyte_indent_does_not_panic() {
        // 键行缩进 2 空格（key_indent = 2）；字段行 = [空格][NBSP]…：
        // 旧 indent_of > 2 且下标 2 落在 NBSP 内部 → panic。
        let text = "templates:\n  \"a.j2\":\n \u{a0}name: \"A\"\n    path: x.j2\n";
        let body = manifest_entry_body(text, "a.j2").expect("键行应找到");
        assert!(
            body.is_empty(),
            "NBSP 前缀行不满足 ASCII 缩进比较: {body:?}"
        );

        // 键行本身用 NBSP 缩进（旧 indent_of = 2 字节）也不得 panic；
        // 字段行的 ASCII 部分照常收集（缩进比较按 ASCII 定义）。
        let text = "\u{a0}\"b.j2\":\n  name: \"B\"\n";
        let body = manifest_entry_body(text, "b.j2").expect("键行应找到");
        assert_eq!(body, vec!["  name: \"B\"".to_string()]);
    }

    /// **P1 回归**：相邻兄弟条目（同缩进、无空行分隔）不得被吞并。
    ///
    /// 旧实现在同级键行不停，把 `"b.j2"` 当字段复制 → `derive` 产出重复键 →
    /// `serde_yaml` 判 `duplicate entry` → 整份清单静默失效。
    #[test]
    fn entry_body_stops_at_adjacent_sibling_entry() {
        let text = "templates:\n  \"a.j2\":\n    name: \"A\"\n  \"b.j2\":\n    name: \"B\"\n";
        let body = manifest_entry_body(text, "a.j2").unwrap();
        assert_eq!(body, vec!["  name: \"A\"".to_string()]);

        // 复制后清单仍须是合法 YAML 且无重复键。
        let out = manifest_append_entry(text, "anew.j2", &body).unwrap();
        let parsed: serde_yaml::Value = serde_yaml::from_str(&out).expect("应是合法 YAML");
        let tpls = parsed["templates"]
            .as_mapping()
            .expect("templates 应是映射");
        assert_eq!(tpls.len(), 3, "应恰好 3 条条目（a/b/anew）: {out}");
        assert_eq!(out.matches("\"b.j2\":").count(), 1, "b.j2 不得重复: {out}");
    }

    /// P2-1：损坏清单必须被识别为**解析失败**（供改写前降级判定）。
    #[test]
    fn manifest_is_parseable_detects_corrupt_text() {
        assert!(manifest_is_parseable(
            "templates:\n  \"a.j2\":\n    name: \"A\"\n"
        ));
        // 重复键（正是 P1 造成的失效形态）应判为不可解析。
        assert!(
            !manifest_is_parseable(
                "templates:\n  \"a.j2\":\n    name: \"A\"\n  \"a.j2\":\n    name: \"B\"\n"
            ),
            "重复键应判为解析失败"
        );
    }

    #[test]
    fn contains_key_detects_present_and_absent() {
        assert!(manifest_contains_key(SAMPLE, "b/two.j2"));
        assert!(!manifest_contains_key(SAMPLE, "c/three.j2"));
    }

    #[test]
    fn crlf_is_preserved() {
        let text = "templates:\r\n  \"a.j2\":\r\n    name: \"A\"\r\n";
        let out = manifest_rewrite_key(text, "a.j2", "b.j2").unwrap();
        assert!(out.contains("\r\n"), "CRLF 必须保留: {out:?}");
        assert!(out.contains("  \"b.j2\":"));
    }

    // 规格自洽校验（L2）的实现与测试**已迁至** `crate::validate::check_spec_consistency`
    // ——那里复用唯一的 `check_spec_defaults`，避免第二份 default 判定（P4 单一来源）。

    // ---- 派生 / 重命名辅助 ----

    #[test]
    fn sibling_key_keeps_directory_and_extension() {
        assert_eq!(sibling_rel_key("turning/a.j2", "b"), "turning/b.j2");
        assert_eq!(sibling_rel_key("a.j2", "b"), "b.j2");
        assert_eq!(sibling_rel_key("turning/a.j2", "b.j2"), "turning/b.j2");
    }

    #[test]
    fn derived_source_updates_name_and_adds_note() {
        let src = "{# NAME: 旧名 #}\n{# DESCRIPTION: d #}\nG0 X0\n";
        let out = build_derived_source(src, "新名", "turning/old.j2", true);
        assert!(out.contains("{# 派生自 turning/old.j2 #}"));
        assert!(out.contains("{# NAME: 新名 #}"));
        assert!(!out.contains("旧名"));
        assert!(out.ends_with('\n'));
    }

    #[test]
    fn derived_source_without_header_inserts_name() {
        let out = build_derived_source("G0 X0\n", "新名", "a.j2", false);
        assert!(out.starts_with("{# NAME: 新名 #}\n"));
        assert!(!out.contains("派生自"));
    }

    #[test]
    fn scan_stale_includes_reports_only_referrers() {
        let tpls = vec![
            (
                "a.j2".to_string(),
                "{% include \"turning/old.j2\" %}".to_string(),
            ),
            ("b.j2".to_string(), "{% include \"other.j2\" %}".to_string()),
            ("c.j2".to_string(), "{% include \"old.j2\" %}".to_string()),
        ];
        let stale = scan_stale_includes(&tpls, "turning/old.j2", "old.j2");
        assert_eq!(stale, vec!["a.j2".to_string(), "c.j2".to_string()]);
    }

    /// 语法坏掉的模板**不参与**引用扫描（其语法问题由写前 L1 校验负责），
    /// 不得因解析失败而中断整轮扫描或误报。
    #[test]
    fn scan_stale_includes_skips_unparseable_source() {
        let tpls = vec![
            ("bad.j2".to_string(), "{% if %}".to_string()),
            (
                "good.j2".to_string(),
                "{% include \"old.j2\" %}".to_string(),
            ),
        ];
        let stale = scan_stale_includes(&tpls, "turning/old.j2", "old.j2");
        assert_eq!(stale, vec!["good.j2".to_string()]);
    }

    #[test]
    fn last_component_returns_file_name() {
        assert_eq!(last_component("turning/a.j2"), "a.j2");
        assert_eq!(last_component("a.j2"), "a.j2");
    }

    // ---- 写路径（真实文件系统） ----

    fn temp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("nctool_tpl_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("turning")).unwrap();
        dir
    }

    #[test]
    fn create_writes_atomically_and_rejects_duplicate() {
        let root = temp_root("create");
        let out = TemplateWriter::create(&root, "turning/new.j2", "G0 X0\n").unwrap();
        assert_eq!(out.action, super::super::WriteAction::Created);
        // 重名 → Conflict（不是静默覆盖）
        let err = TemplateWriter::create(&root, "turning/new.j2", "G0 X1\n").unwrap_err();
        assert!(matches!(err, WriteError::Conflict { .. }), "{err}");
        // 内容未被第二次调用覆盖
        let body = std::fs::read_to_string(root.join("turning/new.j2")).unwrap();
        assert_eq!(body, "G0 X0\n");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn save_honors_optimistic_lock() {
        let root = temp_root("save");
        std::fs::write(root.join("turning/a.j2"), "G0 X0\n").unwrap();
        let fp = WriteKernel::read_fingerprint(&root.join("turning/a.j2"))
            .unwrap()
            .unwrap();
        // 指纹匹配 → 写入
        let out = TemplateWriter::save(&root, "turning/a.j2", "G0 X1\n", Some(fp)).unwrap();
        assert_eq!(out.action, super::super::WriteAction::Updated);
        // 用旧指纹再写 → Conflict
        let err = TemplateWriter::save(&root, "turning/a.j2", "G0 X2\n", Some(fp)).unwrap_err();
        assert!(matches!(err, WriteError::Conflict { .. }), "{err}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn resolve_rel_rejects_escape_components() {
        let root = temp_root("resolve_rel");
        let sp = SafePath::from_root(&root).unwrap();
        for bad in ["../x.j2", "a/../../x.j2", "turning/..", "Z:", ""] {
            assert!(resolve_rel(&sp, bad).is_err(), "{bad:?} 应被判为越界/非法");
        }
        // 正常多级键可解析
        let ok = resolve_rel(&sp, "turning/a.j2").unwrap();
        assert!(ok.starts_with(sp.root()));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn derive_writes_new_file_and_copies_manifest_entry() {
        let root = temp_root("derive");
        std::fs::write(root.join("turning/a.j2"), "{# NAME: A #}\nG0 X0\n").unwrap();
        std::fs::write(
            root.join(MANIFEST_FILE),
            "templates:\n  \"turning/a.j2\":\n    name: \"A\"\n    status: unreviewed\n",
        )
        .unwrap();
        let report = TemplateWriter::derive(
            &root,
            "turning/a.j2",
            "turning/b.j2",
            "{# NAME: B #}\nG0 X0\n",
        )
        .unwrap();
        assert!(root.join("turning/b.j2").exists());
        match report.manifest {
            ManifestOutcome::Written(_) => {}
            other => panic!("应写入清单条目，实际 {other:?}"),
        }
        let manifest = std::fs::read_to_string(root.join(MANIFEST_FILE)).unwrap();
        assert!(manifest.contains("\"turning/b.j2\":"));
        assert!(manifest.contains("status: unreviewed"), "覆盖层应被复制");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn derive_without_manifest_entry_only_writes_file() {
        let root = temp_root("derive_no_entry");
        std::fs::write(root.join("turning/a.j2"), "G0 X0\n").unwrap();
        std::fs::write(
            root.join(MANIFEST_FILE),
            "templates:\n  \"turning/other.j2\":\n    name: \"O\"\n",
        )
        .unwrap();
        let report =
            TemplateWriter::derive(&root, "turning/a.j2", "turning/b.j2", "G0 X0\n").unwrap();
        assert!(matches!(report.manifest, ManifestOutcome::NoEntry));
        let manifest = std::fs::read_to_string(root.join(MANIFEST_FILE)).unwrap();
        assert!(!manifest.contains("turning/b.j2"), "不得凭空造条目");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 清单**不存在**时，`derive` 仍写入模板文件，但清单处理**降级为警告**
    /// （不阻断）——沿用 D13 降级哲学。
    #[test]
    fn derive_without_manifest_degrades_but_writes_file() {
        let root = temp_root("derive_no_manifest");
        std::fs::write(root.join("turning/a.j2"), "G0 X0\n").unwrap();
        let report =
            TemplateWriter::derive(&root, "turning/a.j2", "turning/b.j2", "G0 X0\n").unwrap();
        assert!(root.join("turning/b.j2").exists(), "文件应已写入");
        match report.manifest {
            ManifestOutcome::Degraded(msg) => assert!(msg.contains("清单文件不存在"), "{msg}"),
            other => panic!("应降级，实际 {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 清单存在、源条目也在，但**缺少顶层 `templates:` 块**（条目挂在别的顶层键下）
    /// → `derive` 无法定位插入点，清单处理降级（不阻断文件写入）。
    #[test]
    fn derive_with_manifest_lacking_top_key_degrades() {
        let root = temp_root("derive_no_top");
        std::fs::write(root.join("turning/a.j2"), "G0 X0\n").unwrap();
        std::fs::write(
            root.join(MANIFEST_FILE),
            "# 只有注释\nfoo:\n  \"turning/a.j2\":\n    name: \"A\"\n",
        )
        .unwrap();
        let report =
            TemplateWriter::derive(&root, "turning/a.j2", "turning/b.j2", "G0 X0\n").unwrap();
        assert!(root.join("turning/b.j2").exists(), "文件应已写入");
        match report.manifest {
            ManifestOutcome::Degraded(msg) => assert!(msg.contains("templates:"), "{msg}"),
            other => panic!("应降级，实际 {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 清单**不存在**时 `rename` 仍移动文件，清单处理降级为警告。
    #[test]
    fn rename_without_manifest_degrades_but_moves_file() {
        let root = temp_root("rename_no_manifest");
        std::fs::write(root.join("turning/a.j2"), "G0 X0\n").unwrap();
        let report = TemplateWriter::rename(&root, "turning/a.j2", "turning/b.j2").unwrap();
        assert!(root.join("turning/b.j2").exists());
        assert!(!root.join("turning/a.j2").exists());
        match report.manifest {
            ManifestOutcome::Degraded(msg) => assert!(msg.contains("清单文件不存在"), "{msg}"),
            other => panic!("应降级，实际 {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 清单存在但**无该条目** → `rename` 只移动文件、不动清单（不凭空造条目）。
    #[test]
    fn rename_with_manifest_missing_entry_keeps_manifest() {
        let root = temp_root("rename_no_entry");
        std::fs::write(root.join("turning/a.j2"), "G0 X0\n").unwrap();
        let original = "templates:\n  \"turning/other.j2\":\n    name: \"O\"\n";
        std::fs::write(root.join(MANIFEST_FILE), original).unwrap();
        let report = TemplateWriter::rename(&root, "turning/a.j2", "turning/b.j2").unwrap();
        assert!(matches!(report.manifest, ManifestOutcome::NoEntry));
        assert_eq!(
            std::fs::read_to_string(root.join(MANIFEST_FILE)).unwrap(),
            original,
            "清单不得被改动"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn rename_moves_file_and_rewrites_key() {
        let root = temp_root("rename");
        std::fs::write(root.join("turning/a.j2"), "G0 X0\n").unwrap();
        std::fs::write(
            root.join(MANIFEST_FILE),
            "templates:\n  \"turning/a.j2\":\n    name: \"A\"\n",
        )
        .unwrap();
        let report = TemplateWriter::rename(&root, "turning/a.j2", "turning/b.j2").unwrap();
        assert!(!root.join("turning/a.j2").exists(), "旧文件应被删除");
        assert_eq!(
            std::fs::read_to_string(root.join("turning/b.j2")).unwrap(),
            "G0 X0\n"
        );
        match report.manifest {
            ManifestOutcome::Written(_) => {}
            other => panic!("应改写清单键，实际 {other:?}"),
        }
        let manifest = std::fs::read_to_string(root.join(MANIFEST_FILE)).unwrap();
        assert!(manifest.contains("\"turning/b.j2\":"));
        assert!(!manifest.contains("\"turning/a.j2\":"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn rename_to_existing_target_conflicts() {
        let root = temp_root("rename_conflict");
        std::fs::write(root.join("turning/a.j2"), "G0 X0\n").unwrap();
        std::fs::write(root.join("turning/b.j2"), "G0 Y0\n").unwrap();
        let err = TemplateWriter::rename(&root, "turning/a.j2", "turning/b.j2").unwrap_err();
        assert!(matches!(err, WriteError::Conflict { .. }), "{err}");
        // 旧文件仍在（未半途删除）
        assert!(root.join("turning/a.j2").exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}
