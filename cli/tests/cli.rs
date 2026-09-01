//! nctool CLI 集成测试：用 assert_cmd 调用真实二进制。
//!
//! 覆盖：命令树、模板列表、变量提取、参数校验（退出码）、G-code 渲染
//! （golden 测试，与 nctool-core 管线输出逐字节一致）、JSON 输出、
//! 机床/配置/模板脚手架命令。

use std::path::PathBuf;

use assert_cmd::Command;
use predicates::prelude::*;

fn nctool() -> Command {
    Command::cargo_bin("nctool").unwrap()
}

fn tmp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("nctool_cli_test_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

// ---------------------------------------------------------------------------
// 命令树
// ---------------------------------------------------------------------------

#[test]
fn help_shows_complete_command_tree() {
    nctool()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("templates"))
        .stdout(predicate::str::contains("inspect"))
        .stdout(predicate::str::contains("validate"))
        .stdout(predicate::str::contains("render"))
        .stdout(predicate::str::contains("generate"))
        .stdout(predicate::str::contains("machine"))
        .stdout(predicate::str::contains("config"))
        .stdout(predicate::str::contains("ui"))
        .stdout(predicate::str::contains("completion"));
}

#[test]
fn version_output() {
    nctool()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("nctool"));
}

// ---------------------------------------------------------------------------
// templates
// ---------------------------------------------------------------------------

#[test]
fn templates_list_contains_builtins() {
    nctool()
        .args(["templates", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("drill_cycle"))
        .stdout(predicate::str::contains("program_header"))
        .stdout(predicate::str::contains("tool_change"));
}

#[test]
fn templates_show_prints_source_and_params() {
    nctool()
        .args(["templates", "show", "drill_cycle"])
        .assert()
        .success()
        .stdout(predicate::str::contains("必选参数"))
        .stdout(predicate::str::contains("x"))
        .stdout(predicate::str::contains("G81"));
}

#[test]
fn templates_new_creates_scaffold() {
    let dir = tmp_dir("new");
    nctool()
        .current_dir(&dir)
        .args(["templates", "new", "my_op", "--category", "铣削"])
        .assert()
        .success()
        .stdout(predicate::str::contains("已创建模板"));
    assert!(dir.join("templates/my_op.j2").exists(), "骨架文件应生成");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn templates_new_rejects_duplicate() {
    let dir = tmp_dir("dup");
    std::fs::create_dir_all(dir.join("templates")).unwrap();
    std::fs::write(dir.join("templates/dup.j2"), "X").unwrap();
    nctool()
        .current_dir(&dir)
        .args(["templates", "new", "dup"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("已存在"));
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// inspect
// ---------------------------------------------------------------------------

#[test]
fn inspect_lists_required_and_optional() {
    nctool()
        .args(["inspect", "drill_cycle"])
        .assert()
        .success()
        .stdout(predicate::str::contains("必选参数"))
        // 系统变量 machine 不应出现在必选参数里
        .stdout(predicate::str::contains("machine").not());
}

#[test]
fn inspect_unknown_template_errors() {
    nctool()
        .args(["inspect", "no_such"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("模板不存在"));
}

// ---------------------------------------------------------------------------
// validate
// ---------------------------------------------------------------------------

#[test]
fn validate_missing_required_fails() {
    nctool()
        .args(["validate", "drill_cycle"])
        .assert()
        .code(1)
        .stdout(predicate::str::contains("必选参数缺失"));
}

#[test]
fn validate_with_all_params_passes() {
    nctool()
        .args([
            "validate",
            "drill_cycle",
            "--param",
            "x=21",
            "--param",
            "y=15",
            "--param",
            "depth=-10",
            "--param",
            "feed=100",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("校验通过"));
}

#[test]
fn validate_with_params_file() {
    let dir = tmp_dir("pfile");
    std::fs::write(
        dir.join("params.json"),
        r#"{"x": 21.0, "y": 15.0, "depth": -10.0, "feed": 100.0}"#,
    )
    .unwrap();
    nctool()
        .args(["validate", "drill_cycle", "--params-file"])
        .arg(dir.join("params.json"))
        .assert()
        .success()
        .stdout(predicate::str::contains("校验通过"));
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// render（golden 测试）
// ---------------------------------------------------------------------------

/// golden：CLI 渲染输出与 nctool-core 管线逐字节一致。
#[test]
fn render_drill_cycle_golden() {
    nctool()
        .args([
            "render",
            "drill_cycle",
            "--param",
            "x=21.0",
            "--param",
            "y=15.0",
            "--param",
            "depth=-10.0",
            "--param",
            "feed=100.0",
        ])
        .assert()
        .success()
        .stdout("G0 X21.000 Y15.000\nG1 G98 G81 R5.000 Z-10.000 F100.000\nG80 (取消循环)\n");
}

#[test]
fn render_with_line_numbers_and_header() {
    nctool()
        .args([
            "render",
            "drill_cycle",
            "--param",
            "x=21",
            "--param",
            "y=15",
            "--param",
            "depth=-10",
            "--param",
            "feed=100",
            "--line-numbers",
            "--header",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("( nctool generated G-code )"))
        .stdout(predicate::str::contains("N0010 G0 X21.000 Y15.000"))
        .stdout(predicate::str::contains("N0020"));
}

#[test]
fn render_program_header_golden() {
    // 程序头：规格默认值兜底 + machine 注入
    nctool()
        .args(["render", "program_header", "--param", "prog=1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("O0001"))
        .stdout(predicate::str::contains("G54"))
        .stdout(predicate::str::contains("G94"));
}

#[test]
fn render_missing_params_fails() {
    nctool()
        .args(["render", "drill_cycle"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("必选参数缺失"));
}

#[test]
fn render_out_file_writes() {
    let dir = tmp_dir("out");
    let out = dir.join("demo.nc");
    nctool()
        .args([
            "render",
            "drill_cycle",
            "--param",
            "x=1",
            "--param",
            "y=2",
            "--param",
            "depth=-3",
            "--param",
            "feed=4",
            "--out",
        ])
        .arg(&out)
        .assert()
        .success()
        .stdout(predicate::str::contains("已写入"));
    let content = std::fs::read_to_string(&out).unwrap();
    assert!(content.contains("G81"), "输出文件应含 G81: {content}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn render_json_output() {
    nctool()
        .args([
            "render",
            "drill_cycle",
            "--param",
            "x=1",
            "--param",
            "y=2",
            "--param",
            "depth=-3",
            "--param",
            "feed=4",
            "--format",
            "json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"ok\": true"))
        .stdout(predicate::str::contains("\"output\""))
        .stdout(predicate::str::contains("G81"));
}

#[test]
fn render_lenient_blank_undefined() {
    // 宽松模式：模板直接引用未定义变量渲染为空
    let dir = tmp_dir("lenient");
    std::fs::write(dir.join("plain.j2"), "G1 X{{ x }} ({{ note }})").unwrap();
    nctool()
        .args(["render", "plain.j2", "--template-dir"])
        .arg(&dir)
        .arg("--lenient")
        .assert()
        .success()
        .stdout("G1 X ()\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn render_from_template_file() {
    // 直接以文件路径渲染（不经内置注册表）
    let dir = tmp_dir("file");
    std::fs::write(
        dir.join("move.j2"),
        "G0 X{{ x | nc_fixed(3) }}\nG1 Z{{ depth | nc_fixed(3) }}",
    )
    .unwrap();
    nctool()
        .args(["render", "move.j2", "--template-dir"])
        .arg(&dir)
        .args(["--param", "x=10", "--param", "depth=-5"])
        .assert()
        .success()
        .stdout("G0 X10.000\nG1 Z-5.000\n");
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// machine
// ---------------------------------------------------------------------------

#[test]
fn machine_list_builtins() {
    nctool()
        .args(["machine", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("generic"))
        .stdout(predicate::str::contains("wfl_m65"))
        .stdout(predicate::str::contains("index_ms40"));
}

#[test]
fn machine_show_preset() {
    nctool()
        .args(["machine", "show", "wfl_m65"])
        .assert()
        .success()
        .stdout(predicate::str::contains("WFL"))
        .stdout(predicate::str::contains("max_spindle_rpm"));
}

#[test]
fn machine_show_unknown_errors() {
    nctool()
        .args(["machine", "show", "no_such"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("未知机床"));
}

// ---------------------------------------------------------------------------
// config
// ---------------------------------------------------------------------------

#[test]
fn config_init_generates_file() {
    let dir = tmp_dir("cfginit");
    nctool()
        .current_dir(&dir)
        .args(["config", "init"])
        .assert()
        .success()
        .stdout(predicate::str::contains("已生成示例配置"));
    assert!(dir.join("nctool.toml").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn config_init_rejects_existing() {
    let dir = tmp_dir("cfgexists");
    std::fs::write(dir.join("nctool.toml"), "x = 1").unwrap();
    nctool()
        .current_dir(&dir)
        .args(["config", "init"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("已存在"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn config_show_works() {
    nctool()
        .args(["config", "show"])
        .assert()
        .success()
        .stdout(predicate::str::contains("生效配置"));
}

// ---------------------------------------------------------------------------
// completion / 占位命令
// ---------------------------------------------------------------------------

#[test]
fn completion_generates_script() {
    nctool()
        .args(["completion", "bash"])
        .assert()
        .success()
        .stdout(predicate::str::contains("_nctool"));
}

#[test]
fn ui_reports_not_implemented() {
    nctool()
        .args(["ui"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("尚未实现"));
}

#[test]
fn part_reports_not_implemented() {
    nctool()
        .args(["part", "generate", "x.json"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("尚未实现"));
}

// ---------------------------------------------------------------------------
// 参数类型推断（CLI 行为）
// ---------------------------------------------------------------------------

#[test]
fn render_infers_param_types() {
    // 21 → Number；depth=-10 → Number；类型推断正确即渲染成功
    nctool()
        .args([
            "render",
            "drill_cycle",
            "--param",
            "x=21",
            "--param",
            "y=15",
            "--param",
            "depth=-10",
            "--param",
            "feed=100",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("X21.000"));
}
