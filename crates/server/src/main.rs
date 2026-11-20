mod application;
mod config;
mod data_check;
mod peers;
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use config::Config;
use halcyon_data::GameData;
use std::path::PathBuf;

#[derive(Parser)]
#[command(version, about = "Self-hosted Vainglory CE game server")]
struct Cli {
    #[command(subcommand)]
    action: Action,
}
#[derive(Subcommand)]
enum Action {
    /// Create an instance identity, local TLS certificate and initial config.
    Init {
        #[arg(long, default_value = "runtime")]
        dir: PathBuf,
        #[arg(long, default_value = "localhost")]
        host: String,
    },
    Serve {
        #[arg(long, default_value = "runtime/server.toml")]
        config: PathBuf,
    },
    /// Write a portable JSON list of draft catalog names from decoded CE traffic.
    ExportCatalog {
        #[arg(long)]
        trace: PathBuf,
    },
    /// Validate a local delivery and parse every installed navigation mesh offline.
    CheckData {
        #[arg(long)]
        data_dir: PathBuf,
        #[arg(long)]
        data_extra_dir: Option<PathBuf>,
    },
    /// Inspect or change the voting membership of a trusted pool.
    Pool {
        #[arg(long, default_value = "runtime/server.toml")]
        config: PathBuf,
        #[command(subcommand)]
        action: PoolAction,
    },
}

#[derive(Subcommand)]
enum PoolAction {
    /// Discover the committed membership through the configured peers.
    Members,
    /// Catch up new learners and commit a new equal-vote membership.
    SetVoters {
        #[arg(long, value_delimiter = ',', required = true)]
        expect: Vec<uuid::Uuid>,
        #[arg(long, value_delimiter = ',', required = true)]
        voters: Vec<uuid::Uuid>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    match Cli::parse().action {
        Action::Init { dir, host } => Config::init(&dir, host),
        Action::Serve { config } => {
            application::serve(Config::read(&config)?, tokio::signal::ctrl_c()).await
        }
        Action::CheckData {
            data_dir,
            data_extra_dir,
        } => {
            let data = GameData::load_with_extra(data_dir, data_extra_dir.as_deref())?;
            let report = data_check::inspect(&data)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            Ok(())
        }
        Action::ExportCatalog { trace } => {
            let file = std::fs::File::open(&trace).with_context(|| trace.display().to_string())?;
            let names = halcyon_data::catalog_from_trace(std::io::BufReader::new(file))?;
            anyhow::ensure!(
                !names.is_empty(),
                "trace contains no server draft catalog records"
            );
            serde_json::to_writer_pretty(std::io::stdout().lock(), &names)?;
            Ok(())
        }
        Action::Pool { config, action } => {
            let local = Config::read(&config)?;
            let peers = local
                .federation
                .context("instance has no federation configuration")?;
            let config = halcyon_federation::Config::read(&peers.cluster_file)?;
            anyhow::ensure!(
                config.node_id == local.instance_id,
                "federation identity differs from instance"
            );
            let client = halcyon_federation::PoolAdmin::new(config)?;
            match action {
                PoolAction::Members => println!(
                    "{}",
                    serde_json::to_string_pretty(&client.members().await?)?
                ),
                PoolAction::SetVoters { expect, voters } => {
                    client
                        .change(&halcyon_federation::MemberChange {
                            expected: expect.into_iter().collect(),
                            voters: voters.into_iter().collect(),
                        })
                        .await?
                }
            }
            Ok(())
        }
    }
}
