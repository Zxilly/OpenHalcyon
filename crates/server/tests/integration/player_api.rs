use halcyon_platform::{Chat, CreateGuest, Queue, ResumeGuest};
use std::{
    fs,
    net::TcpListener,
    process::{Child, Command, Stdio},
    time::Duration,
};

use reqwest::{Certificate, Client, StatusCode};
use serde_json::Value;

struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[tokio::test]
async fn production_https_mounts_player_lifecycle() {
    let dir = tempfile::tempdir().unwrap();
    let certificate = rcgen::generate_simple_self_signed(vec!["127.0.0.1".into()]).unwrap();
    let pem = certificate.cert.pem();
    fs::write(dir.path().join("tls.pem"), &pem).unwrap();
    fs::write(
        dir.path().join("tls.key"),
        certificate.key_pair.serialize_pem(),
    )
    .unwrap();
    let secret = [
        *uuid::Uuid::new_v4().as_bytes(),
        *uuid::Uuid::new_v4().as_bytes(),
    ]
    .concat();
    fs::write(dir.path().join("jwt.key"), secret).unwrap();
    // Reserve distinct ports until the child is ready to start binding.
    let listeners = [(); 3].map(|()| TcpListener::bind("127.0.0.1:0").unwrap());
    let [bootstrap, https, notify] = listeners.each_ref().map(|sock| sock.local_addr().unwrap());
    let config = dir.path().join("server.toml");
    let instance = uuid::Uuid::new_v4();
    fs::write(
        &config,
        format!(
            r#"
instance_id = "{instance}"
public_host = "127.0.0.1"
bootstrap_listen = "{bootstrap}"
platform_listen = "{https}"
notify_listen = "{notify}"
game_bind = "127.0.0.1"
max_rooms = 2
seats_per_match = 2
database = "platform.sqlite"
jwt_secret = "jwt.key"
certificate = "tls.pem"
private_key = "tls.key"
"#
        ),
    )
    .unwrap();
    drop(listeners);
    let mut server = Server(
        Command::new(env!("CARGO_BIN_EXE_openhalcyon"))
            .args(["serve", "--config"])
            .arg(config)
            .env("TOKIO_WORKER_THREADS", "2")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let client = Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(2))
        .add_root_certificate(Certificate::from_pem(pem.as_bytes()).unwrap())
        .build()
        .unwrap();
    let origin = format!("https://{https}");
    let api = format!("{origin}/api/v1");
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            assert!(server.0.try_wait().unwrap().is_none(), "serve exited");
            if let Ok(response) = client.get(format!("{origin}/health")).send().await {
                assert_eq!(response.status(), StatusCode::OK);
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("HTTPS listener did not become ready");
    let address = client
        .get(format!("{origin}/bootstrap"))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .text()
        .await
        .unwrap();
    assert_eq!(address.trim_end(), https.to_string());
    let guest_response = client
        .post(format!("{api}/guests"))
        .json(&CreateGuest {
            name: "REST guest".to_string(),
        })
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    assert_eq!(guest_response.headers()["cache-control"], "no-store");
    let guest: Value = guest_response.json().await.unwrap();
    let resumed: Value = client
        .post(format!("{api}/sessions"))
        .json(&ResumeGuest {
            recovery_token: guest["recoveryToken"].as_str().unwrap().to_owned(),
        })
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(resumed["playerId"], guest["playerId"]);
    assert!(resumed.get("recoveryToken").is_none());
    let me = format!("{api}/me");
    let old = guest["sessionToken"].as_str().unwrap();
    let token = resumed["sessionToken"].as_str().unwrap();
    for denied in [client.get(&me), client.get(&me).bearer_auth(old)] {
        assert_eq!(
            denied.send().await.unwrap().status(),
            StatusCode::UNAUTHORIZED
        );
    }
    let queue = format!("{api}/queue");
    // Even on a two-seat instance, each solo queue must offer a match
    // immediately for one guest without a difficulty override.
    for mode in ["5v5_bots_solo", "solo"] {
        assert_eq!(
            client
                .post(&queue)
                .bearer_auth(token)
                .json(&Queue {
                    mode: mode.to_string(),
                    difficulty: None
                })
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::NO_CONTENT
        );
        let profile: Value = client
            .get(&me)
            .bearer_auth(token)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(profile["lobby"]["mode"], mode);
        assert_eq!(profile["lobby"]["phase"], "offered");
        assert!(profile["lobby"]["matchId"].is_string());
        assert_eq!(
            client
                .delete(&queue)
                .bearer_auth(token)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::NO_CONTENT
        );
    }
    assert_eq!(
        client
            .post(&queue)
            .bearer_auth(token)
            .json(&Queue {
                mode: "5v5_pvp_casual".to_string(),
                difficulty: None
            })
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
    );
    let profile: Value = client
        .get(&me)
        .bearer_auth(token)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(profile["playerId"], resumed["playerId"]);
    assert_eq!(profile["name"], "REST guest");
    assert_eq!(profile["lobby"]["mode"], "5v5_pvp_casual");
    assert_eq!(profile["lobby"]["phase"], "queued");
    let events: Value = client
        .get(format!("{api}/events"))
        .bearer_auth(token)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(events["events"].as_array().unwrap().iter().any(|event| {
        event["body"]
            == serde_json::to_value(QueueEvent {
                r#type: "queued",
                mode: "5v5_pvp_casual",
            })
            .unwrap()
    }));
    let second: Value = client
        .post(format!("{api}/guests"))
        .json(&CreateGuest {
            name: "Second guest".to_string(),
        })
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let second_token = second["sessionToken"].as_str().unwrap();
    assert_eq!(
        client
            .post(&queue)
            .bearer_auth(second_token)
            .json(&Queue {
                mode: "5v5_pvp_casual".to_string(),
                difficulty: None
            })
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
    );
    let offered: Value = client
        .get(&me)
        .bearer_auth(token)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(offered["lobby"]["phase"], "offered");
    let match_id = offered["lobby"]["matchId"].as_str().unwrap();
    for participant in [token, second_token] {
        assert_eq!(
            client
                .post(format!("{api}/matches/{match_id}/accept"))
                .bearer_auth(participant)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::NO_CONTENT
        );
    }
    assert_eq!(
        client
            .post(format!("{api}/matches/{match_id}/chat"))
            .bearer_auth(token)
            .json(&Chat {
                text: "Hello from HTTPS".to_string()
            })
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
    );
    let received: Value = client
        .get(format!("{api}/events"))
        .bearer_auth(second_token)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(
        received["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|event| event["body"]["type"] == "chat"
                && event["body"]["text"] == "Hello from HTTPS")
    );
    assert_eq!(
        client
            .delete(queue)
            .bearer_auth(token)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    let cleartext = client
        .get(format!("http://{bootstrap}/api/v1/me"))
        .send()
        .await
        .unwrap();
    assert_eq!(cleartext.status(), StatusCode::NOT_FOUND);
}

#[derive(serde::Serialize)]
struct QueueEvent<'a> {
    r#type: &'a str,
    mode: &'a str,
}
