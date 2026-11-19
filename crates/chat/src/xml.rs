use crate::invalid;
use quick_xml::{Reader, Writer, events::Event};
use roxmltree::Document;
use std::io;
use tokio::io::{AsyncRead, AsyncReadExt, BufReader, Take};

pub(crate) const CLIENT: &str = "jabber:client";
pub(crate) const STREAM: &str = "http://etherx.jabber.org/streams";
pub(crate) const SASL: &str = "urn:ietf:params:xml:ns:xmpp-sasl";
pub(crate) const STARTTLS: &str = "urn:ietf:params:xml:ns:xmpp-tls";
pub(crate) const BIND: &str = "urn:ietf:params:xml:ns:xmpp-bind";
pub(crate) const ROSTER: &str = "jabber:iq:roster";
const MAX_STANZA: u64 = 16 * 1024;

pub(crate) fn escape(s: &str) -> String {
    quick_xml::escape::escape(s).into_owned()
}
pub(crate) struct Input<R> {
    reader: Reader<Take<BufReader<R>>>,
    prefix: String,
    suffix: String,
}
impl<R: AsyncRead + Unpin> Input<R> {
    pub fn new(input: R) -> Self {
        Self::from_buffer(BufReader::new(input))
    }
    fn from_buffer(r: BufReader<R>) -> Self {
        Self {
            reader: Reader::from_reader(r.take(MAX_STANZA)),
            prefix: String::new(),
            suffix: String::new(),
        }
    }
    pub fn into_buffer(self) -> BufReader<R> {
        self.reader.into_inner().into_inner()
    }
    pub fn restart(self) -> Self {
        Self::from_buffer(self.into_buffer())
    }
    pub fn raw(&mut self) -> &mut R {
        self.reader.get_mut().get_mut().get_mut()
    }

    /// Restarted XML streams retain the bookending namespace declarations.
    /// Take limits every stanza before the parser allocates for an entire tag.
    pub async fn open(&mut self, domain: &str) -> io::Result<()> {
        self.reader.get_mut().set_limit(4096);
        let mut buf = vec![];
        loop {
            match self
                .reader
                .read_event_into_async(&mut buf)
                .await
                .map_err(|_| invalid())?
            {
                Event::Decl(_) => {}
                Event::Text(t) if t.as_ref().iter().all(u8::is_ascii_whitespace) => {}
                Event::Start(e) => {
                    self.suffix = format!(
                        "</{}>",
                        std::str::from_utf8(e.name().as_ref()).map_err(|_| invalid())?
                    );
                    let mut w = Writer::new(vec![]);
                    w.write_event(Event::Start(e))?;
                    self.prefix = String::from_utf8(w.into_inner()).map_err(|_| invalid())?;
                    let complete = format!("{}{}", self.prefix, self.suffix);
                    let d = Document::parse(&complete).map_err(|_| invalid())?;
                    let root = d.root_element();
                    if !root.has_tag_name((STREAM, "stream"))
                        || root.default_namespace() != Some(CLIENT)
                        || root.attribute("to") != Some(domain)
                        || root.attribute("version") != Some("1.0")
                    {
                        return Err(invalid());
                    }
                    return Ok(());
                }
                _ => return Err(invalid()),
            }
            buf.clear();
        }
    }

    pub async fn stanza(&mut self) -> io::Result<Option<String>> {
        self.reader.get_mut().set_limit(MAX_STANZA);
        let mut out = Writer::new(Vec::new());
        let mut buf = Vec::new();
        let mut depth = 0usize;
        loop {
            let evt = self
                .reader
                .read_event_into_async(&mut buf)
                .await
                .map_err(|_| invalid())?;
            match &evt {
                Event::End(_) if depth == 0 => return Ok(None),
                Event::Eof if depth == 0 && self.reader.get_ref().limit() > 0 => return Ok(None),
                Event::Text(v) if depth == 0 && v.as_ref().iter().all(u8::is_ascii_whitespace) => {
                    buf.clear();
                    continue;
                }
                Event::Start(_) => {
                    depth += 1;
                    if depth > 32 {
                        return Err(invalid());
                    }
                }
                Event::End(_) => depth -= 1,
                Event::Empty(_) => {}
                Event::Text(_) | Event::CData(_) | Event::GeneralRef(_) if depth > 0 => {}
                _ => return Err(invalid()),
            }
            out.write_event(evt)?;
            buf.clear();
            if depth == 0 {
                break;
            }
        }
        let raw = String::from_utf8(out.into_inner()).map_err(|_| invalid())?;
        Ok(Some(format!("{}{}{}", self.prefix, raw, self.suffix)))
    }
}
pub(crate) fn root<'a>(d: &'a Document<'a>) -> io::Result<roxmltree::Node<'a, 'a>> {
    d.root_element().first_element_child().ok_or_else(invalid)
}
