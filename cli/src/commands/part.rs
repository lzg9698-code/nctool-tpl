//! `part` 子命令（占位）：零件级批量生成规划于阶段 4。

use crate::cli::PartArgs;
use crate::output::CliError;

/// `nctool part generate`：零件级批量生成（阶段 4 实现）。不依赖配置文件。
pub fn run(_args: &PartArgs) -> Result<(), CliError> {
    Err(CliError::new(
        "not_implemented",
        "零件级批量生成尚未实现（规划于阶段 4）",
    ))
}
