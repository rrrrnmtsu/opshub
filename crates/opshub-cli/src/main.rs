use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use opshub_core::paths::default_db_path;
use opshub_core::Storage;
use opshub_runner::{spawn_agent, AgentProfile, RunnerEvent, WinSize};
use std::io::Write;
use std::path::PathBuf;
use tokio::signal::ctrl_c;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(name = "opshub", version, about = "AI agent orchestrator — MVP CLI")]
struct Cli {
    /// Override database path. Defaults to platform-specific data dir.
    #[arg(long, global = true)]
    db: Option<PathBuf>,

    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Spawn an agent under a PTY and stream its output into the opshub DB.
    Launch {
        /// Path to a YAML profile. If omitted, --command is required.
        #[arg(short, long)]
        profile: Option<PathBuf>,
        /// Logical agent id (required when using --command).
        #[arg(long)]
        id: Option<String>,
        /// Free-form command to run (e.g. `--command "/bin/sh -c 'echo hi'"`).
        /// Tokens are split on whitespace; use a profile file for anything fancier.
        #[arg(long)]
        command: Option<String>,
    },
    /// Full-text search across every persisted session.
    Search {
        query: String,
        #[arg(long, default_value_t = 20)]
        limit: i64,
    },
    /// Print the database file path opshub will use.
    DbPath,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();
    let db_path = match cli.db {
        Some(p) => p,
        None => default_db_path().context("resolve default db path")?,
    };

    match cli.command {
        Cmd::DbPath => {
            println!("{}", db_path.display());
            Ok(())
        }
        Cmd::Search { query, limit } => {
            let storage = Storage::open(&db_path)?;
            let hits = storage.search(&query, limit)?;
            if hits.is_empty() {
                eprintln!("(no matches for {query:?})");
                return Ok(());
            }
            for (id, sid, ts, text) in hits {
                let snippet: String = text.chars().take(160).collect();
                println!("#{id} [{sid}] t={ts}  {snippet}");
            }
            Ok(())
        }
        Cmd::Launch {
            profile,
            id,
            command,
        } => {
            let profile = resolve_profile(profile, id, command)?;
            let storage = Storage::open(&db_path)?;
            let running =
                spawn_agent(profile, storage, WinSize::default()).context("spawn_agent")?;

            tracing::info!(session = %running.session_id, "agent launched");
            let mut rx = running.subscribe();

            // Mirror PTY bytes to our own stdout so the user still sees the
            // agent. The real TUI arrives in a later MVP slice.
            let stream_task = tokio::spawn(async move {
                loop {
                    match rx.recv().await {
                        Ok(RunnerEvent::Output(bytes)) => {
                            let mut out = std::io::stdout().lock();
                            let _ = out.write_all(&bytes);
                            let _ = out.flush();
                        }
                        Ok(RunnerEvent::Exited(code)) => {
                            tracing::info!(code, "agent exited");
                            return code as i32;
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => return -1,
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            tracing::warn!(lagged = n, "broadcast lagged");
                            continue;
                        }
                    }
                }
            });

            tokio::select! {
                exit = stream_task => {
                    let code = exit.unwrap_or(-1);
                    std::process::exit(code);
                }
                _ = ctrl_c() => {
                    eprintln!("\n^C received, detaching (child keeps running)");
                    Ok(())
                }
            }
        }
    }
}

fn resolve_profile(
    profile: Option<PathBuf>,
    id: Option<String>,
    command: Option<String>,
) -> Result<AgentProfile> {
    if let Some(path) = profile {
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("read profile {}", path.display()))?;
        let p: AgentProfile = serde_yaml::from_str(&text)
            .with_context(|| format!("parse profile {}", path.display()))?;
        return Ok(p);
    }
    let cmd = command.context("either --profile or --command is required")?;
    let mut parts = cmd.split_whitespace();
    let bin = parts
        .next()
        .context("--command must contain at least one token")?
        .to_string();
    let args: Vec<String> = parts.map(|s| s.to_string()).collect();
    let id = id.unwrap_or_else(|| format!("adhoc-{bin}"));
    Ok(AgentProfile {
        id,
        kind: "generic".into(),
        command: bin,
        args,
        cwd: None,
        env: vec![],
    })
}
