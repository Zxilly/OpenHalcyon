use halcyon_federation::{Bootstrap, Config as FederationConfig, Peer};
use rcgen::{BasicConstraints, CertificateParams, IsCa, KeyPair};
use std::{collections::BTreeMap, fs, net::TcpListener};
use uuid::Uuid;
pub fn configs(path: &std::path::Path) -> (Vec<FederationConfig>, Vec<TcpListener>) {
    let ca_key = KeyPair::generate().unwrap();
    let mut par = CertificateParams::new(Vec::new()).unwrap();
    par.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let ca = par.self_signed(&ca_key).unwrap();
    let ca_path = path.join("ca.pem");
    fs::write(&ca_path, ca.pem()).unwrap();
    let pool_id = Uuid::new_v4();
    let mut peers = BTreeMap::new();
    let mut configs = Vec::new();
    let mut reservations = Vec::new();
    for i in 0..3 {
        let node_id = Uuid::from_u128(i + 1);
        let key = KeyPair::generate().unwrap();
        let mut par = CertificateParams::new(vec!["127.0.0.1".into()]).unwrap();
        par.distinguished_name
            .push(rcgen::DnType::CommonName, node_id.to_string());
        let leaf = par.signed_by(&key, &ca, &ca_key).unwrap();
        let cert = path.join(format!("{i}.pem"));
        let privkey = path.join(format!("{i}.key"));
        fs::write(&cert, leaf.pem()).unwrap();
        fs::write(&privkey, key.serialize_pem()).unwrap();
        let sock = TcpListener::bind("127.0.0.1:0").unwrap();
        let listen = sock.local_addr().unwrap();
        reservations.push(sock);
        peers.insert(
            node_id,
            Peer {
                address: listen.to_string(),
                cert: cert.clone(),
            },
        );
        configs.push(FederationConfig {
            node_id,
            pool_id,
            listen,
            ca: ca_path.clone(),
            cert,
            key: privkey,
            database: path.join(format!("{i}.db")),
            peers: BTreeMap::new(),
            bootstrap: Bootstrap::Manual(i == 0),
        });
    }
    for c in &mut configs {
        c.peers = peers.clone()
    }
    (configs, reservations)
}
