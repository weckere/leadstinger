//! The Apple Speech engine, macOS and iPadOS (D-286 to D-289): the Speech framework's
//! `SFSpeechRecognizer` on the prepared WAV, on the device only.
//!
//! - **On-device only** (D-287): every request sets `requiresOnDeviceRecognition`; a
//!   language without an on-device model is refused, never sent to Apple's servers.
//! - **Never prompts unasked** (D-288): only [`request_permission`] (Settings' "Allow
//!   Speech Recognition…", `transcribe.allowSpeech`) asks; until it's granted, [`open`]
//!   refuses with what to do, so `auto` moves on.
//! - **Only in the app** (D-289): the program must be its app bundle's own executable,
//!   whose Info.plist has the usage description; anything else (cargo run, the Nix build,
//!   leadstinger-cli) is refused without touching the permission calls, which would end
//!   a process that has no usage description.
//!
//! Audio over 50 s is recognised in pieces ([`apple_core::plan_pieces`]); the lines are
//! made from the word segments by [`apple_core::lines`].

use super::apple_core::{self, Blocker, Device, Permission, Segment};
use super::{Context, Engine, EngineKind, Heard, Request, CANCELLED};
use crate::subtitles::SubtitleLine;
use crate::transcode::Progress;
use block2::RcBlock;
use objc2::rc::{autoreleasepool, Retained};
use objc2::{sel, AnyThread};
use objc2_foundation::{NSBundle, NSError, NSLocale, NSObjectProtocol, NSOperationQueue, NSString, NSURL};
use objc2_speech::{
    SFSpeechRecognitionResult, SFSpeechRecognitionTaskState, SFSpeechRecognizer, SFSpeechRecognizerAuthorizationStatus, SFSpeechURLRecognitionRequest,
};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// The Info.plist key the system shows in its permission prompt.
const USAGE_KEY: &str = "NSSpeechRecognitionUsageDescription";
/// How long a [`status`] answer is kept: Settings asks every frame.
const KEEP: Duration = Duration::from_secs(5);
/// How often the worker looks at the task and at cancel.
const TICK: Duration = Duration::from_millis(50);

/// The engine, opened for a locale.
#[derive(Clone, Debug)]
pub struct AppleSpeech {
    /// The recogniser's locale, e.g. `en-US`.
    pub locale: String,
}

/// What the engine would do now with a language setting: the locale it would use and
/// the language's name, whether that locale is on-device, the permission, and what
/// stops it (if anything).
#[derive(Clone, Debug, PartialEq)]
pub struct Status {
    pub permission: Option<Permission>,
    pub locale: Option<String>,
    /// The locale's name in the user's language, e.g. "English (United States)".
    pub language: Option<String>,
    pub on_device: Option<bool>,
    pub blocker: Option<Blocker>,
}

impl Status {
    pub fn json(&self) -> Value {
        json!({
            "permission": self.permission.map(Permission::name),
            "locale": self.locale,
            "language": self.language,
            "onDevice": self.on_device,
            "canAsk": self.blocker.as_ref().is_some_and(Blocker::can_ask),
        })
    }
}

/// Whether this process may use the engine at all (D-289), worked out once.
fn in_app() -> Result<(), Blocker> {
    static IN_APP: OnceLock<Result<(), Blocker>> = OnceLock::new();
    IN_APP
        .get_or_init(|| {
            let exe = std::env::current_exe().ok().and_then(|p| p.canonicalize().ok());
            let cli = exe.as_ref().and_then(|p| p.file_name()).is_some_and(|n| n.to_string_lossy().starts_with(crate::brand::CLI_NAME));
            if cli {
                return Err(Blocker::NotInApp { cli: true });
            }
            let bundle = NSBundle::mainBundle();
            let described = bundle.objectForInfoDictionaryKey(&NSString::from_str(USAGE_KEY)).is_some();
            let own = bundle.executablePath().and_then(|p| PathBuf::from(p.to_string()).canonicalize().ok());
            if described && own.is_some() && own == exe {
                Ok(())
            } else {
                Err(Blocker::NotInApp { cli: false })
            }
        })
        .clone()
}

/// The permission, without asking. Only call after [`in_app`].
fn permission() -> Permission {
    // SAFETY: a class method that reads the stored answer; it never prompts.
    match unsafe { SFSpeechRecognizer::authorizationStatus() } {
        SFSpeechRecognizerAuthorizationStatus::Authorized => Permission::Authorized,
        SFSpeechRecognizerAuthorizationStatus::Denied => Permission::Denied,
        SFSpeechRecognizerAuthorizationStatus::Restricted => Permission::Restricted,
        _ => Permission::NotDetermined,
    }
}

/// The locale for `lang` (`None`: the user's own language) and its name, if the
/// recogniser has one.
fn locale_for(lang: Option<&str>) -> Option<(String, String)> {
    // SAFETY: a class method returning a set of locales.
    let supported: Vec<String> = unsafe { SFSpeechRecognizer::supportedLocales() }.iter().map(|l| l.localeIdentifier().to_string()).collect();
    let current = NSLocale::currentLocale();
    let id = apple_core::pick_locale(lang, &current.localeIdentifier().to_string(), &supported)?;
    let name = current.localizedStringForLocaleIdentifier(&NSString::from_str(&id)).to_string();
    let name = if name.is_empty() { id.clone() } else { name };
    Some((id, name))
}

fn recognizer(locale: &str) -> Option<Retained<SFSpeechRecognizer>> {
    let locale = NSLocale::localeWithLocaleIdentifier(&NSString::from_str(locale));
    // SAFETY: the designated initialiser; it returns nil for a locale it can't take.
    unsafe { SFSpeechRecognizer::initWithLocale(SFSpeechRecognizer::alloc(), &locale) }
}

fn work_out(lang: Option<&str>) -> Status {
    let mut st = Status { permission: None, locale: None, language: None, on_device: None, blocker: None };
    if let Err(b) = in_app() {
        st.blocker = Some(b);
        return st;
    }
    let perm = permission();
    st.permission = Some(perm);
    let Some((locale, name)) = locale_for(lang) else {
        st.blocker = Some(Blocker::NoLanguage(lang.unwrap_or("your language").to_owned()));
        return st;
    };
    st.locale = Some(locale.clone());
    st.language = Some(name.clone());
    let Some(rec) = recognizer(&locale) else {
        st.blocker = Some(Blocker::NoLanguage(name));
        return st;
    };
    // SAFETY: plain property reads.
    let (on_device, available) = unsafe { (rec.supportsOnDeviceRecognition(), rec.isAvailable()) };
    st.on_device = Some(on_device);
    st.blocker = if perm != Permission::Authorized {
        Some(Blocker::Permission(perm))
    } else if !on_device {
        Some(Blocker::NotOnDevice(name))
    } else if !available {
        Some(Blocker::Unavailable(name))
    } else {
        None
    };
    st
}

type Cache = Option<(Option<String>, Instant, Status)>;

fn cache() -> &'static Mutex<Cache> {
    static CACHE: Mutex<Cache> = Mutex::new(None);
    &CACHE
}

/// What the engine would do now for a language setting, kept for [`KEEP`] (Settings
/// asks every frame).
pub fn status(lang: Option<&str>) -> Status {
    let key = lang.map(str::to_owned);
    let mut c = cache().lock().unwrap_or_else(|e| e.into_inner());
    if let Some((k, at, st)) = c.as_ref() {
        if *k == key && at.elapsed() < KEEP {
            return st.clone();
        }
    }
    let st = autoreleasepool(|_| work_out(lang));
    *c = Some((key, Instant::now(), st.clone()));
    st
}

fn forget() {
    *cache().lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// Open the engine for the language setting, or say why it can't run (never asks).
pub fn open(ctx: &Context) -> Result<AppleSpeech, String> {
    let st = status(ctx.lang.as_deref());
    match (st.blocker, st.locale) {
        (Some(b), _) => Err(b.message(Device::here())),
        (None, Some(locale)) => Ok(AppleSpeech { locale }),
        (None, None) => Err(Blocker::NoLanguage("your language".into()).message(Device::here())),
    }
}

/// Ask for Speech Recognition permission (D-288): the system's prompt, only while it
/// hasn't been asked. Returns the permission as it is now (still not-asked while the
/// prompt is up); the next [`status`] after the answer sees it.
pub fn request_permission() -> Result<Permission, String> {
    in_app().map_err(|b| b.message(Device::here()))?;
    let now = permission();
    if now == Permission::NotDetermined {
        let answered = RcBlock::new(|_: SFSpeechRecognizerAuthorizationStatus| forget());
        // SAFETY: the main bundle has the usage description (in_app), so this shows the
        // prompt rather than ending the process; the block is copied by the framework.
        unsafe { SFSpeechRecognizer::requestAuthorization(&answered) };
    }
    forget();
    Ok(now)
}

/// Each final result of a task: its formatted string (UTF-16) and its words.
type Finals = Vec<(Vec<u16>, Vec<Segment>)>;

/// What a recognition task handed back so far.
#[derive(Default)]
struct Collected {
    /// Each final result: its formatted string (UTF-16) and its words.
    finals: Finals,
    /// The error, as (domain, code, description).
    error: Option<(String, isize, String)>,
}

fn segments(result: &SFSpeechRecognitionResult) -> (Vec<u16>, Vec<Segment>) {
    // SAFETY: property reads on a result the framework handed us.
    unsafe {
        let best = result.bestTranscription();
        let formatted: Vec<u16> = best.formattedString().to_string().encode_utf16().collect();
        let segs = best
            .segments()
            .iter()
            .map(|s| {
                let r = s.substringRange();
                Segment { text: s.substring().to_string(), range: (r.location, r.location + r.length), start: s.timestamp(), duration: s.duration() }
            })
            .collect();
        (formatted, segs)
    }
}

impl AppleSpeech {
    /// Recognise one piece's WAV file. `share` is (where the piece starts, how much of
    /// the whole it is), both fractions, for progress.
    fn recognise(&self, path: &Path, seconds: f64, share: (f64, f64), progress: &Arc<Progress>) -> Result<Finals, String> {
        let rec = recognizer(&self.locale).ok_or_else(|| Blocker::NoLanguage(self.locale.clone()).message(Device::here()))?;
        // SAFETY: property reads and sets on objects made here, on this thread; the
        // handler runs on our own queue and only touches the shared Mutex and progress.
        unsafe {
            if !rec.supportsOnDeviceRecognition() {
                return Err(Blocker::NotOnDevice(self.locale.clone()).message(Device::here()));
            }
            // Handlers go to a queue of their own, not the main queue: the worker waits
            // here, and the CLI and tests have no main run loop.
            rec.setQueue(&NSOperationQueue::new());
            let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
            let req = SFSpeechURLRecognitionRequest::initWithURL(SFSpeechURLRecognitionRequest::alloc(), &url);
            req.setRequiresOnDeviceRecognition(true);
            req.setShouldReportPartialResults(true);
            if req.respondsToSelector(sel!(setAddsPunctuation:)) {
                req.setAddsPunctuation(true);
            }
            let got = Arc::new(Mutex::new(Collected::default()));
            let handler = {
                let got = Arc::clone(&got);
                let progress = Arc::clone(progress);
                RcBlock::new(move |result: *mut SFSpeechRecognitionResult, error: *mut NSError| {
                    let mut g = got.lock().unwrap_or_else(|e| e.into_inner());
                    if let Some(result) = result.as_ref() {
                        let (formatted, segs) = segments(result);
                        let heard = segs.iter().map(|s| s.start + s.duration).fold(0.0, f64::max);
                        if heard > 0.0 && seconds > 0.0 {
                            progress.set_fraction(Some(share.0 + share.1 * (heard / seconds).min(1.0)));
                        }
                        if result.isFinal() {
                            g.finals.push((formatted, segs));
                        }
                    }
                    if let Some(e) = error.as_ref() {
                        g.error = Some((e.domain().to_string(), e.code(), e.localizedDescription().to_string()));
                    }
                })
            };
            let task = rec.recognitionTaskWithRequest_resultHandler(&req, &handler);
            let deadline = Instant::now() + Duration::from_secs_f64(seconds * 4.0 + 60.0);
            let mut completed_at: Option<Instant> = None;
            loop {
                std::thread::sleep(TICK);
                if progress.is_cancelled() {
                    task.cancel();
                    let stop = Instant::now() + Duration::from_secs(2);
                    while task.state() != SFSpeechRecognitionTaskState::Completed && Instant::now() < stop {
                        std::thread::sleep(TICK);
                    }
                    return Err(CANCELLED.to_owned());
                }
                if Instant::now() > deadline {
                    task.cancel();
                    return Err("Apple Speech took far longer than the sound lasts, so it was stopped".to_owned());
                }
                let g = got.lock().unwrap_or_else(|e| e.into_inner());
                if g.error.is_some() {
                    break;
                }
                if task.state() == SFSpeechRecognitionTaskState::Completed {
                    // The final result may land just after the state changes.
                    let at = *completed_at.get_or_insert_with(Instant::now);
                    if !g.finals.is_empty() || at.elapsed() > Duration::from_secs(1) {
                        break;
                    }
                }
            }
            let g = std::mem::take(&mut *got.lock().unwrap_or_else(|e| e.into_inner()));
            match g.error {
                Some((domain, code, _)) if apple_core::is_no_speech(&domain, code) => Ok(g.finals),
                Some((_, _, what)) if g.finals.is_empty() => Err(format!("Apple Speech failed: {what}")),
                _ => Ok(g.finals),
            }
        }
    }
}

impl Engine for AppleSpeech {
    fn kind(&self) -> EngineKind {
        EngineKind::AppleSpeech
    }

    fn transcribe(&self, req: &Request, progress: &Arc<Progress>) -> Result<Heard, String> {
        // A language asked for with this request may differ from the setting `open`
        // checked: check it again (on-device only, D-287).
        let engine = match req.lang.as_deref() {
            Some(lang) => {
                let st = status(Some(lang));
                match (st.blocker, st.locale) {
                    (Some(b), _) => return Err(b.message(Device::here())),
                    (None, Some(locale)) => AppleSpeech { locale },
                    (None, None) => self.clone(),
                }
            }
            None => self.clone(),
        };
        let (rate, samples) = apple_core::wav_samples(&req.wav)?;
        let pieces = apple_core::plan_pieces(&samples, rate);
        let total = samples.len().max(1) as f64;
        let dir = std::env::temp_dir();
        let mut lines: Vec<SubtitleLine> = Vec::new();
        progress.set_fraction(Some(0.0));
        for (n, &(from, to)) in pieces.iter().enumerate() {
            if progress.is_cancelled() {
                return Err(CANCELLED.to_owned());
            }
            if to <= from {
                continue;
            }
            let path = dir.join(format!("{}-speech-{}-{n}.wav", crate::brand::SLUG, std::process::id()));
            std::fs::write(&path, crate::wav::mono_bytes(rate, &samples[from..to])).map_err(|e| format!("couldn't write the audio for Apple Speech: {e}"))?;
            let seconds = (to - from) as f64 / f64::from(rate);
            let share = (from as f64 / total, (to - from) as f64 / total);
            let got = autoreleasepool(|_| engine.recognise(&path, seconds, share, progress));
            let _ = std::fs::remove_file(&path);
            let offset = from as f64 / f64::from(rate);
            let mut last_end = f64::NEG_INFINITY;
            for (formatted, segs) in got? {
                // A later final result may repeat words an earlier one gave.
                let fresh: Vec<Segment> = segs.into_iter().filter(|s| s.start >= last_end - 0.05).collect();
                if let Some(end) = fresh.iter().map(|s| s.start + s.duration).reduce(f64::max) {
                    last_end = end;
                }
                lines.extend(apple_core::lines(&formatted, &fresh, offset));
            }
            progress.set_fraction(Some((to as f64 / total).min(1.0)));
        }
        let lang = engine.locale.split('-').next().map(str::to_owned);
        Ok(Heard { lines, model: Some(format!("on-device {}", engine.locale)), lang })
    }
}
