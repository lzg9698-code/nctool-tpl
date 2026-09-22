//! T04 模块二（机床配置编辑）E2E：`machine add / edit / rm / test`。
//!
//! 本文件是**新增**契约测试，不触碰既有 `cli_e2e.rs` 的 44 个用例。
//! 覆盖 T04 设计文档（`docs/ARCH_DESIGN_T04_MACHINE.md` §5）的 AC-2.1 ~ AC-2.10，
//! 以及「失败的命令不得留下文件」这条由实现顺序保证的性质。
//!
//! 三层隔离（缺任何一层断言都会随本机环境漂移）：
//! ① 工作目录 = 临时目录 → 隔离项目 `nctool.toml`（`Ctx::project_config_path` 靠 cwd 向上递归）；
//! ② 模板根 = `--template-dir <work>/templates` → 决定 `preflight` 的"被引用键"集合；
//! ③ 全局配置目录 = `Command::env(APPDATA / XDG_CONFIG_HOME / HOME / USERPROFILE)`。
//! ③ 用**子进程级**环境变量而非 `std::env::set_var`：不污染同进程的其它测试，
//! 因此本文件不需要互斥锁、可与其它测试并行（对比 `cli/src/server.rs` 的预设端点测试）。

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

    fn code_is(&self, want: i32) -> &Self {
        assert_eq!(
            self.code, want,
            "退出码不符（期望 {want}）\nstdout:\n{}\nstderr:\n{}",
            self.stdout, self.stderr
        );
        self
    }

    /// 解析 `--format json` 的输出。
    fn json(&self) -> serde_json::Value {
        serde_json::from_str(&self.stdout)
            .unwrap_or_else(|e| panic!("stdout 不是合法 JSON: {e}\n--- 原文 ---\n{}", self.stdout))
    }
}

/// 在指定工作目录执行 `nctool <args>`，并把全局配置目录指到 `cfg`。
fn run_in(dir: &Path, cfg: &Path, args: &[&str]) -> Run {
    let out = Command::new(env!("CARGO_BIN_EXE_nctool"))
        .args(args)
        .current_dir(dir)
        .env("APPDATA", cfg)
        .env("XDG_CONFIG_HOME", cfg)
        .env("HOME", cfg)
        .env("USERPROFILE", cfg)
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
        "nctool_machine_{}_{}_{}",
        std::process::id(),
        tag,
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("创建临时目录失败");
    dir
}

/// 断言目录内无 `.nctool-tmp-` 残留（写盘内核的"无半成品"契约）。
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

/// 场景夹具：工作目录 + 模板根 + 全局配置目录。
///
/// 默认模板 `t.j2` **只引用参数**、不引用任何 `machine.*` 键，因此 `add` 的
/// preflight 不会被"缺键"阻断 —— 缺键阻断由 `missing_key` 类用例单独构造。
struct Env {
    work: PathBuf,
    root: PathBuf,
    cfg: PathBuf,
}

impl Env {
    fn new(tag: &str) -> Self {
        let work = temp_dir(tag);
        let root = work.join("templates");
        let cfg = work.join("global-config");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&cfg).unwrap();
        std::fs::write(
            root.join("t.j2"),
            "G0 X{{ x | nc_fixed(3) }} Y{{ y | nc_fixed(3) }}\nM30\n",
        )
        .unwrap();
        Env { work, root, cfg }
    }

    /// 写一个引用额外机床键的模板（用于 AC-2.4 的缺键阻断）。
    fn with_machine_key_template(&self, key: &str) {
        std::fs::write(
            self.root.join("m.j2"),
            format!("G0 Z{{{{ machine.{key} | default(\"1\") }}}}\nM30\n"),
        )
        .unwrap();
    }

    /// 写一个语法坏掉的模板（用于"模板不可解析不阻断保存"）。
    fn with_broken_template(&self) {
        std::fs::write(self.root.join("bad.j2"), "{% if %}\n").unwrap();
    }

    fn template_dir(&self) -> &str {
        self.root.to_str().unwrap()
    }

    /// 拼参数表：全局 `--template-dir` 在**子命令之前**。
    fn run(&self, tail: &[&str]) -> Run {
        let mut v = vec!["--template-dir", self.template_dir()];
        v.extend_from_slice(tail);
        run_in(&self.work, &self.cfg, &v)
    }

    /// 同上，但带 `--format json`。
    fn run_json(&self, tail: &[&str]) -> Run {
        let mut v = vec!["--template-dir", self.template_dir(), "--format", "json"];
        v.extend_from_slice(tail);
        run_in(&self.work, &self.cfg, &v)
    }

    fn config_path(&self) -> PathBuf {
        self.work.join("nctool.toml")
    }

    fn read_config(&self) -> String {
        std::fs::read_to_string(self.config_path()).expect("nctool.toml 应存在")
    }

    fn write_config(&self, text: &str) {
        std::fs::write(self.config_path(), text).expect("写 nctool.toml");
    }

    fn config_exists(&self) -> bool {
        self.config_path().exists()
    }

    /// 建一台自定义机床，返回运行结果。
    fn add(&self, id: &str, extra: &[&str]) -> Run {
        let mut tail = vec!["machine", "add", id];
        tail.extend_from_slice(extra);
        self.run(&tail)
    }
}

/// 不存在 `.nctool-tmp-` 残留 + 目标文件未被改动（合起来 = "失败不落盘"）。
fn assert_untouched(env: &Env, before: Option<&str>) {
    match before {
        Some(before) => assert_eq!(
            env.read_config(),
            before,
            "命令失败了，但 nctool.toml 被改动"
        ),
        None => assert!(
            !env.config_exists(),
            "命令失败了，但留下了 nctool.toml：{}",
            env.config_path().display()
        ),
    }
    assert_no_tmp_residue(&env.work);
}

// ---------------------------------------------------------------------------
// AC-2.1：add 以基线预填（generic 19 键 / wfl_m65 20 键）
// ---------------------------------------------------------------------------

#[test]
fn ac_2_1_add_prefills_generic_baseline_of_nineteen_keys() {
    let env = Env::new("ac21a");
    let v = env.run_json(&["machine", "add", "hero_x9"]);
    v.code_is(0);
    let data = &v.json()["data"];
    assert_eq!(data["baseKeys"], serde_json::json!(19));
    assert_eq!(data["configKeys"], serde_json::json!(19));
    assert_eq!(data["vendor"], serde_json::json!("Generic"));
    assert_eq!(data["model"], serde_json::json!("CNC"));
    assert_eq!(data["created"], serde_json::json!(true));
    // 注意 `action` **不是** "created"：`add` 会先用 `EXAMPLE_CONFIG` 把文件建出来，
    // 等 `upsert` 落盘时文件早已存在 → 内核报 `updated`。`WriteAction::Created`
    // 描述的是"文件此前不存在"，而 `created` 字段描述的才是"这台机床此前不存在"。
    assert_eq!(data["action"], serde_json::json!("updated"));

    // `axes` 是可选扩展键，基线**不预填**（20 键 schema − axes = 19）。
    let text = env.read_config();
    assert!(text.contains("[machine.hero_x9]"));
    assert!(!text.contains("axes"));

    // 文本模式同一口径（往已存在的文件里再加一台，动作仍报"新建"）。
    env.add("other_x1", &[]).code_is(0).stdout_contains(&[
        "基线: generic（预填 19 键，来自 generic）",
        "配置键: 19 个",
        "动作: 新建",
    ]);
}

#[test]
fn add_reports_file_action_separately_from_entry_created_flag() {
    // 两个维度，别混：`created` = "这台机床此前不存在"（命令层判定），
    // `action` = `WriteAction`，即**文件**写动作。往已存在的文件里加一台新机床时，
    // created=true 而 action=updated（文件本身早就存在了）。
    let env = Env::new("act1");
    env.add("first", &[]).code_is(0);
    let v = env.run_json(&["machine", "add", "second"]);
    v.code_is(0);
    assert_eq!(v.json()["data"]["created"], serde_json::json!(true));
    assert_eq!(v.json()["data"]["action"], serde_json::json!("updated"));
}

#[test]
fn ac_2_1_from_wfl_m65_prefills_twenty_keys_with_its_vendor() {
    let env = Env::new("ac21b");
    let v = env.run_json(&["machine", "add", "hero_x9", "--from", "wfl_m65"]);
    v.code_is(0);
    let data = &v.json()["data"];
    assert_eq!(data["baseKeys"], serde_json::json!(20));
    assert_eq!(data["vendor"], serde_json::json!("WFL"));
    assert_eq!(data["model"], serde_json::json!("M65"));
    assert!(env.read_config().contains("axes"));
}

#[test]
fn ac_2_1_vendor_and_model_override_the_baseline() {
    let env = Env::new("ac21c");
    let v = env.run_json(&[
        "machine", "add", "hero_x9", "--vendor", "ACME", "--model", "M1",
    ]);
    v.code_is(0);
    assert_eq!(v.json()["data"]["vendor"], serde_json::json!("ACME"));
    assert_eq!(v.json()["data"]["model"], serde_json::json!("M1"));
    env.run(&["machine", "show", "hero_x9"])
        .code_is(0)
        .stdout_contains(&["厂商: ACME", "型号: M1"]);
}

// ---------------------------------------------------------------------------
// AC-2.2：内置机床不可改（三个写命令全拒，且不落盘）
// ---------------------------------------------------------------------------

#[test]
fn ac_2_2_builtin_machines_are_rejected_by_every_write_command() {
    for id in ["generic", "wfl_m65", "index_ms40"] {
        let env = Env::new("ac22");
        env.add(id, &[]).code_is(2).stderr_contains(&["不可修改"]);
        env.run(&["machine", "edit", id, "--set", "units=imperial"])
            .code_is(2)
            .stderr_contains(&["不可修改"]);
        env.run(&["machine", "rm", id, "--yes"])
            .code_is(2)
            .stderr_contains(&["不可修改"]);
        assert_untouched(&env, None);
    }
}

// ---------------------------------------------------------------------------
// AC-2.3：值级校验（Choice 越界）阻断保存
// ---------------------------------------------------------------------------

#[test]
fn ac_2_3_choice_violation_blocks_and_creates_nothing() {
    let env = Env::new("ac23a");
    let r = env.add("hero_x9", &["--set", "units=inch"]);
    r.code_is(1)
        .stderr_contains(&["机床配置校验未通过，未落盘", "units", "取值应属于"]);
    // 校验全部排在写盘之前 → 失败的命令不留下 nctool.toml。
    assert_untouched(&env, None);
}

#[test]
fn ac_2_3_choice_violation_leaves_an_existing_file_byte_identical() {
    let env = Env::new("ac23b");
    env.add("keep_me", &[]).code_is(0);
    let before = env.read_config();

    env.add("hero_x9", &["--set", "feed_mode=G96"]).code_is(1);
    assert_untouched(&env, Some(&before));
    assert!(!before.contains("hero_x9"));
}

#[test]
fn ac_2_3_integer_key_rejects_non_numeric_value() {
    let env = Env::new("ac23c");
    let r = env.add("hero_x9", &["--set", "program_digits=abc"]);
    r.code_is(1)
        .stderr_contains(&["program_digits", "期望整数"]);
    assert_untouched(&env, None);
}

// ---------------------------------------------------------------------------
// AC-2.4：缺失"被模板引用的机床键"阻断（补齐后可保存）
// ---------------------------------------------------------------------------

#[test]
fn ac_2_4_missing_referenced_key_blocks_then_succeeds_after_set() {
    let env = Env::new("ac24");
    env.with_machine_key_template("special_key");

    let r = env.add("hero_x9", &[]);
    r.code_is(1).stderr_contains(&[
        "缺失被模板引用的机床键: special_key",
        "--set special_key=...",
    ]);
    assert_untouched(&env, None);

    env.add("hero_x9", &["--set", "special_key=1"]).code_is(0);
    assert!(env.read_config().contains("special_key"));
}

// ---------------------------------------------------------------------------
// AC-2.5：保存后 show 显示完整键值且无告警
// ---------------------------------------------------------------------------

#[test]
fn ac_2_5_show_lists_full_config_without_warnings() {
    let env = Env::new("ac25");
    env.add("hero_x9", &[]).code_is(0);
    let r = env.run(&["machine", "show", "hero_x9"]);
    r.code_is(0)
        .stdout_contains(&["机床: hero_x9", "配置:", "program_prefix", "units"]);
    assert!(
        !r.stdout.contains("配置告警"),
        "刚保存的机床不应有配置告警：\n{}",
        r.stdout
    );
}

// ---------------------------------------------------------------------------
// AC-2.6（CLI 侧）：machine list 能看到自定义机床
// ---------------------------------------------------------------------------

#[test]
fn ac_2_6_list_marks_the_custom_machine() {
    let env = Env::new("ac26");
    env.add("hero_x9", &[]).code_is(0);
    env.run(&["machine", "list"])
        .code_is(0)
        .stdout_contains(&["hero_x9", "(自定义)"]);

    let v = env.run_json(&["machine", "list"]);
    v.code_is(0);
    let items = v.json()["data"]["machines"].as_array().unwrap().clone();
    let hero = items
        .iter()
        .find(|m| m["id"] == "hero_x9")
        .expect("应含 hero_x9");
    assert_eq!(hero["builtin"], serde_json::json!(false));
}

// ---------------------------------------------------------------------------
// AC-2.7：machine test 与 render --machine 逐字节一致
// ---------------------------------------------------------------------------

#[test]
fn ac_2_7_machine_test_matches_render_byte_for_byte() {
    let env = Env::new("ac27");
    env.add("hero_x9", &[]).code_is(0);
    // 文件模板在注册表里的名字**带扩展名**（`t.j2`），与内置模板的裸名不同。
    let params_file = env.work.join("p.json");
    std::fs::write(&params_file, r#"{"x":7,"y":8}"#).unwrap();
    let pf = params_file.to_str().unwrap();

    let cases: Vec<Vec<&str>> = vec![
        vec!["--param", "x=1", "--param", "y=2"],
        vec!["--param", "x=-1.5", "--param", "y=0"],
        vec!["--params-file", pf],
        vec!["--params-file", pf, "--param", "y=99"],
    ];
    for case in cases {
        let mut test_tail = vec!["machine", "test", "hero_x9", "--template", "t.j2"];
        test_tail.extend_from_slice(&case);
        let t = env.run(&test_tail);
        t.code_is(0);

        let mut render_tail = vec!["render", "t.j2", "--machine", "hero_x9"];
        render_tail.extend_from_slice(&case);
        let r = env.run(&render_tail);
        r.code_is(0);

        assert_eq!(
            t.stdout, r.stdout,
            "machine test 与 render --machine 的 G-code 不一致（参数 {case:?}）"
        );
        // 试渲染的安全提示固定在 stderr，不污染 G-code。
        t.stderr_contains(&["试渲染不能替代真实空运行/工艺评审"]);
    }
}

#[test]
fn machine_test_unknown_template_or_machine_is_not_found() {
    let env = Env::new("mt5");
    env.add("hero_x9", &[]).code_is(0);
    env.run(&["machine", "test", "hero_x9", "--template", "nope"])
        .code_is(5)
        .stderr_contains(&["模板不存在"]);
    env.run(&["machine", "test", "nope", "--template", "t.j2"])
        .code_is(5)
        .stderr_contains(&["未知机床"]);
}

// ---------------------------------------------------------------------------
// AC-2.8 / AC-2.9：扩展键与超大位数只告警、不阻断
// ---------------------------------------------------------------------------

#[test]
fn ac_2_8_extension_key_is_warning_only() {
    let env = Env::new("ac28");
    let v = env.run_json(&["machine", "add", "hero_x9", "--set", "my_ext=1"]);
    v.code_is(0);
    // 扩展键合法：保存成功、键真的落盘，只是带一条 warning。
    let warnings = v.json()["data"]["warnings"].as_array().unwrap().clone();
    assert!(
        warnings
            .iter()
            .any(|w| w.as_str().unwrap().contains("my_ext")),
        "扩展键应产生含键名的 warning，实际：{warnings:?}"
    );
    assert!(env.read_config().contains("my_ext"));
}

#[test]
fn ac_2_8_extension_key_warning_goes_to_stderr_in_text_mode() {
    let env = Env::new("ac28b");
    env.add("hero_x9", &["--set", "my_ext=1"])
        .code_is(0)
        .stderr_contains(&["未知配置键: my_ext"]);
}

#[test]
fn ac_2_9_oversized_line_number_digits_only_warns() {
    let env = Env::new("ac29");
    let r = env.add("hero_x9", &["--set", "line_number_digits=1000000000"]);
    r.code_is(0)
        .stderr_contains(&["line_number_digits", "夹紧"]);
    // 真正的夹紧在 pipeline 后处理（既有测试覆盖），写层不复制逻辑、只提示。
    assert!(env.read_config().contains("line_number_digits"));
}

// ---------------------------------------------------------------------------
// AC-2.10：合并式写入不破坏其它段与注释
// ---------------------------------------------------------------------------

/// 含行内注释、`[machine.other]` 与尾部注释的配置（形状与 core golden 一致）。
const CONFIG_WITH_COMMENTS: &str = r#"# nctool 配置示例（E2E 输入：验证合并不破坏其它段与注释）
template_dir = "templates"     # 行内注释：必须保留
default_machine = "generic"

[machine.other]
id = "other"
vendor = "ACME"
model = "M1"

[machine.other.config]
linear = "G1"
rapid = "G0"

# 尾部注释：upsert 后必须仍在原位置
"#;

#[test]
fn ac_2_10_add_keeps_every_other_byte_intact() {
    let env = Env::new("ac210a");
    env.write_config(CONFIG_WITH_COMMENTS);
    env.add("hero_x9", &[]).code_is(0);

    let after = env.read_config();
    assert!(
        after.starts_with(CONFIG_WITH_COMMENTS),
        "原有内容必须逐字节保留在文件前部\n--- 写入后 ---\n{after}"
    );
    assert!(after.contains("[machine.hero_x9]"));
    assert!(after.contains("行内注释：必须保留"));
    assert!(after.contains("尾部注释：upsert 后必须仍在原位置"));
}

#[test]
fn ac_2_10_edit_keeps_every_other_byte_intact() {
    let env = Env::new("ac210b");
    env.write_config(CONFIG_WITH_COMMENTS);
    env.add("hero_x9", &[]).code_is(0);
    let before = env.read_config();

    env.run(&["machine", "edit", "hero_x9", "--set", "units=imperial"])
        .code_is(0);
    let after = env.read_config();
    // `edit` 原地替换目标子表 → 该段之外仍逐字节不变。
    assert!(after.starts_with(CONFIG_WITH_COMMENTS));
    assert!(after.contains("units = \"imperial\""));
    assert_ne!(before, after, "edit 应当真的改了值");
}

/// **回归**：`add` 写入的示例头注释，不能在随后的 `edit` 里消失。
///
/// 缺陷形态：`edit` 走"替换目标子表 + 整篇重新渲染"，而 `toml_edit` 把紧邻表头的
/// 注释存为该表的 decor —— 替换表即丢弃 decor。示例头恰好是文件第一个表的前缀，
/// 于是一次 `edit` 就把 19 行示例注释全删了（43 行 → 25 行），且无任何提示。
#[test]
fn edit_keeps_the_comment_block_that_add_wrote() {
    let env = Env::new("hdredit");
    env.add("hero_x9", &[]).code_is(0);
    let before = env.read_config();

    env.run(&["machine", "edit", "hero_x9", "--set", "units=imperial"])
        .code_is(0);
    let after = env.read_config();

    assert!(
        after.contains("# nctool 配置示例"),
        "示例头注释被删了：\n{after}"
    );
    assert_eq!(
        before.lines().count(),
        after.lines().count(),
        "行数应不变（只改一个值）\n--- before ---\n{before}\n--- after ---\n{after}"
    );
    assert!(after.contains("units = \"imperial\""));
}

#[test]
fn ac_2_10_first_creation_writes_the_example_config_header() {
    let env = Env::new("ac210c");
    env.add("hero_x9", &[]).code_is(0);
    // 与 `config init` 同一份示例（`cli/src/config.rs::EXAMPLE_CONFIG`）——
    // 头注释同时为 AC-2.10 提供"有注释可验"的输入。
    let text = env.read_config();
    assert!(
        text.starts_with("# nctool 配置示例"),
        "新建文件应以示例头注释开头：\n{text}"
    );
}

// ---------------------------------------------------------------------------
// 重名策略：默认拒绝、--force 覆盖
// ---------------------------------------------------------------------------

#[test]
fn add_duplicate_is_name_conflict_and_force_overwrites() {
    let env = Env::new("dup");
    env.add("hero_x9", &[]).code_is(0);

    let r = env.add("hero_x9", &["--set", "units=imperial"]);
    r.code_is(6)
        .stderr_contains(&["同名机床已存在", "--force 覆盖", "machine edit"]);
    assert!(!env.read_config().contains("imperial"), "拒绝时不应写入");

    let v = env.run_json(&[
        "machine",
        "add",
        "hero_x9",
        "--force",
        "--set",
        "units=imperial",
    ]);
    v.code_is(0);
    assert_eq!(v.json()["data"]["created"], serde_json::json!(false));
    assert_eq!(v.json()["data"]["action"], serde_json::json!("updated"));
    assert!(env.read_config().contains("units = \"imperial\""));
}

// ---------------------------------------------------------------------------
// edit：不存在 / 空操作 / --unset
// ---------------------------------------------------------------------------

#[test]
fn edit_unknown_machine_is_not_found() {
    let env = Env::new("ed5");
    env.add("hero_x9", &[]).code_is(0);
    let before = env.read_config();
    env.run(&["machine", "edit", "nope", "--set", "units=imperial"])
        .code_is(5)
        .stderr_contains(&["机床不存在"]);
    assert_untouched(&env, Some(&before));
}

#[test]
fn edit_without_changes_reports_unchanged_and_rewrites_nothing() {
    let env = Env::new("ed11");
    env.add("hero_x9", &[]).code_is(0);
    let before = env.read_config();

    let v = env.run_json(&["machine", "edit", "hero_x9"]);
    v.code_is(0);
    assert_eq!(v.json()["data"]["action"], serde_json::json!("unchanged"));
    assert_eq!(env.read_config(), before, "无改动不应重写文件");
}

#[test]
fn edit_unset_removes_the_key() {
    let env = Env::new("ed12");
    env.add("hero_x9", &[]).code_is(0);
    env.run(&[
        "machine",
        "edit",
        "hero_x9",
        "--unset",
        "line_number_prefix",
    ])
    .code_is(0);
    assert!(!env.read_config().contains("line_number_prefix"));
}

// ---------------------------------------------------------------------------
// rm：--yes 守卫 / 删除 / 空表剪除 / 不存在
// ---------------------------------------------------------------------------

#[test]
fn rm_without_yes_refuses_and_touches_nothing() {
    let env = Env::new("rm13");
    env.add("hero_x9", &[]).code_is(0);
    let before = env.read_config();

    let r = env.run(&["machine", "rm", "hero_x9"]);
    r.code_is(2).stderr_contains(&["破坏性操作", "--yes"]);
    assert_untouched(&env, Some(&before));
}

#[test]
fn rm_yes_deletes_and_reports_deleted_action() {
    let env = Env::new("rm14");
    env.add("hero_x9", &[]).code_is(0);

    let v = env.run_json(&["machine", "rm", "hero_x9", "--yes"]);
    v.code_is(0);
    assert_eq!(v.json()["data"]["action"], serde_json::json!("deleted"));
    assert!(
        !env.read_config().contains("[machine.hero_x9]"),
        "删除后不应再有该段：\n{}",
        env.read_config()
    );
}

#[test]
fn rm_prunes_the_empty_machine_table() {
    let env = Env::new("rm15");
    env.add("hero_x9", &[]).code_is(0);
    env.run(&["machine", "rm", "hero_x9", "--yes"]).code_is(0);
    // 删到没有自定义机床时，顶层 `[machine]` 空表头一并移除。
    // 注意：新建文件带 `EXAMPLE_CONFIG` 头注释，其中 `# [machine.hero_custom]`
    // 是**被注释掉**的示例 —— 故只能断言没有"未注释的" `[machine` 表头。
    let text = env.read_config();
    let live_tables: Vec<&str> = text
        .lines()
        .filter(|l| l.trim_start().starts_with("[machine"))
        .collect();
    assert!(
        live_tables.is_empty(),
        "空表应被剪除，但仍存在表头：{live_tables:?}\n--- 全文 ---\n{text}"
    );
}

#[test]
fn rm_unknown_machine_is_not_found() {
    let env = Env::new("rm16");
    env.add("hero_x9", &[]).code_is(0);
    let before = env.read_config();
    env.run(&["machine", "rm", "nope", "--yes"])
        .code_is(5)
        .stderr_contains(&["无该段"]);
    assert_untouched(&env, Some(&before));

    // 文件根本不存在时是另一种文案。
    let fresh = Env::new("rm17");
    fresh
        .run(&["machine", "rm", "nope", "--yes"])
        .code_is(5)
        .stderr_contains(&["尚未创建"]);
}

// ---------------------------------------------------------------------------
// 乐观锁：--expect-hash
// ---------------------------------------------------------------------------

#[test]
fn expect_hash_mismatch_is_conflict_and_writes_nothing() {
    let env = Env::new("fp1");
    env.add("hero_x9", &[]).code_is(0);
    let before = env.read_config();

    // 外部改动文件 → 旧指纹失效。
    env.write_config(&format!("{before}\n# 外部改动\n"));
    let modified = env.read_config();

    let r = env.run(&[
        "machine",
        "edit",
        "hero_x9",
        "--set",
        "units=imperial",
        "--expect-hash",
        "fnv1a64:0000000000000000",
    ]);
    r.code_is(6).stderr_contains(&["指纹不匹配", "未写盘"]);
    assert_untouched(&env, Some(&modified));
}

#[test]
fn expect_hash_on_add_is_format_checked() {
    let env = Env::new("fp2");
    let r = env.add("hero_x9", &["--expect-hash", "bogus"]);
    r.code_is(2).stderr_contains(&["--expect-hash 格式"]);
    assert_untouched(&env, None);
}

#[test]
fn expect_hash_format_is_not_checked_by_edit_or_rm() {
    // 已登记的**有意不对称**（设计文档未要求，实现现状）：`add` 先校验格式（→2），
    // `edit`/`rm` 直接进 `resolve_expect` → 格式再怎么错也只表现为指纹不匹配（→6）。
    let env = Env::new("fp3");
    env.add("hero_x9", &[]).code_is(0);
    env.run(&[
        "machine",
        "edit",
        "hero_x9",
        "--set",
        "units=imperial",
        "--expect-hash",
        "bogus",
    ])
    .code_is(6);
    env.run(&[
        "machine",
        "rm",
        "hero_x9",
        "--yes",
        "--expect-hash",
        "bogus",
    ])
    .code_is(6);
}

// ---------------------------------------------------------------------------
// 损坏配置：报错且绝不覆盖
// ---------------------------------------------------------------------------

#[test]
fn corrupt_config_is_rejected_without_being_overwritten() {
    const BROKEN: &str = "[machine\nthis is not toml";
    let env = Env::new("corrupt");
    env.write_config(BROKEN);

    env.add("hero_x9", &[])
        .code_is(4)
        .stderr_contains(&["解析失败", "已拒绝写入"]);
    assert_untouched(&env, Some(BROKEN));

    env.run(&["machine", "edit", "hero_x9", "--set", "units=imperial"])
        .code_is(4);
    assert_untouched(&env, Some(BROKEN));

    env.run(&["machine", "rm", "hero_x9", "--yes"]).code_is(4);
    assert_untouched(&env, Some(BROKEN));
}

// ---------------------------------------------------------------------------
// 失败的命令不得留下文件 / 非法 id（AC-2.3「不落盘」的严格形态）
// ---------------------------------------------------------------------------

#[test]
fn illegal_ids_are_rejected_without_creating_a_config_file() {
    for bad in ["../evil", "a/b", r"a\b", "", "C:", "a\nb", "."] {
        let env = Env::new("badid");
        env.add(bad, &[])
            .code_is(2)
            .stderr_contains(&["非法机床 id"]);
        assert_untouched(&env, None);
    }
}

#[test]
fn illegal_id_does_not_disturb_an_existing_config() {
    let env = Env::new("badid2");
    env.add("keep_me", &[]).code_is(0);
    let before = env.read_config();
    env.add("../evil", &[]).code_is(2);
    assert_untouched(&env, Some(&before));
}

// ---------------------------------------------------------------------------
// 边界：模板不可解析不阻断保存 / 带空格的键被正确转义
// ---------------------------------------------------------------------------

#[test]
fn unparsable_template_does_not_block_saving() {
    // 能走到的降级路径是**注册表构建失败**：一个语法坏的模板会让整份注册表失败，
    // `required_machine_keys` 因此降级为空集 + 一条 warning（设计 §3.4 的"模板解析
    // 失败只记 warning、不阻断"）。要点是**保存不受阻**，而不是某条具体文案。
    let env = Env::new("brokentpl");
    env.with_broken_template();
    env.add("hero_x9", &[])
        .code_is(0)
        .stderr_contains(&["无法加载模板注册表", "保存不受阻"]);
    assert!(env.read_config().contains("[machine.hero_x9]"));
}

#[test]
fn extension_key_with_spaces_is_written_as_a_quoted_toml_key() {
    let env = Env::new("spaced");
    env.add("hero_x9", &["--set", "weird key=1"]).code_is(0);
    let text = env.read_config();
    assert!(
        text.contains(r#""weird key" = "1""#),
        "含空格的键应被引号包裹：\n{text}"
    );
    // 能读回来才算真写对了。
    env.run(&["machine", "show", "hero_x9"])
        .code_is(0)
        .stdout_contains(&["weird key"]);
}
