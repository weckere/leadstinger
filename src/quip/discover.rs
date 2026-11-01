//! Finding QuipClipper on the local network (D-134): QuipClipper can advertise itself over
//! mDNS/DNS-SD (Bonjour) as `_quipclipper._tcp` (its QC_ADVERTISE, off by default), with
//! TXT `url` (its address) and `id` (its server_id). This only looks when asked
//! (`quip.discover`, Find on Network, or the QuipClipper settings or window opened with no
//! server connected), for a few seconds, and never in the background.
//!
//! What it finds is a suggestion and nothing more: anyone on a network can answer an mDNS
//! question, so connecting one still takes Connect, and pairing or the user's say-so
//! (src/quip/pair.rs). The only requests made here are each answer's `/api/health`, with
//! no credentials, to see that it is QuipClipper.

use super::{get_json, list_detail, normalize_server};
use serde_json::{json, Value};
use std::net::Ipv4Addr;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The service QuipClipper advertises.
pub const SERVICE: &str = "_quipclipper._tcp.local.";
/// How long a look listens for answers.
const LISTEN: Duration = Duration::from_secs(3);

/// A QuipClipper that answered.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    /// Its address, to connect to.
    pub server: String,
    /// Its name on the network (the advertised instance: QuipClipper's server_name).
    pub name: String,
    /// The IP address it answered from, shown beside the name so a stranger stands out.
    pub address: String,
    pub server_id: Option<String>,
    /// It has a password (its health check answered 401): pairing or a token, then.
    pub protected: bool,
}

/// One advertisement, before it's checked: what the TXT record and the addresses say.
#[derive(Debug, Clone, PartialEq)]
pub struct Advert {
    pub name: String,
    pub url: Option<String>,
    pub id: Option<String>,
    pub addresses: Vec<Ipv4Addr>,
    pub port: u16,
}

impl Advert {
    /// The addresses to try, best first: the advertised URL (the one QuipClipper's links
    /// name, so they match), then each IP address and the port.
    pub fn servers(&self) -> Vec<String> {
        let mut out: Vec<String> = self.url.iter().filter_map(|u| normalize_server(u).ok()).collect();
        for a in &self.addresses {
            if let Ok(s) = normalize_server(&format!("http://{a}:{}", self.port)) {
                if !out.contains(&s) {
                    out.push(s);
                }
            }
        }
        out
    }
}

struct Look {
    looking: bool,
    started: Option<Instant>,
    found: Vec<Found>,
    error: Option<String>,
}

static LOOK: Mutex<Look> = Mutex::new(Look { looking: false, started: None, found: Vec::new(), error: None });

fn look() -> std::sync::MutexGuard<'static, Look> {
    LOOK.lock().unwrap_or_else(|e| e.into_inner())
}

/// Start a look, on a thread of its own, unless one is under way. Returns whether one
/// started.
pub fn start() -> bool {
    {
        let mut l = look();
        if l.looking {
            return false;
        }
        l.looking = true;
        l.started = Some(Instant::now());
        l.error = None;
    }
    let spawned = std::thread::Builder::new().name("quip-discover".into()).spawn(|| {
        let res = listen(LISTEN).map(|ads| ads.iter().filter_map(check).collect::<Vec<_>>());
        let mut l = look();
        l.looking = false;
        match res {
            Ok(found) => l.found = found,
            Err(e) => l.error = Some(e),
        }
        drop(l);
        super::wake();
    });
    if let Err(e) = spawned {
        let mut l = look();
        l.looking = false;
        l.error = Some(format!("couldn't start looking: {e}"));
    }
    true
}

/// Start a look unless there was one in the last minute: for windows that look by
/// themselves when they open with no server connected.
pub fn start_if_stale() {
    // Not in tests or screenshot runs, which mustn't go on the network by themselves.
    if cfg!(test) || crate::brand::env("SNAPSHOT").is_some() {
        return;
    }
    if look().started.is_none_or(|t| t.elapsed() > Duration::from_secs(60)) {
        start();
    }
}

/// Whether a look is under way.
pub fn looking() -> bool {
    look().looking
}

/// Whether there's been a look since the app started.
pub fn has_looked() -> bool {
    look().started.is_some()
}

/// The servers the last look found, without those connected already (by address or by
/// server id), and why it failed, if it did.
pub fn found(data_dir: &Path) -> (Vec<Found>, Option<String>) {
    let connected = list_detail(data_dir).unwrap_or_default();
    let l = look();
    let new = l
        .found
        .iter()
        .filter(|f| !connected.iter().any(|c| c.server == f.server || (c.server_id.is_some() && c.server_id == f.server_id)))
        .cloned()
        .collect();
    (new, l.error.clone())
}

/// `quip.discover`'s answer.
pub fn json(data_dir: &Path) -> Value {
    let (found, error) = found(data_dir);
    let list: Vec<Value> = found.iter().map(|f| json!({"server": f.server, "name": f.name, "address": f.address, "protected": f.protected})).collect();
    json!({"looking": looking(), "found": list, "error": error})
}

/// Listen for QuipClipper's advertisements for `how_long`.
fn listen(how_long: Duration) -> Result<Vec<Advert>, String> {
    use mdns_sd::{ServiceDaemon, ServiceEvent};
    // Tests hear nothing unless they ask for the network (finds_an_advertised_server).
    if cfg!(test) && std::env::var_os("LEADSTINGER_MDNS_TEST").is_none() {
        return Ok(Vec::new());
    }
    let mdns = ServiceDaemon::new().map_err(|e| format!("can't look on the network: {e}"))?;
    let rx = mdns.browse(SERVICE).map_err(|e| format!("can't look on the network: {e}"))?;
    let deadline = Instant::now() + how_long;
    let mut ads: Vec<Advert> = Vec::new();
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        match rx.recv_timeout(left) {
            Ok(ServiceEvent::ServiceResolved(info)) => {
                let name = info.get_fullname().strip_suffix(&format!(".{SERVICE}")).unwrap_or(info.get_fullname()).to_owned();
                let ad = Advert {
                    name: clean(&name).unwrap_or_else(|| "QuipClipper".into()),
                    url: info.get_property_val_str("url").and_then(clean),
                    id: info.get_property_val_str("id").and_then(clean),
                    addresses: info.get_addresses_v4().into_iter().collect(),
                    port: info.get_port(),
                };
                ads.retain(|a| a.name != ad.name);
                ads.push(ad);
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
    mdns.shutdown().ok();
    Ok(ads)
}

/// A TXT value or name fit to show: one line, at most 200 characters.
fn clean(s: &str) -> Option<String> {
    let t = s.trim();
    (!t.is_empty() && t.chars().count() <= 200 && !t.chars().any(char::is_control)).then(|| t.to_owned())
}

/// The first of an advert's addresses that answers as QuipClipper.
fn check(ad: &Advert) -> Option<Found> {
    ad.servers().into_iter().find_map(|server| {
        let protected = match get_json(&format!("{server}/api/health"), &[]) {
            Ok((200, v)) if v["service"] == "quipclipper-web" => false,
            Ok((401, _)) => true,
            _ => return None,
        };
        let address = ad.addresses.first().map_or_else(|| super::host_of(&server), Ipv4Addr::to_string);
        Some(Found { server, name: ad.name.clone(), address, server_id: ad.id.clone(), protected })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quip::tests::{response, serve};

    #[test]
    fn an_advert_offers_its_url_then_its_addresses() {
        let ad = Advert {
            name: "nas".into(),
            url: Some("http://NAS.local:8896/".into()),
            id: Some("abc".into()),
            addresses: vec![Ipv4Addr::new(192, 168, 1, 20)],
            port: 8896,
        };
        assert_eq!(ad.servers(), ["http://nas.local:8896", "http://192.168.1.20:8896"]);
        // A TXT url that isn't http(s) is ignored.
        let bad = Advert { url: Some("file:///etc/passwd".into()), ..ad.clone() };
        assert_eq!(bad.servers(), ["http://192.168.1.20:8896"]);
        assert_eq!(clean(" nas\n"), Some("nas".into()));
        assert_eq!(clean("a\u{7}b"), None);
    }

    #[test]
    fn only_quipclipper_is_listed() {
        let qc = serve(|req, _| match req {
            "GET /api/health HTTP/1.1" => response("200 OK", "", br#"{"status": "ok", "service": "quipclipper-web"}"#),
            _ => response("404 Not Found", "", b"{}"),
        });
        let gated = serve(|_, _| response("401 Unauthorized", "", b""));
        let other = serve(|_, _| response("200 OK", "", br#"{"status": "ok"}"#));
        let ad = |url: &str| Advert { name: "x".into(), url: Some(url.into()), id: None, addresses: vec![], port: 1 };
        let found = check(&ad(&qc)).unwrap();
        assert_eq!((found.server.as_str(), found.protected), (qc.as_str(), false));
        assert!(check(&ad(&gated)).unwrap().protected);
        assert_eq!(check(&ad(&other)), None);
        assert_eq!(check(&ad("http://127.0.0.1:1")), None);
    }

    /// A real advertisement on this machine's network, found by a real look. Multicast
    /// isn't allowed everywhere tests run, so this one runs only when asked:
    /// `LEADSTINGER_MDNS_TEST=1 cargo test finds_an_advertised_server`.
    #[test]
    fn finds_an_advertised_server() {
        if std::env::var_os("LEADSTINGER_MDNS_TEST").is_none() {
            return;
        }
        let qc = serve(|_, _| response("200 OK", "", br#"{"service": "quipclipper-web"}"#));
        let port: u16 = qc.rsplit(':').next().unwrap().parse().unwrap();
        let daemon = mdns_sd::ServiceDaemon::new().unwrap();
        let host = format!("{}.local.", crate::quip::pair::host_name());
        let props = [("v", "1"), ("url", qc.as_str()), ("id", "test-id")];
        let info = mdns_sd::ServiceInfo::new(SERVICE, "leadstinger-test", &host, "127.0.0.1", port, &props[..]).unwrap();
        daemon.register(info).unwrap();
        let ads = listen(Duration::from_secs(4)).unwrap();
        daemon.shutdown().ok();
        let ad = ads.iter().find(|a| a.name == "leadstinger-test").expect("heard it");
        assert_eq!((ad.url.as_deref(), ad.id.as_deref()), (Some(qc.as_str()), Some("test-id")));
        assert_eq!(check(ad).map(|f| f.server), Some(qc));
    }
}
