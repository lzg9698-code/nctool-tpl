//! T03 模块三（预设参数编辑）E2E：`preset save/list/show/rename/rm/export/import/apply`。
//!
//! 本文件是**新增**契约测试，不触碰既有 `cli_e2e.rs` 的 44 个用例。
//! 覆盖 PRD §3.6 的 AC-3.1 ~ AC-3.10。
//!
//! 统一在**隔离的临时目录**中运行：预设文件用 `--file` 指向临时区，
//! 模板目录用 `--template-dir` 指向临时区的 `templates/`，
//! 既不污染仓库工作区，也顺带验证"预设文件不得落模板根"这条红线。

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
    let dir = std::env::temp_dir().join(format!(
        "nctool_preset_{}_{}_{}",
        std::process::id(),
        tag,
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("创建临时目录失败");
    dir
}

/// 场景夹具：工作目录 + 模板根 + 预设文件路径（预设**在模板根之外**）。
struct Env {
    work: PathBuf,
    root: PathBuf,
    preset: PathBuf,
}

impl Env {
    /// 建一个含两个模板的隔离环境。
    ///
    /// - `t.j2`：`x` / `y` 必选，`feed` 可选（带默认值）
    /// - `u.j2`：只有 `x` 必选（用于跨模板复用）
    fn new(tag: &str) -> Self {
        let work = temp_dir(tag);
        let root = work.join("templates");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("t.j2"),
            "G0 X{{ x | nc_fixed(3) }} Y{{ y | nc_fixed(3) }} F{{ feed | default(100) }}\nM30\n",
        )
        .unwrap();
        std::fs::write(root.join("u.j2"), "G1 X{{ x | nc_fixed(3) }}\nM30\n").unwrap();
        Env {
            preset: work.join("presets.yaml"),
            work,
            root,
        }
    }

    /// 拼参数表：全局 `--template-dir` 在**子命令之前**。
    fn args<'a>(&'a self, tail: &[&'a str]) -> Vec<&'a str> {
        let mut v = vec!["--template-dir", self.root.to_str().unwrap()];
        v.extend_from_slice(tail);
        v
    }

    fn preset_arg(&self) -> &str {
        self.preset.to_str().unwrap()
    }

    /// 保存一个预设（全参数）。
    fn save(&self, name: &str, template: &str, params: &[&str]) -> Run {
        let mut tail = vec![
            "preset",
            "save",
            name,
            template,
            "--file",
            self.preset_arg(),
        ];
        for p in params {
            tail.push("--param");
            tail.push(p);
        }
        run_in(&self.work, &self.args(&tail))
    }
}

// ---------------------------------------------------------------------------
// AC-3.1 / AC-3.2：持久化 + CLI 可见
// ---------------------------------------------------------------------------

/// AC-3.1 + AC-3.2：保存后文件落盘，且可被 CLI 列出/导出（跨进程可见）。
#[test]
fn save_persists_and_cli_can_list_after_restart() {
    let e = Env::new("persist");
    e.save("p1", "t.j2", &["x=1", "y=2"])
        .stdout_contains(&["已保存预设: p1"]);
    assert!(e.preset.is_file(), "预设文件应已落盘");

    // 新进程读取（等价于"刷新页面 / 重启浏览器"）
    let r = run_in(
        &e.work,
        &e.args(&["preset", "list", "--file", e.preset_arg()]),
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    r.stdout_contains(&["预设列表（1 个）", "p1", "t.j2"]);

    // 导出亦可见（AC-3.2 的"CLI 列出/导出"）
    let r = run_in(
        &e.work,
        &e.args(&["preset", "export", "p1", "--file", e.preset_arg()]),
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    r.stdout_contains(&["version: 1", "name: p1", "specFingerprint"]);
}

/// 落盘形状必须是**扁平映射**（不得泄漏 `values:` 内部包装）。
#[test]
fn preset_file_shape_is_flat() {
    let e = Env::new("shape");
    e.save("p1", "t.j2", &["x=1", "y=2"]);
    let text = std::fs::read_to_string(&e.preset).unwrap();
    assert!(!text.contains("values:"), "不得出现 values: 包装：\n{text}");
    assert!(text.contains("specFingerprint:"), "{text}");
    assert!(text.contains("createdAt:"), "{text}");
}

// ---------------------------------------------------------------------------
// AC-3.9：预设值走与手填值相同的校验
// ---------------------------------------------------------------------------

/// AC-3.9：非法参数值无法保存（与手填同一套值级校验）。
#[test]
fn save_rejects_illegal_value() {
    let e = Env::new("illegal");
    // `nope` 不是该模板使用的变量 → 拒绝（否则一落盘就是脏数据）
    let r = e.save("bad", "t.j2", &["nope=1"]);
    assert_eq!(r.code, 2, "{}", r.stderr);
    r.stderr_contains(&["参数不属于模板", "nope"]);
    assert!(!e.preset.exists(), "被拒时不得落盘");
}

/// 非法预设名（`../` 穿越 / 控制字符）→ 与 rename/import 同一条
/// `validate_asset_name` 口径拒绝，且不落盘。
#[test]
fn save_rejects_illegal_name() {
    let e = Env::new("save_bad_name");
    let r = e.save("../evil", "t.j2", &["x=1"]);
    assert_eq!(r.code, 2, "{}", r.stderr);
    r.stderr_contains(&["名称非法", "../evil"]);
    assert!(!e.preset.exists(), "非法名不得创建预设文件");

    // 控制字符名：同样拒绝（否则 `preset list` 会把 ANSI/OSC 序列回显到终端）
    let r = e.save("p\u{7}1", "t.j2", &["x=1"]);
    assert_eq!(r.code, 2, "{}", r.stderr);
    r.stderr_contains(&["名称非法"]);
    assert!(!e.preset.exists(), "被拒两次后仍不得有文件");
}

/// 空参数预设无意义 → 拒绝。
#[test]
fn save_rejects_empty_params() {
    let e = Env::new("empty");
    let r = e.save("empty", "t.j2", &[]);
    assert_eq!(r.code, 2, "{}", r.stderr);
    r.stderr_contains(&["未提供任何参数"]);
}

/// 模板不存在 → template_not_found(5)。
#[test]
fn save_missing_template_exits_5() {
    let e = Env::new("missing_tpl");
    let r = e.save("p", "nope.j2", &["x=1"]);
    assert_eq!(r.code, 5, "{}", r.stderr);
}

// ---------------------------------------------------------------------------
// AC-3.3：重命名
// ---------------------------------------------------------------------------

/// AC-3.3：重命名保留参数与模板绑定；重名被拒绝(6)。
#[test]
fn rename_preserves_binding_and_rejects_duplicate() {
    let e = Env::new("rename");
    e.save("old", "t.j2", &["x=1", "y=2"]);
    e.save("other", "t.j2", &["x=9", "y=8"]);

    let r = run_in(
        &e.work,
        &e.args(&[
            "preset",
            "rename",
            "old",
            "renamed",
            "--file",
            e.preset_arg(),
        ]),
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    r.stdout_contains(&["old → renamed", "已保留"]);

    // 参数仍在（查看新名）
    let r = run_in(
        &e.work,
        &e.args(&["preset", "show", "renamed", "--file", e.preset_arg()]),
    );
    r.stdout_contains(&["x = 1", "y = 2", "绑定模板: t.j2"]);

    // 重名 → name_conflict(6)
    let r = run_in(
        &e.work,
        &e.args(&[
            "preset",
            "rename",
            "renamed",
            "other",
            "--file",
            e.preset_arg(),
        ]),
    );
    assert_eq!(r.code, 6, "{}", r.stderr);
    r.stderr_contains(&["同名预设已存在"]);
}

/// 非法新名（路径穿越）→ args(2)，且文件字节不变。
#[test]
fn rename_rejects_illegal_name() {
    let e = Env::new("rename_bad");
    e.save("p", "t.j2", &["x=1"]);
    let before = std::fs::read_to_string(&e.preset).unwrap();
    let r = run_in(
        &e.work,
        &e.args(&["preset", "rename", "p", "../evil", "--file", e.preset_arg()]),
    );
    assert_eq!(r.code, 2, "{}", r.stderr);
    assert_eq!(std::fs::read_to_string(&e.preset).unwrap(), before);
}

// ---------------------------------------------------------------------------
// AC-3.4：应用前差异预览
// ---------------------------------------------------------------------------

/// AC-3.4：`apply` 输出生效参数与陈旧检测（即差异预览）。
#[test]
fn apply_shows_effective_params_and_staleness() {
    let e = Env::new("apply");
    e.save("p", "t.j2", &["x=1", "y=2"]);
    let r = run_in(
        &e.work,
        &e.args(&["preset", "apply", "p", "--file", e.preset_arg()]),
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    r.stdout_contains(&["目标模板: t.j2", "x=1", "y=2", "陈旧检测"]);
    // R-8：安全提示不得弱化
    r.stdout_contains(&["未经真实工艺评审"]);
}

// ---------------------------------------------------------------------------
// AC-3.6：跨模板复用需显式确认
// ---------------------------------------------------------------------------

/// AC-3.6：跨模板应用列出可复用/需确认/缺失，未 `--confirm` 时不写入。
#[test]
fn cross_template_requires_confirmation() {
    let e = Env::new("cross");
    // 预设绑定 t.j2（需 x, y）；目标 u.j2 只需 x
    e.save("p", "t.j2", &["x=1", "y=2"]);

    // 未 --confirm：只展示，不生效
    let r = run_in(
        &e.work,
        &e.args(&[
            "preset",
            "apply",
            "p",
            "--template",
            "u.j2",
            "--file",
            e.preset_arg(),
        ]),
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    r.stdout_contains(&["跨模板应用: t.j2 → u.j2", "可复用"]);

    // --confirm 后生效
    let r = run_in(
        &e.work,
        &e.args(&[
            "preset",
            "apply",
            "p",
            "--template",
            "u.j2",
            "--confirm",
            "--file",
            e.preset_arg(),
        ]),
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    r.stdout_contains(&["目标模板: u.j2"]);
}

// ---------------------------------------------------------------------------
// AC-3.8：导出 / 导入往返一致
// ---------------------------------------------------------------------------

/// AC-3.8：导出→导入往返后内容与规格指纹一致。
#[test]
fn export_import_roundtrip_preserves_fingerprint() {
    let e = Env::new("roundtrip");
    e.save("p", "t.j2", &["x=1", "y=2"]);
    let dump = e.work.join("dump.yaml");

    run_in(
        &e.work,
        &e.args(&[
            "preset",
            "export",
            "p",
            "--out",
            dump.to_str().unwrap(),
            "--file",
            e.preset_arg(),
        ]),
    )
    .stdout_contains(&["已导出 1 个预设"]);

    // 导入到一个**新**文件，再对比内容
    let target = e.work.join("imported.yaml");
    let r = run_in(
        &e.work,
        &e.args(&[
            "preset",
            "import",
            dump.to_str().unwrap(),
            "--file",
            target.to_str().unwrap(),
        ]),
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    r.stdout_contains(&["已导入 1 个预设: p"]);

    // 指纹与参数须一致（去除 createdAt 之外的整体等价性由 show 佐证）
    let r = run_in(
        &e.work,
        &e.args(&["preset", "show", "p", "--file", target.to_str().unwrap()]),
    );
    r.stdout_contains(&["x = 1", "y = 2", "绑定模板: t.j2"]);
    // 两边的规格指纹必须相同
    let a = std::fs::read_to_string(&e.preset).unwrap();
    let b = std::fs::read_to_string(&target).unwrap();
    let fp = |s: &str| {
        s.lines()
            .find(|l| l.contains("specFingerprint:"))
            .unwrap()
            .trim()
            .to_string()
    };
    assert_eq!(fp(&a), fp(&b), "往返后规格指纹必须一致");
}

/// 导入重名 → 未 `--force` 时 name_conflict(6)；`--force` 后覆盖。
#[test]
fn import_duplicate_requires_force() {
    let e = Env::new("import_dup");
    e.save("p", "t.j2", &["x=1", "y=2"]);
    let dump = e.work.join("dump.yaml");
    run_in(
        &e.work,
        &e.args(&[
            "preset",
            "export",
            "p",
            "--out",
            dump.to_str().unwrap(),
            "--file",
            e.preset_arg(),
        ]),
    );

    let r = run_in(
        &e.work,
        &e.args(&[
            "preset",
            "import",
            dump.to_str().unwrap(),
            "--file",
            e.preset_arg(),
        ]),
    );
    assert_eq!(r.code, 6, "{}", r.stderr);
    r.stderr_contains(&["已存在", "--force"]);

    let r = run_in(
        &e.work,
        &e.args(&[
            "preset",
            "import",
            dump.to_str().unwrap(),
            "--force",
            "--file",
            e.preset_arg(),
        ]),
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
}

/// 导入含非法预设名 → 拒绝且不落盘。
#[test]
fn import_rejects_illegal_name() {
    let e = Env::new("import_bad");
    let bad = e.work.join("bad.yaml");
    std::fs::write(
        &bad,
        "version: 1\npresets:\n- name: \"../evil\"\n  template: t.j2\n  params:\n    x: 1\n  createdAt: x\n  specFingerprint: y\n",
    )
    .unwrap();
    let target = e.work.join("out.yaml");
    let r = run_in(
        &e.work,
        &e.args(&[
            "preset",
            "import",
            bad.to_str().unwrap(),
            "--file",
            target.to_str().unwrap(),
        ]),
    );
    assert_eq!(r.code, 2, "{}", r.stderr);
    assert!(!target.exists(), "被拒时不得落盘");
}

// ---------------------------------------------------------------------------
// 删除 / 安全
// ---------------------------------------------------------------------------

/// 删除需显式 `--yes`（破坏性操作）。
#[test]
fn rm_requires_yes_and_then_succeeds() {
    let e = Env::new("rm");
    e.save("p", "t.j2", &["x=1"]);

    let r = run_in(
        &e.work,
        &e.args(&["preset", "rm", "p", "--file", e.preset_arg()]),
    );
    assert_eq!(r.code, 2, "{}", r.stderr);
    r.stderr_contains(&["破坏性操作", "--yes"]);

    let r = run_in(
        &e.work,
        &e.args(&["preset", "rm", "p", "--yes", "--file", e.preset_arg()]),
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    r.stdout_contains(&["已删除预设: p"]);
}

/// 删除后 JSON 的 `action` 必须是 `deleted`，**不是** `updated`。
///
/// 回归守卫：`PresetStore::remove` 内部是"改完内容再落盘"，早期直接透传
/// `save` 的动作 → 一个删除被报成更新。值合法、不报错，只是**语义错**，
/// 消费方按 action 分支时永远走不到删除分支。
#[test]
fn rm_json_reports_deleted_action() {
    let e = Env::new("rm_action");
    e.save("p", "t.j2", &["x=1"]);

    let r = run_in(
        &e.work,
        &e.args(&[
            "--format",
            "json",
            "preset",
            "rm",
            "p",
            "--yes",
            "--file",
            e.preset_arg(),
        ]),
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    let v: serde_json::Value = serde_json::from_str(&r.stdout)
        .unwrap_or_else(|err| panic!("stdout 不是合法 JSON: {err}\n{}", r.stdout));
    let action = v["data"]["action"].as_str().unwrap_or_default();
    assert_eq!(
        action, "deleted",
        "删除的 action 不得报成 updated: {}",
        r.stdout
    );
}

/// 删除不存在的预设 → `preset_not_found` 归退出码 5，且**不是** io(3)。
///
/// 回归守卫：早期"预设不存在"被塞进 `WriteError::Corrupt`，下游只能靠
/// 匹配消息里的 `"不存在"` 来分类（D7 禁止）。独立成 `NotFound` 后，
/// 分类由**结构化字段**决定。
#[test]
fn rm_missing_preset_is_not_found_not_io_error() {
    let e = Env::new("rm_missing");
    e.save("p", "t.j2", &["x=1"]);

    let r = run_in(
        &e.work,
        &e.args(&["preset", "rm", "ghost", "--yes", "--file", e.preset_arg()]),
    );
    assert_eq!(r.code, 5, "{}", r.stderr);
    r.stderr_contains(&["预设不存在"]);
}

/// 红线（R-9）：预设文件**不得**落在模板根内 —— 且该判定**无条件生效**。
///
/// 三条路径都试：显式 `--template-dir`、默认 `./templates`、模板子目录。
/// 早期实现把它包在 `if let Some(root) = ctx.template_dir` 里，
/// 于是未配置模板目录时红线**静默失效**（实测把 presets.yaml 写进了模板根）。
#[test]
fn preset_file_must_not_live_inside_template_root() {
    let e = Env::new("redline");

    // ① 显式模板根内的预设文件
    let inside = e.root.join("presets.yaml");
    let r = run_in(
        &e.work,
        &e.args(&[
            "preset",
            "save",
            "p",
            "t.j2",
            "--param",
            "x=1",
            "--param",
            "y=2",
            "--file",
            inside.to_str().unwrap(),
        ]),
    );
    assert_eq!(r.code, 2, "{}", r.stderr);
    r.stderr_contains(&["不得落在模板目录内"]);
    assert!(!inside.exists(), "红线被拒时不得落盘");

    // ② 模板子目录同样拒绝
    let sub = e.root.join("nested").join("p.yaml");
    std::fs::create_dir_all(sub.parent().unwrap()).unwrap();
    let r = run_in(
        &e.work,
        &e.args(&[
            "preset",
            "save",
            "p",
            "t.j2",
            "--param",
            "x=1",
            "--param",
            "y=2",
            "--file",
            sub.to_str().unwrap(),
        ]),
    );
    assert_eq!(r.code, 2, "{}", r.stderr);
    assert!(!sub.exists());

    // ③ 未配置模板目录时，约定位置 `./templates` 仍受保护
    //    在 work 下建 ./templates，不带 --template-dir 运行
    let r = run_in(
        &e.work,
        &[
            "preset",
            "save",
            "p",
            "t.j2",
            "--param",
            "x=1",
            "--param",
            "y=2",
            "--file",
            "templates/presets.yaml",
        ],
    );
    assert_eq!(r.code, 2, "未配置模板目录时红线也须生效：{}", r.stderr);
    assert!(!e.work.join("templates").join("presets.yaml").exists());

    // ④ 模板根**之外**的正常路径必须通过（避免过度拦截）
    let ok = e.work.join("ok.yaml");
    let r = run_in(
        &e.work,
        &e.args(&[
            "preset",
            "save",
            "p",
            "t.j2",
            "--param",
            "x=1",
            "--param",
            "y=2",
            "--file",
            ok.to_str().unwrap(),
        ]),
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(ok.is_file());
}

// ---------------------------------------------------------------------------
// AC-3.5：陈旧检测
// ---------------------------------------------------------------------------

/// AC-3.5：模板规格变化后，`list`/`show` 标记陈旧并列出失效项。
#[test]
fn stale_preset_is_flagged_after_spec_change() {
    let e = Env::new("stale");
    e.save("p", "t.j2", &["x=1", "y=2"]);

    // 改模板：删掉 y（旧预设里的 y 变失效）、新增必选 z
    std::fs::write(
        e.root.join("t.j2"),
        "G0 X{{ x | nc_fixed(3) }} Z{{ z | nc_fixed(3) }}\nM30\n",
    )
    .unwrap();

    let r = run_in(
        &e.work,
        &e.args(&["preset", "list", "--file", e.preset_arg()]),
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    r.stdout_contains(&["[陈旧]"]);

    let r = run_in(
        &e.work,
        &e.args(&["preset", "list", "--verbose", "--file", e.preset_arg()]),
    );
    r.stdout_contains(&["失效参数: y", "新增必选（预设未含）: z"]);
}

/// 规格未变时不误报陈旧（避免"狼来了"）。
#[test]
fn fresh_preset_is_not_flagged() {
    let e = Env::new("fresh");
    e.save("p", "t.j2", &["x=1", "y=2"]);
    let r = run_in(
        &e.work,
        &e.args(&["preset", "list", "--file", e.preset_arg()]),
    );
    assert!(
        !r.stdout.contains("[陈旧]"),
        "规格未变不应标记陈旧：\n{}",
        r.stdout
    );

    let r = run_in(
        &e.work,
        &e.args(&["preset", "show", "p", "--file", e.preset_arg()]),
    );
    r.stdout_contains(&["陈旧检测: 通过"]);
}

/// **P1-6 实测更正**：审查报告称"目标模板被删/语法坏时 `apply` 会输出
/// 「陈旧检测: 通过」"——实测**不成立**。
///
/// `apply` 在陈旧检测之前就有 `specs_of(ctx, &target)?`，模板不可解析即硬失败。
/// 本用例把这条**真实保护点**钉住：防止将来有人把那个 `?` 改成降级，
/// 从而真的让"检测通过"变成假象。
#[test]
fn apply_fails_loudly_when_target_template_is_unresolvable() {
    let e = Env::new("apply_gone");
    e.save("p", "t.j2", &["x=1", "y=2"]);

    // ① 模板被删除
    std::fs::remove_file(e.root.join("t.j2")).unwrap();
    let r = run_in(
        &e.work,
        &e.args(&["preset", "apply", "p", "--file", e.preset_arg()]),
    );
    assert_ne!(r.code, 0, "必须非 0 退出：\n{}", r.stdout);
    assert!(
        !r.stdout.contains("陈旧检测: 通过"),
        "绝不能出现假的「检测通过」：\n{}",
        r.stdout
    );
    r.stderr_contains(&["模板不存在"]);

    // ② 模板存在但语法坏（注册表构建阶段即失败）
    std::fs::write(
        e.root.join("t.j2"),
        "G0 X{{ x | nc_fixed(3) }}\n{% for %}\n",
    )
    .unwrap();
    let r = run_in(
        &e.work,
        &e.args(&["preset", "apply", "p", "--file", e.preset_arg()]),
    );
    assert_ne!(r.code, 0, "必须非 0 退出：\n{}", r.stdout);
    assert!(
        !r.stdout.contains("陈旧检测: 通过"),
        "绝不能出现假的「检测通过」：\n{}",
        r.stdout
    );
}

// ---------------------------------------------------------------------------
// 损坏文件：降级但不阻断只读命令
// ---------------------------------------------------------------------------

/// 损坏的预设文件 → 只读命令降级（空列表 + 警告），不崩不阻断。
#[test]
fn corrupt_file_degrades_readonly_commands() {
    let e = Env::new("corrupt");
    std::fs::write(&e.preset, "presets: [ : : oops").unwrap();
    let r = run_in(
        &e.work,
        &e.args(&["preset", "list", "--file", e.preset_arg()]),
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    r.stdout_contains(&["预设列表（0 个）"]);
    r.stderr_contains(&["warning", "解析失败"]);
    // 原文件不得被改动
    assert_eq!(
        std::fs::read_to_string(&e.preset).unwrap(),
        "presets: [ : : oops"
    );
}

/// 损坏文件上的写操作 → 拒绝覆盖（避免把用户手写内容永久抹掉）。
#[test]
fn corrupt_file_refuses_write() {
    let e = Env::new("corrupt_write");
    let broken = "presets: [ : : oops";
    std::fs::write(&e.preset, broken).unwrap();
    let r = e.save("p", "t.j2", &["x=1", "y=2"]);
    assert_eq!(r.code, 3, "{}", r.stderr);
    r.stderr_contains(&["不可用", "已拒绝写入"]);
    assert_eq!(std::fs::read_to_string(&e.preset).unwrap(), broken);
}

// ---------------------------------------------------------------------------
// 未声明的参数 / 写冲突
// ---------------------------------------------------------------------------

/// 同名保存未 `--force` → name_conflict(6)；`--force` 后覆盖且参数更新。
#[test]
fn save_duplicate_requires_force() {
    let e = Env::new("dup");
    e.save("p", "t.j2", &["x=1", "y=2"]);

    let r = e.save("p", "t.j2", &["x=5", "y=6"]);
    assert_eq!(r.code, 6, "{}", r.stderr);
    r.stderr_contains(&["同名预设已存在", "--force"]);

    let mut tail = vec![
        "preset",
        "save",
        "p",
        "t.j2",
        "--force",
        "--file",
        e.preset_arg(),
    ];
    for p in ["x=5", "y=6"] {
        tail.push("--param");
        tail.push(p);
    }
    let r = run_in(&e.work, &e.args(&tail));
    assert_eq!(r.code, 0, "{}", r.stderr);

    let r = run_in(
        &e.work,
        &e.args(&["preset", "show", "p", "--file", e.preset_arg()]),
    );
    r.stdout_contains(&["x = 5", "y = 6"]);
}

// ---------------------------------------------------------------------------
// JSON 契约
// ---------------------------------------------------------------------------

/// JSON 通道给出结构化字段（消费方无需解析消息文本）。
#[test]
fn json_output_is_structured() {
    let e = Env::new("json");
    e.save("p", "t.j2", &["x=1", "y=2"]);

    let r = run_in(
        &e.work,
        &e.args(&[
            "preset",
            "list",
            "--file",
            e.preset_arg(),
            "--format",
            "json",
        ]),
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    r.stdout_contains(&[
        "\"presets\"",
        "\"name\": \"p\"",
        "\"paramCount\"",
        "\"stale\"",
        "\"staleParams\"",
    ]);

    let r = run_in(
        &e.work,
        &e.args(&[
            "preset",
            "show",
            "p",
            "--file",
            e.preset_arg(),
            "--format",
            "json",
        ]),
    );
    r.stdout_contains(&["\"specFingerprint\"", "\"fingerprintChanged\""]);
}
