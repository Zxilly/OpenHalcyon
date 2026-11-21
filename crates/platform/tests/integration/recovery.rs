use halcyon_platform::{AppState, HostedMatch, MatchHost, PlatformConfig, Reservation, Result};
use std::sync::Arc;

struct LocalHost;

impl MatchHost for LocalHost {
    fn reserve(&self, _: Reservation) -> Result<HostedMatch> {
        Ok(HostedMatch {
            address: "127.0.0.1:7010".into(),
        })
    }
}

#[tokio::test]
async fn abandoned_local_room_releases_players_on_restart() {
    let dir = tempfile::tempdir().unwrap();
    let config = PlatformConfig {
        database_path: dir.path().join("local.sqlite"),
        public_address: "127.0.0.1:8443".into(),
        seats_per_match: 1,
    };
    let state = AppState::open(config.clone(), Arc::new(LocalHost)).unwrap();
    let guest = state.create_guest("Returning".into()).await.unwrap();
    state
        .join(&guest.session_token, "solo".into())
        .await
        .unwrap();
    let room = state
        .profile(&guest.session_token)
        .await
        .unwrap()
        .lobby
        .unwrap()
        .match_id
        .unwrap();
    state.accept(&guest.session_token, room).await.unwrap();
    assert_eq!(state.recover_local_rooms(|id| id == room).unwrap(), 0);
    drop(state);
    let resumed = AppState::open(config, Arc::new(LocalHost)).unwrap();
    assert_eq!(resumed.recover_local_rooms(|_| false).unwrap(), 1);
    assert_eq!(resumed.recover_local_rooms(|_| false).unwrap(), 0);
    let identity = resumed
        .resume_guest(guest.recovery_token.unwrap())
        .await
        .unwrap();
    assert_eq!(identity.player_id, guest.player_id);
    assert!(
        resumed
            .profile(&identity.session_token)
            .await
            .unwrap()
            .lobby
            .is_none()
    );
}
