//! A connected QuipClipper's clips library (M21, D-098): its listing (contract
//! `quipclipper.clips/1`), the clips added since last time, previews played from memory,
//! and adding a clip, with its metadata, as a tile (or a set file as a set).
//!
//! Every URL is built here from the connected server's base URL and the clip's path. The
//! listing's own `download_url`/`stream_url` may point at a front proxy on another host,
//! and the token goes nowhere but the server the user connected. Listings are cached by
//! URL with their ETag, so asking again costs the server a `304`.

use super::{encode_clip, err, get_with, list as connected, status_error, token_for, CmdError, Controller, Fetch, Link};
use crate::decode::DecodedAudio;
use crate::model::Tile;
use serde::Deserialize;
use serde_json::{json, Value};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use uuid::Uuid;

/// The contract this client speaks. Within a version fields are only added.
pub const CONTRACT: &str = "quipclipper.clips/1";
/// The voice a preview plays as: not a tile in the library.
pub const PREVIEW_ID: Uuid = Uuid::from_u128(0x5155_4950_7072_4576_8000_0000_0000_0001);
/// The largest listing read.
const MAX_LISTING: u64 = 32 << 20;
/// The largest clip previewed (it's held in memory while it plays).
const MAX_PREVIEW: u64 = 256 << 20;
/// Listings kept with their ETags.
const CACHE_SIZE: usize = 32;
/// How long a cached listing is trusted without asking the server, to find a clip the
/// window just showed (`quip.add`).
const FRESH: Duration = Duration::from_secs(120);
/// Clips the decoder reads as they are; anything else previews as the server's FLAC.
const STREAMABLE: &[&str] = &["wav", "wave", "flac", "mp3", "ogg", "oga", "aif", "aiff", "aifc", "m4a", "caf", "mka"];

// ── the listing ───────────────────────────────────────────────────────────────

/// What a library entry is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipKind {
    Audio,
    Video,
    Gif,
    Set,
}

impl ClipKind {
    pub fn parse(s: &str) -> Option<ClipKind> {
        match s.trim().to_ascii_lowercase().as_str() {
            "audio" => Some(ClipKind::Audio),
            "video" => Some(ClipKind::Video),
            "gif" => Some(ClipKind::Gif),
            "set" | "sets" => Some(ClipKind::Set),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            ClipKind::Audio => "audio",
            ClipKind::Video => "video",
            ClipKind::Gif => "gif",
            ClipKind::Set => "set",
        }
    }

    /// Whether it has sound to preview or add as a tile.
    pub fn has_sound(self) -> bool {
        matches!(self, ClipKind::Audio | ClipKind::Video)
    }
}

/// A span in seconds; either end may be unknown.
#[derive(Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Span {
    pub start: Option<f64>,
    pub end: Option<f64>,
}

#[derive(Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Jellyfin {
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub show: Option<String>,
    pub season: Option<u32>,
    pub episode: Option<u32>,
    pub title: Option<String>,
    pub year: Option<u32>,
}

#[derive(Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Source {
    pub name: Option<String>,
    pub stem: Option<String>,
}

#[derive(Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Parsed {
    pub season: Option<u32>,
    pub episode: Option<u32>,
    pub year: Option<u32>,
}

/// The clip's metadata sidecar. Every field may be missing (a backfilled clip).
#[derive(Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Meta {
    pub cue: Option<String>,
    pub trim: Option<Span>,
    pub line: Option<Span>,
    pub clip: Option<Span>,
    pub duration: Option<f64>,
    pub channels: Option<u32>,
    pub codec: Option<String>,
    pub jellyfin: Option<Jellyfin>,
    pub source: Option<Source>,
    pub parsed: Option<Parsed>,
    /// Sets: the set's name and its tiles.
    pub name: Option<String>,
    pub items: Option<Vec<Value>>,
}

/// One entry of the listing.
#[derive(Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Clip {
    pub name: String,
    /// Its path in the clips library.
    pub clip: String,
    pub folder: Option<String>,
    pub kind: String,
    pub title: Option<String>,
    pub size: Option<u64>,
    pub mtime: Option<f64>,
    pub modified: Option<String>,
    pub stream_url: Option<String>,
    pub flac_url: Option<String>,
    pub meta: Option<Meta>,
}

/// A listing of the clips library.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Listing {
    pub folder: String,
    pub recursive: bool,
    pub folders: Vec<String>,
    pub clips: Vec<Clip>,
    pub cursor: Option<String>,
}

/// What to list.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Query {
    /// A folder of the library, `""` for its top.
    pub folder: String,
    pub recursive: bool,
    /// Words every entry must have.
    pub q: String,
    pub kind: Option<ClipKind>,
    /// Only entries added after this cursor.
    pub since: Option<String>,
    pub newest: bool,
}

impl Query {
    pub fn url(&self, server: &str) -> String {
        let mut q = url::form_urlencoded::Serializer::new(String::new());
        if !self.folder.is_empty() {
            q.append_pair("folder", &self.folder);
        }
        if self.recursive {
            q.append_pair("recursive", "1");
        }
        if !self.q.trim().is_empty() {
            q.append_pair("q", self.q.trim());
        }
        if let Some(k) = self.kind {
            q.append_pair("kind", k.as_str());
        }
        if let Some(s) = &self.since {
            q.append_pair("since", s);
        }
        q.append_pair("sort", if self.newest { "newest" } else { "name" });
        format!("{server}/api/clips?{}", q.finish())
    }
}

/// Text from the server for one line of the window: whitespace runs (a subtitle's line
/// breaks) as one space.
fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// At most `max` characters, with an ellipsis when cut.
fn at_most(s: String, max: usize) -> String {
    if s.chars().count() <= max {
        return s;
    }
    let mut out: String = s.chars().take(max - 1).collect();
    out.push('…');
    out
}

impl Clip {
    pub fn kind(&self) -> Option<ClipKind> {
        ClipKind::parse(&self.kind)
    }

    fn meta(&self) -> &Meta {
        static NONE: std::sync::OnceLock<Meta> = std::sync::OnceLock::new();
        self.meta.as_ref().unwrap_or_else(|| NONE.get_or_init(Meta::default))
    }

    /// The line (`meta.cue`), else the server's title, else the file name: on one line,
    /// at most 200 characters (a tile title's limit).
    pub fn display_title(&self) -> String {
        let pick = [self.meta().cue.as_deref(), self.title.as_deref(), Some(self.name.as_str())];
        let t = pick.into_iter().flatten().map(one_line).find(|t| !t.is_empty()).unwrap_or_else(|| "Clip".to_owned());
        at_most(t, 200)
    }

    fn jellyfin(&self) -> Option<&Jellyfin> {
        self.meta().jellyfin.as_ref()
    }

    /// `S01E04`, from Jellyfin or what the server read from the names.
    pub fn episode_code(&self) -> Option<String> {
        let (s, e) = match self.jellyfin().and_then(|j| j.season.zip(j.episode)) {
            Some(se) => se,
            None => self.meta().parsed.as_ref().and_then(|p| p.season.zip(p.episode))?,
        };
        Some(format!("S{s:02}E{e:02}"))
    }

    /// Where the line comes from, for the row's second line and the tile's notes:
    /// "Over the Garden Wall S01E04 · Songs of the Dark Lantern", "Heat (1995)", or the
    /// source's name.
    pub fn source_text(&self) -> Option<String> {
        if self.kind() == Some(ClipKind::Set) {
            let n = self.meta().items.as_ref().map_or(0, Vec::len);
            return Some(if n == 0 { "Set file".to_owned() } else { format!("Set file, {n} tile{}", if n == 1 { "" } else { "s" }) });
        }
        let j = self.jellyfin();
        let episode = self.episode_code();
        let show = j.and_then(|j| j.show.clone()).or_else(|| self.meta().source.as_ref().and_then(|s| s.name.clone())).map(|s| one_line(&s)).filter(|s| !s.is_empty());
        let title = j.and_then(|j| j.title.clone()).map(|t| one_line(&t)).filter(|t| !t.is_empty() && Some(t) != show.as_ref());
        let year = j.and_then(|j| j.year).or_else(|| self.meta().parsed.as_ref().and_then(|p| p.year));
        let mut out = match (show, &episode) {
            (Some(s), Some(e)) => format!("{s} {e}"),
            (Some(s), None) => s,
            (None, Some(e)) => e.clone(),
            (None, None) => return title,
        };
        match title {
            Some(t) if episode.is_some() => out.push_str(&format!(" · {t}")),
            _ => {
                if let Some(y) = year.filter(|y| episode.is_none() && !out.contains(&format!("({y})"))) {
                    out.push_str(&format!(" ({y})"));
                }
            }
        }
        Some(out)
    }

    pub fn duration(&self) -> Option<f64> {
        self.meta().duration.filter(|d| d.is_finite() && *d > 0.0)
    }

    pub fn channels(&self) -> Option<u32> {
        self.meta().channels
    }

    /// "5.1", "7.1", "4 ch" for more than two channels.
    pub fn surround(&self) -> Option<String> {
        match self.channels()? {
            0..=2 => None,
            6 => Some("5.1".into()),
            8 => Some("7.1".into()),
            n => Some(format!("{n} ch")),
        }
    }

    /// Where the line sits in the clip: the tile's trim. Only when both ends are known.
    pub fn trim(&self) -> Option<(f64, f64)> {
        let t = self.meta().trim.as_ref()?;
        let (s, e) = (t.start?, t.end?);
        (s.is_finite() && e.is_finite() && s >= 0.0 && e > s && e <= 86_400.0).then_some((s, e))
    }

    /// The tile's notes: where the line is from, when in the source, and that it came
    /// from QuipClipper.
    pub fn notes(&self) -> String {
        let mut parts: Vec<String> = self.source_text().into_iter().collect();
        let at = [self.meta().line.as_ref(), self.meta().clip.as_ref()].into_iter().flatten().find_map(|s| s.start);
        if let Some(at) = at.filter(|a| a.is_finite() && *a >= 0.0) {
            parts.push(format!("at {}", crate::logic::format_clock(at, false)));
        }
        parts.push("QuipClipper".to_owned());
        parts.join(" · ")
    }

    /// The entry for `quip.browse` and friends: what a script needs, without the
    /// sidecar's every field.
    pub fn summary(&self) -> Value {
        json!({
            "clip": self.clip, "kind": self.kind, "title": self.display_title(), "source": self.source_text(),
            "episode": self.episode_code(), "duration": self.duration(), "channels": self.channels(),
            "trim": self.trim().map(|(s, e)| json!({"start": s, "end": e})), "size": self.size, "modified": self.modified,
            "flac": self.flac_url.is_some(),
        })
    }
}

/// A listing's reply as the contract words it.
fn parse_listing(body: &[u8], server: &str) -> Result<Listing, String> {
    let v: Value = serde_json::from_slice(body).map_err(|_| format!("the QuipClipper at {server} sent something that isn't a clips listing"))?;
    match v.get("contract").and_then(Value::as_str) {
        Some(CONTRACT) => {}
        Some(other) if other.starts_with("quipclipper.clips/") => {
            return Err(format!("the QuipClipper at {server} speaks {other}, which this {} doesn't know (it speaks {CONTRACT}); update {}", crate::brand::APP_NAME, crate::brand::APP_NAME))
        }
        _ => return Err(format!("the QuipClipper at {server} doesn't serve its clips library to apps yet (no {CONTRACT}); update QuipClipper")),
    }
    let text = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_owned);
    let clips = v.get("clips").and_then(Value::as_array).map(|a| a.iter().filter_map(|c| serde_json::from_value::<Clip>(c.clone()).ok()).filter(|c| !c.clip.is_empty()).collect()).unwrap_or_default();
    let folders = v.get("folders").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_owned).collect()).unwrap_or_default();
    Ok(Listing { folder: text("folder").unwrap_or_default(), recursive: v.get("recursive").and_then(Value::as_bool).unwrap_or(false), folders, clips, cursor: text("cursor") })
}

struct Cached {
    url: String,
    etag: String,
    listing: Arc<Listing>,
    at: Instant,
}

static CACHE: Mutex<Vec<Cached>> = Mutex::new(Vec::new());

fn cache() -> std::sync::MutexGuard<'static, Vec<Cached>> {
    CACHE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Why a listing failed: the HTTP status, when there was one, and what to say.
#[derive(Debug)]
pub struct ListError {
    pub status: Option<u16>,
    pub message: String,
}

impl From<ListError> for String {
    fn from(e: ListError) -> String {
        e.message
    }
}

/// List the clips library, revalidating a cached copy with `If-None-Match`.
pub fn list(server: &str, token: &str, query: &Query) -> Result<Arc<Listing>, ListError> {
    let url = query.url(server);
    let etag = cache().iter().find(|c| c.url == url).map(|c| c.etag.clone());
    let mut headers = vec![("Accept", "application/json")];
    if let Some(e) = &etag {
        headers.push(("If-None-Match", e.as_str()));
    }
    let fail = |status, message| ListError { status, message };
    let resp = get_with(&url, token, &headers).map_err(|e| fail(None, e))?;
    let status = resp.status().as_u16();
    if status == 304 {
        let mut all = cache();
        if let Some(c) = all.iter_mut().find(|c| c.url == url) {
            c.at = Instant::now();
            return Ok(c.listing.clone());
        }
        return Err(fail(Some(304), format!("the QuipClipper at {server} answered 304 (not modified) to a listing it never sent")));
    }
    let new_etag = resp.headers().get("etag").and_then(|v| v.to_str().ok()).map(str::to_owned);
    let body = resp.into_body().with_config().limit(MAX_LISTING).read_to_vec();
    if !(200..=299).contains(&status) {
        // FastAPI says why as {"detail": "..."}.
        let detail = body.ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()).and_then(|v| v["detail"].as_str().map(one_line)).filter(|d| !d.is_empty());
        let mut message = status_error(status, server, "clips library");
        if let Some(d) = detail.filter(|_| !matches!(status, 401 | 403 | 404) || status == 403) {
            message.push_str(&format!(": {d}"));
        }
        return Err(fail(Some(status), message));
    }
    let body = body.map_err(|e| fail(Some(status), format!("the clips listing from {server} broke off: {e}")))?;
    let listing = Arc::new(parse_listing(&body, server).map_err(|m| fail(Some(status), m))?);
    let mut all = cache();
    all.retain(|c| c.url != url);
    if let Some(etag) = new_etag {
        all.push(Cached { url, etag, listing: listing.clone(), at: Instant::now() });
        if all.len() > CACHE_SIZE {
            all.remove(0);
        }
    }
    Ok(listing)
}

/// A clip by its path, from a listing fetched in the last two minutes (the window's), or
/// else from its folder's listing.
pub fn find(server: &str, token: &str, clip: &str) -> Result<Clip, String> {
    let prefix = format!("{server}/api/clips?");
    let cached = cache().iter().rev().filter(|c| c.url.starts_with(&prefix) && c.at.elapsed() < FRESH).find_map(|c| c.listing.clips.iter().find(|e| e.clip == clip).cloned());
    if let Some(c) = cached {
        return Ok(c);
    }
    let folder = clip.rsplit_once('/').map_or("", |(f, _)| f).to_owned();
    let listing = list(server, token, &Query { folder, ..Default::default() })?;
    listing.clips.iter().find(|e| e.clip == clip).cloned().ok_or_else(|| format!("the QuipClipper at {server} has no clip {clip:?} in its clips library"))
}

/// Clips added since a server's new clips were last seen.
#[derive(Clone, Debug, Default)]
pub struct NewClips {
    /// The cursor they're counted from (`None`: this was the first look, now the baseline).
    pub since: Option<String>,
    pub listing: Arc<Listing>,
    /// Whether the cursor was moved on (they're seen now).
    pub marked: bool,
}

/// The clips added since the stored cursor, newest first. The first look at a server
/// stores the current cursor and finds nothing new, so connecting doesn't flag the
/// whole library; so does a cursor the server no longer takes. `mark` stores the new
/// cursor: they're seen.
pub fn new_clips(data_dir: &Path, server: &str, token: &str, mark: bool) -> Result<NewClips, String> {
    let since = super::seen_cursor(data_dir, server)?;
    let all = Query { recursive: true, newest: true, ..Default::default() };
    let baseline = |data_dir: &Path| -> Result<NewClips, String> {
        let listing = list(server, token, &all)?;
        if let Some(c) = &listing.cursor {
            super::set_seen(data_dir, server, c)?;
        }
        Ok(NewClips { since: None, listing: Arc::new(Listing { clips: Vec::new(), ..(*listing).clone() }), marked: true })
    };
    let Some(cursor) = since else { return baseline(data_dir) };
    match list(server, token, &Query { since: Some(cursor.clone()), ..all.clone() }) {
        Ok(listing) => {
            let marked = mark && listing.cursor.as_ref().is_some_and(|c| *c != cursor);
            if marked {
                super::set_seen(data_dir, server, listing.cursor.as_deref().unwrap_or_default())?;
            }
            Ok(NewClips { since: Some(cursor), listing, marked })
        }
        Err(e) if e.status == Some(400) => baseline(data_dir),
        Err(e) => Err(e.message),
    }
}

// ── previews and set files ────────────────────────────────────────────────────

fn extension(clip: &str) -> String {
    clip.rsplit('/').next().and_then(|f| f.rsplit_once('.')).map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default()
}

/// A clip's sound, fetched into memory and decoded: never saved. A clip the decoder
/// reads comes from `stream`; anything else (video, AC-3), or a stream that doesn't
/// decode, as the server's FLAC.
pub fn preview_audio(server: &str, token: &str, clip: &str) -> Result<DecodedAudio, String> {
    let ext = extension(clip);
    let path = encode_clip(clip);
    let mut tries = Vec::new();
    if STREAMABLE.contains(&ext.as_str()) {
        tries.push((format!("{server}/api/clips/stream/{path}"), ext.clone()));
    }
    tries.push((format!("{server}{}{path}?as=flac", super::DOWNLOAD_PATH), "flac".to_owned()));
    let mut last = String::new();
    for (url, ext) in tries {
        match fetch_bytes(&url, token, server, clip) {
            Ok(bytes) => match crate::decode::decode_bytes(bytes, Some(&ext)) {
                Ok(audio) => return Ok(audio),
                Err(e) => last = format!("can't play “{clip}”: {e:#}"),
            },
            Err(e) => last = e,
        }
    }
    Err(last)
}

fn fetch_bytes(url: &str, token: &str, server: &str, clip: &str) -> Result<Vec<u8>, String> {
    let what = format!("clip “{clip}”");
    let resp = get_with(url, token, &[])?;
    let status = resp.status().as_u16();
    if !(200..=299).contains(&status) {
        return Err(status_error(status, server, &what));
    }
    let kind = resp.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("").to_ascii_lowercase();
    if kind.starts_with("text/html") || kind.starts_with("application/json") {
        return Err(format!("the QuipClipper at {server} sent a web page instead of the {what}"));
    }
    let mut out = Vec::new();
    resp.into_body().into_reader().take(MAX_PREVIEW + 1).read_to_end(&mut out).map_err(|e| format!("the {what} broke off: {e}"))?;
    if out.len() as u64 > MAX_PREVIEW {
        return Err(format!("the {what} is too long to preview"));
    }
    Ok(out)
}

/// Work under way off the ui thread: a preview's fetch and set files' downloads. The
/// controller polls it every tick.
#[derive(Default)]
pub struct Jobs {
    preview: Option<PreviewJob>,
    sets: Vec<SetJob>,
    /// The clip last previewed, while its voice may still be sounding.
    previewing: Option<String>,
}

struct PreviewJob {
    clip: String,
    title: String,
    rx: Receiver<Result<DecodedAudio, String>>,
}

struct SetJob {
    file: PathBuf,
    rx: Receiver<Result<(), String>>,
}

/// What the browser shows for previews.
#[derive(Clone, Debug, PartialEq)]
pub enum PreviewState {
    Loading(String),
    Playing(String),
}

impl Jobs {
    /// The clip being fetched for a preview, or sounding.
    pub fn preview_state(&self, playing: bool) -> Option<PreviewState> {
        match (&self.preview, &self.previewing) {
            (Some(j), _) => Some(PreviewState::Loading(j.clip.clone())),
            (None, Some(c)) if playing => Some(PreviewState::Playing(c.clone())),
            _ => None,
        }
    }

    /// Set files still downloading.
    pub fn sets_downloading(&self) -> usize {
        self.sets.len()
    }
}

fn spawn<T: Send + 'static>(name: &str, work: impl FnOnce() -> T + Send + 'static) -> Result<Receiver<T>, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name(name.into())
        .spawn(move || {
            tx.send(work()).ok();
            super::wake();
        })
        .map_err(|e| format!("couldn't start the download: {e}"))?;
    Ok(rx)
}

/// Finish what's done: play a fetched preview, import a downloaded set file.
pub fn poll(c: &mut Controller) {
    if c.quip_jobs.previewing.is_some() && c.quip_jobs.preview.is_none() && c.engine.state(PREVIEW_ID) == crate::logic::PlayState::Stopped {
        c.quip_jobs.previewing = None;
    }
    if let Some(job) = &c.quip_jobs.preview {
        let done = match job.rx.try_recv() {
            Ok(r) => Some(r),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err("the preview stopped without an answer".into())),
        };
        if let Some(r) = done {
            let job = c.quip_jobs.preview.take().expect("checked");
            match r {
                Ok(audio) => {
                    let tile = Tile { id: PREVIEW_ID, title: job.title.clone(), ..Default::default() };
                    c.engine.stop(PREVIEW_ID, 0.0);
                    match c.engine.preview(&tile, Arc::new(audio)) {
                        Ok(()) => {
                            c.quip_jobs.previewing = Some(job.clip);
                            c.announce(format!("Previewing {}", job.title));
                        }
                        // Preview's device went while the clip was fetched: nothing plays (D-136).
                        Err(e) => c.notify_error(e),
                    }
                }
                Err(e) => c.notify_error(format!("Couldn't preview “{}”: {e}", job.title)),
            }
        }
    }
    let mut i = 0;
    while i < c.quip_jobs.sets.len() {
        let done = match c.quip_jobs.sets[i].rx.try_recv() {
            Ok(r) => Some(r),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err("the download stopped without an answer".into())),
        };
        let Some(r) = done else {
            i += 1;
            continue;
        };
        let job = c.quip_jobs.sets.remove(i);
        if let Err(e) = r.and_then(|()| import_set(c, &job.file)) {
            c.notify_error(format!("Couldn't import the set file from QuipClipper: {e}"));
        }
        std::fs::remove_file(&job.file).ok();
    }
}

fn import_set(c: &mut Controller, file: &Path) -> Result<(), String> {
    c.execute("set.import", json!({"path": file})).map(|_| ()).map_err(|e| e.message)
}

// ── commands ──────────────────────────────────────────────────────────────────

fn bad(message: impl Into<String>) -> CmdError {
    err("bad_params", message)
}

fn param<'a>(params: &'a Value, key: &str, id: &str) -> Result<Option<&'a str>, CmdError> {
    super::text(params, key, id)
}

fn flag(params: &Value, key: &str, id: &str) -> Result<Option<bool>, CmdError> {
    match params.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(Value::String(s)) if matches!(s.as_str(), "true" | "1" | "yes" | "on") => Ok(Some(true)),
        Some(Value::String(s)) if matches!(s.as_str(), "false" | "0" | "no" | "off") => Ok(Some(false)),
        Some(_) => Err(bad(format!("{id}: {key} must be true or false"))),
    }
}

/// The server a command means, and its token: the one given (it must be connected), or
/// the only one connected.
pub fn server_for(data_dir: &Path, given: Option<&str>) -> Result<(String, String), CmdError> {
    let failed = |e: String| err("failed", e);
    let server = match given {
        Some(s) => super::normalize_server(s).map_err(bad)?,
        None => {
            let all = connected(data_dir).map_err(failed)?;
            match all.as_slice() {
                [] => {
                    return Err(err("not_connected", format!("No QuipClipper is connected. Connect one: {} quip.connect https://quip.example --token <API token>", crate::brand::CLI_NAME)))
                }
                [(one, _)] => one.clone(),
                many => {
                    let names: Vec<&str> = many.iter().map(|(s, _)| s.as_str()).collect();
                    return Err(bad(format!("Several QuipClippers are connected ({}); say which with server", names.join(", "))));
                }
            }
        }
    };
    match token_for(data_dir, &server).map_err(failed)? {
        Some(t) => Ok((server, t)),
        None => Err(err("not_connected", super::not_connected_message(&server))),
    }
}

/// A folder of the library: relative, `/`-separated, no `.`/`..` parts.
fn folder_param(params: &Value, id: &str) -> Result<String, CmdError> {
    let f = param(params, "folder", id)?.unwrap_or("").trim().trim_matches('/');
    if f.is_empty() {
        return Ok(String::new());
    }
    super::check_clip(f).map_err(|e| bad(format!("{id}: {}", e.replace("clip path", "folder"))))
}

fn kind_param(params: &Value, id: &str) -> Result<Option<ClipKind>, CmdError> {
    match param(params, "kind", id)? {
        None | Some("") | Some("all") => Ok(None),
        Some(k) => ClipKind::parse(k).map(Some).ok_or_else(|| bad(format!("{id}: kind must be audio, video, gif or set (got {k:?})"))),
    }
}

fn listing_value(server: &str, l: &Listing) -> Value {
    json!({
        "server": server, "folder": l.folder, "recursive": l.recursive, "folders": l.folders,
        "clips": l.clips.iter().map(Clip::summary).collect::<Vec<_>>(), "count": l.clips.len(), "cursor": l.cursor,
    })
}

/// `quip.browse`, `quip.search`, `quip.new`, `quip.add` and `quip.preview`.
pub fn command(c: &mut Controller, id: &str, params: &Value) -> Result<Value, CmdError> {
    let data_dir = c.store.data_dir.clone();
    let failed = |e: String| err("failed", e);
    let (server, token) = server_for(&data_dir, param(params, "server", id)?)?;
    match id {
        "quip.browse" | "quip.search" => {
            let q = param(params, "query", id)?.map(one_line).unwrap_or_default();
            let search = id == "quip.search";
            if search && q.is_empty() {
                return Err(bad("quip.search: give the words to look for"));
            }
            let newest = match param(params, "sort", id)? {
                None | Some("newest") => true,
                Some("name") => false,
                Some(other) => return Err(bad(format!("{id}: sort must be newest or name (got {other:?})"))),
            };
            let query = Query {
                folder: folder_param(params, id)?,
                recursive: search || flag(params, "recursive", id)?.unwrap_or(false),
                q,
                kind: kind_param(params, id)?,
                since: None,
                newest,
            };
            let listing = list(&server, &token, &query).map_err(|e| failed(e.message))?;
            Ok(listing_value(&server, &listing))
        }
        "quip.new" => {
            let mark = flag(params, "mark", id)?.unwrap_or(false);
            let new = new_clips(&data_dir, &server, &token, mark).map_err(failed)?;
            c.quip_watch.record(&server, &new);
            let mut v = listing_value(&server, &new.listing);
            v["since"] = json!(new.since);
            v["baseline"] = json!(new.since.is_none());
            v["marked"] = json!(new.marked);
            Ok(v)
        }
        "quip.add" => {
            let res = add(c, &data_dir, &server, &token, id, params);
            if let Err(e) = &res {
                c.report(e);
            }
            res
        }
        "quip.preview" => preview(c, &server, &token, id, params),
        _ => Err(err("unknown_command", format!("no command {id:?}"))),
    }
}

fn clip_param(params: &Value, id: &str) -> Result<String, CmdError> {
    let clip = param(params, "clip", id)?.ok_or_else(|| bad(format!("{id}: give the clip's path in the clips library")))?;
    super::check_clip(clip.trim_matches('/')).map_err(|e| bad(format!("{id}: {e}")))
}

fn add(c: &mut Controller, data_dir: &Path, server: &str, token: &str, id: &str, params: &Value) -> Result<Value, CmdError> {
    let clip = clip_param(params, id)?;
    let set = param(params, "set", id)?.map(|s| super::check_name(s, "set", 100)).transpose().map_err(bad)?;
    let entry = find(server, token, &clip).map_err(|e| err("not_found", super::sentence_case(&e)))?;
    match entry.kind() {
        Some(ClipKind::Set) => add_set(c, data_dir, server, token, &entry.clip, &entry.display_title()),
        Some(ClipKind::Audio | ClipKind::Video) => {
            let title = match param(params, "title", id)? {
                Some(t) => super::check_name(t, "title", 200).map_err(bad)?,
                None => entry.display_title(),
            };
            let (start, end) = entry.trim().unzip();
            let link = Link { server: server.to_owned(), clip: entry.clip.clone(), title: Some(title), start, end, set, flac: entry.flac_url.is_some() };
            super::start_import(c, data_dir, link, Some(entry.notes()), false)
        }
        _ => Err(bad(format!("“{}” has no sound to add (a {})", entry.display_title(), entry.kind))),
    }
}

/// Download a set file and import it as a set: on a thread when there's a window (the
/// controller imports it when it's done), else at once. `quip.add` and set links use it.
pub(crate) fn add_set(c: &mut Controller, data_dir: &Path, server: &str, token: &str, clip: &str, title: &str) -> Result<Value, CmdError> {
    let link = Link { server: server.to_owned(), clip: clip.to_owned(), title: None, start: None, end: None, set: None, flac: false };
    let fetch = Fetch::new(&link, token.to_owned());
    let file = data_dir.join(format!("quip-{}.{}", Uuid::new_v4(), crate::setfile::EXTENSION));
    if !c.has_window {
        let r = fetch.download(&file, &Default::default()).and_then(|()| import_set(c, &file));
        std::fs::remove_file(&file).ok();
        r.map_err(|e| err("failed", format!("Couldn't import “{title}” from QuipClipper: {e}")))?;
        return Ok(json!({"server": server, "clip": clip, "set": title, "imported": true}));
    }
    let dest = file.clone();
    let rx = spawn("quip-set", move || fetch.download(&dest, &Default::default())).map_err(failed_err)?;
    c.quip_jobs.sets.push(SetJob { file, rx });
    c.notify(format!("Downloading the set “{title}” from QuipClipper"));
    Ok(json!({"server": server, "clip": clip, "set": title, "imported": false, "downloading": true}))
}

fn failed_err(e: String) -> CmdError {
    err("failed", e)
}

fn preview(c: &mut Controller, server: &str, token: &str, id: &str, params: &Value) -> Result<Value, CmdError> {
    let stop = flag(params, "stop", id)?.unwrap_or(false);
    let playing = c.engine.state(PREVIEW_ID) != crate::logic::PlayState::Stopped;
    let current = c.quip_jobs.preview_state(playing);
    let clip = if stop { None } else { Some(clip_param(params, id)?) };
    let same = matches!((&current, &clip), (Some(PreviewState::Loading(a) | PreviewState::Playing(a)), Some(b)) if a == b);
    if stop || same {
        c.quip_jobs.preview = None;
        c.quip_jobs.previewing = None;
        c.engine.stop(PREVIEW_ID, 0.05);
        return Ok(json!({"stopped": current.is_some()}));
    }
    let clip = clip.expect("not stopping");
    // Preview's device isn't open: say so now rather than fetch a clip that can't play (D-136).
    if let Some(why) = c.engine.preview_unavailable() {
        return Err(err("preview_unavailable", why));
    }
    match ClipKind::parse(match extension(&clip).as_str() {
        e if e == crate::setfile::EXTENSION => "set",
        "gif" => "gif",
        _ => "audio",
    }) {
        Some(k) if !k.has_sound() => return Err(bad(format!("{id}: a {} has no sound to preview", k.as_str()))),
        _ => {}
    }
    let title = match param(params, "title", id)? {
        Some(t) => one_line(t),
        None => cache().iter().rev().find_map(|l| l.listing.clips.iter().find(|e| e.clip == clip).map(Clip::display_title)).unwrap_or_else(|| {
            // The file name, as QuipClipper titles a clip without a line.
            let file = clip.rsplit('/').next().unwrap_or(&clip);
            one_line(&file.rsplit_once('.').map_or(file, |(stem, _)| stem).replace('_', " "))
        }),
    };
    let (s, t, cl) = (server.to_owned(), token.to_owned(), clip.clone());
    let rx = spawn("quip-preview", move || preview_audio(&s, &t, &cl)).map_err(failed_err)?;
    c.engine.stop(PREVIEW_ID, 0.05);
    c.quip_jobs.previewing = None;
    c.quip_jobs.preview = Some(PreviewJob { clip: clip.clone(), title: title.clone(), rx });
    Ok(json!({"server": server, "clip": clip, "title": title, "loading": true}))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::quip::tests::{controller, response, serve};

    #[test]
    fn reads_an_entry_as_the_window_shows_it() {
        let v = json!({
            "name": "00-00-56_Whew.mka", "clip": "OTGW/00-00-56_Whew.mka", "folder": null, "kind": "audio", "title": "Whew",
            "flac_url": "/api/clips/download/OTGW/00-00-56_Whew.mka?as=flac", "unknown_later_field": [1, 2],
            "meta": {"cue": "Whew.\nWell, that's good", "trim": {"start": 1.0, "end": 3.5}, "line": {"start": 56.2, "end": 58.7},
                     "duration": 15.551, "channels": 6, "jellyfin": {"type": "Episode", "show": "Over the Garden Wall", "season": 1, "episode": 4, "title": "Songs of the Dark Lantern", "year": 2014}},
        });
        let c: Clip = serde_json::from_value(v).unwrap();
        assert_eq!(c.kind(), Some(ClipKind::Audio));
        assert_eq!(c.display_title(), "Whew. Well, that's good");
        assert_eq!(c.episode_code().as_deref(), Some("S01E04"));
        assert_eq!(c.source_text().as_deref(), Some("Over the Garden Wall S01E04 · Songs of the Dark Lantern"));
        assert_eq!((c.surround().as_deref(), c.trim()), (Some("5.1"), Some((1.0, 3.5))));
        assert_eq!(c.notes(), "Over the Garden Wall S01E04 · Songs of the Dark Lantern · at 0:56 · QuipClipper");

        // A movie, and a backfilled clip with what the server read from its name.
        let movie: Clip = serde_json::from_value(json!({"clip": "m.mp4", "kind": "video", "title": "Peanut. Plain.",
            "meta": {"cue": null, "trim": {"start": 0.0, "end": null}, "channels": 2, "jellyfin": {"type": "Movie", "show": "13 Going on 30", "title": "13 Going on 30", "year": 2004}}})).unwrap();
        assert_eq!((movie.display_title().as_str(), movie.source_text().as_deref()), ("Peanut. Plain.", Some("13 Going on 30 (2004)")));
        assert_eq!((movie.trim(), movie.surround()), (None, None), "half a trim is no trim");
        let old: Clip = serde_json::from_value(json!({"clip": "S03E26_x.mkv", "name": "S03E26_x.mkv", "kind": "video", "meta": {"source": {"name": "Night Court"}, "parsed": {"season": 3, "episode": 26}}})).unwrap();
        assert_eq!((old.display_title().as_str(), old.source_text().as_deref()), ("S03E26_x.mkv", Some("Night Court S03E26")));
        let bare: Clip = serde_json::from_value(json!({"clip": "a.wav", "kind": "audio", "meta": null})).unwrap();
        assert_eq!((bare.display_title().as_str(), bare.source_text(), bare.notes().as_str()), ("Clip", None, "QuipClipper"));
        let set: Clip = serde_json::from_value(json!({"clip": "LeadStinger/Heat.lsset", "kind": "set", "title": "Heat", "meta": {"items": [{}, {}]}})).unwrap();
        assert_eq!(set.source_text().as_deref(), Some("Set file, 2 tiles"));
        assert_eq!(at_most("x".repeat(300), 200).chars().count(), 200);
    }

    #[test]
    fn builds_listing_urls() {
        let q = Query { folder: "Heat (1995)/lines".into(), recursive: true, q: " get attached ".into(), kind: Some(ClipKind::Set), since: Some("v1.42".into()), newest: true };
        assert_eq!(q.url("https://quip.example/qc"), "https://quip.example/qc/api/clips?folder=Heat+%281995%29%2Flines&recursive=1&q=get+attached&kind=set&since=v1.42&sort=newest");
        assert_eq!(Query::default().url("http://q"), "http://q/api/clips?sort=name");
    }

    fn listing(clips: Value) -> Vec<u8> {
        serde_json::to_vec(&json!({"contract": CONTRACT, "folder": "", "recursive": false, "folders": ["Heat", "[old] cuts"], "clips": clips, "count": 1, "cursor": "v1.100", "etag": "\"e1\""})).unwrap()
    }

    /// A mock QuipClipper: the clips library contract over HTTP, as the server speaks it.
    /// Its token is `tok`.
    pub(crate) fn mock(hits: Arc<Mutex<Vec<String>>>) -> String {
        let wav = {
            let p = std::env::temp_dir().join(format!("sb-quip-clips-{}.wav", Uuid::new_v4()));
            crate::wav::write_tone(&p, 8000, 2.0, &[440.0], 0.4).unwrap();
            let b = std::fs::read(&p).unwrap();
            std::fs::remove_file(p).ok();
            b
        };
        let set = {
            let (mut c, root) = controller();
            let tone = root.join("Beep.wav");
            std::fs::write(&tone, &wav).unwrap();
            c.store.library.sets.push(crate::model::SoundSet::named("Quips"));
            c.execute("tile.add", json!({"paths": [tone], "set": "Quips"})).unwrap();
            c.store.finish_imports();
            let p = root.join("Quips.lsset");
            let si = c.store.library.sets.len() - 1;
            crate::setfile::export(&mut c.store, &[si], &p).unwrap();
            let bytes = std::fs::read(p).unwrap();
            drop(c);
            std::fs::remove_dir_all(root).ok();
            bytes
        };
        serve(move |req, headers| {
            hits.lock().unwrap().push(req.to_owned());
            if !headers.iter().any(|h| h.eq_ignore_ascii_case("x-api-key: tok")) {
                return response("401 Unauthorized", "", b"{\"detail\": \"bad token\"}");
            }
            let fresh = headers.iter().any(|h| h.eq_ignore_ascii_case("if-none-match: \"e1\""));
            let entry = json!({"name": "line.wav", "clip": "Heat/line.wav", "kind": "audio", "folder": "Heat", "flac_url": "/api/clips/download/Heat/line.wav?as=flac",
                "download_url": "https://elsewhere.example/clips/Heat/line.wav",
                "meta": {"cue": "Don't let yourself get attached", "trim": {"start": 0.5, "end": 1.5}, "duration": 2.0, "channels": 6, "jellyfin": {"show": "Heat", "title": "Heat", "year": 1995}}});
            let path = req.split(' ').nth(1).unwrap_or_default();
            match path {
                p if p.starts_with("/api/clips?") && p.contains("since=v1.100") => response("200 OK", "ETag: \"e2\"\r\n", &serde_json::to_vec(&json!({"contract": CONTRACT, "clips": [entry], "cursor": "v1.200"})).unwrap()),
                p if p.starts_with("/api/clips?") && p.contains("since=bad") => response("400 Bad Request", "", b"{\"detail\": \"malformed cursor\"}"),
                p if p.starts_with("/api/clips?") && p.contains("folder=Away") => response("403 Forbidden", "", b"{\"detail\": \"outside the clips library\"}"),
                p if p.starts_with("/api/clips?") && p.contains("folder=Old") => response("200 OK", "", b"{\"folder\": \"\", \"clips\": []}"),
                p if p.starts_with("/api/clips?") && fresh => response("304 Not Modified", "ETag: \"e1\"\r\n", b""),
                p if p.starts_with("/api/clips?") => response("200 OK", "ETag: \"e1\"\r\n", &listing(json!([entry, {"name": "Quips.lsset", "clip": "LeadStinger/Quips.lsset", "kind": "set", "meta": {"name": "Quips"}}]))),
                "/api/clips/download/Heat/line.wav?as=flac" | "/api/clips/stream/Heat/line.wav" => response("200 OK", "Content-Type: audio/wav\r\n", &wav),
                "/api/clips/download/LeadStinger/Quips.lsset" => response("200 OK", "Content-Type: application/zip\r\n", &set),
                _ => response("404 Not Found", "", b"{\"detail\": \"Not Found\"}"),
            }
        })
    }

    fn wait(c: &mut Controller, done: impl Fn(&Controller) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !done(c) && Instant::now() < deadline {
            c.tick(0.01);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(done(c), "timed out");
    }

    #[test]
    fn browses_caches_with_etags_and_says_what_went_wrong() {
        let hits = Arc::new(Mutex::new(Vec::new()));
        let base = mock(hits.clone());
        let (mut c, root) = controller();
        assert_eq!(c.execute("quip.browse", json!({})).unwrap_err().code, "not_connected");
        c.execute("quip.connect", json!({"url": base, "token": "tok", "check": false})).unwrap();

        let r = c.execute("quip.browse", json!({"kind": "audio", "sort": "name"})).unwrap();
        assert_eq!((r["server"].as_str(), r["folders"][0].as_str(), r["count"].as_u64()), (Some(base.as_str()), Some("Heat"), Some(2)));
        assert_eq!(r["clips"][0]["title"], "Don't let yourself get attached");
        assert_eq!(r["clips"][0]["trim"], json!({"start": 0.5, "end": 1.5}));
        // Again: revalidated, and the 304 gives the same listing.
        assert_eq!(c.execute("quip.browse", json!({"kind": "audio", "sort": "name"})).unwrap(), r);
        assert!(hits.lock().unwrap().iter().filter(|h| h.contains("kind=audio&sort=name")).count() == 2);

        let e = c.execute("quip.browse", json!({"folder": "Away"})).unwrap_err();
        assert!(e.message.contains("HTTP 403") && e.message.contains("outside the clips library"), "{}", e.message);
        let e = c.execute("quip.browse", json!({"folder": "Old"})).unwrap_err();
        assert!(e.message.contains("update QuipClipper"), "{}", e.message);
        assert_eq!(c.execute("quip.browse", json!({"folder": "../etc"})).unwrap_err().code, "bad_params");
        assert_eq!(c.execute("quip.browse", json!({"kind": "podcast"})).unwrap_err().code, "bad_params");
        assert_eq!(c.execute("quip.search", json!({"query": " "})).unwrap_err().code, "bad_params");
        let r = c.execute("quip.search", json!({"query": "attached"})).unwrap();
        assert_eq!(r["recursive"], json!(false), "as the (mock) server says");
        assert!(hits.lock().unwrap().iter().any(|h| h.contains("recursive=1&q=attached&sort=newest")));

        // A wrong token says so, without the token.
        c.execute("quip.connect", json!({"url": base, "token": "nope-token", "check": false})).unwrap();
        let e = c.execute("quip.browse", json!({"folder": "Elsewhere"})).unwrap_err();
        assert!(e.message.contains("refused the token") && !e.message.contains("nope-token"), "{}", e.message);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn new_clips_start_from_a_baseline_kept_beside_the_token() {
        let hits = Arc::new(Mutex::new(Vec::new()));
        let base = mock(hits);
        let (mut c, root) = controller();
        c.execute("quip.connect", json!({"url": base, "token": "tok", "check": false})).unwrap();
        let first = c.execute("quip.new", json!({})).unwrap();
        assert_eq!((first["baseline"].as_bool(), first["count"].as_u64()), (Some(true), Some(0)), "the first look flags nothing");
        assert_eq!(crate::quip::seen_cursor(&c.store.data_dir, &base).unwrap().as_deref(), Some("v1.100"));
        let file = std::fs::read_to_string(c.store.data_dir.join(crate::quip::SERVERS_FILE)).unwrap();
        assert!(file.contains("\"seen\": \"v1.100\""), "{file}");
        assert!(!std::fs::read_to_string(c.store.library_path()).unwrap_or_default().contains("v1.100"), "never in the library");

        let r = c.execute("quip.new", json!({})).unwrap();
        assert_eq!((r["count"].as_u64(), r["since"].as_str(), r["marked"].as_bool()), (Some(1), Some("v1.100"), Some(false)));
        let r = c.execute("quip.new", json!({"mark": true})).unwrap();
        assert_eq!(r["marked"], true);
        assert_eq!(crate::quip::seen_cursor(&c.store.data_dir, &base).unwrap().as_deref(), Some("v1.200"));
        // A new token keeps the cursor; a cursor the server refuses starts again.
        c.execute("quip.connect", json!({"url": base, "token": "tok", "check": false})).unwrap();
        assert_eq!(crate::quip::seen_cursor(&c.store.data_dir, &base).unwrap().as_deref(), Some("v1.200"));
        crate::quip::set_seen(&c.store.data_dir, &base, "bad").unwrap();
        assert_eq!(c.execute("quip.new", json!({})).unwrap()["baseline"], true);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn the_watcher_counts_new_clips_quietly_and_marks_what_it_showed() {
        let hits = Arc::new(Mutex::new(Vec::new()));
        let base = mock(hits.clone());
        let (mut c, root) = controller();
        let dir = c.store.data_dir.clone();
        c.has_window = true;
        c.execute("quip.connect", json!({"url": base, "token": "tok", "check": false})).unwrap();
        // The window is closed and the badge on: a round at the first tick, a baseline.
        wait(&mut c, |c| c.quip_watch.servers().first().is_some_and(|s| s.checked.is_some()));
        assert_eq!(c.quip_watch.count(), 0, "the first look flags nothing");
        assert_eq!(crate::quip::seen_cursor(&dir, &base).unwrap().as_deref(), Some("v1.100"));
        c.quip_watch.ask_now();
        wait(&mut c, |c| c.quip_watch.count() == 1);
        let st = c.execute("status", json!({})).unwrap()["quip_new"].clone();
        assert_eq!((st["count"].as_u64(), st["badge"].as_bool()), (Some(1), Some(true)));
        assert_eq!((st["servers"][0]["server"].as_str(), st["servers"][0]["count"].as_u64()), (Some(base.as_str()), Some(1)));
        assert!(st["servers"][0]["error"].is_null());

        // A click: the window opens on them, and they count as seen with the cursor of
        // the listing shown, a local write.
        let asked = hits.lock().unwrap().len();
        assert_eq!(c.execute("view.quip", json!({"new": true})).unwrap(), json!({"open": true, "server": base, "new": 1}));
        assert_eq!(hits.lock().unwrap().len(), asked, "marking asks the server nothing");
        assert!(c.show_quip);
        let goto = c.quip_goto.take().unwrap();
        assert_eq!((goto.server.as_deref(), goto.new.map(|l| l.clips.len())), (Some(base.as_str()), Some(1)));
        assert_eq!(crate::quip::seen_cursor(&dir, &base).unwrap().as_deref(), Some("v1.200"));
        assert_eq!(c.quip_watch.count(), 0);
        // Nothing new: the window just opens.
        assert_eq!(c.execute("view.quip", json!({"new": true})).unwrap(), json!({"open": true, "server": null, "new": 0}));

        // quip.new's answer is the watcher's too; a cursor moved meanwhile isn't moved back.
        crate::quip::set_seen(&dir, &base, "v1.100").unwrap();
        c.execute("quip.new", json!({})).unwrap();
        assert_eq!(c.quip_watch.count_for(&base), 1);
        crate::quip::set_seen(&dir, &base, "v1.150").unwrap();
        c.execute("view.quip", json!({"new": true, "server": base})).unwrap();
        assert_eq!(crate::quip::seen_cursor(&dir, &base).unwrap().as_deref(), Some("v1.150"));

        // A server out of reach: no notice, its count kept, why in status; it waits.
        c.execute("quip.connect", json!({"url": "http://127.0.0.1:1", "token": "tok", "check": false})).unwrap();
        crate::quip::set_seen(&dir, &base, "v1.100").unwrap();
        c.notice = None;
        c.quip_watch.ask_now();
        wait(&mut c, |c| c.quip_watch.servers().len() == 2 && c.quip_watch.servers()[0].error.is_some() && c.quip_watch.count() == 1);
        assert!(c.notice.is_none(), "quiet: {:?}", c.notice.as_ref().map(|n| n.text.clone()));
        let st = c.execute("status", json!({})).unwrap()["quip_new"].clone();
        assert_eq!((st["servers"][0]["server"].as_str(), st["servers"][1]["count"].as_u64()), (Some("http://127.0.0.1:1"), Some(1)));
        assert!(st["servers"][0]["error"].is_string() && st["servers"][0]["checked"].is_null());
        assert_eq!(c.quip_watch.first_with_new(), Some(base.as_str()));

        // Off, with the window closed: no rounds; open, it asks again.
        c.show_quip = false;
        assert_eq!(c.execute("settings.quipBadge", json!({"on": false})).unwrap()["on"], false);
        assert!(!c.store.library.quip_badge);
        c.quip_watch.ask_now();
        let asked = hits.lock().unwrap().len();
        for _ in 0..20 {
            c.tick(0.01);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(hits.lock().unwrap().len(), asked, "off: the servers aren't asked");
        assert_eq!(c.execute("status", json!({})).unwrap()["quip_new"]["badge"], false);
        c.show_quip = true;
        wait(&mut c, |_| hits.lock().unwrap().len() > asked);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn adds_clips_as_flac_with_their_metadata_and_sets_as_sets() {
        let hits = Arc::new(Mutex::new(Vec::new()));
        let base = mock(hits.clone());
        let (mut c, root) = controller();
        c.execute("quip.connect", json!({"url": base, "token": "tok", "check": false})).unwrap();
        let r = c.execute("quip.add", json!({"clip": "Heat/line.wav", "set": "Movie Lines"})).unwrap();
        assert_eq!((r["flac"].as_bool(), r["trimStart"].as_f64(), r["trimEnd"].as_f64()), (Some(true), Some(0.5), Some(1.5)));
        wait(&mut c, |c| c.store.importing().count() == 0 && !c.store.is_loading());
        let id: Uuid = serde_json::from_value(r["tile"].clone()).unwrap();
        let t = c.store.library.tile(id).unwrap();
        assert_eq!((t.title.as_str(), t.start, t.end), ("Don't let yourself get attached", 0.5, Some(1.5)));
        assert_eq!(t.notes, "Heat (1995) · QuipClipper");
        assert!(matches!(c.store.media(t), Some(crate::store::Media::Ready { .. })));
        let seen = hits.lock().unwrap().clone();
        assert!(seen.iter().any(|h| h.contains("/api/clips/download/Heat/line.wav?as=flac")), "{seen:?}");
        assert!(!seen.iter().any(|h| h.contains("elsewhere")), "never the listing's own URLs");

        // A set file: downloaded on a thread, then imported as a set.
        c.has_window = true;
        let sets = c.store.library.sets.len();
        let r = c.execute("quip.add", json!({"clip": "LeadStinger/Quips.lsset"})).unwrap();
        assert_eq!(r["downloading"], true);
        wait(&mut c, |c| c.quip_jobs.sets_downloading() == 0);
        assert_eq!(c.store.library.sets.len(), sets + 1);
        assert!(!std::fs::read_dir(&c.store.data_dir).unwrap().flatten().any(|e| e.file_name().to_string_lossy().ends_with(".lsset")), "the download is removed");
        let e = c.execute("quip.add", json!({"clip": "Heat/missing.wav"})).unwrap_err();
        assert_eq!(e.code, "not_found");
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn set_links_import_the_whole_set() {
        let hits = Arc::new(Mutex::new(Vec::new()));
        let base = mock(hits.clone());
        let (mut c, root) = controller();
        c.execute("quip.connect", json!({"url": base, "token": "tok", "check": false})).unwrap();
        let link = Link { server: base.clone(), clip: "LeadStinger/Quips.lsset".into(), title: None, start: None, end: None, set: None, flac: false }.to_link();
        // No window: downloaded and imported at once, the download removed.
        c.has_window = false;
        let quips = |c: &Controller| c.store.library.sets.iter().filter(|s| s.name.starts_with("Quips") && !s.tiles.is_empty()).count();
        let r = c.execute("quip.import", json!({"link": link})).unwrap();
        assert_eq!((r["set"].as_str(), r["imported"].as_bool()), (Some("Quips"), Some(true)));
        assert_eq!(quips(&c), 1);
        let leftover = |c: &Controller| std::fs::read_dir(&c.store.data_dir).unwrap().flatten().any(|e| e.file_name().to_string_lossy().ends_with(".lsset"));
        assert!(!leftover(&c), "the download is removed");
        assert!(hits.lock().unwrap().iter().any(|h| h.contains("/api/clips/download/LeadStinger/Quips.lsset")));
        // Parameters for one tile are refused, the link's or the command's.
        for extra in [json!({"title": "x"}), json!({"start": 1}), json!({"end": 2}), json!({"set": "Lines"}), json!({"as": "flac"}), json!({"notes": "n"})] {
            let mut p = json!({"link": link});
            p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            assert_eq!(c.execute("quip.import", p.clone()).unwrap_err().code, "bad_params", "{p}");
        }
        // An opened link with settings.quipAskEachImport: asked about as a set first.
        c.has_window = true;
        c.execute("settings.quipAskEachImport", json!({"on": true})).unwrap();
        let r = c.execute("quip.import", json!({"link": link, "ask": true})).unwrap();
        assert_eq!(r["asked"], true);
        assert!(c.announcement.as_ref().is_some_and(|a| a.text.ends_with("sent the set “Quips”. Import it?")), "{:?}", c.announcement);
        // Yes: downloaded on a thread, then imported as a set.
        let r = c.execute("quip.import", json!({"link": link})).unwrap();
        assert_eq!((r["downloading"].as_bool(), r["imported"].as_bool()), (Some(true), Some(false)));
        assert!(c.announcement.as_ref().is_some_and(|a| a.text.contains("Downloading the set “Quips”")), "{:?}", c.announcement);
        wait(&mut c, |c| c.quip_jobs.sets_downloading() == 0);
        assert_eq!(quips(&c), 2);
        assert!(!leftover(&c), "the download is removed");
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn imports_keep_as_flac() {
        let l = Link::parse("https://quip.example/api/clips/download/Heat/a%20b.mkv?as=flac").unwrap();
        assert!(l.flac);
        assert_eq!(l.download_url(), "https://quip.example/api/clips/download/Heat/a%20b.mkv?as=flac");
        assert_eq!(l.download_name(), "a b.flac");
        assert_eq!(Link::parse(&l.to_link()).unwrap(), l, "and in a link");
        assert!(Link::parse("https://quip.example/api/clips/download/a.mkv?as=mp3").is_err());
        assert!(Link::parse("leadstinger://quip?server=https://q.example&clip=a.mkv&as=wav").is_err());
    }

    #[test]
    fn previews_play_from_memory_on_the_preview_voice() {
        let hits = Arc::new(Mutex::new(Vec::new()));
        let base = mock(hits.clone());
        let (mut c, root) = controller();
        c.execute("quip.connect", json!({"url": base, "token": "tok", "check": false})).unwrap();
        let r = c.execute("quip.preview", json!({"clip": "Heat/line.wav"})).unwrap();
        assert_eq!(r["loading"], true);
        wait(&mut c, |c| c.quip_jobs.preview.is_none());
        assert_eq!(c.quip_jobs.previewing.as_deref(), Some("Heat/line.wav"));
        assert!(hits.lock().unwrap().iter().any(|h| h.contains("/api/clips/stream/Heat/line.wav")), "a WAV streams as it is");
        assert!(!std::fs::read_dir(&c.store.media_dir).map(|d| d.count() > 0).unwrap_or(false), "nothing saved");
        assert_eq!(c.execute("quip.preview", json!({"stop": true})).unwrap()["stopped"], true, "it was previewing");
        assert_eq!(c.execute("quip.preview", json!({"clip": "LeadStinger/Quips.lsset"})).unwrap_err().code, "bad_params");
        // A clip that isn't there: the window hears why.
        c.execute("quip.preview", json!({"clip": "Heat/gone.mkv"})).unwrap();
        wait(&mut c, |c| c.quip_jobs.preview.is_none());
        assert!(c.notice.as_ref().is_some_and(|n| n.error && n.text.contains("HTTP 404")), "{:?}", c.notice.as_ref().map(|n| &n.text));
        // Preview's device isn't there: refused before anything is fetched, never on Main (D-136).
        c.engine = crate::audio::Engine::fake(&["Speakers"]);
        c.execute("settings.previewOutput", json!({"on": true, "device": "Headphones"})).unwrap();
        let e = c.execute("quip.preview", json!({"clip": "Heat/line.wav"})).unwrap_err();
        assert_eq!((e.code, e.message.as_str()), ("preview_unavailable", "Preview's device Headphones isn't connected: nothing played"));
        assert!(c.quip_jobs.preview.is_none());
        std::fs::remove_dir_all(root).ok();
    }
}
