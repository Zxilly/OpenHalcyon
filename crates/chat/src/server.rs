use crate::{Contact, Directory, invalid, xml::*};
use base64::{Engine, engine::general_purpose::STANDARD};
use roxmltree::Document;
use std::{
    collections::{BTreeMap, HashSet},
    io,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncWrite, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::mpsc,
    task::JoinSet,
    time::{Instant, timeout},
};
use tokio_rustls::TlsAcceptor;
use uuid::Uuid;

struct Resource {
    user: Uuid,
    available: bool,
    peers: HashSet<Uuid>,
    tx: mpsc::Sender<(Uuid, String)>,
}
type Hub = Arc<Mutex<BTreeMap<String, Resource>>>;
#[derive(Clone)]
pub struct Server {
    domain: String,
    directory: Arc<dyn Directory>,
    tls: TlsAcceptor,
    hub: Hub,
}
struct Bound {
    jid: String,
    user: Uuid,
    session: String,
    hub: Hub,
}
impl Drop for Bound {
    fn drop(&mut self) {
        let mut hub = self.hub.lock().unwrap();
        if let Some(old) = hub.remove(&self.jid)
            && old.available
        {
            let stanza = format!(
                "<presence from='{}' type='unavailable'/>",
                escape(&self.jid)
            );
            for entry in hub
                .values()
                .filter(|e| old.peers.contains(&e.user) && e.available)
            {
                let _ = entry.tx.try_send((self.user, stanza.clone()));
            }
        }
    }
}
async fn send(w: &mut (impl AsyncWrite + Unpin), s: &str) -> io::Result<()> {
    timeout(Duration::from_secs(5), async {
        w.write_all(s.as_bytes()).await?;
        w.flush().await
    })
    .await?
}
fn parse(s: &str) -> io::Result<Document<'_>> {
    Document::parse(s).map_err(|_| invalid())
}
fn user(s: &str, domain: &str) -> Option<Uuid> {
    let bare = match s.split_once('@') {
        Some((u, d)) if d == domain => u,
        Some(_) => return None,
        None => s,
    };
    Uuid::parse_str(bare).ok()
}
impl Server {
    pub fn new(
        domain: String,
        directory: Arc<dyn Directory>,
        tls: Arc<rustls::ServerConfig>,
    ) -> io::Result<Self> {
        if domain.len() > 253
            || domain.split('.').any(|p| {
                p.is_empty()
                    || p.len() > 63
                    || p.starts_with('-')
                    || p.ends_with('-')
                    || !p.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            })
        {
            return Err(invalid());
        }
        Ok(Self {
            domain,
            directory,
            tls: tls.into(),
            hub: Default::default(),
        })
    }
    pub async fn serve(self, listener: TcpListener) -> io::Result<()> {
        let mut tasks = JoinSet::new();
        loop {
            tokio::select! {
                result=listener.accept()=>{let (sock,_)=result?;if tasks.len()<256{let server=self.clone();tasks.spawn(async move{let _=server.connection(sock).await;});}},
                _=tasks.join_next(),if !tasks.is_empty()=>{}
            }
        }
    }
    fn features(&self, body: &str) -> String {
        format!(
            "<stream:stream from='{}' id='{}' version='1.0' xmlns='jabber:client' xmlns:stream='{STREAM}'><stream:features>{body}</stream:features>",
            self.domain,
            Uuid::new_v4()
        )
    }
    async fn contacts(&self, session: &str) -> io::Result<Vec<Contact>> {
        timeout(Duration::from_secs(5), self.directory.contacts(session))
            .await?
            .filter(|v| v.len() <= 2048)
            .ok_or_else(invalid)
    }
    async fn connection(&self, sock: TcpStream) -> io::Result<()> {
        let (mut input,mut w,mut rx,bound)=timeout(Duration::from_secs(30),async{
            let mut plain=Input::new(sock);plain.open(&self.domain).await?;
            send(plain.raw(),&self.features(&format!("<starttls xmlns='{STARTTLS}'><required/></starttls>"))).await?;
            let frame=plain.stanza().await?.ok_or_else(invalid)?;
            if !root(&parse(&frame)?)?.has_tag_name((STARTTLS,"starttls")){return Err(invalid())}
            send(plain.raw(),&format!("<proceed xmlns='{STARTTLS}'/>")).await?;
            let tls=self.tls.accept(plain.into_buffer()).await?;
            let (r,mut w)=tokio::io::split(tls);let mut input=Input::new(r);
            input.open(&self.domain).await?;
            send(&mut w,&self.features(&format!("<mechanisms xmlns='{SASL}'><mechanism>PLAIN</mechanism></mechanisms>"))).await?;
            let frame=input.stanza().await?.ok_or_else(invalid)?;
            let identity=async{
                let d=parse(&frame).ok()?;let n=root(&d).ok()?;
                if !n.has_tag_name((SASL,"auth"))||n.attribute("mechanism")!=Some("PLAIN"){return None}
                let bytes=STANDARD.decode(n.text()?).ok()?;let mut fields=std::str::from_utf8(&bytes).ok()?.split('\0');
                let authz=fields.next()?;let id=user(fields.next()?,&self.domain)?;let pass=fields.next()?;
                if fields.next().is_some()||pass.is_empty()||(!authz.is_empty()&&user(authz,&self.domain)!=Some(id)){return None}
                Some((id,self.directory.authenticate(id,pass).await?))
            }.await;
            let Some((id,session))=identity else{send(&mut w,&format!("<failure xmlns='{SASL}'><not-authorized/></failure>")).await?;return Err(invalid())};
            send(&mut w,&format!("<success xmlns='{SASL}'/>")).await?;
            let mut input=input.restart();input.open(&self.domain).await?;
            send(&mut w,&self.features(&format!("<bind xmlns='{BIND}'/><session xmlns='urn:ietf:params:xml:ns:xmpp-session'><optional/></session>"))).await?;
            let frame=input.stanza().await?.ok_or_else(invalid)?;let d=parse(&frame)?;let n=root(&d)?;
            let bind=n.first_element_child().filter(|x|x.has_tag_name((BIND,"bind"))).ok_or_else(invalid)?;
            if !n.has_tag_name((CLIENT,"iq"))||n.attribute("type")!=Some("set"){return Err(invalid())}
            let tag=escape(n.attribute("id").ok_or_else(invalid)?);
            let resource=bind.children().find(|x|x.has_tag_name((BIND,"resource"))).and_then(|x|x.text()).map(str::to_owned).unwrap_or_else(||Uuid::new_v4().to_string());
            if resource.is_empty()||resource.len()>128||resource.chars().any(char::is_control){return Err(invalid())}
            let jid=format!("{id}@{}/{resource}",self.domain);let (tx,rx)=mpsc::channel(32);
            {let mut hub=self.hub.lock().unwrap();if hub.contains_key(&jid)||hub.values().filter(|e|e.user==id).count()>=8{return Err(invalid())}
            hub.insert(jid.clone(),Resource{user:id,available:false,peers:HashSet::new(),tx});}
            let bound=Bound{user:id,jid,session,hub:self.hub.clone()};
            send(&mut w,&format!("<iq type='result' id='{tag}'><bind xmlns='{BIND}'><jid>{}</jid></bind></iq>",escape(&bound.jid))).await?;
            Ok((input,w,rx,bound))
        }).await??;
        let mut start = Instant::now();
        let mut count = 0;
        loop {
            let frame = input.stanza();
            tokio::pin!(frame);
            let frame=timeout(Duration::from_secs(600),async{loop{tokio::select!{
                f=&mut frame=>break f,
                delivery=rx.recv()=>{let (from,body)=delivery.ok_or_else(invalid)?;if self.contacts(&bound.session).await?.iter().any(|c|c.id==from){send(&mut w,&body).await?;}}
            }}}).await??;
            let Some(frame) = frame else {
                send(&mut w, "</stream:stream>").await?;
                return Ok(());
            };
            if start.elapsed() >= Duration::from_secs(1) {
                start = Instant::now();
                count = 0;
            }
            count += 1;
            if count > 64 {
                return Err(invalid());
            }
            let contacts = self.contacts(&bound.session).await?;
            if let Some(reply) = self.handle(&bound, &frame, &contacts)? {
                send(&mut w, &reply).await?
            }
        }
    }
    fn handle(&self, b: &Bound, frame: &str, contacts: &[Contact]) -> io::Result<Option<String>> {
        let d = parse(frame)?;
        let n = root(&d)?;
        let from = escape(&b.jid);
        let id = escape(n.attribute("id").unwrap_or(""));
        if n.has_tag_name((CLIENT, "iq")) {
            let child = n.first_element_child();
            let kind = n.attribute("type");
            if kind == Some("get") && child.is_some_and(|c| c.has_tag_name((ROSTER, "query"))) {
                let mut roster = format!("<iq type='result' id='{id}'><query xmlns='{ROSTER}'>");
                for c in contacts {
                    roster.push_str(&format!(
                        "<item jid='{}@{}' name='{}' subscription='both'/>",
                        c.id,
                        self.domain,
                        escape(&c.name)
                    ));
                }
                roster.push_str("</query></iq>");
                return Ok(Some(roster));
            }
            if child.is_some_and(|c| {
                kind == Some("get") && c.has_tag_name(("urn:xmpp:ping", "ping"))
                    || kind == Some("set")
                        && c.has_tag_name(("urn:ietf:params:xml:ns:xmpp-session", "session"))
            }) {
                return Ok(Some(format!("<iq type='result' id='{id}'/>")));
            }
            if matches!(kind, Some("get" | "set")) {
                return Ok(Some(format!(
                    "<iq type='error' id='{id}'><error type='cancel'><feature-not-implemented xmlns='urn:ietf:params:xml:ns:xmpp-stanzas'/></error></iq>"
                )));
            }
        } else if n.has_tag_name((CLIENT, "presence"))
            && n.attribute("to").is_none()
            && matches!(n.attribute("type"), None | Some("unavailable"))
        {
            let available = n.attribute("type").is_none();
            let mut hub = self.hub.lock().unwrap();
            let own = hub.get_mut(&b.jid).ok_or_else(invalid)?;
            let initial = available && !own.available;
            own.available = available;
            own.peers = contacts.iter().map(|c| c.id).collect();
            let tx = own.tx.clone();
            let stanza = if available {
                format!("<presence from='{from}'/>")
            } else {
                format!("<presence from='{from}' type='unavailable'/>")
            };
            let peers: HashSet<_> = contacts.iter().map(|c| c.id).collect();
            for (jid, r) in hub
                .iter()
                .filter(|(_, r)| r.available && peers.contains(&r.user))
            {
                let _ = r.tx.try_send((b.user, stanza.clone()));
                if initial {
                    let _ = tx.try_send((r.user, format!("<presence from='{}'/>", escape(jid))));
                }
            }
        } else if n.has_tag_name((CLIENT, "message"))
            && matches!(n.attribute("type"), Some("chat") | None)
        {
            let to = n.attribute("to").ok_or_else(invalid)?;
            let bare = to.split('/').next().unwrap_or(to);
            let target = bare
                .split_once('@')
                .filter(|(_, d)| *d == self.domain)
                .and_then(|(u, _)| Uuid::parse_str(u).ok());
            let body = n
                .children()
                .find(|c| c.has_tag_name((CLIENT, "body")))
                .and_then(|c| c.text())
                .ok_or_else(invalid)?;
            if body.len() > 4096 {
                return Err(invalid());
            }
            if let Some(target) = target.filter(|id| contacts.iter().any(|c| c.id == *id)) {
                let hub = self.hub.lock().unwrap();
                for (jid, r) in hub.iter().filter(|(jid, r)| {
                    r.user == target
                        && if to.contains('/') {
                            jid.as_str() == to
                        } else {
                            r.available
                        }
                }) {
                    let stanza = format!(
                        "<message type='chat' id='{id}' from='{from}' to='{}'><body>{}</body></message>",
                        escape(jid),
                        escape(body)
                    );
                    if r.tx.try_send((b.user, stanza)).is_ok() {
                        return Ok(None);
                    }
                }
            }
            return Ok(Some(format!(
                "<message type='error' id='{id}'><error type='cancel'><service-unavailable xmlns='urn:ietf:params:xml:ns:xmpp-stanzas'/></error></message>"
            )));
        }
        Ok(None)
    }
}
