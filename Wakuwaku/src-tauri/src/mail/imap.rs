// IMAP through io-imap, the library under himalaya (pimalaya): we open the
// connection (TCP, the system's TLS, STARTTLS) and it speaks the protocol.
// Only what the pet does: sign in, open the inbox, count the unread, list
// letters by their headers, read one whole without marking it read, mark
// one read (or answered), IDLE, and keep a letter sent in the Sent folder.
// The connection is the SMTP client's too (smtp.rs).
use std::any::Any;
use std::io::{self, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use io_imap::client::{ImapClient, ImapClientError, ImapClientStd, ImapStream};
use io_imap::coroutine::{ImapCoroutine, ImapCoroutineState};
use io_imap::rfc2177::idle::{ImapIdle, ImapIdleOptions, ImapIdleYield};
use io_imap::rfc2971::id::ImapServerIdOptions;
use io_imap::rfc3501::append::ImapMessageAppendOptions;
use io_imap::rfc3501::fetch::ImapMessageFetchOptions;
use io_imap::rfc3501::login::{ImapLoginError, ImapLoginOptions};
use io_imap::rfc3501::search::ImapMessageSearchOptions;
use io_imap::rfc3501::store::ImapMessageStoreOptions;
use io_imap::sasl::auth_plain::{ImapAuthPlainError, ImapAuthPlainOptions};
use io_imap::types::core::{AString, IString, NString, Vec1};
use io_imap::types::fetch::{MessageDataItem, MessageDataItemName, Section};
use io_imap::types::flag::{Flag, FlagFetch, StoreType};
use io_imap::types::mailbox::{ListMailbox, Mailbox};
use io_imap::types::response::Capability;
use io_imap::types::search::SearchKey;
use io_imap::types::sequence::SequenceSet;
use serde_json::{json, Value};

use super::proxy::Proxy;
use super::{Auth, Security, Server};

// How long a read or a write may wait once connected; in IDLE, how often
// the wait wakes (to refresh it when it is due).
const IO_WAIT: Duration = Duration::from_secs(20);
// (and to see whether it is asked to stop waiting, idle's `kick`).
const IDLE_WAKE: Duration = Duration::from_secs(5);
// Through a proxy, how often the IDLE is renewed meanwhile: a proxy, or the
// server beyond it, may drop a connection quiet for longer (V2Ray's after
// five minutes, by default; shadowsocks-libev's after one).
const PROXIED_IDLE: Duration = Duration::from_secs(50);

#[derive(Debug, PartialEq)]
pub enum Fail {
    // Not reached (no such host, nothing on that port, no answer).
    Connect(String),
    // The encrypted connection did not come up.
    Tls(String),
    // Something answered, not as IMAP.
    NotImap(String),
    // Something answered, not as SMTP (smtp.rs).
    NotSmtp(String),
    // No STARTTLS where it was asked for.
    NoStartTls,
    // The server said no to the user and password.
    Login(String),
    // The outgoing server would not take a letter to someone: who, and its words.
    Recipient(String),
    // The server said no (NO, BAD) to something else.
    Refused(String),
    // The connection broke, or something else on the way.
    Other(String),
    // Through a proxy (proxy.rs): it was not reached; something there did
    // not answer as a proxy of the kind set; it said no to the server's
    // address; it did not reach the server.
    ProxyDown(Hop),
    NotProxy(Hop),
    ProxyRefused(Hop),
    ProxyServer(Hop),
}

// A hop through a proxy that failed: the proxy's address, why, and what the
// page may suggest ("socks": SOCKS5; "http": HTTP; or nothing).
#[derive(Debug, PartialEq)]
pub struct Hop {
    pub at: String,
    pub why: String,
    pub hint: &'static str,
}

impl Hop {
    fn json(&self, kind: &str) -> Value {
        json!({ "kind": kind, "text": self.why, "proxy": self.at, "hint": self.hint })
    }
}

impl Fail {
    pub fn json(&self) -> Value {
        let (kind, text) = match self {
            Fail::Connect(t) => ("connect", t.as_str()),
            Fail::Tls(t) => ("tls", t.as_str()),
            Fail::NotImap(t) => ("notImap", t.as_str()),
            Fail::NotSmtp(t) => ("notSmtp", t.as_str()),
            Fail::NoStartTls => ("noStartTls", ""),
            Fail::Login(t) => ("login", t.as_str()),
            Fail::Recipient(t) => ("recipient", t.as_str()),
            Fail::Refused(t) => ("refused", t.as_str()),
            Fail::Other(t) => ("other", t.as_str()),
            Fail::ProxyDown(h) => return h.json("proxyDown"),
            Fail::NotProxy(h) => return h.json("notProxy"),
            Fail::ProxyRefused(h) => return h.json("proxyRefused"),
            Fail::ProxyServer(h) => return h.json("proxyServer"),
        };
        match self {
            Fail::Login(t) => json!({ "kind": kind, "text": text, "why": login_why(t) }),
            _ => json!({ "kind": kind, "text": text }),
        }
    }
}

// Why a server said no to signing in, by its words (IMAP's, or SMTP's
// enhanced code), for the page to say what to do: an app password asked
// for in place of the usual one (Gmail: "Application-specific password
// required", 534 5.7.9); a sign-in in the browser first ("Web login
// required", "Please log in via your web browser", 5.7.14); too many
// connections at once; IMAP turned off for the account; or the password
// not taken (Gmail's "Invalid credentials", Dovecot's "Authentication
// failed.", Exchange's "AUTHENTICATE failed.", 535 5.7.8). io-imap keeps a
// NO's text without its code ([ALERT], [AUTHENTICATIONFAILED]), so the
// words alone. "" when they say none of these.
pub fn login_why(text: &str) -> &'static str {
    let t = text.to_ascii_lowercase();
    let has = |s: &str| t.contains(s);
    if has("application-specific password") || has("5.7.9") {
        "appPassword"
    } else if has("web login required") || has("via your web browser") || has("5.7.14") {
        "webLogin"
    } else if has("too many simultaneous connections") {
        "tooMany"
    } else if has("imap") && (has("disabled") || has("not enabled")) {
        "imapOff"
    } else if has("invalid credentials") || has("authentication failed") || has("authenticate failed") || has("authentication unsuccessful") || has("5.7.8") {
        "badPassword"
    } else {
        ""
    }
}

pub(super) fn io_fail(e: io::Error) -> Fail {
    Fail::Other(e.to_string())
}

// What io-imap said went wrong: the connection, or the server saying no.
fn fail(e: ImapClientError) -> Fail {
    match e {
        ImapClientError::Io(e) => io_fail(e),
        e => Fail::Refused(e.to_string()),
    }
}

// The connection: plain (until STARTTLS) or through the system's TLS; Gone
// for the moment it is being upgraded.
pub(super) enum Stream {
    Plain(TcpStream),
    Tls(Box<native_tls::TlsStream<TcpStream>>),
    Gone,
}

fn gone() -> io::Error {
    io::Error::new(io::ErrorKind::NotConnected, "the connection is gone")
}

impl Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Stream::Plain(s) => s.read(buf),
            Stream::Tls(s) => s.read(buf),
            Stream::Gone => Err(gone()),
        }
    }
}

impl Write for Stream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Stream::Plain(s) => s.write(buf),
            Stream::Tls(s) => s.write(buf),
            Stream::Gone => Err(gone()),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Stream::Plain(s) => s.flush(),
            Stream::Tls(s) => s.flush(),
            Stream::Gone => Err(gone()),
        }
    }
}

impl ImapStream for Stream {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        match self {
            Stream::Plain(s) => s.set_read_timeout(timeout),
            Stream::Tls(s) => s.get_ref().set_read_timeout(timeout),
            Stream::Gone => Ok(()),
        }
    }
}

pub(super) fn tls(host: &str, tcp: TcpStream) -> Result<Stream, Fail> {
    let connector = native_tls::TlsConnector::new().map_err(|e| Fail::Tls(e.to_string()))?;
    connector.connect(host, tcp).map(|s| Stream::Tls(Box::new(s))).map_err(|e| Fail::Tls(e.to_string()))
}

// A server reached, straight or through the proxy on the way to it
// (proxy.rs): the connection (encrypted at once for SSL/TLS), its socket (to
// be shut down from elsewhere), and the proxy it went through.
pub(super) fn connect(server: &Server, timeout: Duration) -> Result<(Stream, TcpStream, Option<Proxy>), Fail> {
    let proxy = super::proxy::route(&server.via, &server.host);
    let tcp = match &proxy {
        Some(p) => super::proxy::tunnel(p, &server.host, server.port, timeout)?,
        None => {
            let addr = (server.host.as_str(), server.port)
                .to_socket_addrs()
                .map_err(|e| Fail::Connect(e.to_string()))?
                .next()
                .ok_or_else(|| Fail::Connect("no address".into()))?;
            TcpStream::connect_timeout(&addr, timeout).map_err(|e| Fail::Connect(e.to_string()))?
        }
    };
    let _ = tcp.set_read_timeout(Some(timeout.max(IO_WAIT)));
    let _ = tcp.set_write_timeout(Some(timeout.max(IO_WAIT)));
    let socket = tcp.try_clone().map_err(io_fail)?;
    let stream = if server.security == Security::Ssl { tls(&server.host, tcp)? } else { Stream::Plain(tcp) };
    Ok((stream, socket, proxy))
}

fn caps_of(list: &[Capability]) -> Vec<String> {
    list.iter().map(|c| c.to_string().to_ascii_uppercase()).collect()
}

// Who is asking, for servers that want to know (163, 126 and yeah.net will
// not open a folder before).
fn our_id() -> Vec<(IString<'static>, NString<'static>)> {
    [("name", "Wakuwaku"), ("version", env!("CARGO_PKG_VERSION")), ("vendor", "Wakuwaku")]
        .into_iter()
        .filter_map(|(k, v)| Some((IString::try_from(k).ok()?, NString(Some(IString::try_from(v).ok()?)))))
        .collect()
}

// A letter as the list shows it: its headers (the fields asked for), and
// what the server knows of it.
#[derive(Debug, Default)]
pub struct Head {
    pub uid: u32,
    pub seen: bool,
    // Starred (\Flagged); replied to (\Answered).
    pub flagged: bool,
    pub answered: bool,
    pub size: u32,
    // When the server got it (seconds), for letters whose Date says nothing.
    pub received: Option<i64>,
    pub header: Vec<u8>,
}

pub struct Session {
    client: ImapClientStd,
    // The same socket: shut down from elsewhere to stop.
    pub socket: TcpStream,
    caps: Vec<String>,
    // The proxy it goes through, if any; and so how often an IDLE is renewed.
    pub proxy: Option<Proxy>,
    renew: Option<Duration>,
}

impl Session {
    // Connected, encrypted as asked, the greeting read, and what it can do.
    pub fn open(server: &Server, timeout: Duration) -> Result<Session, Fail> {
        let (stream, socket, proxy) = connect(server, timeout)?;
        let mut client = ImapClientStd::new(stream);
        let greeting = client.greeting().map_err(|e| Fail::NotImap(e.to_string()))?;
        let renew = proxy.as_ref().map(|_| PROXIED_IDLE);
        let mut s = Session { client, socket, caps: caps_of(&greeting.capability), proxy, renew };
        if server.security == Security::StartTls {
            if !s.has("STARTTLS") {
                return Err(Fail::NoStartTls);
            }
            // Bytes the server sent after its OK, before the handshake,
            // would be someone's in the middle: no.
            if !s.client.starttls().map_err(fail)?.is_empty() {
                return Err(Fail::Tls("data before the TLS handshake".into()));
            }
            let plain = s.client.stream.as_any_mut().downcast_mut::<Stream>().map(|st| std::mem::replace(st, Stream::Gone));
            let Some(Stream::Plain(tcp)) = plain else { return Err(Fail::Other("already encrypted".into())) };
            s.client.set_stream(tls(&server.host, tcp)?);
            s.caps = caps_of(&s.client.capability().map_err(fail)?);
        }
        Ok(s)
    }

    pub fn has(&self, cap: &str) -> bool {
        self.caps.iter().any(|c| c.eq_ignore_ascii_case(cap))
    }

    // An IDLE renewed this often (a test's, sooner than PROXIED_IDLE).
    #[cfg(test)]
    pub fn renew_every(&mut self, every: Duration) {
        self.renew = Some(every);
    }

    // What the server can do, as it said before signing in.
    pub fn caps(&self) -> &[String] {
        &self.caps
    }

    // Signed in as `auth` says: AUTHENTICATE PLAIN (any password goes), or
    // LOGIN, or (auto) PLAIN where it is offered, else LOGIN. The
    // credentials go after the server asks, never inline, as Coremail (163,
    // 126) says it takes them inline and does not. Then who we are, where
    // the server asks to be told.
    pub fn login(&mut self, user: &str, password: &str, auth: Auth) -> Result<(), Fail> {
        let plain = match auth {
            Auth::Plain => true,
            Auth::Login => false,
            Auth::Auto => self.has("AUTH=PLAIN"),
        };
        let caps = if plain {
            let opts = ImapAuthPlainOptions { initial_request: false, ensure_capabilities: true, auto_id: None };
            self.client.auth_plain(None, user, password, opts)
        } else if self.has("LOGINDISABLED") {
            return Err(Fail::Login("LOGINDISABLED".into()));
        } else {
            self.client.login(user, password, ImapLoginOptions { ensure_capabilities: true, auto_id: None })
        };
        let caps = caps.map_err(|e| match e {
            ImapClientError::AuthPlain(ImapAuthPlainError::No(why) | ImapAuthPlainError::Bad(why))
            | ImapClientError::Login(ImapLoginError::No(why) | ImapLoginError::Bad(why)) => Fail::Login(why),
            e => fail(e),
        })?;
        if !caps.is_empty() {
            self.caps = caps_of(&caps);
        }
        if self.has("ID") {
            let _ = self.client.id(ImapServerIdOptions { parameters: Some(our_id()) });
        }
        Ok(())
    }

    // The inbox opened, read-only or not; how many letters it holds.
    pub fn inbox(&mut self, write: bool) -> Result<u32, Fail> {
        Ok(self.folder(Mailbox::Inbox, write)?.exists)
    }

    // A folder opened, read-only (EXAMINE) or not (SELECT): how many letters
    // it holds, and its UIDVALIDITY (another one, and its UIDs are not the
    // ones kept: store.rs starts it again).
    pub fn folder(&mut self, mailbox: Mailbox<'static>, write: bool) -> Result<Opened, Fail> {
        let opened = if write { self.client.select(mailbox, Default::default()) } else { self.client.examine(mailbox, Default::default()) };
        let data = opened.map_err(fail)?;
        Ok(Opened { exists: data.exists.unwrap_or(0), validity: data.uid_validity.map_or(0, |v| v.get()) })
    }

    // Every letter of the folder open: its UID and flags (UID FETCH 1:*).
    pub fn flags_all(&mut self) -> Result<Vec<Flags>, Fail> {
        let set = SequenceSet::try_from("1:*").map_err(|e| Fail::Other(e.to_string()))?;
        let items = vec![MessageDataItemName::Uid, MessageDataItemName::Flags];
        let got = self.client.fetch(set, items.into(), ImapMessageFetchOptions { uid: true, ..Default::default() }).map_err(fail)?;
        Ok(got
            .into_values()
            .map(|items| {
                let mut f = Flags::default();
                for item in items {
                    match item {
                        MessageDataItem::Uid(n) => f.uid = n.get(),
                        MessageDataItem::Flags(flags) => {
                            let has = |want: Flag| flags.iter().any(|x| matches!(x, FlagFetch::Flag(f) if *f == want));
                            (f.seen, f.flagged, f.answered) = (has(Flag::Seen), has(Flag::Flagged), has(Flag::Answered));
                        }
                        _ => {}
                    }
                }
                f
            })
            .filter(|f| f.uid > 0)
            .collect())
    }

    // The UIDs above one (letters come in since).
    pub fn newer(&mut self, after: u32) -> Result<Vec<u32>, Fail> {
        let set = SequenceSet::try_from(format!("{}:*", after.saturating_add(1)).as_str()).map_err(|e| Fail::Other(e.to_string()))?;
        // "n:*" finds the last letter when there is none above n.
        Ok(self.search(SearchKey::Uid(set))?.into_iter().filter(|&uid| uid > after).collect())
    }

    // The unread letters' UIDs.
    pub fn unseen(&mut self) -> Result<Vec<u32>, Fail> {
        self.search(SearchKey::Unseen)
    }

    // The UIDs of the letters a search key finds (unread, starred…).
    pub fn search(&mut self, key: SearchKey<'static>) -> Result<Vec<u32>, Fail> {
        let found = self.client.search(Vec1::from(key), ImapMessageSearchOptions { uid: true }).map_err(fail)?;
        Ok(found.into_iter().map(|n| n.get()).collect())
    }

    // The letters in a set ("41:50" by position, or UIDs), by the header
    // fields named and what the server knows, none marked read.
    pub fn heads(&mut self, set: &str, uid: bool, fields: &[&str]) -> Result<Vec<Head>, Fail> {
        let set = SequenceSet::try_from(set).map_err(|e| Fail::Other(e.to_string()))?;
        let names: Vec<AString<'static>> = fields.iter().filter_map(|f| AString::try_from(f.to_string()).ok()).collect();
        let names: Vec1<AString<'static>> = Vec1::try_from(names).map_err(|_| Fail::Other("no header fields".into()))?;
        let items = vec![
            MessageDataItemName::Uid,
            MessageDataItemName::Flags,
            MessageDataItemName::InternalDate,
            MessageDataItemName::Rfc822Size,
            MessageDataItemName::BodyExt { section: Some(Section::HeaderFields(None, names)), partial: None, peek: true },
        ];
        let got = self.client.fetch(set, items.into(), ImapMessageFetchOptions { uid, ..Default::default() }).map_err(fail)?;
        Ok(got
            .into_values()
            .map(|items| {
                let mut head = Head::default();
                for item in items {
                    match item {
                        MessageDataItem::Uid(n) => head.uid = n.get(),
                        MessageDataItem::Flags(flags) => {
                            head.seen = flags.iter().any(|f| matches!(f, FlagFetch::Flag(Flag::Seen)));
                            head.flagged = flags.iter().any(|f| matches!(f, FlagFetch::Flag(Flag::Flagged)));
                            head.answered = flags.iter().any(|f| matches!(f, FlagFetch::Flag(Flag::Answered)));
                        }
                        MessageDataItem::Rfc822Size(n) => head.size = n,
                        MessageDataItem::InternalDate(d) => head.received = Some(d.as_ref().timestamp()),
                        MessageDataItem::BodyExt { data: NString(Some(s)), .. } => head.header = s.as_ref().to_vec(),
                        _ => {}
                    }
                }
                head
            })
            .filter(|h| h.uid > 0)
            .collect())
    }

    // A letter whole, as it came, and whether it is starred, without marking
    // it read; None when there is no such UID.
    pub fn letter(&mut self, uid: u32) -> Result<Option<(Vec<u8>, bool)>, Fail> {
        let set = SequenceSet::try_from(uid.to_string().as_str()).map_err(|e| Fail::Other(e.to_string()))?;
        let items = vec![MessageDataItemName::Flags, MessageDataItemName::BodyExt { section: None, partial: None, peek: true }];
        let got = self.client.fetch(set, items.into(), ImapMessageFetchOptions { uid: true, ..Default::default() }).map_err(fail)?;
        let (mut raw, mut flagged) = (None, false);
        for item in got.into_values().flatten() {
            match item {
                MessageDataItem::BodyExt { section: None, data: NString(Some(s)), .. } => raw = Some(s.as_ref().to_vec()),
                MessageDataItem::Flags(flags) => flagged = flags.iter().any(|f| matches!(f, FlagFetch::Flag(Flag::Flagged))),
                _ => {}
            }
        }
        Ok(raw.map(|r| (r, flagged)))
    }

    // A flag (read, starred) put on a letter or taken off (the inbox opened with write).
    pub fn set_flag(&mut self, uid: u32, flag: Flag<'static>, on: bool) -> Result<(), Fail> {
        let set = SequenceSet::try_from(uid.to_string().as_str()).map_err(|e| Fail::Other(e.to_string()))?;
        let how = if on { StoreType::Add } else { StoreType::Remove };
        self.client.store(set, how, vec![flag], ImapMessageStoreOptions { uid: true }).map_err(fail)?;
        Ok(())
    }

    // The folder sent letters are kept in: the one the server marks \Sent
    // (SPECIAL-USE), else one named as mail programs name it; None when
    // there is neither.
    pub fn mark_unseen(&mut self, mut applied: impl FnMut(&[u32])) -> Result<(), Fail> {
        let uids = self.unseen()?;
        for chunk in uids.chunks(200) {
            self.mark_seen(chunk)?;
            applied(chunk);
        }
        Ok(())
    }

    pub fn mark_seen(&mut self, uids: &[u32]) -> Result<(), Fail> {
        if uids.is_empty() { return Ok(()); }
        let value = uids.iter().map(u32::to_string).collect::<Vec<_>>().join(",");
        let set = SequenceSet::try_from(value.as_str()).map_err(|e| Fail::Other(e.to_string()))?;
        self.client.store(set, StoreType::Add, vec![Flag::Seen], ImapMessageStoreOptions { uid: true }).map_err(fail)?;
        Ok(())
    }

    pub fn sent_box(&mut self) -> Result<Option<Mailbox<'static>>, Fail> {
        let reference = Mailbox::try_from("").map_err(|e| Fail::Other(e.to_string()))?;
        let pattern = ListMailbox::try_from("*").map_err(|e| Fail::Other(e.to_string()))?;
        let all = self.client.list(reference, pattern).map_err(fail)?;
        let has = |attrs: &[io_imap::types::flag::FlagNameAttribute], name: &str| attrs.iter().any(|a| a.to_string().eq_ignore_ascii_case(name));
        if let Some((m, ..)) = all.iter().find(|(_, _, attrs)| has(attrs, "\\Sent")) {
            return Ok(Some(m.clone()));
        }
        let open: Vec<&Mailbox<'static>> = all.iter().filter(|(_, _, attrs)| !has(attrs, "\\Noselect")).map(|(m, ..)| m).collect();
        Ok(SENT_NAMES.iter().find_map(|name| open.iter().find(|m| leaf(m).eq_ignore_ascii_case(name))).map(|m| (*m).clone()))
    }

    // A letter put into a folder, marked read; its UID there, where the
    // server says (UIDPLUS's APPENDUID).
    pub fn append(&mut self, mailbox: Mailbox<'static>, raw: &[u8]) -> Result<Option<u32>, Fail> {
        let opts = ImapMessageAppendOptions { flags: vec![Flag::Seen], ..Default::default() };
        let (_, uid) = self.client.append(mailbox, raw, opts).map_err(fail)?;
        Ok(uid.map(|(_, uid)| uid))
    }

    pub fn noop(&mut self) -> Result<(), Fail> {
        self.client.noop().map_err(fail)
    }

    // Waits for the server to say something changed (true), or `kick` to be
    // set (false); the IDLE ended either way. Meanwhile io-imap renews it
    // every `every` (DONE, then IDLE again), through a proxy every
    // PROXIED_IDLE. A server (or proxy) that leaves one of these unanswered
    // for IO_WAIT is gone, and so is a socket shut down from elsewhere (at
    // once): a failure, to connect again.
    pub fn idle(&mut self, every: Duration, kick: &AtomicBool) -> Result<bool, Fail> {
        let every = self.renew.map_or(every, |r| r.min(every));
        let done = Arc::new(AtomicBool::new(false));
        let mut idle = ImapIdle::new(done.clone(), ImapIdleOptions { timeout: Some(every) });
        // On the stream read from: one set through the copy of the socket
        // does not hold on Windows (it waited IO_WAIT).
        let _ = self.client.stream.set_read_timeout(Some(IDLE_WAKE.min(every)));
        let mut buf = vec![0u8; 16 * 1024];
        let mut got: Option<usize> = None;
        let mut changed = false;
        // Since when something written waits for its answer.
        let mut asked: Option<Instant> = None;
        let result = loop {
            match idle.resume(&mut self.client.fragmentizer, got.take().map(|n| &buf[..n])) {
                ImapCoroutineState::Yielded(ImapIdleYield::Event(_)) => {
                    changed = true;
                    done.store(true, Ordering::SeqCst);
                }
                ImapCoroutineState::Yielded(ImapIdleYield::WantsWrite(bytes)) => {
                    if let Err(e) = self.client.stream.write_all(&bytes) {
                        break Err(io_fail(e));
                    }
                    asked.get_or_insert_with(Instant::now);
                }
                ImapCoroutineState::Yielded(ImapIdleYield::WantsRead) => match self.client.stream.read(&mut buf) {
                    Ok(0) => break Err(Fail::Other("the server closed the connection".into())),
                    Ok(n) => {
                        got = Some(n);
                        asked = None;
                    }
                    // Quiet a while: a chance to refresh the IDLE when it is
                    // due, or to stop when asked to.
                    Err(e) if matches!(e.kind(), io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock) => {
                        if asked.is_some_and(|at| at.elapsed() > IO_WAIT) {
                            break Err(Fail::Other("the server stopped answering".into()));
                        }
                        if kick.load(Ordering::SeqCst) {
                            done.store(true, Ordering::SeqCst);
                        }
                    }
                    Err(e) => break Err(io_fail(e)),
                },
                ImapCoroutineState::Complete(Ok(())) => break Ok(changed),
                ImapCoroutineState::Complete(Err(e)) => break Err(Fail::Other(e.to_string())),
            }
        };
        let _ = self.client.stream.set_read_timeout(Some(IO_WAIT));
        result
    }

    pub fn logout(mut self) {
        let _ = self.client.logout();
    }
}

// The names a Sent folder goes by where the server does not mark it: the
// English ones (Gmail's "[Gmail]/Sent Mail", Exchange's "Sent Items", QQ's
// "Sent Messages"), German (Exchange's), Chinese (163, 126).
const SENT_NAMES: [&str; 9] = ["Sent", "Sent Items", "Sent Messages", "Sent Mail", "Gesendete Elemente", "Gesendete Objekte", "Gesendet", "已发送", "已发送邮件"];

// A folder opened: how many letters, its UIDVALIDITY.
pub struct Opened {
    pub exists: u32,
    pub validity: u32,
}

// A letter's UID and flags.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Flags {
    pub uid: u32,
    pub seen: bool,
    pub flagged: bool,
    pub answered: bool,
}

// A folder by its whole name (as LIST gave it, kept in the store), and back.
pub fn mailbox(name: &str) -> Option<Mailbox<'static>> {
    if name.eq_ignore_ascii_case("INBOX") {
        return Some(Mailbox::Inbox);
    }
    Mailbox::try_from(name.to_string()).ok()
}

pub fn name_of(m: &Mailbox) -> String {
    match m {
        Mailbox::Inbox => "INBOX".into(),
        Mailbox::Other(o) => String::from_utf8_lossy(o.as_ref()).into_owned(),
    }
}

// A folder's own name, the part after its parents (INBOX.Sent is Sent).
fn leaf(m: &Mailbox) -> String {
    let name = match m {
        Mailbox::Inbox => return "INBOX".into(),
        Mailbox::Other(o) => String::from_utf8_lossy(o.as_ref()).into_owned(),
    };
    name.rsplit(['/', '.']).next().unwrap_or("").to_string()
}

// Connected and signed in.
pub fn session(server: &Server, password: &str, timeout: Duration) -> Result<Session, Fail> {
    let mut s = Session::open(server, timeout)?;
    s.login(&server.username, password, server.auth)?;
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn batch_probe(count: usize, reject_batch: usize) -> (usize, Result<(), Fail>, Vec<String>) {
        use std::io::{BufRead, BufReader};
        use std::net::TcpListener;
        use std::sync::Mutex;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let commands = Arc::new(Mutex::new(Vec::new()));
        let observed = commands.clone();
        let server = std::thread::spawn(move || {
            let (socket, _) = listener.accept().unwrap();
            socket.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
            let mut out = socket.try_clone().unwrap();
            out.write_all(b"* OK [CAPABILITY IMAP4rev1] test\r\n").unwrap();
            let mut batches = 0;
            for line in BufReader::new(socket).lines().map_while(Result::ok) {
                observed.lock().unwrap().push(line.clone());
                let tag = line.split_whitespace().next().unwrap();
                if line.contains("UID SEARCH") {
                    let uids = (1..=count).map(|n| n.to_string()).collect::<Vec<_>>().join(" ");
                    write!(out, "* SEARCH{}\r\n{tag} OK search\r\n", if uids.is_empty() { String::new() } else { format!(" {uids}") }).unwrap();
                } else if line.contains("UID STORE") {
                    batches += 1;
                    write!(out, "{tag} {} store\r\n", if batches == reject_batch { "NO" } else { "OK" }).unwrap();
                } else { panic!("unexpected command: {line}"); }
            }
        });
        let config = Server { host: "127.0.0.1".into(), port, security: Security::Plain, username: "test".into(), auth: Auth::Login, via: super::super::proxy::Via::Straight };
        let mut connection = Session::open(&config, Duration::from_secs(3)).unwrap();
        let mut changed = 0;
        let result = connection.mark_unseen(|batch| changed += batch.len());
        drop(connection);
        server.join().unwrap();
        let lines = commands.lock().unwrap().clone();
        (changed, result, lines)
    }

    #[test]
    fn unread_snapshot_is_batched_without_fetching_bodies_or_overwriting_other_flags() {
        let (changed, result, lines) = batch_probe(451, 0);
        assert!(result.is_ok());
        assert_eq!(changed, 451);
        assert_eq!(lines.iter().filter(|s| s.contains("UID SEARCH")).count(), 1);
        let stores: Vec<_> = lines.iter().filter(|s| s.contains("UID STORE")).collect();
        assert_eq!(stores.len(), 3);
        assert!(stores.iter().all(|s| s.contains("+FLAGS (\\Seen)") && !s.contains('*')));
        assert!(!lines.iter().any(|s| s.contains("FETCH")));
    }

    #[test]
    fn empty_unread_snapshot_sends_no_store() {
        let (changed, result, lines) = batch_probe(0, 0);
        assert!(result.is_ok());
        assert_eq!(changed, 0);
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn rejected_batch_keeps_only_acknowledged_changes_and_stops_later_batches() {
        let (changed, result, lines) = batch_probe(451, 2);
        assert!(result.is_err());
        assert_eq!(changed, 200);
        assert_eq!(lines.iter().filter(|s| s.contains("UID STORE")).count(), 2);
    }

    #[test]
    fn a_no_to_signing_in_says_why() {
        // Gmail's words, as io-imap keeps them (no [ALERT] or [AUTHENTICATIONFAILED]).
        assert_eq!(login_why("Application-specific password required: https://support.google.com/accounts/answer/185833 (Failure)"), "appPassword");
        assert_eq!(login_why("Invalid credentials (Failure)"), "badPassword");
        assert_eq!(login_why("Web login required: https://support.google.com/mail/answer/78754 (Failure)"), "webLogin");
        assert_eq!(login_why("Please log in via your web browser: https://support.google.com/mail/accounts/answer/78754 (Failure)"), "webLogin");
        assert_eq!(login_why("Too many simultaneous connections. (Failure)"), "tooMany");
        assert_eq!(login_why("IMAP access is disabled for your domain. (Failure)"), "imapOff");
        assert_eq!(login_why("Your account is not enabled for IMAP use."), "imapOff");
        // Gmail's SMTP, by its enhanced codes.
        assert_eq!(login_why("5.7.9 Application-specific password required. Learn more at"), "appPassword");
        assert_eq!(login_why("5.7.8 Username and Password not accepted. For more information, go to"), "badPassword");
        assert_eq!(login_why("5.7.14 <https://accounts.google.com/signin/continue?…> Please log in with your web browser"), "webLogin");
        // Others' "wrong password".
        assert_eq!(login_why("Authentication failed."), "badPassword");
        assert_eq!(login_why("AUTHENTICATE failed."), "badPassword");
        assert_eq!(login_why("5.7.3 Authentication unsuccessful [SN2PR00CA0018]"), "badPassword");
        // Nothing to go on: the server's words alone.
        assert_eq!(login_why("Login fail. Account is abnormal, service is not open, password is incorrect, login frequency limited, or system is busy."), "");
        assert_eq!(login_why("LOGINDISABLED"), "");
        assert_eq!(Fail::Login("Invalid credentials (Failure)".into()).json(), json!({ "kind": "login", "text": "Invalid credentials (Failure)", "why": "badPassword" }));
        assert_eq!(Fail::Connect("refused".into()).json(), json!({ "kind": "connect", "text": "refused" }));
    }
}
