// The web for the plugins she runs (weather, stock, ci, devserver): one GET,
// with a short time to answer in, the way her other requests go
// (proxy::web_ways: through Windows' proxy first where one is set, then
// straight; this computer, and a host Windows reaches straight, straight).
use std::time::Duration;

use serde_json::Value;

use crate::proxy::{web_ways, Way};

// How long a request may take, all told: the scripts gave 20 seconds; she
// gives less, on a thread of the plugin's own, so nothing of hers waits.
pub const TIMEOUT: Duration = Duration::from_secs(15);

// What came back: its status (any, an error page too) and its words.
pub struct Answer {
    pub status: u16,
    pub body: String,
}

impl Answer {
    pub fn json(&self) -> Option<Value> {
        serde_json::from_str(&self.body).ok()
    }
}

// Why nothing came back, in a few words.
#[derive(Debug, PartialEq)]
pub enum Fail {
    // An address that is none (said as given).
    Address(String),
    // No answer: not reached, refused, too slow (the error's own words).
    Network(String),
}

// Redirects followed as many as PowerShell did (5), or none (devserver).
fn agent(way: &Way, timeout: Duration, redirects: u32) -> Option<ureq::Agent> {
    // An error page is an answer too: its status and words are read here.
    way.agent_with(timeout, |c| c.http_status_as_error(false).max_redirects(redirects).max_redirects_will_error(false))
}

// GET an address with these headers: its answer, whatever its status; or
// why none came, the last way's (each way tried in turn).
pub fn get(url: &str, headers: &[(&str, &str)], timeout: Duration) -> Result<Answer, Fail> {
    let mut last = Fail::Network("no way".into());
    for way in web_ways(url) {
        let Some(agent) = agent(&way, timeout, 5) else { continue };
        let mut req = agent.get(url);
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        match req.call() {
            Ok(mut res) => {
                let status = res.status().as_u16();
                // A few MB at most: a day's chart is 30 KB, a page of pull requests 300.
                let body = res.body_mut().with_config().limit(8 * 1024 * 1024).read_to_string().map_err(|e| Fail::Network(words(&e)))?;
                return Ok(Answer { status, body });
            }
            Err(ureq::Error::BadUri(_) | ureq::Error::Http(_)) => return Err(Fail::Address(url.to_string())),
            Err(e) => last = Fail::Network(words(&e)),
        }
    }
    Err(last)
}

// Whether anything answers there, an error page or a redirect too (the dev
// servers: "any answer counts as up"), within `timeout`; never through a
// proxy for this computer.
pub fn answers(url: &str, timeout: Duration) -> bool {
    for way in web_ways(url) {
        let Some(agent) = agent(&way, timeout, 0) else { continue };
        if agent.get(url).call().is_ok() {
            return true;
        }
    }
    false
}

// A ureq error in a few words: its source's (the system's own, in its
// language) where it has one.
fn words(e: &ureq::Error) -> String {
    match e {
        ureq::Error::Io(io) => io.to_string(),
        ureq::Error::Timeout(_) => "timed out".into(),
        ureq::Error::HostNotFound => "host not found".into(),
        other => other.to_string(),
    }
}

// What a failed request says, in her language: "连不上：…" / "Could not
// connect: …" (as fetch.rs words it), or the server's status and its own
// words for an error page (`message`: GitHub's; Yahoo's description;
// Open-Meteo's reason), cut to a line.
pub fn trouble(lang: &str, fail: &Fail) -> String {
    let zh = lang == "zh";
    match fail {
        Fail::Address(url) => {
            if zh {
                format!("看不懂这个地址：{url}")
            } else {
                format!("Not an address: {url}")
            }
        }
        Fail::Network(why) => {
            if zh {
                format!("连不上：{why}")
            } else {
                format!("Could not connect: {why}")
            }
        }
    }
}

// An error page's status and what it says of itself.
pub fn refused(answer: &Answer) -> String {
    let said = answer.json().and_then(|v| {
        let found = [&v["message"], &v["chart"]["error"]["description"], &v["reason"], &v["error"]["message"], &v["error"]].into_iter().find_map(|m| m.as_str().map(str::to_string));
        found.filter(|s| !s.trim().is_empty())
    });
    match said {
        Some(said) => format!("{}: {}", answer.status, said.trim()),
        None => format!("HTTP {}", answer.status),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_error_page_says_what_it_is() {
        let page = |status, body: &str| Answer { status, body: body.into() };
        assert_eq!(refused(&page(404, include_str!("../../../test/fixtures/plugins/chart-none.json"))), "404: No data found, symbol may be delisted");
        assert_eq!(refused(&page(404, r#"{"message":"Not Found","documentation_url":"https://docs.github.com/rest","status":"404"}"#)), "404: Not Found");
        assert_eq!(refused(&page(400, r#"{"error":true,"reason":"Cannot initialize WeatherVariable from invalid String value x"}"#)), "400: Cannot initialize WeatherVariable from invalid String value x");
        assert_eq!(refused(&page(502, "<html>bad gateway</html>")), "HTTP 502");
        assert_eq!(trouble("zh", &Fail::Network("timed out".into())), "连不上：timed out");
        assert_eq!(trouble("en", &Fail::Address("a b".into())), "Not an address: a b");
    }

    // A server on this computer: any answer is one (an error page, a
    // redirect not followed); nobody there, none.
    #[test]
    fn any_answer_from_a_server_counts() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            for (i, conn) in listener.incoming().take(3).enumerate() {
                let mut conn = conn.unwrap();
                let mut buf = [0u8; 1024];
                let _ = conn.read(&mut buf);
                let reply = match i {
                    0 => "HTTP/1.1 500 Internal Server Error\r\ncontent-length: 2\r\nconnection: close\r\n\r\nno",
                    1 => "HTTP/1.1 302 Found\r\nlocation: http://127.0.0.1:1/elsewhere\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
                    _ => "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 9\r\nconnection: close\r\n\r\n{\"a\": 1}\n",
                };
                let _ = conn.write_all(reply.as_bytes());
            }
        });
        let url = format!("http://127.0.0.1:{port}/api/health");
        assert!(answers(&url, Duration::from_secs(3)));
        assert!(answers(&url, Duration::from_secs(3)));
        let got = get(&url, &[("user-agent", "wakuwaku")], Duration::from_secs(3)).ok().unwrap();
        assert_eq!((got.status, got.json()), (200, Some(serde_json::json!({ "a": 1 }))));
        server.join().unwrap();
        // The port let go: nobody answers there.
        assert!(!answers(&url, Duration::from_secs(3)));
        assert!(matches!(get(&url, &[], Duration::from_secs(3)), Err(Fail::Network(_))));
        assert_eq!(get("http://exa mple.com/", &[], Duration::from_secs(1)).err(), Some(Fail::Address("http://exa mple.com/".into())));
    }
}
