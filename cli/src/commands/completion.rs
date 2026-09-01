//! `completion` 子命令：生成 shell 补全脚本。

use clap::CommandFactory;
use clap_complete::{generate, shells};

use crate::cli::{Cli, CompletionArgs};
use crate::output::CliError;

/// `nctool completion <shell>`：向 stdout 输出补全脚本。
pub fn run(args: &CompletionArgs) -> Result<(), CliError> {
    let mut cmd = Cli::command();
    let mut out = std::io::stdout();
    let bin = "nctool";
    match args.shell {
        crate::cli::ShellArg::Bash => generate(shells::Bash, &mut cmd, bin, &mut out),
        crate::cli::ShellArg::Zsh => generate(shells::Zsh, &mut cmd, bin, &mut out),
        crate::cli::ShellArg::Fish => generate(shells::Fish, &mut cmd, bin, &mut out),
        crate::cli::ShellArg::PowerShell => generate(shells::PowerShell, &mut cmd, bin, &mut out),
        crate::cli::ShellArg::Elvish => generate(shells::Elvish, &mut cmd, bin, &mut out),
    }
    Ok(())
}
