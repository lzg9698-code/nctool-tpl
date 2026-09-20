//! T02 模块一（模板编辑）E2E：`templates edit/derive/rename` 与 `new` 的清单落盘。
//!
//! 本文件是**新增**契约测试，不触碰既有 `cli_e2e.rs` 的 44 个用例。
//! 覆盖 PRD §3.4 的 AC-1.1 ~ AC-1.11。
//!
//! 统一在**隔离的临时模板目录**中运行（`--template-dir`），避免污染仓库工作区。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use assert_cmd::Command;

/// 一次运行的完整结果。
struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

impl Run {
    fn stdout_contains(&self, needles: &[&str]) -> &Self {
        for n in needles {
            assert!(
                self.stdout.contains(n),
                "stdout 缺少 {n:?}\n--- exit {} ---\nstdout:\n{}\nstderr:\n{}",
                self.code,
                self.stdout,
                self.stderr
            );
        }
        self
    }

    fn stderr_contains(&self, needles: &[&str]) -> &Self {
        for n in needles {
            assert!(
                self.stderr.contains(n),
                "stderr 缺少 {n:?}\n--- exit {} ---\nstdout:\n{}\nstderr:\n{}",
                self.code,
                self.stdout,
                self.stderr
            );
        }
        self
    }
}

/// 在指定工作目录执行 `nctool <args>`。
fn run_in(dir: &Path, args: &[&str]) -> Run {
    let out = Command::new(env!("CARGO_BIN_EXE_nctool"))
        .args(args)
        .current_dir(dir)
        .assert();
    let output = out.get_output();
    Run {
        code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

/// 建唯一临时目录（位于系统临时区）。
fn temp_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir =
        std::env::temp_dir().join(format!("nctool_edit_{}_{}_{}", std::process::id(), tag, n));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("创建临时目录失败");
    dir
}

/// 在 `root` 下建模板目录并写入一个模板，返回 `(工作目录, 模板根, 模板文件路径)`。
fn setup_template(tag: &str, rel: &str, source: &str) -> (PathBuf, PathBuf, PathBuf) {
    let work = temp_dir(tag);
    let root = work.join("templates");
    let file = root.join(rel);
    std::fs::create_dir_all(file.parent().unwrap()).expect("建模板子目录");
    std::fs::write(&file, source).expect("写模板");
    (work, root, file)
}

/// 写一份清单到模板根。
fn write_manifest(root: &Path, text: &str) {
    std::fs::write(root.join("templates.yaml"), text).expect("写清单");
}

/// 断言目录内无 `.nctool-tmp-` 残留（AC-1.10 的"无半成品"）。
fn assert_no_tmp_residue(root: &Path) {
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).expect("读目录") {
            let p = e.expect("目录项").path();
            if p.is_dir() {
                stack.push(p);
            } else {
                let name = p.file_name().unwrap().to_string_lossy();
                assert!(
                    !name.contains(".nctool-tmp-"),
                    "发现临时文件残留: {}",
                    p.display()
                );
            }
        }
    }
}

/// 把 `--template-dir` 与子命令拼成参数表。
fn args_with_root<'a>(root: &'a str, tail: &'a [&'a str]) -> Vec<&'a str> {
    let mut v = vec!["--template-dir", root];
    v.extend_from_slice(tail);
    v
}

// ---------------------------------------------------------------------------
// AC-1.4 / AC-1.5 / AC-1.6 / AC-1.7：edit
// ---------------------------------------------------------------------------

/// AC-1.4：保存后 `render` 输出与"手工改文件后 render"**逐字节一致**。
#[test]
fn edit_save_matches_manual_file_edit() {
    let src0 = "G0 X{{ x | nc_fixed(3) }}\nM30\n";
    let src1 = "G0 Y{{ y | nc_fixed(3) }}\nM30\n";

    // (a) 经 edit 保存
    let (work_a, root_a, _f) = setup_template("edit_byte", "t.j2", src0);
    let new_file = work_a.join("new.j2");
    std::fs::write(&new_file, src1).unwrap();
    let r = run_in(
        &work_a,
        &args_with_root(
            root_a.to_str().unwrap(),
            &[
                "templates",
                "edit",
                "t.j2",
                "--from-file",
                new_file.to_str().unwrap(),
            ],
        ),
    );
    assert_eq!(r.code, 0, "编辑应成功：{}", r.stderr);
    let render_a = run_in(
        &work_a,
        &args_with_root(
            root_a.to_str().unwrap(),
            &["render", "t.j2", "--param", "y=12.5"],
        ),
    );
    assert_eq!(render_a.code, 0, "{}", render_a.stderr);

    // (b) 手工把同一内容写进另一个目录再 render
    let (work_b, root_b, _f) = setup_template("manual_byte", "t.j2", src1);
    let render_b = run_in(
        &work_b,
        &args_with_root(
            root_b.to_str().unwrap(),
            &["render", "t.j2", "--param", "y=12.5"],
        ),
    );
    assert_eq!(render_b.code, 0, "{}", render_b.stderr);

    assert_eq!(
        render_a.stdout, render_b.stdout,
        "edit 保存后的 render 必须与手工改文件后逐字节一致"
    );
    assert_no_tmp_residue(&root_a);
}

/// AC-1.5：保存语法错误的模板被**阻断**，给出行列，退出码 1，且文件未变。
#[test]
fn edit_syntax_error_blocked_exit_1() {
    let original = "G0 X0\n";
    let (work, root, file) = setup_template("edit_syntax", "t.j2", original);
    let bad = work.join("bad.j2");
    std::fs::write(&bad, "{% if %}\n").unwrap();
    let r = run_in(
        &work,
        &args_with_root(
            root.to_str().unwrap(),
            &[
                "templates",
                "edit",
                "t.j2",
                "--from-file",
                bad.to_str().unwrap(),
            ],
        ),
    );
    assert_eq!(r.code, 1, "语法错误应阻断（validation=1）：{}", r.stderr);
    r.stderr_contains(&["L1", "行", "列"]);
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        original,
        "校验失败不得落盘"
    );
    assert_no_tmp_residue(&root);
}

/// L2 规格自洽：清单覆盖层声明 `default` 不在白名单 → 阻断（退出码 1）。
#[test]
fn edit_spec_inconsistency_blocked_exit_1() {
    let (work, root, file) = setup_template("edit_spec", "t.j2", "{{ x }}\n");
    write_manifest(
        &root,
        "templates:\n  \"t.j2\":\n    params:\n      - name: x\n        kind: choice\n        options: [\"A\"]\n        default: \"B\"\n",
    );
    let before = std::fs::read_to_string(&file).unwrap();
    let new = work.join("new.j2");
    std::fs::write(&new, "{{ x }}\n").unwrap();
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
    assert_eq!(r.code, 1, "规格不自洽应阻断：{}", r.stderr);
    r.stderr_contains(&["L2"]);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), before);
}

/// 分级校验**明示**：无参数时声明只做了 L1/L2；有参数时声明做了 L3。
#[test]
fn edit_declares_which_validation_levels_ran() {
    let (work, root, _f) = setup_template("edit_tier", "t.j2", "{{ x }}\n");

    let new = work.join("new.j2");
    std::fs::write(&new, "G0 X{{ x }}\n").unwrap();

    // 未提供参数 → 只做 L1/L2，且输出**必须**明示
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
    assert_eq!(r.code, 0, "{}", r.stderr);
    r.stdout_contains(&["未提供参数", "未执行参数值校验"]);

    // 提供参数 → 走 L3
    let r2 = run_in(
        &work,
        &args_with_root(
            root.to_str().unwrap(),
            &[
                "templates",
                "edit",
                "t.j2",
                "--from-file",
                new.to_str().unwrap(),
                "--param",
                "x=3",
            ],
        ),
    );
    assert_eq!(r2.code, 0, "{}", r2.stderr);
    r2.stdout_contains(&["L3"]);
}

/// L3 完整参数校验：提供了参数且取值非法（Choice 越白名单）→ 阻断（退出码 1），
/// 且文件未变。证明"提供了参数就会真的走完整校验"，不是走过场。
#[test]
fn edit_l3_param_error_blocked_exit_1() {
    let (work, root, file) = setup_template("edit_l3", "t.j2", "{{ x }}\n");
    write_manifest(
        &root,
        "templates:\n  \"t.j2\":\n    params:\n      - name: x\n        kind: choice\n        options: [\"A\", \"B\"]\n",
    );
    let before = std::fs::read_to_string(&file).unwrap();
    let new = work.join("new.j2");
    std::fs::write(&new, "G0 X{{ x }}\n").unwrap();
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
                "--param",
                "x=C",
            ],
        ),
    );
    assert_eq!(r.code, 1, "L3 参数非法应阻断：{}", r.stderr);
    r.stderr_contains(&["L3"]);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), before, "不得落盘");
}

/// AC-1.7：并发冲突（`--expect-hash` 不匹配）→ 不静默覆盖，退出码 6。
#[test]
fn edit_expect_hash_mismatch_exits_6() {
    let (work, root, file) = setup_template("edit_conflict", "t.j2", "G0 X0\n");
    let before = std::fs::read_to_string(&file).unwrap();
    let new = work.join("new.j2");
    std::fs::write(&new, "G0 X1\n").unwrap();
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
                "--expect-hash",
                "fnv1a64:0000000000000000",
            ],
        ),
    );
    assert_eq!(r.code, 6, "指纹不匹配应判并发冲突：{}", r.stderr);
    r.stderr_contains(&["冲突"]);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), before, "不得覆盖");
}

/// AC-1.6：保存后必选参数变化能被 `inspect` 正确反映。
#[test]
fn edit_then_inspect_reflects_new_param() {
    let (work, root, _f) = setup_template("edit_inspect", "t.j2", "{{ a }}\n");
    let new = work.join("new.j2");
    std::fs::write(&new, "{{ a }} {{ b }}\n").unwrap();
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
    assert_eq!(r.code, 0, "{}", r.stderr);
    let ins = run_in(
        &work,
        &args_with_root(root.to_str().unwrap(), &["inspect", "t.j2"]),
    );
    assert_eq!(ins.code, 0, "{}", ins.stderr);
    ins.stdout_contains(&["a", "b"]);
}

// ---------------------------------------------------------------------------
// AC-1.8：derive
// ---------------------------------------------------------------------------

/// AC-1.8：派生出的新模板源码与清单条目齐备，`list` 可见、`validate` 通过。
#[test]
fn derive_creates_template_and_manifest_entry() {
    let (work, root, _f) = setup_template("derive", "t.j2", "{# NAME: 源 #}\nG0 X{{ x }}\n");
    write_manifest(
        &root,
        "# 头部注释\ntemplates:\n  \"t.j2\":\n    name: \"源\"\n    params:\n      - name: x\n        kind: number\n        min: 0\n",
    );

    let r = run_in(
        &work,
        &args_with_root(
            root.to_str().unwrap(),
            &["templates", "derive", "t.j2", "derived"],
        ),
    );
    assert_eq!(r.code, 0, "派生应成功：{}", r.stderr);

    // 新文件存在且头部 NAME 已更新、带"派生自"注释
    let new_file = root.join("derived.j2");
    assert!(new_file.exists(), "应生成 derived.j2");
    let body = std::fs::read_to_string(&new_file).unwrap();
    assert!(body.contains("{# NAME: derived #}"), "{body}");
    assert!(body.contains("派生自 t.j2"), "{body}");

    // 清单：新条目出现，且**非目标字节不变**（原有行仍在）
    let manifest = std::fs::read_to_string(root.join("templates.yaml")).unwrap();
    assert!(manifest.contains("\"derived.j2\":"), "{manifest}");
    assert!(manifest.contains("# 头部注释"), "原有注释不得丢失");
    assert!(manifest.contains("min: 0"), "覆盖层应被复制");

    // list 可见
    let ls = run_in(
        &work,
        &args_with_root(root.to_str().unwrap(), &["templates", "list"]),
    );
    ls.stdout_contains(&["derived.j2"]);

    // validate 通过
    let v = run_in(
        &work,
        &args_with_root(
            root.to_str().unwrap(),
            &["validate", "derived.j2", "--param", "x=1"],
        ),
    );
    assert_eq!(v.code, 0, "派生模板应可校验通过：{}", v.stderr);
}

/// 派生到已存在的名字 → `name_conflict`(6)。
#[test]
fn derive_duplicate_exits_6() {
    let (work, root, _f) = setup_template("derive_dup", "t.j2", "G0 X{{ x }}\n");
    std::fs::write(root.join("taken.j2"), "G0 X0\n").unwrap();
    let r = run_in(
        &work,
        &args_with_root(
            root.to_str().unwrap(),
            &["templates", "derive", "t.j2", "taken"],
        ),
    );
    assert_eq!(r.code, 6, "重名应归 name_conflict(6)：{}", r.stderr);
    // 原文件未被覆盖
    assert_eq!(
        std::fs::read_to_string(root.join("taken.j2")).unwrap(),
        "G0 X0\n"
    );
}

/// AC-1.3：派生新名含路径分隔符被拒绝（args=2）。
#[test]
fn derive_rejects_path_separator_name() {
    let (work, root, _f) = setup_template("derive_trav", "t.j2", "G0 X0\n");
    for bad in ["../escape", "a/b", r"..\escape"] {
        let r = run_in(
            &work,
            &args_with_root(
                root.to_str().unwrap(),
                &["templates", "derive", "t.j2", bad],
            ),
        );
        assert_eq!(r.code, 2, "非法新名 {bad} 应被拒绝：{}", r.stderr);
    }
}

// ---------------------------------------------------------------------------
// AC-1.9：rename
// ---------------------------------------------------------------------------

/// AC-1.9：重命名后 list 显示新名、旧名消失；存在 include 引用时给出警告清单。
#[test]
fn rename_updates_listing_and_warns_stale_includes() {
    let (work, root, _f) = setup_template("rename", "a.j2", "G0 X{{ x }}\n");
    // 另一个模板 include 了 a.j2
    std::fs::write(root.join("user.j2"), "{% include \"a.j2\" %}\n").unwrap();
    write_manifest(&root, "templates:\n  \"a.j2\":\n    name: \"A\"\n");

    let r = run_in(
        &work,
        &args_with_root(
            root.to_str().unwrap(),
            &["templates", "rename", "a.j2", "b"],
        ),
    );
    assert_eq!(r.code, 0, "重命名应成功：{}", r.stderr);
    assert!(!root.join("a.j2").exists(), "旧文件应消失");
    assert!(root.join("b.j2").exists(), "新文件应存在");
    r.stdout_contains(&["user.j2"]);

    // 清单键同步
    let manifest = std::fs::read_to_string(root.join("templates.yaml")).unwrap();
    assert!(manifest.contains("\"b.j2\":"));
    assert!(!manifest.contains("\"a.j2\":"));

    // list 显示新名、旧名消失
    let ls = run_in(
        &work,
        &args_with_root(root.to_str().unwrap(), &["templates", "list"]),
    );
    ls.stdout_contains(&["b.j2"]);
    assert!(!ls.stdout.contains("a.j2"), "旧名不应再出现：{}", ls.stdout);
}

/// 重命名到已存在的名字 → `name_conflict`(6)，且旧文件仍在。
#[test]
fn rename_duplicate_exits_6() {
    let (work, root, _f) = setup_template("rename_dup", "a.j2", "G0 X0\n");
    std::fs::write(root.join("b.j2"), "G0 Y0\n").unwrap();
    let r = run_in(
        &work,
        &args_with_root(
            root.to_str().unwrap(),
            &["templates", "rename", "a.j2", "b"],
        ),
    );
    assert_eq!(r.code, 6, "重名应归 name_conflict(6)：{}", r.stderr);
    assert!(root.join("a.j2").exists(), "失败后旧文件必须仍在");
    assert_eq!(
        std::fs::read_to_string(root.join("b.j2")).unwrap(),
        "G0 Y0\n",
        "目标不得被覆盖"
    );
}

/// AC-1.3：重命名新名含路径分隔符被拒绝（args=2）。
#[test]
fn rename_rejects_path_separator_name() {
    let (work, root, _f) = setup_template("rename_trav", "a.j2", "G0 X0\n");
    let r = run_in(
        &work,
        &args_with_root(
            root.to_str().unwrap(),
            &["templates", "rename", "a.j2", "../escape"],
        ),
    );
    assert_eq!(r.code, 2, "{}", r.stderr);
    assert!(root.join("a.j2").exists());
}

// ---------------------------------------------------------------------------
// AC-1.1 / AC-1.2 / AC-1.10：new
// ---------------------------------------------------------------------------

/// AC-1.1：新建模板后 `list` 立即可见；清单存在时条目被追加且非目标字节不变。
#[test]
fn new_appends_manifest_entry_without_touching_others() {
    let work = temp_dir("new_manifest");
    let root = work.join("templates");
    std::fs::create_dir_all(&root).unwrap();
    let before = "# 头部注释\ntemplates:\n  \"keep.j2\":\n    name: \"保留\"\n";
    write_manifest(&root, before);

    let r = run_in(
        &work,
        &args_with_root(
            root.to_str().unwrap(),
            &["templates", "new", "fresh", "--dir", root.to_str().unwrap()],
        ),
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(root.join("fresh.j2").exists());

    let after = std::fs::read_to_string(root.join("templates.yaml")).unwrap();
    assert!(after.contains("\"fresh.j2\":"), "{after}");
    // 原有行全部保留（非目标字节不变）
    for line in before.lines() {
        assert!(after.contains(line), "原有行丢失: {line:?}\n{after}");
    }

    let ls = run_in(
        &work,
        &args_with_root(root.to_str().unwrap(), &["templates", "list"]),
    );
    ls.stdout_contains(&["fresh.j2"]);
}

/// AC-1.2：新建同名被拒绝，不覆盖已有文件（退出码 6）。
#[test]
fn new_duplicate_keeps_existing_file() {
    let work = temp_dir("new_dup");
    let root = work.join("templates");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("dup.j2"), "G0 X0\n").unwrap();
    let r = run_in(
        &work,
        &args_with_root(
            root.to_str().unwrap(),
            &["templates", "new", "dup", "--dir", root.to_str().unwrap()],
        ),
    );
    assert_eq!(r.code, 6, "{}", r.stderr);
    assert_eq!(
        std::fs::read_to_string(root.join("dup.j2")).unwrap(),
        "G0 X0\n",
        "不得覆盖"
    );
    assert_no_tmp_residue(&root);
}

/// 尝试创建**文件**符号链接：**确认真的建成了**才返回 `true`；否则 `false`。
///
/// 关键：某些环境里 `symlink_file` 会返回 `Ok(())` 却**并未真正创建**目录项
/// （本机实测：无开发者模式时即如此，见计划 §6.7）。若只信返回值，测试会在
/// "链接根本不存在"时**假绿**。故创建后用 `symlink_metadata` **回读确认**——
/// 悬空符号链接的 lstat 仍会成功（`file_type().is_symlink()` 为真），因此该断言
/// 恰好区分"真建成"与"API 撒谎"。
fn try_symlink_file(target: &Path, link: &Path) -> bool {
    #[cfg(unix)]
    let created = std::os::unix::fs::symlink(target, link).is_ok();
    #[cfg(windows)]
    let created = std::os::windows::fs::symlink_file(target, link).is_ok();
    #[cfg(not(any(unix, windows)))]
    let created = {
        let _ = (target, link);
        false
    };
    created
        && std::fs::symlink_metadata(link)
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false)
}

/// 行为钉住（R-11 / 计划 §6.7）：`templates new` 迁移到 `write_guarded` 后，写入经
/// **原子 `rename` 替换目录项本身**，不跟随符号链接。即便目标位置是一个**悬空
/// （dangling）符号链接**（`exists()` 返回 false → `resolve` 跳过 canonicalize），
/// 写入也**不会落到模板根之外**——这正是旧 `fs::write` 直写路径的缺口，本用例钉住其闭合。
///
/// 本机可能无法创建符号链接（无特权 / 开发者模式）：该情形下**如实跳过**并打印原因，
/// **不写假绿断言**（与计划 §6.7 / R-13 的"未实测"登记一致）。
#[test]
fn new_does_not_follow_dangling_symlink_outside_root() {
    let work = temp_dir("new_symlink");
    let root = work.join("templates");
    std::fs::create_dir_all(&root).unwrap();
    let outside = work.join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    let escaped = outside.join("escaped.j2");
    let link = root.join("sneaky.j2");

    if !try_symlink_file(&escaped, &link) {
        eprintln!(
            "SKIP new_does_not_follow_dangling_symlink_outside_root: \
             本机无法创建文件符号链接（无特权/开发者模式），悬空链接向量未实测（R-13）"
        );
        return;
    }
    // 前置：链接**确实**是符号链接，且根外目标是悬空的（否则断言无意义）。
    assert!(
        std::fs::symlink_metadata(&link)
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false),
        "前置：符号链接应已真实创建"
    );
    assert!(!escaped.exists(), "前置：根外目标应为悬空（不存在）");

    let r = run_in(
        &work,
        &args_with_root(
            root.to_str().unwrap(),
            &[
                "templates",
                "new",
                "sneaky",
                "--dir",
                root.to_str().unwrap(),
            ],
        ),
    );

    // 关键不变量：写入**不得**跟随符号链接落到根外（无论本次退出码为何）。
    assert!(
        !escaped.exists(),
        "templates new 跟随了符号链接写到根外: {}（exit {}，stderr {}）",
        escaped.display(),
        r.code,
        r.stderr
    );
    assert_no_tmp_residue(&root);
}

/// 清单存在但**缺少顶层 `templates:` 块**时，`templates new` 仍创建模板文件，
/// 清单登记**降级为警告**（不阻断，D13）——非目标字节不变。
#[test]
fn new_with_manifest_lacking_top_key_warns_but_creates() {
    let work = temp_dir("new_no_top");
    let root = work.join("templates");
    std::fs::create_dir_all(&root).unwrap();
    let before = "# 只有注释\nfoo: 1\n";
    write_manifest(&root, before);

    let r = run_in(
        &work,
        &args_with_root(
            root.to_str().unwrap(),
            &["templates", "new", "fresh", "--dir", root.to_str().unwrap()],
        ),
    );
    assert_eq!(r.code, 0, "清单降级不应阻断：{}", r.stderr);
    assert!(root.join("fresh.j2").exists(), "模板文件应已创建");
    assert_eq!(
        std::fs::read_to_string(root.join("templates.yaml")).unwrap(),
        before,
        "清单不得被改动"
    );
}

/// P2-1：**损坏清单**（非法 YAML）→ `templates new` 必须**降级为警告**、
/// **一字节都不改**清单，且模板文件仍照常创建、退出码 0（D13）。
///
/// 定点文本编辑是纯文本操作；若不在改写前显式试解析，「解析失败」永远检测不到，
/// 文档承诺的降级行为就不存在——本用例钉住「检测到 + 不改字节」。
#[test]
fn new_with_corrupt_manifest_warns_and_keeps_bytes() {
    let work = temp_dir("new_corrupt");
    let root = work.join("templates");
    std::fs::create_dir_all(&root).unwrap();
    // 非法 YAML：`bad:` 打开了未闭合的 flow 序列
    let broken = "templates:\n  \"a.j2\":\n    name: \"A\"\n  bad: [\n";
    write_manifest(&root, broken);

    let r = run_in(
        &work,
        &args_with_root(
            root.to_str().unwrap(),
            &["templates", "new", "demo", "--dir", root.to_str().unwrap()],
        ),
    );
    assert_eq!(r.code, 0, "清单损坏不得阻断模板创建：{}", r.stderr);
    assert!(root.join("demo.j2").exists(), "模板文件应照常创建");
    assert_eq!(
        std::fs::read_to_string(root.join("templates.yaml")).unwrap(),
        broken,
        "损坏清单不得被静默改写（一字节都不改）"
    );
    assert!(
        r.stderr.contains("解析失败"),
        "应打印「清单解析失败」降级警告：{}",
        r.stderr
    );
}

/// P2-2：`templates new` 落盘前必须跑 **L1+L2**（§7.15）。
///
/// 构造：清单已声明 `demo.j2` 但规格**不自洽**（悬空 `required_if` 控制参数），
/// 磁盘上并无 `demo.j2`。若 `new` 真的跑了 L2，必须在**写盘前**被拦下（退出码 1）
/// 且不创建文件；若没跑，则会静默创建出一个规格不自洽的模板。
#[test]
fn new_runs_l2_and_blocks_inconsistent_spec() {
    let work = temp_dir("new_l2");
    let root = work.join("templates");
    std::fs::create_dir_all(&root).unwrap();
    write_manifest(
        &root,
        "templates:\n  \"demo.j2\":\n    params:\n      - name: x\n        kind: number\n        required_if: { param: no_such_ctrl, values: [\"R\"] }\n",
    );

    let r = run_in(
        &work,
        &args_with_root(
            root.to_str().unwrap(),
            &["templates", "new", "demo", "--dir", root.to_str().unwrap()],
        ),
    );
    assert_eq!(r.code, 1, "L2 不自洽应在写盘前阻断：{}", r.stderr);
    assert!(!root.join("demo.j2").exists(), "被阻断时不得创建模板文件");
}

/// P3-b：`templates new x` 当 `x.j2` 是**目录**（占位）时 → 退出码 3，且文案须说明
/// 「目标位置已被占用」，**不得**用误导的「数据损坏」（没有任何东西损坏）。
#[test]
fn new_when_target_is_directory_reports_occupied_not_corrupt() {
    let work = temp_dir("new_dir_occupied");
    let root = work.join("templates");
    // 用同名目录占位 `demo.j2`
    std::fs::create_dir_all(root.join("demo.j2")).unwrap();

    let r = run_in(
        &work,
        &args_with_root(
            root.to_str().unwrap(),
            &["templates", "new", "demo", "--dir", root.to_str().unwrap()],
        ),
    );
    assert_eq!(r.code, 3, "目标被占用归 io(3)：{}", r.stderr);
    assert!(
        r.stderr.contains("已被占用"),
        "文案应说明位置被占用：{}",
        r.stderr
    );
    assert!(
        !r.stderr.contains("数据损坏"),
        "不得再用误导的「数据损坏」：{}",
        r.stderr
    );
}

/// AC-1.10：写盘失败（目标不可写）时**无半成品残留**。
///
/// 用"把目标文件设为只读"制造可写性失败；部分平台/文件系统可能仍允许覆盖，
/// 此时退化为"成功且无残留"——两种结局都断言**无 `.nctool-tmp-` 残留**，
/// 因此用例在任何平台都有意义，不写"假绿"断言。
#[test]
fn edit_write_failure_leaves_no_residue() {
    let (work, root, file) = setup_template("edit_ro", "t.j2", "G0 X0\n");
    let new = work.join("new.j2");
    std::fs::write(&new, "G0 X1\n").unwrap();

    let mut perms = std::fs::metadata(&file).unwrap().permissions();
    perms.set_readonly(true);
    std::fs::set_permissions(&file, perms).unwrap();

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
    // 无论成功或被拒，都不得有临时文件残留
    assert_no_tmp_residue(&root);
    if r.code != 0 {
        assert_eq!(r.code, 3, "可写性失败应归 io(3)：{}", r.stderr);
    }

    // 恢复可写，避免清理失败（该 lint 面向 Unix 的 world-writable 语义，此处仅为还原）
    let mut perms = std::fs::metadata(&file).unwrap().permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    perms.set_readonly(false);
    let _ = std::fs::set_permissions(&file, perms);
}

/// AC-1.11：新增写能力不改变既有退出码/JSON 契约——`--format json` 成功包络形状不变。
#[test]
fn edit_json_envelope_shape_is_stable() {
    let (work, root, _f) = setup_template("edit_json", "t.j2", "G0 X{{ x }}\n");
    let new = work.join("new.j2");
    std::fs::write(&new, "G0 X{{ x }}\nM30\n").unwrap();
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
            ],
        ),
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    let v: serde_json::Value = serde_json::from_str(&r.stdout).expect("stdout 应是合法 JSON");
    assert_eq!(v["ok"], serde_json::json!(true));
    assert_eq!(v["data"]["action"], serde_json::json!("updated"));
    // 级别用**数组**（消费方按元素判断，无需解析拼接串）
    assert_eq!(
        v["data"]["validationLevels"],
        serde_json::json!(["L1", "L2"])
    );
}

/// **L3 行为修复**：只提供**部分**必选参数时**不得**因"缺参"而阻断。
///
/// 缺参数是使用期问题，不是模板缺陷——`check_param_values` 只校验已提供参数的值、
/// **不查缺失**。修复前这里会因模板其余必选参数报 `Missing` 而退出码 1，
/// 于是 `--param` 形同不可用、L3 等于不存在。
#[test]
fn edit_partial_params_are_not_blocked() {
    let (work, root, _f) = setup_template("edit_partial", "t.j2", "{{ a }} {{ b }}\n");
    write_manifest(
        &root,
        "templates:\n  \"t.j2\":\n    params:\n      - name: a\n        kind: number\n      - name: b\n        kind: number\n",
    );
    let new = work.join("new.j2");
    std::fs::write(&new, "G0 X{{ a }} Y{{ b }}\n").unwrap();
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
                "--param",
                "a=1",
            ],
        ),
    );
    assert_eq!(
        r.code, 0,
        "只传部分参数不得被阻断（缺参数是使用期问题）: {}",
        r.stderr
    );
    r.stdout_contains(&["L3"]);
}

/// 内置模板（无磁盘文件）不可编辑 → args(2)。
#[test]
fn edit_builtin_template_rejected() {
    let work = temp_dir("edit_builtin");
    let root = work.join("templates");
    std::fs::create_dir_all(&root).unwrap();
    let new = work.join("new.j2");
    std::fs::write(&new, "G0 X0\n").unwrap();
    let r = run_in(
        &work,
        &args_with_root(
            root.to_str().unwrap(),
            &[
                "templates",
                "edit",
                "drill_cycle",
                "--from-file",
                new.to_str().unwrap(),
            ],
        ),
    );
    assert_eq!(r.code, 2, "内置模板不可编辑：{}", r.stderr);
}

/// 不存在的模板 → template_not_found(5)。
#[test]
fn edit_missing_template_exits_5() {
    let work = temp_dir("edit_missing");
    let root = work.join("templates");
    std::fs::create_dir_all(&root).unwrap();
    let new = work.join("new.j2");
    std::fs::write(&new, "G0 X0\n").unwrap();
    let r = run_in(
        &work,
        &args_with_root(
            root.to_str().unwrap(),
            &[
                "templates",
                "edit",
                "nope.j2",
                "--from-file",
                new.to_str().unwrap(),
            ],
        ),
    );
    assert_eq!(r.code, 5, "{}", r.stderr);
}
