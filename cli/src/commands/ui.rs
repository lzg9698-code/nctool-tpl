//! `ui` 子命令（占位）：本地 Web UI 规划于阶段 2。

use crate::cli::UiArgs;
use crate::output::CliError;

/// `nctool ui`：启动本地 Web UI（阶段 2 实现）。不依赖配置文件。
pub fn run(args: &UiArgs) -> Result<(), CliError> {
    Err(CliError::new(
        "not_implemented",
        format!(
            "UI 服务尚未实现（规划于阶段 2）；命令已预留: --host {} --port {} --open {}",
            args.host, args.port, args.open
        ),
    ))
}
