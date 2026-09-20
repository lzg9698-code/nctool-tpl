//! 对抗性验证（QA 严过关 独立构造，刻意区别于工程师自检用例）。
//!
//! 立场：尝试**证伪** T01 写盘底座的四项保证，而不是复述其结论。
//! 断言一律写"正确行为"；若断言失败即说明源码存在缺陷。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use nctool_core::asset::{
    validate_asset_name, FileFingerprint, SafePath, SpecFingerprint, WriteAction, WriteError,
    WriteKernel,
};
use nctool_core::{DeriveRule, ParamKind, ParamSpec, ParamValue, RequiredIf};

fn temp_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("qa_adv_{}_{}_{}", std::process::id(), tag, n));
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

// ===========================================================================
// A. 原子写
// ===========================================================================

/// A1：rename 目标为**非空目录** → 必须失败、目标不变、且目录内无临时文件残留。
#[test]
fn a1_rename_onto_nonempty_dir_fails_without_residue() {
    let dir = temp_dir("a1");
    let target = dir.join("blocked");
    std::fs::create_dir(&target).unwrap();
    std::fs::write(target.join("keep.txt"), b"keep").unwrap();

    let err = WriteKernel::write_atomic(&target, b"NEW").unwrap_err();
    eprintln!("[A1] err = {err:?}");

    assert!(target.is_dir(), "目标目录应保持不变");
    assert_eq!(std::fs::read(target.join("keep.txt")).unwrap(), b"keep");
    assert!(
        !has_temp_residue(&dir),
        "失败后残留临时文件: {:?}",
        dir_names(&dir)
    );
}

/// A2：目标的**父目录是一个普通文件** → 临时文件创建失败，报错且无残留。
#[test]
fn a2_parent_is_a_file_fails_without_residue() {
    let dir = temp_dir("a2");
    let parent_file = dir.join("afile");
    std::fs::write(&parent_file, b"x").unwrap();
    let target = parent_file.join("t.j2");

    let err = WriteKernel::write_atomic(&target, b"y").unwrap_err();
    eprintln!("[A2] err = {err:?}");
    assert!(
        !has_temp_residue(&dir),
        "失败后残留临时文件: {:?}",
        dir_names(&dir)
    );
}

/// A3：目标已存在时 rename 必须真正**替换**（Windows 替换语义）。
#[test]
fn a3_rename_replaces_existing_target() {
    let dir = temp_dir("a3");
    let target = dir.join("t.j2");
    std::fs::write(&target, b"OLD-CONTENT-LONGER").unwrap();

    WriteKernel::write_atomic(&target, b"NEW").unwrap();
    assert_eq!(std::fs::read(&target).unwrap(), b"NEW");
    assert!(!has_temp_residue(&dir));
    assert_eq!(dir_names(&dir), vec!["t.j2".to_string()]);
}

// ===========================================================================
// B. 乐观锁
// ===========================================================================

/// B4（P0 级）：**长度相同、mtime 相同（强制设回）、内容不同** 的改动必须被检出。
/// 这正是设计 §7.4 "禁用纯 mtime" 的判定点：弱比对（len+mtime）会漏检，强比对必须拦下。
#[test]
fn b4_same_length_same_mtime_content_change_is_detected() {
    let dir = temp_dir("b4");
    let target = dir.join("t.j2");
    std::fs::write(&target, b"AAAAAAAA").unwrap();
    let snap = WriteKernel::read_fingerprint(&target).unwrap().unwrap();

    // 外部改动：长度相同（8）、内容不同
    std::fs::write(&target, b"BBBBBBBB").unwrap();
    // 强制把 mtime 设回快照值，模拟"同一秒内两次修改、时间分辨率粗"
    {
        let f = std::fs::OpenOptions::new()
            .write(true)
            .open(&target)
            .unwrap();
        f.set_modified(snap.mtime).unwrap();
    }
    let actual = WriteKernel::read_fingerprint(&target).unwrap().unwrap();

    // 证据：弱比对（仅 len+mtime）在此完全无法区分
    assert_eq!(actual.len, snap.len, "构造应保证长度相同");
    assert_eq!(actual.mtime, snap.mtime, "构造应保证 mtime 相同");
    assert_ne!(actual.hash, snap.hash, "内容不同 → hash 必须不同");
    let weak_would_miss = actual.len == snap.len && actual.mtime == snap.mtime;
    assert!(weak_would_miss, "弱比对应当会漏检（本用例的前提）");

    // 强比对必须检出冲突，且不得覆盖
    let err = WriteKernel::write_guarded(&target, b"CCCCCCCC", Some(snap)).unwrap_err();
    assert!(
        matches!(err, WriteError::Conflict { .. }),
        "同秒同长改动必须判为 Conflict，实际: {err:?}"
    );
    assert_eq!(
        std::fs::read(&target).unwrap(),
        b"BBBBBBBB",
        "冲突时不得覆盖他人改动"
    );
}

/// B5：`expect=None` 与 `expect=Some` 的边界语义。
#[test]
fn b5_expect_semantics() {
    let dir = temp_dir("b5");
    let target = dir.join("t.j2");

    // expect=None 但文件已存在 → Conflict（要求"写前仍不存在"）
    std::fs::write(&target, b"x").unwrap();
    let err = WriteKernel::write_guarded(&target, b"y", None).unwrap_err();
    assert!(matches!(err, WriteError::Conflict { .. }), "{err:?}");
    assert_eq!(std::fs::read(&target).unwrap(), b"x", "冲突时不得写入");

    // expect=Some 但文件被删除 → Conflict（actual=None）
    let snap = WriteKernel::read_fingerprint(&target).unwrap().unwrap();
    std::fs::remove_file(&target).unwrap();
    match WriteKernel::write_guarded(&target, b"z", Some(snap)).unwrap_err() {
        WriteError::Conflict { actual, .. } => assert!(actual.is_none(), "实际应为 None"),
        o => panic!("应为 Conflict，实际 {o:?}"),
    }
    assert!(!target.exists(), "冲突时不得创建文件");

    // expect=None 且文件不存在 → Created
    let out = WriteKernel::write_guarded(&target, b"new", None).unwrap();
    assert_eq!(out.action, WriteAction::Created);
}

/// B6：指纹确定性与 len / mtime 敏感性。
#[test]
fn b6_fingerprint_stable_and_sensitive() {
    let dir = temp_dir("b6");
    let target = dir.join("t.j2");
    std::fs::write(&target, b"hello").unwrap();
    let a = WriteKernel::read_fingerprint(&target).unwrap().unwrap();
    let b = WriteKernel::read_fingerprint(&target).unwrap().unwrap();
    assert!(a.matches(&b));
    assert_eq!(a.as_string(), b.as_string());

    let other = FileFingerprint::of_bytes(b"hello", a.mtime + std::time::Duration::from_secs(1));
    assert!(!a.matches(&other), "mtime 变化必须不等价");
    let longer = FileFingerprint::of_bytes(b"hello!", a.mtime);
    assert!(!a.matches(&longer), "len 变化必须不等价");
}

// ===========================================================================
// C. 路径防护
// ===========================================================================

/// C7：绝对路径 / UNC / 盘符 / 各类穿越一律拒绝。
#[test]
fn c7_validate_rejects_abs_unc_drive_and_traversal() {
    let bad = [
        "",
        ".",
        "..",
        "a/b",
        "../evil",
        r"..\evil",
        r"a\b",
        "/abs",
        r"\\server\share",
        r"C:\Windows",
        "C:/Windows",
        "sub/../..",
    ];
    for b in bad {
        assert!(validate_asset_name(b).is_err(), "{b:?} 应被拒绝");
    }
    for ok in ["a", "a.j2", "钻_孔循环", "wfl_m65", "a-b_c.1"] {
        assert!(validate_asset_name(ok).is_ok(), "{ok:?} 应合法");
    }
}

/// C7b（对抗）：盘符相对前缀（`C:` / `Z:`）既不是合法名称，也**不得**让
/// `SafePath::resolve` 返回安全根之外的路径。这是本次重点排查的逃逸向量：
/// Windows 上 `Path::new("Z:")` 只有一个 Prefix 组件 → 名称校验放行；
/// 而 `root.join("Z:")` 因 RHS 带前缀会**替换**整个路径 → 逃出根。
#[test]
fn c7b_drive_relative_prefix_must_not_escape_root() {
    let root = temp_dir("c7b");
    let sp = SafePath::from_root(&root).unwrap();

    let mut escaped = Vec::new();
    for name in ["C:", "Z:", "Q:"] {
        match sp.resolve(name) {
            Ok(p) if p.starts_with(sp.root()) => {}
            Ok(p) => escaped.push(format!("{name:?} -> {}", p.display())),
            Err(_) => {}
        }
    }
    assert!(
        escaped.is_empty(),
        "SafePath::resolve 返回了安全根之外的路径（路径逃逸）: {escaped:?}"
    );
}

/// C8：指向根**外部**的目录联接必须被拒绝。
///
/// 环境实测（2026-09-20）：本机 `std::os::windows::fs::symlink_dir` 返回 `Ok(())`
/// 却**不创建任何链接**（`read_dir` 为空），故改用 junction（`mklink /J`，无需特权）。
#[test]
fn c8_symlink_or_junction_escape_is_rejected() {
    let root = temp_dir("c8_root");
    let outside = temp_dir("c8_out");
    let link = root.join("escape");

    #[cfg(windows)]
    {
        let st = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&outside)
            .status()
            .expect("调用 mklink 失败");
        assert!(st.success(), "无法创建 junction（环境限制）");
        assert!(link.exists(), "junction 未创建");
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&outside, &link).expect("创建符号链接失败");
    }

    // 证据：链接确实指向安全根之外
    assert_eq!(
        std::fs::canonicalize(&link).unwrap(),
        std::fs::canonicalize(&outside).unwrap(),
        "前置：链接应解析到根外目录"
    );

    let sp = SafePath::from_root(&root).unwrap();
    let err = sp.resolve("escape").unwrap_err();
    assert!(
        matches!(err, WriteError::PathEscape { .. }),
        "指向根外的链接应被拒绝，实际: {err:?}"
    );
}

/// C9（记录性）：控制字符的处理。设计 §7.8 未把控制字符列入拒绝项，
/// 此处仅记录实测行为，不作为缺陷判定。
#[test]
fn c9_control_char_probe() {
    for name in ["a\u{0}b", "a\nb", "a\tb", "a b", "trailing."] {
        eprintln!(
            "[C9] validate_asset_name({name:?}) = {:?}",
            validate_asset_name(name)
        );
    }
}

// ===========================================================================
// E. specFingerprint
// ===========================================================================

fn spec(name: &str, kind: ParamKind) -> ParamSpec {
    ParamSpec::new(name, kind, "doc")
}

#[test]
fn e11_stability_and_e12_order_independence() {
    let a = spec("x", ParamKind::Number);
    let b = spec("y", ParamKind::Integer);
    assert_eq!(
        SpecFingerprint::of(&[a.clone(), b.clone()]),
        SpecFingerprint::of(&[a.clone(), b.clone()]),
        "同输入两次结果必须稳定"
    );
    assert_eq!(
        SpecFingerprint::of(&[a.clone(), b.clone()]),
        SpecFingerprint::of(&[b.clone(), a.clone()]),
        "打乱顺序结果必须不变"
    );
    assert!(SpecFingerprint::of(&[a]).starts_with("fnv1a64:"));
}

/// E13：逐字段敏感性（每个字段单独变化都必须使指纹改变）。
#[test]
fn e13_each_field_change_changes_fingerprint() {
    let fp = SpecFingerprint::of(&[spec("x", ParamKind::Number)]);

    let mut variants: Vec<(&str, ParamSpec)> = Vec::new();
    variants.push(("name", spec("z", ParamKind::Number)));
    variants.push(("kind", spec("x", ParamKind::String)));

    let mut s = spec("x", ParamKind::Number);
    s.required = true;
    variants.push(("required", s));
    let mut s = spec("x", ParamKind::Number);
    s.min = Some(0.0);
    variants.push(("min", s));
    let mut s = spec("x", ParamKind::Number);
    s.max = Some(9.0);
    variants.push(("max", s));
    let mut s = spec("x", ParamKind::Number);
    s.integer = true;
    variants.push(("integer", s));
    let mut s = spec("x", ParamKind::Number);
    s.unit = Some("mm".into());
    variants.push(("unit", s));
    let mut s = spec("x", ParamKind::Number);
    s.required_if = Some(RequiredIf::new("side", [ParamValue::String("R".into())]));
    variants.push(("required_if", s));
    let mut s = spec("x", ParamKind::Number);
    s.derive = Some(DeriveRule {
        from: "src".into(),
        table: vec![],
        fallback: None,
    });
    variants.push(("derive.from", s));
    let mut s = spec("x", ParamKind::Number);
    s.derive = Some(DeriveRule {
        from: "src".into(),
        table: vec![(ParamValue::String("A".into()), ParamValue::Number(1.0))],
        fallback: None,
    });
    variants.push(("derive.table", s));
    let mut s = spec("x", ParamKind::Number);
    s.derive = Some(DeriveRule {
        from: "src".into(),
        table: vec![],
        fallback: Some(ParamValue::Number(2.0)),
    });
    variants.push(("derive.fallback", s));
    let mut s = spec("x", ParamKind::Number);
    s.default = Some(ParamValue::Number(1.5));
    variants.push(("default", s));

    for (field, v) in variants {
        assert_ne!(
            fp,
            SpecFingerprint::of(std::slice::from_ref(&v)),
            "字段 {field} 变化未使指纹改变"
        );
    }
}

/// E13b：`options` 字段**单独**变化（其余字段全同）必须使指纹改变。
#[test]
fn e13b_options_field_isolated() {
    let mut none = spec("x", ParamKind::Choice);
    let mut some = spec("x", ParamKind::Choice);
    some.options = Some(vec![ParamValue::String("A".into())]);
    // 消掉未使用告警（none 有意不设 options）
    none.options = None;
    assert_ne!(
        SpecFingerprint::of(&[none]),
        SpecFingerprint::of(&[some]),
        "options 单独变化未使指纹改变"
    );
}

/// E14：`options` / `required_if` 取值 / `derive.table` 的**顺序**不应改变指纹。
#[test]
fn e14_order_normalization() {
    let mut a = spec("x", ParamKind::Choice);
    a.options = Some(vec![
        ParamValue::String("b".into()),
        ParamValue::String("a".into()),
    ]);
    let mut b = spec("x", ParamKind::Choice);
    b.options = Some(vec![
        ParamValue::String("a".into()),
        ParamValue::String("b".into()),
    ]);
    assert_eq!(SpecFingerprint::of(&[a]), SpecFingerprint::of(&[b]));

    let mut r1 = spec("x", ParamKind::Number);
    r1.required_if = Some(RequiredIf::new(
        "side",
        [
            ParamValue::String("L".into()),
            ParamValue::String("R".into()),
        ],
    ));
    let mut r2 = spec("x", ParamKind::Number);
    r2.required_if = Some(RequiredIf::new(
        "side",
        [
            ParamValue::String("R".into()),
            ParamValue::String("L".into()),
        ],
    ));
    assert_eq!(SpecFingerprint::of(&[r1]), SpecFingerprint::of(&[r2]));

    let mut d1 = spec("x", ParamKind::Number);
    d1.derive = Some(DeriveRule {
        from: "s".into(),
        table: vec![
            (ParamValue::String("A".into()), ParamValue::Number(1.0)),
            (ParamValue::String("B".into()), ParamValue::Number(2.0)),
        ],
        fallback: None,
    });
    let mut d2 = spec("x", ParamKind::Number);
    d2.derive = Some(DeriveRule {
        from: "s".into(),
        table: vec![
            (ParamValue::String("B".into()), ParamValue::Number(2.0)),
            (ParamValue::String("A".into()), ParamValue::Number(1.0)),
        ],
        fallback: None,
    });
    assert_eq!(SpecFingerprint::of(&[d1]), SpecFingerprint::of(&[d2]));
}

/// E15（对抗）：`None` 记 `-` 的歧义 —— `unit=None` 与 `unit=Some("-")` 若撞车，
/// 则"真实值恰为 `-`"的规格变化无法被检出。断言二者**必须**可区分。
#[test]
fn e15_none_must_not_collide_with_literal_dash() {
    let mut none = spec("x", ParamKind::Number);
    none.unit = None;
    let mut dash = spec("x", ParamKind::Number);
    dash.unit = Some("-".into());

    let f_none = SpecFingerprint::of(&[none]);
    let f_dash = SpecFingerprint::of(&[dash]);
    eprintln!("[E15] unit=None      -> {f_none}");
    eprintln!("[E15] unit=Some(\"-\") -> {f_dash}");
    assert_ne!(
        f_none, f_dash,
        "unit=None 与 unit=Some(\"-\") 指纹撞车（None 记 `-` 的歧义）"
    );
}

/// C7c（第 2 轮补充）：P1 修复后，**所有**已知逃逸向量要么被拒，要么解析结果
/// 必落安全根内。特别覆盖盘符相对前缀的变体与 UNC / `\\?\` 前缀。
#[test]
fn c7c_extra_escape_vectors() {
    let root = temp_dir("c7c");
    let sp = SafePath::from_root(&root).unwrap();
    let vectors = [
        "Z:",
        "Q:",
        "C:",
        "Z:foo",
        "Z:/x",
        "Z:\\x",
        r"\\?\C:\x",
        "//server/share",
        r"\\server\share",
        "/abs",
        r"C:\Windows",
        "..",
        "../x",
        "a/b",
        ".",
        "",
    ];
    for v in vectors {
        match sp.resolve(v) {
            Ok(p) => assert!(
                p.starts_with(sp.root()),
                "resolve({v:?}) 放行了根外路径: {}",
                p.display()
            ),
            Err(e) => assert!(
                matches!(e, WriteError::PathEscape { .. }),
                "{v:?} 应判为 PathEscape，实际: {e:?}"
            ),
        }
    }
}

/// C7d（第 2 轮补充）：控制字符被拒（工程师新增），但**正常名称不得被误杀**。
#[test]
fn c7d_control_chars_rejected_normal_names_kept() {
    for bad in ["a\u{0}b", "a\nb", "a\tb", "\u{7}", "\u{1b}[31m"] {
        assert!(
            validate_asset_name(bad).is_err(),
            "{bad:?} 应被拒绝（控制字符）"
        );
    }
    for ok in [
        "钻_孔循环",
        "my.op",
        "a-b_c.1",
        "wfl_m65",
        "程序1",
        "x.y.z",
        "A1",
        "index_ms40",
    ] {
        assert!(validate_asset_name(ok).is_ok(), "{ok:?} 应合法");
    }
}

/// C10（第 2 轮补充）：`from_root` 的 `is_dir()` 校验——根是普通文件必须报错。
#[test]
fn c10_from_root_rejects_non_directory() {
    let dir = temp_dir("c10");
    let file = dir.join("afile");
    std::fs::write(&file, b"x").unwrap();
    assert!(SafePath::from_root(&file).is_err(), "根是普通文件应报错");
    assert!(SafePath::from_root(&dir).is_ok(), "目录根应被接受");
}

/// E13c（第 2 轮补充，架构师显式要求）：`canonical` 里 `name`/`kind`/`required`/
/// `integer` 四个标量字段**确实套了长度前缀** `<len>:<值>`，而非裸值。
/// 直接对 `canonical` 串断言，是"文档/代码漂移"的最后一道拦截。
#[test]
fn e13c_canonical_uses_length_prefix_for_scalar_fields() {
    let mut s = spec("a|b", ParamKind::Number);
    s.required = true;
    s.integer = true;
    let canon = SpecFingerprint::canonical(&[s]);

    assert!(
        canon.contains(&format!("{}:a|b", "a|b".len())),
        "name 未做长度前缀: {canon}"
    );
    assert!(
        canon.contains(&format!("{}:Number", "Number".len())),
        "kind 未做长度前缀: {canon}"
    );
    assert!(
        canon.contains(&format!("{}:true", "true".len())),
        "required/integer 未做长度前缀: {canon}"
    );
    // 不得出现"裸值紧邻分隔符"的形态（即未套前缀）
    assert!(!canon.contains("|Number|"), "kind 出现裸值: {canon}");
    assert!(
        !canon.contains("|false|") && !canon.contains("|true|"),
        "布尔字段出现裸值: {canon}"
    );
}

/// E13d（第 2 轮补充）：分隔符注入不撞车——`name` 含 `|` 的规格，与把相同字符
/// 拆进其它字段的规格，指纹必须不同（长度前缀使值自定界）。
#[test]
fn e13d_separator_injection_does_not_collide() {
    let plain = spec("a", ParamKind::Number);
    let injected = spec("a|Number|false", ParamKind::Number);
    assert_ne!(
        SpecFingerprint::of(&[plain]),
        SpecFingerprint::of(&[injected]),
        "name 含分隔符时与普通 name 撞车 → 字段未自定界"
    );
}

/// E13e（第 2 轮补充）：`description` 不参与指纹（改错别字不得让预设失效）。
#[test]
fn e13e_description_does_not_change_fingerprint() {
    let mut a = spec("x", ParamKind::Number);
    a.description = "说明一".into();
    let mut b = spec("x", ParamKind::Number);
    b.description = "完全不同的一段说明，故意含错别字".into();
    assert_eq!(
        SpecFingerprint::of(&[a]),
        SpecFingerprint::of(&[b]),
        "description 变化不得改变指纹"
    );
}

/// E16（第 2 轮补充）：`ParamValue::List` 的 `l:{n}[...]` 编码未被"统一"掉，
/// 且元素含 `,` 时与"两个元素"不撞车。
#[test]
fn e16_list_encoding_unchanged_and_unambiguous() {
    let mut one = spec("x", ParamKind::List);
    one.default = Some(ParamValue::List(vec![ParamValue::String("a,b".into())]));
    let mut two = spec("x", ParamKind::List);
    two.default = Some(ParamValue::List(vec![
        ParamValue::String("a".into()),
        ParamValue::String("b".into()),
    ]));
    assert_ne!(
        SpecFingerprint::of(&[one]),
        SpecFingerprint::of(&[two]),
        "List 元素含 `,` 与两个元素撞车"
    );

    let mut l = spec("x", ParamKind::List);
    l.default = Some(ParamValue::List(vec![
        ParamValue::Integer(1),
        ParamValue::Integer(2),
    ]));
    let canon = SpecFingerprint::canonical(&[l]);
    assert!(canon.contains("l:2["), "List 编码应为 `l:<n>[` : {canon}");
}
