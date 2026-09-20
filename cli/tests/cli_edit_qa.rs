//! QA（严过关）对 T02「模块一 · 模板编辑」的**独立对抗性回归**用例。
//!
//! 本文件由 QA 新增，补充 `cli_edit_e2e.rs` 未覆盖的 L2 覆盖面与一个**已定位缺陷**
//! 的复现。命名以 `qa_` 前缀与工程师用例区分。
//!
//! 用例分两类：
//! - **普通用例**（当前应绿）：钉住 QA 实测确认的行为；
//! - **P1 回归闸门**（`qa_derive_adjacent_entries_must_not_duplicate_sibling`）：
//!   曾复现"`manifest_entry_body` 吞并相邻兄弟条目 → `derive` 复制出重复键 →
//!   整份清单静默失效"的 P1 缺陷。P1 修复后已**去掉 `#[ignore]`**，转为永久闸门。

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
    let dir = std::env::temp_dir().join(format!("nctool_qa_{}_{}_{}", std::process::id(), tag, n));
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

// ---------------------------------------------------------------------------
// L2 覆盖面（工程师 E2E 只覆盖了 default 一项；这里补 required_if / derive）
// ---------------------------------------------------------------------------

/// L2：`required_if` 的**控制参数未在本规格中声明** → 阻断（退出码 1）。
#[test]
fn qa_l2_blocks_dangling_required_if_control() {
    let work = temp_dir("qa_l2_ri");
    let root = work.join("templates");
    write(&root.join("t.j2"), "{{ x }}\n");
    write(
        &root.join("templates.yaml"),
        "templates:\n  \"t.j2\":\n    params:\n      - name: x\n        kind: number\n        required_if: { param: no_such_ctrl, values: [\"Right\"] }\n",
    );
    let new = work.join("new.j2");
    write(&new, "{{ x }}\n");
    let r = run_in(
        &work,
        &args_with_root(
            root.to_str().unwrap(),
            &[
                "templates",
                "edit",
                "t.j2",
                "--from-file",
                new.to_str().unwrap(),
            ],
        ),
    );
    assert_eq!(r.code, 1, "悬空 required_if 控制参数应阻断：{}", r.stderr);
    assert!(r.stderr.contains("L2"), "stderr 应标注 L2：{}", r.stderr);
}

/// L2：`derive` 的**源参数未在本规格中声明** → 阻断（退出码 1）。
#[test]
fn qa_l2_blocks_dangling_derive_source() {
    let work = temp_dir("qa_l2_dv");
    let root = work.join("templates");
    write(&root.join("t.j2"), "{{ x }}\n");
    write(
        &root.join("templates.yaml"),
        "templates:\n  \"t.j2\":\n    params:\n      - name: x\n        kind: number\n        derive: { from: no_such_src, table: [] }\n",
    );
    let new = work.join("new.j2");
    write(&new, "{{ x }}\n");
    let r = run_in(
        &work,
        &args_with_root(
            root.to_str().unwrap(),
            &[
                "templates",
                "edit",
                "t.j2",
                "--from-file",
                new.to_str().unwrap(),
            ],
        ),
    );
    assert_eq!(r.code, 1, "悬空 derive 源参数应阻断：{}", r.stderr);
    assert!(r.stderr.contains("L2"), "stderr 应标注 L2：{}", r.stderr);
}

// ---------------------------------------------------------------------------
// L2 关键反例：规格完全自洽、但必选参数一个都没提供 → 必须成功（证明 L2 不是
// "以空参数调用 validate"）
// ---------------------------------------------------------------------------

/// 规格自洽（a、b 均 number、无 default、无悬空引用），**一个参数都不提供** →
/// 保存必须成功（退出码 0）。若被阻断，说明 L2 走成了"空参数调 validate"。
#[test]
fn qa_l2_consistent_spec_with_no_params_still_saves() {
    let work = temp_dir("qa_l2_ok");
    let root = work.join("templates");
    write(&root.join("t.j2"), "{{ a }} {{ b }}\n");
    write(
        &root.join("templates.yaml"),
        "templates:\n  \"t.j2\":\n    params:\n      - name: a\n        kind: number\n      - name: b\n        kind: number\n",
    );
    let new = work.join("new.j2");
    write(&new, "G0 X{{ a }} Y{{ b }}\n");
    let r = run_in(
        &work,
        &args_with_root(
            root.to_str().unwrap(),
            &[
                "templates",
                "edit",
                "t.j2",
                "--from-file",
                new.to_str().unwrap(),
            ],
        ),
    );
    assert_eq!(
        r.code, 0,
        "规格自洽但未提供参数必须能保存（L2 不得走空参数 validate）：{}",
        r.stderr
    );
    assert!(r.stdout.contains("未提供参数"), "文本通道应明示未做 L3");
}

/// JSON 通道：提供了参数时 `validationLevels` 必须是**三元素** `["L1","L2","L3"]`。
#[test]
fn qa_json_validation_levels_three_when_params_given() {
    let work = temp_dir("qa_json3");
    let root = work.join("templates");
    write(&root.join("t.j2"), "{{ x }}\n");
    write(
        &root.join("templates.yaml"),
        "templates:\n  \"t.j2\":\n    params:\n      - name: x\n        kind: choice\n        options: [\"A\", \"B\"]\n",
    );
    let new = work.join("new.j2");
    write(&new, "G0 X{{ x }}\n");
    let r = run_in(
        &work,
        &args_with_root(
            root.to_str().unwrap(),
            &[
                "--format",
                "json",
                "templates",
                "edit",
                "t.j2",
                "--from-file",
                new.to_str().unwrap(),
                "--param",
                "x=A",
            ],
        ),
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    let v: serde_json::Value = serde_json::from_str(&r.stdout).expect("stdout 应是合法 JSON");
    assert_eq!(
        v["data"]["validationLevels"],
        serde_json::json!(["L1", "L2", "L3"])
    );
}

// ---------------------------------------------------------------------------
// P1 缺陷复现（#[ignore]：修复后请去掉 ignore）
// ---------------------------------------------------------------------------

/// **P1 复现**：清单里两条**相邻**条目（合法 YAML，无空行分隔）时，`derive`
/// 会把**后一条兄弟条目**当成源条目的字段一并复制，产生**重复键**，使整份清单
/// 被 `serde_yaml` 判为非法（`duplicate entry`），**该清单的全部元数据静默丢失**。
///
/// 触发输入是**完全合法**的 YAML（相邻兄弟键）。根因：`manifest_entry_body`
/// 只在空行 / 顶格行处停止，不在**同级键行**处停止。
///
/// P1 已修复（`manifest_entry_body` 现在在"缩进不严格大于键行"处停止），故本用例
/// **已转为永久回归闸门**（去掉 `#[ignore]`）。断言未削弱——它仍是缺陷复现证据。
#[test]
fn qa_derive_adjacent_entries_must_not_duplicate_sibling() {
    let work = temp_dir("qa_adj");
    let root = work.join("templates");
    write(&root.join("a.j2"), "G0 X{{ x }}\n");
    write(&root.join("b.j2"), "G0 Y{{ y }}\n");
    // 合法 YAML：两条相邻兄弟条目，中间无空行
    write(
        &root.join("templates.yaml"),
        "templates:\n  \"a.j2\":\n    name: \"A\"\n  \"b.j2\":\n    name: \"B\"\n",
    );
    let r = run_in(
        &work,
        &args_with_root(
            root.to_str().unwrap(),
            &["templates", "derive", "a.j2", "anew"],
        ),
    );
    assert_eq!(r.code, 0, "{}", r.stderr);

    let manifest = std::fs::read_to_string(root.join("templates.yaml")).unwrap();
    let dup = manifest.matches("\"b.j2\":").count();
    assert_eq!(
        dup, 1,
        "派生不得复制出重复的兄弟键 \"b.j2\"（当前 {dup} 次）:\n{manifest}"
    );

    // 清单必须仍可被工具加载（无 "duplicate entry" 警告）
    let ls = run_in(
        &work,
        &args_with_root(root.to_str().unwrap(), &["templates", "list"]),
    );
    assert!(
        !ls.stderr.contains("duplicate"),
        "清单被复制成非法 YAML（duplicate entry）：{}",
        ls.stderr
    );
}
