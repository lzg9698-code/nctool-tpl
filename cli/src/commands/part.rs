//! `part` 子命令（占位）：零件级批量生成规划于阶段 4。

use crate::cli::PartArgs;
use crate::context::Ctx;
use crate::output::CliError;

/// `nctool part generate`：零件级批量生成（阶段 4 实现）。
pub fn run(_ctx: &Ctx, _args: &PartArgs) -> Result<(), CliError> {
    Err(CliError::new(
        "not_implemented",
        "零件级批量生成尚未实现（规划于阶段 4）",
    ))
}
