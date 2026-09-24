//! 子命令实现与分发。

pub mod completion;
pub mod config_cmd;
pub mod inspect;
pub mod lint;
pub mod machine;
pub mod part;
pub mod preset;
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
        // completion 不读任何配置，先行分发 —— 否则 CWD 存在损坏的
        // nctool.toml 时连补全生成都被拦下。
        //
        // `part` 走**正常分支**（需要 Ctx）：它要解析模板（依赖 `--template-dir`
        // 与配置里的模板目录）与机床（工序级可引用配置里的自定义机床），
        // 两者都在 Ctx 里。早期它在无配置分支只因是占位实现，现已改为真实
        // 实现，必须拿到 Ctx。
        if let Command::Completion(a) = self {
            return completion::run(a);
        }
        let ctx = Ctx::from_global(g)?;
        match self {
            Command::Templates(a) => templates::run(&ctx, a),
            Command::Inspect(a) => inspect::run(&ctx, a),
            Command::Lint(a) => lint::run(&ctx, a),
            Command::Validate(a) => validate::run(&ctx, a),
            Command::Render(a) | Command::Generate(a) => render::run(&ctx, a),
            Command::Machine(a) => machine::run(&ctx, a),
            Command::Preset(a) => preset::run(&ctx, a),
            Command::Config(a) => config_cmd::run(&ctx, a),
            Command::Ui(a) => ui::run(&ctx, a),
            Command::Part(a) => match &a.command {
                crate::cli::PartCommand::Generate(gen_args) => part::run(&ctx, gen_args),
            },
            // 已在上方无配置分发（此臂仅满足穷尽性）
            Command::Completion(_) => unreachable!(),
        }
    }
}
