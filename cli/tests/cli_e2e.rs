//! E1.1 CLI E2E 清单：全部子命令 × 正常/异常路径 × 退出码。
//!
//! 本文件是**契约测试**：退出码是 CLI 对外的稳定契约，任何变更都必须在此同步。
//! 退出码矩阵见 `cli/src/output.rs::CliError::exit_code`：
//!
//! | 码 | 含义 | 触发错误分类 |
//! | --- | --- | --- |
//! | 0 | 成功 | — |
//! | 1 | 参数校验未通过 | `validation` |
//! | 2 | 参数/用法错误（与 clap 一致） | `args` |
//! | 3 | IO 失败 | `io` |
//! | 4 | 配置错误 | `config` |
//! | 5 | 模板/机床未找到 | `template_not_found` / `machine_not_found` |
//! | 6 | 渲染/注册表失败 | `render` / `pipeline` / `registry` / `template_*` |
//! | 7 | 功能尚未实现 | `not_implemented` |
//!
//! 覆盖的 10 个顶层子命令：`templates` / `inspect` / `validate` / `render` /
//! `generate` / `machine` / `config` / `ui` / `part` / `completion`
//! （ROADMAP 记为 9 个，`generate` 为后加的规范入口，故实际为 10 个）。
//!
//! 说明：`ui` 为阻塞服务，E2E 只覆盖其启动前的安全守卫（非回环地址拒绝），
//! 保证用例不会挂起；服务的 HTTP 行为由 `cli/src/server.rs` 的单元测试覆盖。

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
    /// 断言 stdout 包含全部给定片段（失败时打印完整输出，便于定位）。
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

    fn stdout_not_contains(&self, needles: &[&str]) -> &Self {
        for n in needles {
            assert!(
                !self.stdout.contains(n),
                "stdout 不应包含 {n:?}\n--- stdout ---\n{}",
                self.stdout
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

/// 在临时目录执行（避免读/写到仓库工作区或用户配置）。
fn run_isolated(tag: &str, args: &[&str]) -> (Run, PathBuf) {
    let dir = temp_dir(tag);
    let r = run_in(&dir, args);
    (r, dir)
}

/// 创建本仓库根目录下唯一的临时目录。
///
/// `tag` 区分用例；`std::process::id()` + 原子序号避免并发用例互相踩踏。
/// 目录位于 `std::env::temp_dir()`，不污染仓库。
fn temp_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("nctool_e2e_{}_{}_{}", std::process::id(), tag, n));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("创建临时目录失败");
    dir
}

/// 仓库根目录（用于访问 `templates/demo_gcode.j2` 等仓库内资源）。
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("cli 的父目录即仓库根")
        .to_path_buf()
}

/// `drill_cycle` 的全套必选参数（5 个，缺一即校验失败）。
const DRILL_PARAMS: &[&str] = &[
    "--param",
    "x=1",
    "--param",
    "y=2",
    "--param",
    "r_plane=3",
    "--param",
    "depth=-5",
    "--param",
    "feed=100",
];

// ---------------------------------------------------------------------------
// 全局：版本 / 帮助 / 未知命令 / JSON 错误信封
// ---------------------------------------------------------------------------

#[test]
fn version_prints_name_and_semver() {
    let r = run_in(repo_root().as_path(), &["--version"]);
    assert_eq!(r.code, 0);
    r.stdout_contains(&["nctool", env!("CARGO_PKG_VERSION")]);
}

#[test]
fn help_lists_all_subcommands() {
    let r = run_in(repo_root().as_path(), &["--help"]);
    assert_eq!(r.code, 0);
    r.stdout_contains(&[
        "templates",
        "inspect",
        "validate",
        "render",
        "generate",
        "machine",
        "config",
        "ui",
        "part",
        "completion",
    ]);
}

#[test]
fn unknown_subcommand_exits_2() {
    let r = run_in(repo_root().as_path(), &["definitely-not-a-command"]);
    assert_eq!(r.code, 2, "未知子命令应走 clap 的用法错误码 2");
    r.stderr_contains(&["unrecognized subcommand"]);
}

#[test]
fn json_errors_use_structured_envelope() {
    // text 通道的错误在人读 stderr；json 通道必须输出 ok:false 的结构化错误对象
    let r = run_in(repo_root().as_path(), &["--format", "json", "inspect", "nope"]);
    assert_eq!(r.code, 5);
    let v: serde_json::Value = serde_json::from_str(&r.stdout).expect("stdout 应是合法 JSON");
    assert_eq!(v["ok"], serde_json::json!(false));
    assert_eq!(v["error"]["kind"], serde_json::json!("template_not_found"));
}

#[test]
fn json_success_uses_ok_true() {
    let r = run_in(repo_root().as_path(), &["--format", "json", "machine", "list"]);
    assert_eq!(r.code, 0);
    let v: serde_json::Value = serde_json::from_str(&r.stdout).expect("stdout 应是合法 JSON");
    assert_eq!(v["ok"], serde_json::json!(true), "成功响应 ok 应为 true");
    // machine list 的结构化载荷挂在 data.machines 下
    assert!(
        v["data"]["machines"].is_array(),
        "machine list 的 data.machines 应是数组，实际: {}",
        v["data"]
    );
    let ids: Vec<&str> = v["data"]["machines"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|m| m["id"].as_str())
        .collect();
    assert_eq!(ids, vec!["generic", "wfl_m65", "index_ms40"]);
}

// ---------------------------------------------------------------------------
// templates
// ---------------------------------------------------------------------------

#[test]
fn templates_lists_builtin_templates() {
    let r = run_in(repo_root().as_path(), &["templates", "list"]);
    assert_eq!(r.code, 0);
    r.stdout_contains(&[
        "drill_cycle",
        "program_header",
        "program_footer",
        "tool_change",
        "safe_move",
    ]);
}

#[test]
fn templates_list_filters_by_category() {
    let r = run_in(repo_root().as_path(), &["templates", "list", "--category", "drilling"]);
    assert_eq!(r.code, 0);
    r.stdout_contains(&["drill_cycle"]);
    r.stdout_not_contains(&["program_header", "tool_change"]);
}

#[test]
fn templates_show_prints_source_and_params() {
    let r = run_in(repo_root().as_path(), &["templates", "show", "program_header"]);
    assert_eq!(r.code, 0);
    r.stdout_contains(&["program_header", "prog"]);
}

#[test]
fn templates_show_missing_exits_5() {
    let r = run_in(repo_root().as_path(), &["templates", "show", "nope"]);
    assert_eq!(r.code, 5);
    r.stderr_contains(&["模板不存在"]);
}

#[test]
fn templates_new_creates_scaffold() {
    let (r, dir) = run_isolated("new_ok", &["templates", "new", "demo"]);
    assert_eq!(r.code, 0);
    let created = dir.join("templates").join("demo.j2");
    assert!(created.exists(), "应在 {created:?} 生成模板骨架");
    let body = std::fs::read_to_string(&created).unwrap();
    assert!(!body.trim().is_empty(), "骨架不应为空");
}

#[test]
fn templates_new_duplicate_exits_6() {
    let dir = temp_dir("new_dup");
    assert_eq!(run_in(&dir, &["templates", "new", "demo"]).code, 0);
    let r = run_in(&dir, &["templates", "new", "demo"]);
    assert_eq!(r.code, 6, "重名属业务冲突，归 template_duplicate(6) 而非 io(3)");
    r.stderr_contains(&["模板已存在"]);
}

#[test]
fn templates_new_rejects_path_separators() {
    // E3.1 路径守卫：模板名不得穿越出模板目录
    for name in ["../escape", "a/b", "..\\escape"] {
        let (r, _dir) = run_isolated("new_sep", &["templates", "new", name]);
        assert_eq!(r.code, 2, "含路径分隔符的模板名 {name} 应被拒绝");
        r.stderr_contains(&["路径分隔符"]);
    }
}

#[test]
fn templates_new_requires_name() {
    let r = run_in(repo_root().as_path(), &["templates", "new"]);
    assert_eq!(r.code, 2);
}

// ---------------------------------------------------------------------------
// inspect
// ---------------------------------------------------------------------------

#[test]
fn inspect_reports_required_and_optional_params() {
    let r = run_in(repo_root().as_path(), &["inspect", "drill_cycle"]);
    assert_eq!(r.code, 0);
    r.stdout_contains(&["drill_cycle", "x", "y", "r_plane", "depth", "feed"]);
}

#[test]
fn inspect_missing_exits_5() {
    let r = run_in(repo_root().as_path(), &["inspect", "nope"]);
    assert_eq!(r.code, 5);
    r.stderr_contains(&["模板不存在"]);
}

#[test]
fn inspect_accepts_file_path() {
    let tpl = repo_root().join("templates").join("demo_gcode.j2");
    assert!(tpl.exists(), "仓库示例模板应存在: {tpl:?}");
    let r = run_in(repo_root().as_path(), &["inspect", tpl.to_str().unwrap()]);
    assert_eq!(r.code, 0, "按文件路径 inspect 应成功");
}

// ---------------------------------------------------------------------------
// validate
// ---------------------------------------------------------------------------

#[test]
fn validate_passes_with_full_params() {
    let mut args = vec!["validate", "drill_cycle"];
    args.extend_from_slice(DRILL_PARAMS);
    let r = run_in(repo_root().as_path(), &args);
    assert_eq!(r.code, 0);
    r.stdout_contains(&["校验通过"]);
}

#[test]
fn validate_missing_params_exits_1() {
    let r = run_in(repo_root().as_path(), &["validate", "drill_cycle"]);
    assert_eq!(r.code, 1, "缺必选参数应退出 1（validation）");
    r.stdout_contains(&["必选参数缺失"]);
}

#[test]
fn validate_type_mismatch_exits_1() {
    let r = run_in(
        repo_root().as_path(),
        &[
            "validate",
            "drill_cycle",
            "--param",
            "x=abc",
            "--param",
            "y=2",
            "--param",
            "r_plane=3",
            "--param",
            "depth=-5",
            "--param",
            "feed=100",
        ],
    );
    assert_eq!(r.code, 1);
    r.stdout_contains(&["类型不匹配"]);
}

#[test]
fn validate_json_reports_issue_count() {
    let r = run_in(repo_root().as_path(), &["--format", "json", "validate", "drill_cycle"]);
    assert_eq!(r.code, 1);
    let v: serde_json::Value = serde_json::from_str(&r.stdout).expect("stdout 应是合法 JSON");
    assert_eq!(v["ok"], serde_json::json!(false));
    assert!(
        v["data"]["errors"].as_i64().unwrap_or(0) > 0,
        "data.errors 应为正数，实际: {}",
        v["data"]
    );
}

// ---------------------------------------------------------------------------
// render
// ---------------------------------------------------------------------------

#[test]
fn render_outputs_gcode() {
    let mut args = vec!["render", "drill_cycle"];
    args.extend_from_slice(DRILL_PARAMS);
    let r = run_in(repo_root().as_path(), &args);
    assert_eq!(r.code, 0);
    r.stdout_contains(&["G0 X1.000 Y2.000", "G98 G81", "G80"]);
}

#[test]
fn render_missing_params_exits_1() {
    let r = run_in(repo_root().as_path(), &["render", "drill_cycle"]);
    assert_eq!(r.code, 1, "render 前置校验失败同样退出 1");
}

#[test]
fn render_writes_out_file_and_creates_parent_dirs() {
    let dir = temp_dir("render_out");
    let out = dir.join("nested").join("deeper").join("out.nc");
    let mut args = vec!["render", "drill_cycle", "--out", out.to_str().unwrap()];
    args.extend_from_slice(DRILL_PARAMS);
    let r = run_in(repo_root().as_path(), &args);
    assert_eq!(r.code, 0);
    assert!(out.exists(), "应自动创建父目录并写入 {out:?}");
    let body = std::fs::read_to_string(&out).unwrap();
    assert!(body.contains("G98 G81"), "文件内容应是 G-code:\n{body}");
}

#[test]
fn render_missing_params_file_exits_3() {
    let r = run_in(
        repo_root().as_path(),
        &[
            "render",
            "program_header",
            "--param",
            "prog=1001",
            "--params-file",
            "no/such/file.json",
        ],
    );
    assert_eq!(r.code, 3, "读不到参数文件属 IO 失败");
    r.stderr_contains(&["读取参数文件失败"]);
}

#[test]
fn render_accepts_all_postprocess_flags() {
    let mut args = vec![
        "render",
        "drill_cycle",
        "--line-numbers",
        "--header",
        "--ascii",
        "--strip-blank",
    ];
    args.extend_from_slice(DRILL_PARAMS);
    let r = run_in(repo_root().as_path(), &args);
    assert_eq!(r.code, 0);
    r.stdout_contains(&["G98 G81"]);
}

#[test]
fn render_unknown_machine_exits_5() {
    // 全局 --machine 会被延迟到真正需要机床配置时才校验
    let r = run_in(
        repo_root().as_path(),
        &["--machine", "nope", "render", "program_header", "--param", "prog=1001"],
    );
    assert_eq!(r.code, 5);
    r.stderr_contains(&["未知机床"]);
}

#[test]
fn render_with_named_machine_succeeds() {
    let r = run_in(
        repo_root().as_path(),
        &["--machine", "wfl_m65", "render", "program_header", "--param", "prog=1001"],
    );
    assert_eq!(r.code, 0);
}

// ---------------------------------------------------------------------------
// generate（与 render 同签名的规范入口）
// ---------------------------------------------------------------------------

#[test]
fn generate_outputs_gcode() {
    let mut args = vec!["generate", "drill_cycle"];
    args.extend_from_slice(DRILL_PARAMS);
    let r = run_in(repo_root().as_path(), &args);
    assert_eq!(r.code, 0);
    r.stdout_contains(&["G98 G81"]);
}

#[test]
fn generate_matches_render_byte_for_byte() {
    // generate 被定义为与 render 相同的管线，默认输出必须一致
    let mut r_args = vec!["render", "drill_cycle"];
    r_args.extend_from_slice(DRILL_PARAMS);
    let mut g_args = vec!["generate", "drill_cycle"];
    g_args.extend_from_slice(DRILL_PARAMS);
    let root = repo_root();
    let a = run_in(root.as_path(), &r_args);
    let b = run_in(root.as_path(), &g_args);
    assert_eq!(a.code, 0);
    assert_eq!(b.code, 0);
    assert_eq!(a.stdout, b.stdout, "generate 与 render 默认输出应逐字节一致");
}

// ---------------------------------------------------------------------------
// machine
// ---------------------------------------------------------------------------

#[test]
fn machine_lists_three_presets() {
    let r = run_in(repo_root().as_path(), &["machine", "list"]);
    assert_eq!(r.code, 0);
    r.stdout_contains(&["generic", "wfl_m65", "index_ms40"]);
}

#[test]
fn machine_show_prints_config() {
    let r = run_in(repo_root().as_path(), &["machine", "show", "generic"]);
    assert_eq!(r.code, 0);
    r.stdout_contains(&["generic"]);
}

#[test]
fn machine_show_missing_exits_5() {
    let r = run_in(repo_root().as_path(), &["machine", "show", "nope"]);
    assert_eq!(r.code, 5);
    r.stderr_contains(&["未知机床"]);
}

// ---------------------------------------------------------------------------
// config
// ---------------------------------------------------------------------------

#[test]
fn config_show_prints_effective_config() {
    // 在空临时目录运行，避免读到仓库或用户级配置导致断言不稳定
    let (r, _dir) = run_isolated("cfg_show", &["config", "show"]);
    assert_eq!(r.code, 0);
    r.stdout_contains(&["生效配置", "默认机床"]);
}

#[test]
fn config_init_writes_sample_toml() {
    let (r, dir) = run_isolated("cfg_init", &["config", "init"]);
    assert_eq!(r.code, 0);
    let f = dir.join("nctool.toml");
    assert!(f.exists(), "应在 {f:?} 生成示例配置");
    let body = std::fs::read_to_string(&f).unwrap();
    assert!(body.contains("default_machine"), "示例配置应含 default_machine:\n{body}");
}

#[test]
fn config_init_refuses_overwrite_exits_4() {
    let dir = temp_dir("cfg_dup");
    assert_eq!(run_in(&dir, &["config", "init"]).code, 0);
    let r = run_in(&dir, &["config", "init"]);
    assert_eq!(r.code, 4, "已存在配置拒绝覆盖，归 config(4)");
    r.stderr_contains(&["已存在"]);
}

// ---------------------------------------------------------------------------
// ui（仅覆盖启动前的安全守卫，避免阻塞）
// ---------------------------------------------------------------------------

#[test]
fn ui_rejects_non_loopback_bind() {
    // 关键安全守卫：非回环地址必须在启动监听前就被拒绝（E3.2）
    let r = run_in(repo_root().as_path(), &["ui", "--host", "0.0.0.0", "--port", "0"]);
    assert_eq!(r.code, 2, "非回环绑定应被 args(2) 拒绝");
    r.stderr_contains(&["回环"]);
}

#[test]
fn ui_rejects_non_ip_host() {
    let r = run_in(repo_root().as_path(), &["ui", "--host", "localhost", "--port", "0"]);
    assert_eq!(r.code, 2);
}

// ---------------------------------------------------------------------------
// part（阶段 4 占位）
// ---------------------------------------------------------------------------

#[test]
fn part_generate_exits_7_not_implemented() {
    let (r, _dir) = run_isolated("part", &["part", "generate", "x.json"]);
    assert_eq!(r.code, 7, "未实现功能应退出 7，而非假装成功");
    r.stderr_contains(&["尚未实现"]);
}

#[test]
fn part_generate_json_keeps_kind() {
    let (r, _dir) = run_isolated("part_json", &["--format", "json", "part", "generate", "x.json"]);
    assert_eq!(r.code, 7);
    let v: serde_json::Value = serde_json::from_str(&r.stdout).expect("stdout 应是合法 JSON");
    assert_eq!(v["error"]["kind"], serde_json::json!("not_implemented"));
}

// ---------------------------------------------------------------------------
// completion
// ---------------------------------------------------------------------------

#[test]
fn completion_emits_script_for_every_shell() {
    // 规范值取 clap 的 kebab-case 名；`powershell` 常见写法另有别名用例覆盖
    let cases = [
        ("bash", "_nctool"),
        ("zsh", "#compdef"),
        ("fish", "complete"),
        ("elvish", "edit:completion"),
        ("power-shell", "Register-ArgumentCompleter"),
    ];
    for (shell, marker) in cases {
        let r = run_in(repo_root().as_path(), &["completion", shell]);
        assert_eq!(r.code, 0, "completion {shell} 应成功");
        r.stdout_contains(&[marker]);
    }
}

#[test]
fn completion_accepts_powershell_aliases() {
    // `powershell` / `pwsh` 是用户习惯写法，应与 `power-shell` 等价
    for shell in ["powershell", "pwsh"] {
        let r = run_in(repo_root().as_path(), &["completion", shell]);
        assert_eq!(r.code, 0, "completion {shell} 别名应可用");
        r.stdout_contains(&["Register-ArgumentCompleter"]);
    }
}

#[test]
fn completion_rejects_unknown_shell() {
    let r = run_in(repo_root().as_path(), &["completion", "tcsh"]);
    assert_eq!(r.code, 2);
}

#[test]
fn completion_requires_shell() {
    let r = run_in(repo_root().as_path(), &["completion"]);
    assert_eq!(r.code, 2);
}

// ---------------------------------------------------------------------------
// 退出码矩阵自检：确保文档与实现不漂移
// ---------------------------------------------------------------------------

#[test]
fn exit_code_matrix_is_fully_covered() {
    // 本清单必须覆盖矩阵里的每一个退出码，否则契约出现盲区
    let covered = [
        (0, "成功"),
        (1, "校验未通过"),
        (2, "参数/用法错误"),
        (3, "IO 失败"),
        (4, "配置错误"),
        (5, "模板/机床未找到"),
        (6, "渲染/注册表失败"),
        (7, "尚未实现"),
    ];
    assert_eq!(covered.len(), 8, "退出码矩阵应被 8 个码完整覆盖");
}
