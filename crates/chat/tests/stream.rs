use async_trait::async_trait;
use base64::{Engine, engine::general_purpose::STANDARD};
use halcyon_chat::{Contact, Directory, Server};
use rustls::{ClientConfig, RootCertStore, ServerConfig, pki_types::PrivatePkcs8KeyDer};
use std::{sync::Arc, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    time::timeout,
};
use tokio_rustls::{TlsConnector, client::TlsStream};
use uuid::Uuid;

const OPEN: &str = "<stream:stream xmlns='jabber:client' xmlns:stream='http://etherx.jabber.org/streams' to='localhost' version='1.0'>";
use std::sync::atomic::{AtomicBool, Ordering};
#[derive(Default)]
struct People {
    hidden: AtomicBool,
    muted: AtomicBool,
}
#[async_trait]
impl Directory for People {
    async fn authenticate(&self, user: Uuid, pass: &str) -> Option<String> {
        (pass == "test-grant").then(|| user.to_string())
    }
    async fn presence_allowed(&self, _session: &str) -> bool {
        !self.hidden.load(Ordering::Relaxed)
    }
    async fn receives_presence(&self, _session: &str) -> bool {
        !self.muted.load(Ordering::Relaxed)
    }
    async fn contacts(&self, session: &str) -> Option<Vec<Contact>> {
        let other = match Uuid::parse_str(session).ok()?.as_u128() {
            1 => 2,
            2 => 1,
            _ => return Some(vec![]),
        };
        Some(vec![Contact {
            id: Uuid::from_u128(other),
            name: "A & B".into(),
        }])
    }
}
async fn until(r: &mut (impl AsyncRead + Unpin), end: &str) -> String {
    timeout(Duration::from_secs(4), async {
        let mut bytes = vec![];
        while !bytes.ends_with(end.as_bytes()) {
            bytes.push(r.read_u8().await.unwrap());
            assert!(bytes.len() < 8192);
        }
        String::from_utf8(bytes).unwrap()
    })
    .await
    .unwrap()
}
async fn login(
    addr: std::net::SocketAddr,
    tls: &TlsConnector,
    n: u128,
    pass: &str,
) -> Option<TlsStream<TcpStream>> {
    let mut s = TcpStream::connect(addr).await.unwrap();
    s.write_all(OPEN.as_bytes()).await.unwrap();
    assert!(
        until(&mut s, "</stream:features>")
            .await
            .contains("starttls")
    );
    s.write_all(b"<starttls xmlns='urn:ietf:params:xml:ns:xmpp-tls'/>")
        .await
        .unwrap();
    until(&mut s, "/>").await;
    let mut s = tls
        .connect("localhost".try_into().unwrap(), s)
        .await
        .unwrap();
    s.write_all(OPEN.as_bytes()).await.unwrap();
    assert!(until(&mut s, "</stream:features>").await.contains("PLAIN"));
    let id = Uuid::from_u128(n);
    let token = STANDARD.encode(format!("\0{id}\0{pass}"));
    s.write_all(
        format!("<auth xmlns='urn:ietf:params:xml:ns:xmpp-sasl' mechanism='PLAIN'>{token}</auth>")
            .as_bytes(),
    )
    .await
    .unwrap();
    let answer = until(&mut s, "/>").await;
    if !answer.contains("<success") {
        assert!(answer.contains("not-authorized"));
        return None;
    }
    s.write_all(OPEN.as_bytes()).await.unwrap();
    until(&mut s, "</stream:features>").await;
    s.write_all(b"<iq type='set' id='b'><bind xmlns='urn:ietf:params:xml:ns:xmpp-bind'><resource>game</resource></bind></iq>").await.unwrap();
    assert!(
        until(&mut s, "</iq>")
            .await
            .contains(&format!("{id}@localhost/game"))
    );
    Some(s)
}
#[tokio::test]
async fn two_encrypted_streams_route_authorized_stanzas() {
    let key = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let cert = key.cert.der().clone();
    let mut roots = RootCertStore::empty();
    roots.add(cert.clone()).unwrap();
    let tls = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            vec![cert],
            PrivatePkcs8KeyDer::from(key.key_pair.serialize_der()).into(),
        )
        .unwrap();
    let client = TlsConnector::from(Arc::new(
        ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth(),
    ));
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    let directory = Arc::new(People::default());
    let task = tokio::spawn(
        Server::new("localhost".into(), directory.clone(), Arc::new(tls))
            .unwrap()
            .serve(l),
    );
    assert!(login(addr, &client, 1, "wrong").await.is_none());
    let mut a = login(addr, &client, 1, "test-grant").await.unwrap();
    let mut b = login(addr, &client, 2, "test-grant").await.unwrap();
    a.write_all(b"<presence/><iq type='get' id='r'><query xmlns='jabber:iq:roster'/></iq>")
        .await
        .unwrap();
    assert!(until(&mut a, "</iq>").await.contains("A &amp; B"));
    b.write_all(b"<presence/>").await.unwrap();
    assert!(
        until(&mut a, "/>")
            .await
            .contains(&Uuid::from_u128(2).to_string())
    );
    until(&mut b, "/>").await;
    let to = Uuid::from_u128(2);
    a.write_all(format!("<message from='impostor' to='{to}@localhost' type='chat'><body>hi &amp; bye</body></message>").as_bytes()).await.unwrap();
    let msg = until(&mut b, "</message>").await;
    assert!(msg.contains("hi &amp; bye"));
    assert!(msg.contains(&format!("from='{}@localhost/game'", Uuid::from_u128(1))));
    let third = Uuid::from_u128(3);
    a.write_all(format!("<message to='{third}@localhost'><body>x</body></message>").as_bytes())
        .await
        .unwrap();
    assert!(
        until(&mut a, "</message>")
            .await
            .contains("service-unavailable")
    );
    for flag in [&directory.hidden, &directory.muted] {
        flag.store(true, Ordering::Relaxed);
        b.write_all(
            format!(
                "<presence/><message to='{}@localhost'><body>still delivered</body></message>",
                Uuid::from_u128(1)
            )
            .as_bytes(),
        )
        .await
        .unwrap();
        let received = until(&mut a, "</message>").await;
        assert!(received.contains("still delivered") && !received.contains("<presence"));
        flag.store(false, Ordering::Relaxed);
    }
    b.write_all(b"</stream:stream>").await.unwrap();
    assert!(until(&mut a, "/>").await.contains("unavailable"));
    task.abort();
}
