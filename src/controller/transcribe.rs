//! Transcription as commands (D-277 to D-285): `tile.transcribe`, `set.transcribe`,
//! `tile.alignWords`, `transcribe.status`, `transcribe.cancel`, `transcribe.engines` and
//! `settings.transcribe`. The commands queue tiles; one runs at a time on a worker thread
//! (src/transcribe.rs does the work) and each tile's result is applied when it finishes,
//! as one undo step of its own.

use super::{bad, CmdError, Controller, Effect, Res, Step, P};
use crate::model::Tile;
use crate::transcode::Progress;
use crate::transcribe::{self, Audio, Choice, Context, Engine, EngineKind, Heard};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use uuid::Uuid;

/// Finished jobs kept for `transcribe.status`.
const KEEP_FINISHED: usize = 16;
/// A line's words are listened for this far either side of it.
const LINE_PAD: f64 = 0.5;

/// Opens an engine: [`transcribe::pick`], or a stand-in in tests.
pub type Opener = Arc<dyn Fn(Choice, &Context) -> Result<Box<dyn Engine>, String> + Send + Sync>;

/// One tile waiting its turn.
struct Item {
    job: u64,
    tile: Uuid,
    title: String,
    /// The lines to time (`None`: the whole tile).
    targets: Option<Vec<usize>>,
    /// Seconds into the file to listen to (`None`: all of it).
    range: Option<(f64, f64)>,
    choice: Choice,
    lang: Option<String>,
    /// Add what's heard in the gaps of the tile's known subtitles (D-296).
    gaps: bool,
}

type Done = Result<(Heard, Vec<f64>), String>;
/// A tile to transcribe, the lines to time (`None`: all) and the stretch to hear.
type Wanted = (Uuid, Option<Vec<usize>>, Option<(f64, f64)>, bool);

/// The tile being transcribed.
struct Running {
    item: Item,
    kind: EngineKind,
    progress: Arc<Progress>,
    rx: Receiver<Done>,
}

/// What one command asked for, and how it went.
#[derive(Clone, Debug)]
pub struct Job {
    pub id: u64,
    pub total: usize,
    pub done: usize,
    pub results: Vec<Value>,
    pub errors: Vec<Value>,
    pub cancelled: bool,
    /// The notice for its end was shown.
    reported: bool,
}

impl Job {
    pub fn finished(&self) -> bool {
        self.cancelled || self.done >= self.total
    }

    fn json(&self, t: &Transcriber) -> Value {
        let running = t.running.as_ref().filter(|r| r.item.job == self.id);
        json!({
            "job": self.id, "total": self.total, "done": self.done, "finished": self.finished(), "cancelled": self.cancelled,
            "current": running.map(|r| json!({"tile": r.item.tile, "title": r.item.title, "engine": r.kind.name(), "fraction": r.progress.fraction()})),
            "queued": t.queue.iter().filter(|i| i.job == self.id).count(),
            "results": self.results, "errors": self.errors,
        })
    }
}

/// The queue, the running tile and the jobs.
pub struct Transcriber {
    queue: VecDeque<Item>,
    running: Option<Running>,
    pub jobs: Vec<Job>,
    next_job: u64,
    pub opener: Opener,
}

impl Default for Transcriber {
    fn default() -> Transcriber {
        Transcriber { queue: VecDeque::new(), running: None, jobs: Vec::new(), next_job: 1, opener: Arc::new(transcribe::pick) }
    }
}

/// A tile's part in transcription, for the UI.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Busy {
    Queued,
    /// Under way; how far, when the engine says.
    Running(Option<f32>),
}

impl Transcriber {
    pub fn busy(&self, tile: Uuid) -> Option<Busy> {
        match &self.running {
            Some(r) if r.item.tile == tile => Some(Busy::Running(r.progress.fraction())),
            _ => self.queue.iter().any(|i| i.tile == tile).then_some(Busy::Queued),
        }
    }

    pub fn is_idle(&self) -> bool {
        self.running.is_none() && self.queue.is_empty()
    }
}

/// The connected QuipClipper servers for `transcribe.engines`: each one's address and
/// name, whether it transcribes and why not, and the one a transcription would use.
fn quip_servers(ctx: &Context) -> Value {
    use crate::quip::transcribe::{choose, why_chosen, State};
    let chosen = choose(&ctx.quip, ctx.quip_server.as_deref()).ok();
    let list: Vec<Value> = ctx
        .quip
        .iter()
        .map(|o| {
            let caps = match &o.state {
                State::Upload(c) => json!({"maxBytes": c.max_bytes, "maxSeconds": c.max_seconds}),
                _ => Value::Null,
            };
            let used = chosen.is_some_and(|c| c.server == o.server);
            json!({
                "server": o.server, "name": o.name, "transcribes": o.reason().is_none(), "reason": o.reason(), "upload": caps,
                "used": used, "why": used.then(|| why_chosen(&ctx.quip, o, ctx.quip_server.as_deref())),
            })
        })
        .collect();
    json!(list)
}

fn plural(n: usize, one: &str) -> String {
    format!("{n} {one}{}", if n == 1 { "" } else { "s" })
}

impl Controller {
    /// `transcribe.allowSpeech`: the system's Speech Recognition prompt, if it hasn't
    /// been answered (D-288: nothing else shows it).
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    fn allow_speech(&self) -> Res {
        use transcribe::apple_core::{Device, Permission};
        let before = transcribe::apple::request_permission().map_err(|why| CmdError::new("unavailable", why))?;
        let st = transcribe::apple::status(self.store.library.transcribe.lang.as_deref());
        Ok(json!({"asked": before == Permission::NotDetermined, "permission": before.name(), "apple": st.json(), "reason": st.blocker.map(|b| b.message(Device::here()))}))
    }

    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    fn allow_speech(&self) -> Res {
        Err(CmdError::new("unavailable", "Apple Speech is only on macOS and iPadOS"))
    }

    /// What engines need to know, from the settings and what's connected. Makes no
    /// request itself: in the window, QuipClipper servers not asked yet whether they
    /// transcribe are asked in the background (src/quip/transcribe.rs).
    pub fn transcribe_context(&self) -> Context {
        let s = &self.store.library.transcribe;
        Context {
            whisper_model: s.whisper_model.clone(),
            quip: crate::quip::transcribe::offers(&self.store.data_dir, self.has_window),
            quip_server: s.quip_server.clone(),
            data_dir: Some(self.store.data_dir.clone()),
            lang: s.lang.clone(),
        }
    }

    /// [`Self::transcribe_context`], once every connected QuipClipper has been asked whether
    /// it transcribes (for a command, which may wait a few seconds for that).
    fn checked_context(&self) -> Context {
        let mut ctx = self.transcribe_context();
        if ctx.quip.iter().any(|o| matches!(o.state, crate::quip::transcribe::State::Unchecked | crate::quip::transcribe::State::Checking)) {
            ctx.quip = crate::quip::transcribe::checked(&self.store.data_dir);
        }
        ctx
    }

    /// The engine `transcribe.engines` and the window would use now, and where it sends
    /// the audio (`None`: nowhere, it stays on this computer).
    pub fn transcribe_destination(&self) -> Option<String> {
        let choice = Choice::parse(&self.store.library.transcribe.engine).unwrap_or_default();
        (self.transcriber.opener)(choice, &self.transcribe_context()).ok().and_then(|e| e.destination())
    }

    fn transcribe_choice(&self, p: &P) -> Result<Choice, CmdError> {
        match p.str("engine")? {
            Some(e) => Choice::parse(e).ok_or_else(|| bad(format!("{}: engine must be auto, whisper, quip or apple (got {e:?})", p.id))),
            None => Ok(Choice::parse(&self.store.library.transcribe.engine).unwrap_or_default()),
        }
    }

    fn transcribe_lang(&self, p: &P) -> Result<Option<String>, CmdError> {
        Ok(p.str("lang")?.map(str::to_owned).or_else(|| self.store.library.transcribe.lang.clone()).filter(|l| !l.trim().is_empty() && l != "auto"))
    }

    /// The engine a command would use and where it sends the audio, or the reason none
    /// can run (code unavailable). A QuipClipper not asked yet is asked first, when no
    /// engine can run without it.
    fn transcribe_engine(&self, choice: Choice) -> Result<(EngineKind, Option<String>), CmdError> {
        let found = (self.transcriber.opener)(choice, &self.transcribe_context()).or_else(|_| (self.transcriber.opener)(choice, &self.checked_context()));
        found.map(|e| (e.kind(), e.destination())).map_err(|why| CmdError::new("unavailable", format!("Can't transcribe: {why}")))
    }

    /// Why this tile can't be transcribed, if it can't.
    fn untranscribable(&self, t: &Tile) -> Option<String> {
        if t.action.is_some() {
            return Some("an action tile has no audio".into());
        }
        if t.file.is_none() {
            return Some("it has no sound".into());
        }
        if self.store.import_of(t).is_some() {
            return Some("it's still being imported".into());
        }
        if let Some((si, _)) = self.store.library.locate(t.id) {
            let set = &self.store.library.sets[si];
            if set.locked {
                return Some(format!("“{}” is locked", set.name));
            }
        }
        self.transcriber.busy(t.id).map(|_| "it's already being transcribed".into())
    }

    fn locked_error(&self, set: usize) -> CmdError {
        let name = &self.store.library.sets[set].name;
        CmdError::new("locked", format!("“{name}” is locked: unlock it first (set.unlock, or the lock in its settings)"))
    }

    pub(super) fn transcribe_command(&mut self, id: &str, p: &P) -> Res {
        match id {
            "tile.transcribe" => {
                let ids = match (p.get("tiles"), p.str("tile")?) {
                    (Some(list), _) => self.tile_refs(list, id)?,
                    (None, Some(r)) => vec![self.resolve_tile(r)?],
                    (None, None) => self.selected(),
                };
                if ids.is_empty() {
                    return Err(bad("tile.transcribe: which tile? Give tile (or tiles), or select one"));
                }
                if let Some(si) = ids.iter().filter_map(|t| self.store.library.locate(*t)).map(|(si, _)| si).find(|&si| self.store.library.sets[si].locked) {
                    return Err(self.locked_error(si));
                }
                let gaps = p.bool("gaps")?.unwrap_or(true);
                let items = ids.into_iter().map(|t| (t, None, None, gaps)).collect();
                self.queue_transcription(id, p, items)
            }
            "set.transcribe" => {
                let si = self.set_param(p)?;
                if self.store.library.sets[si].locked {
                    return Err(self.locked_error(si));
                }
                let choice = self.transcribe_choice(p)?;
                let gaps = p.bool("gaps")?.unwrap_or(false);
                let set = &self.store.library.sets[si];
                let sounds = || set.tiles.iter().filter(|t| t.action.is_none() && t.file.is_some());
                let bare: Vec<Uuid> = sounds().filter(|t| t.subtitles.is_empty()).map(|t| t.id).collect();
                // With gaps, the tiles whose subtitles came from elsewhere too (D-298).
                let subtitled: Vec<Uuid> = sounds().filter(|t| !t.subtitles.is_empty() && !transcribe::is_machine(t.subtitles_source.as_ref())).map(|t| t.id).collect();
                if p.bool("ask")? == Some(true) && self.has_window {
                    let (engine, sends_to) = self.transcribe_engine(choice)?;
                    let (tiles, with_subtitles) = (bare.len(), subtitled.len());
                    let set = set.id;
                    self.effects.push(Effect::ConfirmTranscribe { set, tiles, subtitled: with_subtitles, engine: engine.label().to_owned(), sends_to: sends_to.clone() });
                    return Ok(json!({"asked": true, "set": set, "tiles": tiles, "subtitled": with_subtitles, "engine": engine.name(), "sendsTo": sends_to}));
                }
                let wanted: Vec<Uuid> = if gaps { sounds().filter(|t| bare.contains(&t.id) || subtitled.contains(&t.id)).map(|t| t.id).collect() } else { bare };
                if wanted.is_empty() {
                    let why = if gaps { "has no subtitles from elsewhere to fill, and none without".to_owned() } else { "has subtitles already".to_owned() };
                    return Err(CmdError::new("nothing_to_do", format!("Every tile in “{}” with a sound {why}", set.name)));
                }
                let items = wanted.into_iter().map(|t| (t, None, None, gaps)).collect();
                self.queue_transcription(id, p, items)
            }
            "tile.alignWords" => {
                let tile = self.tile_param(p)?;
                let t = self.store.library.tile(tile).expect("resolved");
                if let Some((si, _)) = self.store.library.locate(tile).filter(|(si, _)| self.store.library.sets[*si].locked) {
                    return Err(self.locked_error(si));
                }
                if t.subtitles.is_empty() {
                    return Err(bad(format!("tile.alignWords: “{}” has no subtitles (tile.transcribe makes them)", t.title)));
                }
                let n = t.subtitles.len();
                let lines: Vec<usize> = match p.get("line") {
                    Some(v) => {
                        let k = v.as_u64().or_else(|| v.as_str().and_then(|s| s.trim().trim_start_matches('#').parse().ok()));
                        match k {
                            Some(k) if k >= 1 && k as usize <= n => vec![k as usize - 1],
                            _ => return Err(bad(format!("tile.alignWords: line must be a line number from 1 to {n}"))),
                        }
                    }
                    None => (0..n).filter(|&i| t.subtitles[i].words.is_empty() && !t.subtitles[i].aligned).collect(),
                };
                if let Some(&i) = lines.iter().find(|&&i| t.subtitles[i].aligned) {
                    return Err(CmdError::new("nothing_to_do", format!("tile.alignWords: line {} was placed by hand; Reset it first to time it again", i + 1)));
                }
                // A line given by number is timed again from its text even if it has words.
                if lines.is_empty() {
                    return Err(CmdError::new("nothing_to_do", format!("Every line of “{}” has its word timings", t.title)));
                }
                let range = match lines.as_slice() {
                    [one] => Some(((t.subtitles[*one].start - LINE_PAD).max(0.0), t.subtitles[*one].end + LINE_PAD)),
                    _ => None,
                };
                self.queue_transcription(id, p, vec![(tile, Some(lines), range, false)])
            }
            "transcribe.status" => {
                // The offline CLI has no tick: look at the worker here.
                self.poll_transcription();
                let t = &self.transcriber;
                let jobs: Vec<Value> = match p.f64("job")? {
                    Some(j) => vec![t.jobs.iter().find(|r| r.id as f64 == j).ok_or_else(|| CmdError::new("not_found", format!("transcribe.status: no transcription job {j}")))?.json(t)],
                    None => t.jobs.iter().map(|j| j.json(t)).collect(),
                };
                Ok(json!({"jobs": jobs, "busy": !t.is_idle()}))
            }
            "transcribe.cancel" => {
                let job = p.f64("job")?.map(|j| j as u64);
                let n = self.cancel_transcription(job);
                Ok(json!({"cancelled": n}))
            }
            "transcribe.engines" => {
                let ctx = self.checked_context();
                let setting = Choice::parse(&self.store.library.transcribe.engine).unwrap_or_default();
                let engines: Vec<Value> = transcribe::availability(&ctx)
                    .into_iter()
                    .map(|a| {
                        let mut v = json!({"engine": a.kind.name(), "label": a.kind.label(), "available": a.available, "reason": a.reason});
                        if a.kind == EngineKind::Quip {
                            v["servers"] = quip_servers(&ctx);
                        }
                        v
                    })
                    .collect();
                let picked = (self.transcriber.opener)(setting, &ctx);
                let (use_, sends_to, reason) = match picked {
                    Ok(e) => (Some(e.kind().name()), e.destination(), None),
                    Err(why) => (None, None, Some(why)),
                };
                #[cfg(any(target_os = "macos", target_os = "ios"))]
                let apple = transcribe::apple::status(ctx.lang.as_deref()).json();
                #[cfg(not(any(target_os = "macos", target_os = "ios")))]
                let apple = Value::Null;
                Ok(json!({"setting": setting.name(), "uses": use_, "sendsTo": sends_to, "reason": reason, "engines": engines, "apple": apple}))
            }
            "transcribe.allowSpeech" => self.allow_speech(),
            "settings.transcribe" => {
                if let Some(e) = p.str("engine")? {
                    let c = Choice::parse(e).ok_or_else(|| bad(format!("settings.transcribe: engine must be auto, whisper, quip or apple (got {e:?})")))?;
                    self.store.library.transcribe.engine = c.name().to_owned();
                }
                if p.has("lang") {
                    let lang = p.str("lang")?.map(|l| l.trim().to_ascii_lowercase()).filter(|l| !l.is_empty() && l != "auto");
                    if lang.as_ref().is_some_and(|l| l.len() > 8 || !l.chars().all(|c| c.is_ascii_alphabetic() || c == '-')) {
                        return Err(bad("settings.transcribe: lang is a language code like en, de or pt-br, or auto"));
                    }
                    self.store.library.transcribe.lang = lang;
                }
                if p.has("whisperModel") {
                    self.store.library.transcribe.whisper_model = p.str("whisperModel")?.filter(|m| !m.trim().is_empty()).map(PathBuf::from);
                }
                if p.has("quipServer") {
                    let server = match p.str("quipServer")?.map(str::trim).filter(|s| !s.is_empty()) {
                        None => None,
                        Some(s) => {
                            let s = crate::quip::normalize_server(s).map_err(|e| bad(format!("settings.transcribe: quipServer: {e}")))?;
                            let connected = crate::quip::list(&self.store.data_dir).unwrap_or_default();
                            if !connected.iter().any(|(c, _)| *c == s) {
                                return Err(bad(format!("settings.transcribe: {s} isn't a connected QuipClipper (quip.list shows them)")));
                            }
                            Some(s)
                        }
                    };
                    self.store.library.transcribe.quip_server = server;
                }
                self.store.mark_dirty();
                let s = &self.store.library.transcribe;
                Ok(json!({"engine": s.engine, "lang": s.lang, "whisperModel": s.whisper_model, "quipServer": s.quip_server}))
            }
            _ => unreachable!("{id}"),
        }
    }

    /// Queue these tiles as one job; tiles that can't be transcribed are skipped and
    /// listed. Fails when that leaves nothing, or no engine can run.
    fn queue_transcription(&mut self, id: &str, p: &P, wanted: Vec<Wanted>) -> Res {
        let choice = self.transcribe_choice(p)?;
        let lang = self.transcribe_lang(p)?;
        let (engine, sends_to) = self.transcribe_engine(choice)?;
        let mut items = Vec::new();
        let mut skipped = Vec::new();
        for (tile, targets, range, gaps) in wanted {
            let Some(t) = self.store.library.tile(tile) else { continue };
            let timed = targets.is_none()
                && !gaps
                && !t.subtitles.is_empty()
                && !transcribe::is_machine(t.subtitles_source.as_ref())
                && t.subtitles.iter().all(|l| !l.words.is_empty() || l.aligned);
            let why = self.untranscribable(t).or_else(|| timed.then(|| "it has subtitles with word timings already".to_owned()));
            match why {
                Some(why) => skipped.push(json!({"tile": tile, "title": t.title, "reason": why})),
                None => items.push(Item { job: 0, tile, title: t.title.clone(), targets, range, choice, lang: lang.clone(), gaps }),
            }
        }
        if items.is_empty() {
            let why = match skipped.as_slice() {
                [one] => format!("“{}”: {}", one["title"].as_str().unwrap_or_default(), one["reason"].as_str().unwrap_or_default()),
                _ => "none of the tiles can be".to_owned(),
            };
            return Err(CmdError::new("nothing_to_do", format!("{id}: nothing to transcribe ({why})")));
        }
        let t = &mut self.transcriber;
        let job = t.next_job;
        t.next_job += 1;
        let total = items.len();
        for mut i in items {
            i.job = job;
            t.queue.push_back(i);
        }
        while t.jobs.iter().filter(|j| j.finished() && j.reported).count() >= KEEP_FINISHED {
            let Some(i) = t.jobs.iter().position(|j| j.finished() && j.reported) else { break };
            t.jobs.remove(i);
        }
        t.jobs.push(Job { id: job, total, done: 0, results: Vec::new(), errors: Vec::new(), cancelled: false, reported: false });
        let what = if total == 1 { "Transcribing".to_owned() } else { format!("Transcribing {}", plural(total, "tile")) };
        match &sends_to {
            Some(to) => self.notify(format!("{what}: sending the audio to {to}…")),
            None => self.notify(format!("{what} with {}…", engine.label())),
        }
        self.poll_transcription();
        Ok(json!({"job": job, "tiles": total, "engine": engine.name(), "sendsTo": sends_to, "skipped": skipped}))
    }

    /// Stop the jobs (`None`: every one): queued tiles are dropped, the running one is
    /// stopped. Returns how many tiles were stopped.
    pub fn cancel_transcription(&mut self, job: Option<u64>) -> usize {
        let t = &mut self.transcriber;
        let before = t.queue.len();
        t.queue.retain(|i| job.is_some_and(|j| i.job != j));
        let mut n = before - t.queue.len();
        if let Some(r) = t.running.as_ref().filter(|r| job.is_none_or(|j| r.item.job == j)) {
            r.progress.cancel();
            n += 1;
        }
        for j in t.jobs.iter_mut().filter(|j| !j.finished() && job.is_none_or(|k| j.id == k)) {
            // The running tile's own end is reported when its thread stops.
            if !t.running.as_ref().is_some_and(|r| r.item.job == j.id) {
                j.cancelled = true;
            }
        }
        n
    }

    /// For quitting: stop the running tile and wait for it.
    pub fn stop_transcription(&mut self) {
        self.cancel_transcription(None);
        if let Some(r) = self.transcriber.running.take() {
            r.rx.recv().ok();
        }
    }

    /// "Transcribing “Intro” 45% (+2)" while one runs, for the status bar.
    pub fn transcribe_summary(&self) -> Option<String> {
        let t = &self.transcriber;
        let r = t.running.as_ref()?;
        let pct = r.progress.fraction().map(|f| format!(" {:.0}%", f * 100.0)).unwrap_or_default();
        let more = if t.queue.is_empty() { String::new() } else { format!(" (+{})", t.queue.len()) };
        Some(format!("Transcribing “{}”{pct}{more}", r.item.title))
    }

    /// Each tick: apply a finished tile, start the next, tell the user about ended jobs.
    pub(super) fn poll_transcription(&mut self) {
        if let Some(r) = &self.transcriber.running {
            match r.rx.try_recv() {
                Ok(done) => {
                    let r = self.transcriber.running.take().expect("checked");
                    self.finish_tile(r, done);
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    let r = self.transcriber.running.take().expect("checked");
                    self.finish_tile(r, Err("the transcription stopped unexpectedly".into()));
                }
            }
        }
        if self.transcriber.running.is_none() {
            self.start_next();
        }
        self.report_transcription();
    }

    /// Start the next queued tile, skipping any that can no longer run.
    fn start_next(&mut self) {
        while let Some(item) = self.transcriber.queue.pop_front() {
            let Some(t) = self.store.library.tile(item.tile).cloned() else {
                self.tile_failed(&item, "the tile was removed".into());
                continue;
            };
            if let Some(why) = self.untranscribable(&t) {
                self.tile_failed(&item, why);
                continue;
            }
            let engine = match (self.transcriber.opener)(item.choice, &self.transcribe_context()) {
                Ok(e) => e,
                Err(why) => {
                    self.tile_failed(&item, why);
                    continue;
                }
            };
            let audio = match (self.store.audio(&t), &t.file) {
                (Some(a), _) => Audio::Decoded(a),
                (None, Some(f)) => Audio::File(self.store.media_dir.join(f)),
                (None, None) => {
                    self.tile_failed(&item, "it has no sound".into());
                    continue;
                }
            };
            let progress = Arc::new(Progress::default());
            let (tx, rx) = std::sync::mpsc::channel();
            let (range, lang, p) = (item.range, item.lang.clone(), progress.clone());
            let kind = engine.kind();
            let spawned = std::thread::Builder::new().name("transcribe".into()).spawn(move || {
                let done = transcribe::run(engine.as_ref(), audio, range, lang, &p);
                tx.send(done).ok();
                crate::quip::wake();
            });
            if let Err(e) = spawned {
                self.tile_failed(&item, format!("couldn't start: {e}"));
                continue;
            }
            self.transcriber.running = Some(Running { item, kind, progress, rx });
            return;
        }
    }

    fn job_mut(&mut self, job: u64) -> Option<&mut Job> {
        self.transcriber.jobs.iter_mut().find(|j| j.id == job)
    }

    fn tile_failed(&mut self, item: &Item, why: String) {
        let cancelled = why == transcribe::CANCELLED;
        if let Some(j) = self.job_mut(item.job) {
            j.done += 1;
            if cancelled {
                j.cancelled = true;
            } else {
                j.errors.push(json!({"tile": item.tile, "title": item.title, "error": why}));
            }
        }
    }

    /// A tile's transcription ended: apply it (one undo step) or record why not.
    fn finish_tile(&mut self, r: Running, done: Done) {
        let (heard, moved) = match done {
            Ok(d) => d,
            Err(why) => return self.tile_failed(&r.item, why),
        };
        let Some(t) = self.store.library.tile(r.item.tile).cloned() else {
            return self.tile_failed(&r.item, "the tile was removed".into());
        };
        if let Some((si, _)) = self.store.library.locate(t.id).filter(|(si, _)| self.store.library.sets[*si].locked) {
            let name = self.store.library.sets[si].name.clone();
            return self.tile_failed(&r.item, format!("“{name}” was locked meanwhile"));
        }
        let heard_lines = heard.lines.len();
        let heard_words: usize = heard.lines.iter().map(|l| l.words.len()).sum();
        let (model, lang) = (heard.model.clone(), heard.lang.clone());
        let applied = transcribe::apply(&t.subtitles, t.subtitles_source.as_ref(), r.kind, heard, r.item.targets.as_deref(), r.item.gaps);
        let changed = !transcribe::same_lines(&applied.lines, &t.subtitles) || applied.source != t.subtitles_source;
        if changed {
            let step = Step { label: "Transcribe", sets: self.store.library.sets.clone(), selected: self.store.library.selected_set };
            let h = &mut self.history;
            h.undo.push(step);
            if h.undo.len() > super::UNDO_LIMIT {
                h.undo.remove(0);
            }
            h.last = None;
            h.redo.clear();
            let held = h.media();
            self.store.hold_media(held);
            let tile = self.store.library.tile_mut(t.id).expect("checked");
            tile.subtitles = applied.lines.clone();
            tile.subtitles_source = applied.source.clone();
            self.store.mark_dirty();
        }
        let abs: Vec<f64> = moved.iter().map(|m| m.abs()).collect();
        let shifted = abs.iter().filter(|&&m| m > 1e-6).count();
        let mean = if abs.is_empty() { 0.0 } else { abs.iter().sum::<f64>() / abs.len() as f64 };
        let max = abs.iter().copied().fold(0.0, f64::max);
        let result = json!({
            "tile": t.id, "title": t.title, "engine": r.kind.name(), "model": model, "lang": lang,
            "heard": {"lines": heard_lines, "words": heard_words},
            "replaced": applied.replaced, "lines": applied.lines.len(), "kept_aligned": applied.kept_aligned,
            "aligned": {"lines": applied.aligned.lines, "matched": applied.aligned.matched, "placed": applied.aligned.placed, "missed": applied.aligned.missed},
            "gaps": {"added": applied.gaps.added, "replaced": applied.gaps.replaced, "kept": applied.gaps.kept},
            "refined": {"words": abs.len(), "moved": shifted, "mean_ms": (mean * 1000.0).round(), "max_ms": (max * 1000.0).round()},
            "changed": changed,
        });
        let title = t.title.clone();
        let note = if !changed {
            format!("Nothing new for “{title}”: {} heard nothing it could use", r.kind.label())
        } else if applied.replaced {
            format!("Transcribed “{title}”: {}", plural(applied.lines.len(), "line"))
        } else {
            let heard = plural(applied.gaps.added, "line");
            match (applied.aligned.lines, applied.gaps.added) {
                (0, 0) => format!("Took out {} it had heard in “{title}” before", plural(applied.gaps.replaced, "machine-made line")),
                (0, _) => format!("Added {heard} heard between the subtitles of “{title}”"),
                (n, 0) => format!("Timed the words of {} in “{title}”", plural(n, "line")),
                (n, _) => format!("Timed the words of {} in “{title}” and added {heard} heard between them", plural(n, "line")),
            }
        };
        if let Some(j) = self.job_mut(r.item.job) {
            j.done += 1;
            j.results.push(result);
        }
        let single = self.transcriber.jobs.iter().find(|j| j.id == r.item.job).is_some_and(|j| j.total == 1);
        if single {
            self.notify(note);
        }
    }

    /// Notices for jobs that ended since the last look.
    fn report_transcription(&mut self) {
        let mut notes = Vec::new();
        for j in self.transcriber.jobs.iter_mut().filter(|j| j.finished() && !j.reported) {
            j.reported = true;
            let ok = j.results.len();
            notes.push(match (j.cancelled, j.errors.first(), j.total) {
                (true, _, _) => ("Transcription cancelled".to_owned(), false),
                (false, Some(e), 1) => (format!("Couldn't transcribe “{}”: {}", e["title"].as_str().unwrap_or_default(), e["error"].as_str().unwrap_or_default()), true),
                (false, Some(e), _) => (format!("Transcribed {} of {}; couldn't do “{}”: {}", ok, j.total, e["title"].as_str().unwrap_or_default(), e["error"].as_str().unwrap_or_default()), true),
                // One tile said so itself when it finished.
                (false, None, 1) => continue,
                (false, None, n) => (format!("Transcribed {}", plural(n, "tile")), false),
            });
        }
        for (text, error) in notes {
            if error {
                self.notify_error(text);
            } else {
                self.notify(text);
            }
        }
    }
}

#[cfg(test)]
mod tests;
