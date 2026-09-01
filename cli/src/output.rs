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
    /// `3` IO 失败；`4` 配置错误；`5` 模板/机床未找到；`6` 渲染失败；
    /// 其余分类兜底归 `1`。
    pub fn exit_code(&self) -> u8 {
        match self.kind {
            "validation" => 1,
            "args" => 2,
            "io" => 3,
            "config" => 4,
            "template_not_found" | "machine_not_found" => 5,
            "render" => 6,
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
                if err.silent {
                    return;
                }
                let obj = serde_json::json!({
                    "ok": false,
                    "error": { "kind": err.kind, "message": err.message },
                });
                let text = serde_json::to_string_pretty(&obj).unwrap_or_default();
                write_stdout_quiet(&format!("{text}\n"));
            }
        }
    }

    /// 输出成功结果：text → 原样打印；json → 包一层 `{"ok":true,"data":...}`。
    pub fn print_ok<T: serde::Serialize>(&self, text: &str, data: T) {
        match self {
            OutputStyle::Text => {
                let mut buf = text.to_string();
                if !text.ends_with('\n') {
                    buf.push('\n');
                }
                write_stdout_quiet(&buf);
            }
            OutputStyle::Json => {
                let obj = serde_json::json!({ "ok": true, "data": data });
                let text = serde_json::to_string_pretty(&obj).unwrap_or_default();
                write_stdout_quiet(&format!("{text}\n"));
            }
        }
    }
}
