//! Domain-neutral command line launcher.
use clap::{Parser, Subcommand};
use nctool_cli::composition::{default_home, App};
use nctool_plugin_sdk::*;
use nctool_runtime::config;
use std::{net::IpAddr, path::PathBuf, process::ExitCode};
#[derive(Parser)]
#[command(
    name = "nctool",
    version,
    about = "Jinja template platform with optional domain plugins"
)]
struct Cli {
    #[arg(long, global = true)]
    home: Option<PathBuf>,
    #[arg(long, global = true, default_value = ".")]
    workspace: PathBuf,
    #[arg(long, global = true)]
    profile: Option<String>,
    #[command(subcommand)]
    command: Commands,
}
#[derive(Subcommand)]
enum Commands {
    /// Start the local WebUI.
    Ui {
        #[arg(long, default_value = "127.0.0.1")]
        host: IpAddr,
        #[arg(long, default_value_t = 8788)]
        port: u16,
    },
    /// List registered actions and their schemas.
    Actions,
    /// Run any registered action. '-' reads JSON from stdin.
    Run {
        action: String,
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Manage installed plugins; configuration changes apply at next start.
    Plugins {
        #[command(subcommand)]
        command: PluginCommand,
    },
}
#[derive(Subcommand)]
enum PluginCommand {
    List,
    Install { source: PathBuf },
    Enable { id: String },
    Disable { id: String },
    Uninstall { id: String },
}
fn run(cli: Cli) -> PluginResult<()> {
    let home = cli.home.unwrap_or_else(default_home);
    if let Commands::Plugins { command } = &cli.command {
        let value = match command {
            PluginCommand::Install { source } => {
                json!({"installed":config::install(&home,source)?,"enabled":false,"applies":"next_start"})
            }
            PluginCommand::Enable { id } => {
                if !["template", "math", "nc", "process"].contains(&id.as_str())
                    && !config::installed(&home)?
                        .iter()
                        .any(|(m, _)| &m.descriptor.id == id)
                {
                    return Err(PluginError::new("plugin_missing", id));
                }
                config::set_enabled(&home, id, true)?;
                json!({"enabled":id,"applies":"next_start"})
            }
            PluginCommand::Disable { id } => {
                config::set_enabled(&home, id, false)?;
                json!({"disabled":id,"applies":"next_start"})
            }
            PluginCommand::Uninstall { id } => {
                config::uninstall(&home, id)?;
                json!({"uninstalled":id,"applies":"next_start"})
            }
            PluginCommand::List => {
                let (c, _) = config::load(&home)?;
                json!({"config":c,"installed":config::installed(&home)?.into_iter().map(|(m,_)|m.descriptor).collect::<Vec<_>>(),"builtins":["template"],"nc_bundle_available":cfg!(feature="nc-bundle")})
            }
        };
        println!("{}", serde_json::to_string_pretty(&value)?);
        return Ok(());
    }
    let app = App::boot(&home, &cli.workspace, cli.profile.as_deref())?;
    match cli.command {
        Commands::Actions => println!(
            "{}",
            serde_json::to_string_pretty(&app.runtime.capabilities())?
        ),
        Commands::Run { action, input, out } => {
            use std::io::Read;
            let mut bytes = Vec::new();
            if input.to_str() == Some("-") {
                std::io::stdin()
                    .take(1024 * 1024 + 1)
                    .read_to_end(&mut bytes)?;
            } else {
                std::fs::File::open(input)?
                    .take(1024 * 1024 + 1)
                    .read_to_end(&mut bytes)?;
            }
            if bytes.len() > 1024 * 1024 {
                return Err(PluginError::new("input_limit", "input exceeds 1 MiB"));
            }
            let result = app.runtime.invoke(&action, parse_json(&bytes)?)?;
            if let Some(out) = out {
                let text = result.data["text"]
                    .as_str()
                    .ok_or_else(|| PluginError::new("no_text", "action returned no text output"))?;
                // No output is opened before the complete action has succeeded.
                nctool_runtime::config::write_output(&out, text.as_bytes())?;
            }
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Commands::Ui { host, port } => {
            tokio::runtime::Runtime::new()?.block_on(nctool_cli::server::serve(app, host, port))?
        }
        Commands::Plugins { .. } => unreachable!(),
    }
    Ok(())
}
fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{}", json!({"ok":false,"error":error}));
            ExitCode::FAILURE
        }
    }
}
