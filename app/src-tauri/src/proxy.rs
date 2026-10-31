// Proxies: Windows' own, as Settings (Network, Proxy) has it and as Clash or
// v2rayN set it when their "system proxy" is on, read for the web (the
// updates, the crash reports, LHM's download, the plugins: web_ways, a URL
// for ureq, tried before going straight) and for mail (mail/proxy.rs: an
// address and a kind to go through); and a proxy as one is typed in. The
// registry's ProxyServer is "host:port" for every protocol, or one for each:
// "http=host:port;https=host:port;socks=host:port"; ProxyOverride lists the
// hosts reached straight ("localhost;127.*;10.*;<local>", <local> being a
// name without a dot). A PAC script (AutoConfigURL, Clash's "PAC mode") is
// not read: as no proxy.
//
// Which way, the General page's Network proxy says (the setting netProxy,
// zhoupengjie/Wakuwaku#68): Windows' (the default), one typed in, or none;
// mail follows it unless its page or an account says otherwise.

use std::sync::Mutex;
use std::time::Duration;

use serde_json::{json, Value};
use ureq::config::ConfigBuilder;
use ureq::typestate::AgentScope;

// How a proxy is spoken to: HTTP's CONNECT, or SOCKS5.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Http,
    Socks5,
}

impl Kind {
    pub fn parse(s: &str) -> Option<Kind> {
        match s {
            "http" => Some(Kind::Http),
            "socks5" => Some(Kind::Socks5),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Kind::Http => "http",
            Kind::Socks5 => "socks5",
        }
    }
}

// A proxy: how it is spoken to, and where it is.
#[derive(Clone, Debug, PartialEq)]
pub struct Proxy {
    pub kind: Kind,
    pub host: String,
    pub port: u16,
}

impl Proxy {
    // As one writes it: 127.0.0.1:7890, [::1]:1080.
    pub fn at(&self) -> String {
        if self.host.contains(':') {
            format!("[{}]:{}", self.host, self.port)
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }

    // As typed: host:port (an IPv6 address in brackets), of the kind given
    // unless a scheme before it says (http://, socks5://, socks5h://,
    // socks://); a slash after is left off. None when it is not one.
    pub fn parse(text: &str, kind: Kind) -> Option<Proxy> {
        let text = text.trim();
        let (kind, rest) = match text.split_once("://") {
            Some((scheme, rest)) => {
                let kind = match scheme.to_ascii_lowercase().as_str() {
                    "http" => Kind::Http,
                    "socks" | "socks5" | "socks5h" => Kind::Socks5,
                    _ => return None,
                };
                (kind, rest)
            }
            None => (kind, text),
        };
        let rest = rest.trim_end_matches('/');
        let (host, port) = match rest.strip_prefix('[') {
            Some(v6) => v6.split_once("]:").filter(|(h, _)| h.contains(':'))?,
            None => rest.rsplit_once(':').filter(|(h, _)| !h.contains(':'))?,
        };
        let port = port.parse::<u16>().ok().filter(|p| *p > 0)?;
        let ok = !host.is_empty() && host.len() <= 253 && host.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ':'));
        ok.then(|| Proxy { kind, host: host.to_ascii_lowercase(), port })
    }
}

// A way to go, as set: { mode: system | manual | none, kind: http | socks5,
// address: "host:port" } (netProxy, mailProxy, an account's own).
#[derive(Clone, Debug, PartialEq, Default)]
pub enum Via {
    // Windows' proxy, read again at each connection: Clash's switched on or
    // off meanwhile is taken at the next.
    #[default]
    System,
    Manual(Proxy),
    Straight,
}

impl Via {
    // As kept in the settings; None when it is not one (a manual one with no
    // address that reads as one, a key of another name).
    pub fn of(v: &Value) -> Option<Via> {
        let m = v.as_object()?;
        if m.keys().any(|k| !matches!(k.as_str(), "mode" | "kind" | "address")) {
            return None;
        }
        match m.get("mode")?.as_str()? {
            "system" => Some(Via::System),
            "none" => Some(Via::Straight),
            "manual" => {
                let kind = Kind::parse(m.get("kind").and_then(Value::as_str).unwrap_or("http"))?;
                Proxy::parse(m.get("address")?.as_str()?, kind).map(Via::Manual)
            }
            _ => None,
        }
    }

    pub fn json(&self) -> Value {
        match self {
            Via::System => json!({ "mode": "system" }),
            Via::Straight => json!({ "mode": "none" }),
            Via::Manual(p) => json!({ "mode": "manual", "kind": p.kind.name(), "address": p.at() }),
        }
    }
}

// Windows' proxy as set: ProxyServer, ProxyOverride.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct System {
    pub server: String,
    pub bypass: String,
}

impl System {
    // One protocol's entry of ProxyServer ("https=…"), when it has them.
    fn entry(&self, scheme: &str) -> Option<&str> {
        self.server.split(';').find_map(|part| {
            let (name, value) = part.split_once('=')?;
            Some(value.trim()).filter(|v| name.trim().eq_ignore_ascii_case(scheme) && !v.is_empty())
        })
    }

    // For the web (the crash reports' relays), as a URL: the https entry,
    // else the http one, else the one for every protocol.
    pub fn for_web(&self) -> Option<String> {
        let pick = if self.server.contains('=') { self.entry("https").or_else(|| self.entry("http"))? } else { self.server.trim() };
        if pick.is_empty() {
            return None;
        }
        Some(if pick.contains("://") { pick.to_string() } else { format!("http://{pick}") })
    }

    // For mail, to any port: the socks entry where there is one (as Windows
    // means it, the proxy for other protocols), else the https or http one,
    // else the one for every protocol, which Windows speaks HTTP to (Clash
    // sets its mixed port, which takes either; v2rayN its HTTP port).
    pub fn for_mail(&self) -> Option<Proxy> {
        if !self.server.contains('=') {
            return Proxy::parse(&self.server, Kind::Http);
        }
        match self.entry("socks") {
            Some(socks) => Proxy::parse(socks, Kind::Socks5),
            None => Proxy::parse(self.entry("https").or_else(|| self.entry("http"))?, Kind::Http),
        }
    }

    // Whether a host is reached straight, by ProxyOverride: an entry as the
    // host is (* for any run of characters), or <local> for a name with no dot.
    pub fn bypasses(&self, host: &str) -> bool {
        self.bypass.split(';').map(str::trim).filter(|e| !e.is_empty()).any(|e| {
            if e.eq_ignore_ascii_case("<local>") {
                !host.contains('.') && !host.contains(':')
            } else {
                glob(e.as_bytes(), host.as_bytes())
            }
        })
    }
}

// A pattern with * against a name, letters in either case.
fn glob(pattern: &[u8], text: &[u8]) -> bool {
    match pattern.split_first() {
        None => text.is_empty(),
        Some((b'*', rest)) => (0..=text.len()).any(|i| glob(rest, &text[i..])),
        Some((c, rest)) => text.split_first().is_some_and(|(t, more)| t.eq_ignore_ascii_case(c) && glob(rest, more)),
    }
}

// Windows' proxy, when one is on (and is not only a PAC script).
#[cfg(windows)]
pub fn system() -> Option<System> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    let key = RegKey::predef(HKEY_CURRENT_USER).open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Internet Settings").ok()?;
    if key.get_value::<u32, _>("ProxyEnable").ok()? == 0 {
        return None;
    }
    let server: String = key.get_value("ProxyServer").ok()?;
    let bypass: String = key.get_value("ProxyOverride").unwrap_or_default();
    (!server.trim().is_empty()).then_some(System { server, bypass })
}

#[cfg(not(windows))]
pub fn system() -> Option<System> {
    None
}

// Windows' proxy as the web takes it, for the page: { kind, address }, or null.
pub fn web_system_view() -> Value {
    let p = system().and_then(|s| s.for_web()).and_then(|url| Proxy::parse(&url, Kind::Http));
    p.map_or(Value::Null, |p| json!({ "kind": p.kind.name(), "address": p.at() }))
}

// --- The web (updates, crash reports, LHM's download, the plugins) --------

// Every request of hers to the web goes one way, as netProxy says. Windows'
// (system): through Windows' proxy first where one is set (and none in the
// environment, which ureq takes by itself) and its list (ProxyOverride)
// does not name the host; then as ureq would; a host the list names
// straight. One typed in (manual): through it alone, our own way through
// (mail/proxy.rs tunnel: HTTP's CONNECT or SOCKS5, the name looked up by
// the proxy). None: straight. This computer itself always straight.
// PowerShell, which ran the plugins before, went Windows' way.

// netProxy as last set (set_web: as she starts, and when it is changed).
static WEB: Mutex<Via> = Mutex::new(Via::System);

pub fn set_web(v: &Value) {
    *WEB.lock().unwrap() = Via::of(v).unwrap_or_default();
}

pub fn web_setting() -> Via {
    WEB.lock().unwrap().clone()
}

// One way to try: through Windows' proxy (its URL), through the one typed
// in, as ureq would by itself (the environment's proxy if one is set there,
// else straight), or straight.
#[derive(Clone, Debug, PartialEq)]
pub enum Way {
    Through(String),
    Manual(Proxy),
    Default,
    Straight,
}

// The layers of every call's budget now live in crate::config (single entry,
// red line 5): the values and their environment overrides are there; this
// module keeps the public names so call sites stay put, and delegates.
// CONNECT and HEAD clamp to the whole call at use, in agent_with.

// A whole number of milliseconds from an override text (or none), or the
// default when it is empty, not a number, or out of range. Kept here (public
// to the crate) so tests and updates.rs inject values instead of mutating
// process globals (red line 9); the parsing itself is config::ms_of.
pub(crate) fn timeout_ms_of(text: Option<&str>, default_ms: u64) -> Duration {
    crate::config::ms_of(text, default_ms)
}

pub fn connect_timeout() -> Duration {
    crate::config::net_connect_timeout()
}

pub fn head_timeout() -> Duration {
    crate::config::net_head_timeout()
}

pub fn pet_timeout() -> Duration {
    crate::config::pet_timeout()
}

pub fn update_timeout() -> Duration {
    crate::config::update_timeout()
}

pub fn auth_timeout() -> Duration {
    crate::config::auth_timeout()
}

impl Way {
    // For the log: "through Windows' proxy http://127.0.0.1:7890",
    // "through the environment's proxy", "straight".
    pub fn name(&self) -> String {
        match self {
            Way::Through(p) => format!("through Windows' proxy {p}"),
            Way::Manual(p) => format!("through the proxy set, {} {}", p.kind.name(), p.at()),
            Way::Default if ureq::Proxy::try_from_env().is_some() => "through the environment's proxy".into(),
            Way::Default | Way::Straight => "straight".into(),
        }
    }

    // The proxy's address, for the page ("127.0.0.1:7890"); none straight.
    pub fn proxy_at(&self) -> Option<String> {
        match self {
            Way::Through(url) => Proxy::parse(url, Kind::Http).map(|p| p.at()),
            Way::Manual(p) => Some(p.at()),
            Way::Default | Way::Straight => None,
        }
    }

    // An agent for it, a whole call at most `timeout` (and CONNECT, HEAD as
    // configured), its config as `tweak` makes it; none for a proxy ureq
    // cannot take.
    pub fn agent_with(&self, timeout: Duration, tweak: impl FnOnce(ConfigBuilder<AgentScope>) -> ConfigBuilder<AgentScope>) -> Option<ureq::Agent> {
        let connect = connect_timeout().min(timeout);
        let head = head_timeout().min(timeout);
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            .timeout_connect(Some(connect))
            .timeout_recv_response(Some(head));
        Some(match self {
            Way::Through(p) => tweak(config.proxy(Some(ureq::Proxy::new(p).ok()?))).build().new_agent(),
            Way::Manual(p) => crate::mail::proxy::through(p, connect, tweak(config)),
            Way::Straight => tweak(config.proxy(None)).build().new_agent(),
            Way::Default => tweak(config).build().new_agent(),
        })
    }

    // An agent for it: redirects followed as ureq does (10), statuses from
    // 400 errors.
    pub fn agent(&self, timeout: Duration) -> Option<ureq::Agent> {
        self.agent_with(timeout, |c| c)
    }
}

// The host of an address, lower case; IPv6 without its brackets.
pub fn host_of(url: &str) -> Option<String> {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let authority = rest.split(['/', '?', '#']).next()?;
    let authority = authority.rsplit_once('@').map_or(authority, |(_, a)| a);
    let host = match authority.strip_prefix('[') {
        Some(v6) => v6.split_once(']').map(|(h, _)| h)?,
        None => authority.rsplit_once(':').map_or(authority, |(h, _)| h),
    };
    (!host.is_empty()).then(|| host.to_ascii_lowercase())
}

// This computer itself: never through a proxy.
pub fn is_loopback(host: &str) -> bool {
    host == "localhost" || host.ends_with(".localhost") || host == "::1" || host.starts_with("127.")
}

fn ways(host: &str, via: &Via, system: Option<System>, env_proxy: bool) -> Vec<Way> {
    if is_loopback(host) {
        return vec![Way::Straight];
    }
    match (via, system) {
        (Via::Manual(p), _) => vec![Way::Manual(p.clone())],
        (Via::Straight, _) => vec![Way::Straight],
        (Via::System, Some(s)) if s.bypasses(host) => vec![Way::Straight],
        (Via::System, Some(s)) if !env_proxy => s.for_web().map(Way::Through).into_iter().chain([Way::Default]).collect(),
        (Via::System, _) => vec![Way::Default],
    }
}

// The ways to try for an address, in turn, as netProxy and the system have
// them now.
pub fn web_ways(url: &str) -> Vec<Way> {
    web_ways_by(url, &web_setting())
}

// The same, for a way given (the page's test of one).
pub fn web_ways_by(url: &str, via: &Via) -> Vec<Way> {
    let system = if *via == Via::System { system() } else { None };
    ways(&host_of(url).unwrap_or_default(), via, system, ureq::Proxy::try_from_env().is_some())
}

// The page's Test: GitHub asked the way given (each of its ways in turn, as
// an update goes), any answer counting. { ok, via: the proxy it went
// through or null, ms } or { ok: false, via, error }; and, should it fail,
// a proxy listening on this computer at a port such apps use (Clash 7890,
// Clash Verge 7897, v2rayN 10809 HTTP, 10808 SOCKS), for the page to
// suggest: `listening`, { address, kind } or null.
//
// A proxy's port is probed before GitHub is asked through it (#21-B): a
// port nothing listens on says so at once — "代理 {addr} 没有开" / "the
// proxy at {addr} is not listening" — instead of a tunnel's words; the
// straight ways (Default, Straight) are probed by their own call.
pub fn test(via: &Via) -> Value {
    const URL: &str = "https://github.com/";
    let mut failed = json!({ "ok": false, "via": null, "error": "no way" });
    for way in web_ways_by(URL, via) {
        // The proxy's port first: nothing listening there, that is the
        // answer (the person's own words, not a tunnel's).
        if let (Some(proxy), Some(address)) = (way_proxy(&way), way.proxy_at()) {
            if !port_open(&proxy, crate::config::proxy_probe_timeout()) {
                failed = json!({ "ok": false, "via": address, "error": port_closed_text(&address), "portClosed": true });
                continue;
            }
        }
        let started = std::time::Instant::now();
        let Some(agent) = way.agent_with(crate::config::proxy_test_timeout(), |c| c.http_status_as_error(false).max_redirects(0)) else { continue };
        match agent.get(URL).header("user-agent", &format!("wakuwaku/{}", crate::updates::current())).call() {
            Ok(_) => return json!({ "ok": true, "via": way.proxy_at(), "ms": started.elapsed().as_millis() as u64 }),
            Err(e) => failed = json!({ "ok": false, "via": way.proxy_at(), "error": e.to_string() }),
        }
    }
    failed["listening"] = listening().map_or(Value::Null, |(port, kind)| json!({ "address": format!("127.0.0.1:{port}"), "kind": kind.name() }));
    failed
}

// The proxy a way goes through, when it goes through one.
fn way_proxy(way: &Way) -> Option<Proxy> {
    match way {
        Way::Through(url) => Proxy::parse(url, Kind::Http),
        Way::Manual(p) => Some(p.clone()),
        Way::Default | Way::Straight => None,
    }
}

// Whether a proxy's port answers a TCP connect: it being closed is the
// first thing to know of a manual or Windows proxy (the tunnel's error for
// it says nothing of the port). A name is looked up (or the local host,
// where the name does not resolve here: this computer's own proxies are
// what the page mostly sets).
pub(crate) fn port_open(proxy: &Proxy, wait: Duration) -> bool {
    use std::net::ToSocketAddrs;
    let addrs = (proxy.host.as_str(), proxy.port).to_socket_addrs().map(|it| it.collect::<Vec<_>>()).unwrap_or_default();
    let addrs = if addrs.is_empty() { vec![std::net::SocketAddr::from(([127, 0, 0, 1], proxy.port))] } else { addrs };
    addrs.iter().any(|addr| std::net::TcpStream::connect_timeout(addr, wait).is_ok())
}

// What the page says of a proxy's port nothing answers on.
fn port_closed_text(address: &str) -> String {
    match crate::i18n::lang("") {
        "zh" => format!("代理 {address} 没有开：这个端口上没有程序在听，先启动代理软件，或换一个地址"),
        _ => format!("the proxy at {address} is not listening: nothing answers on that port — start the proxy app, or try another address"),
    }
}

fn listening() -> Option<(u16, Kind)> {
    [(7890, Kind::Http), (7897, Kind::Http), (10809, Kind::Http), (10808, Kind::Socks5)].into_iter().find(|(port, _)| {
        std::net::TcpStream::connect_timeout(&std::net::SocketAddr::from(([127, 0, 0, 1], *port)), crate::config::proxy_connect_timeout()).is_ok()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sys(server: &str) -> System {
        System { server: server.into(), bypass: String::new() }
    }

    #[test]
    fn what_clash_and_v2rayn_write_is_read() {
        // Clash (Verge, for Windows): its mixed port, for everything.
        let clash = sys("127.0.0.1:7890");
        assert_eq!(clash.for_mail(), Some(Proxy { kind: Kind::Http, host: "127.0.0.1".into(), port: 7890 }));
        assert_eq!(clash.for_web().as_deref(), Some("http://127.0.0.1:7890"));
        // v2rayN: its HTTP port.
        assert_eq!(sys("127.0.0.1:10809").for_mail().map(|p| p.at()).as_deref(), Some("127.0.0.1:10809"));
        // One for each protocol: mail through the socks one, the web through the https one.
        let each = sys("http=127.0.0.1:10809;https=127.0.0.1:10809;socks=127.0.0.1:10808");
        assert_eq!(each.for_mail(), Some(Proxy { kind: Kind::Socks5, host: "127.0.0.1".into(), port: 10808 }));
        assert_eq!(each.for_web().as_deref(), Some("http://127.0.0.1:10809"));
        assert_eq!(sys("http=127.0.0.1:8080;https=127.0.0.1:8443").for_mail().map(|p| (p.kind, p.port)), Some((Kind::Http, 8443)));
        assert_eq!(sys("ftp=1.2.3.4:21").for_mail(), None);
        assert_eq!(sys("SOCKS=localhost:1080").for_mail().map(|p| (p.kind, p.host)), Some((Kind::Socks5, "localhost".into())));
        assert_eq!(sys(" ").for_mail(), None);
        assert_eq!(sys("socks5://127.0.0.1:7891").for_mail().map(|p| p.kind), Some(Kind::Socks5));
    }

    #[test]
    fn some_hosts_are_reached_straight() {
        // Clash's list, shortened.
        let s = System { server: "127.0.0.1:7890".into(), bypass: "localhost;127.*;192.168.*;10.*;172.16.*;*.local;<local>".into() };
        assert!(s.bypasses("127.0.0.1"));
        assert!(s.bypasses("LOCALHOST"));
        assert!(s.bypasses("192.168.1.20"));
        assert!(s.bypasses("mail.corp.local"));
        assert!(s.bypasses("intranet"));
        assert!(!s.bypasses("imap.gmail.com"));
        assert!(!s.bypasses("10mail.example.com"));
        assert!(!sys("127.0.0.1:7890").bypasses("127.0.0.1"));
    }

    #[test]
    fn a_proxy_is_typed_as_one_writes_it() {
        let p = |t: &str| Proxy::parse(t, Kind::Http);
        assert_eq!(p(" 127.0.0.1:7890 "), Some(Proxy { kind: Kind::Http, host: "127.0.0.1".into(), port: 7890 }));
        assert_eq!(p("socks5://127.0.0.1:10808/").map(|x| (x.kind, x.port)), Some((Kind::Socks5, 10808)));
        assert_eq!(p("http://Proxy.Example.com:3128").map(|x| x.host), Some("proxy.example.com".into()));
        assert_eq!(p("[::1]:1080").map(|x| x.at()), Some("[::1]:1080".into()));
        for bad in ["127.0.0.1", "127.0.0.1:0", "127.0.0.1:99999", ":7890", "a b:1", "::1:1080", "https://x:1", "x:1 --y"] {
            assert_eq!(p(bad), None, "{bad}");
        }
    }

    #[test]
    fn hosts_are_read_from_addresses() {
        assert_eq!(host_of("http://localhost:3000").as_deref(), Some("localhost"));
        assert_eq!(host_of("https://API.github.com/repos/a/b").as_deref(), Some("api.github.com"));
        assert_eq!(host_of("http://[::1]:5173/api").as_deref(), Some("::1"));
        assert_eq!(host_of("http://user:pw@127.0.0.1:8080/x?y").as_deref(), Some("127.0.0.1"));
        assert_eq!(host_of("https://Release-Assets.githubusercontent.com:443/a?b").as_deref(), Some("release-assets.githubusercontent.com"));
        assert_eq!(host_of("http://").as_deref(), None);
        assert!(is_loopback("127.0.0.1") && is_loopback("localhost") && is_loopback("::1") && is_loopback("app.localhost"));
        assert!(!is_loopback("example.com") && !is_loopback("10.0.0.1"));
    }

    // This computer straight; Windows' proxy then as ureq would; straight
    // for a host its list names; the environment's alone when one is set
    // there.
    #[test]
    fn the_web_through_windows_proxy_first() {
        let clash = || Some(System { server: "127.0.0.1:7890".into(), bypass: "localhost;127.*;192.168.*;<local>".into() });
        let sys_way = &Via::System;
        assert_eq!(ways("localhost", sys_way, clash(), false), [Way::Straight]);
        assert_eq!(ways("github.com", sys_way, clash(), false), [Way::Through("http://127.0.0.1:7890".into()), Way::Default]);
        assert_eq!(ways("192.168.1.5", sys_way, clash(), false), [Way::Straight]);
        assert_eq!(ways("devbox", sys_way, clash(), false), [Way::Straight]);
        assert_eq!(ways("api.github.com", sys_way, None, false), [Way::Default]);
        assert_eq!(ways("api.github.com", sys_way, clash(), true), [Way::Default]);
        assert_eq!(ways("::1", sys_way, None, true), [Way::Straight]);
        assert_eq!(ways("127.0.0.1", sys_way, Some(sys("127.0.0.1:7890")), false), [Way::Straight]);
    }

    // One typed in (netProxy manual): through it alone, whatever Windows or
    // the environment say; none: straight, the environment's too. This
    // computer straight still.
    #[test]
    fn the_web_as_the_general_page_says() {
        let socks = Proxy { kind: Kind::Socks5, host: "127.0.0.1".into(), port: 10808 };
        let manual = Via::Manual(socks.clone());
        assert_eq!(ways("github.com", &manual, Some(sys("127.0.0.1:7890")), true), [Way::Manual(socks.clone())]);
        assert_eq!(ways("localhost", &manual, None, false), [Way::Straight]);
        assert_eq!(ways("github.com", &Via::Straight, Some(sys("127.0.0.1:7890")), true), [Way::Straight]);
        assert_eq!(Way::Manual(socks).proxy_at().as_deref(), Some("127.0.0.1:10808"));
        assert_eq!(Way::Through("http://127.0.0.1:7890".into()).proxy_at().as_deref(), Some("127.0.0.1:7890"));
        assert_eq!(Via::of(&json!({ "mode": "manual", "kind": "socks5", "address": "127.0.0.1:10808" })).map(|v| v.json()), Some(json!({ "mode": "manual", "kind": "socks5", "address": "127.0.0.1:10808" })));
        assert_eq!(Via::of(&json!({ "mode": "follow" })), None);
    }

    // #21-B：netProxy 的端口先探测——开着与没开，两种结论（测试自己起一个
    // 监听端口当「开着」，另挑一个没程序听的端口当「没开」）。
    #[test]
    fn a_proxys_port_is_probed_before_github_is_asked() {
        use std::net::TcpListener;
        // 开着：本测试进程听上它。
        let server = TcpListener::bind(("127.0.0.1", 0)).expect("listen");
        let port = server.local_addr().expect("addr").port();
        let open = Proxy { kind: Kind::Http, host: "127.0.0.1".into(), port };
        assert!(port_open(&open, Duration::from_millis(600)), "监听中的端口应判为开");
        // 没开：找一个人工端口，先确认没有程序在听。
        let closed = 'found: {
            for probe in 40000u16..40100 {
                let p = Proxy { kind: Kind::Http, host: "127.0.0.1".into(), port: probe };
                if TcpListener::bind(("127.0.0.1", probe)).is_err() {
                    // 已被占（可能是 TIME_WAIT 存根）：跳过。
                    continue;
                }
                break 'found p;
            }
            panic!("找不到一个空闲端口");
        };
        assert!(!port_open(&closed, Duration::from_millis(300)), "无人监听的端口应判为关");
        // 代理两种说法都在：直连一句话，代理失败一句话。
        let zh = port_closed_text("127.0.0.1:7890");
        assert!(zh.contains("7890") && zh.contains("没有开"), "{zh}");
        let at = Way::Manual(Proxy { kind: Kind::Socks5, host: "127.0.0.1".into(), port: 10808 });
        assert_eq!(way_proxy(&at).map(|p| p.at()).as_deref(), Some("127.0.0.1:10808"));
        assert_eq!(way_proxy(&Way::Straight), None);
        assert_eq!(way_proxy(&Way::Through("http://127.0.0.1:7890".into())).map(|p| p.port), Some(7890));
    }
}
