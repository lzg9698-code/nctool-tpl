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

impl Command {
    /// 执行当前子命令。
    pub fn run(&self, g: &GlobalArgs) -> Result<(), CliError> {
        // 与配置无关的命令先行分发：completion/ui/part 不读配置文件，
        // 避免 CWD 存在损坏的 nctool.toml 时连补全生成也被拦下
        match self {
            Command::Completion(a) => return completion::run(a),
            Command::Ui(a) => return ui::run(a),
            Command::Part(a) => return part::run(a),
            _ => {}
        }
        let ctx = Ctx::from_global(g)?;
        match self {
            Command::Templates(a) => templates::run(&ctx, a),
            Command::Inspect(a) => inspect::run(&ctx, a),
            Command::Validate(a) => validate::run(&ctx, a),
            Command::Render(a) | Command::Generate(a) => render::run(&ctx, a),
            Command::Machine(a) => machine::run(&ctx, a),
            Command::Config(a) => config_cmd::run(&ctx, a),
            // 已在上方无配置分发（此臂仅满足穷尽性）
            Command::Ui(_) | Command::Part(_) | Command::Completion(_) => unreachable!(),
        }
    }
}
