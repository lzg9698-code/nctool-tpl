//! QA（严过关）**第 2 轮独立复验**：CLI 侧的 P2-1（损坏清单降级）与
//! P2-2（`new` 跑 L1/L2）。
//!
//! 与工程师 `cli_edit_e2e.rs` 的区别：这里**三条命令全验**（new / derive / rename），
//! 并补一条**反向**用例（合法清单不得被误判为损坏），以及 `new` 的 stdout
//! "不得新增级别声明行"。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use assert_cmd::Command;

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

fn run_in(dir: &Path, args: &[&str]) -> Run {
    let output = Command::new(env!("CARGO_BIN_EXE_nctool"))
        .args(args)
        .current_dir(dir)
        .assert()
        .get_output()
        .clone();
    Run {
        code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

fn temp_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("nctool_qa2_{}_{}_{}", std::process::id(), tag, n));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("建临时目录");
    dir
}

fn write(p: &Path, s: &str) {
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).expect("建父目录");
    }
    std::fs::write(p, s).expect("写文件");
}

fn args_with_root<'a>(root: &'a str, tail: &'a [&'a str]) -> Vec<&'a str> {
    let mut v = vec!["--template-dir", root];
    v.extend_from_slice(tail);
    v
}

/// 损坏清单（非法 YAML：未闭合 flow 序列）。
const CORRUPT: &str = "templates:\n  \"a.j2\":\n    name: \"A\"\n  bad: [\n";

// ---------------------------------------------------------------------------
// P2-1：损坏清单 → 三条命令都必须「警告 + 零字节改动 + 退出码 0 + 文件照常处理」
// ---------------------------------------------------------------------------

#[test]
fn qa_corrupt_manifest_new_degrades_and_keeps_bytes() {
    let work = temp_dir("qa2_c_new");
    let root = work.join("templates");
    write(&root.join("keep.j2"), "G0 X0\n");
    write(&root.join("templates.yaml"), CORRUPT);

    let r = run_in(
        &work,
        &args_with_root(
            root.to_str().unwrap(),
            &["templates", "new", "demo", "--dir", root.to_str().unwrap()],
        ),
    );
    assert_eq!(r.code, 0, "损坏清单不得阻断：{}", r.stderr);
    assert!(root.join("demo.j2").exists(), "模板文件应照常创建");
    assert!(
        r.stderr.contains("解析失败") || r.stderr.contains("清单"),
        "应有降级警告：{}",
        r.stderr
    );
    assert_eq!(
        std::fs::read_to_string(root.join("templates.yaml")).unwrap(),
        CORRUPT,
        "损坏清单必须一字节不改"
    );
}

#[test]
fn qa_corrupt_manifest_derive_degrades_and_keeps_bytes() {
    let work = temp_dir("qa2_c_derive");
    let root = work.join("templates");
    write(&root.join("a.j2"), "G0 X{{ x }}\n");
    write(&root.join("templates.yaml"), CORRUPT);

    let r = run_in(
        &work,
        &args_with_root(
            root.to_str().unwrap(),
            &["templates", "derive", "a.j2", "b"],
        ),
    );
    assert_eq!(r.code, 0, "损坏清单不得阻断 derive：{}", r.stderr);
    assert!(root.join("b.j2").exists(), "派生文件应照常创建");
    assert!(
        r.stderr.contains("解析失败") || r.stderr.contains("清单"),
        "应有降级警告：{}",
        r.stderr
    );
    assert_eq!(
        std::fs::read_to_string(root.join("templates.yaml")).unwrap(),
        CORRUPT,
        "损坏清单必须一字节不改"
    );
}

#[test]
fn qa_corrupt_manifest_rename_degrades_and_keeps_bytes() {
    let work = temp_dir("qa2_c_rename");
    let root = work.join("templates");
    write(&root.join("a.j2"), "G0 X0\n");
    write(&root.join("templates.yaml"), CORRUPT);

    let r = run_in(
        &work,
        &args_with_root(
            root.to_str().unwrap(),
            &["templates", "rename", "a.j2", "b"],
        ),
    );
    assert_eq!(r.code, 0, "损坏清单不得阻断 rename：{}", r.stderr);
    assert!(root.join("b.j2").exists(), "新文件应存在");
    assert!(!root.join("a.j2").exists(), "旧文件应被移动");
    assert!(
        r.stderr.contains("解析失败") || r.stderr.contains("清单"),
        "应有降级警告：{}",
        r.stderr
    );
    assert_eq!(
        std::fs::read_to_string(root.join("templates.yaml")).unwrap(),
        CORRUPT,
        "损坏清单必须一字节不改"
    );
}

/// **反向（防过度降级）**：合法清单**不得**被误判为损坏而拒绝写入。
#[test]
fn qa_valid_manifest_is_written_not_falsely_degraded() {
    let work = temp_dir("qa2_valid");
    let root = work.join("templates");
    write(&root.join("keep.j2"), "G0 X0\n");
    let valid = "# 头注释\ntemplates:\n  \"keep.j2\":\n    name: \"保留\"\n";
    write(&root.join("templates.yaml"), valid);

    let r = run_in(
        &work,
        &args_with_root(
            root.to_str().unwrap(),
            &["templates", "new", "fresh", "--dir", root.to_str().unwrap()],
        ),
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    let after = std::fs::read_to_string(root.join("templates.yaml")).unwrap();
    assert!(
        after.contains("\"fresh.j2\":"),
        "合法清单应被正常写入新条目（不得误判损坏）:\n{after}"
    );
    // 原有行保留
    for line in valid.lines() {
        assert!(after.contains(line), "原有行丢失: {line:?}\n{after}");
    }
    assert!(
        !r.stderr.contains("解析失败"),
        "合法清单不得报解析失败：{}",
        r.stderr
    );
}

// ---------------------------------------------------------------------------
// P2-2：`new` 跑 L1/L2 且不新增 stdout 级别声明行
// ---------------------------------------------------------------------------

/// 清单声明 `demo.j2` 但规格悬空（`derive.from` 指向不存在的参数）→
/// `new demo` 必须在**写盘前**被拦下（退出码 1）且不创建文件。
#[test]
fn qa_new_blocks_dangling_derive_spec_without_creating_file() {
    let work = temp_dir("qa2_new_l2");
    let root = work.join("templates");
    write(
        &root.join("templates.yaml"),
        "templates:\n  \"demo.j2\":\n    params:\n      - name: x\n        kind: number\n        derive: { from: no_such_src, table: [] }\n",
    );

    let r = run_in(
        &work,
        &args_with_root(
            root.to_str().unwrap(),
            &["templates", "new", "demo", "--dir", root.to_str().unwrap()],
        ),
    );
    assert_eq!(r.code, 1, "L2 悬空引用应在写盘前阻断：{}", r.stderr);
    assert!(r.stderr.contains("L2"), "stderr 应标注 L2：{}", r.stderr);
    assert!(!root.join("demo.j2").exists(), "被阻断时不得创建文件");
}

/// `new` 的 stdout **不得**新增"级别声明行"（声明义务只落在 edit/derive）。
/// 对照组：`edit` 的 stdout 必须**有**声明行。
#[test]
fn qa_new_stdout_has_no_level_declaration_but_edit_does() {
    let work = temp_dir("qa2_new_stdout");
    let root = work.join("templates");
    write(&root.join("keep.j2"), "G0 X0\n");
    write(
        &root.join("templates.yaml"),
        "templates:\n  \"keep.j2\":\n    name: \"保留\"\n",
    );

    let new = run_in(
        &work,
        &args_with_root(
            root.to_str().unwrap(),
            &["templates", "new", "fresh", "--dir", root.to_str().unwrap()],
        ),
    );
    assert_eq!(new.code, 0, "{}", new.stderr);
    for needle in ["L1", "L2", "L3", "校验", "未提供参数", "未执行参数值校验"] {
        assert!(
            !new.stdout.contains(needle),
            "`new` 的 stdout 不得出现级别声明 {needle:?}：\n{}",
            new.stdout
        );
    }

    // 对照组：edit 必须声明
    let newf = work.join("new.j2");
    write(&newf, "G0 Y{{ y }}\n");
    let edit = run_in(
        &work,
        &args_with_root(
            root.to_str().unwrap(),
            &[
                "templates",
                "edit",
                "keep.j2",
                "--from-file",
                newf.to_str().unwrap(),
            ],
        ),
    );
    assert_eq!(edit.code, 0, "{}", edit.stderr);
    assert!(
        edit.stdout.contains("L1") && edit.stdout.contains("未提供参数"),
        "`edit` 的 stdout 应有级别声明：\n{}",
        edit.stdout
    );
}
