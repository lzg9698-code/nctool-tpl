//! 统一错误类型与 text/JSON 输出。

use std::fmt;

use nctool_core::pipeline::PipelineError;
use nctool_core::registry::RegistryError;

use crate::cli::FormatArg;

/// CLI 错误：所有命令失败的统一出口。
///
/// `kind` 为 JSON 输出使用的错误分类；`message` 为人类可读描述。
/// `silent`：命令已自行输出完整错误（如 validate 已打印报告），
/// 仅抑制 JSON 通道的重复错误对象；text 通道的 stderr 提示仍保留。
#[derive(Debug)]
pub struct CliError {
    /// 错误分类标识（JSON 输出用）
    pub kind: &'static str,
    /// 人类可读错误描述
    pub message: String,
    /// 是否抑制 JSON 通道的重复输出（命令已自行输出完整错误）
    pub silent: bool,
}

impl CliError {
    pub fn new(kind: &'static str, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            silent: false,
        }
    }

    /// 标记为"已输出完整错误"，抑制 JSON 通道的重复错误对象。
    pub fn silent(mut self) -> Self {
        self.silent = true;
        self
    }

    /// 命令失败对应的进程退出码。
    ///
    /// 矩阵：`0` 成功；`1` 参数校验未通过；`2` 参数/用法错误（与 clap 一致）；
    /// `3` IO 失败；`4` 配置错误；`5` 模板/机床未找到；`6` 渲染/注册表失败；
    /// `7` 功能尚未实现；未知分类兜底归 `1`。
    pub fn exit_code(&self) -> u8 {
        match self.kind {
            "validation" => 1,
            "args" => 2,
            "io" => 3,
            "config" => 4,
            "template_not_found" | "machine_not_found" => 5,
            "render" | "pipeline" | "registry" | "template_duplicate" | "template_empty"
            | "template_compile" => 6,
            "not_implemented" => 7,
            _ => 1,
        }
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.kind, self.message)
    }
}

impl std::error::Error for CliError {}

impl From<std::io::Error> for CliError {
    fn from(err: std::io::Error) -> Self {
        CliError::new("io", err.to_string())
    }
}

impl From<RegistryError> for CliError {
    fn from(err: RegistryError) -> Self {
        // RegistryError 为 non_exhaustive：未来新增变体归入 registry 分类
        let kind = match &err {
            RegistryError::NotFound(_) => "template_not_found",
            RegistryError::Duplicate(_) => "template_duplicate",
            RegistryError::EmptySource(_) => "template_empty",
            RegistryError::Compile { .. } => "template_compile",
            RegistryError::Io(_) => "io",
            _ => "registry",
        };
        CliError::new(kind, err.to_string())
    }
}

impl From<PipelineError> for CliError {
    fn from(err: PipelineError) -> Self {
        // PipelineError 为 non_exhaustive：未来新增变体归入 pipeline 分类
        match err {
            PipelineError::TemplateNotFound(name) => {
                CliError::new("template_not_found", format!("模板不存在: {name}"))
            }
            PipelineError::Validation(report) => CliError::new("validation", report.summary()),
            PipelineError::Render(err) => CliError::new("render", err.to_string()),
            PipelineError::Registry(err) => CliError::new("registry", err.to_string()),
            _ => CliError::new("pipeline", err.to_string()),
        }
    }
}

impl From<nctool_tpl::TplError> for CliError {
    fn from(err: nctool_tpl::TplError) -> Self {
        CliError::new("render", err.to_string())
    }
}

/// 结果输出风格。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputStyle {
    Text,
    Json,
}

impl From<&FormatArg> for OutputStyle {
    fn from(f: &FormatArg) -> Self {
        match f {
            FormatArg::Text => OutputStyle::Text,
            FormatArg::Json => OutputStyle::Json,
        }
    }
}

/// 向 stdout 写入文本。
///
/// 断管道（`BrokenPipe`，如 `nctool ... | head`）静默忽略、进程正常退出；
/// 其余写入错误打印到 stderr。避免 `println!` 在管道下游提前关闭时以 panic 收场。
fn write_stdout_quiet(text: &str) {
    use std::io::Write;
    let stdout = std::io::stdout();
    let mut lock = stdout.lock();
    if let Err(e) = lock.write_all(text.as_bytes()) {
        if e.kind() != std::io::ErrorKind::BrokenPipe {
            eprintln!("error: 输出失败: {e}");
        }
    }
}

/// text 通道的成功输出：保证**恰好一个**结尾换行。
///
/// 多补一个换行会让 `$(nctool ...)` 之类的调用多出空行；少补一个则与
/// `println!` 语义不一致——两种都是"看起来没问题"的输出缺陷，故单独成函数
/// 并加测试钉住。
fn text_ok_buf(text: &str) -> String {
    let mut buf = text.to_string();
    if !text.ends_with('\n') {
        buf.push('\n');
    }
    buf
}

/// JSON 通道的失败包络文本（含结尾换行）；`silent` 错误返回 `None`。
///
/// `silent` 表示命令已自行输出完整错误（如 `validate` 已打印报告），
/// JSON 通道不再重复——重复输出会让 `--format json` 的消费方收到两条错误。
fn json_error_text(err: &CliError) -> Option<String> {
    if err.silent {
        return None;
    }
    let obj = serde_json::json!({
        "ok": false,
        "error": { "kind": err.kind, "message": err.message },
    });
    Some(format!(
        "{}\n",
        serde_json::to_string_pretty(&obj).unwrap_or_default()
    ))
}

/// JSON 通道的成功包络文本（含结尾换行）：`{"ok":true,"data":...}`。
fn json_ok_text<T: serde::Serialize>(data: T) -> String {
    let obj = serde_json::json!({ "ok": true, "data": data });
    format!(
        "{}\n",
        serde_json::to_string_pretty(&obj).unwrap_or_default()
    )
}

impl OutputStyle {
    /// 输出错误：text → stderr 单行；json → 结构化错误对象（stdout）。
    ///
    /// `silent` 错误在 JSON 通道不重复输出（命令已自行输出完整错误）。
    pub fn print_error(&self, err: &CliError) {
        match self {
            OutputStyle::Text => {
                eprintln!("error: {}", err.message);
            }
            OutputStyle::Json => {
                if let Some(text) = json_error_text(err) {
                    write_stdout_quiet(&text);
                }
            }
        }
    }

    /// 输出成功结果：text → 原样打印；json → 包一层 `{"ok":true,"data":...}`。
    pub fn print_ok<T: serde::Serialize>(&self, text: &str, data: T) {
        match self {
            OutputStyle::Text => write_stdout_quiet(&text_ok_buf(text)),
            OutputStyle::Json => write_stdout_quiet(&json_ok_text(data)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nctool_core::derive::DeriveError;
    use nctool_core::validate::ValidationReport;
    use nctool_tpl::TplError;

    /// 退出码是对外契约（README 有完整矩阵，clap 的用法错误也按 2 走），
    /// 逐个钉住，改动即红。
    #[test]
    fn exit_code_matrix() {
        for (kind, want) in [
            ("validation", 1),
            ("args", 2),
            ("io", 3),
            ("config", 4),
            ("template_not_found", 5),
            ("machine_not_found", 5),
            ("render", 6),
            ("pipeline", 6),
            ("registry", 6),
            ("template_duplicate", 6),
            ("template_empty", 6),
            ("template_compile", 6),
            ("not_implemented", 7),
        ] {
            assert_eq!(CliError::new(kind, "x").exit_code(), want, "kind={kind}");
        }
    }

    #[test]
    fn unknown_kind_falls_back_to_1() {
        // 新增分类若忘了进矩阵，退出码会悄悄变成 1（与"校验未通过"撞车）——
        // 这个兜底是有意的，但不能是"没想过"的结果
        assert_eq!(CliError::new("brand_new_kind", "x").exit_code(), 1);
    }

    #[test]
    fn display_is_kind_colon_message() {
        let err = CliError::new("args", "参数格式应为 k=v");
        assert_eq!(err.to_string(), "args: 参数格式应为 k=v");
    }

    #[test]
    fn silent_sets_flag_and_keeps_other_fields() {
        let err = CliError::new("validation", "缺参数").silent();
        assert!(err.silent);
        assert_eq!(err.kind, "validation");
        assert_eq!(err.message, "缺参数");
        // 默认不静默
        assert!(!CliError::new("validation", "缺参数").silent);
    }

    #[test]
    fn io_error_maps_to_io() {
        let err: CliError =
            std::io::Error::new(std::io::ErrorKind::NotFound, "没有这个文件").into();
        assert_eq!(err.kind, "io");
        assert_eq!(err.exit_code(), 3);
        assert!(err.message.contains("没有这个文件"));
    }

    #[test]
    fn registry_errors_map_to_kinds() {
        let cases: Vec<(RegistryError, &str, u8)> = vec![
            (RegistryError::NotFound("t".into()), "template_not_found", 5),
            (
                RegistryError::Duplicate("t".into()),
                "template_duplicate",
                6,
            ),
            (RegistryError::EmptySource("t".into()), "template_empty", 6),
            (
                RegistryError::Compile {
                    name: "t".into(),
                    err: TplError::Render {
                        name: "t".into(),
                        message: "boom".into(),
                    },
                },
                "template_compile",
                6,
            ),
            (RegistryError::Io(std::io::Error::other("读失败")), "io", 3),
        ];
        for (err, kind, code) in cases {
            let mapped: CliError = err.into();
            assert_eq!(mapped.kind, kind);
            assert_eq!(mapped.exit_code(), code, "kind={kind}");
        }
    }

    #[test]
    fn pipeline_errors_map_to_kinds() {
        let cases: Vec<(PipelineError, &str, u8)> = vec![
            (
                PipelineError::TemplateNotFound("t".into()),
                "template_not_found",
                5,
            ),
            (
                PipelineError::Validation(ValidationReport::default()),
                "validation",
                1,
            ),
            (
                PipelineError::Render(TplError::Render {
                    name: "t".into(),
                    message: "boom".into(),
                }),
                "render",
                6,
            ),
            (
                PipelineError::Registry(RegistryError::NotFound("t".into())),
                "registry",
                6,
            ),
            // 兜底分支：新增变体时不会被静默归到别的分类
            (
                PipelineError::Derive(DeriveError::MissingSource {
                    target: "tip".into(),
                    from: "dia".into(),
                }),
                "pipeline",
                6,
            ),
        ];
        for (err, kind, code) in cases {
            let mapped: CliError = err.into();
            assert_eq!(mapped.kind, kind);
            assert_eq!(mapped.exit_code(), code, "kind={kind}");
        }
    }

    #[test]
    fn tpl_error_maps_to_render() {
        let err: CliError = TplError::Render {
            name: "t".into(),
            message: "boom".into(),
        }
        .into();
        assert_eq!(err.kind, "render");
        assert_eq!(err.exit_code(), 6);
    }

    #[test]
    fn output_style_follows_format_arg() {
        assert_eq!(OutputStyle::from(&FormatArg::Text), OutputStyle::Text);
        assert_eq!(OutputStyle::from(&FormatArg::Json), OutputStyle::Json);
    }

    #[test]
    fn text_ok_has_exactly_one_trailing_newline() {
        assert_eq!(text_ok_buf("G0 X0"), "G0 X0\n");
        // 已带换行时不得再补一个（否则多出空行）
        assert_eq!(text_ok_buf("G0 X0\n"), "G0 X0\n");
        assert_eq!(text_ok_buf(""), "\n");
    }

    #[test]
    fn json_error_envelope_shape() {
        let text = json_error_text(&CliError::new("args", "格式错误")).expect("非 silent 应有输出");
        let v: serde_json::Value = serde_json::from_str(&text).expect("应为合法 JSON");
        assert_eq!(v["ok"], false);
        assert_eq!(v["error"]["kind"], "args");
        assert_eq!(v["error"]["message"], "格式错误");
        assert!(text.ends_with('\n'));
    }

    #[test]
    fn silent_error_writes_nothing_in_json() {
        // 命令已自行输出完整错误 → JSON 通道必须**一条都不发**，
        // 否则消费方会收到两条错误对象
        let err = CliError::new("validation", "缺参数").silent();
        assert!(json_error_text(&err).is_none());
    }

    #[test]
    fn json_ok_envelope_shape() {
        let text = json_ok_text(serde_json::json!({ "count": 2 }));
        let v: serde_json::Value = serde_json::from_str(&text).expect("应为合法 JSON");
        assert_eq!(v["ok"], true);
        assert_eq!(v["data"]["count"], 2);
        assert!(text.ends_with('\n'));
    }

    #[test]
    fn print_paths_do_not_panic() {
        // 这是 CLI 唯一的输出出口，写失败/静默分支都不该 panic。
        // 内容正确性由 cli_e2e 的端到端断言覆盖，这里只守"不炸"。
        // 注意：本测试会把两个 JSON 包络打到**测试进程的 stdout**（`cargo test`
        // 输出里能看到），这是 in-process 测试无法避免的，不是输出串了。
        let err = CliError::new("args", "x");
        OutputStyle::Json.print_error(&err);
        OutputStyle::Json.print_error(&CliError::new("args", "x").silent());
        OutputStyle::Text.print_error(&err);
        OutputStyle::Json.print_ok("", serde_json::json!({}));
        OutputStyle::Text.print_ok("G0 X0", serde_json::json!({}));
    }
}
