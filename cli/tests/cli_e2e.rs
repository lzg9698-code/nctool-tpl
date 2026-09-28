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
//! 覆盖的 11 个顶层子命令：`templates` / `inspect` / `lint` / `validate` / `render` /
//! `generate` / `machine` / `config` / `ui` / `part` / `completion`
//! （ROADMAP 记为 9 个，`generate` 为后加的规范入口、`lint` 为 2026-09-22 新增，故实际为 11 个）。
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

/// 建一个唯一的临时目录（位于系统临时区，**不**是仓库根目录）。
///
/// `tag` 区分用例；`std::process::id()` + 原子序号避免并发用例互相踩踏。
fn temp_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("nctool_e2e_{}_{}_{}", std::process::id(), tag, n));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("创建临时目录失败");
    dir
}

/// 仓库根目录（用于访问 `templates/turning/demo_gcode.j2` 等仓库内资源）。
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
    let r = run_in(
        repo_root().as_path(),
        &["--format", "json", "inspect", "nope"],
    );
    assert_eq!(r.code, 5);
    let v: serde_json::Value = serde_json::from_str(&r.stdout).expect("stdout 应是合法 JSON");
    assert_eq!(v["ok"], serde_json::json!(false));
    assert_eq!(v["error"]["kind"], serde_json::json!("template_not_found"));
}

#[test]
fn json_success_uses_ok_true() {
    let r = run_in(
        repo_root().as_path(),
        &["--format", "json", "machine", "list"],
    );
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
    let r = run_in(
        repo_root().as_path(),
        &["templates", "list", "--category", "drilling"],
    );
    assert_eq!(r.code, 0);
    r.stdout_contains(&["drill_cycle"]);
    r.stdout_not_contains(&["program_header", "tool_change"]);
}

#[test]
fn templates_show_prints_source_and_params() {
    let r = run_in(
        repo_root().as_path(),
        &["templates", "show", "program_header"],
    );
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
    assert_eq!(
        r.code, 6,
        "重名属业务冲突，归 template_duplicate(6) 而非 io(3)"
    );
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
    let tpl = repo_root()
        .join("templates")
        .join("turning")
        .join("demo_gcode.j2");
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
    let r = run_in(
        repo_root().as_path(),
        &["--format", "json", "validate", "drill_cycle"],
    );
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
fn lint_clean_template_exits_0() {
    // 内置模板不含弧度三角函数 → 无发现项
    let r = run_in(repo_root().as_path(), &["lint", "drill_cycle"]);
    assert_eq!(r.code, 0, "干净模板应退出 0: {}", r.stderr);
    r.stdout_contains(&["静态检查通过"]);
}

#[test]
fn lint_radian_trig_exits_1() {
    let dir = temp_dir("lint_trig");
    let path = dir.join("t.j2");
    std::fs::write(&path, "G0 X{{ angle | sin }}\n").unwrap();
    let r = run_in(&dir, &["lint", path.to_str().unwrap()]);
    assert_eq!(r.code, 1, "有发现应退出 1");
    r.stdout_contains(&["弧度", "sin_d"]);
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
fn render_oversized_params_file_is_rejected() {
    // 回归（第四轮 P1-10）：`--params-file` 曾无上限地 `read_to_string`，
    // 指向大文件 / 网络盘时会把内容整体读进内存。现改为 1 MiB 上限，超限即报错。
    let dir = temp_dir("big_params");
    let path = dir.join("huge.json");
    let f = std::fs::File::create(&path).unwrap();
    f.set_len(1024 * 1024 + 1).unwrap(); // 上限 + 1 字节
    drop(f);

    let r = run_in(
        dir.as_path(),
        &[
            "render",
            "program_header",
            "--param",
            "prog=1001",
            "--params-file",
            path.to_str().unwrap(),
        ],
    );
    assert_eq!(r.code, 3, "参数文件超限属 IO 类失败");
    r.stderr_contains(&["过大", "上限"]);
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
fn render_line_step_and_max_line_are_honored() {
    // 隐患修复：`--line-step` / `--max-line` 曾只存在于 Web UI，CLI 无法复现
    // 同一份带自定义步进的输出。现 CLI 与 API 共用同一份 GenerationOptions 映射。
    let mut args = vec![
        "render",
        "drill_cycle",
        "--line-numbers",
        "--line-step",
        "100",
        "--max-line",
        "250",
    ];
    args.extend_from_slice(DRILL_PARAMS);
    let r = run_in(repo_root().as_path(), &args);
    assert_eq!(r.code, 0, "stderr: {}", r.stderr);

    // 按 token 抽出行号，相邻差值恒为 100，且不超过上限 250
    let nums: Vec<u32> = r
        .stdout
        .split_whitespace()
        .filter_map(|t| t.strip_prefix('N'))
        .filter_map(|t| t.parse().ok())
        .collect();
    assert!(nums.len() >= 2, "应有多行带行号: {}", r.stdout);
    assert!(
        nums.windows(2).all(|w| w[1] - w[0] == 100),
        "步进应为 100: {nums:?}"
    );
    assert!(
        nums.iter().all(|&n| n <= 250),
        "行号不应超过上限 250: {nums:?}"
    );
}

#[test]
fn render_unknown_machine_exits_5() {
    // 全局 --machine 会被延迟到真正需要机床配置时才校验
    let r = run_in(
        repo_root().as_path(),
        &[
            "--machine",
            "nope",
            "render",
            "program_header",
            "--param",
            "prog=1001",
        ],
    );
    assert_eq!(r.code, 5);
    r.stderr_contains(&["未知机床"]);
}

#[test]
fn render_with_named_machine_succeeds() {
    let r = run_in(
        repo_root().as_path(),
        &[
            "--machine",
            "wfl_m65",
            "render",
            "program_header",
            "--param",
            "prog=1001",
        ],
    );
    assert_eq!(r.code, 0);
}

#[test]
fn cli_rejects_spindle_speed_above_machine_limit() {
    // P0-2/Q-01（SUMMARY §8 行2）：wfl_m65 主轴上限 3500，S5000 必须以**非 0**
    // 退出且 stderr 报告含 `3500`。修复前静态上界 6000 会放行 `M3 S5000`
    // （超机床极限 43%）——本 e2e 钉死"校验用的机床 = 渲染用的机床"。
    let r = run_in(
        repo_root().as_path(),
        &[
            "--machine",
            "wfl_m65",
            "render",
            "tool_change",
            "--param",
            "tool_num=1",
            "--param",
            "spindle_speed=5000",
        ],
    );
    assert_ne!(
        r.code, 0,
        "超机床上限必须非 0 退出\nstdout: {}\nstderr: {}",
        r.stdout, r.stderr
    );
    r.stderr_contains(&["3500"]);
    assert!(
        !r.stdout.contains("M3 S5000"),
        "被拒绝时不得输出超程 G-code：{}",
        r.stdout
    );
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
    assert_eq!(
        a.stdout, b.stdout,
        "generate 与 render 默认输出应逐字节一致"
    );
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
    assert!(
        body.contains("default_machine"),
        "示例配置应含 default_machine:\n{body}"
    );
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
    let r = run_in(
        repo_root().as_path(),
        &["ui", "--host", "0.0.0.0", "--port", "0"],
    );
    assert_eq!(r.code, 2, "非回环绑定应被 args(2) 拒绝");
    r.stderr_contains(&["回环"]);
}

#[test]
fn ui_rejects_non_ip_host() {
    let r = run_in(
        repo_root().as_path(),
        &["ui", "--host", "localhost", "--port", "0"],
    );
    assert_eq!(r.code, 2);
}

// ---------------------------------------------------------------------------
// part（零件级批量生成：多工序一次生成）
//
// 这些用例覆盖的是**跨组件的真实契约**，不是 core 的单元测试的重复：
//   * core 单测用内存注册表；这里走 CLI 进程 → 真实模板目录 → 磁盘文件
//   * 退出码 / 文件是否落盘 / 行号是否跨工序续编，只有端到端跑才看得到
// 行号续编与事务语义这两条是本命令存在的理由（E5 §5.1 / §5.2），
// 因此断言必须落在**产物内容**上，而不是只看退出码。
// ---------------------------------------------------------------------------

/// 造一份零件定义 JSON 并写入临时目录，返回零件文件路径。
///
/// `ops` 是「模板名 → 参数 JSON」的列表；`extra` 为顶层附加字段
/// （如 `"default_machine"`）。用 `serde_json` 拼装而非手写字符串，
/// 避免转义错误把测试失败伪装成功能缺陷。
fn write_part(dir: &Path, name: &str, ops: &[(&str, serde_json::Value)]) -> PathBuf {
    let ops: Vec<serde_json::Value> = ops
        .iter()
        .map(|(tpl, params)| serde_json::json!({ "template": tpl, "params": params }))
        .collect();
    let spec = serde_json::json!({ "name": name, "ops": ops });
    let path = dir.join(format!("{name}.json"));
    std::fs::write(&path, serde_json::to_string_pretty(&spec).unwrap()).expect("写出零件定义失败");
    path
}

/// 三个工序的 drill_cycle 零件（drill_cycle 的必选参数见 DRILL_PARAMS）。
fn drill_part_json() -> serde_json::Value {
    serde_json::json!({ "x": 1, "y": 2, "r_plane": 3, "depth": -5, "feed": 100 })
}

#[test]
fn part_generate_concatenates_ops_and_writes_output() {
    let dir = temp_dir("part_ok");
    let part = write_part(
        &dir,
        "two_ops",
        &[
            ("drill_cycle", drill_part_json()),
            ("drill_cycle", drill_part_json()),
        ],
    );
    let out = dir.join("prog.nc");
    let r = run_in(
        &dir,
        &[
            "part",
            "generate",
            part.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ],
    );
    assert_eq!(r.code, 0, "两工序零件应生成成功\nstderr:\n{}", r.stderr);

    let text = std::fs::read_to_string(&out).expect("产物文件应存在");
    // drill_cycle 的产物含 G81/G80 循环，两次渲染 → 关键指令出现两次
    assert_eq!(
        text.matches("G81").count(),
        2,
        "两个工序应各自产生一次钻孔循环；实际产物:\n{text}"
    );
}

/// P2-1：撞行号上限必须有一条**显式告警** —— 产物看起来完全正常，只是后半段
/// 没有行号，用户从产物里看不出来，静默等于永远不知道。
#[test]
fn render_warns_when_line_number_cap_is_hit() {
    let dir = temp_dir("lineno_cap_hit");
    let r = run_in(
        &dir,
        &[
            "render",
            "drill_cycle",
            "--param",
            "x=21",
            "--param",
            "y=15",
            "--param",
            "r_plane=3",
            "--param",
            "depth=-10",
            "--param",
            "feed=100",
            "--line-numbers",
            "--line-step",
            "10",
            "--max-line",
            "20",
        ],
    );
    assert_eq!(r.code, 0, "撞上限不阻断生成\nstderr:\n{}", r.stderr);
    assert!(
        r.stderr.contains("warning:"),
        "应输出显式告警:\n{}",
        r.stderr
    );
    assert!(
        r.stderr.contains("20"),
        "告警应回显上限值，用户才知道调哪个旋钮:\n{}",
        r.stderr
    );
    // 程序本身仍然产出（只是后半段没行号）
    assert!(r.stdout.contains("G81"), "程序应照常生成:\n{}", r.stdout);
}

/// 对照组：没撞上限就不许有告警 —— 告警本身变成噪声，用户会学会忽略。
#[test]
fn render_stays_silent_when_line_numbers_are_under_cap() {
    let dir = temp_dir("lineno_cap_free");
    let r = run_in(
        &dir,
        &[
            "render",
            "drill_cycle",
            "--param",
            "x=21",
            "--param",
            "y=15",
            "--param",
            "r_plane=3",
            "--param",
            "depth=-10",
            "--param",
            "feed=100",
            "--line-numbers",
        ],
    );
    assert_eq!(r.code, 0, "\nstderr:\n{}", r.stderr);
    assert!(
        !r.stderr.contains("warning:"),
        "未撞上限不应有告警:\n{}",
        r.stderr
    );
}

/// P2-3：零件定义里的**未知字段**必须让命令失败，而不是静默忽略。
///
/// core 单测（`core/src/part.rs`）只断言 serde 会报错；这里补上端到端才看得到的
/// 两件事：**CLI 真的非零退出**，且**错误信息点名写错的字段名** ——
/// 后者决定了用户是去改字段名、还是被"缺少参数"之类的话引到错误方向。
#[test]
fn part_generate_rejects_unknown_field_in_part_file() {
    let dir = temp_dir("part_unknown_field");
    // `params` 误写成 `parameters`：程序级参数会全部丢失，却仍能生成出一份零件程序
    let spec = serde_json::json!({
        "name": "typo",
        "parameters": { "x": 1 },
        "ops": [{ "template": "drill_cycle", "params": drill_part_json() }],
    });
    let part = dir.join("typo.json");
    std::fs::write(&part, serde_json::to_string_pretty(&spec).unwrap()).expect("写出零件定义失败");
    let out = dir.join("prog.nc");

    let r = run_in(
        &dir,
        &[
            "part",
            "generate",
            part.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ],
    );
    assert_ne!(r.code, 0, "未知字段应让命令失败，而不是静默忽略后照常生成");
    assert!(
        !out.exists(),
        "解析失败时不得写出任何产物: {}",
        out.display()
    );
    let combined = format!("{}\n{}", r.stdout, r.stderr);
    assert!(
        combined.contains("parameters"),
        "错误应点名写错的字段名: {combined}"
    );
}

#[test]
fn part_generate_continues_line_numbers_across_ops() {
    // E5 §5.1：行号必须跨工序续编，每个工序不能各自从 N0010 重开
    let dir = temp_dir("part_lineno");
    let part = write_part(
        &dir,
        "lineno",
        &[
            ("drill_cycle", drill_part_json()),
            ("drill_cycle", drill_part_json()),
        ],
    );
    let out = dir.join("prog.nc");
    let r = run_in(
        &dir,
        &[
            "part",
            "generate",
            part.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
            "--line-numbers",
        ],
    );
    assert_eq!(r.code, 0, "带行号生成应成功\nstderr:\n{}", r.stderr);

    let text = std::fs::read_to_string(&out).expect("产物文件应存在");
    // 取出所有 N 开头的行号，确认严格递增且无重复（重复 = 未续编）
    let nums: Vec<u32> = text
        .lines()
        .filter_map(|l| l.strip_prefix('N'))
        .filter_map(|rest| rest.split_whitespace().next())
        .filter_map(|n| n.parse().ok())
        .collect();
    assert!(nums.len() >= 4, "应产生多行带号程序；实际产物:\n{text}");
    for w in nums.windows(2) {
        assert!(
            w[1] > w[0],
            "行号必须严格递增（跨工序续编），出现 {} → {}；实际产物:\n{text}",
            w[0],
            w[1]
        );
    }
}

#[test]
fn part_generate_fails_atomically_when_an_op_is_broken() {
    // E5 §5.2：任一工序失败 → 整体失败，且**不留下半个程序**
    let dir = temp_dir("part_broken");
    let part = write_part(
        &dir,
        "broken",
        &[
            ("drill_cycle", drill_part_json()),
            // 缺 depth / feed —— 必选参数不全，该工序必然失败
            (
                "drill_cycle",
                serde_json::json!({ "x": 1, "y": 2, "r_plane": 3 }),
            ),
        ],
    );
    let out = dir.join("prog.nc");
    let r = run_in(
        &dir,
        &[
            "part",
            "generate",
            part.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ],
    );
    assert_eq!(
        r.code, 1,
        "工序校验失败应退出 1（validation）\nstdout:\n{}",
        r.stdout
    );
    assert!(
        !out.exists(),
        "事务语义：任一工序失败时不得写出任何文件，实际写出了 {}",
        out.display()
    );
    // 失败信息要指明是哪一道工序（索引从 1 数更符合人的直觉，这里只断言关键片段）
    r.stderr_contains(&["工序"]);
}

#[test]
fn part_generate_reports_every_failing_op_not_just_the_first() {
    // 聚合而非短路：两个工序都坏，一次运行要报出两条，避免「修一个报一个」
    let dir = temp_dir("part_aggregate");
    let part = write_part(
        &dir,
        "two_broken",
        &[
            ("drill_cycle", serde_json::json!({ "x": 1 })),
            ("definitely_no_such_template", serde_json::json!({})),
        ],
    );
    let r = run_in(
        &dir,
        &[
            "part",
            "generate",
            part.to_str().unwrap(),
            "--out",
            "unused.nc",
        ],
    );
    assert_eq!(r.code, 1);
    // 第一条：参数缺失；第二条：模板不存在 —— 两种不同根因都要出现
    r.stderr_contains(&["definitely_no_such_template"]);
}

#[test]
fn part_generate_missing_part_file_is_io_error() {
    let (r, _dir) = run_isolated("part_missing", &["part", "generate", "no_such_part.json"]);
    // 读文件失败 → 3（io），而不是 7 或 2：这不是「未实现」也不是「用法错」
    assert_eq!(r.code, 3, "零件文件不存在应退出 3（io）");
}

#[test]
fn part_generate_json_channel_reports_per_op_outcomes() {
    // JSON 通道给自动化用：必须能拿到逐工序的结果，而不只是一句「失败」
    let dir = temp_dir("part_json");
    let part = write_part(
        &dir,
        "json_ok",
        &[
            ("drill_cycle", drill_part_json()),
            ("drill_cycle", drill_part_json()),
        ],
    );
    let out = dir.join("prog.nc");
    let r = run_in(
        &dir,
        &[
            "--format",
            "json",
            "part",
            "generate",
            part.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ],
    );
    assert_eq!(r.code, 0, "JSON 通道应成功\nstderr:\n{}", r.stderr);
    let v: serde_json::Value = serde_json::from_str(&r.stdout).expect("stdout 应是合法 JSON");
    assert_eq!(v["ok"], serde_json::json!(true));
    assert_eq!(v["data"]["op_count"], serde_json::json!(2));
    let ops = v["data"]["ops"].as_array().expect("data.ops 应是数组");
    assert_eq!(ops.len(), 2, "两个工序都应出现在结果里");
    assert_eq!(ops[0]["index"], serde_json::json!(0));
    assert_eq!(ops[1]["index"], serde_json::json!(1));
    assert_eq!(ops[0]["template"], serde_json::json!("drill_cycle"));
    // `--out` 模式下正文在磁盘上（与 `render` 的约定一致），故只有 output_file
    assert_eq!(
        v["data"]["output_file"],
        serde_json::json!(out.display().to_string())
    );
    assert!(out.exists(), "应真的写出文件");
}

#[test]
fn part_generate_json_channel_without_out_carries_program_and_cursor() {
    // 不开 --out 时程序走 stdout/JSON；此时 `ops[].end_line_number` 是调用方
    // 唯一能知道「每道工序占用了哪些行号」的途径，必须真的填上（不能恒为 0）。
    let dir = temp_dir("part_json_stdout");
    let part = write_part(
        &dir,
        "json_stdout",
        &[
            ("drill_cycle", drill_part_json()),
            ("drill_cycle", drill_part_json()),
        ],
    );
    let r = run_in(
        &dir,
        &[
            "--format",
            "json",
            "part",
            "generate",
            part.to_str().unwrap(),
            "--line-numbers",
        ],
    );
    assert_eq!(r.code, 0, "应成功\nstderr:\n{}", r.stderr);
    let v: serde_json::Value = serde_json::from_str(&r.stdout).expect("stdout 应是合法 JSON");
    let ops = v["data"]["ops"].as_array().expect("data.ops 应是数组");
    // 第一道工序 3 行 → 末行号 30；第二道接着到 60（续编）
    assert_eq!(
        ops[0]["end_line_number"],
        serde_json::json!(30),
        "第一道工序的末行号应被填上"
    );
    assert_eq!(
        ops[1]["end_line_number"],
        serde_json::json!(60),
        "第二道工序的末行号应接着第一道（跨工序续编）"
    );
    // 未开 --out 时程序正文必须能被调用方拿到
    let prog = v["data"]["output"]
        .as_str()
        .expect("data.output 应是字符串");
    assert!(
        prog.contains("N0010") && prog.contains("N0060"),
        "程序正文应含首末行号；实际:\n{prog}"
    );
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
// P1-10 对拍：模板的**引用方式**不得改变参数校验强度
// ---------------------------------------------------------------------------

/// P1-10 对拍：同一份模板文件，用「注册表名 / cwd 相对路径 / 绝对路径」三种方式
/// 引用，参数校验结论必须**完全一致**。
///
/// 修复前：文件路径分支注册进临时注册表时传的是**空规格**，于是同一份模板
/// `render turning/parity.j2`（注册名）会校验类型/区间/白名单，
/// `render <绝对路径>` 一层都不校验 —— 而两者对用户显示的都是"校验通过"。
/// 这是本项目零容忍的**静默少校验**：越界参数会一路渲染成 G-code（撞刀风险）。
/// 本测试在修复前必须在路径两条上退出 0（即红）。
#[test]
fn render_by_path_and_by_registry_name_agree_on_specs() {
    let dir = temp_dir("specs_parity");
    let tpl_dir = dir.join("templates");
    let sub = tpl_dir.join("turning");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(sub.join("parity.j2"), "G1 X{{ x }} F{{ feed }}\n").unwrap();
    // 第 2 层：变量库（按名生效）—— feed 上界
    std::fs::write(
        tpl_dir.join("variables.yaml"),
        "variables:\n  - name: feed\n    kind: number\n    max: 5000\n",
    )
    .unwrap();
    // 第 3 层：清单覆盖层 —— x 区间 [0, 10]
    std::fs::write(
        tpl_dir.join("templates.yaml"),
        "templates:\n  \"turning/parity.j2\":\n    params:\n      - name: x\n        kind: number\n        min: 0\n        max: 10\n",
    )
    .unwrap();

    let abs = sub.join("parity.j2");
    let abs_s = abs.to_str().unwrap().to_string();
    let refs: [&str; 3] = ["turning/parity.j2", "templates/turning/parity.j2", &abs_s];

    // ① 合法参数：三种引用方式都必须成功，且**产物逐字节一致**
    let mut outs = Vec::new();
    for r in refs {
        let out = run_in(
            &dir,
            &[
                "--template-dir",
                "templates",
                "render",
                r,
                "--param",
                "x=5",
                "--param",
                "feed=100",
            ],
        );
        assert_eq!(out.code, 0, "{r} 合法参数应成功；stderr: {}", out.stderr);
        outs.push(out.stdout);
    }
    assert_eq!(outs[0], outs[1], "注册表名与相对路径的产物必须一致");
    assert_eq!(outs[0], outs[2], "注册表名与绝对路径的产物必须一致");

    // ② 清单层越界（x=999 ∉ [0,10]）：三种方式都必须拒绝
    for r in refs {
        let out = run_in(
            &dir,
            &[
                "--template-dir",
                "templates",
                "render",
                r,
                "--param",
                "x=999",
                "--param",
                "feed=100",
            ],
        );
        assert_eq!(
            out.code, 1,
            "{r} 清单层越界必须被拒绝（退出码 1）；stdout: {}",
            out.stdout
        );
    }

    // ③ 变量库层越界（feed=99999 > 5000）：三种方式都必须拒绝
    for r in refs {
        let out = run_in(
            &dir,
            &[
                "--template-dir",
                "templates",
                "render",
                r,
                "--param",
                "x=5",
                "--param",
                "feed=99999",
            ],
        );
        assert_eq!(
            out.code, 1,
            "{r} 变量库层越界必须被拒绝（退出码 1）；stdout: {}",
            out.stdout
        );
    }

    // ④ `validate` 与 `render` 共用 `resolve_registry`，同样必须一致
    for r in refs {
        let out = run_in(
            &dir,
            &[
                "--template-dir",
                "templates",
                "validate",
                r,
                "--param",
                "x=999",
                "--param",
                "feed=100",
            ],
        );
        assert_eq!(out.code, 1, "{r} validate 也必须拒绝越界参数");
    }

    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// 退出码矩阵自检：确保文档与实现不漂移
// ---------------------------------------------------------------------------

/// 守的是**文档侧**契约：README 的退出码矩阵是脚本作者判分支的依据，
/// 漏掉或写错一个码不会让任何代码报错，只会让下游判错分支。
///
/// `kind → 退出码` 的映射本身由 `cli/src/output.rs` 的 `exit_code_matrix`
/// 单元测试逐个钉住（改动即红），此处不重复；
/// 「真实进程确实返回该码」由本文件其余用例逐条断言。
///
/// 本用例此前是**自证测试**：断言对象是函数内硬编码的局部数组，
/// 与 `CliError::exit_code` 无任何链接，删掉矩阵里任一码它照样绿。
#[test]
fn exit_code_matrix_in_docs_is_complete() {
    let readme = std::fs::read_to_string(repo_root().join("README.md"))
        .expect("应能读取仓库根目录的 README.md");

    let mut codes: Vec<String> = Vec::new();
    let mut in_matrix = false;
    for line in readme.lines() {
        let t = line.trim();
        if t.starts_with("| 码 |") {
            in_matrix = true;
            continue;
        }
        if !in_matrix {
            continue;
        }
        if !t.starts_with('|') {
            break; // 表格结束
        }
        if t.starts_with("| ---") {
            continue; // 分隔行
        }
        let first_cell = t
            .trim_start_matches('|')
            .split('|')
            .next()
            .unwrap_or("")
            .trim();
        codes.push(first_cell.to_string());
    }

    let want: Vec<String> = (0..=7).map(|c| c.to_string()).collect();
    assert_eq!(
        codes, want,
        "README 退出码矩阵应连续覆盖 0..=7（无缺号、无多余），实际: {codes:?}"
    );
}
