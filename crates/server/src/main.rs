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
        Action::Serve { config } => serve(Config::read(&config)?).await,
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

async fn serve(c: Config) -> Result<()> {
    // An instance owns its local rooms. Two processes sharing a database would
    // each mistake the other process's active rooms for abandoned games.
    let owner = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(c.database.with_added_extension("owner"))?;
    owner
        .try_lock()
        .context("platform database already in use")?;
    let _ = rustls::crypto::ring::default_provider().install_default();
    let tls = axum_server::tls_rustls::RustlsConfig::from_pem_file(&c.certificate, &c.private_key)
        .await?;
    let data = c
        .data_dir
        .as_ref()
        .map(GameData::load)
        .transpose()?
        .map(Arc::new);
    let digest = halcyon_sim::rules_digest(data.as_deref());
    let rooms = Rooms::with_policy(
        c.game_bind,
        c.public_host.clone(),
        c.max_rooms,
        data,
        c.admission,
        c.game_port_range,
    )?;
    if let Some(dir) = &c.replay_dir {
        rooms.set_replay_directory(dir)?;
    }
    let authority = format!("{}:{}", c.public_host, c.platform_listen.port());
    let state = AppState::open(
        PlatformConfig {
            database_path: c.database.clone(),
            public_address: authority.clone(),
            seats_per_match: c.seats_per_match,
        },
        Arc::new(rooms.clone()),
    )?;
    let stopped = state.recover_local_rooms(|id| rooms.detail(id).is_some())?;
    if stopped > 0 {
        tracing::info!(stopped, "released matches lost on instance restart");
    }
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
    let mut ce = match c.content_file {
        Some(path) => ce.with_content(&path).await?,
        None => ce,
    };
    if let Some(provider) = c.provider {
        Platform::configure_provider(&mut ce, provider)?;
    }
    let (ce, xmpp) = if let Some(addr) = c.xmpp_listen {
        let ce = ce.with_xmpp(&format!("{}:{}", c.public_host, addr.port()))?;
        let mut chat_tls = (*tls.get_inner()).clone();
        chat_tls.alpn_protocols.clear();
        let chat =
            halcyon_chat::Server::new(c.public_host.clone(), ce.clone(), Arc::new(chat_tls))?;
        let listener = TcpListener::bind(addr).await.context("binding XMPP")?;
        (ce, Some((chat, listener)))
    } else {
        (ce, None)
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
            rooms.clone(),
            network
                .as_ref()
                .map(|n| n.view.clone())
                .unwrap_or_else(|| Arc::new(Vec::new)),
        )?);
    }
    tracing::info!(bootstrap=%c.bootstrap_listen,https=%c.platform_listen,notify=%c.notify_listen,"starting listeners");
    let chat = async move {
        match xmpp {
            Some((server, listener)) => server.serve(listener).await,
            None => std::future::pending().await,
        }
    };
    let result = tokio::select! {
        result=axum::serve(bootstrap,ce.bootstrap())=>result,
        result=axum::serve(notify,ce.notifications())=>result,
        result=axum_server::bind_rustls(c.platform_listen,tls).serve(router.into_make_service())=>result,
        result=chat=>result,
        signal=tokio::signal::ctrl_c()=>signal,
    };
    rooms.clear_finish_handler();
    if let Some(n) = network {
        n.shutdown().await?;
    }
    Ok(result?)
}
