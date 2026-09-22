//! 对抗性验证（QA 独立构造，刻意区别于 `machine_write.rs` 的工程师自检用例）。
//!
//! 立场：尝试**证伪** T04 机床写策略的四项保证，而不是复述其结论。断言一律写
//! "正确行为"；断言失败即说明源码存在缺陷。
//!
//! 两条用例带**变异判别**（照 `asset_manifest_boundary_qa.rs` 的手法）：不只断言
//! "用例绿"，还用一个**缺陷变体**重放同一输入，证明这组输入真的能区分对错实现
//! ——否则用例是空洞的（输入本身触不出差异，写不写检查都会绿）。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use nctool_core::asset::{MachineWriter, WriteError, WriteKernel};
use nctool_core::MachineConfig;

fn temp_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("qa_machine_{}_{}_{}", std::process::id(), tag, n));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("创建临时目录失败");
    dir
}

fn has_temp_residue(dir: &Path) -> bool {
    std::fs::read_dir(dir)
        .map(|it| {
            it.filter_map(|e| e.ok())
                .any(|e| e.file_name().to_string_lossy().contains(".nctool-tmp-"))
        })
        .unwrap_or(false)
}

fn dir_names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

fn machine(id: &str) -> MachineConfig {
    let mut config = BTreeMap::new();
    config.insert("program_prefix".to_string(), "O".to_string());
    config.insert("linear".to_string(), "G1".to_string());
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

// ===========================================================================
// 1. AC-2.10 的组合边界：CRLF + 尾部无换行 + 真实 [template_dir] 表 + 既有段 + 注释
// ===========================================================================

/// 四条边界**同时**出现时，除了新增的那一段，其余字节必须逐字节不变。
///
/// 已有的 `machine_write.rs` 用例各自只覆盖一条边界（CRLF / 尾无换行 / 注释），
/// 组合起来才逼出"追加路径是否真的原样搬运字节"这一点。
#[test]
fn hostile_input_combination_keeps_every_other_byte() {
    let dir = temp_dir("combo");
    let path = dir.join("nctool.toml");
    // 注意：整段用 CRLF，且**末尾没有换行**（两处最容易在写入时被"顺手修好"的地方）。
    let before = concat!(
        "# 头部注释\r\n",
        "[template_dir]\r\n",
        "path = \"templates\"   # 行内注释\r\n",
        "\r\n",
        "[machine.other]\r\n",
        "id = \"other\"\r\n",
        "vendor = \"ACME\"\r\n",
        "\r\n",
        "[machine.other.config]\r\n",
        "linear = \"G1\"\r\n",
        "\r\n",
        "# 尾部注释（无结尾换行）",
    );
    std::fs::write(&path, before).unwrap();

    MachineWriter::upsert(&path, &machine("hero_x9"), Some(fingerprint(&path))).unwrap();

    let after = std::fs::read_to_string(&path).unwrap();
    assert!(
        after.starts_with(before),
        "原有字节必须原样留在文件前部\n--- 写入后 ---\n{after}"
    );
    // 新增段可读回（不是只搬了字节、写了个空段）。
    let loaded = MachineWriter::load(&path).unwrap();
    assert_eq!(loaded.get("hero_x9"), Some(&machine("hero_x9")));
    assert_eq!(loaded.get("other").map(|m| m.vendor.as_str()), Some("ACME"));
    assert!(!has_temp_residue(&dir), "残留: {:?}", dir_names(&dir));
}

// ===========================================================================
// 2. 结构被写歪：拒绝写入，且文件原样不动
// ===========================================================================

/// `machine = "x"`（顶层被写成普通键）→ `Corrupt`，**不写盘**。
///
/// 若这里静默把它当普通值覆盖，用户的机床段会被无声替换成一个表。
#[test]
fn machine_written_as_plain_key_is_refused() {
    let dir = temp_dir("plainkey");
    let path = dir.join("nctool.toml");
    std::fs::write(&path, "machine = \"x\"\n").unwrap();
    let before = std::fs::read(&path).unwrap();

    let err = MachineWriter::upsert(&path, &machine("hero"), Some(fingerprint(&path))).unwrap_err();
    assert!(matches!(err, WriteError::Corrupt(_)), "{err:?}");
    assert_eq!(
        std::fs::read(&path).unwrap(),
        before,
        "拒绝写入时文件不得变"
    );
    assert!(!has_temp_residue(&dir), "残留: {:?}", dir_names(&dir));
}

/// 重复键（`linear` 写两遍）→ TOML 本身非法 → `Corrupt`，不写盘。
#[test]
fn duplicate_keys_are_refused_not_silently_deduped() {
    let dir = temp_dir("dupkey");
    let path = dir.join("nctool.toml");
    let before = "[machine.hero]\nid = \"hero\"\nlinear = \"G1\"\nlinear = \"G2\"\n";
    std::fs::write(&path, before).unwrap();

    let err = MachineWriter::upsert(&path, &machine("hero"), Some(fingerprint(&path))).unwrap_err();
    assert!(matches!(err, WriteError::Corrupt(_)), "{err:?}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
}

// ===========================================================================
// 3. 行内表 `config = { … }`：读路径与写路径必须给同一个答案
// ===========================================================================

/// `config` 的两种写法语义等价：标准子表与**行内表**。
///
/// 缺陷形态：`extract_machines` 只认子表，而行内表是 `Item::Value` 而非
/// `Item::Table` → `load` 静默返回**空配置**；`machine edit` 于是把行内表里的键
/// 当成"不存在"，在注册表降级（模板目录里有坏模板）导致 preflight 放行时**静默
/// 丢掉**它们 —— 值合法、不报错、只是错。读路径（serde）两种都认，写路径也必须。
#[test]
fn inline_table_config_is_read_like_a_standard_subtable() {
    let dir = temp_dir("inline");
    let path = dir.join("nctool.toml");
    std::fs::write(
        &path,
        "[machine.hero]\nid = \"hero\"\nvendor = \"ACME\"\nmodel = \"X9\"\n\
         config = { linear = \"G9\", rapid = \"G0\", max_spindle_rpm = 4200 }\n",
    )
    .unwrap();

    let loaded = MachineWriter::load(&path).unwrap();
    let hero = loaded.get("hero").expect("应读回 hero");
    assert_eq!(hero.vendor, "ACME");
    assert_eq!(hero.config.get("linear").map(String::as_str), Some("G9"));
    assert_eq!(hero.config.get("rapid").map(String::as_str), Some("G0"));
    // 非字符串标量取原样文本（与子表形态同口径）
    assert_eq!(
        hero.config.get("max_spindle_rpm").map(String::as_str),
        Some("4200")
    );

    // 写回时这些键**不得丢**（形状可能从行内表变成标准子表，语义必须等价）。
    let mut edited = hero.clone();
    edited.config.insert("linear".to_string(), "G1".to_string());
    MachineWriter::upsert(&path, &edited, Some(fingerprint(&path))).unwrap();
    let after = MachineWriter::load(&path).unwrap();
    let got = after.get("hero").unwrap();
    assert_eq!(got.config.get("linear").map(String::as_str), Some("G1"));
    assert_eq!(
        got.config.get("rapid").map(String::as_str),
        Some("G0"),
        "行内表里的其它键被静默丢了"
    );
    assert_eq!(
        got.config.get("max_spindle_rpm").map(String::as_str),
        Some("4200")
    );
}

/// **非字符串值不得带装饰空白**（两种写法都要）。
///
/// 缺陷形态：`toml_edit` 把 `key = value` 里 `=` 之后的空白存进该值的 **decor**，
/// `Value::to_string()` 会把它一并渲染出来 —— `max_spindle_rpm = 4200` 读成
/// `" 4200"`，行内表末值还会带尾随空格。值会经 `machine.<key>` 注入渲染，多一个
/// 空格就进了 G-code。
#[test]
fn scalar_values_have_no_decor_whitespace() {
    let dir = temp_dir("decor_ws");

    // 标准子表形态：裸整数 / 裸布尔
    let a = dir.join("a.toml");
    std::fs::write(
        &a,
        "[machine.hero]\nid = \"hero\"\n[machine.hero.config]\nmax_spindle_rpm = 4200\nlinear=\"G1\"\n",
    )
    .unwrap();
    let cfg = MachineWriter::load(&a).unwrap();
    let hero = cfg.get("hero").unwrap();
    assert_eq!(
        hero.config.get("max_spindle_rpm").map(String::as_str),
        Some("4200"),
        "裸整数不应带装饰空白"
    );
    assert_eq!(hero.config.get("linear").map(String::as_str), Some("G1"));

    // 行内表形态：末值最容易带上尾随空格
    let b = dir.join("b.toml");
    std::fs::write(
        &b,
        "[machine.hero]\nconfig = { n = 4200, ok = true, s = \"G1\" }\n",
    )
    .unwrap();
    let cfg = MachineWriter::load(&b).unwrap();
    let got = &cfg.get("hero").unwrap().config;
    assert_eq!(got.get("n").map(String::as_str), Some("4200"), "{got:?}");
    assert_eq!(got.get("ok").map(String::as_str), Some("true"), "{got:?}");
    assert_eq!(got.get("s").map(String::as_str), Some("G1"), "{got:?}");
}

// ===========================================================================
// 4. 反空洞：变异判别
// ===========================================================================

/// **判别 1**：证明非法 id 用例真的能区分"校验 / 不校验"两版实现。
///
/// 朴素变体（直接 `join`，不校验名称）会把 `../evil.toml` 写到**根外**；
/// 正式实现拒绝同一输入。若朴素变体也写不出去，说明这组输入本身没有区分力，
/// `illegal_ids_are_rejected` 就是一条空洞用例。
#[test]
fn illegal_id_input_discriminates_naive_join_variant() {
    let dir = temp_dir("disc_id");
    let root = dir.join("project"); // 模拟"资产根"
    std::fs::create_dir_all(&root).unwrap();

    // 朴素变体：不校验名称直接拼路径。
    let naive_target = root.join("../evil.toml");
    WriteKernel::write_atomic(&naive_target, b"naive").unwrap();
    assert!(
        dir.join("evil.toml").exists(),
        "朴素变体应能写到根外 —— 否则这组输入没有区分力，用例是空洞的"
    );
    std::fs::remove_file(dir.join("evil.toml")).unwrap();

    // 正式实现：同一输入被拒，根外不再出现文件。
    let err =
        MachineWriter::upsert(&root.join("nctool.toml"), &machine("../evil"), None).unwrap_err();
    assert!(matches!(err, WriteError::PathEscape { .. }), "{err:?}");
    assert!(!dir.join("evil.toml").exists(), "正式实现不得写到根外");
    assert_no_escape(&dir);
}

/// **判别 2**：证明"替换条目保留注释"的用例真的能区分"保 decor / 丢 decor"两版。
///
/// 朴素变体（`Table::insert` 换入新表，不搬 decor）会把表头上的注释块丢掉；
/// 正式实现保留它。若朴素变体也保住了注释，说明输入没有区分力。
#[test]
fn comment_preservation_input_discriminates_naive_replace() {
    let src = "# 注释块\n[machine.hero]\nid = \"hero\"\n";

    // 朴素变体：直接插入新表（不复刻正式实现的 decor 搬运）。
    let mut doc: toml_edit::DocumentMut = src.parse().unwrap();
    let mut entry = toml_edit::Table::new();
    entry.insert("id", toml_edit::value("hero"));
    doc["machine"]["hero"] = toml_edit::Item::Table(entry);
    let naive = doc.to_string();
    assert!(
        !naive.contains("# 注释块"),
        "朴素变体应丢掉注释 —— 否则这组输入没有区分力：\n{naive}"
    );

    // 正式实现：注释必须保留。
    let dir = temp_dir("disc_decor");
    let path = dir.join("nctool.toml");
    std::fs::write(&path, src).unwrap();
    MachineWriter::upsert(&path, &machine("hero"), Some(fingerprint(&path))).unwrap();
    let after = std::fs::read_to_string(&path).unwrap();
    assert!(after.contains("# 注释块"), "正式实现丢了注释：\n{after}");
}

/// 目录内不得出现任何逃出 `root` 的痕迹（临时文件、被拒的写入）。
fn assert_no_escape(root: &Path) {
    assert!(!has_temp_residue(root), "残留: {:?}", dir_names(root));
}

// ===========================================================================
// 5. 有意接受的边界（钉成"决定"，而不是"事故"）
// ===========================================================================

/// 替换 `[machine.<id>]` **段内**的注释会被丢弃（设计 §10 已登记、有意接受），
/// 但**段外**的一个字节都不能动。
///
/// 把已登记的取舍写成用例，一是避免将来被误报成缺陷，二是防止有人"顺手"扩大
/// 保留范围而破坏"其余字节不变"这条硬保证。
#[test]
fn replacing_a_section_drops_inner_comments_but_not_anything_outside() {
    let dir = temp_dir("inner");
    let path = dir.join("nctool.toml");
    let before = concat!(
        "# 段外：必须保留\n",
        "[machine.hero]\n",
        "id = \"hero\"\n",
        "# 段内：允许被替换\n",
        "linear = \"G1\"\n",
        "\n",
        "[machine.other]\n",
        "id = \"other\"\n",
        "vendor = \"ACME\"\n",
        "\n",
        "# 段外：末尾也必须保留\n",
    );
    std::fs::write(&path, before).unwrap();

    MachineWriter::upsert(&path, &machine("hero"), Some(fingerprint(&path))).unwrap();
    let after = std::fs::read_to_string(&path).unwrap();

    assert!(after.starts_with("# 段外：必须保留\n"), "\n{after}");
    assert!(after.contains("# 段外：末尾也必须保留"), "\n{after}");
    assert!(
        after.contains("vendor = \"ACME\""),
        "兄弟段不得被动：\n{after}"
    );
    assert!(
        !after.contains("# 段内：允许被替换"),
        "段内注释的丢弃是有意取舍（§10）；若将来改为保留，请同步更新本条与设计文档：\n{after}"
    );
}
