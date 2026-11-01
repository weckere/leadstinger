//! Clips from QuipClipper (D-076, D-081, D-082): `leadstinger://quip?…` links and the `quip.*`
//! commands.
//!
//! QuipClipper, a self-hosted web app, finds a line of dialogue by its subtitles and cuts
//! it with a couple of seconds either side. Its "Send to LeadStinger" button opens
//!
//! ```text
//! leadstinger://quip?server=<base URL>&clip=<clip path>&title=<text>&start=<s>&end=<s>&set=<name>
//! ```
//!
//! and this module downloads `<server>/api/clips/download/<clip>` into a new tile whose
//! trim is the line itself, so the cut can be fine-tuned without losing the handles.
//! docs/QUIPCLIPPER.md is the format's reference.
//!
//! Only servers the user connected (`quip.connect`, `quip.pair`, or Connect in the window's
//! first-contact prompt) are ever fetched from: a link naming any other server is refused
//! before anything goes over the network, or, when the system opened it with a window up,
//! asks first (D-132), so a crafted link on a web page can't make the app download from
//! somewhere else. The tokens live in `quip-servers.json` in the data dir (0600), never in
//! the library or a set file, and never appear in messages or logs. A server with no
//! password can be connected without one: it gets no credentials at all.

use crate::controller::{CmdError, Controller, Effect};
use crate::model::SoundSet;
use crate::transcode::{Progress, CANCELLED};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use url::Url;

pub mod clips;
pub mod discover;
pub mod pair;
pub mod watch;

/// Where QuipClipper serves a finished clip, under the server's base URL.
pub const DOWNLOAD_PATH: &str = "/api/clips/download/";
/// The largest clip downloaded.
pub const MAX_BYTES: u64 = 1 << 30;
/// The file the tokens are kept in, in the data dir.
pub const SERVERS_FILE: &str = "quip-servers.json";
/// Same-server redirects followed (to another host: never).
const MAX_REDIRECTS: usize = 5;
const LINK_PARAMS: &[&str] = &["server", "clip", "title", "start", "end", "set", "as"];

/// The schemes a link may use: `leadstinger`, and this brand's own (src/brand.rs).
pub fn schemes() -> [&'static str; 2] {
    ["leadstinger", crate::brand::SLUG]
}

/// Whether a command-line argument is a link for us (Linux passes it as `%u`).
pub fn is_link(arg: &str) -> bool {
    arg.split_once(':').is_some_and(|(scheme, _)| schemes().iter().any(|s| s.eq_ignore_ascii_case(scheme)))
}

/// The server a `leadstinger://pair?server=<base URL>` link names: QuipClipper's Connect
/// LeadStinger button (D-133). It takes nothing but `server`.
pub fn parse_pair_link(s: &str) -> Result<String, String> {
    let url = Url::parse(s.trim()).map_err(|e| format!("the link isn't a valid URL ({e})"))?;
    if !is_link(s.trim()) || !url.host_str().is_some_and(|h| h.eq_ignore_ascii_case("pair")) || !matches!(url.path(), "" | "/") {
        return Err(format!("the link must start {}://pair?", schemes()[0]));
    }
    if !url.username().is_empty() || url.password().is_some() || url.port().is_some() || url.fragment().is_some() {
        return Err(format!("the link must start {}://pair? and have no #fragment", schemes()[0]));
    }
    let mut server = None;
    for (k, v) in url.query_pairs() {
        if k != "server" {
            return Err(format!("the link has an unknown parameter {k:?} (it takes server)"));
        }
        if server.replace(v.into_owned()).is_some() {
            return Err("the link gives server twice".into());
        }
    }
    normalize_server(&server.ok_or("the link has no server")?)
}

/// What a link or set file the system opened us with runs: Connect for a pair link, an
/// import for a clip link, and an import after asking for a set file. Each asks first
/// where it should (`ask`), and brings the window to the front.
pub fn opened_command(opened: &str) -> (&'static str, Value) {
    if !is_link(opened) {
        return ("set.import", json!({"path": opened, "ask": true, "raise": true}));
    }
    match parse_pair_link(opened) {
        Ok(server) => ("quip.pair", json!({"url": server, "ask": true, "raise": true})),
        // A pair link that's malformed still gets its own message.
        Err(e) if Url::parse(opened).ok().and_then(|u| u.host_str().map(|h| h.eq_ignore_ascii_case("pair"))) == Some(true) => {
            ("quip.pair", json!({"link_error": e}))
        }
        Err(_) => ("quip.import", json!({"link": opened, "ask": true, "raise": true})),
    }
}

// ── links ─────────────────────────────────────────────────────────────────────

/// A clip to fetch: what a link (or a clip URL plus parameters) says.
#[derive(Debug, Clone, PartialEq)]
pub struct Link {
    /// The server's base URL, normalised ([`normalize_server`]).
    pub server: String,
    /// The clip's path under QuipClipper's clips folder, `/`-separated, decoded.
    pub clip: String,
    pub title: Option<String>,
    /// The line's start and end in the clip (seconds): the tile's trim.
    pub start: Option<f64>,
    pub end: Option<f64>,
    /// The set to put the tile in (created if missing); else the current one.
    pub set: Option<String>,
    /// Ask the server for the clip as FLAC (`?as=flac`): every channel kept, and no
    /// ffmpeg needed here for video or AC-3 clips.
    pub flac: bool,
}

impl Link {
    /// A `leadstinger://quip?…` link, or a clip's download URL
    /// (`<server>/api/clips/download/<clip>`).
    pub fn parse(s: &str) -> Result<Link, String> {
        let s = s.trim();
        if s.len() > 8192 {
            return Err("the link is too long".into());
        }
        if is_link(s) {
            Link::parse_link(s)
        } else if s.starts_with("http://") || s.starts_with("https://") || s.starts_with("HTTP") {
            Link::parse_clip_url(s)
        } else {
            Err(format!("not a {}://quip link or a QuipClipper clip URL", schemes()[0]))
        }
    }

    fn parse_link(s: &str) -> Result<Link, String> {
        let url = Url::parse(s).map_err(|e| format!("the link isn't a valid URL ({e})"))?;
        if !url.host_str().is_some_and(|h| h.eq_ignore_ascii_case("quip")) || !matches!(url.path(), "" | "/") {
            return Err(format!("the link must start {}://quip?", schemes()[0]));
        }
        if !url.username().is_empty() || url.password().is_some() || url.port().is_some() || url.fragment().is_some() {
            return Err(format!("the link must start {}://quip? and have no #fragment", schemes()[0]));
        }
        let mut seen: BTreeMap<String, String> = BTreeMap::new();
        for (k, v) in url.query_pairs() {
            if !LINK_PARAMS.contains(&k.as_ref()) {
                return Err(format!("the link has an unknown parameter {k:?} (it takes {})", LINK_PARAMS.join(", ")));
            }
            if seen.insert(k.to_string(), v.into_owned()).is_some() {
                return Err(format!("the link gives {k} twice"));
            }
        }
        let server = seen.get("server").ok_or("the link has no server")?;
        let clip = seen.get("clip").ok_or("the link has no clip")?;
        let mut link = Link { server: normalize_server(server)?, clip: check_clip(clip)?, title: None, start: None, end: None, set: None, flac: false };
        link.apply(seen.get("title").map(String::as_str), seen.get("start").map(String::as_str), seen.get("end").map(String::as_str), seen.get("set").map(String::as_str))?;
        if let Some(a) = seen.get("as") {
            link.set_as(a)?;
        }
        link.check_set()?;
        Ok(link)
    }

    fn parse_clip_url(s: &str) -> Result<Link, String> {
        let url = Url::parse(s).map_err(|e| format!("the clip URL isn't valid ({e})"))?;
        // `?as=flac` is the one query QuipClipper's flac_url carries.
        let flac = url.query() == Some("as=flac");
        if (url.query().is_some() && !flac) || url.fragment().is_some() {
            return Err("the clip URL must have no ?query (other than ?as=flac) or #fragment".into());
        }
        let path = url.path();
        let Some(at) = path.find(DOWNLOAD_PATH) else {
            return Err(format!("not a QuipClipper clip URL: its path has no {DOWNLOAD_PATH}"));
        };
        let mut base = url.clone();
        base.set_path(&path[..at]);
        let clip = path[at + DOWNLOAD_PATH.len()..].split('/').map(percent_decode).collect::<Result<Vec<_>, _>>()?.join("/");
        base.set_query(None);
        let link = Link { server: normalize_server(base.as_str())?, clip: check_clip(&clip)?, title: None, start: None, end: None, set: None, flac };
        link.check_set()?;
        Ok(link)
    }

    /// The clip is a set file (`.lsset`): the link imports the whole set, as
    /// QuipClipper's Send to LeadStinger does with several lines ticked.
    pub fn is_set(&self) -> bool {
        self.file_name().rsplit_once('.').is_some_and(|(_, ext)| ext.eq_ignore_ascii_case(crate::setfile::EXTENSION))
    }

    /// A set link carries nothing for one tile: no title, start, end, set or `as`.
    pub fn check_set(&self) -> Result<(), String> {
        if !self.is_set() {
            return Ok(());
        }
        let given: Vec<&str> = [("title", self.title.is_some()), ("start", self.start.is_some()), ("end", self.end.is_some()), ("set", self.set.is_some()), ("as", self.flac)]
            .into_iter()
            .filter_map(|(k, on)| on.then_some(k))
            .collect();
        if given.is_empty() {
            Ok(())
        } else {
            Err(format!("a link to a set file imports the whole set and takes no {} (it takes only server and clip)", given.join(", ")))
        }
    }

    /// What the link sent, for prompts: “title”, or the set “name”.
    pub fn described(&self) -> String {
        if self.is_set() {
            format!("the set “{}”", self.tile_title())
        } else {
            format!("“{}”", self.tile_title())
        }
    }

    /// `as`: the format to ask the server for; only `flac` (or nothing) is known.
    pub fn set_as(&mut self, a: &str) -> Result<(), String> {
        match a.trim() {
            "flac" => self.flac = true,
            "" => self.flac = false,
            other => return Err(format!("as must be flac (got {other:?})")),
        }
        Ok(())
    }

    /// Set the optional fields, checking each; `None` leaves one as it is.
    pub fn apply(&mut self, title: Option<&str>, start: Option<&str>, end: Option<&str>, set: Option<&str>) -> Result<(), String> {
        if let Some(t) = title {
            self.title = Some(check_name(t, "title", 200)?);
        }
        if let Some(s) = set {
            self.set = Some(check_name(s, "set", 100)?);
        }
        if let Some(s) = start {
            self.start = Some(seconds(s, "start")?);
        }
        if let Some(e) = end {
            self.end = Some(seconds(e, "end")?);
        }
        if let (Some(s), Some(e)) = (self.start, self.end) {
            if e <= s {
                return Err(format!("the end ({e} s) must be after the start ({s} s)"));
            }
        }
        Ok(())
    }

    /// The clip's file name (its last path segment).
    pub fn file_name(&self) -> &str {
        self.clip.rsplit('/').next().unwrap_or(&self.clip)
    }

    /// The name of the file that arrives: the clip's, or its stem with `.flac`.
    pub fn download_name(&self) -> String {
        let f = self.file_name();
        if self.flac {
            format!("{}.flac", f.rsplit_once('.').map_or(f, |(stem, _)| stem))
        } else {
            f.to_owned()
        }
    }

    /// The tile's title: the link's, or the clip's file name without its extension.
    pub fn tile_title(&self) -> String {
        self.title.clone().unwrap_or_else(|| {
            let f = self.file_name();
            f.rsplit_once('.').map_or(f, |(stem, _)| stem).to_owned()
        })
    }

    /// Where the clip is downloaded from.
    pub fn download_url(&self) -> String {
        format!("{}{DOWNLOAD_PATH}{}{}", self.server, encode_clip(&self.clip), if self.flac { "?as=flac" } else { "" })
    }

    /// The link for this clip, as QuipClipper builds it.
    pub fn to_link(&self) -> String {
        let mut q = url::form_urlencoded::Serializer::new(String::new());
        q.append_pair("server", &self.server).append_pair("clip", &self.clip);
        if let Some(t) = &self.title {
            q.append_pair("title", t);
        }
        for (k, v) in [("start", self.start), ("end", self.end)] {
            if let Some(v) = v {
                q.append_pair(k, &v.to_string());
            }
        }
        if let Some(s) = &self.set {
            q.append_pair("set", s);
        }
        if self.flac {
            q.append_pair("as", "flac");
        }
        // %20 rather than +, as JavaScript's encodeURIComponent writes it; both parse.
        format!("{}://quip?{}", schemes()[0], q.finish().replace('+', "%20"))
    }
}

/// A server's base URL in one form, so a link's server matches the one connected:
/// `scheme://host[:port][/path]` with the scheme and host lower case, the default port
/// and any trailing `/` dropped. Only http and https, with no user name, password, query
/// or fragment.
pub fn normalize_server(s: &str) -> Result<String, String> {
    let url = Url::parse(s.trim()).map_err(|e| format!("{s:?} isn't a server URL ({e}); give one like https://quip.example"))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(format!("the server must be an http or https URL (got {}:)", url.scheme()));
    }
    if url.host_str().is_none_or(str::is_empty) {
        return Err("the server URL has no host".into());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("the server URL must not hold a user name or password; give the token with --token".into());
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err("the server URL must have no ?query or #fragment".into());
    }
    let mut out = url.as_str().trim_end_matches('/').to_owned();
    // `Url` already lower-cases the scheme and host and drops a default port.
    if out.ends_with(DOWNLOAD_PATH.trim_end_matches('/')) {
        out.truncate(out.len() - DOWNLOAD_PATH.trim_end_matches('/').len());
    }
    Ok(out)
}

fn check_clip(clip: &str) -> Result<String, String> {
    let bad = |why: &str| Err(format!("the clip path {clip:?} {why}"));
    if clip.is_empty() || clip.len() > 1024 {
        return bad("must be 1 to 1024 bytes");
    }
    if clip.chars().any(|c| c.is_control() || c == '\\') {
        return bad("has a control character or a backslash");
    }
    if clip.split('/').any(|seg| matches!(seg, "" | "." | "..")) {
        return bad("must be relative, with no empty, . or .. parts");
    }
    Ok(clip.to_owned())
}

fn check_name(s: &str, what: &str, max: usize) -> Result<String, String> {
    let t = s.trim();
    if t.is_empty() || t.chars().count() > max || t.chars().any(char::is_control) {
        return Err(format!("the {what} must be 1 to {max} characters, on one line"));
    }
    Ok(t.to_owned())
}

fn seconds(s: &str, what: &str) -> Result<f64, String> {
    match s.trim().parse::<f64>() {
        Ok(v) if v.is_finite() && (0.0..=86_400.0).contains(&v) => Ok(v),
        _ => Err(format!("the {what} must be a number of seconds from 0 to 86400 (got {s:?})")),
    }
}

/// A clip path for a URL: each part percent-encoded, the `/`s kept.
pub fn encode_clip(clip: &str) -> String {
    clip.split('/').map(percent_encode).collect::<Vec<_>>().join("/")
}

/// Every byte but A–Z a–z 0–9 `-._~` as %XX: one path segment.
fn percent_encode(seg: &str) -> String {
    let mut out = String::with_capacity(seg.len());
    for b in seg.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn percent_decode(seg: &str) -> Result<String, String> {
    let bytes = seg.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = bytes.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u8::from_str_radix(h, 16).ok());
            out.push(hex.ok_or_else(|| format!("bad %-escape in {seg:?}"))?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| format!("{seg:?} isn't UTF-8 text"))
}

// ── connected servers ─────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Default)]
struct Servers {
    servers: BTreeMap<String, Entry>,
}

#[derive(Serialize, Deserialize, Clone)]
struct Entry {
    /// `None`: a server with no password that the user approved (D-132); it's sent no
    /// credentials at all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    token: Option<String>,
    /// When it was connected (Unix seconds).
    added: u64,
    /// The clips library's `cursor` when its new clips were last seen (`quip.new`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    seen: Option<String>,
    /// QuipClipper's id for this app's pairing (`quip.pair`), so Disconnect can revoke it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    client_id: Option<String>,
    /// The server's own id (`/api/config`), so it's known under another address too.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    server_id: Option<String>,
}

/// How a server is connected: its token if it has one (a paired one, or QC_API_TOKEN),
/// and the ids pairing and `/api/config` gave.
#[derive(Default, Clone)]
pub struct Connection {
    pub token: Option<String>,
    pub client_id: Option<String>,
    pub server_id: Option<String>,
}

impl std::fmt::Debug for Connection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Connection").field("token", &self.token.as_ref().map(|_| "…")).field("client_id", &self.client_id).field("server_id", &self.server_id).finish()
    }
}

/// One change to the servers file at a time (the window's poller runs on a thread).
static SERVERS_LOCK: Mutex<()> = Mutex::new(());

fn update(data_dir: &Path, change: impl FnOnce(&mut Servers) -> bool) -> Result<(), String> {
    let _held = SERVERS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut all = load(data_dir)?;
    if change(&mut all) {
        save(data_dir, &all)?;
    }
    Ok(())
}

fn servers_path(data_dir: &Path) -> PathBuf {
    data_dir.join(SERVERS_FILE)
}

fn load(data_dir: &Path) -> Result<Servers, String> {
    let path = servers_path(data_dir);
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Servers::default()),
        Err(e) => return Err(format!("can't read {}: {e}", path.display())),
    };
    tighten(&path);
    // serde's message could quote the file; it holds tokens.
    serde_json::from_slice(&bytes).map_err(|_| format!("{} isn't readable; remove it and connect again", path.display()))
}

/// Make the token file private again if something loosened it.
fn tighten(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if std::fs::metadata(path).is_ok_and(|m| m.permissions().mode() & 0o077 != 0) {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).ok();
        }
    }
}

fn save(data_dir: &Path, servers: &Servers) -> Result<(), String> {
    let path = servers_path(data_dir);
    let tmp = data_dir.join(format!("{SERVERS_FILE}.tmp"));
    let write = || -> std::io::Result<()> {
        std::fs::create_dir_all(data_dir)?;
        std::fs::remove_file(&tmp).ok();
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
        let mut f = opts.open(&tmp)?;
        f.write_all(&serde_json::to_vec_pretty(servers).map_err(std::io::Error::other)?)?;
        f.sync_all()?;
        std::fs::rename(&tmp, &path)
    };
    write().map_err(|e| {
        std::fs::remove_file(&tmp).ok();
        format!("can't save {}: {e}", path.display())
    })?;
    tighten(&path);
    Ok(())
}

pub fn check_token(token: &str) -> Result<(), String> {
    if token.is_empty() || token.len() > 4096 || !token.bytes().all(|b| b.is_ascii_graphic()) {
        return Err("the token must be 1 to 4096 printable characters with no spaces".into());
    }
    Ok(())
}

/// The token stored for a server (normalised URL), if it's connected. An open server
/// connected without one gives `""`: requests to it carry no credentials ([`get_with`]).
pub fn token_for(data_dir: &Path, server: &str) -> Result<Option<String>, String> {
    Ok(load(data_dir)?.servers.get(server).map(|e| e.token.clone().unwrap_or_default()))
}

/// How a server is connected, if it is.
pub fn connection(data_dir: &Path, server: &str) -> Result<Option<Connection>, String> {
    Ok(load(data_dir)?.servers.get(server).map(|e| Connection { token: e.token.clone(), client_id: e.client_id.clone(), server_id: e.server_id.clone() }))
}

/// Store a server's token. Returns the server's normalised URL.
pub fn connect(data_dir: &Path, server: &str, token: &str) -> Result<String, String> {
    check_token(token)?;
    connect_with(data_dir, server, Connection { token: Some(token.to_owned()), ..Connection::default() })
}

/// Store how a server is connected: with a token, or (`token: None`) an open server with
/// none. Returns the server's normalised URL.
pub fn connect_with(data_dir: &Path, server: &str, how: Connection) -> Result<String, String> {
    let server = normalize_server(server)?;
    if let Some(t) = &how.token {
        check_token(t)?;
    }
    let added = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    update(data_dir, |all| {
        // A new token for a server keeps where its new clips were last seen.
        let seen = all.servers.get(&server).and_then(|e| e.seen.clone());
        all.servers.insert(server.clone(), Entry { token: how.token, added, seen, client_id: how.client_id, server_id: how.server_id });
        true
    })?;
    Ok(server)
}

/// Forget a server's token. Returns whether it was connected.
pub fn disconnect(data_dir: &Path, server: &str) -> Result<bool, String> {
    let server = normalize_server(server)?;
    let mut had = false;
    update(data_dir, |all| {
        had = all.servers.remove(&server).is_some();
        had
    })?;
    Ok(had)
}

/// The connected servers and when each was connected; never their tokens.
pub fn list(data_dir: &Path) -> Result<Vec<(String, u64)>, String> {
    Ok(load(data_dir)?.servers.into_iter().map(|(s, e)| (s, e.added)).collect())
}

/// A connected server as `quip.list` shows it: never its token.
#[derive(Debug, Clone, PartialEq)]
pub struct Listed {
    pub server: String,
    /// When it was connected (Unix seconds).
    pub added: u64,
    /// Whether it has a token (else it's an open server, sent no credentials).
    pub token: bool,
    /// Whether the token came from pairing (and Disconnect revokes it).
    pub paired: bool,
    pub server_id: Option<String>,
}

/// The connected servers and how each is connected.
pub fn list_detail(data_dir: &Path) -> Result<Vec<Listed>, String> {
    Ok(load(data_dir)?
        .servers
        .into_iter()
        .map(|(server, e)| Listed { server, added: e.added, token: e.token.is_some(), paired: e.client_id.is_some(), server_id: e.server_id })
        .collect())
}

/// Where a server's new clips were last seen (`quip.new`), if ever.
pub fn seen_cursor(data_dir: &Path, server: &str) -> Result<Option<String>, String> {
    Ok(load(data_dir)?.servers.get(server).and_then(|e| e.seen.clone()))
}

/// Remember a server's clips library cursor as seen. A server that isn't connected
/// (any more) is left alone.
pub fn set_seen(data_dir: &Path, server: &str, cursor: &str) -> Result<(), String> {
    update(data_dir, |all| match all.servers.get_mut(server) {
        Some(e) if e.seen.as_deref() != Some(cursor) => {
            e.seen = Some(cursor.to_owned());
            true
        }
        _ => false,
    })
}

/// Move a server's seen cursor from `from` to `to`, only if it's still at `from` (so a
/// cursor moved on meanwhile, by the CLI or another copy of the app, isn't moved back).
/// Returns whether it moved.
pub fn advance_seen(data_dir: &Path, server: &str, from: &str, to: &str) -> Result<bool, String> {
    let mut moved = false;
    update(data_dir, |all| match all.servers.get_mut(server) {
        Some(e) if e.seen.as_deref() == Some(from) && from != to => {
            e.seen = Some(to.to_owned());
            moved = true;
            true
        }
        _ => false,
    })?;
    Ok(moved)
}

/// What a link from an unconnected server is refused with: how to connect it.
pub fn not_connected_message(server: &str) -> String {
    format!("Not connected to QuipClipper at {server}; nothing downloaded. Connect it in Settings ▸ QuipClipper, or: {} quip.pair {server}", crate::brand::CLI_NAME)
}

// ── HTTP ──────────────────────────────────────────────────────────────────────

/// A clip download, run on the store's import thread. Its token never shows in `Debug`.
pub struct Fetch {
    url: String,
    token: String,
    file: String,
    max_bytes: u64,
}

impl std::fmt::Debug for Fetch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Fetch").field("url", &self.url).field("file", &self.file).finish_non_exhaustive()
    }
}

fn agent() -> ureq::Agent {
    use ureq::tls::{RootCerts, TlsConfig};
    ureq::Agent::config_builder()
        .max_redirects(0)
        .http_status_as_error(false)
        .user_agent(format!("{}/{}", crate::brand::APP_NAME, env!("CARGO_PKG_VERSION")))
        .timeout_connect(Some(Duration::from_secs(10)))
        .timeout_recv_response(Some(Duration::from_secs(60)))
        .timeout_recv_body(Some(Duration::from_secs(30 * 60)))
        .tls_config(TlsConfig::builder().root_certs(RootCerts::PlatformVerifier).build())
        .build()
        .new_agent()
}

fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            out.push(if i <= c.len() { T[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
        }
    }
    out
}

/// A redirect may stay on the server (or move it from http to https), never leave it.
fn same_server(from: &Url, to: &Url) -> bool {
    from.host_str() == to.host_str()
        && ((from.scheme() == to.scheme() && from.port_or_known_default() == to.port_or_known_default())
            || (from.scheme() == "http" && to.scheme() == "https"))
}

/// GET `url` with the token, following redirects only within the server. The response
/// is whatever status the server gave; the error never holds the token.
fn get(url: &str, token: &str) -> Result<ureq::http::Response<ureq::Body>, String> {
    get_with(url, token, &[])
}

/// [`get`] with more headers (`If-None-Match`, `Accept`).
fn get_with(url: &str, token: &str, headers: &[(&str, &str)]) -> Result<ureq::http::Response<ureq::Body>, String> {
    let agent = agent();
    let mut url = Url::parse(url).map_err(|e| format!("bad URL {url}: {e}"))?;
    // QuipClipper takes the token as X-API-Key; the same token as the `api` user's
    // password also gets through its optional basic-auth gate (nginx, QC_PASSWORD). An
    // open server connected without a token (`""`) gets neither.
    let basic = format!("Basic {}", base64(format!("api:{token}").as_bytes()));
    for _ in 0..=MAX_REDIRECTS {
        let mut req = agent.get(url.as_str());
        if !token.is_empty() {
            req = req.header("X-API-Key", token).header("Authorization", &basic);
        }
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        let resp = req
            .call()
            .map_err(|e| format!("couldn't reach {}: {}", origin(&url), describe(&e)))?;
        // 304 Not Modified answers If-None-Match; it isn't a redirect.
        if !resp.status().is_redirection() || resp.status().as_u16() == 304 {
            return Ok(resp);
        }
        let to = resp.headers().get("location").and_then(|l| l.to_str().ok()).and_then(|l| url.join(l).ok());
        match to {
            Some(to) if same_server(&url, &to) => url = to,
            Some(to) => return Err(format!("the server sent the request on to {}, another host; it wasn't followed", origin(&to))),
            None => return Err(format!("the server answered HTTP {} without a usable Location", resp.status().as_u16())),
        }
    }
    Err("the server redirected too many times".into())
}

/// POST JSON to the server (pairing, D-133), with QuipClipper's CSRF header and no
/// credentials. No redirect is followed: an answer is whatever the server said.
fn post_json(url: &str, body: &Value, headers: &[(&str, &str)]) -> Result<(u16, Value), String> {
    let parsed = Url::parse(url).map_err(|e| format!("bad URL {url}: {e}"))?;
    let mut req = agent().post(parsed.as_str()).header("X-Quipclipper", "1").header("Content-Type", "application/json");
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let resp = req.send(body.to_string()).map_err(|e| format!("couldn't reach {}: {}", origin(&parsed), describe(&e)))?;
    answer(resp)
}

/// DELETE with the token (revoking a pairing), no redirect followed.
fn delete(url: &str, token: &str) -> Result<(u16, Value), String> {
    let parsed = Url::parse(url).map_err(|e| format!("bad URL {url}: {e}"))?;
    let resp = agent()
        .delete(parsed.as_str())
        .header("X-API-Key", token)
        .header("X-Quipclipper", "1")
        .call()
        .map_err(|e| format!("couldn't reach {}: {}", origin(&parsed), describe(&e)))?;
    answer(resp)
}

/// GET without credentials and without following redirects, as JSON: pairing's polls and
/// the probes of servers nobody has connected yet.
fn get_json(url: &str, headers: &[(&str, &str)]) -> Result<(u16, Value), String> {
    let parsed = Url::parse(url).map_err(|e| format!("bad URL {url}: {e}"))?;
    let mut req = agent().get(parsed.as_str());
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let resp = req.call().map_err(|e| format!("couldn't reach {}: {}", origin(&parsed), describe(&e)))?;
    answer(resp)
}

/// A response's status and JSON body (`null` when it has none or isn't JSON), read with
/// a small cap: these are small control answers, never clips.
fn answer(resp: ureq::http::Response<ureq::Body>) -> Result<(u16, Value), String> {
    let status = resp.status().as_u16();
    let body = resp.into_body().with_config().limit(256 * 1024).read_to_vec().unwrap_or_default();
    Ok((status, serde_json::from_slice(&body).unwrap_or(Value::Null)))
}

/// What `/api/config` says about a server, asked without credentials.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ServerInfo {
    /// It has a password gate (`auth_required`, or the gate itself answered 401).
    pub auth_required: bool,
    /// It offers pairing (`quipclipper.pair/1`).
    pub pairing: bool,
    pub server_id: Option<String>,
    pub server_name: Option<String>,
}

/// Ask a server nobody has connected yet what it is: only after the user said to connect
/// it. Not QuipClipper (no `/api/config`, or not its JSON) is an error.
pub fn server_info(server: &str) -> Result<ServerInfo, String> {
    let (status, v) = get_json(&format!("{server}/api/config"), &[])?;
    let short = |k: &str| v[k].as_str().map(str::trim).filter(|s| !s.is_empty() && s.len() <= 100 && !s.chars().any(char::is_control)).map(str::to_owned);
    match status {
        200 if v.get("media_roots").is_some() || v.get("auth_required").is_some() => Ok(ServerInfo {
            auth_required: v["auth_required"].as_bool().unwrap_or(false),
            pairing: v["pairing"].as_bool().unwrap_or(false),
            server_id: short("server_id"),
            server_name: short("server_name"),
        }),
        // The password gate (nginx) answers before QuipClipper does.
        401 | 403 => Ok(ServerInfo { auth_required: true, pairing: true, ..ServerInfo::default() }),
        200 | 404 => Err(format!("{server} doesn't look like QuipClipper (no /api/config); check the address")),
        s => Err(format!("the QuipClipper at {server} answered HTTP {s} for its settings")),
    }
}

fn origin(u: &Url) -> String {
    u.origin().ascii_serialization()
}

fn describe(e: &ureq::Error) -> String {
    match e {
        ureq::Error::Timeout(_) => "it took too long to answer".into(),
        ureq::Error::Io(io) => io.to_string(),
        other => other.to_string(),
    }
}

fn status_error(status: u16, server: &str, what: &str) -> String {
    match status {
        401 | 403 => format!(
            "the QuipClipper at {server} refused the token (HTTP {status}); connect it again with `{} quip.connect {server} --token <API token>`",
            crate::brand::CLI_NAME
        ),
        404 => format!("the QuipClipper at {server} has no {what} (HTTP 404)"),
        422 => format!("the QuipClipper at {server} couldn't convert the {what} to FLAC: it has no audio (HTTP 422)"),
        _ => format!("the QuipClipper at {server} answered HTTP {status} for {what}"),
    }
}

/// Check that `server` is a QuipClipper that takes `token` (`GET /api/config`).
pub fn check_server(server: &str, token: &str) -> Result<(), String> {
    let resp = get(&format!("{server}/api/config"), token)?;
    match resp.status().as_u16() {
        200..=299 => Ok(()),
        404 => Err(format!("{server} doesn't look like QuipClipper (no /api/config); check the URL")),
        // Not status_error's "connect it again": this is the connect.
        s @ (401 | 403) => Err(format!("the QuipClipper at {server} refused the token (HTTP {s}); check it against QC_API_TOKEN on the server")),
        s => Err(status_error(s, server, "its settings")),
    }
}

impl Fetch {
    pub fn new(link: &Link, token: String) -> Fetch {
        Fetch { url: link.download_url(), token, file: link.download_name(), max_bytes: MAX_BYTES }
    }

    /// The clip's file name, for messages and its extension.
    pub fn file_name(&self) -> &str {
        &self.file
    }

    /// Download the clip to `dest`, reporting progress and stopping when cancelled. A
    /// failed or cancelled download leaves nothing at `dest`.
    pub fn download(&self, dest: &Path, progress: &Progress) -> Result<(), String> {
        progress.set_downloading(true);
        progress.set_fraction(None);
        let res = self.fetch(dest, progress);
        progress.set_downloading(false);
        progress.set_fraction(None);
        if res.is_err() {
            std::fs::remove_file(dest).ok();
        }
        res
    }

    fn fetch(&self, dest: &Path, progress: &Progress) -> Result<(), String> {
        let server = self.url.split(DOWNLOAD_PATH).next().unwrap_or(&self.url).to_owned();
        let what = format!("clip “{}”", self.file);
        let resp = get(&self.url, &self.token)?;
        let status = resp.status().as_u16();
        if !(200..=299).contains(&status) {
            return Err(status_error(status, &server, &what));
        }
        let kind = resp.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("").to_ascii_lowercase();
        if kind.starts_with("text/html") || kind.starts_with("application/json") {
            return Err(format!("the QuipClipper at {server} sent a web page instead of the {what}"));
        }
        let too_big = || format!("the {what} is bigger than the {} MB limit", self.max_bytes / (1 << 20));
        let length = resp.body().content_length();
        if length.is_some_and(|n| n > self.max_bytes) {
            return Err(too_big());
        }
        let mut body = resp.into_body().into_reader();
        let mut out = std::fs::File::create(dest).map_err(|e| format!("saving the download: {e}"))?;
        let mut buf = vec![0u8; 64 * 1024];
        let mut got: u64 = 0;
        loop {
            if progress.is_cancelled() {
                return Err(CANCELLED.to_owned());
            }
            let n = body.read(&mut buf).map_err(|e| format!("the download of the {what} broke off: {e}"))?;
            if n == 0 {
                break;
            }
            got += n as u64;
            if got > self.max_bytes {
                return Err(too_big());
            }
            out.write_all(&buf[..n]).map_err(|e| format!("saving the download: {e}"))?;
            if let Some(len) = length.filter(|l| *l > 0) {
                progress.set_fraction(Some(got as f64 / len as f64));
            }
        }
        if got == 0 {
            return Err(format!("the QuipClipper at {server} sent an empty file for the {what}"));
        }
        out.sync_all().map_err(|e| format!("saving the download: {e}"))
    }
}

// ── links from the system ─────────────────────────────────────────────────────

static PENDING: Mutex<Vec<String>> = Mutex::new(Vec::new());
static WAKE: OnceLock<eframe::egui::Context> = OnceLock::new();

/// A link or set file the system opened us with (an Apple Event, or the command line at
/// a cold start): the window imports a link on its next frame, and asks before importing
/// a set file (an absolute path).
pub fn deliver(link: String) {
    PENDING.lock().unwrap_or_else(|e| e.into_inner()).push(link);
    if let Some(ctx) = WAKE.get() {
        ctx.request_repaint();
    }
}

/// The links and set files waiting for the window.
pub fn take_links() -> Vec<String> {
    std::mem::take(&mut *PENDING.lock().unwrap_or_else(|e| e.into_inner()))
}

/// A command-line argument naming a set file to open (a path, or the `file://` URL a
/// launcher passes for `%u`), made absolute so the running app finds it from its own
/// working directory. `None` for anything else, flags included.
pub fn opened_file(arg: &str) -> Option<PathBuf> {
    let path = if arg.starts_with("file:") {
        Url::parse(arg).ok()?.to_file_path().ok()?
    } else if arg.starts_with('-') {
        return None;
    } else {
        PathBuf::from(arg)
    };
    crate::setfile::is_set_file(&path).then(|| std::path::absolute(&path).unwrap_or(path))
}

/// Ask the window for a frame, so work finished off its thread shows (a no-op without
/// a window).
pub fn wake() {
    if let Some(ctx) = WAKE.get() {
        ctx.request_repaint();
    }
}

/// Ask the window for a frame in `after`, for work that's due then (a no-op without a
/// window).
pub fn wake_after(after: Duration) {
    if let Some(ctx) = WAKE.get() {
        ctx.request_repaint_after(after);
    }
}

/// An error from here as a sentence, for a message of its own.
pub fn sentence_case(e: &str) -> String {
    let mut c = e.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

/// Let [`deliver`] wake the window.
pub fn set_waker(ctx: eframe::egui::Context) {
    let _ = WAKE.set(ctx);
}

/// macOS hands `leadstinger://` links to the running app as `GURL` Apple Events, and
/// opened set files as `odoc` ones, at a cold start as well, once the app has launched.
/// The link handler is installed before the event loop starts, so the first link isn't
/// missed (winit's application delegate doesn't take URLs or files). AppKit installs its
/// own `odoc` handler while it finishes launching (which would answer that the app can't
/// open the file), so ours goes in when it says it will finish launching: after AppKit's,
/// and before the documents that launched the app are handed over.
#[cfg(target_os = "macos")]
pub fn listen_for_links() {
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, NSObject};
    use objc2::{define_class, msg_send, sel, AllocAnyThread};
    use objc2_foundation::{NSAppleEventDescriptor, NSAppleEventManager, NSNotification, NSNotificationCenter, NSString};

    const AEVT: u32 = u32::from_be_bytes(*b"aevt");
    const ODOC: u32 = u32::from_be_bytes(*b"odoc");
    fn listen_for_files(target: &AnyObject) {
        let manager = NSAppleEventManager::sharedAppleEventManager();
        // SAFETY: as for GURL below.
        unsafe {
            let _: () = msg_send![&*manager, setEventHandler: target, andSelector: sel!(handleOpenDocuments:withReplyEvent:), forEventClass: AEVT, andEventID: ODOC];
        }
    }

    define_class!(
        // SAFETY: NSObject has no subclassing requirements, and this has no Drop.
        #[unsafe(super(NSObject))]
        #[name = "SoundboardQuipLinkHandler"]
        struct LinkHandler;

        impl LinkHandler {
            #[unsafe(method(handleGetURLEvent:withReplyEvent:))]
            fn handle(&self, event: &NSAppleEventDescriptor, _reply: Option<&NSAppleEventDescriptor>) {
                const DIRECT_OBJECT: u32 = u32::from_be_bytes(*b"----");
                // SAFETY: a plain getter with a FourCharCode argument.
                let param: Option<Retained<NSAppleEventDescriptor>> = unsafe { msg_send![event, paramDescriptorForKeyword: DIRECT_OBJECT] };
                if let Some(s) = param.and_then(|p| p.stringValue()) {
                    deliver(s.to_string());
                }
            }

            #[unsafe(method(handleOpenDocuments:withReplyEvent:))]
            fn open_documents(&self, event: &NSAppleEventDescriptor, _reply: Option<&NSAppleEventDescriptor>) {
                const DIRECT_OBJECT: u32 = u32::from_be_bytes(*b"----");
                // SAFETY: as above.
                let Some(list): Option<Retained<NSAppleEventDescriptor>> = (unsafe { msg_send![event, paramDescriptorForKeyword: DIRECT_OBJECT] }) else { return };
                // A list of files, numbered from 1 (a single file may come bare).
                let n = list.numberOfItems();
                let items: Vec<_> = if n == 0 { vec![list] } else { (1..=n).filter_map(|i| list.descriptorAtIndex(i)).collect() };
                for path in items.iter().filter_map(|d| d.fileURLValue()?.path()) {
                    deliver(path.to_string());
                }
            }

            #[unsafe(method(willFinishLaunching:))]
            fn will_finish_launching(&self, _note: &NSNotification) {
                listen_for_files(self);
            }
        }
    );

    const GURL: u32 = u32::from_be_bytes(*b"GURL");
    let handler: Retained<LinkHandler> = unsafe { msg_send![LinkHandler::alloc(), init] };
    let manager = NSAppleEventManager::sharedAppleEventManager();
    let target: &AnyObject = &handler;
    // SAFETY: the handler answers this selector with the arguments it's called with;
    // the manager doesn't retain it, so it's kept for the app's life below.
    unsafe {
        let _: () = msg_send![&*manager, setEventHandler: target, andSelector: sel!(handleGetURLEvent:withReplyEvent:), forEventClass: GURL, andEventID: GURL];
        let name = NSString::from_str("NSApplicationWillFinishLaunchingNotification");
        NSNotificationCenter::defaultCenter().addObserver_selector_name_object(target, sel!(willFinishLaunching:), Some(&name), None);
    }
    std::mem::forget(handler);
}

/// The launcher entry that opens our links and set files with `exe`: hidden from menus,
/// since the packaged entry (or none, for an AppImage) is the one people start the app
/// from.
pub fn link_desktop_entry(exe: &Path) -> Result<String, String> {
    let exe = crate::launcher::exec_path(exe)?;
    Ok(format!(
        "[Desktop Entry]\nType=Application\nName={} (links and set files)\nExec=\"{exe}\" %u\nTerminal=false\nNoDisplay=true\nMimeType=x-scheme-handler/{};{};\n",
        crate::brand::APP_NAME,
        crate::brand::SLUG,
        crate::setfile::MIME,
    ))
}

/// `leadstinger --register-links`: make this program (the AppImage, when run from one)
/// open our links and set files, for installs without a packaged launcher entry. Writes
/// `<slug>-links.desktop` to the user's applications folder and the set file's MIME type
/// to `mime/packages/<slug>.xml`, and makes the entry the default for both; an AppImage
/// also gets its launcher entry and icon (src/launcher.rs). Returns what was done.
pub fn register_links() -> Result<String, String> {
    if cfg!(target_os = "macos") {
        return Ok(format!("macOS opens {}:// links with {} once the app is in Applications; nothing to do", crate::brand::SLUG, crate::brand::APP_NAME));
    }
    let appimage = std::env::var_os("APPIMAGE").map(PathBuf::from);
    let exe = match &appimage {
        Some(p) => p.clone(),
        None => crate::launcher::program_path(
            &std::env::current_exe().map_err(|e| format!("can't tell where this program is: {e}"))?,
            std::env::var_os("PATH").as_deref(),
        ),
    };
    let data = crate::launcher::data_home()?;
    // An AppImage has no launcher entry or icon outside it: add those too (D-131).
    if appimage.is_some() {
        crate::launcher::install(&data, &exe)?;
    }
    let apps = data.join("applications");
    let name = format!("{}-links.desktop", crate::brand::SLUG);
    let entry = link_desktop_entry(&exe)?;
    std::fs::create_dir_all(&apps).and_then(|_| std::fs::write(apps.join(&name), entry)).map_err(|e| format!("can't write {}: {e}", apps.join(&name).display()))?;
    let run = |cmd: &str, args: &[&std::ffi::OsStr]| std::process::Command::new(cmd).args(args).status().is_ok_and(|s| s.success());
    let mime_dir = data.join("mime");
    let package = mime_dir.join("packages").join(format!("{}.xml", crate::brand::SLUG));
    std::fs::create_dir_all(mime_dir.join("packages"))
        .and_then(|_| std::fs::write(&package, crate::setfile::MIME_XML))
        .map_err(|e| format!("can't write {}: {e}", package.display()))?;
    run("update-mime-database", &[mime_dir.as_os_str()]);
    run("update-desktop-database", &[apps.as_os_str()]);
    let scheme = format!("x-scheme-handler/{}", crate::brand::SLUG);
    let made_default = run("xdg-mime", &["default".as_ref(), name.as_ref(), scheme.as_ref(), crate::setfile::MIME.as_ref()]);
    Ok(format!(
        "{} now opens {}:// links and .{} set files (with {}){}",
        apps.join(&name).display(),
        crate::brand::SLUG,
        crate::setfile::EXTENSION,
        exe.display(),
        if made_default { "" } else { "; xdg-mime isn't installed, so your desktop may ask which app to use the first time" }
    ))
}

// ── commands ──────────────────────────────────────────────────────────────────

fn err(code: &'static str, message: impl Into<String>) -> CmdError {
    CmdError { code, message: message.into(), candidates: Vec::new() }
}

fn text<'a>(params: &'a Value, key: &str, id: &str) -> Result<Option<&'a str>, CmdError> {
    match params.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(_) => Err(err("bad_params", format!("{id}: {key} must be text"))),
    }
}

/// `start`/`end` as given: a number or text.
fn number(params: &Value, key: &str, id: &str) -> Result<Option<String>, CmdError> {
    match params.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) => Ok(Some(n.to_string())),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(err("bad_params", format!("{id}: {key} must be a number of seconds"))),
    }
}

/// The `quip.*` commands (src/commands.rs).
pub fn command(c: &mut Controller, id: &str, params: &Value) -> Result<Value, CmdError> {
    let data_dir = c.store.data_dir.clone();
    let failed = |e: String| err("failed", e);
    match id {
        "quip.connect" => {
            let server = text(params, "url", id)?.ok_or_else(|| err("bad_params", "quip.connect: give the server's URL, e.g. https://quip.example"))?;
            let server = normalize_server(server).map_err(|e| err("bad_params", e))?;
            let checked = !matches!(params.get("check"), Some(Value::Bool(false)));
            match text(params, "token", id)? {
                Some(token) => {
                    check_token(token).map_err(|e| err("bad_params", e))?;
                    if checked {
                        check_server(&server, token).map_err(failed)?;
                    }
                    connect(&data_dir, &server, token).map_err(failed)?;
                }
                // No token: an open server (D-132), so the check isn't optional.
                None => {
                    let info = server_info(&server).map_err(failed)?;
                    if info.auth_required {
                        return Err(failed(format!(
                            "the QuipClipper at {server} has a password: pair with it ({} quip.pair {server}) or give its API token (--token -)",
                            crate::brand::CLI_NAME
                        )));
                    }
                    connect_with(&data_dir, &server, Connection { server_id: info.server_id, ..Connection::default() }).map_err(failed)?;
                }
            }
            c.notify(format!("Connected to QuipClipper at {server}"));
            Ok(json!({"server": server, "checked": checked}))
        }
        "quip.disconnect" => {
            let server = text(params, "url", id)?.ok_or_else(|| err("bad_params", "quip.disconnect: give the server's URL"))?;
            let server = normalize_server(server).map_err(|e| err("bad_params", e))?;
            let how = connection(&data_dir, &server).map_err(failed)?;
            pair::cancel(&server);
            let removed = disconnect(&data_dir, &server).map_err(failed)?;
            // A paired token is revoked on the server too (best effort): off the window's
            // thread, which mustn't wait on a server that may be gone.
            let revoke = how.filter(|h| h.client_id.is_some()).and_then(|h| h.token);
            let revoked = match revoke {
                Some(token) if c.has_window => {
                    let s = server.clone();
                    std::thread::Builder::new().name("quip-revoke".into()).spawn(move || pair::revoke(&s, &token)).ok();
                    Value::Null
                }
                Some(token) => json!(pair::revoke(&server, &token)),
                None => Value::Null,
            };
            Ok(json!({"server": server, "removed": removed, "revoked": revoked}))
        }
        "quip.list" => {
            let servers = list_detail(&data_dir).map_err(failed)?;
            Ok(json!(servers.into_iter().map(|l| json!({"server": l.server, "added": l.added, "token": l.token, "paired": l.paired})).collect::<Vec<_>>()))
        }
        "quip.pair" => {
            let res = pair_command(c, &data_dir, id, params);
            if let Err(e) = &res {
                c.report(e);
            }
            res
        }
        "quip.pairing" => {
            let server = text(params, "url", id)?.map(normalize_server).transpose().map_err(|e| err("bad_params", e))?;
            Ok(pair::json(server.as_deref()))
        }
        "quip.discover" => {
            let again = !matches!(params.get("again"), Some(Value::Bool(false)));
            if again {
                discover::start();
            }
            Ok(discover::json(&data_dir))
        }
        "quip.import" => {
            let res = import(c, &data_dir, id, params);
            if let Err(e) = &res {
                c.report(e);
            }
            res
        }
        "quip.browse" | "quip.search" | "quip.new" | "quip.add" | "quip.preview" => clips::command(c, id, params),
        _ => Err(err("unknown_command", format!("no command {id:?}"))),
    }
}

fn import(c: &mut Controller, data_dir: &Path, id: &str, params: &Value) -> Result<Value, CmdError> {
    let raw = text(params, "link", id)?.ok_or_else(|| err("bad_params", "quip.import: give a link or a clip URL"))?;
    let bad_link = |e: String| err("bad_params", format!("This QuipClipper link can't be used: {e}"));
    let mut link = Link::parse(raw).map_err(bad_link)?;
    let (start, end) = (number(params, "start", id)?, number(params, "end", id)?);
    link.apply(text(params, "title", id)?, start.as_deref(), end.as_deref(), text(params, "set", id)?).map_err(bad_link)?;
    if let Some(a) = text(params, "as", id)? {
        link.set_as(a).map_err(|e| err("bad_params", format!("quip.import: {e}")))?;
    }
    link.check_set().map_err(bad_link)?;
    let notes = text(params, "notes", id)?.map(str::trim).filter(|n| !n.is_empty()).map(str::to_owned);
    if notes.is_some() && link.is_set() {
        return Err(bad_link("a link to a set file takes no notes".into()));
    }
    if notes.as_ref().is_some_and(|n| n.chars().count() > 2000) {
        return Err(err("bad_params", "quip.import: notes must be at most 2000 characters"));
    }
    let raise = params.get("raise") == Some(&Value::Bool(true));
    // A link the system opened (`ask`), with a window to ask in: a server that isn't
    // connected asks before anything goes over the network (D-132), and with
    // settings.quipAskEachImport every link does (D-135).
    if params.get("ask") == Some(&Value::Bool(true)) && c.has_window {
        let connected = token_for(data_dir, &link.server).map_err(|e| err("failed", e))?.is_some();
        if !connected || c.store.library.quip_ask_each {
            let server = link.server.clone();
            c.announce(if connected { format!("QuipClipper at {} sent {}. Import it?", host_of(&server), link.described()) } else { format!("Connect to QuipClipper at {}?", host_of(&server)) });
            c.effects.push(Effect::ConfirmQuip(Box::new(Ask { server: server.clone(), link: Some(link), notes, connected })));
            if raise {
                c.effects.push(Effect::Raise);
            }
            return Ok(json!({"asked": true, "server": server, "connected": connected}));
        }
    }
    start_import(c, data_dir, link, notes, raise)
}

/// A link (or a pair link's Connect) the window asks about before acting on it: whether to
/// connect its server, and, with a clip, to import it after (D-132, D-135).
#[derive(Debug, Clone, PartialEq)]
pub struct Ask {
    pub server: String,
    /// The clip a link sent; none for a pair link.
    pub link: Option<Link>,
    pub notes: Option<String>,
    /// The server is connected already (settings.quipAskEachImport asked).
    pub connected: bool,
}

/// A server's host (and port, if it has one), for prompts: what the user has to recognise.
pub fn host_of(server: &str) -> String {
    Url::parse(server).ok().and_then(|u| u.host_str().map(|h| match u.port() {
        Some(p) => format!("{h}:{p}"),
        None => h.to_owned(),
    })).unwrap_or_else(|| server.to_owned())
}

/// Where a server is, as the first-contact prompt says it, so a link from a web page to a
/// server somewhere unexpected stands out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Whereabouts {
    /// This machine.
    ThisComputer,
    /// A private address, a `.local` name or a one-word host name.
    LocalNetwork,
    /// Tailscale: `*.ts.net`, or 100.64.0.0/10.
    Tailnet,
    Internet,
}

impl Whereabouts {
    pub fn of(server: &str) -> Whereabouts {
        use std::net::IpAddr;
        let Some(host) = Url::parse(server).ok().and_then(|u| u.host_str().map(|h| h.trim_matches(['[', ']']).to_ascii_lowercase())) else {
            return Whereabouts::Internet;
        };
        match host.parse::<IpAddr>() {
            Ok(ip) if ip.is_loopback() => Whereabouts::ThisComputer,
            Ok(IpAddr::V4(v4)) if v4.octets()[0] == 100 && (64..128).contains(&v4.octets()[1]) => Whereabouts::Tailnet,
            Ok(IpAddr::V4(v4)) if v4.is_private() || v4.is_link_local() => Whereabouts::LocalNetwork,
            // fc00::/7 (unique local) and fe80::/10 (link local).
            Ok(IpAddr::V6(v6)) if (v6.segments()[0] & 0xfe00) == 0xfc00 || (v6.segments()[0] & 0xffc0) == 0xfe80 => Whereabouts::LocalNetwork,
            Ok(_) => Whereabouts::Internet,
            Err(_) if host == "localhost" || host.ends_with(".localhost") => Whereabouts::ThisComputer,
            Err(_) if host.ends_with(".ts.net") => Whereabouts::Tailnet,
            Err(_) if host.ends_with(".local") || host.ends_with(".lan") || host.ends_with(".home.arpa") || !host.contains('.') => Whereabouts::LocalNetwork,
            Err(_) => Whereabouts::Internet,
        }
    }

    pub fn describe(self) -> &'static str {
        match self {
            Whereabouts::ThisComputer => "on this computer",
            Whereabouts::LocalNetwork => "on your local network",
            Whereabouts::Tailnet => "on your tailnet",
            Whereabouts::Internet => "on the internet",
        }
    }
}

/// Plain http to a server on the internet: worth a warning before connecting.
pub fn unencrypted_on_internet(server: &str) -> bool {
    server.starts_with("http://") && Whereabouts::of(server) == Whereabouts::Internet
}

/// `quip.pair`: connect a server by pairing with it (or, an open server with no pairing,
/// without a token). With `ask` (a pair link the system opened) the window asks first.
fn pair_command(c: &mut Controller, data_dir: &Path, id: &str, params: &Value) -> Result<Value, CmdError> {
    if let Some(e) = text(params, "link_error", id)? {
        return Err(err("bad_params", format!("This QuipClipper link can't be used: {e}")));
    }
    let server = text(params, "url", id)?.ok_or_else(|| err("bad_params", "quip.pair: give the server's URL, e.g. https://quip.example"))?;
    let server = normalize_server(server).map_err(|e| err("bad_params", e))?;
    let raise = params.get("raise") == Some(&Value::Bool(true)) && c.has_window;
    if params.get("cancel") == Some(&Value::Bool(true)) {
        return Ok(json!({"server": server, "cancelled": pair::cancel(&server)}));
    }
    if token_for(data_dir, &server).map_err(|e| err("failed", e))?.is_some() {
        c.notify(format!("Already connected to QuipClipper at {server}"));
        if raise {
            c.effects.push(Effect::Raise);
        }
        return Ok(json!({"server": server, "status": "connected", "already": true}));
    }
    if params.get("ask") == Some(&Value::Bool(true)) {
        if !c.has_window {
            return Err(err("not_connected", format!("Open {} to connect QuipClipper at {server}", crate::brand::APP_NAME)));
        }
        c.announce(format!("Connect to QuipClipper at {}?", host_of(&server)));
        c.effects.push(Effect::ConfirmQuip(Box::new(Ask { server: server.clone(), link: None, notes: None, connected: false })));
        if raise {
            c.effects.push(Effect::Raise);
        }
        return Ok(json!({"asked": true, "server": server}));
    }
    pair::start(data_dir, &server);
    Ok(pair::json(Some(&server)).as_array().and_then(|a| a.first().cloned()).unwrap_or_else(|| json!({"server": server, "status": "starting"})))
}

/// Download `link` into a new tile (in the background), once its server is known to be
/// connected; a set link's set file is imported as a set ([`clips::add_set`]).
pub(crate) fn start_import(c: &mut Controller, data_dir: &Path, link: Link, notes: Option<String>, raise: bool) -> Result<Value, CmdError> {
    // Before anything goes over the network.
    let Some(token) = token_for(data_dir, &link.server).map_err(|e| err("failed", e))? else {
        return Err(err("not_connected", not_connected_message(&link.server)));
    };
    if link.is_set() {
        let r = clips::add_set(c, data_dir, &link.server, &token, &link.clip, &link.tile_title());
        if raise && c.has_window {
            c.effects.push(Effect::Raise);
        }
        return r;
    }
    let set_idx = match &link.set {
        None => c.set_idx(),
        Some(name) => match c.store.library.sets.iter().position(|s| s.name == *name) {
            Some(i) => i,
            None => {
                c.store.library.sets.push(SoundSet::named(name.clone()));
                c.store.library.sets.len() - 1
            }
        },
    };
    let title = link.tile_title();
    let tile = c.store.import_download(Fetch::new(&link, token), &title, set_idx);
    if let Some(t) = c.store.library.tile_mut(tile) {
        t.start = link.start.unwrap_or(0.0);
        t.end = link.end;
        if let Some(n) = &notes {
            t.notes = n.clone();
        }
    }
    let set = c.store.library.sets[set_idx].name.clone();
    c.notify(format!("Downloading “{title}” from QuipClipper into {set}"));
    if raise && c.has_window {
        c.effects.push(Effect::Raise);
    }
    Ok(json!({
        "tile": tile, "title": title, "set": set, "server": link.server, "clip": link.clip,
        "trimStart": link.start.unwrap_or(0.0), "trimEnd": link.end, "flac": link.flac,
    }))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("sb-quip-{name}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn parses_good_links() {
        let l = Link::parse("leadstinger://quip?server=https%3A%2F%2Fquip.example%2F&clip=Heat%2FHeat%20-%20line.mkv&title=Don%27t%20let%20yourself%20get%20attached&start=2.0&end=5.25&set=Movie%20Lines").unwrap();
        assert_eq!(l.server, "https://quip.example");
        assert_eq!(l.clip, "Heat/Heat - line.mkv");
        assert_eq!(l.title.as_deref(), Some("Don't let yourself get attached"));
        assert_eq!((l.start, l.end), (Some(2.0), Some(5.25)));
        assert_eq!(l.set.as_deref(), Some("Movie Lines"));
        assert_eq!(l.download_url(), "https://quip.example/api/clips/download/Heat/Heat%20-%20line.mkv");
        // Round trip, and only server and clip are needed.
        assert_eq!(Link::parse(&l.to_link()).unwrap(), l);
        let min = Link::parse("leadstinger://quip?server=http://Quip.Example:8896/qc/&clip=a.wav").unwrap();
        assert_eq!(min.server, "http://quip.example:8896/qc");
        assert_eq!((&min.title, min.start, min.end, &min.set), (&None, None, None, &None));
        assert_eq!(min.tile_title(), "a");
        // The default port goes; + is a space, as in any query.
        assert_eq!(Link::parse("LeadStinger://quip/?server=https://quip.example:443&clip=a+b.wav").unwrap().clip, "a b.wav");
        // A clip URL, as quip.import takes it.
        let u = Link::parse("https://quip.example/api/clips/download/Heat/Heat%20-%20line.mkv").unwrap();
        assert_eq!((u.server.as_str(), u.clip.as_str()), ("https://quip.example", "Heat/Heat - line.mkv"));
        assert_eq!(normalize_server("https://quip.example/api/clips/download/").unwrap(), "https://quip.example");
        // A set file: the whole set, with only server and clip.
        let set = Link::parse("leadstinger://quip?server=https://quip.example&clip=LeadStinger%2FMorning%20lines.LSSET").unwrap();
        assert!(set.is_set() && !min.is_set() && !Link::parse("leadstinger://quip?server=https://q.example&clip=a.zip").unwrap().is_set());
        assert_eq!(set.described(), "the set “Morning lines”");
        assert_eq!(Link::parse(&set.to_link()).unwrap(), set);
        assert!(Link::parse("https://quip.example/api/clips/download/LeadStinger/a.lsset").unwrap().is_set());
    }

    #[test]
    fn refuses_bad_links() {
        let bad = |s: &str, why: &str| {
            let e = Link::parse(s).unwrap_err();
            assert!(e.contains(why), "{s}: {e}");
        };
        bad("leadstinger://quip?clip=a.wav", "no server");
        bad("leadstinger://quip?server=https://q.example", "no clip");
        bad("leadstinger://other?server=https://q.example&clip=a.wav", "must start");
        bad("leadstinger://quip/x?server=https://q.example&clip=a.wav", "must start");
        bad("leadstinger://quip?server=https://q.example&clip=a.wav#x", "fragment");
        bad("leadstinger://quip?server=https://q.example&clip=a.wav&volume=1", "unknown parameter");
        bad("leadstinger://quip?server=https://q.example&clip=a.wav&clip=b.wav", "twice");
        bad("leadstinger://quip?server=file:///etc&clip=a.wav", "http or https");
        bad("leadstinger://quip?server=ftp://q.example&clip=a.wav", "http or https");
        bad("leadstinger://quip?server=https://me:pw@q.example&clip=a.wav", "user name");
        bad("leadstinger://quip?server=https://q.example/?x=1&clip=a.wav", "query");
        bad("leadstinger://quip?server=https://q.example&clip=../etc/passwd", "relative");
        bad("leadstinger://quip?server=https://q.example&clip=/etc/passwd", "relative");
        bad("leadstinger://quip?server=https://q.example&clip=a//b.wav", "relative");
        bad("leadstinger://quip?server=https://q.example&clip=a%5Cb.wav", "backslash");
        bad("leadstinger://quip?server=https://q.example&clip=a%0A.wav", "control");
        bad("leadstinger://quip?server=https://q.example&clip=a.wav&start=abc", "start");
        bad("leadstinger://quip?server=https://q.example&clip=a.wav&end=NaN", "end");
        bad("leadstinger://quip?server=https://q.example&clip=a.wav&start=-1", "start");
        bad("leadstinger://quip?server=https://q.example&clip=a.wav&start=5&end=2", "after the start");
        bad("leadstinger://quip?server=https://q.example&clip=a.wav&title=%20", "title");
        bad("leadstinger://quip?server=https://q.example&clip=a.wav&set=a%0Ab", "set");
        bad("https://q.example/clips/a.wav", "no /api/clips/download/");
        for extra in ["title=x", "start=1", "end=2", "set=Lines", "as=flac"] {
            bad(&format!("leadstinger://quip?server=https://q.example&clip=a.lsset&{extra}"), "imports the whole set and takes no");
        }
        bad("leadstinger://quip?server=https://q.example&clip=a.lsset&start=1&end=2", "takes no start, end");
        bad("https://q.example/api/clips/download/a.lsset?as=flac", "takes no as");
        bad("https://q.example/api/clips/download/a.wav?x=1", "query");
        bad("https://q.example/api/clips/download/a%ZZ.wav", "escape");
        bad("javascript:alert(1)", "not a");
        assert!(is_link("leadstinger://quip?x") && is_link(&format!("{}://quip", crate::brand::SLUG)) && !is_link("https://x"));
    }

    #[test]
    fn tokens_are_private_and_kept_apart() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp("tokens");
        assert_eq!(connect(&dir, "https://Quip.Example/", "s3cret-token").unwrap(), "https://quip.example");
        let path = dir.join(SERVERS_FILE);
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(token_for(&dir, "https://quip.example").unwrap().as_deref(), Some("s3cret-token"));
        assert_eq!(list(&dir).unwrap().iter().map(|(s, _)| s.as_str()).collect::<Vec<_>>(), ["https://quip.example"]);
        // A loosened file is made private again when it's read.
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        token_for(&dir, "https://quip.example").unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        assert!(check_token("has space").is_err() && check_token("").is_err());
        assert!(disconnect(&dir, "https://quip.example/").unwrap());
        assert!(!disconnect(&dir, "https://quip.example").unwrap());
        assert_eq!(token_for(&dir, "https://quip.example").unwrap(), None);
        // Never in Debug output.
        let link = Link::parse("leadstinger://quip?server=https://quip.example&clip=a.wav").unwrap();
        assert!(!format!("{:?}", Fetch::new(&link, "s3cret-token".into())).contains("s3cret"));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn writes_a_launcher_entry_for_links() {
        let e = link_desktop_entry(Path::new("/home/me/Apps/LeadStinger 0.3.AppImage")).unwrap();
        assert!(e.contains("Exec=\"/home/me/Apps/LeadStinger 0.3.AppImage\" %u\n"), "{e}");
        assert!(e.contains(&format!("MimeType=x-scheme-handler/{};application/x-leadstinger-set;", crate::brand::SLUG)) && e.contains("NoDisplay=true"));
        assert!(link_desktop_entry(Path::new("/tmp/$x")).is_err());
    }

    #[test]
    fn set_files_are_opened_by_path_or_file_url() {
        let abs = |p: &str| std::path::absolute(p).unwrap();
        assert_eq!(opened_file("/home/me/Show.lsset"), Some(PathBuf::from("/home/me/Show.lsset")));
        assert_eq!(opened_file("Show.LSSET"), Some(abs("Show.LSSET")));
        assert_eq!(opened_file("file:///home/me/My%20Show.lsset"), Some(PathBuf::from("/home/me/My Show.lsset")));
        for not in ["--register-links", "-h", "notes.txt", "file:///home/me/clip.wav", "leadstinger://quip?server=x"] {
            assert_eq!(opened_file(not), None, "{not}");
        }
        // The launcher entries and the MIME package claim the same type.
        assert!(crate::setfile::MIME_XML.contains(&format!("type=\"{}\"", crate::setfile::MIME)));
        assert!(crate::setfile::MIME_XML.contains(&format!("pattern=\"*.{}\"", crate::setfile::EXTENSION)));
        let entry = include_str!("../assets/linux/leadstinger.desktop");
        assert!(entry.contains(&format!(";{};\n", crate::setfile::MIME)), "{entry}");
    }

    #[test]
    fn base64_matches_the_standard() {
        assert_eq!(base64(b"api:tok"), "YXBpOnRvaw==");
        assert_eq!(base64(b"api:to"), "YXBpOnRv");
        assert_eq!(base64(b"api:t"), "YXBpOnQ=");
    }

    /// A tiny HTTP server: answers each connection by calling `reply` with the request
    /// line and headers. Returns its base URL.
    pub(crate) fn serve(reply: impl Fn(&str, &[String]) -> Vec<u8> + Send + 'static) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut request = String::new();
                reader.read_line(&mut request).ok();
                let mut headers = Vec::new();
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line.trim().is_empty() {
                        break;
                    }
                    headers.push(line.trim().to_owned());
                }
                stream.write_all(&reply(request.trim(), &headers)).ok();
            }
        });
        base
    }

    pub(crate) fn response(status: &str, extra: &str, body: &[u8]) -> Vec<u8> {
        let mut r = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n{extra}\r\n", body.len()).into_bytes();
        r.extend_from_slice(body);
        r
    }

    fn tone() -> Vec<u8> {
        let p = std::env::temp_dir().join(format!("sb-quip-tone-{}.wav", uuid::Uuid::new_v4()));
        crate::wav::write_tone(&p, 8000, 3.0, &[440.0], 0.4).unwrap();
        let bytes = std::fs::read(&p).unwrap();
        std::fs::remove_file(p).ok();
        bytes
    }

    pub(crate) fn controller() -> (Controller, PathBuf) {
        let root = temp("ctl");
        let store = crate::store::Store::open(root.join("data")).unwrap();
        (Controller::new(store, crate::audio::Engine::disabled("test")), root)
    }

    #[test]
    fn links_from_unconnected_servers_are_refused_without_a_fetch() {
        let hits = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let h = hits.clone();
        let base = serve(move |_, _| {
            h.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            response("200 OK", "", b"x")
        });
        let (mut c, root) = controller();
        let link = format!("leadstinger://quip?server={base}&clip=a.wav");
        let e = c.execute("quip.import", json!({"link": link})).unwrap_err();
        assert_eq!(e.code, "not_connected");
        assert!(e.message.contains("quip.pair") && e.message.contains(&base), "{}", e.message);
        assert_eq!(c.notice.as_ref().map(|n| n.text.clone()), Some(e.message.clone()), "shown in the window too");
        assert_eq!(c.store.library.sets.iter().map(|s| s.tiles.len()).sum::<usize>(), 0);
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 0, "nothing was fetched");
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn downloads_a_clip_into_a_trimmed_tile() {
        let wav = tone();
        let seen = std::sync::Arc::new(Mutex::new(Vec::<String>::new()));
        let s = seen.clone();
        let base = serve(move |req, headers| {
            s.lock().unwrap().push(req.to_owned());
            let key_ok = headers.iter().any(|h| h == "X-API-Key: tok3n" || h == "x-api-key: tok3n");
            let basic_ok = headers.iter().any(|h| h.eq_ignore_ascii_case("authorization: Basic YXBpOnRvazNu"));
            if !(key_ok && basic_ok) {
                return response("401 Unauthorized", "", b"{}");
            }
            match req {
                "GET /qc/api/config HTTP/1.1" => response("200 OK", "Content-Type: application/json\r\n", b"{}"),
                "GET /qc/api/clips/download/Heat/Heat%20line.wav HTTP/1.1" => response("302 Found", "Location: /qc/api/clips/download/Heat/final.wav\r\n", b""),
                "GET /qc/api/clips/download/Heat/final.wav HTTP/1.1" => response("200 OK", "Content-Type: audio/wav\r\n", &wav),
                _ => response("404 Not Found", "", b"{}"),
            }
        });
        let server = format!("{base}/qc");
        let (mut c, root) = controller();
        // A wrong token is caught at connect.
        let e = c.execute("quip.connect", json!({"url": server, "token": "wrong"})).unwrap_err();
        assert!(e.message.contains("refused the token") && !e.message.contains("wrong"), "{}", e.message);
        assert!(quip_list(&mut c).is_empty());
        c.execute("quip.connect", json!({"url": format!("{server}/"), "token": "tok3n"})).unwrap();
        assert_eq!(quip_list(&mut c), std::slice::from_ref(&server));
        assert!(!std::fs::read_to_string(c.store.library_path()).unwrap_or_default().contains("tok3n"));

        let link = Link { server: server.clone(), clip: "Heat/Heat line.wav".into(), title: Some("Get attached".into()), start: Some(0.5), end: Some(2.25), set: Some("Quips".into()), flac: false };
        let r = c.execute("quip.import", json!({"link": link.to_link()})).unwrap();
        assert_eq!((r["title"].as_str(), r["set"].as_str(), r["trimEnd"].as_f64()), (Some("Get attached"), Some("Quips"), Some(2.25)));
        let id: uuid::Uuid = serde_json::from_value(r["tile"].clone()).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while (c.store.importing().count() > 0 || c.store.is_loading()) && std::time::Instant::now() < deadline {
            c.store.poll();
            std::thread::sleep(Duration::from_millis(10));
        }
        let (si, ti) = c.store.library.locate(id).unwrap();
        let set = &c.store.library.sets[si];
        assert_eq!(set.name, "Quips", "a new set");
        let t = &set.tiles[ti];
        assert_eq!((t.title.as_str(), t.start, t.end), ("Get attached", 0.5, Some(2.25)));
        assert!(matches!(c.store.media(t), Some(crate::store::Media::Ready { .. })), "decoded");
        assert!((t.duration.unwrap() - 3.0).abs() < 0.01);
        assert!(seen.lock().unwrap().iter().any(|r| r.contains("final.wav")), "followed the same-server redirect");

        // Undoable like any import.
        c.execute("edit.undo", json!({})).unwrap();
        assert!(c.store.library.locate(id).is_none() && !c.store.library.sets.iter().any(|s| s.name == "Quips"));
        // A 404 fails the tile, not the command.
        let r = c.execute("quip.import", json!({"link": format!("{server}/api/clips/download/missing.wav")})).unwrap();
        let id: uuid::Uuid = serde_json::from_value(r["tile"].clone()).unwrap();
        c.store.finish_imports();
        let t = c.store.library.tile(id).unwrap();
        assert!(matches!(c.store.media(t), Some(crate::store::Media::Failed(e)) if e.contains("HTTP 404")));
        assert!(!std::fs::read_dir(&c.store.media_dir).unwrap().flatten().any(|e| e.file_name().to_string_lossy().ends_with(".part")));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn an_open_server_connects_without_a_token_and_gets_no_credentials() {
        let wav = tone();
        let creds = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let n = creds.clone();
        let open = serve(move |req, headers| {
            if headers.iter().any(|h| h.to_ascii_lowercase().starts_with("x-api-key") || h.to_ascii_lowercase().starts_with("authorization")) {
                n.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
            match req {
                "GET /api/config HTTP/1.1" => response("200 OK", "", br#"{"media_roots": [], "auth_required": false, "server_id": "s1"}"#),
                "GET /api/clips/download/a.wav HTTP/1.1" => response("200 OK", "Content-Type: audio/wav\r\n", &wav),
                _ => response("404 Not Found", "", b"{}"),
            }
        });
        let gated = serve(|_, _| response("200 OK", "", br#"{"media_roots": [], "auth_required": true}"#));
        let (mut c, root) = controller();
        let e = c.execute("quip.connect", json!({"url": gated})).unwrap_err();
        assert!(e.message.contains("has a password") && e.message.contains("quip.pair"), "{}", e.message);
        c.execute("quip.connect", json!({"url": open})).unwrap();
        let listed = c.execute("quip.list", json!({})).unwrap();
        assert_eq!((listed[0]["token"].as_bool(), listed[0]["paired"].as_bool()), (Some(false), Some(false)));
        assert_eq!(connection(&c.store.data_dir, &open).unwrap().unwrap().server_id.as_deref(), Some("s1"));
        let r = c.execute("quip.import", json!({"link": format!("{open}/api/clips/download/a.wav")})).unwrap();
        let id: uuid::Uuid = serde_json::from_value(r["tile"].clone()).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while (c.store.importing().count() > 0 || c.store.is_loading()) && std::time::Instant::now() < deadline {
            c.store.poll();
            std::thread::sleep(Duration::from_millis(10));
        }
        let media = c.store.media(c.store.library.tile(id).unwrap());
        let failed = match media {
            Some(crate::store::Media::Failed(e)) => e.clone(),
            _ => String::new(),
        };
        assert!(matches!(media, Some(crate::store::Media::Ready { .. })), "{failed}");
        assert_eq!(creds.load(std::sync::atomic::Ordering::SeqCst), 0, "no credentials went to it");
        // The file still reads in the old shape (a token string) and the new one.
        let file = std::fs::read_to_string(c.store.data_dir.join(SERVERS_FILE)).unwrap();
        assert!(!file.contains("\"token\""), "{file}");
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn old_servers_files_still_read() {
        let dir = temp("old");
        std::fs::write(dir.join(SERVERS_FILE), r#"{"servers": {"https://quip.example": {"token": "t0k", "added": 5, "seen": "c1"}}}"#).unwrap();
        assert_eq!(token_for(&dir, "https://quip.example").unwrap().as_deref(), Some("t0k"));
        assert_eq!(seen_cursor(&dir, "https://quip.example").unwrap().as_deref(), Some("c1"));
        let l = &list_detail(&dir).unwrap()[0];
        assert!(l.token && !l.paired && l.server_id.is_none());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn opened_links_ask_first_and_fetch_nothing_until_then() {
        let hits = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let h = hits.clone();
        let base = serve(move |_, _| {
            h.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            response("200 OK", "", b"x")
        });
        let (mut c, root) = controller();
        let link = format!("leadstinger://quip?server={base}&clip=a.wav&title=Hold%20on");
        let (id, params) = opened_command(&link);
        assert_eq!((id, params["ask"].as_bool(), params["raise"].as_bool()), ("quip.import", Some(true), Some(true)));
        // With a window: asked, in the window and out loud.
        let r = c.execute(id, params.clone()).unwrap();
        assert_eq!((r["asked"].as_bool(), r["connected"].as_bool()), (Some(true), Some(false)));
        let asked = c.effects.iter().find_map(|e| match e {
            Effect::ConfirmQuip(a) => Some(a.clone()),
            _ => None,
        });
        let asked = asked.expect("the window asks");
        assert_eq!((asked.server.as_str(), asked.link.as_ref().map(Link::tile_title)), (base.as_str(), Some("Hold on".into())));
        assert!(c.effects.contains(&Effect::Raise));
        assert!(c.announcement.as_ref().is_some_and(|a| a.text.starts_with("Connect to QuipClipper at 127.0.0.1")));
        // Without a window, and from scripts (no ask): refused, as before.
        c.has_window = false;
        assert_eq!(c.execute(id, params).unwrap_err().code, "not_connected");
        c.has_window = true;
        assert_eq!(c.execute("quip.import", json!({"link": link})).unwrap_err().code, "not_connected");
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 0, "nothing went over the network");

        // Connected, with settings.quipAskEachImport: asked too; without it, imported.
        connect_with(&c.store.data_dir, &base, Connection::default()).unwrap();
        c.effects.clear();
        c.execute("settings.quipAskEachImport", json!({"on": true})).unwrap();
        let r = c.execute("quip.import", json!({"link": link, "ask": true})).unwrap();
        assert_eq!((r["asked"].as_bool(), r["connected"].as_bool()), (Some(true), Some(true)));
        c.execute("settings.quipAskEachImport", json!({"on": false})).unwrap();
        let r = c.execute("quip.import", json!({"link": link, "ask": true})).unwrap();
        assert!(r["tile"].is_string(), "{r}");
        c.store.finish_imports();
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn pair_links_name_a_server_and_nothing_else() {
        assert_eq!(parse_pair_link("leadstinger://pair?server=https%3A%2F%2FQuip.Example%2Fqc%2F").unwrap(), "https://quip.example/qc");
        assert_eq!(parse_pair_link(&format!("{}://pair/?server=http://nas.local:8896", crate::brand::SLUG)).unwrap(), "http://nas.local:8896");
        for (bad, why) in [
            ("leadstinger://pair", "no server"),
            ("leadstinger://pair?server=https://a.example&clip=x", "unknown parameter"),
            ("leadstinger://pair?server=https://a.example&server=https://b.example", "twice"),
            ("leadstinger://pair?server=ftp://a.example", "http or https"),
            ("leadstinger://pair/x?server=https://a.example", "must start"),
            ("leadstinger://pair?server=https://a.example#x", "fragment"),
            ("https://pair?server=https://a.example", "must start"),
        ] {
            let e = parse_pair_link(bad).unwrap_err();
            assert!(e.contains(why), "{bad}: {e}");
        }
        assert_eq!(opened_command("leadstinger://pair?server=https://a.example").0, "quip.pair");
        assert!(opened_command("leadstinger://pair?server=x").1["link_error"].is_string());
        assert_eq!(opened_command("/home/me/Show.lsset").0, "set.import");

        // A pair link asks in the window; one for a connected server just says so.
        let (mut c, root) = controller();
        let r = c.execute("quip.pair", json!({"url": "https://a.example", "ask": true})).unwrap();
        assert_eq!(r["asked"].as_bool(), Some(true));
        assert!(c.effects.iter().any(|e| matches!(e, Effect::ConfirmQuip(a) if a.link.is_none() && a.server == "https://a.example")));
        connect_with(&c.store.data_dir, "https://a.example", Connection::default()).unwrap();
        let r = c.execute("quip.pair", json!({"url": "https://a.example", "ask": true})).unwrap();
        assert_eq!(r["already"].as_bool(), Some(true));
        let e = c.execute("quip.pair", json!({"link_error": "the link has no server"})).unwrap_err();
        assert!(e.message.contains("can't be used"), "{}", e.message);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn says_where_a_server_is() {
        use Whereabouts::*;
        for (s, w) in [
            ("http://127.0.0.1:8896", ThisComputer),
            ("http://localhost:8896", ThisComputer),
            ("http://192.168.1.20:8896", LocalNetwork),
            ("http://10.0.0.5", LocalNetwork),
            ("http://nas.local:8896", LocalNetwork),
            ("http://nas:8896", LocalNetwork),
            ("http://[fe80::1]:8896", LocalNetwork),
            ("https://quip.example-tailnet.ts.net", Tailnet),
            ("http://100.101.102.103", Tailnet),
            ("https://quip.example", Internet),
            ("http://8.8.8.8", Internet),
        ] {
            assert_eq!(Whereabouts::of(s), w, "{s}");
        }
        assert!(unencrypted_on_internet("http://quip.example") && !unencrypted_on_internet("https://quip.example") && !unencrypted_on_internet("http://nas.local"));
        assert_eq!(host_of("http://nas.local:8896/qc"), "nas.local:8896");
    }

    fn quip_list(c: &mut Controller) -> Vec<String> {
        let v = c.execute("quip.list", json!({})).unwrap();
        v.as_array().unwrap().iter().map(|s| s["server"].as_str().unwrap().to_owned()).collect()
    }

    #[test]
    fn never_follows_a_redirect_to_another_host_or_takes_too_much() {
        let elsewhere = serve(|_, _| response("200 OK", "", b"RIFF"));
        let other_host = elsewhere.replace("127.0.0.1", "localhost");
        let base = serve(move |req, _| {
            if req.contains("away.wav") {
                response("302 Found", &format!("Location: {other_host}/x.wav\r\n"), b"")
            } else if req.contains("page.wav") {
                response("200 OK", "Content-Type: text/html\r\n", b"<html>log in</html>")
            } else {
                response("200 OK", "", &[0u8; 4096])
            }
        });
        let fetch = |clip: &str, max| {
            let link = Link { server: base.clone(), clip: clip.into(), title: None, start: None, end: None, set: None, flac: false };
            let f = Fetch { max_bytes: max, ..Fetch::new(&link, "t".into()) };
            let dest = temp("dl").join("out.part");
            let r = f.download(&dest, &Progress::default());
            assert!(r.is_ok() || !dest.exists(), "nothing left behind");
            r
        };
        let e = fetch("away.wav", MAX_BYTES).unwrap_err();
        assert!(e.contains("another host") && e.contains("localhost"), "{e}");
        assert!(fetch("page.wav", MAX_BYTES).unwrap_err().contains("web page"));
        assert!(fetch("big.wav", 1024).unwrap_err().contains("limit"));
        fetch("big.wav", 4096).unwrap();
        let cancelled = Progress::default();
        cancelled.cancel();
        let link = Link { server: base.clone(), clip: "big.wav".into(), title: None, start: None, end: None, set: None, flac: false };
        assert_eq!(Fetch::new(&link, "t".into()).download(&temp("dl").join("c.part"), &cancelled).unwrap_err(), CANCELLED);
        assert!(same_server(&Url::parse("http://q.example/a").unwrap(), &Url::parse("https://q.example/b").unwrap()));
        assert!(!same_server(&Url::parse("https://q.example/a").unwrap(), &Url::parse("http://q.example/b").unwrap()));
    }
}
