//! QuipClipper transcription against a mock server that speaks the endpoint as QuipClipper
//! documents it (POST /api/transcribe, GET and DELETE /api/jobs/{id}, /api/config's
//! `transcribe.upload`).

use super::*;
use crate::controller::Controller;
use crate::quip::tests::response;
use serde_json::json;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::atomic::AtomicUsize;

/// A request as the mock saw it.
#[derive(Clone, Debug)]
struct Req {
    method: String,
    path: String,
    headers: Vec<String>,
    body: Vec<u8>,
}

impl Req {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find_map(|h| {
            let (k, v) = h.split_once(':')?;
            k.trim().eq_ignore_ascii_case(name).then(|| v.trim())
        })
    }
}

type Log = Arc<Mutex<Vec<Req>>>;

/// A server answering each request (one thread per connection, body read) with
/// `reply`; an empty reply drops the connection unanswered. Returns its base URL and
/// every request it was sent.
fn mock(reply: impl Fn(&Req) -> Vec<u8> + Send + Sync + 'static) -> (String, Log) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let log: Log = Arc::default();
    let (reply, seen) = (Arc::new(reply), log.clone());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let (reply, seen) = (reply.clone(), seen.clone());
            std::thread::spawn(move || {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut first = String::new();
                reader.read_line(&mut first).ok();
                let mut headers = Vec::new();
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line.trim().is_empty() {
                        break;
                    }
                    headers.push(line.trim().to_owned());
                }
                let mut parts = first.split_whitespace();
                let (method, path) = (parts.next().unwrap_or("").to_owned(), parts.next().unwrap_or("").to_owned());
                let mut req = Req { method, path, headers, body: Vec::new() };
                let len: usize = req.header("content-length").and_then(|l| l.parse().ok()).unwrap_or(0);
                req.body = vec![0; len];
                reader.read_exact(&mut req.body).ok();
                seen.lock().unwrap().push(req.clone());
                let out = reply(&req);
                if !out.is_empty() {
                    stream.write_all(&out).ok();
                }
            });
        }
    });
    (base, log)
}

fn answer(status: &str, v: Value) -> Vec<u8> {
    response(status, "Content-Type: application/json\r\n", v.to_string().as_bytes())
}

fn config(max_bytes: u64, max_seconds: f64) -> Value {
    json!({"server_name": "Booth", "transcribe": {"upload": {"max_bytes": max_bytes, "max_seconds": max_seconds}}})
}

fn cues_with_words() -> Value {
    json!({
        "cues": [{"start": 0.5, "end": 1.5, "text": "Hailing frequencies open.", "machine": true, "aligned": true,
                  "words": [{"start": 0.5, "end": 0.9, "text": "Hailing"}, {"start": 0.9, "end": 1.3, "text": "frequencies"}, {"start": 1.3, "end": 1.5, "text": "open."}]}],
        "cues_source": {"kind": "machine", "engine": "whisper.cpp", "model": "small.en", "lang": "en"},
        "duration": 2.0
    })
}

/// A server that takes uploads (64 MB, 300 s) and hears `result` after `running` polls
/// that say "0:01 of 0:02 transcribed".
fn quip_server(result: Value, running: usize) -> (String, Log) {
    let polls = AtomicUsize::new(0);
    mock(move |r| match (r.method.as_str(), r.path.split('?').next().unwrap_or("")) {
        ("GET", "/api/config") => answer("200 OK", config(64 << 20, 300.0)),
        ("POST", PATH) => answer("200 OK", json!({"job_id": "j1", "duration": 2.0})),
        ("GET", "/api/jobs/j1") if polls.fetch_add(1, Ordering::SeqCst) < running => {
            answer("200 OK", json!({"id": "j1", "status": "running", "progress": "0:01 of 0:02 transcribed"}))
        }
        ("GET", "/api/jobs/j1") => answer("200 OK", json!({"id": "j1", "status": "done", "result": result})),
        ("DELETE", "/api/jobs/j1") => answer("200 OK", json!({"cancelling": true})),
        _ => answer("404 Not Found", json!({"detail": "Not Found"})),
    })
}

fn temp() -> PathBuf {
    let d = std::env::temp_dir().join(format!("sb-quip-tr-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A data folder connected to `server` with a token, and an engine for it (checked, so
/// one config request), polling quickly.
fn engine(server: &str) -> (Quip, PathBuf) {
    let dir = temp();
    crate::quip::connect(&dir, server, "tok3n").unwrap();
    let ctx = Context { quip: checked(&dir), data_dir: Some(dir.clone()), ..Context::default() };
    let mut q = Quip::open(&ctx).unwrap();
    q.poll = Duration::from_millis(20);
    (q, dir)
}

fn request(seconds: f64) -> Request {
    let n = (seconds * 16_000.0) as usize;
    let samples: Vec<f32> = (0..n).map(|i| (i as f32 * 0.05).sin() * 0.3).collect();
    Request { wav: Arc::new(crate::wav::mono_bytes(16_000, &samples)), seconds, lang: Some("en".into()) }
}

fn count(log: &Log, method: &str, path: &str) -> usize {
    log.lock().unwrap().iter().filter(|r| r.method == method && r.path.starts_with(path)).count()
}

/// Wait up to two seconds for a request to arrive.
fn arrives(log: &Log, method: &str, path: &str) -> bool {
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if count(log, method, path) > 0 {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

#[test]
fn reads_what_a_server_takes_and_how_far_a_job_is() {
    assert_eq!(upload_state(&config(1000, 30.0)), State::Upload(Caps { max_bytes: Some(1000), max_seconds: Some(30.0) }));
    assert_eq!(upload_state(&json!({"transcribe": {"upload": {}}})), State::Upload(Caps { max_bytes: None, max_seconds: None }));
    assert_eq!(upload_state(&json!({"transcribe": {"upload": false}})), State::Off);
    assert_eq!(upload_state(&json!({"transcribe": {"enabled": true}})), State::Off, "an older QuipClipper");
    assert_eq!(upload_state(&json!({"media_roots": []})), State::Off, "transcription off: no block at all");
    assert_eq!(upload_state(&json!({"transcribe": {"upload": {"max_bytes": 0, "max_seconds": 300}}})), State::Off);
    assert_eq!(fraction(&json!("0:12 of 0:40 transcribed")), Some(0.3));
    assert_eq!(fraction(&json!("1:00:00 of 2:00:00 transcribed")), Some(0.5));
    assert_eq!(fraction(&json!("0:50 of 0:40 transcribed")), Some(1.0));
    assert_eq!(fraction(&json!(0.25)), Some(0.25));
    assert_eq!(fraction(&json!(40)), Some(0.4));
    assert_eq!(fraction(&json!("decoding")), None);
    assert_eq!(fraction(&Value::Null), None);
}

#[test]
fn picks_a_server_by_setting_then_by_address() {
    let offer = |s: &str, state: State| Offer { server: s.into(), name: None, state };
    let up = State::Upload(Caps { max_bytes: None, max_seconds: None });
    assert!(choose(&[], None).unwrap_err().contains("no QuipClipper server is connected"));
    let one = [offer("http://a.example", State::Off)];
    assert!(choose(&one, None).unwrap_err().contains("doesn't transcribe"));
    let all = [offer("http://a.example", State::Off), offer("http://b.example", up.clone()), offer("http://c.example", up.clone())];
    let first = choose(&all, None).unwrap();
    assert_eq!(first.server, "http://b.example");
    assert_eq!(why_chosen(&all, first, None), "the first, by address, of the connected servers that transcribe");
    let picked = choose(&all, Some("http://c.example")).unwrap();
    assert_eq!((picked.server.as_str(), why_chosen(&all, picked, Some("http://c.example"))), ("http://c.example", "chosen in Settings"));
    // A chosen server that can't is said so, not passed over; one no longer connected is.
    assert!(choose(&all, Some("http://a.example")).unwrap_err().contains("a.example doesn't transcribe"));
    assert_eq!(choose(&all, Some("http://gone.example")).unwrap().server, "http://b.example");
    let none = [offer("http://a.example", State::Off), offer("http://b.example", State::Failed("couldn't be asked whether it transcribes: refused".into()))];
    let e = choose(&none, None).unwrap_err();
    assert!(e.starts_with("none of the 2 connected QuipClipper servers transcribes"), "{e}");
    let named = Offer { name: Some("Booth".into()), ..offer("http://q.example:8080", up) };
    assert_eq!(named.label(), "Booth (q.example:8080)");
}

#[test]
fn transcribes_with_words_and_sends_the_audio_only_to_its_server() {
    let (server, log) = quip_server(cues_with_words(), 2);
    let (q, _dir) = engine(&server);
    assert_eq!(q.destination().as_deref(), Some(format!("QuipClipper at Booth ({})", crate::quip::host_of(&server)).as_str()));
    let req = request(2.0);
    let heard = q.transcribe(&req, &Arc::new(Progress::default())).unwrap();
    assert_eq!((heard.model.as_deref(), heard.lang.as_deref()), (Some("small.en"), Some("en")));
    let l = &heard.lines[0];
    assert_eq!((l.start, l.end, l.text.as_str()), (0.5, 1.5, "Hailing frequencies open."));
    assert_eq!(l.words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>(), ["Hailing", "frequencies", "open."]);
    assert!(!l.aligned, "a server's words are never placed by hand");
    let log = log.lock().unwrap();
    let post = log.iter().find(|r| r.method == "POST").unwrap();
    assert_eq!(post.path, "/api/transcribe?lang=en");
    assert_eq!(post.body, *req.wav, "the body is the WAV itself");
    assert_eq!((post.header("content-type"), post.header("x-api-key"), post.header("x-quipclipper")), (Some("audio/wav"), Some("tok3n"), Some("1")));
    assert!(log.iter().all(|r| r.header("x-api-key") == Some("tok3n")), "every request carries the token");
}

#[test]
fn lines_without_words_come_back_bare() {
    let result = json!({"cues": [{"start": 0.2, "end": 1.0, "text": "No words here", "machine": true}], "cues_source": {"kind": "machine", "model": "small.en", "lang": "en"}});
    let (server, _) = quip_server(result, 0);
    let (q, _dir) = engine(&server);
    let heard = q.transcribe(&request(1.5), &Arc::new(Progress::default())).unwrap();
    assert_eq!(heard.lines.len(), 1);
    assert!(heard.lines[0].words.is_empty());
    let (server, _) = quip_server(json!({"cues": [], "cues_source": {"kind": "machine"}}), 0);
    let (q, _dir) = engine(&server);
    assert_eq!(q.transcribe(&request(1.0), &Arc::new(Progress::default())).unwrap().lines, vec![], "no speech: no lines");
}

#[test]
fn shows_how_far_the_server_is() {
    let release = Arc::new(AtomicBool::new(false));
    let r = release.clone();
    let (server, _) = mock(move |req| match (req.method.as_str(), req.path.split('?').next().unwrap_or("")) {
        ("GET", "/api/config") => answer("200 OK", config(64 << 20, 300.0)),
        ("POST", PATH) => answer("200 OK", json!({"job_id": "j1"})),
        ("GET", "/api/jobs/j1") if !r.load(Ordering::SeqCst) => answer("200 OK", json!({"status": "running", "progress": "0:12 of 0:40 transcribed"})),
        ("GET", "/api/jobs/j1") => answer("200 OK", json!({"status": "done", "result": cues_with_words()})),
        _ => answer("404 Not Found", json!({})),
    });
    let (q, _dir) = engine(&server);
    let progress = Arc::new(Progress::default());
    let p = progress.clone();
    let worker = std::thread::spawn(move || q.transcribe(&request(1.0), &p));
    let deadline = Instant::now() + Duration::from_secs(5);
    while progress.fraction().is_none() {
        assert!(Instant::now() < deadline, "no progress shown");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!((progress.fraction().unwrap() - 0.3).abs() < 1e-6);
    release.store(true, Ordering::SeqCst);
    assert!(worker.join().unwrap().is_ok());
}

/// The error a server answering the upload with `status` gives.
fn upload_error(status: &'static str, body: Value) -> (String, String) {
    let (server, _) = mock(move |r| match (r.method.as_str(), r.path.split('?').next().unwrap_or("")) {
        ("GET", "/api/config") => answer("200 OK", config(64 << 20, 300.0)),
        ("POST", PATH) => answer(status, body.clone()),
        _ => answer("404 Not Found", json!({})),
    });
    let (q, _dir) = engine(&server);
    let e = q.transcribe(&request(0.5), &Arc::new(Progress::default())).unwrap_err();
    (e, server)
}

#[test]
fn each_refusal_is_one_plain_sentence() {
    let cases: &[(&str, &str)] = &[
        ("413 Payload Too Large", "refused it as too long or too large (it takes up to 64 MB or 5:00); trim the tile"),
        ("400 Bad Request", "couldn't read the audio it was sent"),
        ("422 Unprocessable Entity", "doesn't hear the language en"),
        ("429 Too Many Requests", "is busy with other uploads"),
        ("503 Service Unavailable", "has transcription switched off or not set up"),
        ("401 Unauthorized", "no longer accepts this app's pairing: connect it again"),
        ("500 Internal Server Error", "answered HTTP 500, which this version doesn't understand"),
        ("200 OK", "answered the upload without a job id"),
    ];
    for (status, want) in cases {
        let (e, server) = upload_error(status, json!({"detail": "x"}));
        assert!(e.starts_with("the QuipClipper at Booth ("), "{status}: {e}");
        assert!(e.contains(want), "{status}: {e}");
        assert!(!e.contains('\n') && e.matches(". ").count() == 0, "one sentence: {e}");
        if ["503", "401", "500", "413"].iter().any(|s| status.starts_with(s)) {
            assert_eq!(cache().probes[&server].state, State::Unchecked, "{status}: asked again before it's offered next");
        }
    }
}

#[test]
fn a_job_that_fails_goes_missing_or_loses_its_server_says_so() {
    let job = |status: &'static str, body: Value| {
        let (server, _) = mock(move |r| match (r.method.as_str(), r.path.split('?').next().unwrap_or("")) {
            ("GET", "/api/config") => answer("200 OK", config(64 << 20, 300.0)),
            ("POST", PATH) => answer("200 OK", json!({"job_id": "j1"})),
            ("GET", "/api/jobs/j1") if status.is_empty() => Vec::new(),
            ("GET", "/api/jobs/j1") => answer(status, body.clone()),
            _ => answer("404 Not Found", json!({})),
        });
        let (q, _dir) = engine(&server);
        q.transcribe(&request(0.5), &Arc::new(Progress::default())).unwrap_err()
    };
    let e = job("200 OK", json!({"status": "failed", "error": "whisper.cpp exited 1\nstack…"}));
    assert!(e.ends_with("couldn't transcribe it: whisper.cpp exited 1"), "{e}");
    assert!(job("404 Not Found", json!({})).contains("no longer has the transcription (it may have restarted)"));
    assert!(job("403 Forbidden", json!({})).contains("no longer accepts this app's pairing"));
    assert!(job("200 OK", json!({"status": "exploded"})).contains("answered a job status, which this version doesn't understand"));
    assert!(job("200 OK", json!({"status": "done", "result": {"text": "hi"}})).contains("a finished transcription without its lines"));
    let e = job("", json!({}));
    assert!(e.starts_with("lost the connection to the QuipClipper at Booth"), "{e}");
}

#[test]
fn a_server_that_no_longer_answers_or_was_disconnected() {
    let (server, _) = quip_server(cues_with_words(), 0);
    let (q, dir) = engine(&server);
    crate::quip::disconnect(&dir, &server).unwrap();
    let e = q.transcribe(&request(0.5), &Arc::new(Progress::default())).unwrap_err();
    assert!(e.contains("isn't connected any more"), "{e}");
    // Nothing listening there: one sentence on what happened.
    let dir = temp();
    let gone = {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        format!("http://{}", l.local_addr().unwrap())
    };
    crate::quip::connect(&dir, &gone, "tok3n").unwrap();
    let ctx = Context { quip: checked(&dir), data_dir: Some(dir), ..Context::default() };
    let e = Quip::open(&ctx).err().unwrap();
    assert!(e.contains("couldn't be asked whether it transcribes: couldn't reach"), "{e}");
}

#[test]
fn cancel_stops_within_a_tick_and_deletes_the_job() {
    let (server, log) = mock(move |r| match (r.method.as_str(), r.path.split('?').next().unwrap_or("")) {
        ("GET", "/api/config") => answer("200 OK", config(64 << 20, 300.0)),
        ("POST", PATH) => answer("200 OK", json!({"job_id": "j1"})),
        ("GET", "/api/jobs/j1") => answer("200 OK", json!({"status": "running", "progress": "0:01 of 0:40 transcribed"})),
        ("DELETE", "/api/jobs/j1") => answer("200 OK", json!({"cancelling": true})),
        _ => answer("404 Not Found", json!({})),
    });
    let (q, _dir) = engine(&server);
    let progress = Arc::new(Progress::default());
    let p = progress.clone();
    let worker = std::thread::spawn(move || q.transcribe(&request(0.5), &p));
    let deadline = Instant::now() + Duration::from_secs(5);
    while count(&log, "GET", "/api/jobs/j1") == 0 {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    let t0 = Instant::now();
    progress.cancel();
    assert_eq!(worker.join().unwrap().unwrap_err(), CANCELLED);
    assert!(t0.elapsed() < Duration::from_millis(150), "stopped in {:?}", t0.elapsed());
    assert!(arrives(&log, "DELETE", "/api/jobs/j1"), "the job is cancelled on the server");
}

#[test]
fn cancel_during_the_upload_needn_t_wait_for_it() {
    let (server, log) = mock(move |r| match (r.method.as_str(), r.path.split('?').next().unwrap_or("")) {
        ("GET", "/api/config") => answer("200 OK", config(64 << 20, 300.0)),
        ("POST", PATH) => {
            std::thread::sleep(Duration::from_millis(600));
            answer("200 OK", json!({"job_id": "j9"}))
        }
        ("DELETE", "/api/jobs/j9") => answer("200 OK", json!({"cancelled": true})),
        _ => answer("404 Not Found", json!({})),
    });
    let (q, _dir) = engine(&server);
    let progress = Arc::new(Progress::default());
    let p = progress.clone();
    let worker = std::thread::spawn(move || q.transcribe(&request(0.5), &p));
    assert!(arrives(&log, "POST", PATH));
    let t0 = Instant::now();
    progress.cancel();
    assert_eq!(worker.join().unwrap().unwrap_err(), CANCELLED);
    assert!(t0.elapsed() < Duration::from_millis(150), "stopped in {:?}", t0.elapsed());
    assert!(arrives(&log, "DELETE", "/api/jobs/j9"), "the job the upload started is cancelled once it's known");
    assert_eq!(count(&log, "GET", "/api/jobs/j9"), 0);
}

#[test]
fn the_token_never_follows_a_redirect_to_another_host() {
    let (elsewhere, theirs) = mock(|_| answer("200 OK", json!({"job_id": "stolen"})));
    let other_host = elsewhere.replace("127.0.0.1", "localhost");
    let (server, ours) = mock(move |r| match (r.method.as_str(), r.path.split('?').next().unwrap_or("")) {
        ("GET", "/api/config") => answer("200 OK", config(64 << 20, 300.0)),
        ("POST", PATH) => response("307 Temporary Redirect", &format!("Location: {other_host}/api/transcribe\r\n"), b""),
        _ => answer("404 Not Found", json!({})),
    });
    let (q, _dir) = engine(&server);
    let e = q.transcribe(&request(0.5), &Arc::new(Progress::default())).unwrap_err();
    assert!(e.contains("another host; it wasn't followed"), "{e}");
    assert_eq!(theirs.lock().unwrap().len(), 0, "nothing reached the other host");
    assert_eq!(count(&ours, "POST", PATH), 1);

    // A 302 would turn the upload into a GET: not followed. A 307 on the same server is,
    // body and all.
    let (server, log) = mock(move |r| match (r.method.as_str(), r.path.split('?').next().unwrap_or("")) {
        ("GET", "/api/config") => answer("200 OK", config(64 << 20, 300.0)),
        ("POST", "/api/transcribe") => response("302 Found", "Location: /elsewhere\r\n", b""),
        _ => answer("404 Not Found", json!({})),
    });
    let (q, _dir) = engine(&server);
    assert!(q.transcribe(&request(0.5), &Arc::new(Progress::default())).unwrap_err().contains("HTTP 302 (a redirect); the request wasn't sent again"));
    assert_eq!(count(&log, "GET", "/elsewhere"), 0);
    let (server, log) = mock(move |r| match (r.method.as_str(), r.path.split('?').next().unwrap_or("")) {
        ("GET", "/api/config") => answer("200 OK", config(64 << 20, 300.0)),
        ("POST", "/api/transcribe") => response("308 Permanent Redirect", "Location: /qc/api/transcribe\r\n", b""),
        ("POST", "/qc/api/transcribe") => answer("200 OK", json!({"job_id": "j1"})),
        ("GET", "/api/jobs/j1") => answer("200 OK", json!({"status": "done", "result": cues_with_words()})),
        _ => answer("404 Not Found", json!({})),
    });
    let (q, _dir) = engine(&server);
    let req = request(0.5);
    q.transcribe(&req, &Arc::new(Progress::default())).unwrap();
    let moved = log.lock().unwrap().iter().find(|r| r.path == "/qc/api/transcribe").cloned().unwrap();
    assert_eq!((moved.body.len(), moved.header("x-api-key")), (req.wav.len(), Some("tok3n")));
}

#[test]
fn too_big_or_too_long_is_refused_before_sending() {
    let (server, log) = mock(move |r| match r.path.as_str() {
        "/api/config" => answer("200 OK", config(10_000, 1.0)),
        _ => answer("200 OK", json!({"job_id": "j1"})),
    });
    let (q, _dir) = engine(&server);
    let e = q.transcribe(&request(0.5), &Arc::new(Progress::default())).unwrap_err();
    assert!(e.contains("its sound is 16 KB to send, more than the 10 KB the QuipClipper at Booth"), "{e}");
    let mut q = q;
    q.caps.max_bytes = None;
    let e = q.transcribe(&request(2.0), &Arc::new(Progress::default())).unwrap_err();
    assert_eq!(e, format!("it's 0:02 long, longer than the 0:01 the QuipClipper at Booth ({}) takes; trim the tile and try again", crate::quip::host_of(&server)));
    assert_eq!(count(&log, "POST", PATH), 0, "nothing was sent");
}

#[test]
fn whether_a_server_transcribes_is_asked_once_and_kept() {
    let (server, log) = quip_server(cues_with_words(), 0);
    let (off, off_log) = mock(|r| match r.path.as_str() {
        "/api/config" => answer("200 OK", json!({"server_name": "Old", "transcribe": {"upload": false}})),
        _ => answer("404 Not Found", json!({})),
    });
    let dir = temp();
    crate::quip::connect(&dir, &server, "tok3n").unwrap();
    // Without asking: known, not asked, and no request made, however often it's looked at.
    for _ in 0..50 {
        let o = offers(&dir, false);
        assert_eq!((o.len(), &o[0].state), (1, &State::Unchecked));
    }
    assert_eq!(log.lock().unwrap().len(), 0);
    // The window asks in the background, once.
    let first = offers(&dir, true);
    assert!(matches!(first[0].state, State::Checking | State::Upload(_)), "{first:?}");
    let deadline = Instant::now() + Duration::from_secs(5);
    while !matches!(offers(&dir, true)[0].state, State::Upload(_)) {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    for _ in 0..50 {
        offers(&dir, true);
    }
    assert_eq!(count(&log, "GET", "/api/config"), 1, "asked once");
    assert_eq!(offers(&dir, false)[0].name.as_deref(), Some("Booth"));
    // Connecting another server lists it and asks only it.
    crate::quip::connect(&dir, &off, "other").unwrap();
    let now = checked(&dir);
    assert_eq!(now.len(), 2);
    let off_offer = now.iter().find(|o| o.server == off).unwrap();
    assert_eq!(off_offer.state, State::Off);
    assert!(off_offer.reason().unwrap().contains("Old ("), "named by its answer: {:?}", off_offer.reason());
    assert_eq!((count(&log, "GET", "/api/config"), count(&off_log, "GET", "/api/config")), (1, 1));
    // Settings opening asks every one again; a failed call asks that one again.
    refresh(&dir);
    checked(&dir);
    assert_eq!((count(&log, "GET", "/api/config"), count(&off_log, "GET", "/api/config")), (2, 2));
    stale(&server);
    checked(&dir);
    assert_eq!((count(&log, "GET", "/api/config"), count(&off_log, "GET", "/api/config")), (3, 2));
    // Disconnecting takes it off the list.
    crate::quip::disconnect(&dir, &off).unwrap();
    assert_eq!(offers(&dir, false).len(), 1);
}

// ── through the commands ────────────────────────────────────────────────────

/// A controller with a spoken tile "Talk" (synthesized, no subtitles), connected to
/// `server`.
fn talk(server: &str) -> (Controller, Vec<f64>) {
    let (mut c, root) = crate::quip::tests::controller();
    let (mono, starts) = crate::demo::synth::speech_with_words("1 1 1 | 1 1", 130.0, 0.4, 3.0, 7);
    let wav = root.join("Talk.wav");
    crate::wav::write_mono(&wav, crate::demo::synth::RATE, &mono).unwrap();
    c.execute("tile.add", json!({"paths": [wav]})).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while (c.store.is_loading() || c.store.library.sets[0].tiles.iter().any(|t| c.store.import_of(t).is_some())) && Instant::now() < deadline {
        c.store.poll();
        std::thread::sleep(Duration::from_millis(10));
    }
    c.tick(0.0);
    c.execute("quip.connect", json!({"url": server, "token": "tok3n"})).unwrap();
    (c, starts)
}

fn wait(c: &mut Controller, job: &Value) -> Value {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let s = c.execute("transcribe.status", json!({"job": job["job"]})).unwrap()["jobs"][0].clone();
        if s["finished"] == json!(true) {
            return s;
        }
        assert!(Instant::now() < deadline, "the job didn't finish: {s}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn commands_transcribe_on_the_server_by_name_and_by_auto() {
    // Lines without words, at the tile's speech: the shared steps split them evenly and
    // move each word onto its sound.
    let (_, starts) = crate::demo::synth::speech_with_words("1 1 1 | 1 1", 130.0, 0.4, 3.0, 7);
    let result = json!({
        "cues": [{"start": starts[0], "end": starts[2] + 0.3, "text": "we have a", "machine": true},
                 {"start": starts[3], "end": starts[4] + 0.3, "text": "show tonight", "machine": true}],
        "cues_source": {"kind": "machine", "engine": "whisper.cpp", "model": "small.en", "lang": "en"},
    });
    let (server, log) = quip_server(result, 1);
    let (mut c, _) = talk(&server);
    let host = crate::quip::host_of(&server);

    let engines = c.execute("transcribe.engines", json!({})).unwrap();
    let quip = engines["engines"].as_array().unwrap().iter().find(|e| e["engine"] == "quip").unwrap().clone();
    assert_eq!(quip["available"], json!(true), "{engines}");
    assert_eq!(quip["servers"][0]["upload"], json!({"maxBytes": 64 << 20, "maxSeconds": 300.0}));
    assert_eq!(quip["servers"][0]["why"], "the only connected server that transcribes");

    let job = c.execute("tile.transcribe", json!({"tile": "Talk", "engine": "quip"})).unwrap();
    assert_eq!((job["engine"].as_str(), job["sendsTo"].as_str()), (Some("quip"), Some(format!("QuipClipper at Booth ({host})").as_str())));
    assert!(c.notice.as_ref().unwrap().text.contains("sending the audio to QuipClipper at Booth"), "{}", c.notice.as_ref().unwrap().text);
    let done = wait(&mut c, &job);
    assert_eq!(done["errors"], json!([]), "{done}");
    let r = &done["results"][0];
    assert_eq!((r["engine"].as_str(), r["model"].as_str(), r["lines"].as_u64()), (Some("quip"), Some("small.en"), Some(2)), "{done}");
    let s = c.execute("tile.subtitles", json!({"tile": "Talk"})).unwrap();
    assert_eq!(s["cues_source"], json!({"kind": "machine", "lang": "en", "engine": "quipclipper", "model": "small.en"}));
    let words: Vec<f64> = s["cues"].as_array().unwrap().iter().flat_map(|l| l["words"].as_array().unwrap().iter().map(|w| w["start"].as_f64().unwrap())).collect();
    assert_eq!(words.len(), 5, "an even split gave each line its words: {s}");
    assert!(r["refined"]["words"].as_u64() == Some(5), "{r}");
    let found = c.execute("tile.search", json!({"query": "tonight"})).unwrap();
    assert_eq!(found.as_array().unwrap().len(), 1, "{found}");
    // The tile's audio, and only it, went to the server.
    let post = log.lock().unwrap().iter().find(|r| r.method == "POST").cloned().unwrap();
    assert_eq!(&post.body[..4], b"RIFF");

    // auto, with no whisper model: QuipClipper is the engine that can run.
    c.execute("edit.undo", json!({})).unwrap();
    c.execute("settings.transcribe", json!({"engine": "auto"})).unwrap();
    let engines = c.execute("transcribe.engines", json!({})).unwrap();
    if engines["engines"].as_array().unwrap().iter().any(|e| e["engine"] != "quip" && e["available"] == json!(true)) {
        return; // A real engine on this machine comes first in auto's order.
    }
    assert_eq!(engines["uses"], "quip", "{engines}");
    let job = c.execute("tile.transcribe", json!({"tile": "Talk"})).unwrap();
    assert_eq!(job["engine"], "quip");
    assert_eq!(wait(&mut c, &job)["results"][0]["lines"], 2);
}

#[test]
fn settings_choose_among_servers_and_refuse_strangers() {
    let (a, _) = quip_server(cues_with_words(), 0);
    let (b, _) = quip_server(cues_with_words(), 0);
    let (mut c, _) = talk(&a);
    c.execute("quip.connect", json!({"url": b, "token": "tok3n"})).unwrap();
    let first = if a < b { &a } else { &b };
    let other = if a < b { &b } else { &a };
    let engines = c.execute("transcribe.engines", json!({})).unwrap();
    assert_eq!(engines["sendsTo"], format!("QuipClipper at Booth ({})", crate::quip::host_of(first)), "{engines}");
    let v = c.execute("settings.transcribe", json!({"quipServer": other})).unwrap();
    assert_eq!(v["quipServer"], json!(other));
    let engines = c.execute("transcribe.engines", json!({})).unwrap();
    let servers = engines["engines"].as_array().unwrap().iter().find(|e| e["engine"] == "quip").unwrap()["servers"].clone();
    let used: Vec<&Value> = servers.as_array().unwrap().iter().filter(|s| s["used"] == json!(true)).collect();
    assert_eq!((used.len(), used[0]["server"].as_str(), used[0]["why"].as_str()), (1, Some(other.as_str()), Some("chosen in Settings")));
    let e = c.execute("settings.transcribe", json!({"quipServer": "https://stranger.example"})).unwrap_err();
    assert!(e.message.contains("isn't a connected QuipClipper"), "{}", e.message);
    c.execute("settings.transcribe", json!({"quipServer": ""})).unwrap();
    assert_eq!(c.store.library.transcribe.quip_server, None);
}
