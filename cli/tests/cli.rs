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

/// 回归：模板名含路径分隔符/`..` 应被拒绝（防止逃出模板目录）。
#[test]
fn templates_new_rejects_path_traversal() {
    let dir = tmp_dir("trav");
    std::fs::create_dir_all(dir.join("templates")).unwrap();
    nctool()
        .current_dir(&dir)
        .args(["templates", "new", "../evil"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("路径分隔符"));
    assert!(!dir.join("..").join("evil.j2").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

/// 名字已带 .j2 时不追加扩展名（避免生成 a.j2.j2）。
#[test]
fn templates_new_preserves_j2_extension() {
    let dir = tmp_dir("j2ext");
    std::fs::create_dir_all(dir.join("templates")).unwrap();
    nctool()
        .current_dir(&dir)
        .args(["templates", "new", "my_op.j2"])
        .assert()
        .success();
    assert!(dir.join("templates/my_op.j2").exists());
    assert!(!dir.join("templates/my_op.j2.j2").exists());
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

/// 回归：模板目录外的文件路径也应正常校验（不再误报"模板不存在"）。
#[test]
fn validate_external_file_path_works() {
    let dir = tmp_dir("valext");
    std::fs::write(dir.join("op.j2"), "G1 X{{ a }}").unwrap();
    nctool()
        .current_dir(&dir)
        .args(["validate", "op.j2"])
        .assert()
        .code(1)
        .stdout(predicate::str::contains("必选参数缺失"))
        .stdout(predicate::str::contains("a"))
        .stderr(predicate::str::contains("模板不存在").not());
    let _ = std::fs::remove_dir_all(&dir);
}

/// 回归：validate 失败时 JSON 输出必须是单个可解析对象，且 ok:false。
#[test]
fn validate_json_failure_is_single_object() {
    let output = nctool()
        .args(["validate", "drill_cycle", "--format", "json"])
        .assert()
        .code(1)
        .get_output()
        .clone();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let v: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout 应为单个合法 JSON 对象: {e}\n{stdout}"));
    assert_eq!(v["ok"], serde_json::Value::Bool(false));
    assert!(v["data"]["errors"].as_i64().unwrap() >= 1);
    // 统一失败结构：error:{kind,message} 与 data 并存（脚本可据 kind 分流）
    assert_eq!(
        v["error"]["kind"],
        serde_json::Value::String("validation".into())
    );
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
    // 程序头：规格默认值兜底 + machine 注入；字节级 golden
    // （坐标系/进给模式直接输出配置值 G54/G94，不重复 G 前缀）
    nctool()
        .args(["render", "program_header", "--param", "prog=1"])
        .assert()
        .success()
        .stdout("O0001\n(  )\n(  )\nG21 (metric)\nG54\nG94\nM5\nM9\n");
}

#[test]
fn render_tool_change_golden() {
    // 刀具号 T 字址只接受整数：CLI 类型推断 tool_num=5 → f64 5.0 → nc_strip → 5
    nctool()
        .args(["render", "tool_change", "--param", "tool_num=5"])
        .assert()
        .success()
        .stdout("M5\nM6 T5\nG40 (取消刀具补偿)\n");
}

#[test]
fn render_lenient_applies_spec_defaults_and_postprocess() {
    // 宽松模式是严格模式的超集：省略 r_plane（规格默认 5.0）+ --lenient
    // + 行号/头部——兜底与后处理均应生效（修复前该组合直接渲染失败）
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
            "--lenient",
            "--line-numbers",
            "--header",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("( nctool generated G-code )"))
        .stdout(predicate::str::contains("N0010 G0 X21.000 Y15.000"))
        .stdout(predicate::str::contains("R5.000"));
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

/// `--out` 父目录缺失时自动创建（与 `templates new` 的目录策略一致）。
#[test]
fn render_out_creates_parent_dirs() {
    let dir = tmp_dir("out_dirs");
    let out = dir.join("sub/dir/result.nc");
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
    assert!(out.exists(), "父目录应被创建: {}", out.display());
    let _ = std::fs::remove_dir_all(&dir);
}

/// `--out` 指向源模板自身时拒绝写入（防止渲染结果覆盖并销毁模板源码）。
#[test]
fn render_out_rejects_overwriting_source_template() {
    let dir = tmp_dir("out_self");
    let tpl = dir.join("tpl.j2");
    std::fs::write(&tpl, "G1 X{{ x }}").unwrap();
    nctool()
        .args(["render", "tpl.j2", "--template-dir"])
        .arg(&dir)
        .args(["--param", "x=1", "--out"])
        .arg(&tpl)
        .assert()
        .failure()
        .stderr(predicate::str::contains("相同"));
    // 模板源码必须完好
    let content = std::fs::read_to_string(&tpl).unwrap();
    assert_eq!(content, "G1 X{{ x }}");
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

/// completion/ui/part 不依赖配置文件：CWD 存在损坏的 nctool.toml 也不应失败。
#[test]
fn completion_ignores_broken_project_config() {
    let dir = tmp_dir("cfg_broken");
    std::fs::write(dir.join("nctool.toml"), "not [ valid toml").unwrap();
    nctool()
        .current_dir(&dir)
        .args(["completion", "bash"])
        .assert()
        .success()
        .stdout(predicate::str::contains("_nctool"));
    let _ = std::fs::remove_dir_all(&dir);
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
