//! Pairing with QuipClipper (D-133): connecting a server without anyone copying its API
//! token. The contract is QuipClipper's `quipclipper.pair/1` (its docs/API.md, Pairing).
//!
//! `quip.pair <url>` asks the server for a pairing (`POST /api/pair`, with this app's name
//! and the machine's host name, and no credentials), which answers with a short code. The
//! user sees the same code in QuipClipper's web page and clicks Allow there; meanwhile this
//! asks every 2 s (`GET /api/pair/<id>` with the request's secret) until the server hands
//! over a token of this app's own, which is stored like any other ([`super::connect_with`])
//! and can be revoked on its own (Disconnect does, [`revoke`]). A server without pairing
//! that has no password is connected without a token; one with a password and no pairing
//! needs its API token, given in Settings or with `quip.connect --token -`.
//!
//! Everything runs on a thread of its own, so neither the window nor a command waits on
//! the server: `quip.pair` returns at once and [`json`] (`quip.pairing`) says how it went.

use super::{connect_with, get_json, post_json, server_info, Connection};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How often the server is asked whether the user has answered.
const POLL: Duration = if cfg!(test) { Duration::from_millis(40) } else { Duration::from_secs(2) };
/// The longest a pairing is waited for, whatever the server says.
const MAX_WAIT: Duration = Duration::from_secs(15 * 60);

/// Where a pairing has got to.
#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    /// Asking the server what it is, and for a pairing.
    Starting,
    /// Waiting for the user to allow it in QuipClipper, where `code` shows too.
    Waiting { code: String, approve_url: String, expires_in: u64 },
    /// Connected: with a token of its own, or (`token: false`) an open server without one.
    Connected { token: bool },
    Denied,
    Expired,
    Cancelled,
    Failed(String),
}

impl Status {
    pub fn is_done(&self) -> bool {
        !matches!(self, Status::Starting | Status::Waiting { .. })
    }

    fn json(&self, server: &str) -> Value {
        let mut v = json!({"server": server});
        let (status, extra) = match self {
            Status::Starting => ("starting", json!({})),
            Status::Waiting { code, approve_url, expires_in } => ("waiting", json!({"code": code, "approve_url": approve_url, "expires_in": expires_in})),
            Status::Connected { token } => ("connected", json!({"token": token})),
            Status::Denied => ("denied", json!({"message": "QuipClipper said no (Deny)"})),
            Status::Expired => ("expired", json!({"message": "Nobody allowed it in QuipClipper in time"})),
            Status::Cancelled => ("cancelled", json!({})),
            Status::Failed(m) => ("failed", json!({"message": m})),
        };
        v["status"] = json!(status);
        if let (Some(v), Some(extra)) = (v.as_object_mut(), extra.as_object()) {
            v.extend(extra.clone());
        }
        v
    }
}

struct Pairing {
    status: Status,
    /// Which start this is: a thread from an earlier one leaves the status alone.
    generation: u64,
    cancel: Arc<AtomicBool>,
}

static PAIRINGS: Mutex<BTreeMap<String, Pairing>> = Mutex::new(BTreeMap::new());

fn pairings() -> std::sync::MutexGuard<'static, BTreeMap<String, Pairing>> {
    PAIRINGS.lock().unwrap_or_else(|e| e.into_inner())
}

/// Start pairing with `server` (normalised), on a thread of its own; one under way for it
/// already is left running. [`status`] says how it goes.
pub fn start(data_dir: &Path, server: &str) {
    let mut all = pairings();
    if all.get(server).is_some_and(|p| !p.status.is_done()) {
        return;
    }
    let generation = all.get(server).map_or(0, |p| p.generation + 1);
    let cancel = Arc::new(AtomicBool::new(false));
    all.insert(server.to_owned(), Pairing { status: Status::Starting, generation, cancel: cancel.clone() });
    drop(all);
    let (dir, s) = (data_dir.to_path_buf(), server.to_owned());
    let spawned = std::thread::Builder::new().name("quip-pair".into()).spawn(move || {
        let end = run(&dir, &s, generation, &cancel);
        set(&s, generation, end);
    });
    if let Err(e) = spawned {
        set(server, generation, Status::Failed(format!("couldn't start pairing: {e}")));
    }
}

/// Stop waiting for a pairing. Returns whether one was under way.
pub fn cancel(server: &str) -> bool {
    let mut all = pairings();
    let Some(p) = all.get_mut(server).filter(|p| !p.status.is_done()) else { return false };
    p.cancel.store(true, Ordering::SeqCst);
    p.status = Status::Cancelled;
    true
}

/// How the pairing with `server` went, if one was started since the app started.
pub fn status(server: &str) -> Option<Status> {
    pairings().get(server).map(|p| p.status.clone())
}

/// `quip.pairing`: the pairings started since the app started (or just `server`'s).
pub fn json(server: Option<&str>) -> Value {
    let all = pairings();
    Value::Array(all.iter().filter(|(s, _)| server.is_none_or(|w| w == s.as_str())).map(|(s, p)| p.status.json(s)).collect())
}

/// Forget a finished pairing, so the window stops showing how it ended.
pub fn forget(server: &str) {
    let mut all = pairings();
    if all.get(server).is_some_and(|p| p.status.is_done()) {
        all.remove(server);
    }
}

fn set(server: &str, generation: u64, status: Status) {
    let mut all = pairings();
    if let Some(p) = all.get_mut(server).filter(|p| p.generation == generation && !matches!(p.status, Status::Cancelled)) {
        p.status = status;
    }
    drop(all);
    super::wake();
}

/// The pairing itself, on its thread: what it ended as.
fn run(data_dir: &Path, server: &str, generation: u64, cancel: &AtomicBool) -> Status {
    let info = match server_info(server) {
        Ok(i) => i,
        Err(e) => return Status::Failed(e),
    };
    let open = |info: &super::ServerInfo| {
        if info.auth_required {
            return Status::Failed(format!("the QuipClipper at {server} has a password and doesn't offer pairing: connect it with its API token (QC_API_TOKEN) in Settings ▸ QuipClipper"));
        }
        match connect_with(data_dir, server, Connection { server_id: info.server_id.clone(), ..Connection::default() }) {
            Ok(_) => Status::Connected { token: false },
            Err(e) => Status::Failed(e),
        }
    };
    if !info.pairing {
        return open(&info);
    }
    let body = json!({"name": crate::brand::APP_NAME, "host": host_name()});
    let (status, v) = match post_json(&format!("{server}/api/pair"), &body, &[]) {
        Ok(a) => a,
        Err(e) => return Status::Failed(e),
    };
    let (id, code, secret) = match status {
        200 | 201 => match (field(&v, "id"), field(&v, "code"), field(&v, "poll_secret")) {
            (Some(id), Some(code), Some(secret)) => (id, code, secret),
            _ => return Status::Failed(format!("the QuipClipper at {server} answered the pairing request with something it doesn't understand")),
        },
        // An older QuipClipper, or pairing turned off.
        404 | 405 => return open(&super::ServerInfo { pairing: false, ..info }),
        401 | 403 => return open(&super::ServerInfo { pairing: false, auth_required: true, ..info }),
        429 => return Status::Failed(format!("the QuipClipper at {server} has too many pairing requests waiting; answer or let them expire, then try again")),
        s => return Status::Failed(format!("the QuipClipper at {server} answered HTTP {s} to the pairing request")),
    };
    let expires_in = v["expires_in"].as_u64().unwrap_or(600).clamp(30, MAX_WAIT.as_secs());
    let approve_url = format!("{server}/?pair={}", super::percent_encode(&id));
    set(server, generation, Status::Waiting { code, approve_url, expires_in });
    let deadline = Instant::now() + Duration::from_secs(expires_in);
    let url = format!("{server}/api/pair/{}", super::percent_encode(&id));
    loop {
        // Asleep in short steps, so Cancel is quick.
        let wake_at = Instant::now() + POLL;
        while Instant::now() < wake_at {
            if cancel.load(Ordering::SeqCst) {
                return Status::Cancelled;
            }
            std::thread::sleep(Duration::from_millis(20).min(POLL));
        }
        if Instant::now() > deadline {
            return Status::Expired;
        }
        // A server that doesn't answer for a while is asked again until the deadline.
        let Ok((status, v)) = get_json(&url, &[("X-Pair-Secret", &secret)]) else { continue };
        match (status, v["status"].as_str()) {
            (200, Some("pending")) => {}
            (200, Some("approved")) => {
                let Some(token) = field(&v, "token").filter(|t| super::check_token(t).is_ok()) else {
                    return Status::Failed(format!("the QuipClipper at {server} approved the pairing but sent no usable token"));
                };
                if cancel.load(Ordering::SeqCst) {
                    // Allowed after all, but the user gave up: don't keep it.
                    revoke(server, &token);
                    return Status::Cancelled;
                }
                let how = Connection { token: Some(token), client_id: field(&v, "client_id"), server_id: info.server_id.clone() };
                return match connect_with(data_dir, server, how) {
                    Ok(_) => Status::Connected { token: true },
                    Err(e) => Status::Failed(e),
                };
            }
            (200, Some("denied")) => return Status::Denied,
            (200, Some("expired")) | (404, _) => return Status::Expired,
            _ => {}
        }
    }
}

/// A short, plain value from a server's answer: at most 200 printable ASCII characters.
fn field(v: &Value, k: &str) -> Option<String> {
    v[k].as_str().filter(|s| !s.is_empty() && s.len() <= 200 && s.bytes().all(|b| b.is_ascii_graphic())).map(str::to_owned)
}

/// Tell a server to revoke this app's paired token (`DELETE /api/clients/self`). Returns
/// whether it said it did.
pub fn revoke(server: &str, token: &str) -> bool {
    matches!(super::delete(&format!("{server}/api/clients/self"), token), Ok((200..=299, v)) if v["removed"] != json!(false))
}

/// This machine's short host name, so QuipClipper can say which computer is asking.
pub fn host_name() -> String {
    let read = |p: &str| std::fs::read_to_string(p).ok();
    let run = |cmd: &str, args: &[&str]| std::process::Command::new(cmd).args(args).output().ok().filter(|o| o.status.success()).and_then(|o| String::from_utf8(o.stdout).ok());
    let name = if cfg!(target_os = "macos") {
        run("/usr/sbin/scutil", &["--get", "LocalHostName"]).or_else(|| run("hostname", &["-s"]))
    } else {
        read("/proc/sys/kernel/hostname").or_else(|| read("/etc/hostname")).or_else(|| run("hostname", &[]))
    };
    let name = name.unwrap_or_default();
    let short = name.trim().split('.').next().unwrap_or_default();
    short.chars().filter(|c| !c.is_control()).take(100).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quip::tests::{controller, response, serve};
    use std::sync::atomic::AtomicUsize;

    fn wait_done(server: &str) -> Status {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let s = status(server).expect("started");
            if s.is_done() || Instant::now() > deadline {
                return s;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn wait_waiting(server: &str) -> Status {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let s = status(server).expect("started");
            if matches!(s, Status::Waiting { .. }) || s.is_done() || Instant::now() > deadline {
                return s;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    const CONFIG: &[u8] = br#"{"media_roots": [], "auth_required": false, "pairing": true, "server_id": "abc123", "server_name": "nas"}"#;

    /// A QuipClipper that pairs: pending for `pending` polls, then `answer`.
    fn pairing_server(pending: usize, answer: &'static str) -> (String, Arc<Mutex<Vec<String>>>) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let polls = Arc::new(AtomicUsize::new(0));
        let s = seen.clone();
        let base = serve(move |req, headers| {
            s.lock().unwrap().push(format!("{req} {}", headers.join(" | ")));
            let secret_ok = headers.iter().any(|h| h.eq_ignore_ascii_case("x-pair-secret: sekrit"));
            match req {
                "GET /api/config HTTP/1.1" => response("200 OK", "Content-Type: application/json\r\n", CONFIG),
                "POST /api/pair HTTP/1.1" => response("200 OK", "Content-Type: application/json\r\n", br#"{"id": "r1", "code": "7F3K", "poll_secret": "sekrit", "expires_in": 600}"#),
                "GET /api/pair/r1 HTTP/1.1" if secret_ok => {
                    if polls.fetch_add(1, Ordering::SeqCst) < pending {
                        response("200 OK", "", br#"{"status": "pending"}"#)
                    } else {
                        response("200 OK", "", answer.as_bytes())
                    }
                }
                "DELETE /api/clients/self HTTP/1.1" => response("200 OK", "", br#"{"removed": true}"#),
                _ => response("404 Not Found", "", b"{}"),
            }
        });
        (base, seen)
    }

    #[test]
    fn pairs_and_keeps_the_token_it_was_given() {
        let (server, seen) = pairing_server(3, r#"{"status": "approved", "token": "qcc_t0ken", "client_id": "c9"}"#);
        let (mut c, root) = controller();
        let r = c.execute("quip.pair", json!({"url": server})).unwrap();
        assert!(matches!(r["status"].as_str(), Some("starting" | "waiting")), "{r}");
        match wait_waiting(&server) {
            Status::Waiting { code, approve_url, .. } => {
                assert_eq!(code, "7F3K");
                assert_eq!(approve_url, format!("{server}/?pair=r1"));
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(wait_done(&server), Status::Connected { token: true });
        let dir = &c.store.data_dir;
        assert_eq!(crate::quip::token_for(dir, &server).unwrap().as_deref(), Some("qcc_t0ken"));
        let how = crate::quip::connection(dir, &server).unwrap().unwrap();
        assert_eq!((how.client_id.as_deref(), how.server_id.as_deref()), (Some("c9"), Some("abc123")));
        let listed = c.execute("quip.list", json!({})).unwrap();
        assert_eq!((listed[0]["token"].as_bool(), listed[0]["paired"].as_bool()), (Some(true), Some(true)));
        assert!(!listed.to_string().contains("qcc_t0ken"));
        // Nothing but the pairing calls, and no credentials before the token arrived.
        let seen = seen.lock().unwrap().clone();
        assert!(seen.iter().all(|r| !r.to_ascii_lowercase().contains("x-api-key") && !r.to_ascii_lowercase().contains("authorization")), "{seen:?}");
        assert!(seen.iter().any(|r| r.starts_with("POST /api/pair") && r.to_ascii_lowercase().contains("x-quipclipper: 1")), "{seen:?}");
        // Disconnect revokes the paired token on the server (waiting for it, with no window).
        c.has_window = false;
        let r = c.execute("quip.disconnect", json!({"url": server})).unwrap();
        assert_eq!((r["removed"].as_bool(), r["revoked"].as_bool()), (Some(true), Some(true)));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn a_denied_or_expired_pairing_stores_nothing() {
        for (answer, end) in [(r#"{"status": "denied"}"#, Status::Denied), (r#"{"status": "expired"}"#, Status::Expired)] {
            let (server, _) = pairing_server(1, answer);
            let (mut c, root) = controller();
            c.execute("quip.pair", json!({"url": server})).unwrap();
            assert_eq!(wait_done(&server), end);
            assert_eq!(crate::quip::token_for(&c.store.data_dir, &server).unwrap(), None);
            std::fs::remove_dir_all(root).ok();
        }
    }

    #[test]
    fn cancel_stops_waiting() {
        let (server, _) = pairing_server(usize::MAX, "{}");
        let (mut c, root) = controller();
        c.execute("quip.pair", json!({"url": server})).unwrap();
        assert!(matches!(wait_waiting(&server), Status::Waiting { .. }));
        let r = c.execute("quip.pair", json!({"url": server, "cancel": true})).unwrap();
        assert_eq!(r["cancelled"].as_bool(), Some(true));
        assert_eq!(status(&server), Some(Status::Cancelled));
        std::thread::sleep(POLL * 3);
        assert_eq!(status(&server), Some(Status::Cancelled), "the thread doesn't overwrite it");
        assert_eq!(crate::quip::token_for(&c.store.data_dir, &server).unwrap(), None);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn falls_back_without_pairing() {
        // An older QuipClipper with no password: connected without a token, and sent none.
        let seen = Arc::new(Mutex::new(Vec::new()));
        let s = seen.clone();
        let open = serve(move |req, headers| {
            s.lock().unwrap().push(headers.join(" | "));
            match req {
                "GET /api/config HTTP/1.1" => response("200 OK", "", br#"{"media_roots": [], "auth_required": false}"#),
                _ => response("404 Not Found", "", b"{}"),
            }
        });
        let (mut c, root) = controller();
        c.execute("quip.pair", json!({"url": open})).unwrap();
        assert_eq!(wait_done(&open), Status::Connected { token: false });
        assert_eq!(crate::quip::token_for(&c.store.data_dir, &open).unwrap().as_deref(), Some(""));
        assert!(seen.lock().unwrap().iter().all(|h| !h.to_ascii_lowercase().contains("authorization")));

        // One that says pairing but 404s it (turned off): the same.
        let off = serve(|req, _| match req {
            "GET /api/config HTTP/1.1" => response("200 OK", "", br#"{"media_roots": [], "auth_required": false, "pairing": true}"#),
            _ => response("404 Not Found", "", b"{}"),
        });
        c.execute("quip.pair", json!({"url": off})).unwrap();
        assert_eq!(wait_done(&off), Status::Connected { token: false });

        // A password and no pairing: it needs the token.
        let gated = serve(|_, _| response("401 Unauthorized", "WWW-Authenticate: Basic realm=\"q\"\r\n", b""));
        c.execute("quip.pair", json!({"url": gated})).unwrap();
        assert!(matches!(wait_done(&gated), Status::Failed(m) if m.contains("API token")));
        assert_eq!(crate::quip::token_for(&c.store.data_dir, &gated).unwrap(), None);

        // Not QuipClipper at all.
        let other = serve(|_, _| response("200 OK", "Content-Type: text/html\r\n", b"<html></html>"));
        c.execute("quip.pair", json!({"url": other})).unwrap();
        assert!(matches!(wait_done(&other), Status::Failed(m) if m.contains("doesn't look like QuipClipper")));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn host_name_is_short_and_plain() {
        let h = host_name();
        assert!(!h.contains('.') && !h.chars().any(char::is_control) && h.chars().count() <= 100, "{h:?}");
    }
}
