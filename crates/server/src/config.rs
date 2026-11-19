use anyhow::{Context, Result, ensure};
use argon2::{Argon2, PasswordHasher, password_hash::SaltString};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    net::{IpAddr, SocketAddr},
    path::{Path, PathBuf},
};
use uuid::Uuid;

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub instance_id: Uuid,
    pub public_host: String,
    pub bootstrap_listen: SocketAddr,
    pub platform_listen: SocketAddr,
    pub notify_listen: SocketAddr,
    pub xmpp_listen: Option<SocketAddr>,
    pub game_bind: IpAddr,
    pub game_port_range: Option<halcyon_game::PortRange>,
    #[serde(default)]
    pub admission: halcyon_game::AdmissionPolicy,
    pub max_rooms: usize,
    pub seats_per_match: usize,
    pub database: PathBuf,
    pub jwt_secret: PathBuf,
    pub certificate: PathBuf,
    pub private_key: PathBuf,
    pub data_dir: Option<PathBuf>,
    pub replay_dir: Option<PathBuf>,
    pub manifests_dir: Option<PathBuf>,
    pub content_file: Option<PathBuf>,
    pub provider: Option<halcyon_ce_platform::ProviderConfig>,
    pub admin: Option<Admin>,
    pub federation: Option<Peers>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Peers {
    /// JSON configuration for the node identities, mTLS and Raft database.
    pub cluster_file: PathBuf,
    pub journal: PathBuf,
    #[serde(default)]
    pub latency_ms: std::collections::BTreeMap<Uuid, u32>,
    pub max_latency_ms: u32,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Admin {
    pub origin: String,
    pub username: String,
    pub password_hash: String,
}

fn create(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut options = fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(path)
        .with_context(|| format!("creating {}", path.display()))?
        .write_all(bytes)?;
    Ok(())
}

impl Config {
    pub fn init(dir: &Path, host: String) -> Result<()> {
        ensure!(
            host.parse::<axum::http::uri::Authority>().is_ok() && !host.contains(['/', ':', '@']),
            "expected DNS name or IPv4 address"
        );
        fs::create_dir_all(dir)?;
        let dir = dir.canonicalize()?;
        ensure!(
            fs::read_dir(&dir)?.next().is_none(),
            "initialization needs an empty directory"
        );
        let bound = IpAddr::from([0, 0, 0, 0]);
        let password = Uuid::new_v4().simple().to_string();
        let salt = SaltString::encode_b64(Uuid::new_v4().as_bytes())
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let hash = Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map_err(|e| anyhow::anyhow!(e.to_string()))?
            .to_string();
        let config = Self {
            instance_id: Uuid::new_v4(),
            public_host: host.clone(),
            bootstrap_listen: SocketAddr::new(bound, 8080),
            platform_listen: SocketAddr::new(bound, 8443),
            notify_listen: SocketAddr::new(bound, 2112),
            xmpp_listen: Some(SocketAddr::new(bound, 5222)),
            game_bind: bound,
            game_port_range: None,
            admission: Default::default(),
            max_rooms: 8,
            seats_per_match: 10,
            database: "platform.sqlite".into(),
            jwt_secret: "jwt.key".into(),
            certificate: "tls.pem".into(),
            private_key: "tls.key".into(),
            data_dir: None,
            replay_dir: None,
            manifests_dir: None,
            content_file: None,
            provider: None,
            federation: None,
            admin: Some(Admin {
                origin: format!("https://{host}:8443"),
                username: "admin".into(),
                password_hash: hash,
            }),
        };
        let cert = rcgen::generate_simple_self_signed(vec![host, "localhost".into()])?;
        create(
            &dir.join(&config.private_key),
            cert.key_pair.serialize_pem().as_bytes(),
        )?;
        create(&dir.join(&config.certificate), cert.cert.pem().as_bytes())?;
        let secret = [*Uuid::new_v4().as_bytes(), *Uuid::new_v4().as_bytes()].concat();
        create(&dir.join(&config.jwt_secret), &secret)?;
        create(&dir.join("initial-admin-password"), password.as_bytes())?;
        create(
            &dir.join("server.toml"),
            toml::to_string_pretty(&config)?.as_bytes(),
        )
    }
    pub fn read(path: &Path) -> Result<Self> {
        let mut c: Self = toml::from_str(
            &fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?,
        )?;
        let base = path.parent().context("missing configuration directory")?;
        for dest in [
            &mut c.database,
            &mut c.jwt_secret,
            &mut c.certificate,
            &mut c.private_key,
        ]
        .into_iter()
        .chain(c.data_dir.iter_mut())
        .chain(c.replay_dir.iter_mut())
        .chain(c.manifests_dir.iter_mut())
        .chain(c.content_file.iter_mut())
        {
            if dest.is_relative() {
                *dest = base.join(&*dest);
            }
        }
        ensure!(
            c.max_rooms > 0 && (1..=10).contains(&c.seats_per_match),
            "invalid game capacity"
        );
        if let Some(peers) = &mut c.federation {
            peers.cluster_file = base.join(&peers.cluster_file);
            peers.journal = base.join(&peers.journal);
        }
        if let Some(provider) = &mut c.provider {
            provider.public_key_pem = base.join(&provider.public_key_pem);
        }
        Ok(c)
    }
}

#[cfg(test)]
mod latency_config_tests {
    use super::Peers;

    #[test]
    fn omitted_and_empty_rtt_maps_deserialize_to_auto_mode() {
        let base = "cluster_file = 'pool.json'\njournal = 'host.sqlite'\nmax_latency_ms = 90\n";
        let omitted: Peers = toml::from_str(base).unwrap();
        assert!(omitted.latency_ms.is_empty());
        let empty: Peers = toml::from_str(&format!("{base}[latency_ms]\n")).unwrap();
        assert!(empty.latency_ms.is_empty());
        let explicit: Peers = toml::from_str(&format!(
            "{base}[latency_ms]\n'00000000-0000-0000-0000-000000000001' = 27\n"
        ))
        .unwrap();
        assert_eq!(explicit.latency_ms[&uuid::Uuid::from_u128(1)], 27);
    }
}
