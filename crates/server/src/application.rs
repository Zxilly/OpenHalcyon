//! Composes the headless application through Bevy and drives socket I/O on Tokio.
use std::{fs::File, future::Future, io, sync::Arc};

use anyhow::{Context, Result};
use axum::{Router, routing::get};
use bevy_app::{App, Plugin, Startup};
use bevy_ecs::{
    prelude::{Commands, Res, Resource, Schedule},
    schedule::ScheduleLabel,
};
use halcyon_ce_platform::Platform;
use halcyon_data::GameData;
use halcyon_game::Rooms;
use halcyon_platform::{AppState, PlatformConfig};
use tokio::net::TcpListener;

use crate::{config::Config, peers::Network};

#[derive(Resource)]
struct Settings(Config);

#[derive(Resource)]
struct LocalServices {
    rooms: Rooms,
    state: AppState,
    authority: String,
}

#[derive(Resource)]
struct StartupFailure(anyhow::Error);

#[derive(Resource)]
struct FederatedServices(Network);

#[derive(ScheduleLabel, Clone, PartialEq, Eq, Hash, Debug)]
struct StartRoomSettlement;

#[derive(ScheduleLabel, Clone, PartialEq, Eq, Hash, Debug)]
struct StopRoomSettlement;

struct LocalServicesPlugin;

impl Plugin for LocalServicesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, prepare_local_services);
    }
}

struct RoomSettlementPlugin;

impl Plugin for RoomSettlementPlugin {
    fn build(&self, app: &mut App) {
        app.add_schedule(Schedule::new(StartRoomSettlement))
            .add_systems(StartRoomSettlement, install_room_settlement)
            .add_schedule(Schedule::new(StopRoomSettlement))
            .add_systems(StopRoomSettlement, clear_room_settlement);
    }
}

fn prepare_local_services(settings: Res<Settings>, mut commands: Commands) {
    match open_local_services(&settings.0) {
        Ok(local) => commands.insert_resource(local),
        Err(error) => commands.insert_resource(StartupFailure(error)),
    }
}

#[derive(Resource)]
struct DatabaseOwner {
    _lock: File,
}

impl DatabaseOwner {
    fn acquire(c: &Config) -> Result<Self> {
        // A second process would treat the first process's rooms as abandoned.
        let owner = File::options()
            .create(true)
            .truncate(false)
            .write(true)
            .open(c.database.with_added_extension("owner"))?;
        owner
            .try_lock()
            .context("platform database already in use")?;
        Ok(Self { _lock: owner })
    }
}

fn open_local_services(c: &Config) -> Result<LocalServices> {
    let data = c
        .data_dir
        .as_ref()
        .map(|dir| GameData::load_with_extra(dir, c.data_extra_dir.as_deref()))
        .transpose()?
        .map(Arc::new);
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
    let stopped = state.recover_local_rooms(|id| rooms.contains(id))?;
    if stopped > 0 {
        tracing::info!(stopped, "released matches lost on instance restart");
    }
    Ok(LocalServices {
        rooms,
        state,
        authority,
    })
}

fn install_room_settlement(local: Res<LocalServices>, network: Option<Res<FederatedServices>>) {
    let LocalServices { rooms, state, .. } = &*local;
    let network = network.as_deref().map(|services| &services.0);
    let worker = network.map(|n| n.worker.clone());
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
}
fn clear_room_settlement(local: Res<LocalServices>) {
    local.rooms.clear_finish_handler();
}

pub(crate) async fn serve(c: Config, shutdown: impl Future<Output = io::Result<()>>) -> Result<()> {
    let owner = DatabaseOwner::acquire(&c)?;
    let _ = rustls::crypto::ring::default_provider().install_default();
    let tls = axum_server::tls_rustls::RustlsConfig::from_pem_file(&c.certificate, &c.private_key)
        .await?;
    let mut app = App::new();
    app.insert_resource(owner)
        .insert_resource(Settings(c))
        .add_plugins((LocalServicesPlugin, RoomSettlementPlugin));
    // Headless startup runs once. Simulation time and listener polling belong to
    // their separate room schedules and Tokio tasks, respectively.
    app.update();
    if let Some(failure) = app.world_mut().remove_resource::<StartupFailure>() {
        return Err(failure.0);
    }
    let mut settings = app
        .world_mut()
        .remove_resource::<Settings>()
        .context("missing server configuration")?
        .0;
    let local = app.world().resource::<LocalServices>();
    if let Some(peers) = settings.federation.take() {
        let network = Network::start(
            peers,
            settings.instance_id,
            halcyon_sim::rules_digest(local.rooms.game_data().map(Arc::as_ref)),
            local.state.clone(),
            local.rooms.clone(),
        )
        .await?;
        app.insert_resource(FederatedServices(network));
    }
    app.world_mut().run_schedule(StartRoomSettlement);
    let local = app.world().resource::<LocalServices>();
    let network = app
        .world()
        .get_resource::<FederatedServices>()
        .map(|services| &services.0);
    let result = serve_endpoints(settings, tls, local, network, shutdown).await;
    app.world_mut().run_schedule(StopRoomSettlement);
    let network_shutdown = match app.world_mut().remove_resource::<FederatedServices>() {
        Some(services) => services.0.shutdown().await,
        None => Ok(()),
    };
    match result {
        Ok(()) => network_shutdown,
        Err(error) => {
            if let Err(shutdown_error) = network_shutdown {
                tracing::error!(%shutdown_error, "could not shut down federation after listener error");
            }
            Err(error)
        }
    }
}

async fn serve_endpoints(
    c: Config,
    tls: axum_server::tls_rustls::RustlsConfig,
    local: &LocalServices,
    network: Option<&Network>,
    shutdown: impl Future<Output = io::Result<()>>,
) -> Result<()> {
    let LocalServices {
        authority,
        rooms,
        state,
        ..
    } = local;
    let authority = authority.clone();
    let ce_cfg = halcyon_ce_platform::Config {
        authority,
        notify_authority: format!("{}:{}", c.public_host, c.notify_listen.port()),
    };
    let key = tokio::fs::read(&c.jwt_secret).await?;
    let ce = match network {
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
    let mut ce = ce;
    Platform::configure_game_data(&mut ce, rooms.game_data().cloned())?;
    let mut ce = match c.content_file {
        Some(path) => ce.with_content(&path).await?,
        None => ce,
    };
    Platform::configure_ping_sites(&mut ce, c.ping_sites)?;
    Platform::configure_native_state_updates(&mut ce, c.native_state_updates)?;
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
    let player_router = ce.public_api().layer(axum::middleware::map_response(
        |mut response: axum::response::Response| async move {
            response.headers_mut().insert(
                axum::http::header::CACHE_CONTROL,
                axum::http::HeaderValue::from_static("no-store"),
            );
            response
        },
    ));
    let mut router = ce
        .rpc()
        .merge(player_router)
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
            state.clone(),
            rooms.clone(),
            network
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
        signal=shutdown=>signal,
    };
    Ok(result?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn serving_lifecycle_releases_database_owner_after_shutdown_or_bind_failure() {
        let directory = tempfile::tempdir().unwrap();
        Config::init(directory.path(), "localhost".into()).unwrap();
        let path = directory.path().join("server.toml");
        let read_settings = || {
            let mut c = Config::read(&path).unwrap();
            let ephemeral = "127.0.0.1:0".parse().unwrap();
            c.bootstrap_listen = ephemeral;
            c.platform_listen = ephemeral;
            c.notify_listen = ephemeral;
            c.xmpp_listen = None;
            c.admin = None;
            c
        };

        let collision = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut config = read_settings();
        config.notify_listen = collision.local_addr().unwrap();
        let failure = serve(config, std::future::pending()).await.unwrap_err();
        assert_eq!(failure.to_string(), "binding notifications");

        // Use a controlled shutdown future instead of sending a signal to the test runner.
        let config = read_settings();
        let owner_path = config.database.with_added_extension("owner");
        serve(config, async {
            let duplicate = File::options().write(true).open(&owner_path)?;
            assert!(duplicate.try_lock().is_err(), "serve must retain ownership");
            Ok(())
        })
        .await
        .unwrap();
        File::options()
            .write(true)
            .open(owner_path)
            .unwrap()
            .try_lock()
            .unwrap();
    }
}
