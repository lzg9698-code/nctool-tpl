//! nctool —— 面向数控加工 G-code 模板的 CLI 工具。
//!
//! 在 `nctool-tpl`（模板解析）+ `nctool-core`（生成管线）之上提供命令行入口：
//! 模板浏览、变量提取、参数校验、G-code 渲染生成，以及配置层叠加载。
//!
//! 所有真实逻辑都在 `nctool-core`，CLI 只做参数解析、配置加载与结果展示，
//! 不复制业务逻辑。

mod args;
mod cli;
mod commands;
mod config;
mod context;
mod output;

use std::process::ExitCode;

use clap::Parser;

use crate::cli::Cli;
use crate::output::OutputStyle;

fn main() -> ExitCode {
    // 解析命令行。`Cli::parse` 在参数错误时由 clap 自行退出（--help / 用法错误）。
    let cli = Cli::parse();
    let style = OutputStyle::from(&cli.global.format);

    match cli.command.run(&cli.global) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            // 统一错误输出（text 到 stderr；json 输出结构化错误对象）
            style.print_error(&err);
            ExitCode::from(err.exit_code())
        }
    }
}
