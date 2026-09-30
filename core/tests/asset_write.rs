//! `core::asset` 写内核集成测试：原子写 / 乐观锁 / 路径穿越 / 只读 / 符号链接逃逸。
//!
//! 全部用例都在系统临时目录中进行，不触碰仓库工作区或用户配置。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use nctool_core::asset::{
    validate_asset_name, FileFingerprint, SafePath, WriteAction, WriteError, WriteKernel,
};

/// 建一个唯一的临时目录（位于系统临时区）。
fn temp_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir =
        std::env::temp_dir().join(format!("nctool_asset_{}_{}_{}", std::process::id(), tag, n));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("创建临时目录失败");
    dir
}

/// 目录中是否存在本内核的临时文件残留（`.nctool-tmp-` 标记）。
fn has_temp_residue(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries
        .filter_map(|e| e.ok())
        .any(|e| e.file_name().to_string_lossy().contains(".nctool-tmp-"))
}

// ---------------------------------------------------------------------------
// 原子写
// ---------------------------------------------------------------------------

#[test]
fn atomic_write_creates_and_overwrites() {
    let dir = temp_dir("atomic_basic");
    let target = dir.join("demo.j2");

    WriteKernel::write_atomic(&target, b"one").unwrap();
    assert_eq!(std::fs::read(&target).unwrap(), b"one");

    WriteKernel::write_atomic(&target, b"two-longer").unwrap();
    assert_eq!(std::fs::read(&target).unwrap(), b"two-longer");

    assert!(!has_temp_residue(&dir), "成功写后不应残留临时文件");
}

#[test]
fn atomic_write_leaves_no_temp_files_after_many_writes() {
    let dir = temp_dir("atomic_many");
    let target = dir.join("a.j2");
    for i in 0..20 {
        WriteKernel::write_atomic(&target, format!("v{i}").as_bytes()).unwrap();
    }
    assert_eq!(std::fs::read(&target).unwrap(), b"v19");
    assert!(!has_temp_residue(&dir));
    // 目录里应只有目标文件本身
    let names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec!["a.j2".to_string()], "实际目录内容: {names:?}");
}

/// 失败/中断场景：目标无法被替换（此处用非空目录充当目标），
/// 断言目标内容不变、且**不残留**任何临时文件（无半成品）。
#[test]
fn failed_write_leaves_no_temp_and_target_intact() {
    let dir = temp_dir("atomic_fail");
    let target = dir.join("blocked");
    std::fs::create_dir(&target).unwrap();
    std::fs::write(target.join("inner"), b"keep").unwrap();

    let err = WriteKernel::write_atomic(&target, b"NEW");
    assert!(err.is_err(), "向目录写文件应失败");

    assert!(target.is_dir(), "目标目录应保持不变");
    assert_eq!(std::fs::read(target.join("inner")).unwrap(), b"keep");
    assert!(!has_temp_residue(&dir), "失败后不应残留临时文件");
}

/// 父目录不存在 → 临时文件创建失败，报错且无残留。
#[test]
fn atomic_write_fails_when_parent_missing() {
    let dir = temp_dir("atomic_noparent");
    let target = dir.join("no_such_subdir").join("a.j2");
    let err = WriteKernel::write_atomic(&target, b"x");
    assert!(err.is_err(), "父目录不存在应报错");
    assert!(!has_temp_residue(&dir), "失败后不应残留临时文件");
}

#[test]
fn guarded_remove_preserves_external_edits() {
    let dir = temp_dir("guarded_remove_conflict");
    let target = dir.join("source.j2");
    WriteKernel::write_guarded(&target, b"original", None).unwrap();
    let opened = WriteKernel::read_fingerprint(&target).unwrap().unwrap();
    std::fs::write(&target, b"external edit").unwrap();

    let err = WriteKernel::remove_guarded(&target, Some(opened)).unwrap_err();
    assert!(matches!(err, WriteError::Conflict { .. }), "{err}");
    assert_eq!(std::fs::read(&target).unwrap(), b"external edit");
}

// ---------------------------------------------------------------------------
// 乐观锁
// ---------------------------------------------------------------------------

#[test]
fn guarded_write_reports_created_updated_unchanged() {
    let dir = temp_dir("guarded_actions");
    let target = dir.join("a.j2");

    // 新建
    let out = WriteKernel::write_guarded(&target, b"one", None).unwrap();
    assert_eq!(out.action, WriteAction::Created);
    assert_eq!(out.path, target);
    assert!(out.fingerprint.is_some(), "写后应给出指纹字符串");

    let snap = WriteKernel::read_fingerprint(&target)
        .unwrap()
        .expect("应存在");

    // 内容相同 → Unchanged（不落盘）
    let out = WriteKernel::write_guarded(&target, b"one", Some(snap)).unwrap();
    assert_eq!(out.action, WriteAction::Unchanged);

    // 内容不同 → Updated
    let out = WriteKernel::write_guarded(&target, b"two", Some(snap)).unwrap();
    assert_eq!(out.action, WriteAction::Updated);
    assert_eq!(std::fs::read(&target).unwrap(), b"two");

    assert!(!has_temp_residue(&dir));
}

#[test]
fn guarded_write_expect_none_conflicts_when_file_exists() {
    let dir = temp_dir("guarded_expect_none");
    let target = dir.join("a.j2");
    std::fs::write(&target, b"x").unwrap();

    let err = WriteKernel::write_guarded(&target, b"y", None).unwrap_err();
    assert!(
        matches!(err, WriteError::Conflict { .. }),
        "expect=None 但文件已存在应冲突，实际: {err:?}"
    );
    assert_eq!(std::fs::read(&target).unwrap(), b"x", "冲突时不得写入");
}

/// 关键回归：外部改动在**同一秒内**（mtime 可能相同）也必须被检出——
/// 这正是"禁用纯 mtime 比对"的原因。
#[test]
fn guarded_write_conflict_detected_on_external_change() {
    let dir = temp_dir("guarded_conflict");
    let target = dir.join("a.j2");
    std::fs::write(&target, b"v1").unwrap();
    let snap = WriteKernel::read_fingerprint(&target)
        .unwrap()
        .expect("应存在");

    // 外部改动（内容变化；即便 mtime 与上一次相同也应检出）
    std::fs::write(&target, b"v2-external").unwrap();

    let err = WriteKernel::write_guarded(&target, b"v3", Some(snap)).unwrap_err();
    match err {
        WriteError::Conflict {
            expected, actual, ..
        } => {
            assert_eq!(expected, Some(snap));
            assert!(actual.is_some(), "实际指纹应存在");
        }
        other => panic!("应为 Conflict，实际: {other:?}"),
    }
    assert_eq!(
        std::fs::read(&target).unwrap(),
        b"v2-external",
        "冲突时不得覆盖他人改动"
    );
}

#[test]
fn read_fingerprint_none_when_missing() {
    let dir = temp_dir("fp_missing");
    let missing = dir.join("nope.j2");
    assert!(WriteKernel::read_fingerprint(&missing).unwrap().is_none());
}

#[test]
fn fingerprint_deterministic_and_length_sensitive() {
    let dir = temp_dir("fp_det");
    let p = dir.join("p.j2");
    std::fs::write(&p, b"abc").unwrap();
    let fp = WriteKernel::read_fingerprint(&p).unwrap().unwrap();
    assert_eq!(fp.len, 3);

    let same = FileFingerprint::of_bytes(b"abc", fp.mtime);
    assert!(fp.matches(&same), "同内容同时间应等价");
    assert_eq!(fp.as_string(), same.as_string());

    let diff = FileFingerprint::of_bytes(b"abcd", fp.mtime);
    assert!(!fp.matches(&diff), "内容变化应不等价");
}

// ---------------------------------------------------------------------------
// 路径穿越 / 符号链接逃逸
// ---------------------------------------------------------------------------

#[test]
fn validate_asset_name_shared_rules() {
    assert!(validate_asset_name("my_op").is_ok());
    assert!(validate_asset_name("钻_孔循环").is_ok());
    for bad in ["", ".", "..", "a/b", "../evil", r"..\evil"] {
        assert!(validate_asset_name(bad).is_err(), "{bad} 应被拒绝");
    }
}

#[test]
fn safe_path_rejects_traversal() {
    let dir = temp_dir("safe_traversal");
    let sp = SafePath::from_root(&dir).unwrap();
    for bad in ["../evil", "a/b", "..", ".", "", r"..\evil"] {
        let err = sp.resolve(bad).unwrap_err();
        assert!(
            matches!(err, WriteError::PathEscape { .. }),
            "{bad} 应判为路径越界，实际: {err:?}"
        );
    }
    // 合法名称落在根内
    let ok = sp.resolve("demo.j2").unwrap();
    assert!(ok.starts_with(sp.root()));
}

#[cfg(unix)]
#[test]
fn symlink_escape_rejected() {
    let root = temp_dir("symlink_root");
    let outside = temp_dir("symlink_outside");
    let link = root.join("escape");
    std::os::unix::fs::symlink(&outside, &link).expect("创建符号链接失败");

    let sp = SafePath::from_root(&root).unwrap();
    let err = sp.resolve("escape").unwrap_err();
    assert!(
        matches!(err, WriteError::PathEscape { .. }),
        "指向根外的符号链接应被拒绝，实际: {err:?}"
    );
}

// ---------------------------------------------------------------------------
// 只读
// ---------------------------------------------------------------------------

/// Unix：把父目录设为只读，临时文件创建失败 → `ReadOnly`。
#[cfg(unix)]
#[test]
fn read_only_dir_maps_to_readonly() {
    use std::os::unix::fs::PermissionsExt;

    let dir = temp_dir("ro_dir");
    let target = dir.join("t.j2");

    let mut perms = std::fs::metadata(&dir).unwrap().permissions();
    perms.set_mode(0o555);
    std::fs::set_permissions(&dir, perms).unwrap();

    let err = WriteKernel::write_atomic(&target, b"x").unwrap_err();

    // 恢复权限，保证临时目录可清理
    let mut perms = std::fs::metadata(&dir).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&dir, perms).unwrap();

    assert!(
        matches!(err, WriteError::ReadOnly { .. }),
        "只读目录应归 ReadOnly，实际: {err:?}"
    );
}

/// Windows：把**目标文件**设为只读，`rename` 覆盖失败 → `ReadOnly`
/// （Windows 上目录的只读属性不阻止在其中创建文件，故改测只读目标）。
#[cfg(windows)]
#[test]
// 该 lint 针对 Unix 语义（world-writable）；本用例仅在 Windows 编译，恢复只读属性是必要的清理。
#[allow(clippy::permissions_set_readonly_false)]
fn read_only_target_maps_to_readonly() {
    let dir = temp_dir("ro_target");
    let target = dir.join("t.j2");
    std::fs::write(&target, b"old").unwrap();

    let mut perms = std::fs::metadata(&target).unwrap().permissions();
    perms.set_readonly(true);
    std::fs::set_permissions(&target, perms).unwrap();

    let err = WriteKernel::write_atomic(&target, b"new").unwrap_err();

    // 恢复属性，保证临时目录可清理
    let mut perms = std::fs::metadata(&target).unwrap().permissions();
    perms.set_readonly(false);
    std::fs::set_permissions(&target, perms).unwrap();

    assert!(
        matches!(err, WriteError::ReadOnly { .. }),
        "只读目标应归 ReadOnly，实际: {err:?}"
    );
    assert_eq!(
        std::fs::read(&target).unwrap(),
        b"old",
        "只读目标内容不得变"
    );
    assert!(!has_temp_residue(&dir), "失败后不应残留临时文件");
}

// ---------------------------------------------------------------------------
// 损坏输入
// ---------------------------------------------------------------------------

#[test]
fn read_fingerprint_on_directory_is_corrupt() {
    let dir = temp_dir("corrupt_dir");
    let err = WriteKernel::read_fingerprint(&dir).unwrap_err();
    assert!(
        matches!(err, WriteError::Corrupt(_)),
        "目录不是普通文件，应报 Corrupt，实际: {err:?}"
    );
}
