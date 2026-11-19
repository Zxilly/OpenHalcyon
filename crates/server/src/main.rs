mod config;
mod peers;
use anyhow::{Context, Result};
use axum::{Router, routing::get};
use clap::{Parser, Subcommand};
use config::Config;
use halcyon_ce_platform::Platform;
use halcyon_data::GameData;
use halcyon_game::Rooms;
use halcyon_platform::{AppState, PlatformConfig};
use std::{path::PathBuf, sync::Arc};
use tokio::net::TcpListener;

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
        Action::Serve { config } => serve(Config::read(&config)?).await,
    }
}

async fn serve(c: Config) -> Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let tls = axum_server::tls_rustls::RustlsConfig::from_pem_file(&c.certificate, &c.private_key)
        .await?;
    let data = c
        .data_dir
        .as_ref()
        .map(GameData::load)
        .transpose()?
        .map(Arc::new);
    let digest = data
        .as_ref()
        .map(|d| d.content_digest.clone())
        .unwrap_or_else(|| "halcyon-validation-lane".into());
    let rooms = Rooms::new(c.game_bind, c.public_host.clone(), c.max_rooms, data);
    let authority = format!("{}:{}", c.public_host, c.platform_listen.port());
    let state = AppState::open(
        PlatformConfig {
            database_path: c.database.clone(),
            public_address: authority.clone(),
            seats_per_match: c.seats_per_match,
        },
        Arc::new(rooms.clone()),
    )?;
    let network = match c.federation {
        Some(peers) => Some(
            peers::Network::start(peers, c.instance_id, digest, state.clone(), rooms.clone())
                .await?,
        ),
        None => None,
    };
    let worker = network.as_ref().map(|n| n.worker.clone());
    let completion = state.clone();
    rooms.set_finish_handler(Arc::new(move |id, result| {
        let completion = completion.clone();
        let worker = worker.clone();
        tokio::task::spawn_blocking(move || {
            if let Some(worker) = worker {
                match worker.finish(id, result.clone()) {
                    Ok(true) => return,
                    Ok(false) => {}
                    Err(e) => {
                        tracing::error!(%id,%e,"recording federated outcome");
                        return;
                    }
                }
            }
            let outcome = if result["reason"] == "admin_terminated" || result["reason"] == "error" {
                completion.abort_match(id)
            } else {
                completion.finish_match(id, result)
            };
            if let Err(error) = outcome {
                tracing::error!(%id,%error,"could not persist room outcome");
            }
        });
    }));
    let ce_cfg = halcyon_ce_platform::Config {
        authority,
        notify_authority: format!("{}:{}", c.public_host, c.notify_listen.port()),
    };
    let key = std::fs::read(c.jwt_secret)?;
    let ce = match &network {
        Some(n) => Platform::with_peers(
            state.clone(),
            ce_cfg,
            &key,
            &c.database,
            c.manifests_dir.as_deref(),
            n.worker.clone(),
        )?,
        None => Platform::new(
            state.clone(),
            ce_cfg,
            &key,
            &c.database,
            c.manifests_dir.as_deref(),
        )?,
    };
    let bootstrap = TcpListener::bind(c.bootstrap_listen)
        .await
        .context("binding bootstrap")?;
    let notify = TcpListener::bind(c.notify_listen)
        .await
        .context("binding notifications")?;
    let mut router = ce
        .rpc()
        .merge(Router::new().route("/health", get(|| async { "ok\n" })));
    if let Some(admin) = c.admin {
        router = router.merge(halcyon_admin::router(
            halcyon_admin::AdminConfig {
                origin: admin.origin,
                username: admin.username,
                password_hash: admin.password_hash,
                instance_id: c.instance_id,
                started_at: chrono::Utc::now(),
                client_build: 147219,
            },
            state,
            rooms,
            network
                .as_ref()
                .map(|n| n.view.clone())
                .unwrap_or_else(|| Arc::new(Vec::new)),
        )?);
    }
    tracing::info!(bootstrap=%c.bootstrap_listen,https=%c.platform_listen,notify=%c.notify_listen,"starting listeners");
    tokio::select! {
        result=axum::serve(bootstrap,ce.bootstrap())=>result?,
        result=axum::serve(notify,ce.notifications())=>result?,
        result=axum_server::bind_rustls(c.platform_listen,tls).serve(router.into_make_service())=>result?,
        signal=tokio::signal::ctrl_c()=>signal?,
    }
    if let Some(n) = network {
        n.shutdown().await?;
    }
    Ok(())
}
