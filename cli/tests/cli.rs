//! nctool CLI 集成测试：用 assert_cmd 调用真实二进制。
//!
//! 覆盖：命令树、模板列表、变量提取、参数校验（退出码）、G-code 渲染
//! （golden 测试，与 nctool-core 管线输出逐字节一致）、JSON 输出、
//! 机床/配置/模板脚手架命令。

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::time::Duration;

use assert_cmd::Command;
use predicates::prelude::*;

fn nctool() -> Command {
    Command::cargo_bin("nctool").unwrap()
}

/// 读取 core 的 golden 基线（`tests/golden/<stem>.nc`）。
///
/// CLI 的渲染测试必须**读同一份基线**，而不是各自硬编码一份期望输出：硬编码时
/// 模板一改就要手工同步两处，而没有任何测试能发现两份已经不一致 —— 测试名却
/// 声称"与 nctool-core 管线逐字节一致"（第四轮 P2-32）。
fn read_golden(stem: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("tests")
        .join("golden")
        .join(format!("{stem}.nc"));
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("读取 golden 失败 {}: {e}", path.display()))
        .replace("\r\n", "\n")
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

/// 回归：带 include 的组合模板，参数表须并入片段变量并给出「引用了 …」提示。
/// 这同时覆盖非空 `from_closure` 与含引用两条分支（原来只有内置无 include 模板的用例）。
#[test]
fn inspect_reports_include_closure_hint() {
    let dir = tmp_dir("inspect_closure");
    std::fs::write(
        dir.join("frag.j2"),
        "G1 X{{ fx | nc_fixed(3) }} \u{ff08}frag\u{ff09}\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("main.j2"),
        "{% include \"frag.j2\" %}\nG0 Z{{ top_z | nc_fixed(3) }}\n",
    )
    .unwrap();

    let out = nctool()
        .args(["inspect", "main.j2", "--template-dir"])
        .arg(&dir)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8_lossy(&out);
    assert!(text.contains("引用了"), "应提示 include 穿透: {text}");
    assert!(text.contains("frag.j2"), "应列出被引用片段: {text}");
    // 片段里的参数也必须并入参数表
    assert!(text.contains("fx"), "片段变量 fx 应被并入: {text}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// 无外部变量的模板应输出「（无外部变量引用）」，而不是空的参数分组。
#[test]
fn inspect_reports_no_variables() {
    let dir = tmp_dir("inspect_novars");
    std::fs::write(dir.join("const.j2"), "G21 G90 G54\n").unwrap();
    nctool()
        .args(["inspect", "const.j2", "--template-dir"])
        .arg(&dir)
        .assert()
        .success()
        .stdout(predicate::str::contains("无外部变量引用"));
    let _ = std::fs::remove_dir_all(&dir);
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
        .stdout(read_golden("drill_cycle_generic"));
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
    // 程序头：machine 注入（坐标系/进给模式直接输出配置值 G54/G94，不重复 G 前缀）。
    // 参数与 golden fixture 对齐（`program_header_generic` 用 part_name=DEMO），
    // 期望值直接读同一份基线。
    nctool()
        .args([
            "render",
            "program_header",
            "--param",
            "prog=1",
            "--param",
            "part_name=DEMO",
        ])
        .assert()
        .success()
        .stdout(read_golden("program_header_generic"));

    // 省略可选参数时走规格默认值（`part_name` 默认空串）—— 与上面共用同一模板，
    // 但期望值不能来自 golden（那份带了 DEMO），故单独断言注释头为空。
    nctool()
        .args(["render", "program_header", "--param", "prog=1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("(  )\n(  )"));
}

#[test]
fn render_tool_change_golden() {
    // 刀具号 T 字址只接受整数：CLI 类型推断 tool_num=5 → f64 5.0 → nc_strip → 5
    // 主轴启动：tool_change 内置模板新增必选 spindle_speed，发出 M3 S<转速> 与
    // 刀长补偿 G43 H、冷却 M8（修复前整套程序从不启动主轴）。
    nctool()
        .args([
            "render",
            "tool_change",
            "--param",
            "tool_num=5",
            "--param",
            "spindle_speed=3000",
        ])
        .assert()
        .success()
        .stdout(read_golden("tool_change_generic"));
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

#[test]
fn machine_show_custom_unknown_key_warns() {
    // A4：自定义机床配置的未知键应给出告警（模板拼错键名的最早发现点）。
    // 注入配置目录指向临时目录：
    //   Windows: %APPDATA%\nctool\config.toml
    //   Unix:    $XDG_CONFIG_HOME/nctool/config.toml
    // 两者都指向 dir/nctool/config.toml，跨平台一致。
    let dir = tmp_dir("mkeywarn");
    let cfg_dir = dir.join("nctool");
    std::fs::create_dir_all(&cfg_dir).unwrap();
    std::fs::write(
        cfg_dir.join("config.toml"),
        r#"[machine.my_custom]
id = "my_custom"
vendor = "Acme"
model = "X1"
config.program_prefix = "P"
config.feed_modd = "G94"
config.axes = "X Y Z"
"#,
    )
    .unwrap();
    nctool()
        .env("APPDATA", &dir)
        .env("XDG_CONFIG_HOME", &dir)
        .env("HOME", &dir)
        .env("USERPROFILE", &dir)
        .args(["machine", "show", "my_custom"])
        .assert()
        .success()
        .stdout(predicate::str::contains("feed_modd"))
        .stdout(predicate::str::contains("配置告警"));
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
fn ui_rejects_non_loopback_host() {
    nctool()
        .args(["ui", "--host", "0.0.0.0", "--port", "0"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("仅允许绑定回环地址"));
}

fn reserve_port() -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    port
}

fn wait_for_ui(port: u16) -> std::process::Child {
    let port_arg = port.to_string();
    let mut child = std::process::Command::new(
        std::env::var_os("CARGO_BIN_EXE_nctool").expect("应设置 nctool 二进制路径"),
    )
    .args(["ui", "--port", &port_arg])
    .spawn()
    .expect("应能启动 UI 服务");
    for _ in 0..50 {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if let Ok(response) = http_request(port, "GET", "/health", "") {
            if response_text(&response).starts_with("HTTP/1.1 200") {
                return child;
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = child.kill();
    let _ = child.wait();
    panic!("UI 服务未在超时时间内就绪");
}

fn http_request(port: u16, method: &str, path: &str, body: &str) -> std::io::Result<Vec<u8>> {
    let mut stream = TcpStream::connect_timeout(
        &format!("127.0.0.1:{port}").parse().unwrap(),
        Duration::from_millis(300),
    )?;
    stream.set_read_timeout(Some(Duration::from_millis(500)))?;
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes())?;
    let mut response = Vec::new();
    stream.read_to_end(&mut response)?;
    Ok(response)
}

fn response_text(response: &[u8]) -> String {
    String::from_utf8_lossy(response).into_owned()
}

#[test]
fn ui_http_contracts_and_frontend_mode() {
    let port = reserve_port();
    let mut child = wait_for_ui(port);

    let page = response_text(&http_request(port, "GET", "/", "").unwrap());
    assert!(page.starts_with("HTTP/1.1 200"));
    // v2：前端模式按协议自动判定 —— file:// 走演示模式（离线可用），
    // http(s):// 走服务模式。经 `nctool ui` 提供服务时即服务模式。
    assert!(page.contains(r#"location.protocol === "file:""#));
    assert!(page.contains(r#"? "demo" : "server""#));
    assert!(page.contains("window.location.origin"));

    let templates = response_text(&http_request(port, "GET", "/api/templates", "").unwrap());
    assert!(templates.starts_with("HTTP/1.1 200"));
    assert!(templates.contains("drill_cycle"));

    let detail =
        response_text(&http_request(port, "GET", "/api/templates/drill_cycle", "").unwrap());
    assert!(detail.starts_with("HTTP/1.1 200"));
    assert!(detail.contains("source"));
    assert!(detail.contains("params"));

    let params = r#"{"template":"drill_cycle","params":{"x":21,"y":15,"depth":-10,"feed":100}}"#;
    let validate = response_text(&http_request(port, "POST", "/api/validate", params).unwrap());
    assert!(validate.starts_with("HTTP/1.1 200"));
    assert!(validate.contains("\"ok\":true"));

    let render = response_text(&http_request(port, "POST", "/api/render", params).unwrap());
    assert!(render.starts_with("HTTP/1.1 200"));
    assert!(render.contains("X21.000"));

    let traversal =
        response_text(&http_request(port, "GET", "/api/templates/../Cargo.toml", "").unwrap());
    assert!(traversal.starts_with("HTTP/1.1 404"));
    assert!(!traversal.contains("[workspace]"));

    let invalid_category =
        response_text(&http_request(port, "GET", "/api/templates?category=unknown", "").unwrap());
    assert!(invalid_category.starts_with("HTTP/1.1 400"));

    let unknown_machine = r#"{"template":"drill_cycle","machine":"missing"}"#;
    let machine_response =
        response_text(&http_request(port, "POST", "/api/render", unknown_machine).unwrap());
    assert!(machine_response.starts_with("HTTP/1.1 404"));

    let oversized = "x".repeat(1024 * 1024 + 1);
    let oversized_response =
        response_text(&http_request(port, "POST", "/api/inspect", &oversized).unwrap());
    assert!(oversized_response.starts_with("HTTP/1.1 413"));
    assert!(oversized_response.contains("payload_too_large"));

    child.kill().unwrap();
    child.wait().unwrap();
}

#[test]
fn ui_loads_directory_template_and_custom_machine() {
    let dir = tmp_dir("ui_config");
    let templates = dir.join("templates");
    std::fs::create_dir_all(&templates).unwrap();
    std::fs::write(
        templates.join("custom.j2"),
        "G1 X{{ x }} ({{ machine.vendor }})",
    )
    .unwrap();
    std::fs::write(
        dir.join("nctool.toml"),
        "template_dir = \"templates\"\n[machine.custom]\nid = \"custom\"\nvendor = \"Acme\"\nmodel = \"Demo\"\n[machine.custom.config]\nmax_spindle_rpm = \"1234\"\n",
    )
    .unwrap();

    let port = reserve_port();
    let port_arg = port.to_string();
    let mut child = std::process::Command::new(
        std::env::var_os("CARGO_BIN_EXE_nctool").expect("应设置 nctool 二进制路径"),
    )
    .current_dir(&dir)
    .args(["ui", "--port", &port_arg])
    .spawn()
    .expect("应能启动配置 UI 服务");
    for _ in 0..50 {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if let Ok(response) = http_request(port, "GET", "/health", "") {
            if response_text(&response).starts_with("HTTP/1.1 200") {
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }

    let templates_json = response_text(&http_request(port, "GET", "/api/templates", "").unwrap());
    assert!(templates_json.contains("custom.j2"));
    let machines_json = response_text(&http_request(port, "GET", "/api/machines", "").unwrap());
    assert!(machines_json.contains("Acme"));
    let render_body = r#"{"template":"custom.j2","params":{"x":42},"machine":"custom"}"#;
    let rendered = response_text(&http_request(port, "POST", "/api/render", render_body).unwrap());
    assert!(rendered.starts_with("HTTP/1.1 200"));
    assert!(rendered.contains("X42"));
    assert!(rendered.contains("Acme"));

    child.kill().unwrap();
    child.wait().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ui_serves_health_and_exits_when_killed() {
    // 端口由操作系统分配，避免依赖默认端口或并行测试环境。
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let port_arg = port.to_string();
    let mut child = std::process::Command::new(
        std::env::var_os("CARGO_BIN_EXE_nctool").expect("应设置 nctool 二进制路径"),
    )
    .args(["ui", "--port", &port_arg])
    .spawn()
    .expect("应能启动 UI 服务");
    let mut healthy = false;
    for _ in 0..50 {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if let Ok(mut stream) = TcpStream::connect_timeout(
            &format!("127.0.0.1:{port}").parse().unwrap(),
            Duration::from_millis(100),
        ) {
            stream
                .set_read_timeout(Some(Duration::from_millis(100)))
                .unwrap();
            stream
                .write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
                .unwrap();
            let mut response = String::new();
            let _ = stream.read_to_string(&mut response);
            if response.starts_with("HTTP/1.1 200") {
                healthy = true;
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    if child.try_wait().unwrap().is_none() {
        child.kill().unwrap();
    }
    child.wait().unwrap();
    assert!(healthy, "UI 服务应响应 GET /health");
}

#[test]
fn part_reports_not_implemented() {
    nctool()
        .args(["part", "generate", "x.json"])
        .assert()
        .failure()
        .code(7)
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

/// 两份 UI 页面必须保持一致。
///
/// `cli/ui/index.html` 被 `include_str!` 嵌进二进制（`nctool ui` 提供的那份），
/// `ui/index.html` 是给用户直接双击打开的 `file://` 演示版。两者是同一份页面的
/// 两份拷贝——手工同步迟早漂移，届时"改了页面却看不到变化"会很难查。
/// 加断言把漂移变成 CI 上的失败。
#[test]
fn ui_html_copies_stay_in_sync() {
    let cli_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let embedded = std::fs::read_to_string(cli_dir.join("ui").join("index.html"))
        .expect("cli/ui/index.html 应存在");
    let demo = std::fs::read_to_string(cli_dir.join("..").join("ui").join("index.html"))
        .expect("ui/index.html 应存在");
    assert_eq!(
        embedded, demo,
        "两份 UI 页面已漂移：请把改动同步到 cli/ui/index.html 与 ui/index.html 两份"
    );
}
