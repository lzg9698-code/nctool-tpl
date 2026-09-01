//! 子命令实现与分发。

pub mod completion;
pub mod config_cmd;
pub mod inspect;
pub mod machine;
pub mod part;
pub mod render;
pub mod templates;
pub mod ui;
pub mod validate;

use crate::cli::{Command, GlobalArgs};
use crate::context::Ctx;
use crate::output::CliError;

/// 系统注入变量：渲染时由管线注入上下文（如 `machine`），不算外部必选参数。
pub const SYSTEM_VARS: &[&str] = &["machine"];

impl Command {
    /// 执行当前子命令。
    pub fn run(&self, g: &GlobalArgs) -> Result<(), CliError> {
        let ctx = Ctx::from_global(g)?;
        match self {
            Command::Templates(a) => templates::run(&ctx, a),
            Command::Inspect(a) => inspect::run(&ctx, a),
            Command::Validate(a) => validate::run(&ctx, a),
            Command::Render(a) | Command::Generate(a) => render::run(&ctx, a),
            Command::Machine(a) => machine::run(&ctx, a),
            Command::Config(a) => config_cmd::run(&ctx, a),
            Command::Ui(a) => ui::run(&ctx, a),
            Command::Part(a) => part::run(&ctx, a),
            Command::Completion(a) => completion::run(a),
        }
    }
}
