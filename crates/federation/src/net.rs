use super::*;
use anyhow::{Context, ensure};
use axum::body::Body;
use bytes::Bytes;
use http_body_util::{BodyExt, Full, Limited};
use hyper::Request;
use hyper_util::rt::TokioIo;
use openraft::{
    error::{InstallSnapshotError, RPCError, RaftError, RemoteError, Unreachable},
    network::{RPCOption, RaftNetwork, RaftNetworkFactory},
    raft::*,
};
use rustls::{
    ClientConfig, RootCertStore, ServerConfig,
    pki_types::{CertificateDer, PrivateKeyDer, ServerName},
    server::WebPkiClientVerifier,
};
use std::{fs::File, io::BufReader, path::Path, sync::Arc, time::Duration};
use tokio::net::TcpStream;
use tokio_rustls::{TlsAcceptor, TlsConnector};

pub(crate) const MAX_BODY: usize = 16 * 1024 * 1024;
type Sender = hyper::client::conn::http1::SendRequest<Full<Bytes>>;
type Rpc<T, E = openraft::error::Infallible> =
    Result<T, RPCError<Uuid, BasicNode, RaftError<Uuid, E>>>;
fn certs(path: &Path) -> anyhow::Result<Vec<CertificateDer<'static>>> {
    let certs = rustls_pemfile::certs(&mut BufReader::new(File::open(path)?))
        .collect::<Result<Vec<_>, _>>()?;
    ensure!(
        !certs.is_empty(),
        "empty certificate file {}",
        path.display()
    );
    Ok(certs)
}
fn key(path: &Path) -> anyhow::Result<PrivateKeyDer<'static>> {
    rustls_pemfile::private_key(&mut BufReader::new(File::open(path)?))?
        .context("missing private key")
}
#[derive(Clone)]
pub(crate) struct Transport {
    pub cfg: Arc<Config>,
    pub accept: TlsAcceptor,
    connect: TlsConnector,
    pins: Arc<BTreeMap<Vec<u8>, Uuid>>,
    // One idle HTTP connection per peer. Concurrent votes and capacity reads
    // may use separate sockets instead of waiting for a snapshot to complete.
    idle: Arc<std::sync::Mutex<BTreeMap<Uuid, Sender>>>,
}
impl Transport {
    pub fn new(cfg: Config) -> anyhow::Result<Self> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let mut roots = RootCertStore::empty();
        for c in certs(&cfg.ca)? {
            roots.add(c)?;
        }
        let client = ClientConfig::builder_with_provider(provider.clone())
            .with_safe_default_protocol_versions()?
            .with_root_certificates(roots.clone())
            .with_client_auth_cert(certs(&cfg.cert)?, key(&cfg.key)?)?;
        let verifier =
            WebPkiClientVerifier::builder_with_provider(Arc::new(roots), provider.clone())
                .build()?;
        let server = ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()?
            .with_client_cert_verifier(verifier)
            .with_single_cert(certs(&cfg.cert)?, key(&cfg.key)?)?;
        let mut pins = BTreeMap::new();
        for (id, peer) in &cfg.peers {
            ensure!(
                pins.insert(certs(&peer.cert)?[0].to_vec(), *id).is_none(),
                "duplicate peer certificate"
            );
        }
        ensure!(
            pins.get(certs(&cfg.cert)?[0].as_ref()) == Some(&cfg.node_id),
            "local certificate must match configured member"
        );
        Ok(Self {
            cfg: Arc::new(cfg),
            connect: TlsConnector::from(Arc::new(client)),
            accept: TlsAcceptor::from(Arc::new(server)),
            pins: Arc::new(pins),
            idle: Default::default(),
        })
    }
    pub fn identity(&self, chain: Option<&[CertificateDer]>) -> anyhow::Result<Uuid> {
        chain
            .and_then(|v| v.first())
            .and_then(|c| self.pins.get(c.as_ref()))
            .copied()
            .context("unrecognized peer")
    }
    async fn sender(&self, id: Uuid, addr: &str) -> anyhow::Result<Sender> {
        if let Some(sender) = self.idle.lock().unwrap().remove(&id)
            && !sender.is_closed()
        {
            return Ok(sender);
        }
        let authority: hyper::http::uri::Authority = addr.parse()?;
        let name = ServerName::try_from(authority.host().trim_matches(['[', ']']).to_owned())?;
        let socket = TcpStream::connect(addr).await?;
        socket.set_nodelay(true)?;
        let tls = self.connect.connect(name, socket).await?;
        ensure!(
            self.identity(tls.get_ref().1.peer_certificates())? == id,
            "peer certificate mismatch"
        );
        let (sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(tls)).await?;
        tokio::spawn(conn);
        Ok(sender)
    }
    pub async fn post<I: Serialize + Sync, O: DeserializeOwned>(
        &self,
        id: Uuid,
        path: &str,
        input: &I,
        timeout: Duration,
    ) -> anyhow::Result<O> {
        tokio::time::timeout(timeout, async {
            let addr = &self
                .cfg
                .peers
                .get(&id)
                .context("unconfigured peer")?
                .address;
            let mut sender = self.sender(id, addr).await?;
            let req = Request::post(path)
                .header("host", addr)
                .header("x-halcyon-pool", self.cfg.pool_id.to_string())
                .header("content-type", "application/cbor")
                .body(Full::new(Bytes::from(encode(input)?)))?;
            let res = sender.send_request(req).await?;
            ensure!(
                res.status().is_success(),
                "peer {id} returned {}",
                res.status()
            );
            let b = Limited::new(res.into_body(), MAX_BODY)
                .collect()
                .await
                .map_err(|e| anyhow::anyhow!("{e}"))?
                .to_bytes();
            self.idle.lock().unwrap().entry(id).or_insert(sender);
            decode(&b)
        })
        .await?
    }
}
pub(crate) struct Link {
    target: Uuid,
    transport: Transport,
}
impl RaftNetworkFactory<Types> for Transport {
    type Network = Link;
    async fn new_client(&mut self, target: Uuid, _: &BasicNode) -> Link {
        Link {
            target,
            transport: self.clone(),
        }
    }
}
impl Link {
    async fn call<
        I: Serialize + Sync,
        O: DeserializeOwned,
        E: std::error::Error + DeserializeOwned,
    >(
        &self,
        path: &str,
        req: &I,
        option: RPCOption,
    ) -> Rpc<O, E> {
        let res: Result<O, RaftError<Uuid, E>> = self
            .transport
            .post(self.target, path, req, option.hard_ttl())
            .await
            .map_err(|e| {
                RPCError::Unreachable(Unreachable::new(&std::io::Error::other(e.to_string())))
            })?;
        res.map_err(|e| RPCError::RemoteError(RemoteError::new(self.target, e)))
    }
}
impl RaftNetwork<Types> for Link {
    async fn append_entries(
        &mut self,
        rpc: AppendEntriesRequest<Types>,
        option: RPCOption,
    ) -> Rpc<AppendEntriesResponse<Uuid>> {
        self.call("/internal/append", &rpc, option).await
    }
    async fn vote(&mut self, rpc: VoteRequest<Uuid>, option: RPCOption) -> Rpc<VoteResponse<Uuid>> {
        self.call("/internal/vote", &rpc, option).await
    }
    async fn install_snapshot(
        &mut self,
        rpc: InstallSnapshotRequest<Types>,
        option: RPCOption,
    ) -> Rpc<InstallSnapshotResponse<Uuid>, InstallSnapshotError> {
        self.call("/internal/snapshot", &rpc, option).await
    }
}
pub(crate) fn cbor(v: &impl Serialize) -> anyhow::Result<hyper::Response<Body>> {
    Ok(hyper::Response::builder()
        .header("content-type", "application/cbor")
        .body(Body::from(encode(v)?))?)
}
