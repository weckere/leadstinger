//! Transcription on a connected QuipClipper (D-290 to D-293): the tile's audio is sent to
//! the server, only when the user asks, and the lines it hears come back with their word
//! timings. QuipClipper keeps nothing it was sent.
//!
//! The endpoint, as QuipClipper documents it (its docs/API.md, "Transcribing audio an app
//! sends"): `POST /api/transcribe?lang=` with the audio as the raw request body answers
//! `{job_id}`; `GET /api/jobs/{id}` gives `status`, `progress` ("0:12 of 0:40
//! transcribed") and, when done, `result: {cues, cues_source}`; `DELETE /api/jobs/{id}`
//! cancels it. `/api/config`'s `transcribe.upload` is `{max_bytes, max_seconds}` on a
//! server that takes uploads, else `false` or missing.
//!
//! Whether each connected server takes uploads is asked once and kept here
//! ([`offers`]), so Settings can show it every frame without a request: it's asked again
//! when Settings opens ([`refresh`]), when a server is connected or disconnected
//! ([`changed`]), and after a call to it failed ([`stale`]). Every URL is built from the
//! connected server's own base URL and its token goes nowhere else (D-098).

use super::{list_detail, percent_encode, send, Timeouts};
use crate::transcribe::{Context, Engine, EngineKind, Heard, Request, CANCELLED};
use crate::transcode::Progress;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The path the audio is sent to.
pub const PATH: &str = "/api/transcribe";
/// The content type it's sent as (what [`crate::transcribe::prepare`] makes).
const CONTENT_TYPE: &str = "audio/wav";
/// How often a running job is asked how far it is.
const POLL: Duration = Duration::from_millis(500);
/// How often Cancel is looked for.
const TICK: Duration = Duration::from_millis(25);
/// Polls in a row that may fail to reach the server before the job counts as lost.
const LOST_AFTER: u32 = 6;
/// Asking whether a server transcribes: quick, since a command may wait for it.
const PROBE: Timeouts = Timeouts { connect: 4, response: 6, body: 6 };
/// The upload: the server decodes the audio before it answers.
const UPLOAD: Timeouts = Timeouts { connect: 10, response: 120, body: 300 };
/// A job's status.
const STATUS: Timeouts = Timeouts { connect: 10, response: 30, body: 60 };
/// The largest answer read (a finished job carries every line and word).
const MAX_ANSWER: u64 = 8 << 20;
/// How long a command waits for a check already under way.
const WAIT_CHECK: Duration = Duration::from_secs(8);

// ── what each server offers ─────────────────────────────────────────────────

/// What a server takes, from its `transcribe.upload`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Caps {
    pub max_bytes: Option<u64>,
    pub max_seconds: Option<f64>,
}

/// Whether a connected server transcribes uploads, as last asked.
#[derive(Clone, Debug, PartialEq)]
pub enum State {
    /// Not asked yet (or asked again soon).
    Unchecked,
    /// Being asked.
    Checking,
    /// It takes uploads.
    Upload(Caps),
    /// It answered, and doesn't take uploads (`transcribe.upload` isn't an object).
    Off,
    /// It couldn't be asked: why, as a phrase that follows "the QuipClipper at X".
    Failed(String),
}

/// A connected server and whether it transcribes.
#[derive(Clone, Debug, PartialEq)]
pub struct Offer {
    pub server: String,
    /// Its own name (`/api/config`'s `server_name`), once it answered.
    pub name: Option<String>,
    pub state: State,
}

impl Offer {
    /// How the UI names it: "Name (host)", or the host.
    pub fn label(&self) -> String {
        let host = super::host_of(&self.server);
        match &self.name {
            Some(n) if !n.eq_ignore_ascii_case(&host) => format!("{n} ({host})"),
            _ => host,
        }
    }

    /// Why it can't transcribe, or `None` when it can.
    pub fn reason(&self) -> Option<String> {
        let at = format!("the QuipClipper at {}", self.label());
        match &self.state {
            State::Upload(_) => None,
            State::Unchecked => Some(format!("{at} hasn't been asked yet whether it transcribes (open Settings to ask)")),
            State::Checking => Some(format!("asking {at} whether it transcribes…")),
            State::Off => Some(format!("{at} doesn't transcribe audio sent to it: it needs a QuipClipper that takes uploads (POST /api/transcribe) with transcription switched on")),
            State::Failed(why) => Some(format!("{at} {why}")),
        }
    }
}

struct Probe {
    state: State,
    name: Option<String>,
    generation: u64,
}

#[derive(Default)]
struct Cache {
    /// The connected servers, per data folder, as last read from its servers file.
    lists: BTreeMap<PathBuf, Vec<String>>,
    probes: BTreeMap<String, Probe>,
    generation: u64,
}

static CACHE: Mutex<Cache> = Mutex::new(Cache { lists: BTreeMap::new(), probes: BTreeMap::new(), generation: 0 });

fn cache() -> std::sync::MutexGuard<'static, Cache> {
    CACHE.lock().unwrap_or_else(|e| e.into_inner())
}

impl Cache {
    fn reset(&mut self, server: &str) {
        self.generation += 1;
        let generation = self.generation;
        let name = self.probes.get(server).and_then(|p| p.name.clone());
        self.probes.insert(server.to_owned(), Probe { state: State::Unchecked, name, generation });
    }
}

/// A server was connected or disconnected in `data_dir`: read the list again, and ask it
/// afresh.
pub fn changed(data_dir: &Path, server: &str) {
    let mut c = cache();
    c.lists.remove(data_dir);
    c.probes.remove(server);
}

/// Ask every server connected in `data_dir` again (Settings opened).
pub fn refresh(data_dir: &Path) {
    let mut c = cache();
    let servers = c.lists.remove(data_dir).unwrap_or_default();
    for s in servers {
        if !matches!(c.probes.get(&s).map(|p| &p.state), Some(State::Checking)) {
            c.reset(&s);
        }
    }
}

/// A call to `server` failed: ask it again before it's offered next.
pub fn stale(server: &str) {
    cache().reset(server);
}

/// The servers connected in `data_dir` (in address order) and what each offers, as last
/// asked; no request is made here unless `ask` is set, which starts asking the ones not
/// asked yet, each on a thread of its own (the window: the answer wakes it).
pub fn offers(data_dir: &Path, ask: bool) -> Vec<Offer> {
    let listed = cache().lists.get(data_dir).cloned();
    let servers = match listed {
        Some(s) => s,
        None => {
            let s: Vec<String> = list_detail(data_dir).unwrap_or_default().into_iter().map(|l| l.server).collect();
            cache().lists.insert(data_dir.to_owned(), s.clone());
            s
        }
    };
    let mut c = cache();
    let mut out = Vec::with_capacity(servers.len());
    for server in servers {
        if !c.probes.contains_key(&server) {
            c.reset(&server);
        }
        let p = c.probes.get_mut(&server).expect("just made");
        if ask && p.state == State::Unchecked {
            p.state = State::Checking;
            let (dir, s, generation) = (data_dir.to_owned(), server.clone(), p.generation);
            let spawned = std::thread::Builder::new().name("quip-transcribe-check".into()).spawn(move || {
                let (state, name) = probe(&dir, &s);
                store(&s, generation, state, name);
                super::wake();
            });
            if spawned.is_err() {
                p.state = State::Unchecked;
            }
        }
        out.push(Offer { server, name: p.name.clone(), state: p.state.clone() });
    }
    out
}

fn store(server: &str, generation: u64, state: State, name: Option<String>) {
    let mut c = cache();
    if let Some(p) = c.probes.get_mut(server).filter(|p| p.generation == generation) {
        p.state = state;
        if name.is_some() {
            p.name = name;
        }
    }
}

/// For a command: ask every server not asked yet, now, and wait for those being asked.
/// Returns what they offer.
pub fn checked(data_dir: &Path) -> Vec<Offer> {
    let now = offers(data_dir, false);
    for o in now.iter().filter(|o| o.state == State::Unchecked) {
        let generation = {
            let mut c = cache();
            let p = c.probes.get_mut(&o.server).expect("listed");
            p.state = State::Checking;
            p.generation
        };
        let (state, name) = probe(data_dir, &o.server);
        store(&o.server, generation, state, name);
    }
    let deadline = Instant::now() + WAIT_CHECK;
    loop {
        let now = offers(data_dir, false);
        if Instant::now() > deadline || !now.iter().any(|o| o.state == State::Checking) {
            return now;
        }
        std::thread::sleep(TICK);
    }
}

/// Ask `server` (with its token) whether it takes uploads, and its name.
fn probe(data_dir: &Path, server: &str) -> (State, Option<String>) {
    let token = match super::connection(data_dir, server) {
        Ok(Some(c)) => c.token.unwrap_or_default(),
        Ok(None) => return (State::Failed("isn't connected".into()), None),
        Err(e) => return (State::Failed(format!("couldn't be looked up: {e}")), None),
    };
    match send("GET", &format!("{server}/api/config"), &token, None, PROBE, 1 << 20) {
        Ok((200, v)) => {
            let name = v["server_name"].as_str().map(str::trim).filter(|s| !s.is_empty() && s.len() <= 100 && !s.chars().any(char::is_control)).map(str::to_owned);
            (upload_state(&v), name)
        }
        Ok((401 | 403, _)) => (State::Failed(REPAIR.into()), None),
        Ok((404, _)) => (State::Failed("doesn't answer like QuipClipper (no /api/config)".into()), None),
        Ok((s, _)) => (State::Failed(format!("answered HTTP {s} when asked whether it transcribes")), None),
        Err(e) => (State::Failed(format!("couldn't be asked whether it transcribes: {e}")), None),
    }
}

/// What a health or config answer says about uploads.
pub fn upload_state(v: &Value) -> State {
    match v.get("transcribe").and_then(|t| t.get("upload")) {
        Some(Value::Object(o)) => {
            let max_bytes = o.get("max_bytes").and_then(Value::as_u64);
            let max_seconds = o.get("max_seconds").and_then(Value::as_f64).filter(|s| s.is_finite());
            if max_bytes == Some(0) || max_seconds.is_some_and(|s| s <= 0.0) {
                State::Off
            } else {
                State::Upload(Caps { max_bytes, max_seconds })
            }
        }
        _ => State::Off,
    }
}

/// What follows "the QuipClipper at X" when it refuses this app's token.
const REPAIR: &str = "no longer accepts this app's pairing: connect it again in Settings ▸ QuipClipper";

/// The server a transcription goes to: the one chosen in Settings when it's connected
/// (and then only if it transcribes), else the first, in address order, that transcribes
/// (D-292). The error says why none can.
pub fn choose<'a>(offers: &'a [Offer], wanted: Option<&str>) -> Result<&'a Offer, String> {
    if offers.is_empty() {
        return Err("no QuipClipper server is connected: connect one in Settings ▸ QuipClipper".into());
    }
    if let Some(o) = wanted.and_then(|w| offers.iter().find(|o| o.server == w)) {
        return match o.reason() {
            None => Ok(o),
            Some(why) => Err(why),
        };
    }
    if let Some(o) = offers.iter().find(|o| o.reason().is_none()) {
        return Ok(o);
    }
    let first = offers[0].reason().unwrap_or_default();
    Err(match offers.len() {
        1 => first,
        n => format!("none of the {n} connected QuipClipper servers transcribes: {first}"),
    })
}

/// Why `chosen` was picked, for Settings and `transcribe.engines`.
pub fn why_chosen(offers: &[Offer], chosen: &Offer, wanted: Option<&str>) -> &'static str {
    let able = offers.iter().filter(|o| o.reason().is_none()).count();
    if wanted == Some(chosen.server.as_str()) {
        "chosen in Settings"
    } else if able <= 1 {
        "the only connected server that transcribes"
    } else {
        "the first, by address, of the connected servers that transcribe"
    }
}

// ── the engine ──────────────────────────────────────────────────────────────

/// A transcription on one connected server.
pub struct Quip {
    server: String,
    label: String,
    caps: Caps,
    data_dir: PathBuf,
    poll: Duration,
}

impl Quip {
    /// The server [`choose`] picks from what's known, without a request.
    pub fn open(ctx: &Context) -> Result<Quip, String> {
        let o = choose(&ctx.quip, ctx.quip_server.as_deref())?;
        let State::Upload(caps) = o.state.clone() else { unreachable!("chosen servers transcribe") };
        let data_dir = ctx.data_dir.clone().ok_or("no library folder to find the server's token in")?;
        Ok(Quip { server: o.server.clone(), label: o.label(), caps, data_dir, poll: POLL })
    }

    fn at(&self) -> String {
        format!("the QuipClipper at {}", self.label)
    }

    /// A call failed: say so, and ask the server again before offering it next.
    fn failed(&self, why: String) -> String {
        stale(&self.server);
        why
    }

    fn unexpected(&self, what: &str) -> String {
        self.failed(format!("{} answered {what}, which this version doesn't understand", self.at()))
    }

    /// Stop the job on the server, on a thread of its own, so Cancel needn't wait.
    fn cancel_job(&self, job: String, token: String) {
        let url = format!("{}/api/jobs/{}", self.server, percent_encode(&job));
        std::thread::Builder::new().name("quip-transcribe-cancel".into()).spawn(move || send("DELETE", &url, &token, None, STATUS, 64 << 10)).ok();
    }

    /// A refused upload, as one sentence.
    fn refused(&self, status: u16, req: &Request) -> String {
        let at = self.at();
        match status {
            413 => {
                let limits = match (self.caps.max_bytes, self.caps.max_seconds) {
                    (Some(b), Some(s)) => format!(" (it takes up to {} or {})", megabytes(b), clock(s)),
                    (Some(b), None) => format!(" (it takes up to {})", megabytes(b)),
                    (None, Some(s)) => format!(" (it takes up to {})", clock(s)),
                    (None, None) => String::new(),
                };
                self.failed(format!("{at} refused it as too long or too large{limits}; trim the tile and try again"))
            }
            400 => format!("{at} couldn't read the audio it was sent"),
            422 => format!("{at} doesn't hear the language {}: change it in Settings ▸ Subtitles", req.lang.as_deref().unwrap_or("asked for")),
            429 => format!("{at} is busy with other uploads; try again when they're done"),
            503 => self.failed(format!("{at} has transcription switched off or not set up")),
            401 | 403 => self.failed(format!("{at} {REPAIR}")),
            s => self.unexpected(&format!("HTTP {s}")),
        }
    }
}

impl Engine for Quip {
    fn kind(&self) -> EngineKind {
        EngineKind::Quip
    }

    fn destination(&self) -> Option<String> {
        Some(format!("QuipClipper at {}", self.label))
    }

    fn transcribe(&self, req: &Request, progress: &Arc<Progress>) -> Result<Heard, String> {
        let at = self.at();
        let token = match super::connection(&self.data_dir, &self.server) {
            Ok(Some(c)) => c.token.unwrap_or_default(),
            Ok(None) => return Err(self.failed(format!("{at} isn't connected any more: connect it again in Settings ▸ QuipClipper"))),
            Err(e) => return Err(e),
        };
        if let Some(max) = self.caps.max_bytes.filter(|&m| req.wav.len() as u64 > m) {
            return Err(format!("its sound is {} to send, more than the {} {at} takes; trim the tile and try again", megabytes(req.wav.len() as u64), megabytes(max)));
        }
        if let Some(max) = self.caps.max_seconds.filter(|&m| req.seconds > m + 0.01) {
            return Err(format!("it's {} long, longer than the {} {at} takes; trim the tile and try again", clock(req.seconds), clock(max)));
        }
        let mut url = format!("{}{PATH}", self.server);
        if let Some(lang) = &req.lang {
            url.push_str(&format!("?lang={}", percent_encode(lang)));
        }

        // The upload runs on a thread of its own, so Cancel needn't wait for it; a job it
        // starts after Cancel is stopped there.
        let abandoned = Arc::new(AtomicBool::new(false));
        let (tx, rx) = std::sync::mpsc::channel();
        {
            let (wav, token, abandoned, server) = (req.wav.clone(), token.clone(), abandoned.clone(), self.server.clone());
            std::thread::Builder::new()
                .name("quip-transcribe-upload".into())
                .spawn(move || {
                    let answer = send("POST", &url, &token, Some((CONTENT_TYPE, &wav)), UPLOAD, 1 << 20);
                    if abandoned.load(Ordering::SeqCst) {
                        if let Some(job) = answer.as_ref().ok().and_then(|(_, v)| v["job_id"].as_str()) {
                            let url = format!("{server}/api/jobs/{}", percent_encode(job));
                            send("DELETE", &url, &token, None, STATUS, 64 << 10).ok();
                        }
                    }
                    tx.send(answer).ok();
                })
                .map_err(|e| format!("couldn't start sending it: {e}"))?;
        }
        let answer = loop {
            match rx.recv_timeout(TICK) {
                Ok(a) => break a,
                Err(RecvTimeoutError::Timeout) if progress.is_cancelled() => {
                    abandoned.store(true, Ordering::SeqCst);
                    // It may have answered meanwhile: then its job is stopped here.
                    if let Ok(Ok((_, v))) = rx.try_recv() {
                        if let Some(job) = v["job_id"].as_str() {
                            self.cancel_job(job.to_owned(), token);
                        }
                    }
                    return Err(CANCELLED.to_owned());
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return Err(format!("lost the connection to {at} while sending it")),
            }
        };
        let job = match answer {
            Err(e) => return Err(self.failed(format!("couldn't send it to {at}: {e}"))),
            Ok((200..=202, v)) => match v["job_id"].as_str().filter(|j| !j.is_empty() && j.len() <= 200) {
                Some(j) => j.to_owned(),
                None => return Err(self.unexpected("the upload without a job id")),
            },
            Ok((s, _)) => return Err(self.refused(s, req)),
        };

        // Follow the job until it ends.
        let job_url = format!("{}/api/jobs/{}", self.server, percent_encode(&job));
        let mut next = Instant::now() + TICK * 4;
        let mut misses = 0;
        loop {
            if progress.is_cancelled() {
                self.cancel_job(job, token);
                return Err(CANCELLED.to_owned());
            }
            if Instant::now() < next {
                std::thread::sleep(TICK);
                continue;
            }
            next = Instant::now() + self.poll;
            let (status, v) = match send("GET", &job_url, &token, None, STATUS, MAX_ANSWER) {
                Ok(a) => {
                    misses = 0;
                    a
                }
                Err(_) if misses + 1 < LOST_AFTER => {
                    misses += 1;
                    continue;
                }
                Err(_) => return Err(self.failed(format!("lost the connection to {at}; try again when it's back"))),
            };
            match (status, v["status"].as_str()) {
                (200, Some("queued")) => progress.set_fraction(None),
                (200, Some("running")) => progress.set_fraction(fraction(&v["progress"])),
                (200, Some("done")) => return self.heard(&v["result"]),
                (200, Some("failed")) => {
                    let why = v["error"].as_str().map(|e| e.lines().next().unwrap_or("").trim().chars().take(200).collect::<String>()).filter(|e| !e.is_empty());
                    return Err(match why {
                        Some(e) => format!("{at} couldn't transcribe it: {e}"),
                        None => format!("{at} couldn't transcribe it"),
                    });
                }
                (200, Some("cancelled")) => return Err(format!("{at} stopped the transcription")),
                (200, _) => return Err(self.unexpected("a job status")),
                (404, _) => return Err(self.failed(format!("{at} no longer has the transcription (it may have restarted); try again"))),
                (401 | 403, _) => return Err(self.failed(format!("{at} {REPAIR}"))),
                (s, _) => return Err(self.unexpected(&format!("HTTP {s} about the transcription"))),
            }
        }
    }
}

impl Quip {
    /// A finished job's `result`: its `cues` as lines (words where it gave them), never
    /// marked as placed by hand; the model and language from `cues_source`.
    fn heard(&self, result: &Value) -> Result<Heard, String> {
        let Some(cues) = result.get("cues").filter(|c| c.is_array()) else {
            return Err(self.unexpected("a finished transcription without its lines"));
        };
        let mut lines = crate::subtitles::parse_lines(cues).lines;
        for l in &mut lines {
            l.aligned = false;
            l.source_timing = None;
        }
        let source = &result["cues_source"];
        let short = |k: &str| source[k].as_str().map(str::trim).filter(|s| !s.is_empty() && s.len() <= 64).map(str::to_owned);
        Ok(Heard { lines, model: short("model"), lang: short("lang") })
    }
}

/// How far a job is, from its `progress`: "0:12 of 0:40 transcribed", or a number (a
/// fraction, or a percentage over 1).
pub fn fraction(v: &Value) -> Option<f64> {
    if let Some(n) = v.as_f64() {
        let f = if n > 1.0 { n / 100.0 } else { n };
        return f.is_finite().then(|| f.clamp(0.0, 1.0));
    }
    let text = v.as_str()?;
    let mut parts = text.split(" of ");
    let done = seconds(parts.next()?.split_whitespace().last()?)?;
    let total = seconds(parts.next()?.split_whitespace().next()?)?;
    (total > 0.0).then(|| (done / total).clamp(0.0, 1.0))
}

/// "1:02:03", "0:12" or "12" as seconds.
fn seconds(s: &str) -> Option<f64> {
    s.split(':').try_fold(0.0, |acc, part| part.parse::<f64>().ok().filter(|p| p.is_finite() && *p >= 0.0).map(|p| acc * 60.0 + p))
}

fn megabytes(b: u64) -> String {
    let mb = b as f64 / (1024.0 * 1024.0);
    if b < 1 << 20 {
        format!("{} KB", b.div_ceil(1024))
    } else if mb >= 10.0 {
        format!("{mb:.0} MB")
    } else {
        format!("{mb:.1} MB")
    }
}

fn clock(s: f64) -> String {
    let s = s.round() as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

#[cfg(test)]
pub(crate) mod tests;
