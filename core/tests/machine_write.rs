//! 模块二（机床配置编辑）写策略的集成测试：`MachineWriter` 的
//! `upsert` / `remove` / `load` 在**真实文件**上的行为。
//!
//! 重点钉住 **AC-2.10**：写入 `[machine.<id>]` 后，`nctool.toml` 的其余段与
//! 注释必须**逐字节不变**（golden 基线 `tests/golden/machine/*`）。
//!
//! 单元级（纯函数）覆盖见 `core/src/asset/machine.rs` 的 `mod tests`；本文件只做
//! "落盘后整文件长什么样"的端到端断言——那正是 golden 能钉住、单测钉不住的部分。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use nctool_core::asset::{MachineWriter, WriteAction, WriteError, WriteKernel};
use nctool_core::MachineConfig;

// ---------------------------------------------------------------------------
// 夹具
// ---------------------------------------------------------------------------

fn tmpdir(tag: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("nctool_mwrite_{tag}_{}_{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("建临时目录");
    dir
}

fn golden_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("tests")
        .join("golden")
        .join("machine")
        .join(name)
}

fn normalize_newlines(s: &str) -> String {
    s.replace("\r\n", "\n").replace('\r', "\n")
}

/// 断言与 golden 一致；`NCTOOL_UPDATE_GOLDEN=1` 时刷新（CI 下硬拦）。
fn assert_golden(name: &str, actual: &str) {
    let actual = normalize_newlines(actual);
    let path = golden_path(name);
    if std::env::var_os("NCTOOL_UPDATE_GOLDEN").is_some() {
        assert!(
            std::env::var_os("CI").is_none(),
            "CI 环境禁止刷新 golden 基线（检测到 NCTOOL_UPDATE_GOLDEN）"
        );
        std::fs::create_dir_all(path.parent().unwrap()).expect("建 golden 目录");
        std::fs::write(&path, actual.as_bytes()).expect("写 golden 文件");
        return;
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("读取 golden 失败 {}: {e}", path.display()));
    assert_eq!(
        actual,
        normalize_newlines(&expected),
        "golden 不匹配: {}",
        path.display()
    );
}

fn machine(id: &str) -> MachineConfig {
    let mut config = BTreeMap::new();
    // 覆盖非字符串值来源（都存字符串）与普通键
    config.insert("program_prefix".to_string(), "O".to_string());
    config.insert("linear".to_string(), "G1".to_string());
    config.insert("max_spindle_rpm".to_string(), "8000".to_string());
    MachineConfig {
        id: id.to_string(),
        vendor: "HERO".to_string(),
        model: "X9".to_string(),
        config,
    }
}

fn fingerprint(p: &Path) -> nctool_core::asset::FileFingerprint {
    WriteKernel::read_fingerprint(p)
        .unwrap()
        .expect("文件应存在")
}

// ---------------------------------------------------------------------------
// AC-2.10：golden —— 其余段与注释逐字节不变
// ---------------------------------------------------------------------------

/// AC-2.10（本任务最关键验收）：把 `[machine.hero_x9]` 写入一份带注释、带既有
/// `[machine.other]`、带行内注释与尾部注释的 `nctool.toml`，其余内容必须逐字节不变。
///
/// golden 基线在仓库中恒为 LF；比较前归一化行尾（见 `integration.rs` 同一约定）。
#[test]
fn ac_2_10_upsert_preserves_other_sections_and_comments() {
    let dir = tmpdir("golden");
    let path = dir.join("nctool.toml");
    let input = std::fs::read_to_string(golden_path("nctool_input.toml")).expect("读 golden 输入");
    std::fs::write(&path, &input).expect("写输入");

    MachineWriter::upsert(&path, &machine("hero_x9"), Some(fingerprint(&path)))
        .expect("upsert 应成功");

    let after = std::fs::read_to_string(&path).expect("读回");
    // 输入整段必须原样为前缀（含行内注释与尾部注释）
    assert!(
        after.starts_with(&input),
        "其余段与注释必须逐字节不变：\n--- before ---\n{input}\n--- after ---\n{after}"
    );
    assert_golden("nctool_after_upsert.toml", &after);
}

/// 边界：CRLF 行尾的既有内容必须原样保留（不被 toml_edit 归一成 LF）。
#[test]
fn ac_2_10_crlf_input_is_byte_preserved() {
    let dir = tmpdir("crlf");
    let path = dir.join("nctool.toml");
    let input =
        "# 头注释\r\ntemplate_dir = \"templates\"\r\n\r\n[machine.other]\r\nid = \"other\"\r\n";
    std::fs::write(&path, input).expect("写输入");

    MachineWriter::upsert(&path, &machine("hero_x9"), Some(fingerprint(&path))).unwrap();
    let after = std::fs::read_to_string(&path).unwrap();
    assert!(
        after.starts_with(input),
        "CRLF 既有内容必须逐字节保留：\n{:?}",
        after
    );
}

/// 边界：既有文件**尾部无换行**时也不得被改写/吞掉最后一行。
#[test]
fn ac_2_10_no_trailing_newline_is_preserved() {
    let dir = tmpdir("notrail");
    let path = dir.join("nctool.toml");
    let input = "template_dir = \"templates\""; // 无尾换行
    std::fs::write(&path, input).unwrap();

    MachineWriter::upsert(&path, &machine("hero_x9"), Some(fingerprint(&path))).unwrap();
    let after = std::fs::read_to_string(&path).unwrap();
    assert!(
        after.starts_with(input),
        "尾部无换行的内容必须保留：{after:?}"
    );
    assert!(after.contains("[machine.hero_x9]"));
}

/// 首次创建（CLI 先写 `EXAMPLE_CONFIG` 头注释）：纯注释内容必须仍在**顶部**，
/// 且不得出现空 `[machine]` 表头。
#[test]
fn comment_only_file_keeps_header_at_top() {
    let dir = tmpdir("comment_only");
    let path = dir.join("nctool.toml");
    let input =
        "# nctool 配置示例\n# 配置层级：项目 ./nctool.toml\n\n# template_dir = \"templates\"\n";
    std::fs::write(&path, input).unwrap();

    MachineWriter::upsert(&path, &machine("hero_x9"), Some(fingerprint(&path))).unwrap();
    let after = std::fs::read_to_string(&path).unwrap();
    assert!(after.starts_with(input), "注释必须在顶部：\n{after}");
    assert!(
        !after.contains("\n[machine]\n"),
        "不应有空 [machine] 表头：\n{after}"
    );
}

// ---------------------------------------------------------------------------
// 回读 / 删除
// ---------------------------------------------------------------------------

#[test]
fn upsert_roundtrips_all_config_keys() {
    let dir = tmpdir("roundtrip");
    let path = dir.join("nctool.toml");
    let cfg = machine("hero_x9");
    MachineWriter::upsert(&path, &cfg, None).unwrap();

    let loaded = MachineWriter::load(&path).unwrap();
    let got = loaded.get("hero_x9").expect("应能取回");
    assert_eq!(got, &cfg, "往返后配置必须完全一致");
}

#[test]
fn remove_deletes_section_and_prunes_empty_machine_table() {
    let dir = tmpdir("remove");
    let path = dir.join("nctool.toml");
    MachineWriter::upsert(&path, &machine("only"), None).unwrap();
    assert!(std::fs::read_to_string(&path)
        .unwrap()
        .contains("[machine.only]"));

    let out = MachineWriter::remove(&path, "only", Some(fingerprint(&path))).unwrap();
    assert_eq!(
        out.action,
        WriteAction::Deleted,
        "删除动作必须报 Deleted 而非 Updated"
    );

    let after = std::fs::read_to_string(&path).unwrap();
    assert!(!after.contains("machine.only"), "段应被删除：\n{after}");
    assert!(
        !after.contains("[machine]"),
        "空 [machine] 表头应被一并移除：\n{after}"
    );
    assert!(MachineWriter::load(&path).unwrap().is_empty());
}

#[test]
fn remove_missing_machine_is_not_found_not_silent() {
    let dir = tmpdir("remove_missing");
    let path = dir.join("nctool.toml");
    MachineWriter::upsert(&path, &machine("present"), None).unwrap();
    let err = MachineWriter::remove(&path, "ghost", Some(fingerprint(&path))).unwrap_err();
    assert!(matches!(err, WriteError::NotFound(_)), "{err:?}");
}

/// 乐观锁：`expect` 与实测不符时必须拒绝，且不写盘。
#[test]
fn upsert_with_stale_expect_is_conflict_and_no_write() {
    let dir = tmpdir("conflict");
    let path = dir.join("nctool.toml");
    MachineWriter::upsert(&path, &machine("a"), None).unwrap();
    let before = std::fs::read(&path).unwrap();

    // 用一个"文件不存在"的期望（None）去写已存在的文件 → Conflict
    let err = MachineWriter::upsert(&path, &machine("b"), None).unwrap_err();
    assert!(matches!(err, WriteError::Conflict { .. }), "{err:?}");
    assert_eq!(std::fs::read(&path).unwrap(), before, "冲突时不得写盘");
}

/// 损坏的 `nctool.toml`：读/写都必须报错（`Corrupt`），**绝不**降级为空后覆盖。
#[test]
fn corrupt_config_is_error_not_silent_overwrite() {
    let dir = tmpdir("corrupt");
    let path = dir.join("nctool.toml");
    std::fs::write(&path, "not [ valid toml").unwrap();
    let before = std::fs::read(&path).unwrap();

    assert!(matches!(
        MachineWriter::load(&path).unwrap_err(),
        WriteError::Corrupt(_)
    ));
    assert!(matches!(
        MachineWriter::upsert(&path, &machine("x"), Some(fingerprint(&path))).unwrap_err(),
        WriteError::Corrupt(_)
    ));
    assert_eq!(std::fs::read(&path).unwrap(), before, "损坏文件不得被覆盖");
}

// ---------------------------------------------------------------------------
// 替换既有条目：表头之上 / 之下的注释必须保留
// ---------------------------------------------------------------------------

/// **回归（T04-b 收口时发现并修复）**：替换 `[machine.<id>]` 时，该表**上方**的
/// 注释块与**后续**同级注释必须原样保留，位置也不能变。
///
/// 缺陷形态：`toml_edit` 把紧邻表头的注释存为该表的 `decor`，而 `Table::insert`
/// 换入新表时连同旧 decor 一起丢弃 —— 于是 `machine edit` **静默删掉用户的注释**。
/// 最典型的一例是 `machine add` 首次创建写入的 `EXAMPLE_CONFIG` 示例头（19 行），
/// 它恰是文件第一个表的前缀，一次 `edit` 就全数消失（实测 43 行 → 25 行）。
#[test]
fn upsert_replacing_an_entry_keeps_surrounding_comments() {
    let dir = tmpdir("decor");
    let path = dir.join("nctool.toml");
    let before = concat!(
        "# 头部注释块第一行\n",
        "# 头部注释块第二行\n",
        "\n",
        "[machine.hero]\n",
        "id = \"hero\"\n",
        "vendor = \"HERO\"\n",
        "model = \"X9\"\n",
        "\n",
        "# 目标表内 config 子表之上的注释\n",
        "[machine.hero.config]\n",
        "linear = \"G1\"\n",
        "\n",
        "# 目标段之后的尾部注释\n",
        "\n",
        "[machine.other]\n",
        "id = \"other\"\n",
        "vendor = \"OTHER\"\n",
        "model = \"M1\"\n",
    );
    std::fs::write(&path, before).unwrap();

    let mut cfg = machine("hero");
    cfg.config.insert("linear".to_string(), "G2".to_string());
    MachineWriter::upsert(&path, &cfg, Some(fingerprint(&path))).unwrap();

    let after = std::fs::read_to_string(&path).unwrap();
    for needle in [
        "# 头部注释块第一行",
        "# 头部注释块第二行",
        "# 目标表内 config 子表之上的注释",
        "# 目标段之后的尾部注释",
        "[machine.other]",
    ] {
        assert!(
            after.contains(needle),
            "替换目标表后丢了 {needle:?}：\n{after}"
        );
    }
    assert!(after.contains("linear = \"G2\""), "值应被更新：\n{after}");
    // 位置也不能漂：文件头仍在最前，后面的条目仍在末尾。
    assert!(after.starts_with("# 头部注释块第一行"), "\n{after}");
    assert!(after.trim_end().ends_with("model = \"M1\""), "\n{after}");
}

/// id 注入防护：非法 id 一律 `PathEscape`，不落盘。
#[test]
fn illegal_ids_are_rejected() {
    let dir = tmpdir("badid");
    let path = dir.join("nctool.toml");
    for bad in ["../evil", "a/b", "a\\b", "", "C:", "a\nb"] {
        let err = MachineWriter::upsert(&path, &machine(bad), None).unwrap_err();
        assert!(
            matches!(err, WriteError::PathEscape { .. }),
            "{bad:?} 应被拒: {err:?}"
        );
    }
    assert!(!path.exists(), "非法 id 不得创建文件");
}
