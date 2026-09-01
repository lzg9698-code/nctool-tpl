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

    /// 命令失败对应的进程退出码（1 = 校验/执行失败）。
    pub fn exit_code(&self) -> u8 {
        1
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
        let kind = match err {
            RegistryError::NotFound(_) => "template_not_found",
            RegistryError::Duplicate(_) => "template_duplicate",
            RegistryError::EmptySource(_) => "template_empty",
            RegistryError::Compile(_) => "template_compile",
            RegistryError::Io(_) => "io",
        };
        CliError::new(kind, err.to_string())
    }
}

impl From<PipelineError> for CliError {
    fn from(err: PipelineError) -> Self {
        match err {
            PipelineError::TemplateNotFound(name) => {
                CliError::new("template_not_found", format!("模板不存在: {name}"))
            }
            PipelineError::Validation(report) => CliError::new("validation", report.summary()),
            PipelineError::Render(err) => CliError::new("render", err.to_string()),
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
                println!("{}", serde_json::to_string_pretty(&obj).unwrap_or_default());
            }
        }
    }

    /// 输出成功结果：text → 原样打印；json → 包一层 `{"ok":true,"data":...}`。
    pub fn print_ok<T: serde::Serialize>(&self, text: &str, data: T) {
        match self {
            OutputStyle::Text => {
                print!("{text}");
                if !text.ends_with('\n') {
                    println!();
                }
            }
            OutputStyle::Json => {
                let obj = serde_json::json!({ "ok": true, "data": data });
                println!("{}", serde_json::to_string_pretty(&obj).unwrap_or_default());
            }
        }
    }
}
