//! Check for Updates (D-206): ask GitHub for the latest published release and compare it
//! with this build. Only the Help menu item, `app.checkForUpdates` or the opt-in launch
//! check (Settings, off by default) ever call it; the app downloads and installs nothing,
//! and a newer release only opens its page in the browser. Parsing and comparing are pure
//! functions; the request runs on its own thread so the window never waits for it.

use serde_json::{json, Value};
use std::cmp::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The project's page, from Cargo.toml (`repository`).
pub const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");
/// This build's version.
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");
/// Release notes longer than this are cut (they're shown, never parsed).
const MAX_NOTES: usize = 20_000;
/// The largest answer read from GitHub; a release's JSON is a few kB.
const MAX_BODY: u64 = 1024 * 1024;

/// A version as a release tag or Cargo gives it: `major.minor.patch`, an optional
/// `-pre.release` and `+build` (ignored), with or without a leading `v`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Version {
    pub numbers: [u64; 3],
    /// The pre-release identifiers (`rc.1` → `["rc", "1"]`); empty for a release.
    pub pre: Vec<String>,
}

impl Version {
    /// `v0.3.0`, `0.3`, `1.2.3-rc.1+abc`. Missing minor or patch numbers read as 0.
    pub fn parse(tag: &str) -> Option<Version> {
        let t = tag.trim();
        let t = t.strip_prefix(['v', 'V']).unwrap_or(t);
        let t = t.split('+').next().unwrap_or(t);
        let (core, pre) = match t.split_once('-') {
            Some((core, pre)) => (core, pre.split('.').map(str::to_owned).collect::<Vec<_>>()),
            None => (t, Vec::new()),
        };
        if pre.iter().any(|p| p.is_empty()) {
            return None;
        }
        let parts: Vec<&str> = core.split('.').collect();
        if parts.is_empty() || parts.len() > 3 {
            return None;
        }
        let mut numbers = [0; 3];
        for (n, p) in numbers.iter_mut().zip(&parts) {
            if p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            *n = p.parse().ok()?;
        }
        Some(Version { numbers, pre })
    }

    pub fn is_prerelease(&self) -> bool {
        !self.pre.is_empty()
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let [a, b, c] = self.numbers;
        write!(f, "{a}.{b}.{c}")?;
        if self.is_prerelease() {
            write!(f, "-{}", self.pre.join("."))?;
        }
        Ok(())
    }
}

impl Ord for Version {
    /// Semantic-versioning precedence: the numbers, then a pre-release below its release,
    /// then the pre-release identifiers one by one (numeric ones as numbers, and below
    /// words).
    fn cmp(&self, other: &Self) -> Ordering {
        self.numbers.cmp(&other.numbers).then_with(|| match (self.pre.is_empty(), other.pre.is_empty()) {
            (true, true) => Ordering::Equal,
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            (false, false) => {
                for (a, b) in self.pre.iter().zip(&other.pre) {
                    let o = match (a.parse::<u64>(), b.parse::<u64>()) {
                        (Ok(x), Ok(y)) => x.cmp(&y),
                        (Ok(_), Err(_)) => Ordering::Less,
                        (Err(_), Ok(_)) => Ordering::Greater,
                        (Err(_), Err(_)) => a.cmp(b),
                    };
                    if o != Ordering::Equal {
                        return o;
                    }
                }
                self.pre.len().cmp(&other.pre.len())
            }
        })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// The latest release, as GitHub's `releases/latest` describes it.
#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    /// Its tag without the leading `v`.
    pub version: String,
    /// The release notes (`body`), as written: Markdown, shown as plain text.
    pub notes: String,
    /// The release's page, where its downloads are.
    pub url: String,
    /// A draft or pre-release (by GitHub's flags or its tag), never offered.
    pub prerelease: bool,
}

/// GitHub's API address for a repository's latest release, from its page
/// (`https://github.com/owner/name`).
pub fn latest_api_url(repository: &str) -> Option<String> {
    let path = repository.trim().strip_prefix("https://github.com/")?.trim_end_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    match path.split('/').collect::<Vec<_>>().as_slice() {
        [owner, name] if !owner.is_empty() && !name.is_empty() => Some(format!("https://api.github.com/repos/{owner}/{name}/releases/latest")),
        _ => None,
    }
}

/// The page to open for a release: its own `html_url` when that is a GitHub https page,
/// else the repository's latest-release page (so an odd answer can't send the browser
/// anywhere else).
fn release_page(html_url: Option<&str>, repository: &str) -> String {
    match html_url {
        Some(u) if u.starts_with("https://github.com/") && !u.contains(char::is_whitespace) => u.to_owned(),
        _ => format!("{}/releases/latest", repository.trim_end_matches('/')),
    }
}

/// Read a `releases/latest` answer.
pub fn parse_release(json: &str, repository: &str) -> Result<Release, String> {
    let v: Value = serde_json::from_str(json).map_err(|e| format!("GitHub's answer isn't a release ({e})"))?;
    let tag = v["tag_name"].as_str().ok_or("GitHub's answer has no release tag")?;
    let version = Version::parse(tag).ok_or_else(|| format!("the latest release's tag, {tag:?}, isn't a version number"))?;
    let flagged = v["draft"].as_bool() == Some(true) || v["prerelease"].as_bool() == Some(true);
    let mut notes = v["body"].as_str().unwrap_or("").replace("\r\n", "\n").trim().to_owned();
    if notes.len() > MAX_NOTES {
        let cut = (0..=MAX_NOTES).rev().find(|&i| notes.is_char_boundary(i)).unwrap_or(0);
        notes.truncate(cut);
        notes.push_str("\n…");
    }
    Ok(Release {
        version: tag.trim().trim_start_matches(['v', 'V']).to_owned(),
        notes,
        url: release_page(v["html_url"].as_str(), repository),
        prerelease: flagged || version.is_prerelease(),
    })
}

/// What a check found.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// A newer release is out.
    Newer(Release),
    /// Nothing newer: `latest` is the newest release published, if any could be read.
    UpToDate { latest: Option<String> },
    /// The check couldn't be made; a one-line reason.
    Failed(String),
}

/// Compare the latest release with `current`. Drafts and pre-releases are never offered.
pub fn compare(current: &str, release: Release) -> Outcome {
    let Some(have) = Version::parse(current) else {
        return Outcome::Failed(format!("this build's version, {current}, isn't a version number"));
    };
    match Version::parse(&release.version) {
        Some(v) if !release.prerelease && v > have => Outcome::Newer(release),
        _ => Outcome::UpToDate { latest: (!release.prerelease).then_some(release.version) },
    }
}

/// What an HTTP status other than 200 means here.
fn status_error(status: u16) -> String {
    match status {
        404 => "no release has been published yet".into(),
        403 | 429 => "GitHub is limiting requests from this network; try again in an hour".into(),
        s => format!("GitHub answered HTTP {s}"),
    }
}

/// Ask GitHub. `LEADSTINGER_UPDATE_JSON` names a file to read the answer from instead
/// (tests and screenshots, which must never touch the network).
pub fn fetch() -> Outcome {
    let body = match crate::brand::env("UPDATE_JSON") {
        Some(path) => std::fs::read_to_string(&path).map_err(|e| format!("can't read {}: {e}", std::path::Path::new(&path).display())),
        None => fetch_github(),
    };
    match body.and_then(|b| parse_release(&b, REPOSITORY)) {
        Ok(r) => compare(CURRENT, r),
        Err(e) => Outcome::Failed(e),
    }
}

fn fetch_github() -> Result<String, String> {
    use ureq::tls::{RootCerts, TlsConfig};
    let url = latest_api_url(REPOSITORY).ok_or("the project's repository isn't on GitHub")?;
    let agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .user_agent(format!("{}/{CURRENT}", crate::brand::APP_NAME))
        .timeout_connect(Some(Duration::from_secs(5)))
        .timeout_global(Some(Duration::from_secs(15)))
        .tls_config(TlsConfig::builder().root_certs(RootCerts::PlatformVerifier).build())
        .build()
        .new_agent();
    let resp = agent
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .call()
        .map_err(|e| match e {
            ureq::Error::Timeout(_) => "GitHub took too long to answer".to_owned(),
            ureq::Error::Io(io) => format!("couldn't reach GitHub: {io}"),
            other => format!("couldn't reach GitHub: {other}"),
        })?;
    let status = resp.status().as_u16();
    if status != 200 {
        return Err(status_error(status));
    }
    resp.into_body().with_config().limit(MAX_BODY).read_to_string().map_err(|e| format!("couldn't read GitHub's answer: {e}"))
}

/// Who sees a check's answer in the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Show {
    /// No one (the CLI with no window).
    Never,
    /// Only a newer release (the launch check: silent when up to date or offline).
    IfNewer,
    /// Every answer (the menu item).
    Always,
}

/// A finished check.
#[derive(Clone, Debug, PartialEq)]
pub struct Done {
    /// Counts checks, so the window shows each answer once.
    pub seq: u64,
    pub outcome: Outcome,
    pub show: Show,
}

#[derive(Default)]
struct State {
    checking: bool,
    /// The running check's audience (raised if the menu asks while the launch check runs).
    show: Option<Show>,
    seq: u64,
    last: Option<Done>,
}

/// Runs checks off the caller's thread and keeps the last answer, for the window and for
/// `app.checkForUpdates` polls.
#[derive(Clone)]
pub struct Checker {
    state: Arc<Mutex<State>>,
    fetch: fn() -> Outcome,
}

impl Default for Checker {
    fn default() -> Self {
        // Tests never reach the network: a controller built in a test checks nothing.
        #[cfg(test)]
        let fetch: fn() -> Outcome = || Outcome::Failed("no network in tests".into());
        #[cfg(not(test))]
        let fetch: fn() -> Outcome = fetch;
        Checker::with(fetch)
    }
}

impl Checker {
    /// A checker that asks `fetch` (tests pass a stand-in for GitHub).
    pub fn with(fetch: fn() -> Outcome) -> Checker {
        Checker { state: Arc::default(), fetch }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Start a check unless one is running; either way its answer goes to `show`'s
    /// audience at least.
    pub fn start(&self, show: Show) {
        let mut s = self.state();
        s.show = s.show.max(Some(show));
        if s.checking {
            return;
        }
        s.checking = true;
        drop(s);
        let (state, fetch) = (self.state.clone(), self.fetch);
        let spawned = std::thread::Builder::new().name("update-check".into()).spawn(move || {
            let outcome = fetch();
            let mut s = state.lock().unwrap_or_else(|e| e.into_inner());
            s.seq += 1;
            s.last = Some(Done { seq: s.seq, outcome, show: s.show.take().unwrap_or(Show::Never) });
            s.checking = false;
            drop(s);
            crate::quip::wake();
        });
        if spawned.is_err() {
            let mut s = self.state();
            s.seq += 1;
            s.last = Some(Done { seq: s.seq, outcome: Outcome::Failed("couldn't start the check".into()), show: s.show.take().unwrap_or(Show::Never) });
            s.checking = false;
        }
    }

    pub fn checking(&self) -> bool {
        self.state().checking
    }

    /// The last answer, if it came after the one numbered `seen`.
    pub fn finished_since(&self, seen: u64) -> Option<Done> {
        self.state().last.clone().filter(|d| d.seq > seen)
    }

    /// `app.checkForUpdates`'s answer: still checking, or the last outcome.
    pub fn json(&self) -> Value {
        let s = self.state();
        if s.checking {
            return json!({"checking": true, "current": CURRENT});
        }
        match s.last.as_ref().map(|d| &d.outcome) {
            None => json!({"checking": false, "current": CURRENT, "status": null}),
            Some(o) => outcome_json(o),
        }
    }
}

pub fn outcome_json(o: &Outcome) -> Value {
    match o {
        Outcome::Newer(r) => json!({"checking": false, "current": CURRENT, "status": "newer", "latest": r.version, "url": r.url, "notes": r.notes}),
        Outcome::UpToDate { latest } => json!({"checking": false, "current": CURRENT, "status": "latest", "latest": latest}),
        Outcome::Failed(e) => json!({"checking": false, "current": CURRENT, "status": "failed", "error": e}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPO: &str = "https://github.com/weckere/leadstinger";

    /// A trimmed `releases/latest` answer, in GitHub's shape.
    fn fixture(tag: &str, draft: bool, prerelease: bool) -> String {
        json!({
            "url": "https://api.github.com/repos/weckere/leadstinger/releases/1",
            "html_url": format!("https://github.com/weckere/leadstinger/releases/tag/{tag}"),
            "tag_name": tag,
            "name": format!("LeadStinger {tag}"),
            "draft": draft,
            "prerelease": prerelease,
            "published_at": "2026-11-01T12:00:00Z",
            "body": "## What's new\r\n\r\n- Check for Updates\r\n- Faster pads\r\n",
            "assets": [{"name": "LeadStinger.dmg", "browser_download_url": "https://github.com/weckere/leadstinger/releases/download/x/LeadStinger.dmg"}]
        })
        .to_string()
    }

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap_or_else(|| panic!("{s} parses"))
    }

    #[test]
    fn the_api_url_comes_from_the_repository() {
        assert_eq!(latest_api_url(REPO).as_deref(), Some("https://api.github.com/repos/weckere/leadstinger/releases/latest"));
        assert_eq!(latest_api_url(REPOSITORY).as_deref(), Some("https://api.github.com/repos/weckere/leadstinger/releases/latest"));
        assert_eq!(latest_api_url("https://github.com/a/b.git/").as_deref(), Some("https://api.github.com/repos/a/b/releases/latest"));
        assert_eq!(latest_api_url("https://gitlab.com/a/b"), None);
        assert_eq!(latest_api_url("https://github.com/a"), None);
        assert_eq!(latest_api_url("https://github.com/a/b/c"), None);
    }

    #[test]
    fn versions_parse_with_or_without_v() {
        assert_eq!(v("v0.3.0"), Version { numbers: [0, 3, 0], pre: vec![] });
        assert_eq!(v("1.2"), Version { numbers: [1, 2, 0], pre: vec![] });
        assert_eq!(v("V2"), Version { numbers: [2, 0, 0], pre: vec![] });
        assert_eq!(v("1.2.3-rc.1+build.5"), Version { numbers: [1, 2, 3], pre: vec!["rc".into(), "1".into()] });
        assert_eq!(v("0.3.0-beta").to_string(), "0.3.0-beta");
        for bad in ["", "v", "latest", "1.2.3.4", "1..2", "1.x", "1.2.3-", "1.2.3-rc..1", "-1.2"] {
            assert_eq!(Version::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn versions_compare_numerically() {
        assert!(v("0.10.0") > v("0.9.9"));
        assert!(v("1.0.0") > v("0.99.99"));
        assert!(v("0.3.1") > v("0.3.0"));
        assert_eq!(v("v0.3.0").cmp(&v("0.3")), Ordering::Equal);
    }

    #[test]
    fn pre_releases_sort_below_their_release() {
        assert!(v("0.3.0-rc.1") < v("0.3.0"));
        assert!(v("0.3.0-rc.1") > v("0.2.9"));
        assert!(v("0.3.0-alpha") < v("0.3.0-alpha.1"));
        assert!(v("0.3.0-alpha.1") < v("0.3.0-alpha.beta"));
        assert!(v("0.3.0-beta.2") < v("0.3.0-beta.11"));
        assert!(v("0.3.0-beta.11") < v("0.3.0-rc.1"));
    }

    #[test]
    fn a_release_is_read_from_githubs_json() {
        let r = parse_release(&fixture("v0.3.0", false, false), REPO).unwrap();
        assert_eq!(r.version, "0.3.0");
        assert_eq!(r.url, "https://github.com/weckere/leadstinger/releases/tag/v0.3.0");
        assert_eq!(r.notes, "## What's new\n\n- Check for Updates\n- Faster pads");
        assert!(!r.prerelease);
    }

    #[test]
    fn a_bad_answer_is_an_error() {
        assert!(parse_release("<html>rate limited</html>", REPO).is_err());
        assert!(parse_release(r#"{"message": "Not Found"}"#, REPO).unwrap_err().contains("no release tag"));
        assert!(parse_release(r#"{"tag_name": "nightly"}"#, REPO).unwrap_err().contains("isn't a version"));
    }

    #[test]
    fn the_page_opened_stays_on_github() {
        let mut j: Value = serde_json::from_str(&fixture("v0.3.0", false, false)).unwrap();
        j["html_url"] = json!("https://evil.example/download");
        assert_eq!(parse_release(&j.to_string(), REPO).unwrap().url, "https://github.com/weckere/leadstinger/releases/latest");
        j["html_url"] = Value::Null;
        assert_eq!(parse_release(&j.to_string(), REPO).unwrap().url, "https://github.com/weckere/leadstinger/releases/latest");
    }

    #[test]
    fn long_notes_are_cut() {
        let mut j: Value = serde_json::from_str(&fixture("v0.3.0", false, false)).unwrap();
        j["body"] = json!("é".repeat(MAX_NOTES));
        let r = parse_release(&j.to_string(), REPO).unwrap();
        assert!(r.notes.len() <= MAX_NOTES + 4 && r.notes.ends_with('…'));
    }

    #[test]
    fn only_a_newer_release_is_offered() {
        let release = |tag: &str| parse_release(&fixture(tag, false, false), REPO).unwrap();
        assert!(matches!(compare("0.2.0", release("v0.3.0")), Outcome::Newer(r) if r.version == "0.3.0"));
        assert!(matches!(compare("0.2.0", release("v0.10.0")), Outcome::Newer(_)));
        assert_eq!(compare("0.3.0", release("v0.3.0")), Outcome::UpToDate { latest: Some("0.3.0".into()) });
        assert_eq!(compare("0.4.0", release("v0.3.0")), Outcome::UpToDate { latest: Some("0.3.0".into()) });
        // A pre-release build is offered its release.
        assert!(matches!(compare("0.3.0-rc.1", release("v0.3.0")), Outcome::Newer(_)));
        assert!(matches!(compare("not a version", release("v0.3.0")), Outcome::Failed(_)));
    }

    #[test]
    fn drafts_and_pre_releases_are_ignored() {
        for (tag, draft, pre) in [("v9.0.0", true, false), ("v9.0.0", false, true), ("v9.0.0-rc.1", false, false)] {
            let r = parse_release(&fixture(tag, draft, pre), REPO).unwrap();
            assert!(r.prerelease, "{tag} {draft} {pre}");
            assert_eq!(compare("0.2.0", r), Outcome::UpToDate { latest: None });
        }
    }

    #[test]
    fn http_errors_read_plainly() {
        assert_eq!(status_error(404), "no release has been published yet");
        assert!(status_error(403).contains("try again"));
        assert_eq!(status_error(500), "GitHub answered HTTP 500");
    }

    #[test]
    fn the_checker_runs_off_the_thread_and_keeps_the_answer() {
        let c = Checker::with(|| {
            std::thread::sleep(Duration::from_millis(50));
            Outcome::UpToDate { latest: Some("0.2.0".into()) }
        });
        assert_eq!(c.json()["status"], Value::Null);
        c.start(Show::IfNewer);
        // A second request while it runs joins it, and widens who sees the answer.
        c.start(Show::Always);
        assert_eq!(c.json()["checking"], true);
        let t = std::time::Instant::now();
        while c.checking() {
            assert!(t.elapsed() < Duration::from_secs(5), "the check finishes");
            std::thread::sleep(Duration::from_millis(5));
        }
        let done = c.finished_since(0).unwrap();
        assert_eq!((done.seq, done.show), (1, Show::Always));
        assert_eq!(c.finished_since(1), None);
        assert_eq!(c.json()["status"], "latest");
        assert_eq!(c.json()["latest"], "0.2.0");
    }

    #[test]
    fn tests_never_reach_github() {
        assert_eq!((Checker::default().fetch)(), Outcome::Failed("no network in tests".into()));
    }
}
