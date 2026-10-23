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
    /// Explicit history backup, retention preview/apply and restore. Does not start plugins.
    History {
        #[command(subcommand)]
        command: HistoryCommand,
    },
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
        /// Atomically deliver all artifacts into a new directory.
        #[arg(long, conflicts_with = "out")]
        out_dir: Option<PathBuf>,
    },
    /// Inspect or explicitly migrate saved legacy configuration without booting the app.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Manage installed plugins; configuration changes apply at next start.
    Plugins {
        #[command(subcommand)]
        command: PluginCommand,
    },
}
#[derive(Subcommand)]
enum ConfigCommand {
    /// Read-only legacy configuration report and proposed configuration/composition.
    Check,
    /// Apply migration after checking the exact fingerprint, preserving a backup.
    Migrate {
        #[arg(long)]
        expected: String,
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
#[derive(Subcommand)]
enum HistoryCommand {
    Backup {
        #[arg(long)]
        out: PathBuf,
    },
    Restore {
        #[arg(long)]
        archive: PathBuf,
    },
    Prune {
        #[arg(long)]
        before_ms: u64,
        #[arg(long, default_value_t = 100)]
        keep_latest: usize,
        #[arg(long, requires = "backup")]
        apply: Option<String>,
        #[arg(long, requires = "apply")]
        backup: Option<PathBuf>,
    },
}
fn run(cli: Cli) -> PluginResult<()> {
    if let Commands::History { command } = &cli.command {
        let result = match command {
            HistoryCommand::Backup { out } => nctool_cli::history::backup(&cli.workspace, out)?,
            HistoryCommand::Restore { archive } => {
                nctool_cli::history::restore(&cli.workspace, archive)?
            }
            HistoryCommand::Prune {
                before_ms,
                keep_latest,
                apply,
                backup,
            } => nctool_cli::history::prune(
                &cli.workspace,
                *before_ms,
                *keep_latest,
                apply.as_deref(),
                backup.as_deref(),
            )?,
        };
        println!("{}", serde_json::to_string_pretty(&result)?);
        return Ok(());
    }
    if let Commands::Config { command } = &cli.command {
        let home = cli.home.as_deref().ok_or_else(|| {
            PluginError::new(
                "explicit_home_required",
                "配置检查/迁移必须显式指定 --home <目录>，不会自动读取或写入真实默认 home。",
            )
        })?;
        let value = match command {
            ConfigCommand::Check => App::migration_check_at(home)?,
            ConfigCommand::Migrate { expected } => {
                App::migration_apply_at(home, serde_json::from_str(expected)?)?
            }
        };
        println!("{}", serde_json::to_string_pretty(&value)?);
        return Ok(());
    }
    let home = cli.home.unwrap_or_else(default_home);
    if let Commands::Plugins { command } = &cli.command {
        let value = match command {
            PluginCommand::Install { source } => {
                json!({"installed":config::install(&home,source)?,"enabled":false,"applies":"next_start"})
            }
            PluginCommand::Enable { id } => {
                config::ensure_plugin_allowed(id)?;
                #[cfg(not(feature = "math"))]
                if id == "math" {
                    return Err(PluginError::new(
                        "feature_unavailable",
                        "此构建未包含 math 扩展；请使用 --features math 构建。",
                    ));
                }
                if !config::BUILTIN_PLUGIN_IDS.contains(&id.as_str())
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
                json!({"config":c,"installed":config::installed(&home)?.into_iter().map(|(m,_)|m.descriptor).collect::<Vec<_>>(),"builtins":config::BUILTIN_PLUGIN_IDS,"math_available":cfg!(feature="math"),"retired_reserved_ids":config::RETIRED_PLUGIN_IDS})
            }
        };
        println!("{}", serde_json::to_string_pretty(&value)?);
        return Ok(());
    }
    let app = if matches!(&cli.command, Commands::Run { .. } | Commands::Actions) {
        App::boot_for_actions(&home, &cli.workspace, cli.profile.as_deref())?
    } else {
        App::boot(&home, &cli.workspace, cli.profile.as_deref())?
    };
    match cli.command {
        Commands::Actions => println!(
            "{}",
            serde_json::to_string_pretty(&app.runtime.capabilities())?
        ),
        Commands::Run {
            action,
            input,
            out,
            out_dir,
        } => {
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
            if let Some(directory) = out_dir {
                deliver_artifacts(&directory, &result.artifacts)?;
            }
            if let Some(out) = out {
                if result.artifacts.len() > 1 {
                    return Err(PluginError::new(
                        "multiple_artifacts",
                        "多文件结果请使用 --out-dir",
                    ));
                }
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
        Commands::Plugins { .. } | Commands::Config { .. } | Commands::History { .. } => {
            unreachable!()
        }
    }
    Ok(())
}
fn deliver_artifacts(directory: &std::path::Path, artifacts: &[Artifact]) -> PluginResult<()> {
    if artifacts.is_empty() {
        return Err(PluginError::new(
            "no_artifacts",
            "action returned no artifacts",
        ));
    }
    if directory.exists() {
        return Err(PluginError::new(
            "conflict",
            "output directory already exists",
        ));
    }
    let mut names = std::collections::BTreeSet::new();
    for artifact in artifacts {
        if artifact.name.is_empty()
            || artifact.name == "."
            || artifact.name == ".."
            || artifact.name.contains(['/', '\\', ':'])
            || artifact.name.chars().any(char::is_control)
            || !names.insert(&artifact.name)
        {
            return Err(PluginError::new(
                "invalid_artifact",
                "artifact names must be unique file names",
            ));
        }
    }
    let parent = directory
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(std::path::Path::new("."));
    std::fs::create_dir_all(parent)?;
    let staging = parent.join(format!(
        ".nctool-output-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    std::fs::create_dir(&staging)?;
    let result = (|| {
        for artifact in artifacts {
            use std::io::Write;
            // Let the target filesystem detect aliases as well as exact duplicates.
            // Staging is private and is only published after every file succeeds.
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(staging.join(&artifact.name))
                .map_err(|error| {
                    if error.kind() == std::io::ErrorKind::AlreadyExists {
                        PluginError::new(
                            "invalid_artifact",
                            "artifact names resolve to the same file",
                        )
                    } else {
                        error.into()
                    }
                })?;
            file.write_all(artifact.text.as_bytes())?;
            file.sync_all()?;
        }
        if directory.exists() {
            return Err(PluginError::new(
                "conflict",
                "output directory appeared during delivery",
            ));
        }
        std::fs::rename(&staging, directory)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    result
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

#[cfg(test)]
mod tests {
    use super::*;

    // Tests in this binary run on parallel threads, and the system clock can hand
    // two of them the same nanosecond value; the counter keeps each root distinct
    // so one test's cleanup cannot race another's `create_dir`.
    static NEXT_OUTPUT_ROOT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    struct OutputRoot(PathBuf);
    impl OutputRoot {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "nctool-delivery-{}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT_OUTPUT_ROOT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for OutputRoot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn artifact(name: &str, text: &str) -> Artifact {
        Artifact {
            name: name.into(),
            media_type: "text/plain".into(),
            text: text.into(),
        }
    }
    #[test]
    fn delivery_publishes_all_files_without_overwriting_existing_output() {
        let root = OutputRoot::new();
        let output = root.0.join("output");
        let artifacts = [
            artifact("first.txt", "FIRST"),
            artifact("second.txt", "SECOND"),
        ];
        deliver_artifacts(&output, &artifacts).unwrap();
        assert_eq!(
            std::fs::read_to_string(output.join("first.txt")).unwrap(),
            "FIRST"
        );
        assert_eq!(
            std::fs::read_to_string(output.join("second.txt")).unwrap(),
            "SECOND"
        );
        assert_eq!(
            deliver_artifacts(&output, &artifacts).unwrap_err().code,
            "conflict"
        );
        assert_eq!(std::fs::read_dir(&root.0).unwrap().count(), 1);
    }
    #[test]
    fn invalid_delivery_leaves_no_partial_output() {
        for names in [["same.txt", "same.txt"], ["valid.txt", "../escape.txt"]] {
            let root = OutputRoot::new();
            assert!(deliver_artifacts(
                &root.0.join("output"),
                &[artifact(names[0], "FIRST"), artifact(names[1], "SECOND")]
            )
            .is_err());
            assert_eq!(std::fs::read_dir(&root.0).unwrap().count(), 0);
        }
    }
    #[cfg(windows)]
    #[test]
    fn filesystem_aliases_reject_the_entire_delivery() {
        for names in [["A.txt", "a.txt"], ["a.txt", "a.txt."], ["a.txt", "a.txt "]] {
            let root = OutputRoot::new();
            let error = deliver_artifacts(
                &root.0.join("output"),
                &[artifact(names[0], "FIRST"), artifact(names[1], "SECOND")],
            )
            .unwrap_err();
            assert_eq!(error.code, "invalid_artifact");
            assert_eq!(std::fs::read_dir(&root.0).unwrap().count(), 0);
        }
    }
}
