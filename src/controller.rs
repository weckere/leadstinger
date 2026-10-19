//! The Controller: everything the app does, as commands (docs/CLI.md).
//!
//! It owns the library ([`Store`]), the audio [`Engine`], the cue pointers and the
//! view state the commands change (Perform/Edit, the selected tile, open windows).
//! [`Controller::execute`] runs any command in [`crate::commands::COMMANDS`] by id,
//! for the GUI's widgets, menus and shortcuts today and for the CLI (`leadstinger-cli`) from M7.
//!
//! What only the window can do (a file picker, editing a name in place, switching the
//! theme, a screenshot, quitting) comes back as an [`Effect`] for the GUI to carry out.

use crate::audio::{Engine, VoiceStatus};
use crate::commands::spec;
#[cfg(test)]
use crate::commands::COMMANDS;
use crate::logic::{
    assign_keys_by_position, db_to_gain, gain_to_db, next_cue, normalize_key, resolve_trigger, set_hotkey,
    tile_for_key, PlayState, TriggerAction, TriggerEvent,
};
use crate::model::{Appearance, ImageFit, ImageLayer, Layout, PlayMode, SoundSet, ThemeKind, Tile, TileColor};
use crate::picture::is_image_path;
use crate::store::{Media, Store};
use serde::de::DeserializeOwned;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use uuid::Uuid;

/// A message for the status bar.
pub struct Notice {
    pub text: String,
    pub error: bool,
    pub at: Instant,
}

/// Something a command asks the window to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    /// The theme or appearance changed: restyle egui.
    ApplyLook,
    /// `tile.add` without paths: open the file picker, then add to this set.
    PickAudio { set: Uuid, at: Option<usize> },
    /// `tile.replaceAudio` without a path.
    PickReplacement(Uuid),
    /// `tile.picture` without a path.
    PickPicture(Uuid),
    /// `set.import` without a path: choose a set file.
    PickSetFile,
    /// `set.export` without a path: ask where to save these sets (`None`: every set).
    SaveSetFile(Option<Uuid>),
    /// `set.create`/`set.rename` without a name: edit it in place.
    StartRename(Uuid),
    Screenshot(PathBuf),
    ClearClip,
    /// Playback was stopped wholesale or the set changed: forget held keys and buttons.
    ReleaseHeld,
    Quit,
}

/// A command that didn't run. `code` is stable for scripts and the CLI's exit codes.
#[derive(Clone, Debug, PartialEq)]
pub struct CmdError {
    pub code: &'static str,
    pub message: String,
    /// For `ambiguous`: the matches, so the caller can pick one.
    pub candidates: Vec<Value>,
}

impl CmdError {
    fn new(code: &'static str, message: impl Into<String>) -> CmdError {
        CmdError { code, message: message.into(), candidates: Vec::new() }
    }

    /// Informational outcomes (nothing broke) that the GUI shows without the error colour.
    pub fn is_info(&self) -> bool {
        matches!(self.code, "loading" | "no_audio" | "end_of_cues" | "nothing_to_paste")
    }
}

impl std::fmt::Display for CmdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

type Res = Result<Value, CmdError>;

fn bad(message: impl Into<String>) -> CmdError {
    CmdError::new("bad_params", message)
}

fn ok() -> Res {
    Ok(json!({}))
}

/// Typed access to a command's parameter object.
struct P<'a> {
    id: &'a str,
    m: &'a Map<String, Value>,
}

impl<'a> P<'a> {
    fn new(id: &'a str, v: &'a Value) -> Result<P<'a>, CmdError> {
        static EMPTY: std::sync::OnceLock<Map<String, Value>> = std::sync::OnceLock::new();
        match v {
            Value::Object(m) => Ok(P { id, m }),
            Value::Null => Ok(P { id, m: EMPTY.get_or_init(Map::new) }),
            _ => Err(bad(format!("{id}: parameters must be a JSON object"))),
        }
    }

    fn has(&self, k: &str) -> bool {
        self.m.contains_key(k)
    }

    /// A present, non-null value.
    fn get(&self, k: &str) -> Option<&'a Value> {
        self.m.get(k).filter(|v| !v.is_null())
    }

    fn str(&self, k: &str) -> Result<Option<&'a str>, CmdError> {
        match self.get(k) {
            None => Ok(None),
            Some(Value::String(s)) => Ok(Some(s)),
            Some(Value::Number(n)) => Err(bad(format!("{}: {k} must be text (got {n}; quote it)", self.id))),
            Some(_) => Err(bad(format!("{}: {k} must be text", self.id))),
        }
    }

    fn need_str(&self, k: &str) -> Result<&'a str, CmdError> {
        self.str(k)?.ok_or_else(|| bad(format!("{}: {k} is required", self.id)))
    }

    fn f64(&self, k: &str) -> Result<Option<f64>, CmdError> {
        match self.get(k) {
            None => Ok(None),
            Some(v) => v
                .as_f64()
                .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
                .map(Some)
                .ok_or_else(|| bad(format!("{}: {k} must be a number", self.id))),
        }
    }

    fn need_f64(&self, k: &str) -> Result<f64, CmdError> {
        self.f64(k)?.ok_or_else(|| bad(format!("{}: {k} is required", self.id)))
    }

    fn bool(&self, k: &str) -> Result<Option<bool>, CmdError> {
        match self.get(k) {
            None => Ok(None),
            Some(Value::Bool(b)) => Ok(Some(*b)),
            Some(Value::String(s)) => match s.as_str() {
                "true" | "on" | "yes" | "1" => Ok(Some(true)),
                "false" | "off" | "no" | "0" => Ok(Some(false)),
                _ => Err(bad(format!("{}: {k} must be true or false", self.id))),
            },
            Some(_) => Err(bad(format!("{}: {k} must be true or false", self.id))),
        }
    }

    /// An enum by its snake_case name, e.g. `"padd"` or `"underlay"`.
    fn named<T: DeserializeOwned>(&self, k: &str, choices: &str) -> Result<Option<T>, CmdError> {
        match self.str(k)? {
            None => Ok(None),
            Some(s) => serde_json::from_value(json!(s.to_ascii_lowercase()))
                .map(Some)
                .map_err(|_| bad(format!("{}: {k} must be one of {choices} (got {s:?})", self.id))),
        }
    }

    /// One path or a list of them.
    fn paths(&self, k: &str) -> Result<Option<Vec<PathBuf>>, CmdError> {
        match self.get(k) {
            None => Ok(None),
            Some(Value::String(s)) => Ok(Some(vec![PathBuf::from(s)])),
            Some(Value::Array(a)) => a
                .iter()
                .map(|v| v.as_str().map(PathBuf::from).ok_or_else(|| bad(format!("{}: {k} must be file paths", self.id))))
                .collect::<Result<Vec<_>, _>>()
                .map(Some),
            Some(_) => Err(bad(format!("{}: {k} must be a path or a list of paths", self.id))),
        }
    }
}

fn state_name(s: PlayState) -> &'static str {
    match s {
        PlayState::Stopped => "stopped",
        PlayState::Playing => "playing",
        PlayState::Paused => "paused",
        PlayState::Stopping => "stopping",
    }
}

fn enum_name<T: serde::Serialize>(v: T) -> Value {
    serde_json::to_value(v).unwrap_or(Value::Null)
}

pub struct Controller {
    pub store: Store,
    pub engine: Engine,
    /// Per set: the index of the next cue.
    pub cue: HashMap<Uuid, usize>,
    /// The tile most recently started, shown in the counter.
    pub last_started: Option<Uuid>,
    /// Voices this tick, refreshed by [`Controller::tick`].
    pub statuses: HashMap<Uuid, VoiceStatus>,
    pub edit_mode: bool,
    pub selected_tile: Option<Uuid>,
    pub show_settings: bool,
    pub show_about: bool,
    pub show_shortcuts: bool,
    /// Output devices, as last listed.
    pub devices: Vec<String>,
    pub notice: Option<Notice>,
    /// Work for the window, drained by the GUI each frame.
    pub effects: Vec<Effect>,
    /// The window runs on X11/XWayland (Linux): which clipboard tool to ask first.
    pub x11: bool,
    /// False for the offline CLI and `leadstinger-cli serve`: commands that would open a file
    /// picker, edit a name in place or take a screenshot then fail with `no_window`.
    pub has_window: bool,
    last_reopen: Instant,
    last_tick: Instant,
    /// `events.subscribe` listeners (the control socket's `watch`).
    subscribers: Vec<std::sync::mpsc::Sender<Value>>,
    /// What the last tick saw, to turn into events.
    seen: Seen,
}

/// The state events are derived from.
#[derive(Default)]
struct Seen {
    states: HashMap<Uuid, PlayState>,
    revision: u64,
    next_cue: Option<(Uuid, usize)>,
    master: f32,
    set: Option<Uuid>,
    edit_mode: bool,
}

impl Controller {
    pub fn new(store: Store, engine: Engine) -> Controller {
        engine.set_master(store.library.master_volume);
        let notice = store.load_warning.clone().map(|text| Notice { text, error: true, at: Instant::now() });
        Controller {
            store,
            engine,
            cue: HashMap::new(),
            last_started: None,
            statuses: HashMap::new(),
            edit_mode: false,
            selected_tile: None,
            show_settings: false,
            show_about: false,
            show_shortcuts: false,
            devices: Vec::new(),
            notice,
            effects: Vec::new(),
            x11: false,
            has_window: true,
            last_reopen: Instant::now(),
            last_tick: Instant::now(),
            subscribers: Vec::new(),
            seen: Seen::default(),
        }
    }

    /// Listen for events: `tile.started|paused|resumed|stopped|finished`,
    /// `library.changed`, `cue.changed`, `master.changed`, `set.selected`, `mode.changed`.
    pub fn subscribe(&mut self) -> std::sync::mpsc::Receiver<Value> {
        let (tx, rx) = std::sync::mpsc::channel();
        if self.subscribers.is_empty() {
            self.seen = self.snapshot(); // start from now, not from the app's launch
        }
        self.subscribers.push(tx);
        rx
    }

    /// [`Controller::tick`] by the time since the last tick.
    pub fn tick_now(&mut self) {
        let now = Instant::now();
        let dt = (now - self.last_tick).as_secs_f64();
        self.tick(dt);
    }

    fn snapshot(&self) -> Seen {
        let set = self.current_set();
        Seen {
            states: self.statuses.iter().map(|(id, s)| (*id, s.state)).collect(),
            revision: self.store.revision,
            next_cue: Some((set.id, self.cue.get(&set.id).copied().unwrap_or(0))),
            master: self.store.library.master_volume,
            set: Some(set.id),
            edit_mode: self.edit_mode,
        }
    }

    fn publish(&mut self, finished: &[Uuid]) {
        if self.subscribers.is_empty() {
            return;
        }
        let now = self.snapshot();
        let old = std::mem::take(&mut self.seen);
        let mut events = Vec::new();
        let tile = |c: &Controller, id: &Uuid| {
            json!(c.store.library.tile(*id).map(|t| json!({"tile": t.id, "title": t.title})).unwrap_or(json!({"tile": id})))
        };
        for (id, state) in &now.states {
            let name = match (old.states.get(id), state) {
                (None, PlayState::Playing) => "tile.started",
                (Some(PlayState::Playing), PlayState::Paused) => "tile.paused",
                (Some(PlayState::Paused), PlayState::Playing) => "tile.resumed",
                (Some(PlayState::Stopping), PlayState::Playing) => "tile.started",
                (Some(PlayState::Playing | PlayState::Paused), PlayState::Stopping) => "tile.stopping",
                _ => continue,
            };
            let mut e = tile(self, id);
            e["event"] = json!(name);
            events.push(e);
        }
        for id in old.states.keys().filter(|id| !now.states.contains_key(id)) {
            let mut e = tile(self, id);
            e["event"] = json!(if finished.contains(id) { "tile.finished" } else { "tile.stopped" });
            events.push(e);
        }
        if now.revision != old.revision {
            events.push(json!({"event": "library.changed", "revision": now.revision}));
        }
        if now.set != old.set {
            events.push(json!({"event": "set.selected", "set": self.current_set().id, "name": self.current_set().name}));
        } else if now.next_cue != old.next_cue {
            let (_, i) = now.next_cue.unwrap_or_default();
            events.push(json!({"event": "cue.changed", "set": self.current_set().name, "next": i + 1}));
        }
        if now.master != old.master {
            events.push(json!({"event": "master.changed", "master": now.master}));
        }
        if now.edit_mode != old.edit_mode {
            events.push(json!({"event": "mode.changed", "edit_mode": now.edit_mode}));
        }
        self.seen = now;
        if !events.is_empty() {
            self.subscribers.retain(|tx| events.iter().all(|e| tx.send(e.clone()).is_ok()));
        }
    }

    pub fn set_idx(&self) -> usize {
        self.store.library.selected_index()
    }

    pub fn current_set(&self) -> &SoundSet {
        &self.store.library.sets[self.set_idx()]
    }

    pub fn notify(&mut self, text: impl Into<String>) {
        self.notice = Some(Notice { text: text.into(), error: false, at: Instant::now() });
    }

    pub fn notify_error(&mut self, text: impl Into<String>) {
        self.notice = Some(Notice { text: text.into(), error: true, at: Instant::now() });
    }

    /// Show a failed command in the status bar.
    pub fn report(&mut self, e: &CmdError) {
        if e.is_info() {
            self.notify(e.message.clone());
        } else {
            self.notify_error(e.message.clone());
        }
    }

    /// Fail a command that needs the window when there isn't one.
    fn needs_window(&self, id: &str, missing: &str) -> Result<(), CmdError> {
        if self.has_window {
            Ok(())
        } else {
            Err(CmdError::new("no_window", format!("{id} needs {missing}: there's no window to ask in")))
        }
    }

    /// Advance time: decodes, a lost device, finished voices and auto-advance.
    pub fn tick(&mut self, dt: f64) {
        self.last_tick = Instant::now();
        self.engine.tick_without_device(dt);
        self.store.poll();
        if self.engine.stream_failed() && self.last_reopen.elapsed() > Duration::from_secs(3) {
            self.last_reopen = Instant::now();
            let dev = self.store.library.output_device.clone();
            self.engine.open(dev.as_deref());
            self.engine.set_master(self.store.library.master_volume);
        }
        self.statuses = self.engine.statuses();
        let finished = self.engine.take_finished();
        for &id in &finished {
            let Some((si, ti)) = self.store.library.locate(id) else { continue };
            let set = &self.store.library.sets[si];
            if set.layout == Layout::List && set.auto_advance && self.cue.get(&set.id) == Some(&(ti + 1)) {
                if let Err(e) = self.play_next_cue(si) {
                    self.report(&e);
                }
            }
        }
        self.publish(&finished);
    }

    // ── references ──────────────────────────────────────────────────────────

    /// A set by UUID, name (case-insensitive) or `#N`; `None` is the current set.
    pub fn resolve_set(&self, r: Option<&str>) -> Result<usize, CmdError> {
        let Some(r) = r.map(str::trim) else { return Ok(self.set_idx()) };
        let sets = &self.store.library.sets;
        if let Some(n) = r.strip_prefix('#') {
            let n: usize = n.parse().map_err(|_| bad(format!("bad set position {r:?}")))?;
            return (1..=sets.len()).contains(&n).then(|| n - 1).ok_or_else(|| {
                CmdError::new("not_found", format!("there's no set #{n} (there are {})", sets.len()))
            });
        }
        if let Ok(id) = Uuid::parse_str(r) {
            if let Some(i) = self.store.library.set_index(id) {
                return Ok(i);
            }
        }
        let hits: Vec<usize> = (0..sets.len()).filter(|&i| sets[i].name.eq_ignore_ascii_case(r)).collect();
        match hits.as_slice() {
            [i] => Ok(*i),
            [] => Err(CmdError::new("not_found", format!("no set called {r:?}"))),
            _ => Err(CmdError {
                code: "ambiguous",
                message: format!("{} sets are called {r:?}; use #N or an id", hits.len()),
                candidates: hits.iter().map(|&i| json!({"set": sets[i].id, "name": sets[i].name, "position": i + 1})).collect(),
            }),
        }
    }

    /// A tile by UUID, `key:q`, `#N`, `Set/Title` or title (the current set first).
    pub fn resolve_tile(&self, r: &str) -> Result<Uuid, CmdError> {
        let r = r.trim();
        let lib = &self.store.library;
        let cur = self.current_set();
        if let Ok(id) = Uuid::parse_str(r) {
            if lib.tile(id).is_some() {
                return Ok(id);
            }
        }
        if let Some(k) = r.strip_prefix("key:") {
            let c = k.chars().next().filter(|_| k.chars().count() == 1).and_then(normalize_key);
            let c = c.ok_or_else(|| bad(format!("{r:?}: a hotkey is one of 1–0, a–z, ; , . /")))?;
            return tile_for_key(&cur.tiles, c)
                .map(|t| t.id)
                .ok_or_else(|| CmdError::new("not_found", format!("no tile has the hotkey {} in “{}”", c.to_ascii_uppercase(), cur.name)));
        }
        if let Some(n) = r.strip_prefix('#') {
            let n: usize = n.parse().map_err(|_| bad(format!("bad tile position {r:?}")))?;
            return cur.tiles.get(n.wrapping_sub(1)).map(|t| t.id).ok_or_else(|| {
                CmdError::new("not_found", format!("“{}” has no tile #{n} (it has {})", cur.name, cur.tiles.len()))
            });
        }
        let candidate = |si: usize, t: &Tile| json!({"tile": t.id, "title": t.title, "set": lib.sets[si].name});
        // "Set/Title": try every split point, since either part may contain a slash.
        for (i, _) in r.match_indices('/') {
            let (set_name, title) = (&r[..i], &r[i + 1..]);
            if let Some(si) = lib.sets.iter().position(|s| s.name.eq_ignore_ascii_case(set_name)) {
                let hits: Vec<&Tile> = lib.sets[si].tiles.iter().filter(|t| t.title.eq_ignore_ascii_case(title)).collect();
                match hits.as_slice() {
                    [t] => return Ok(t.id),
                    [] => {}
                    _ => {
                        return Err(CmdError {
                            code: "ambiguous",
                            message: format!("{} tiles in “{}” are called {title:?}", hits.len(), lib.sets[si].name),
                            candidates: hits.iter().map(|t| candidate(si, t)).collect(),
                        })
                    }
                }
            }
        }
        let si_cur = self.set_idx();
        for scope in [Some(si_cur), None] {
            let hits: Vec<(usize, &Tile)> = lib
                .sets
                .iter()
                .enumerate()
                .filter(|(si, _)| scope.is_none_or(|s| s == *si))
                .flat_map(|(si, s)| s.tiles.iter().map(move |t| (si, t)))
                .filter(|(_, t)| t.title.eq_ignore_ascii_case(r))
                .collect();
            match hits.as_slice() {
                [(_, t)] => return Ok(t.id),
                [] => continue,
                _ => {
                    return Err(CmdError {
                        code: "ambiguous",
                        message: format!("{} tiles are called {r:?}; use Set/Title, #N or an id", hits.len()),
                        candidates: hits.iter().map(|(si, t)| candidate(*si, t)).collect(),
                    })
                }
            }
        }
        Err(CmdError::new("not_found", format!("no tile called {r:?}")))
    }

    fn tile_param(&self, p: &P) -> Result<Uuid, CmdError> {
        self.resolve_tile(p.need_str("tile")?)
    }

    fn set_param(&self, p: &P) -> Result<usize, CmdError> {
        self.resolve_set(p.str("set")?)
    }

    // ── playback ────────────────────────────────────────────────────────────

    fn trigger(&mut self, id: Uuid, event: TriggerEvent) -> Result<(), CmdError> {
        let Some(tile) = self.store.library.tile(id).cloned() else { return Ok(()) };
        match resolve_trigger(tile.mode, self.engine.state(id), event) {
            TriggerAction::None => {}
            TriggerAction::Play => self.start(&tile)?,
            TriggerAction::Stop => self.engine.stop(id, tile.fade_out),
            TriggerAction::Pause => self.engine.pause(id),
            TriggerAction::Resume => self.engine.resume(id),
        }
        Ok(())
    }

    fn start(&mut self, tile: &Tile) -> Result<(), CmdError> {
        match self.store.media(tile) {
            Some(Media::Ready { audio, .. }) => {
                let audio = audio.clone();
                self.engine.play(tile, audio);
                self.last_started = Some(tile.id);
                if let Some((si, ti)) = self.store.library.locate(tile.id) {
                    self.cue.insert(self.store.library.sets[si].id, ti + 1);
                }
                Ok(())
            }
            Some(Media::Loading) => Err(CmdError::new("loading", format!("“{}” is still loading…", tile.title))),
            Some(Media::Failed(e)) => Err(CmdError::new("decode_failed", format!("Can't play “{}”: {e}", tile.title))),
            None => Err(CmdError::new("no_audio", format!("“{}” has no audio. Drop a file on it.", tile.title))),
        }
    }

    fn play_next_cue(&mut self, set_idx: usize) -> Result<Value, CmdError> {
        let set = &self.store.library.sets[set_idx];
        let from = self.cue.get(&set.id).copied().unwrap_or(0);
        match next_cue(&set.tiles, from) {
            Some(i) => {
                let tile = set.tiles[i].clone();
                self.start(&tile)?;
                Ok(json!({"tile": tile.id, "title": tile.title, "position": i + 1}))
            }
            None => Err(CmdError::new("end_of_cues", "End of the cue list. Press ⏮ to go back to the top.")),
        }
    }

    fn set_master(&mut self, v: f32) -> Res {
        let v = v.clamp(0.0, 1.0);
        self.store.library.master_volume = v;
        self.engine.set_master(v);
        self.store.mark_dirty();
        Ok(json!({"master": v, "master_db": if v <= 0.0 { Value::Null } else { json!(gain_to_db(v)) }}))
    }

    fn paste(&mut self, text: Option<&str>) -> Res {
        let files = crate::paste::clipboard_files(self.x11, text);
        if files.is_empty() {
            return Err(CmdError::new("nothing_to_paste", "Nothing to paste. Copy sound files in your file manager, then paste here."));
        }
        // Set files become new sets.
        let (set_files, files): (Vec<PathBuf>, Vec<PathBuf>) = files.into_iter().partition(|p| crate::setfile::is_set_file(p));
        let mut sets = Vec::new();
        for f in &set_files {
            match self.execute("set.import", json!({"path": f})) {
                Ok(v) => sets.push(v),
                Err(e) => self.notify_error(e.message),
            }
        }
        let (images, sounds): (Vec<PathBuf>, Vec<PathBuf>) = files.into_iter().partition(|p| is_image_path(p));
        let mut picture = Value::Null;
        if let Some(img) = images.first() {
            match self.selected_tile {
                Some(id) => {
                    self.store.set_image(id, img).map_err(|e| CmdError::new("failed", format!("Couldn't use that picture: {e:#}")))?;
                    picture = json!(id);
                }
                None => self.notify("Select a tile (Edit mode), then paste a picture to put it on that tile."),
            }
        }
        let mut added = Vec::new();
        let mut errors = Vec::new();
        if !sounds.is_empty() {
            let si = self.set_idx();
            (added, errors) = self.store.import(&sounds, si, None);
            self.report_import(added.len(), &errors);
        }
        Ok(json!({"added": added, "errors": errors, "picture": picture, "sets": sets}))
    }

    fn report_import(&mut self, added: usize, errors: &[String]) {
        if !errors.is_empty() {
            self.notify_error(format!("Added {added}; skipped {}: {}", errors.len(), errors.join("; ")));
        } else if added > 0 {
            self.notify(format!("Added {added} sound{}", if added == 1 { "" } else { "s" }));
        }
    }

    // ── views of the library ────────────────────────────────────────────────

    pub fn tile_json(&self, id: Uuid) -> Value {
        let lib = &self.store.library;
        let Some((si, ti)) = lib.locate(id) else { return Value::Null };
        let t = &lib.sets[si].tiles[ti];
        let media = match self.store.media(t) {
            None if t.file.is_some() => "not_loaded",
            None => "none",
            Some(Media::Loading) => "loading",
            Some(Media::Ready { .. }) => "ready",
            Some(Media::Failed(_)) => "failed",
        };
        json!({
            "id": t.id,
            "title": t.title,
            "set": lib.sets[si].name,
            "position": ti + 1,
            "key": t.hotkey.map(|c| c.to_string()),
            "color": enum_name(t.color),
            "mode": enum_name(t.mode),
            "volume": t.volume,
            "loop": t.looping,
            "solo": t.solo,
            "fade_in": t.fade_in,
            "fade_out": t.fade_out,
            "trim_start": t.start,
            "trim_end": t.end,
            "duration": t.duration,
            "audio": media,
            "picture": t.image.is_some(),
            "picture_layer": enum_name(t.image_layer),
            "picture_fit": enum_name(t.image_fit),
            "picture_opacity": t.image_opacity,
            "state": state_name(self.engine.state(t.id)),
        })
    }

    fn status(&self) -> Value {
        let lib = &self.store.library;
        let mut playing: Vec<(Uuid, VoiceStatus)> = self.engine.statuses().into_iter().collect();
        playing.sort_by(|a, b| a.1.remaining().total_cmp(&b.1.remaining()));
        let set = self.current_set();
        let pointer = self.cue.get(&set.id).copied().unwrap_or(0);
        let v = lib.master_volume;
        json!({
            "playing": playing.iter().map(|(id, s)| {
                let t = lib.tile(*id);
                json!({
                    "tile": id,
                    "title": t.map(|t| t.title.clone()),
                    "set": lib.locate(*id).map(|(si, _)| lib.sets[si].name.clone()),
                    "state": state_name(s.state),
                    "position": s.position,
                    "remaining": s.remaining(),
                    "length": s.length,
                })
            }).collect::<Vec<_>>(),
            "master": v,
            "master_db": if v <= 0.0 { Value::Null } else { json!(gain_to_db(v)) },
            "device": self.engine.device_name,
            "device_error": self.engine.error,
            "sample_rate": self.engine.sample_rate(),
            "set": set.name,
            "layout": enum_name(set.layout),
            "next_cue": next_cue(&set.tiles, pointer).map(|i| json!({"position": i + 1, "title": set.tiles[i].title})),
            "edit_mode": self.edit_mode,
            "selected": self.selected_tile,
            "theme": enum_name(lib.theme),
            "loading": self.store.is_loading(),
            "window": self.has_window,
        })
    }

    // ── execute ─────────────────────────────────────────────────────────────

    /// Run one command. `params` is a JSON object (or null for none).
    pub fn execute(&mut self, id: &str, params: Value) -> Res {
        if spec(id).is_none() {
            return Err(CmdError::new("unknown_command", format!("no command {id:?}; `{} commands` lists them", crate::brand::CLI_NAME)));
        }
        let p = P::new(id, &params)?;
        match id {
            // ── status
            "status" => Ok(self.status()),
            "library.get" => serde_json::to_value(&self.store.library).map_err(|e| CmdError::new("failed", e.to_string())),
            "library.path" => Ok(json!({"library": self.store.library_path(), "data_dir": self.store.data_dir})),

            // ── transport
            "tile.trigger" => {
                let id = self.tile_param(&p)?;
                let event = match p.str("event")? {
                    None | Some("press") => TriggerEvent::Press,
                    Some("release") => TriggerEvent::Release,
                    Some(e) => return Err(bad(format!("tile.trigger: event must be press or release (got {e:?})"))),
                };
                self.trigger(id, event)?;
                Ok(json!({"tile": id, "state": state_name(self.engine.state(id))}))
            }
            "tile.play" => {
                let id = self.tile_param(&p)?;
                let state = self.engine.state(id);
                if p.bool("restart")?.unwrap_or(false) || !matches!(state, PlayState::Playing | PlayState::Paused) {
                    let tile = self.store.library.tile(id).cloned().expect("resolved");
                    self.start(&tile)?;
                } else if state == PlayState::Paused {
                    self.engine.resume(id);
                }
                Ok(json!({"tile": id, "state": state_name(self.engine.state(id))}))
            }
            "tile.stop" => {
                let id = self.tile_param(&p)?;
                let fade = match p.f64("fade")? {
                    Some(f) => f.max(0.0) as f32,
                    None => self.store.library.tile(id).map_or(0.0, |t| t.fade_out),
                };
                self.engine.stop(id, fade);
                Ok(json!({"tile": id, "state": state_name(self.engine.state(id))}))
            }
            "tile.pause" | "tile.resume" => {
                let id = self.tile_param(&p)?;
                if self.engine.state(id) == PlayState::Stopped {
                    return Err(CmdError::new("not_playing", "that tile isn't playing"));
                }
                if p.id == "tile.pause" {
                    self.engine.pause(id);
                } else {
                    self.engine.resume(id);
                }
                Ok(json!({"tile": id, "state": state_name(self.engine.state(id))}))
            }
            "tile.preview" => {
                let id = self.tile_param(&p)?;
                if self.engine.state(id) == PlayState::Stopped {
                    let tile = self.store.library.tile(id).cloned().expect("resolved");
                    self.start(&tile)?;
                } else {
                    self.engine.stop(id, 0.05);
                }
                Ok(json!({"tile": id, "state": state_name(self.engine.state(id))}))
            }
            "transport.stopAll" => {
                let fade = if p.bool("immediate")?.unwrap_or(false) {
                    0.0
                } else {
                    p.f64("fade")?.map_or(self.store.library.stop_all_fade, |f| f.max(0.0) as f32)
                };
                self.engine.stop_all(fade);
                self.effects.push(Effect::ReleaseHeld);
                Ok(json!({"fade": fade}))
            }
            "cue.go" => {
                let si = self.set_param(&p)?;
                self.play_next_cue(si)
            }
            "cue.set" | "cue.top" => {
                let si = self.set_param(&p)?;
                let set = &self.store.library.sets[si];
                let index = if id == "cue.top" { 1 } else { p.need_f64("index")? as usize };
                if !(1..=set.tiles.len() + 1).contains(&index) {
                    return Err(bad(format!("cue.set: index must be 1–{} in “{}”", set.tiles.len() + 1, set.name)));
                }
                self.cue.insert(set.id, index - 1);
                Ok(json!({"set": set.name, "next": index}))
            }

            // ── master
            "master.volume" => match (p.f64("volume")?, p.f64("db")?) {
                (Some(v), _) => self.set_master(v as f32),
                (None, Some(db)) => self.set_master(if db <= -60.0 { 0.0 } else { db_to_gain(db.min(0.0) as f32) }),
                (None, None) => Err(bad("master.volume: give volume (0-1) or db")),
            },
            "master.up" | "master.down" => {
                // 3 dB steps; down from −60 dB goes to silence, up from silence to −60.
                let v = self.store.library.master_volume;
                let db = if v <= 0.0 { -63.0 } else { gain_to_db(v) };
                let db = if id == "master.up" { (db + 3.0).min(0.0) } else { db - 3.0 };
                self.set_master(if db < -60.0 { 0.0 } else { db_to_gain(db) })
            }
            "meter.clearClip" => {
                self.effects.push(Effect::ClearClip);
                ok()
            }

            // ── sets
            "set.list" => {
                let lib = &self.store.library;
                let cur = self.set_idx();
                Ok(json!(lib
                    .sets
                    .iter()
                    .enumerate()
                    .map(|(i, s)| json!({
                        "id": s.id,
                        "name": s.name,
                        "position": i + 1,
                        "tiles": s.tiles.len(),
                        "layout": enum_name(s.layout),
                        "columns": s.columns,
                        "auto_advance": s.auto_advance,
                        "current": i == cur,
                    }))
                    .collect::<Vec<_>>()))
            }
            "set.create" => {
                let n = self.store.library.sets.len() + 1;
                let name = p.str("name")?.map(str::trim).filter(|s| !s.is_empty());
                let set = SoundSet::named(name.map_or_else(|| format!("Set {n}"), str::to_owned));
                let new_id = set.id;
                self.store.library.sets.push(set);
                self.store.library.selected_set = Some(new_id);
                self.selected_tile = None;
                self.store.mark_dirty();
                if name.is_none() && self.has_window {
                    self.effects.push(Effect::StartRename(new_id));
                }
                Ok(json!({"id": new_id}))
            }
            "set.rename" => {
                let si = self.set_param(&p)?;
                let sid = self.store.library.sets[si].id;
                match p.str("name")?.map(str::trim) {
                    None => {
                        self.needs_window(id, "a name")?;
                        self.effects.push(Effect::StartRename(sid));
                        ok()
                    }
                    Some("") => Err(bad("set.rename: the name can't be empty")),
                    Some(name) => {
                        self.store.library.sets[si].name = name.to_owned();
                        self.store.mark_dirty();
                        Ok(json!({"id": sid, "name": name}))
                    }
                }
            }
            "set.duplicate" => {
                let si = self.set_param(&p)?;
                let copy = self.store.library.sets[si].duplicate();
                let new_id = copy.id;
                self.store.library.sets.insert(si + 1, copy);
                self.store.library.selected_set = Some(new_id);
                self.store.mark_dirty();
                Ok(json!({"id": new_id}))
            }
            "set.delete" => {
                let si = self.set_param(&p)?;
                if self.store.library.sets.len() <= 1 {
                    return Err(CmdError::new("last_set", "the last set can't be deleted"));
                }
                for t in &self.store.library.sets[si].tiles {
                    self.engine.stop(t.id, 0.0);
                }
                self.selected_tile = None;
                let name = self.store.library.sets[si].name.clone();
                self.store.remove_set(si);
                Ok(json!({"deleted": name}))
            }
            "set.select" => {
                let si = self.resolve_set(Some(p.need_str("set")?))?;
                let sid = self.store.library.sets[si].id;
                if self.store.library.selected_set != Some(sid) {
                    self.store.library.selected_set = Some(sid);
                    self.selected_tile = None;
                    self.effects.push(Effect::ReleaseHeld);
                    self.store.mark_dirty();
                }
                Ok(json!({"id": sid, "name": self.store.library.sets[si].name}))
            }
            "set.layout" => {
                let si = self.set_param(&p)?;
                let layout: Layout = p.named("layout", "grid, list")?.ok_or_else(|| bad("set.layout: layout is required"))?;
                self.store.library.sets[si].layout = layout;
                self.store.mark_dirty();
                ok()
            }
            "set.columns" => {
                let si = self.set_param(&p)?;
                let n = p.need_f64("columns")?;
                if !(1.0..=12.0).contains(&n) {
                    return Err(bad("set.columns: columns must be 1–12"));
                }
                self.store.library.sets[si].columns = n as u32;
                self.store.mark_dirty();
                ok()
            }
            "set.autoAdvance" => {
                let si = self.set_param(&p)?;
                let set = &mut self.store.library.sets[si];
                set.auto_advance = p.bool("on")?.unwrap_or(!set.auto_advance);
                let on = set.auto_advance;
                self.store.mark_dirty();
                Ok(json!({"on": on}))
            }
            "set.reassignKeys" => {
                let si = self.set_param(&p)?;
                assign_keys_by_position(&mut self.store.library.sets[si].tiles);
                self.store.mark_dirty();
                ok()
            }

            // ── tiles
            "set.import" => match p.str("path")? {
                None => {
                    self.needs_window(id, "a path")?;
                    self.effects.push(Effect::PickSetFile);
                    ok()
                }
                Some(path) => {
                    let sets = crate::setfile::import(&mut self.store, path.as_ref())
                        .map_err(|e| CmdError::new("failed", format!("Couldn't import {path}: {e:#}")))?;
                    // Show the first new set.
                    self.store.library.selected_set = Some(sets[0].id);
                    self.selected_tile = None;
                    self.effects.push(Effect::ReleaseHeld);
                    let tiles: usize = sets.iter().map(|s| s.tiles).sum();
                    let what = match sets.as_slice() {
                        [one] => format!("“{}”", one.name),
                        many => format!("{} sets", many.len()),
                    };
                    self.notify(format!("Imported {what} ({tiles} sound{})", if tiles == 1 { "" } else { "s" }));
                    Ok(json!({"sets": sets}))
                }
            },
            "set.export" => {
                let all = p.bool("all")?.unwrap_or(false);
                let si = self.set_param(&p)?;
                match p.str("path")? {
                    None => {
                        self.needs_window(id, "a path")?;
                        self.effects.push(Effect::SaveSetFile((!all).then(|| self.store.library.sets[si].id)));
                        ok()
                    }
                    Some(path) => {
                        let sets: Vec<usize> = if all { (0..self.store.library.sets.len()).collect() } else { vec![si] };
                        let out = crate::setfile::export(&self.store, &sets, path.as_ref())
                            .map_err(|e| CmdError::new("failed", format!("Couldn't export: {e:#}")))?;
                        let name = out.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        if out.skipped.is_empty() {
                            self.notify(format!("Exported {} sound{} to {name}", out.tiles, if out.tiles == 1 { "" } else { "s" }));
                        } else {
                            self.notify_error(format!("Exported to {name}; left out (audio missing): {}", out.skipped.join(", ")));
                        }
                        Ok(json!(out))
                    }
                }
            }
            "tile.list" => {
                let query = p.str("query")?.map(str::to_lowercase);
                let sets: Vec<usize> = match p.str("set")? {
                    Some("*") => (0..self.store.library.sets.len()).collect(),
                    r => vec![self.resolve_set(r)?],
                };
                let ids: Vec<Uuid> = sets
                    .iter()
                    .flat_map(|&si| self.store.library.sets[si].tiles.iter())
                    .filter(|t| query.as_ref().is_none_or(|q| t.title.to_lowercase().contains(q)))
                    .map(|t| t.id)
                    .collect();
                Ok(json!(ids.iter().map(|&id| self.tile_json(id)).collect::<Vec<_>>()))
            }
            "tile.get" => {
                let id = self.tile_param(&p)?;
                Ok(self.tile_json(id))
            }
            "tile.add" => {
                let si = self.set_param(&p)?;
                let at = p.f64("at")?.map(|a| (a.max(1.0) as usize) - 1);
                match p.paths("paths")? {
                    None => {
                        self.needs_window(id, "paths")?;
                        self.effects.push(Effect::PickAudio { set: self.store.library.sets[si].id, at });
                        ok()
                    }
                    Some(paths) => {
                        let (added, errors) = self.store.import(&paths, si, at);
                        self.report_import(added.len(), &errors);
                        if added.is_empty() && !errors.is_empty() {
                            return Err(CmdError::new("failed", errors.join("; ")));
                        }
                        Ok(json!({"added": added, "errors": errors}))
                    }
                }
            }
            "tile.paste" => self.paste(p.str("text")?),
            "tile.replaceAudio" => {
                let id = self.tile_param(&p)?;
                match p.str("path")? {
                    None => {
                        self.needs_window("tile.replaceAudio", "a path")?;
                        self.effects.push(Effect::PickReplacement(id));
                        ok()
                    }
                    Some(path) => {
                        self.engine.stop(id, 0.0);
                        self.store
                            .replace_audio(id, path.as_ref())
                            .map_err(|e| CmdError::new("failed", format!("Couldn't replace the audio: {e:#}")))?;
                        ok()
                    }
                }
            }
            "tile.duplicate" => {
                let id = self.tile_param(&p)?;
                let (s, t) = self.store.library.locate(id).expect("resolved");
                let copy = self.store.library.sets[s].tiles[t].duplicate();
                let new_id = copy.id;
                self.store.library.sets[s].tiles.insert(t + 1, copy);
                self.store.mark_dirty();
                Ok(json!({"id": new_id}))
            }
            "tile.remove" => {
                let id = self.tile_param(&p)?;
                let title = self.store.library.tile(id).map(|t| t.title.clone());
                self.engine.stop(id, 0.0);
                if self.selected_tile == Some(id) {
                    self.selected_tile = None;
                }
                self.store.remove_tile(id);
                Ok(json!({"removed": title}))
            }
            "tile.move" => {
                let id = self.tile_param(&p)?;
                let to = p.need_f64("to")?.max(1.0) as usize - 1;
                let (s, from) = self.store.library.locate(id).expect("resolved");
                let tiles = &mut self.store.library.sets[s].tiles;
                let tile = tiles.remove(from);
                let to = to.min(tiles.len());
                tiles.insert(to, tile);
                self.store.mark_dirty();
                Ok(json!({"position": to + 1}))
            }
            "tile.set" => self.tile_set(&p),
            "tile.picture" => {
                let id = self.tile_param(&p)?;
                match p.str("path")? {
                    None => {
                        self.needs_window("tile.picture", "a path")?;
                        self.effects.push(Effect::PickPicture(id));
                        ok()
                    }
                    Some(path) => {
                        self.store
                            .set_image(id, path.as_ref())
                            .map_err(|e| CmdError::new("failed", format!("Couldn't use that picture: {e:#}")))?;
                        ok()
                    }
                }
            }
            "tile.pictureClear" => {
                let id = self.tile_param(&p)?;
                self.store.clear_image(id);
                ok()
            }

            // ── view
            "view.perform" | "view.edit" | "view.toggleEdit" => {
                self.edit_mode = match id {
                    "view.perform" => false,
                    "view.edit" => true,
                    _ => !self.edit_mode,
                };
                Ok(json!({"edit_mode": self.edit_mode}))
            }
            "view.select" => {
                self.selected_tile = match p.str("tile")? {
                    Some(r) => Some(self.resolve_tile(r)?),
                    None => None,
                };
                ok()
            }
            "view.waveforms" => {
                let lib = &mut self.store.library;
                lib.show_waveforms = p.bool("on")?.unwrap_or(!lib.show_waveforms);
                let on = lib.show_waveforms;
                self.store.mark_dirty();
                Ok(json!({"on": on}))
            }
            "view.theme" => {
                let k: ThemeKind = p.named("theme", "console, padd")?.ok_or_else(|| bad("view.theme: theme is required"))?;
                self.store.library.theme = k;
                self.store.mark_dirty();
                self.effects.push(Effect::ApplyLook);
                ok()
            }
            "view.appearance" => {
                let a: Appearance = p
                    .named("appearance", "dark, light, system")?
                    .ok_or_else(|| bad("view.appearance: appearance is required"))?;
                self.store.library.appearance = a;
                self.store.mark_dirty();
                self.effects.push(Effect::ApplyLook);
                ok()
            }
            "view.settings" => {
                self.show_settings = p.bool("open")?.unwrap_or(true);
                if self.show_settings {
                    self.devices = Engine::output_devices();
                }
                ok()
            }
            "view.shortcuts" => {
                self.show_shortcuts = true;
                ok()
            }
            "view.about" => {
                self.show_about = true;
                ok()
            }
            "view.screenshot" => {
                let path = PathBuf::from(p.need_str("path")?);
                self.needs_window(id, "a window")?;
                self.effects.push(Effect::Screenshot(path));
                ok()
            }
            "app.quit" => {
                if let Err(e) = self.store.save_now() {
                    eprintln!("{}: failed to save library: {e:#}", crate::brand::SLUG);
                }
                self.effects.push(Effect::Quit);
                ok()
            }

            // ── settings
            "device.list" => {
                self.devices = Engine::output_devices();
                Ok(json!({"devices": self.devices, "current": self.store.library.output_device}))
            }
            "device.set" | "device.reconnect" => {
                if id == "device.set" {
                    if !p.has("name") {
                        return Err(bad("device.set: name is required (null for the system default)"));
                    }
                    self.store.library.output_device = p.str("name")?.map(str::to_owned);
                    self.store.mark_dirty();
                }
                let dev = self.store.library.output_device.clone();
                self.engine.open(dev.as_deref());
                self.engine.set_master(self.store.library.master_volume);
                match &self.engine.error {
                    Some(e) => Err(CmdError::new("device_failed", e.clone())),
                    None => Ok(json!({"device": self.engine.device_name, "sample_rate": self.engine.sample_rate()})),
                }
            }
            "settings.stopAllFade" => {
                let s = p.need_f64("seconds")?;
                if !(0.0..=5.0).contains(&s) {
                    return Err(bad("settings.stopAllFade: seconds must be 0–5"));
                }
                self.store.library.stop_all_fade = s as f32;
                self.store.mark_dirty();
                ok()
            }
            "settings.windowSystem" => {
                self.store.library.use_x11 = p.bool("x11")?.ok_or_else(|| bad("settings.windowSystem: x11 is required"))?;
                self.store.mark_dirty();
                ok()
            }
            "library.reveal" => {
                let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
                std::process::Command::new(opener)
                    .arg(&self.store.data_dir)
                    .spawn()
                    .map_err(|e| CmdError::new("failed", format!("Couldn't open the folder: {e}")))?;
                ok()
            }
            other => unreachable!("command {other} is registered but not implemented"),
        }
    }

    fn tile_set(&mut self, p: &P) -> Res {
        let id = self.tile_param(p)?;
        let mut t = self.store.library.tile(id).cloned().expect("resolved");
        let unit = |k: &str, v: Option<f64>| -> Result<Option<f32>, CmdError> {
            match v {
                Some(x) if !(0.0..=1.0).contains(&x) => Err(bad(format!("tile.set: {k} must be 0–1"))),
                x => Ok(x.map(|x| x as f32)),
            }
        };
        let secs = |k: &str, v: Option<f64>| -> Result<Option<f32>, CmdError> {
            match v {
                Some(x) if x < 0.0 => Err(bad(format!("tile.set: {k} can't be negative"))),
                x => Ok(x.map(|x| x as f32)),
            }
        };
        // Stored exactly as given: the inspector sends every keystroke, so a trailing
        // space or a briefly empty field is normal while typing.
        if let Some(s) = p.str("title")? {
            t.title = s.to_owned();
        }
        if let Some(c) = p.named::<TileColor>("color", "red, orange, yellow, lime, green, mint, teal, cyan, blue, indigo, purple, pink, rose, crimson, brown, gray")? {
            t.color = c;
        }
        if let Some(m) = p.named::<PlayMode>("mode", "toggle, restart, hold, pause")? {
            t.mode = m;
        }
        if let Some(v) = unit("volume", p.f64("volume")?)? {
            t.volume = v;
        }
        if let Some(v) = secs("fadeIn", p.f64("fadeIn")?)? {
            t.fade_in = v;
        }
        if let Some(v) = secs("fadeOut", p.f64("fadeOut")?)? {
            t.fade_out = v;
        }
        if let Some(b) = p.bool("loop")? {
            t.looping = b;
        }
        if let Some(b) = p.bool("solo")? {
            t.solo = b;
        }
        if let Some(s) = p.f64("trimStart")? {
            t.start = s.max(0.0);
        }
        if p.has("trimEnd") {
            t.end = p.f64("trimEnd")?;
        }
        if let Some(end) = t.end {
            if end <= t.start {
                return Err(bad("tile.set: trimEnd must be after trimStart"));
            }
        }
        if let Some(l) = p.named::<ImageLayer>("pictureLayer", "underlay, overlay")? {
            t.image_layer = l;
        }
        if let Some(f) = p.named::<ImageFit>("pictureFit", "fill, fit")? {
            t.image_fit = f;
        }
        if let Some(v) = unit("pictureOpacity", p.f64("pictureOpacity")?)? {
            t.image_opacity = v;
        }
        let hotkey = if p.has("hotkey") {
            match p.str("hotkey")? {
                None | Some("") => Some(None),
                Some(k) => {
                    let c = k.chars().next().filter(|_| k.chars().count() == 1).and_then(normalize_key);
                    Some(Some(c.ok_or_else(|| bad("tile.set: hotkey must be one of 1–0, a–z, ; , . /"))?))
                }
            }
        } else {
            None
        };
        let known = [
            "tile", "title", "color", "mode", "volume", "fadeIn", "fadeOut", "loop", "solo", "trimStart", "trimEnd",
            "pictureLayer", "pictureFit", "pictureOpacity", "hotkey",
        ];
        if let Some(k) = p.m.keys().find(|k| !known.contains(&k.as_str())) {
            return Err(bad(format!("tile.set: unknown setting {k:?}")));
        }
        self.engine.update_params(&t);
        let (si, _) = self.store.library.locate(id).expect("resolved");
        if let Some(slot) = self.store.library.tile_mut(id) {
            *slot = t;
        }
        if let Some(key) = hotkey {
            set_hotkey(&mut self.store.library.sets[si].tiles, id, key);
        }
        self.store.mark_dirty();
        Ok(self.tile_json(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wav;

    fn controller() -> (Controller, PathBuf) {
        let root = std::env::temp_dir().join(format!("sb-ctl-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let store = Store::open(root.join("data")).unwrap();
        (Controller::new(store, Engine::disabled("test")), root)
    }

    fn wait_loaded(c: &mut Controller) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while c.store.is_loading() && Instant::now() < deadline {
            c.store.poll();
            std::thread::sleep(Duration::from_millis(10));
        }
        c.tick(0.0);
    }

    fn add(c: &mut Controller, root: &std::path::Path, names: &[&str]) -> Vec<Uuid> {
        let paths: Vec<String> = names
            .iter()
            .map(|n| {
                let p = root.join(format!("{n}.wav"));
                wav::write_tone(&p, 8000, 0.5, &[330.0], 0.4).unwrap();
                p.to_string_lossy().into_owned()
            })
            .collect();
        let r = c.execute("tile.add", json!({"paths": paths})).unwrap();
        wait_loaded(c);
        serde_json::from_value(r["added"].clone()).unwrap()
    }

    /// Every registered command has an implementation: run each with empty parameters;
    /// anything but "unknown_command" or a panic counts.
    #[test]
    fn every_registered_command_is_implemented() {
        for id in COMMANDS.iter().map(|s| s.id) {
            // Skip the ones with side effects outside the test (opening a folder).
            if id == "library.reveal" {
                continue;
            }
            let (mut c, root) = controller();
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| c.execute(id, json!({}))));
            match r {
                Ok(Err(e)) => assert_ne!(e.code, "unknown_command", "{id}"),
                Ok(Ok(_)) => {}
                Err(_) => panic!("{id} panicked with empty parameters"),
            }
            std::fs::remove_dir_all(root).ok();
        }
    }

    #[test]
    fn resolves_tiles_by_every_kind_of_ref() {
        let (mut c, root) = controller();
        let ids = add(&mut c, &root, &["Rimshot", "Air Horn", "Applause"]);
        c.execute("set.create", json!({"name": "Game Night"})).unwrap();
        let other = add(&mut c, &root, &["Applause"]);
        c.execute("set.select", json!({"set": "#1"})).unwrap();

        assert_eq!(c.resolve_tile(&ids[1].to_string()).unwrap(), ids[1]);
        assert_eq!(c.resolve_tile("rimshot").unwrap(), ids[0]);
        assert_eq!(c.resolve_tile("#3").unwrap(), ids[2]);
        assert_eq!(c.resolve_tile("key:2").unwrap(), ids[1]);
        assert_eq!(c.resolve_tile("Game Night/Applause").unwrap(), other[0]);
        // The current set wins over the others.
        assert_eq!(c.resolve_tile("Applause").unwrap(), ids[2]);
        assert_eq!(c.resolve_tile("#9").unwrap_err().code, "not_found");
        assert_eq!(c.resolve_tile("Nobody").unwrap_err().code, "not_found");

        // Two of the same name in one set is ambiguous, with both as candidates.
        c.execute("tile.duplicate", json!({"tile": "Air Horn"})).unwrap();
        let e = c.resolve_tile("Air Horn").unwrap_err();
        assert_eq!(e.code, "ambiguous");
        assert_eq!(e.candidates.len(), 2);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn plays_stops_and_follows_the_cue_list() {
        let (mut c, root) = controller();
        add(&mut c, &root, &["One", "Two"]);
        let r = c.execute("tile.trigger", json!({"tile": "One"})).unwrap();
        assert_eq!(r["state"], "playing");
        assert_eq!(c.execute("status", json!({})).unwrap()["playing"][0]["title"], "One");
        c.execute("tile.trigger", json!({"tile": "One"})).unwrap(); // Play/Stop toggles off
        assert_ne!(c.engine.state(c.resolve_tile("One").unwrap()), PlayState::Playing);

        c.execute("cue.top", json!({})).unwrap();
        assert_eq!(c.execute("cue.go", json!({})).unwrap()["title"], "One");
        assert_eq!(c.execute("cue.go", json!({})).unwrap()["title"], "Two");
        assert_eq!(c.execute("cue.go", json!({})).unwrap_err().code, "end_of_cues");
        c.execute("transport.stopAll", json!({"immediate": true})).unwrap();
        assert!(c.effects.contains(&Effect::ReleaseHeld));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn tile_set_validates_and_moves_hotkeys() {
        let (mut c, root) = controller();
        add(&mut c, &root, &["A", "B"]);
        let r = c
            .execute("tile.set", json!({"tile": "A", "title": "Ding", "color": "Gold", "volume": 0.5}))
            .unwrap_err();
        assert_eq!(r.code, "bad_params");
        assert!(r.message.contains("color"), "{}", r.message);
        let r = c
            .execute("tile.set", json!({"tile": "A", "title": "Ding", "color": "teal", "mode": "hold", "loop": true, "hotkey": "Q"}))
            .unwrap();
        assert_eq!((r["title"].as_str(), r["color"].as_str(), r["mode"].as_str(), r["key"].as_str()), (Some("Ding"), Some("teal"), Some("hold"), Some("q")));
        // Taking B's key moves it.
        c.execute("tile.set", json!({"tile": "B", "hotkey": "q"})).unwrap();
        assert_eq!(c.execute("tile.get", json!({"tile": "Ding"})).unwrap()["key"], Value::Null);
        assert_eq!(c.execute("tile.set", json!({"tile": "B", "trimStart": 0.3, "trimEnd": 0.2})).unwrap_err().code, "bad_params");
        assert_eq!(c.execute("tile.set", json!({"tile": "B", "colour": "red"})).unwrap_err().code, "bad_params");
        // Typing in the inspector: spaces and an empty field survive each keystroke.
        let b = c.resolve_tile("B").unwrap().to_string();
        for typed in ["Rim", "", "Rimshot"] {
            assert_eq!(c.execute("tile.set", json!({"tile": b, "title": typed})).unwrap()["title"], typed);
        }
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn sets_and_view_commands() {
        let (mut c, root) = controller();
        assert_eq!(c.execute("set.delete", json!({})).unwrap_err().code, "last_set");
        c.execute("set.create", json!({})).unwrap();
        assert!(matches!(c.effects.last(), Some(Effect::StartRename(_))));
        c.execute("set.rename", json!({"name": "Encore"})).unwrap();
        c.execute("set.columns", json!({"columns": 7})).unwrap();
        c.execute("set.layout", json!({"layout": "list"})).unwrap();
        let sets = c.execute("set.list", json!({})).unwrap();
        assert_eq!(sets[1]["name"], "Encore");
        assert_eq!((sets[1]["columns"].as_u64(), sets[1]["layout"].as_str(), sets[1]["current"].as_bool()), (Some(7), Some("list"), Some(true)));
        assert_eq!(c.execute("set.columns", json!({"columns": 13})).unwrap_err().code, "bad_params");

        c.execute("view.theme", json!({"theme": "padd"})).unwrap();
        assert_eq!(c.store.library.theme, ThemeKind::Panel);
        assert!(c.effects.contains(&Effect::ApplyLook));
        c.execute("view.toggleEdit", json!({})).unwrap();
        assert!(c.edit_mode);
        c.execute("master.volume", json!({"db": -6})).unwrap();
        assert!((gain_to_db(c.store.library.master_volume) + 6.0).abs() < 0.01);
        c.execute("set.delete", json!({"set": "Encore"})).unwrap();
        assert_eq!(c.store.library.sets.len(), 1);
        assert_eq!(c.execute("nope", json!({})).unwrap_err().code, "unknown_command");
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn file_commands_without_paths_ask_the_window() {
        let (mut c, root) = controller();
        let ids = add(&mut c, &root, &["A"]);
        c.effects.clear();
        c.execute("tile.add", json!({})).unwrap();
        c.execute("tile.picture", json!({"tile": "A"})).unwrap();
        c.execute("tile.replaceAudio", json!({"tile": "A"})).unwrap();
        let set = c.current_set().id;
        assert_eq!(c.effects, [Effect::PickAudio { set, at: None }, Effect::PickPicture(ids[0]), Effect::PickReplacement(ids[0])]);
        std::fs::remove_dir_all(root).ok();
    }
}
