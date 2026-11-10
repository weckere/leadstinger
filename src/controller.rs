//! The Controller: everything the app does, as commands (docs/CLI.md).
//!
//! It owns the library ([`Store`]), the audio [`Engine`], the cue pointers and the
//! view state the commands change (the selected tile, the inspector's tab, open windows).
//! [`Controller::execute`] runs any command in [`crate::commands::COMMANDS`] by id,
//! for the GUI's widgets, menus and shortcuts today and for the CLI (`leadstinger-cli`) from M7.
//!
//! What only the window can do (a file picker, editing a name in place, switching the
//! theme, a screenshot, quitting) comes back as an [`Effect`] for the GUI to carry out.

use crate::audio::{Engine, VoiceStatus};
use crate::bindings::{Binding, Fire, MidiEvent, MidiKind, MidiSource, Source};
use crate::hotkeys::HotkeyStatus;
use crate::input::Input;
use crate::midi::MidiStatus;
use crate::osc::{OscMessage, OscStatus};
use crate::commands::spec;
#[cfg(test)]
use crate::commands::COMMANDS;
use crate::logic::{
    db_to_gain, gain_to_db, next_cue, play_range, resolve_press, set_hotkey, set_hotkey_auto, PadKey,
    tile_for_key, matches_query, next_armed, pointer_after_next, pointer_before_next, Ballistics, PlayState, TriggerAction, TriggerEvent,
};
use crate::level;
use crate::model::{zoom_step, ZOOM_MAX, ZOOM_MIN, ActionKind, CueLink, Target as ActionTarget, WAIT_MAX, TileAction, REPLAY_MAX, SPEED_MAX, SPEED_MIN, Appearance, Duck, FadeCurve, Marker, ImageFit, ImageLayer, Layout, PlayMode, PressOptions, SoundSet, ThemeKind, Tile, TileColor, GAIN_DB_MAX};
use crate::model::{tidy_channel_mix, tidy_sends, ChannelRoute, ChannelTo, GridMode, TileSend, DUCK_DB_MIN, EMOJI_MAX_CHARS, MAIN_OUTPUT};
use crate::rows::Place;
use crate::picture::is_image_path;
use crate::store::{Media, Store};
use serde::de::DeserializeOwned;
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use uuid::Uuid;

mod export;
mod themes;

/// The parameter a fader fills when it's learned for `cmd`.
fn fader_param(cmd: &str) -> Option<String> {
    matches!(cmd, "master.volume" | "tile.set").then(|| "volume".to_owned())
}

/// Where `view.quip` sends the QuipClipper browser; `None` leaves that part as it is.
#[derive(Debug, Clone, PartialEq)]
pub struct QuipGoto {
    pub server: Option<String>,
    pub folder: Option<String>,
    pub query: Option<String>,
    /// The new clips to list (`new: true`), already counted as seen.
    pub new: Option<std::sync::Arc<crate::quip::clips::Listing>>,
}

/// Where `view.editorZoom` asked the waveform editor to go (D-104); the window takes it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EditorZoom {
    /// The whole file.
    Fit,
    /// From one time to another, in seconds into the file.
    Span(f64, f64),
    /// In by this factor (out below 1), around the cursor or the middle.
    By(f64),
}

/// The tile inspector's tabs (D-144): what the rest of its settings are grouped into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InspectorTab {
    /// Fades, the stop fade, the fade shape and ducking.
    Play,
    /// Gain and Normalise, speed, pitch, the trim, outputs and channels.
    Sound,
    /// Notes, the pre-wait, what starts the next cue, the vamp and markers.
    Cue,
    /// The hotkey, MIDI pads and keys, and the messages it sends.
    Control,
}

impl InspectorTab {
    pub const ALL: [InspectorTab; 4] = [InspectorTab::Play, InspectorTab::Sound, InspectorTab::Cue, InspectorTab::Control];

    pub fn name(self) -> &'static str {
        match self {
            InspectorTab::Play => "play",
            InspectorTab::Sound => "sound",
            InspectorTab::Cue => "cue",
            InspectorTab::Control => "control",
        }
    }

    pub fn named(name: &str) -> Option<InspectorTab> {
        InspectorTab::ALL.into_iter().find(|t| t.name().eq_ignore_ascii_case(name))
    }

    pub fn label(self) -> &'static str {
        match self {
            InspectorTab::Play => "Play",
            InspectorTab::Sound => "Sound",
            InspectorTab::Cue => "Cue",
            InspectorTab::Control => "Control",
        }
    }
}

/// Set settings changed without an undo step; with every undoable command, what a locked
/// set refuses (M22). Grid or List (`set.layout`) isn't among them: it's how a set is
/// run, not what's in it (D-149).
const SET_SETTINGS: &[&str] = &["set.columns", "set.rows", "set.autoAdvance", "set.listMode", "set.crossfade"];

/// What a pad key can be, for errors.
const KEY_HELP: &str = "a hotkey is a key on the keyboard's four rows (1–=, q–], a–', z–/), or one with alt+ (Option on macOS), e.g. alt+1";

/// Undoable commands a locked set lets through: the lock itself, new sets and copies,
/// and finishing a recording that had already started.
const LOCK_FREE: &[&str] = &["set.lock", "set.unlock", "set.create", "set.duplicate", "set.import", "record.stop"];

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
    PickAudio { set: Uuid, place: Place },
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
    /// `view.screenshot`: the main window, or a set's window (`window`).
    Screenshot { path: PathBuf, window: Option<Uuid> },
    /// `window.focus`, `window.move`, `window.gather`: the main window (`None`) or a
    /// set's window.
    Window { window: Option<Uuid>, op: WindowOp },
    ClearClip,
    /// Playback was stopped wholesale or the set changed: forget held keys and buttons.
    ReleaseHeld,
    /// `view.search` opened the search box: give it the keyboard.
    FocusSearch,
    /// `tile.render`/`set.render` without a path: the Export Audio window, for these
    /// tiles (none: the set).
    ExportAudio { tiles: Vec<Uuid>, set: Uuid },
    /// `quip.import` from an opened link: bring the window to the front.
    Raise,
    /// An opened QuipClipper link (or pair link) whose server isn't connected, or any one
    /// with settings.quipAskEachImport: ask before connecting or importing (D-132).
    ConfirmQuip(Box<crate::quip::Ask>),
    /// `theme.import` without a path: choose a theme.
    PickTheme,
    /// `theme.export` without a path: ask where to save this theme (its id).
    SaveTheme(String),
    /// `set.import` with `ask` (a set file opened from the file manager): ask before
    /// importing it.
    ConfirmSetFile(PathBuf),
    /// `set.import` installed the set file's theme: ask whether to use it now (its id and
    /// name).
    ConfirmTheme { id: String, name: String },
    /// Scroll the board to this tile (a recording started in it).
    Reveal(Uuid),
    Quit,
}

/// What a window command asks of a window (D-224).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WindowOp {
    Focus,
    /// Its top-left corner and inside size, in the desktop's logical points.
    Place { pos: Option<[f32; 2]>, size: Option<[f32; 2]> },
    /// Every set window beside the main window, one below another.
    Gather,
}

/// A window as the app last drew it, for `window.list`: the main window, or a set's.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WindowView {
    /// `None`: the main window.
    pub set: Option<Uuid>,
    pub title: String,
    /// Its top-left corner in the desktop's logical points; `None` where the compositor
    /// keeps positions to itself (Wayland).
    pub pos: Option<[f32; 2]>,
    pub size: Option<[f32; 2]>,
    pub focused: bool,
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
        matches!(self.code, "loading" | "no_audio" | "end_of_cues" | "nothing_to_paste" | "disabled" | "end_of_sets")
    }
}

impl std::fmt::Display for CmdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

type Res = Result<Value, CmdError>;

/// A pad palette of one's own, as JSON (src/pads.rs).
fn palette_table(js: &str) -> Result<crate::pads::Palette, CmdError> {
    let mut v: Value = serde_json::from_str(js).map_err(|e| bad(format!("midi.feedback: colors: {e}")))?;
    if v.get("name").is_none() {
        v["name"] = json!("custom");
    }
    let p: crate::pads::Palette = serde_json::from_value(v).map_err(|e| bad(format!("midi.feedback: colors: {e}")))?;
    p.validate().map_err(|e| bad(format!("midi.feedback: colors: {e}")))?;
    Ok(p)
}

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

/// A List set's playback mode as the CLI names it (`auto_advance` on is Continuous).
fn list_mode_name(continuous: bool) -> &'static str {
    if continuous { "continuous" } else { "queued" }
}

/// Fade All's fade, in seconds.
const FADE_ALL: f64 = 2.0;

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
    /// Tiles that have played since their List set was last reset (`cue.reset`): their
    /// armed marker is cleared and advancing passes over them. Not saved, like `cue`.
    pub played: HashSet<Uuid>,
    /// In a List set the selection is the next cue: the set, pointer and selected tile
    /// as last matched by [`Controller::sync_list_selection`].
    list_sync: Option<(Uuid, usize, Option<Uuid>)>,
    /// Per List set: the cue it's playing, the one it last started (D-197). When it ends
    /// or stops, Continuous starts the next armed cue and Queued selects it.
    list_current: HashMap<Uuid, Uuid>,
    /// The tile most recently started, shown in the counter.
    pub last_started: Option<Uuid>,
    /// Voices this tick, refreshed by [`Controller::tick`].
    pub statuses: HashMap<Uuid, VoiceStatus>,
    /// Each sounding tile's meter (t-m19-tile-meters), fed by [`Controller::tick`].
    pub levels: HashMap<Uuid, Ballistics>,
    /// The tab the tile inspector shows (`view.inspector {tab}`, D-144).
    pub inspector_tab: InspectorTab,
    pub selected_tile: Option<Uuid>,
    /// Every selected tile when several are (⌘/Shift-click), `selected_tile` among them;
    /// ignored once `selected_tile` changes elsewhere. Read it through
    /// [`Controller::selected`].
    pub selection: Vec<Uuid>,
    /// The search box's text while it's open (`view.search`); the board shows its hits.
    pub search: Option<String>,
    pub show_settings: bool,
    /// The QuipClipper browser is open (`view.quip`).
    pub show_quip: bool,
    /// Where `view.quip` asked the browser to go: server, folder, search words.
    pub quip_goto: Option<QuipGoto>,
    /// QuipClipper previews being fetched and set files downloading (src/quip/clips.rs).
    pub quip_jobs: crate::quip::clips::Jobs,
    /// New clips on the connected servers, asked for now and then (src/quip/watch.rs).
    pub quip_watch: crate::quip::watch::Watch,
    pub show_about: bool,
    pub show_shortcuts: bool,
    /// Check for Updates (src/update.rs): the check running off the thread, its answer.
    pub updates: crate::update::Checker,
    /// The tile open in the full-window editor (`view.editor`), which takes the board's
    /// place while it's open.
    pub editor: Option<Uuid>,
    /// The editor's selection, in seconds into the file, start first (`view.editorSelect`).
    pub editor_selection: Option<(f64, f64)>,
    /// A zoom `view.editorZoom` asked for, until the editor takes it.
    pub editor_zoom: Option<EditorZoom>,
    /// Output devices, as last listed.
    pub devices: Vec<String>,
    /// Input devices, as last listed: by `input.list`, or the first time something needs
    /// the input or Settings shows the Voice section (`list_inputs_once`).
    pub input_devices: Vec<String>,
    inputs_listed: bool,
    /// The system default input's name, as last listed.
    pub input_default: Option<String>,
    /// Lists the inputs and names the default: `Input::devices`, or a stand-in in tests.
    pub input_lister: fn() -> (Vec<String>, Option<String>),
    /// Whether the input hears the board, while voice ducking listens (D-127).
    pub hears_board: crate::input::HearsBoard,
    /// The status bar has said the input hears the board (once a run).
    hearing_noted: bool,
    /// The stand-in input of a recording staged for a screenshot (`stage_recording`).
    staged_input: Option<String>,
    pub notice: Option<Notice>,
    /// The latest thing a screen reader should hear without moving focus (D-093); the
    /// window puts it in a live region.
    pub announcement: Option<Announcement>,
    /// What the announcements last covered: which tiles were playing, the next cue,
    /// the imports under way.
    heard: Heard,
    /// The system voice, started the first time a cue name is spoken.
    speaker: Option<crate::speech::Speaker>,
    /// Work for the window, drained by the GUI each frame.
    pub effects: Vec<Effect>,
    /// The window runs on X11/XWayland (Linux): which clipboard tool to ask first.
    pub x11: bool,
    /// False for the offline CLI and `leadstinger-cli serve`: commands that would open a file
    /// picker, edit a name in place or take a screenshot then fail with `no_window`.
    pub has_window: bool,
    /// The windows as the app last drew them, main first (`window.list`).
    pub windows: Vec<WindowView>,
    last_reopen: Instant,
    last_tick: Instant,
    /// `events.subscribe` listeners (the control socket's `watch`).
    subscribers: Vec<std::sync::mpsc::Sender<Value>>,
    /// What the last tick saw, to turn into events.
    seen: Seen,
    pub history: History,
    /// What the MIDI input thread last saw (`midi.ports`).
    pub midi_status: MidiStatus,
    /// `bind.learn` waiting for the next pad or key.
    pub learning: Option<Learn>,
    /// What the window last registered as system-wide keys (`hotkey.status`).
    pub hotkey_status: HotkeyStatus,
    /// What the OSC listener last did (`osc.status`).
    pub osc_status: OscStatus,
    /// Sends state to the OSC feedback address, while one is set.
    osc_feedback: Option<crate::osc::Feedback>,
    /// The sounding tiles' levels as OSC feedback sends them (0-1 on the meter's
    /// curve), sampled every [`crate::osc::LEVEL_EVERY`] seconds of ticks.
    osc_levels: HashMap<Uuid, f32>,
    /// Seconds of ticks since `osc_levels` was sampled.
    osc_level_clock: f64,
    /// The microphone, open while voice ducking or a recording needs it.
    pub input: Option<Input>,
    /// A recording in progress (`record.start`).
    pub recording: Option<Recording>,
    capture: Option<Capture>,
    /// The last `replay_seconds` of input, interleaved stereo at `replay_rate`.
    replay: std::collections::VecDeque<f32>,
    replay_rate: u32,
    /// Why the input couldn't open, and when it was last tried.
    pub input_error: Option<String>,
    input_tried: Option<Instant>,
    /// Opens the input: `Input::open`, or a stand-in in tests.
    pub input_opener: crate::input::Opener,
    /// The input being opened on a thread of its own, never under the controller (D-101):
    /// macOS holds the open while it asks for the microphone, and every command would
    /// wait for that.
    input_opening: Option<Opening>,
    /// The last on/off state of each MIDI controller, so a CC button fires once per push.
    cc_state: HashMap<(String, u8, u8), bool>,
    /// Pad lights go out here (the MIDI thread's output), and what each pad was last sent.
    pub midi_out: Option<std::sync::mpsc::Sender<Vec<u8>>>,
    /// Each pad (channel, note) and what it was last sent: (channel, velocity).
    leds: HashMap<(u8, u8), (u8, u8)>,
    leds_sent: Option<Instant>,
    /// Tiles' start and stop messages (src/messages.rs).
    pub messages: crate::messages::Tracker,
    /// The page auto-mapped pads show (src/pads.rs).
    pub pad_paging: crate::pads::Paging,
    /// `tile.random`: per pool (set and colour), the tiles not played since the pool last
    /// filled, and the one played last (so a refill doesn't start with it).
    random: HashMap<String, (Vec<Uuid>, Option<Uuid>)>,
    /// Starts waiting for their time, oldest first (group offsets, pre-waits, cue
    /// links); Stop All drops them.
    pub pending: Vec<(Instant, Wait)>,
    /// How deep zero-wait cue links are starting cues right now.
    link_depth: u8,
    /// Until when a faded Stop All is still fading: another Stop All before then stops
    /// at once (Esc's second press). Starting a tile ends it.
    stop_all_fading: Option<Instant>,
    /// How deep groups are firing groups, so one that contains itself stops.
    action_depth: u8,
    /// Rendered exports under way or done (`tile.render`, `set.render`), oldest first.
    pub renders: Vec<crate::render::Job>,
    next_render: u32,
    /// The theme in use (D-105, D-106): a built-in or one from the data folder's themes.
    /// The window restyles from it on [`Effect::ApplyLook`].
    pub look: std::sync::Arc<crate::theme::Look>,
    /// What reading the theme in use warned about (D-107).
    pub look_warnings: Vec<String>,
    /// Hot reload of a user theme in use, and the installed themes for Settings.
    theme_state: themes::State,
}

/// A start the controller makes later, from `tick`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Wait {
    /// A group's target, at its offset.
    Target(Uuid),
    /// A tile at the end of its pre-wait.
    PreWait(Uuid),
    /// The cue after this tile in its set: its continue or follow link.
    Next(Uuid),
}

/// A binding waiting for its source (`bind.learn`).
#[derive(Clone, Debug)]
pub struct Learn {
    /// A key combination (the window catches it) rather than a MIDI message.
    pub key: bool,
    pub command: String,
    pub params: Map<String, Value>,
    pub value: Option<String>,
    pub until: Instant,
}

/// The longest recording; it stops by itself there.
const RECORD_MAX: Duration = Duration::from_secs(10 * 60);

/// A recording in progress: the input's samples collect here as interleaved stereo.
/// What the status bar, Settings and `status` say while the input is opening.
pub const INPUT_WAITING: &str =
    if cfg!(target_os = "macos") { "Waiting for microphone permission…" } else { "Opening the input…" };

/// An input opening on its own thread (`Controller::keep_input`).
struct Opening {
    /// The input setting it's being opened for.
    wanted: Option<String>,
    rx: std::sync::mpsc::Receiver<anyhow::Result<Input>>,
    since: Instant,
    /// The status bar has said it's waiting.
    noted: bool,
}

/// What Settings and the status bar say when the input hears the board (D-127).
pub const HEARS_BOARD: &str = "Your microphone hears the board: use headphones or a close mic, or raise the threshold";

/// A recording in progress, for the window: its tile, the seconds so far, and the input
/// (`None` while it's still opening) with its level in dBFS.
pub struct RecordingView {
    pub tile: Uuid,
    pub title: String,
    pub seconds: f64,
    pub input: Option<String>,
    pub level_db: Option<f32>,
}

pub struct Recording {
    pub tile: Uuid,
    /// The tile was made for this recording (removed again if nothing is recorded).
    made: bool,
    data: Vec<f32>,
    rate: u32,
    pub started: Instant,
}

impl Recording {
    /// Seconds recorded so far.
    pub fn seconds(&self) -> f64 {
        self.data.len() as f64 / 2.0 / self.rate.max(1) as f64
    }
}

/// The input's tap, while a recording or the replay buffer wants its samples.
struct Capture {
    rx: std::sync::mpsc::Receiver<Vec<f32>>,
    rate: u32,
    channels: usize,
}

/// Interleaved samples, `channels` wide, as stereo (mono doubled; extra channels dropped).
fn to_stereo(block: &[f32], channels: usize) -> Vec<f32> {
    match channels {
        1 => block.iter().flat_map(|s| [*s, *s]).collect(),
        2 => block.to_vec(),
        n => block.chunks_exact(n.max(1)).flat_map(|f| [f[0], f[1]]).collect(),
    }
}

/// A marker by name (any case) or 1-based position.
fn find_marker(t: &Tile, r: &str) -> Result<Marker, CmdError> {
    let by_pos = r.trim_start_matches('#').parse::<usize>().ok().and_then(|n| n.checked_sub(1)).and_then(|i| t.markers.get(i));
    by_pos
        .or_else(|| t.markers.iter().find(|m| m.name.eq_ignore_ascii_case(r)))
        .cloned()
        .ok_or_else(|| CmdError::new("not_found", format!("{} has no marker {r:?}", t.title)))
}

/// Triggers, bindings and GO don't start a disabled tile.
fn refuse_disabled(t: &Tile) -> Result<(), CmdError> {
    if t.disabled {
        return Err(CmdError::new("disabled", format!("“{}” is disabled", t.title)));
    }
    Ok(())
}

/// What a binding runs: command, parameters and the fader's parameter.
type Target = (String, Map<String, Value>, Option<String>);

/// How long `bind.learn` waits.
const LEARN_FOR: Duration = Duration::from_secs(30);

/// How many undo steps are kept.
const UNDO_LIMIT: usize = 50;
/// Repeated `tile.set`s on one tile closer together than this are one undo step (a
/// slider drag, typing a title).
const COALESCE: Duration = Duration::from_millis(1500);

/// The library's sets as they were before a change.
pub struct Step {
    pub label: &'static str,
    sets: Vec<SoundSet>,
    selected: Option<Uuid>,
}

/// Undo and redo: snapshots of the sets, taken around every command marked `undo` in
/// the registry. Settings, the master volume, the view and playback aren't recorded.
#[derive(Default)]
pub struct History {
    undo: Vec<Step>,
    redo: Vec<Step>,
    /// The last recorded `tile.set` target, and when it last changed.
    last: Option<(String, Instant)>,
}

impl History {
    pub fn undo_label(&self) -> Option<&'static str> {
        self.undo.last().map(|s| s.label)
    }

    pub fn redo_label(&self) -> Option<&'static str> {
        self.redo.last().map(|s| s.label)
    }

    /// Every media file a step refers to, so the store keeps them.
    fn media(&self) -> std::collections::HashSet<String> {
        self.undo
            .iter()
            .chain(&self.redo)
            .flat_map(|s| s.sets.iter().flat_map(|set| &set.tiles))
            .flat_map(|t| t.file.iter().chain(&t.image).cloned())
            .collect()
    }
}

/// A message for screen readers: `seq` counts them, so the same words said twice are
/// still two announcements.
#[derive(Clone, Debug, PartialEq)]
pub struct Announcement {
    pub text: String,
    pub seq: u64,
}

/// The state announcements are derived from (see [`Controller::announce_changes`]).
#[derive(Default)]
struct Heard {
    playing: HashSet<Uuid>,
    next_cue: Option<(Uuid, usize)>,
}

/// The state events are derived from.
#[derive(Default)]
struct Seen {
    states: HashMap<Uuid, PlayState>,
    revision: u64,
    next_cue: Option<(Uuid, usize)>,
    master: f32,
    set: Option<Uuid>,
}

/// Where tile.add and tile.move put a tile (D-116, D-143): {row, slot} into that slot of
/// the row (a blank one fills; otherwise the slots from there move along), {row, at?}
/// into the gap before slot `at` (default the row's end), {row, newRow: true} as a new
/// row there (rows + 1 for the end), or the flat place `flat` (tile.add's `at`,
/// tile.move's `to`), joining the row of the tile it displaces. All 1-based. `None`
/// when none of them is given.
fn place_param(p: &P<'_>, flat: &str) -> Result<Option<Place>, CmdError> {
    if let Some(row) = p.f64("row")? {
        let row = row.max(1.0) as usize - 1;
        if p.bool("newRow")?.unwrap_or(false) {
            return Ok(Some(Place::NewRow(row)));
        }
        if let Some(slot) = p.f64("slot")? {
            return Ok(Some(Place::Slot { row, at: slot.max(1.0) as usize - 1 }));
        }
        let at = p.f64("at")?.map_or(usize::MAX, |a| a.max(1.0) as usize - 1);
        return Ok(Some(Place::InRow { row, at }));
    }
    Ok(p.f64(flat)?.map(|a| Place::Flat(a.max(1.0) as usize - 1)))
}

impl Controller {
    pub fn new(store: Store, engine: Engine) -> Controller {
        engine.set_master(store.library.master_volume);
        engine.set_duck(store.library.duck_db, store.library.duck_attack, store.library.duck_release);
        let mut engine = engine;
        if !store.library.outputs.is_empty() || store.library.preview_output {
            Controller::configure_engine(&mut engine, &store.library);
        }
        let notice = store.load_warning.clone().map(|text| Notice { text, error: true, at: Instant::now() }).or_else(|| {
            store.migrated.clone().filter(|t| !t.is_empty()).map(|t| Notice { text: format!("Library updated: {t}."), error: false, at: Instant::now() })
        });
        let mut c = Controller {
            store,
            engine,
            cue: HashMap::new(),
            played: HashSet::new(),
            list_sync: None,
            list_current: HashMap::new(),
            last_started: None,
            statuses: HashMap::new(),
            levels: HashMap::new(),
            inspector_tab: InspectorTab::Play,
            selected_tile: None,
            selection: Vec::new(),
            search: None,
            show_settings: false,
            show_quip: false,
            quip_goto: None,
            quip_jobs: Default::default(),
            quip_watch: Default::default(),
            show_about: false,
            show_shortcuts: false,
            updates: Default::default(),
            editor: None,
            editor_selection: None,
            editor_zoom: None,
            devices: Vec::new(),
            notice,
            announcement: None,
            heard: Heard::default(),
            speaker: None,
            effects: Vec::new(),
            x11: false,
            has_window: true,
            windows: Vec::new(),
            last_reopen: Instant::now(),
            last_tick: Instant::now(),
            subscribers: Vec::new(),
            seen: Seen::default(),
            history: History::default(),
            midi_status: MidiStatus::default(),
            learning: None,
            osc_status: OscStatus::default(),
            osc_feedback: None,
            osc_levels: HashMap::new(),
            osc_level_clock: 0.0,
            input: None,
            recording: None,
            capture: None,
            replay: std::collections::VecDeque::new(),
            replay_rate: 48_000,
            input_devices: Vec::new(),
            inputs_listed: false,
            input_default: None,
            input_lister: Input::devices,
            hears_board: Default::default(),
            hearing_noted: false,
            staged_input: None,
            input_error: None,
            input_tried: None,
            input_opener: std::sync::Arc::new(|device: Option<&str>, voice| Input::open(device, voice)),
            input_opening: None,
            hotkey_status: HotkeyStatus { note: Some("System-wide keys need the app's window".into()), ..Default::default() },
            cc_state: HashMap::new(),
            midi_out: None,
            leds: HashMap::new(),
            leds_sent: None,
            messages: Default::default(),
            pad_paging: Default::default(),
            random: HashMap::new(),
            pending: Vec::new(),
            link_depth: 0,
            stop_all_fading: None,
            action_depth: 0,
            renders: Vec::new(),
            next_render: 1,
            look: std::sync::Arc::new(crate::theme::Look::console()),
            look_warnings: Vec::new(),
            theme_state: Default::default(),
        };
        c.restore_theme();
        c
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

    /// Show `text` in the status bar; screen readers hear it too.
    pub fn notify(&mut self, text: impl Into<String>) {
        let text = text.into();
        self.announce(text.clone());
        self.notice = Some(Notice { text, error: false, at: Instant::now() });
    }

    pub fn notify_error(&mut self, text: impl Into<String>) {
        let text = text.into();
        self.announce(text.clone());
        self.notice = Some(Notice { text, error: true, at: Instant::now() });
    }

    /// Tell screen readers `text` without moving their focus (the window's live region).
    pub fn announce(&mut self, text: impl Into<String>) {
        let seq = self.announcement.as_ref().map_or(1, |a| a.seq + 1);
        self.announcement = Some(Announcement { text: text.into(), seq });
    }

    /// Announce what changed since the last tick: tiles that started ("Playing …", and
    /// their titles spoken when `speak_cues` is on), a cue list's next cue, imports that
    /// finished or failed. Stop All and the status-bar notices announce themselves.
    fn announce_changes(&mut self) {
        // From the store, not by comparing ticks: an import can start and end between two.
        let imported = self.store.take_imported();
        let lib = &self.store.library;
        let started: Vec<Uuid> = self
            .statuses
            .iter()
            .filter(|(id, s)| s.state == PlayState::Playing && !self.heard.playing.contains(id))
            .map(|(id, _)| *id)
            .collect();
        let titles: Vec<String> = started.iter().filter_map(|id| lib.tile(*id)).map(|t| t.title.clone()).collect();
        let mut parts = Vec::new();
        if !titles.is_empty() {
            parts.push(format!("Playing {}", titles.join(", ")));
        }
        // The next cue, when a cue list's pointer moved (not when the set changed).
        let set = self.current_set();
        let pointer = self.cue.get(&set.id).copied().unwrap_or(0);
        let next = Some((set.id, pointer));
        if set.layout == Layout::List && next != self.heard.next_cue && self.heard.next_cue.is_some_and(|(id, _)| id == set.id) {
            parts.push(match next_cue(&set.tiles, pointer) {
                Some(i) => format!("Next cue {}, {}", i + 1, set.tiles[i].title),
                None => "End of the cue list".to_owned(),
            });
        }
        // Imports: each one that finished, and how.
        let store = &self.store;
        let (mut done, mut failed) = (Vec::new(), Vec::new());
        for name in &imported {
            let Some(tile) = lib.sets.iter().flat_map(|s| &s.tiles).find(|t| t.file.as_deref() == Some(name.as_str())) else {
                continue; // removed meanwhile
            };
            match store.media(tile) {
                Some(crate::store::Media::Failed(e)) => failed.push(format!("Couldn't import {}: {e}", tile.title)),
                Some(_) => done.push(tile.title.clone()),
                None => {}
            }
        }
        match done.as_slice() {
            [] => {}
            [one] => parts.push(format!("Imported {one}")),
            many => parts.push(format!("Imported {} sounds", many.len())),
        }
        parts.extend(failed);
        self.heard.next_cue = next;
        self.heard.playing = self.statuses.iter().filter(|(_, s)| s.state != PlayState::Stopping).map(|(id, _)| *id).collect();
        if self.store.library.speak_cues && !titles.is_empty() {
            self.speak(&titles.join(", "));
        }
        if !parts.is_empty() {
            self.announce(parts.join(". "));
        }
    }

    /// Say `text` with the system voice, on the Preview output's device where the platform
    /// can route it. Never waits: the speech runs on its own thread (src/speech.rs).
    fn speak(&mut self, text: &str) {
        if self.speaker.is_none() {
            self.speaker = crate::speech::Speaker::new();
        }
        let lib = &self.store.library;
        let device = lib.preview_output.then_some(lib.preview_device.as_deref()).flatten();
        if let Some(s) = &self.speaker {
            s.say(text, device);
        }
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
    /// The set a window command names (default: the current one), which must be open in
    /// a window of its own.
    fn open_window(&self, id: &str, r: Option<&str>) -> Result<Uuid, CmdError> {
        let set = &self.store.library.sets[self.resolve_set(r)?];
        if self.store.library.set_windows.iter().any(|w| w.set == set.id) {
            Ok(set.id)
        } else {
            Err(CmdError::new("not_open", format!("{id}: “{}” isn't open in a window of its own (window.open opens it)", set.name)))
        }
    }

    /// `window.*` (D-222, D-224): sets in windows of their own. The library keeps which
    /// are open and where; the app opens, moves and reports the windows.
    fn window_command(&mut self, id: &str, p: &P) -> Res {
        self.needs_window(id, "the app's windows")?;
        let point = |k: &str| -> Result<Option<f32>, CmdError> {
            match p.f64(k)? {
                Some(v) if !v.is_finite() || v.abs() >= 100_000.0 => Err(bad(format!("{id}: {k} must be a number of points"))),
                v => Ok(v.map(|v| v as f32)),
            }
        };
        let (x, y, width, height) = (point("x")?, point("y")?, point("width")?, point("height")?);
        let pos = x.zip(y).map(|(x, y)| [x, y]);
        if x.is_some() != y.is_some() {
            return Err(bad(format!("{id}: give x and y together")));
        }
        let [min_w, min_h] = crate::model::SET_WINDOW_MIN;
        // A size missing one side keeps that side (0 here; the app fills it in).
        let size = (width.is_some() || height.is_some()).then(|| [width.map_or(0.0, |w| w.max(min_w)), height.map_or(0.0, |h| h.max(min_h))]);
        let window_id = |set: Option<Uuid>| set.map_or_else(|| json!("main"), |s| json!(s));
        match id {
            "window.open" => {
                let si = self.set_param(p)?;
                let set = &self.store.library.sets[si];
                let (sid, name) = (set.id, set.name.clone());
                if self.store.library.set_windows.iter().any(|w| w.set == sid) {
                    self.effects.push(Effect::Window { window: Some(sid), op: WindowOp::Focus });
                    if pos.is_some() || size.is_some() {
                        self.effects.push(Effect::Window { window: Some(sid), op: WindowOp::Place { pos, size } });
                    }
                    return Ok(json!({"set": sid, "name": name, "opened": false}));
                }
                self.store.library.set_windows.push(crate::model::SetWindow {
                    set: sid,
                    x,
                    y,
                    width: width.map(|w| w.max(min_w)),
                    height: height.map(|h| h.max(min_h)),
                });
                self.store.mark_dirty();
                Ok(json!({"set": sid, "name": name, "opened": true}))
            }
            "window.close" => {
                let closing: Vec<Uuid> = if p.bool("all")? == Some(true) {
                    self.store.library.set_windows.iter().map(|w| w.set).collect()
                } else {
                    vec![self.open_window(id, p.str("set")?)?]
                };
                let lib = &mut self.store.library;
                lib.set_windows.retain(|w| !closing.contains(&w.set));
                self.windows.retain(|w| w.set.is_none_or(|s| !closing.contains(&s)));
                let names: Vec<String> = closing.iter().filter_map(|s| lib.set_index(*s)).map(|i| lib.sets[i].name.clone()).collect();
                if !closing.is_empty() {
                    self.store.mark_dirty();
                }
                Ok(json!({"closed": names}))
            }
            "window.list" => {
                let lib = &self.store.library;
                let windows: Vec<Value> = self
                    .windows
                    .iter()
                    .map(|w| {
                        let mut o = json!({"window": window_id(w.set), "title": w.title, "focused": w.focused});
                        o["name"] = match w.set.and_then(|s| lib.set_index(s)) {
                            Some(i) => json!(lib.sets[i].name),
                            None => json!(crate::brand::APP_NAME),
                        };
                        if let Some(s) = w.set {
                            o["set"] = json!(s);
                        }
                        if let Some([x, y]) = w.pos {
                            (o["x"], o["y"]) = (json!(x.round()), json!(y.round()));
                        }
                        if let Some([w, h]) = w.size {
                            (o["width"], o["height"]) = (json!(w.round()), json!(h.round()));
                        }
                        o
                    })
                    .collect();
                Ok(json!({"windows": windows}))
            }
            "window.focus" => {
                let window = match p.str("set")? {
                    Some(r) => Some(self.open_window(id, Some(r))?),
                    None => None,
                };
                self.effects.push(Effect::Window { window, op: WindowOp::Focus });
                Ok(json!({"window": window_id(window)}))
            }
            "window.move" => {
                let window = match p.str("set")? {
                    Some(r) => Some(self.open_window(id, Some(r))?),
                    None => None,
                };
                if pos.is_none() && size.is_none() {
                    return Err(bad(format!("{id}: give x and y, width or height")));
                }
                self.effects.push(Effect::Window { window, op: WindowOp::Place { pos, size } });
                Ok(json!({"window": window_id(window)}))
            }
            _ => {
                let moved = self.store.library.set_windows.len();
                self.effects.push(Effect::Window { window: None, op: WindowOp::Gather });
                Ok(json!({"moved": moved}))
            }
        }
    }

    fn needs_window(&self, id: &str, missing: &str) -> Result<(), CmdError> {
        if self.has_window {
            Ok(())
        } else {
            Err(CmdError::new("no_window", format!("{id} needs {missing}: there's no window to ask in")))
        }
    }

    /// Point the engine at the library's outputs: Main's device, the extra outputs and
    /// the preview output.
    fn configure_engine(engine: &mut Engine, lib: &crate::model::Library) {
        let routes: Vec<crate::audio::Route> = lib
            .outputs
            .iter()
            .map(|o| crate::audio::Route { name: o.name.clone(), device: o.device.clone(), channel: o.channel.max(1) - 1 })
            .collect();
        let preview = lib.preview_output.then_some(lib.preview_device.as_deref());
        engine.configure(lib.output_device.as_deref(), &routes, preview);
        engine.set_master(lib.master_volume);
        engine.set_duck(lib.duck_db, lib.duck_attack, lib.duck_release);
    }

    fn outputs_json(&self) -> Value {
        let lib = &self.store.library;
        let routes = self.engine.routes();
        let open = |name: &str| routes.iter().any(|(r, ok)| r.name == name && *ok);
        json!({
            "outputs": lib.outputs.iter().map(|o| json!({"name": o.name, "device": o.device, "channel": o.channel, "open": open(&o.name)})).collect::<Vec<_>>(),
            "errors": self.engine.output_errors.iter().map(|(n, e)| json!({"output": n, "error": e})).collect::<Vec<_>>(),
            "preview": {"on": lib.preview_output, "device": lib.preview_device, "open": open("Preview")},
        })
    }

    fn output_commands(&mut self, id: &str, p: &P) -> Res {
        let channel = |p: &P| -> Result<Option<u16>, CmdError> {
            match p.f64("channel")? {
                Some(c) if (1.0..=64.0).contains(&c) => Ok(Some(c as u16)),
                Some(_) => Err(bad(format!("{id}: channel is the first of the pair, 1-64"))),
                None => Ok(None),
            }
        };
        match id {
            "output.list" => {
                let mut out = self.outputs_json();
                out["devices"] = json!(Engine::output_devices());
                return Ok(out);
            }
            "output.add" => {
                let lib = &self.store.library;
                let name = match p.str("name")? {
                    Some(n) => n.trim().to_owned(),
                    None => (1..).map(|n| format!("Output {n}")).find(|n| !lib.outputs.iter().any(|o| &o.name == n)).expect("free name"),
                };
                if name.is_empty() || name.eq_ignore_ascii_case("main") || name.eq_ignore_ascii_case("preview") || lib.outputs.iter().any(|o| o.name.eq_ignore_ascii_case(&name)) {
                    return Err(bad(format!("output.add: {name:?} is taken (Main and Preview are built in)")));
                }
                let def = crate::model::OutputDef { name, device: p.str("device")?.map(str::to_owned), channel: channel(p)?.unwrap_or(1) };
                self.store.library.outputs.push(def);
            }
            "output.set" => {
                let name = p.need_str("name")?.to_owned();
                let new_name = p.str("rename")?.map(|s| s.trim().to_owned());
                let ch = channel(p)?;
                let lib = &mut self.store.library;
                if let Some(n) = &new_name {
                    if n.is_empty() || n.eq_ignore_ascii_case("main") || n.eq_ignore_ascii_case("preview") || lib.outputs.iter().any(|o| o.name.eq_ignore_ascii_case(n) && o.name != name) {
                        return Err(bad(format!("output.set: {n:?} is taken")));
                    }
                }
                let o = lib.outputs.iter_mut().find(|o| o.name == name).ok_or_else(|| CmdError::new("not_found", format!("no output {name:?}")))?;
                if p.has("device") {
                    o.device = p.str("device")?.map(str::to_owned);
                }
                if let Some(c) = ch {
                    o.channel = c;
                }
                if let Some(n) = new_name {
                    o.name = n.clone();
                    for s in lib.sets.iter_mut().flat_map(|s| s.tiles.iter_mut()).flat_map(|t| t.sends.iter_mut()).filter(|s| s.output == name) {
                        s.output = n.clone();
                    }
                }
            }
            "output.remove" => {
                let name = p.need_str("name")?.to_owned();
                let lib = &mut self.store.library;
                let before = lib.outputs.len();
                lib.outputs.retain(|o| o.name != name);
                if lib.outputs.len() == before {
                    return Err(CmdError::new("not_found", format!("no output {name:?}")));
                }
                // Its sends go; a tile with none left plays on Main.
                for t in lib.sets.iter_mut().flat_map(|s| s.tiles.iter_mut()) {
                    t.sends.retain(|s| s.output != name);
                    tidy_sends(&mut t.sends);
                }
            }
            "settings.previewOutput" => {
                let lib = &mut self.store.library;
                lib.preview_output = p.bool("on")?.unwrap_or(true);
                if p.has("device") {
                    lib.preview_device = p.str("device")?.map(str::to_owned);
                }
            }
            other => unreachable!("{other} isn't an output command"),
        }
        self.store.mark_dirty();
        Controller::configure_engine(&mut self.engine, &self.store.library);
        Ok(self.outputs_json())
    }

    /// An output's name as the library spells it, for a send: Main or one it defines.
    fn send_output(&self, cmd: &str, name: &str) -> Result<String, CmdError> {
        let name = name.trim();
        if name.is_empty() || name.eq_ignore_ascii_case(MAIN_OUTPUT) {
            return Ok(MAIN_OUTPUT.into());
        }
        let def = self.store.library.outputs.iter().find(|d| d.name.eq_ignore_ascii_case(name));
        Ok(def.ok_or_else(|| bad(format!("{cmd}: no output {name:?} (output.add makes one)")))?.name.clone())
    }

    /// A send's level in dB, checked.
    fn send_db(cmd: &str, db: Option<f64>) -> Result<Option<f32>, CmdError> {
        match db {
            Some(d) if !(-GAIN_DB_MAX as f64..=GAIN_DB_MAX as f64).contains(&d) => Err(bad(format!("{cmd}: gainDb must be −{GAIN_DB_MAX}…+{GAIN_DB_MAX}"))),
            d => Ok(d.map(|d| d as f32)),
        }
    }

    /// `tile.set sends`: a list of `{output, gainDb?}` (or output names), or null for Main.
    fn parse_sends(&self, v: &Value) -> Result<Vec<TileSend>, CmdError> {
        let list = match v {
            Value::Null => return Ok(Vec::new()),
            Value::String(raw) => match serde_json::from_str::<Value>(raw) {
                Ok(Value::Array(a)) => a,
                _ => raw.split(',').map(|o| json!(o.trim())).collect(),
            },
            Value::Array(a) => a.clone(),
            _ => return Err(bad("tile.set: sends is a list of {output, gainDb}")),
        };
        let mut sends = Vec::new();
        for item in &list {
            let (name, db) = match item {
                Value::String(o) => (o.as_str(), None),
                Value::Object(m) => (
                    m.get("output").and_then(Value::as_str).ok_or_else(|| bad("tile.set: each send needs an output"))?,
                    m.get("gainDb").or_else(|| m.get("gain_db")).and_then(Value::as_f64),
                ),
                _ => return Err(bad("tile.set: sends is a list of {output, gainDb}")),
            };
            sends.push(TileSend { output: self.send_output("tile.set", name)?, gain_db: Controller::send_db("tile.set", db)?.unwrap_or(0.0) });
        }
        Ok(sends)
    }

    /// `tile.set channels`: "auto" (or null), or one route per source channel, as a list
    /// of `{to, gainDb?}` or of strings like `left`, `both:-3`, `mute`, or one string
    /// with them separated by commas.
    fn parse_channel_mix(&self, tile: &Tile, v: &Value) -> Result<Vec<ChannelRoute>, CmdError> {
        let usage = "tile.set: channels is auto, or one of left|right|both|mute (with :dB) per channel";
        let one = |item: &str| -> Result<ChannelRoute, CmdError> {
            let (to, db) = item.trim().split_once(':').map_or((item.trim(), None), |(t, d)| (t.trim(), Some(d.trim())));
            let to = match to.to_ascii_lowercase().as_str() {
                "left" | "l" => ChannelTo::Left,
                "right" | "r" => ChannelTo::Right,
                "both" | "l+r" | "lr" => ChannelTo::Both,
                "mute" | "off" | "none" => ChannelTo::Mute,
                _ => return Err(bad(usage)),
            };
            let db = db.map(|d| d.trim_end_matches("dB").trim().parse::<f64>().map_err(|_| bad(usage))).transpose()?;
            Ok(ChannelRoute { to, gain_db: Controller::send_db("tile.set", db)?.unwrap_or(0.0) })
        };
        let mix = match v {
            Value::Null => Vec::new(),
            Value::String(s) if s.trim().eq_ignore_ascii_case("auto") || s.trim().is_empty() => Vec::new(),
            Value::String(s) => match serde_json::from_str::<Value>(s) {
                Ok(list @ Value::Array(_)) => return self.parse_channel_mix(tile, &list),
                _ => s.split(',').map(one).collect::<Result<_, _>>()?,
            },
            Value::Array(items) => items
                .iter()
                .map(|item| match item {
                    Value::String(s) => one(s),
                    Value::Object(_) => serde_json::from_value::<ChannelRoute>(item.clone()).map_err(|_| bad(usage)).and_then(|c| {
                        Controller::send_db("tile.set", Some(c.gain_db as f64))?;
                        Ok(c)
                    }),
                    _ => Err(bad(usage)),
                })
                .collect::<Result<_, _>>()?,
            _ => return Err(bad(usage)),
        };
        // Checked against the file when it's loaded (the offline CLI can't tell).
        if let Some(n) = self.store.audio(tile).and_then(|a| a.native.as_ref().map(|n| n.count())) {
            if !mix.is_empty() && mix.len() != n {
                return Err(bad(format!("tile.set: “{}” has {n} channels; give one route for each", tile.title)));
            }
        } else if !mix.is_empty() && self.store.audio(tile).is_some() {
            return Err(bad(format!("tile.set: “{}” is mono or stereo; a channel mix is for files with more channels", tile.title)));
        }
        Ok(mix)
    }

    /// `tile.send` and `tile.unsend`: add, change or remove one of a tile's sends.
    fn send_commands(&mut self, id: &str, p: &P) -> Res {
        let tile = self.tile_param(p)?;
        let mut t = self.store.library.tile(tile).cloned().expect("resolved");
        let output = self.send_output(id, p.need_str("output")?)?;
        // No sends means Main; spell it out before changing the list.
        t.sends = t.routes();
        let at = t.sends.iter().position(|s| s.output == output);
        match id {
            "tile.send" => {
                let db = Controller::send_db(id, p.f64("gainDb")?)?;
                match at {
                    Some(i) => t.sends[i].gain_db = db.unwrap_or(t.sends[i].gain_db),
                    None => t.sends.push(TileSend { output, gain_db: db.unwrap_or(0.0) }),
                }
            }
            "tile.unsend" => {
                let i = at.ok_or_else(|| CmdError::new("not_found", format!("tile.unsend: “{}” has no send to {output}", t.title)))?;
                t.sends.remove(i);
            }
            other => unreachable!("{other} isn't a send command"),
        }
        tidy_sends(&mut t.sends);
        self.engine.update_params(&t);
        if let Some(slot) = self.store.library.tile_mut(tile) {
            *slot = t;
        }
        self.store.mark_dirty();
        Ok(self.tile_json(tile))
    }

    /// Open the input while something needs it (and there's a real audio device, so the
    /// offline CLI and screenshot runs never touch the microphone); close it otherwise.
    fn keep_input(&mut self) {
        let lib = &self.store.library;
        let tap = self.recording.is_some() || lib.replay_seconds > 0;
        let wanted = (lib.voice_duck || tap) && self.engine.device_name.is_some();
        if wanted {
            self.list_inputs_once();
        }
        self.poll_input(wanted);
        let lib = &self.store.library;
        if !wanted {
            self.input = None;
            self.capture = None;
            self.replay.clear();
            // (The offline CLI has no window and no engine either; nothing to say there.)
            self.input_error = (lib.voice_duck && self.engine.device_name.is_none() && self.has_window)
                .then(|| "There's no audio output, so the microphone stays closed".to_owned());
            return;
        }
        if self.input.as_ref().is_some_and(|i| i.wanted != lib.input_device) {
            self.input = None;
            self.capture = None;
            self.replay.clear();
        }
        if self.input.is_none() && self.input_opening.is_none() && self.input_tried.is_none_or(|t| t.elapsed() > Duration::from_secs(3)) {
            self.input_tried = Some(Instant::now());
            self.open_input();
        }
        let lib = &self.store.library;
        if let Some(i) = &self.input {
            i.set_threshold(lib.voice_duck.then_some(lib.voice_threshold_db));
            match (tap, self.capture.is_some()) {
                (true, false) => self.capture = Some(Capture { rx: i.tap(), rate: i.rate, channels: i.channels }),
                (false, true) => {
                    i.untap();
                    self.capture = None;
                    self.replay.clear();
                }
                _ => {}
            }
        }
    }

    /// Fill `input_devices` if nothing has yet, so the input menu isn't empty until
    /// Refresh. Never at launch: only once voice ducking, a recording or instant replay
    /// wants the input, or Settings shows the Voice section, and only with a real audio
    /// device, like the input itself (listing doesn't ask for the microphone, D-125).
    pub fn list_inputs_once(&mut self) {
        if !self.inputs_listed && self.engine.device_name.is_some() {
            self.list_inputs();
        }
    }

    fn list_inputs(&mut self) {
        (self.input_devices, self.input_default) = (self.input_lister)();
        self.inputs_listed = true;
    }

    /// The input recording, instant replay and voice ducking use, by name: the one open,
    /// else the one chosen in Settings, else the system default.
    pub fn input_name(&self) -> String {
        match &self.input {
            Some(i) => i.device.clone(),
            None => self.store.library.input_device.clone().or_else(|| self.input_default.clone()).unwrap_or_else(|| "the system default input".into()),
        }
    }

    /// A recording in progress, as the window shows it (`None` when nothing records).
    pub fn recording_view(&self) -> Option<RecordingView> {
        let r = self.recording.as_ref()?;
        Some(RecordingView {
            tile: r.tile,
            title: self.store.library.tile(r.tile).map(|t| t.title.clone()).unwrap_or_default(),
            seconds: r.seconds(),
            input: self.input.as_ref().map(|i| i.device.clone()).or_else(|| self.staged_input.clone()),
            level_db: self.input.as_ref().map(|i| i.shared.level_db()).or(self.staged_input.as_ref().map(|_| -18.0)),
        })
    }

    /// Voice ducking: follow whether the input hears the board (D-127), and say so once.
    fn watch_hearing(&mut self, dt: f64) {
        let lib = &self.store.library;
        let Some(input) = self.input.as_ref().filter(|_| lib.voice_duck) else {
            self.hears_board.clear();
            return;
        };
        let master = if lib.master_volume <= 0.0 { crate::logic::DB_FLOOR } else { crate::logic::gain_to_db(lib.master_volume) };
        let board = self.statuses.keys().map(|id| self.level_db(*id)).fold(crate::logic::DB_FLOOR, f32::max) + master;
        if self.hears_board.feed(dt, board, input.shared.level_db(), lib.voice_threshold_db) && !self.hearing_noted {
            self.hearing_noted = true;
            self.notify(HEARS_BOARD);
        }
    }

    /// Start opening the input on a thread of its own; `poll_input` takes it from there.
    fn open_input(&mut self) {
        let wanted = self.store.library.input_device.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let (open, device, voice) = (self.input_opener.clone(), wanted.clone(), self.engine.voice_flag());
        let spawned = std::thread::Builder::new().name("input-open".into()).spawn(move || {
            tx.send(open(device.as_deref(), voice)).ok();
        });
        match spawned {
            Ok(_) => self.input_opening = Some(Opening { wanted, rx, since: Instant::now(), noted: false }),
            Err(e) => self.input_error = Some(format!("Couldn't open the input: {e}")),
        }
    }

    /// Take the input once its thread has opened it, or say why it couldn't. One that's
    /// no longer wanted, or was opened for another device, is closed again.
    fn poll_input(&mut self, wanted: bool) {
        let Some(mut opening) = self.input_opening.take() else { return };
        let result = match opening.rx.try_recv() {
            Ok(r) => r,
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                // Opening an input takes milliseconds; longer, and macOS is asking.
                if wanted && !opening.noted && opening.since.elapsed() > Duration::from_millis(500) {
                    opening.noted = true;
                    self.notify(INPUT_WAITING);
                }
                self.input_opening = Some(opening);
                return;
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Err(anyhow::anyhow!("its thread stopped")),
        };
        match result {
            Ok(i) if wanted && opening.wanted == self.store.library.input_device => {
                self.input = Some(i);
                self.input_error = None;
                if self.recording.is_some() {
                    self.recording_started();
                }
            }
            // Not wanted any more, or for another device: open the right one next.
            Ok(_) => self.input_tried = None,
            Err(e) => {
                let why = format!("Couldn't open the input: {e}");
                self.input_error = Some(why.clone());
                if let Some(r) = self.recording.take() {
                    if r.made {
                        self.store.remove_tile(r.tile);
                    }
                    self.notify_error(format!("Couldn't record: {why}"));
                }
            }
        }
    }

    /// What Settings and `status` say about the input when it isn't simply open: why it
    /// couldn't open, or that it's still opening.
    pub fn input_note(&self) -> Option<String> {
        self.input_error.clone().or_else(|| self.input_opening.as_ref().map(|_| INPUT_WAITING.to_owned()))
    }

    /// The input is opening (macOS may be asking for the microphone).
    pub fn input_waiting(&self) -> bool {
        self.input_opening.is_some()
    }

    /// OSC feedback: the current set's tiles (state 0 stopped, 1 playing, 2 paused, title,
    /// colour, remaining time, progress and, unless turned off, level), the set's name and
    /// tile count, the cue pointer, the tile in the counter (`/ls/now/...`), master, the
    /// duck and recording.
    pub fn osc_state(&self) -> Vec<(String, crate::osc::OscArg)> {
        use crate::osc::OscArg::{Num, Str};
        use crate::osc::{coarse, countdown};
        let set = self.current_set();
        let mut out = vec![("/ls/set".to_owned(), Str(set.name.clone())), ("/ls/tiles".to_owned(), Num(set.tiles.len() as f32))];
        let timing = |s: Option<&VoiceStatus>| match s {
            Some(s) => (coarse(s.remaining(), 0.1), coarse(s.progress() as f64, 0.01), countdown(s.remaining())),
            None => (0.0, 0.0, String::new()),
        };
        let levels = self.store.library.osc.levels;
        // 0 as soon as the tile stops, without waiting for the next sample.
        let level = |id: Uuid| if self.statuses.contains_key(&id) { self.osc_levels.get(&id).copied().unwrap_or(0.0) } else { 0.0 };
        for (i, t) in set.tiles.iter().enumerate() {
            let status = self.statuses.get(&t.id);
            let state = match status.map(|s| s.state) {
                Some(PlayState::Playing | PlayState::Stopping) => 1.0,
                Some(PlayState::Paused) => 2.0,
                _ => 0.0,
            };
            let n = i + 1;
            let (remaining, progress, time) = timing(status);
            out.push((format!("/ls/tile/{n}/state"), Num(state)));
            out.push((format!("/ls/tile/{n}/title"), Str(t.title.clone())));
            out.push((format!("/ls/tile/{n}/color"), Str(enum_name(t.color).as_str().unwrap_or_default().to_owned())));
            let rgb = crate::theme::tile_fill(t.color);
            out.push((format!("/ls/tile/{n}/rgb"), Str(format!("#{:02X}{:02X}{:02X}", rgb.r(), rgb.g(), rgb.b()))));
            out.push((format!("/ls/tile/{n}/remaining"), Num(remaining)));
            out.push((format!("/ls/tile/{n}/progress"), Num(progress)));
            out.push((format!("/ls/tile/{n}/time"), Str(time)));
            if levels {
                out.push((format!("/ls/tile/{n}/level"), Num(level(t.id))));
            }
        }
        let pointer = self.cue.get(&set.id).copied().unwrap_or(0);
        let next = next_cue(&set.tiles, pointer);
        out.push(("/ls/cue/next".into(), Num(next.map_or(0.0, |i| i as f32 + 1.0))));
        out.push(("/ls/cue/next/title".into(), Str(next.map(|i| set.tiles[i].title.clone()).unwrap_or_default())));
        // The pointer sits just past the cue GO last played.
        out.push(("/ls/cue/current".into(), Num(pointer.min(set.tiles.len()) as f32)));
        // The counter's tile, as the window picks it: the last one started while it
        // sounds, or else the playing tile that ends first.
        let now = self
            .last_started
            .and_then(|id| self.statuses.get(&id).map(|s| (id, s)))
            .or_else(|| self.statuses.iter().filter(|(_, s)| s.state == PlayState::Playing).min_by(|a, b| a.1.remaining().total_cmp(&b.1.remaining())).map(|(id, s)| (*id, s)));
        let (remaining, progress, time) = timing(now.map(|(_, s)| s));
        out.push(("/ls/now/title".into(), Str(now.and_then(|(id, _)| self.store.library.tile(id)).map(|t| t.title.clone()).unwrap_or_default())));
        out.push(("/ls/now/remaining".into(), Num(remaining)));
        out.push(("/ls/now/progress".into(), Num(progress)));
        out.push(("/ls/now/time".into(), Str(time)));
        if levels {
            out.push(("/ls/now/level".into(), Num(now.map_or(0.0, |(id, _)| level(id)))));
        }
        let pages = crate::pads::pages(set.tiles.len(), self.store.library.midi.bank);
        out.push(("/ls/midi/page".into(), Num(self.pad_paging.page(set.id, pages) as f32 + 1.0)));
        out.push(("/ls/midi/pages".into(), Num(pages as f32)));
        out.push(("/ls/master".into(), Num(self.store.library.master_volume)));
        out.push(("/ls/duck".into(), Num(if self.engine.ducking() { 1.0 } else { 0.0 })));
        out.push(("/ls/recording".into(), Num(if self.recording.is_some() { 1.0 } else { 0.0 })));
        out
    }

    /// Pad lights: what each bound or auto-mapped pad (channel, note) should show now, as
    /// (channel, velocity): `lit`/`unlit`, or the tile's colour from the pad palette.
    fn led_state(&self) -> HashMap<(u8, u8), (u8, u8)> {
        let m = &self.store.library.midi;
        let sounding = |id: Uuid| self.statuses.get(&id).is_some_and(|s| matches!(s.state, PlayState::Playing | PlayState::Stopping));
        let light = |t: &Tile, ch: u8| match &m.palette {
            Some(p) => p.light(t.color, sounding(t.id), ch),
            None => (ch, if sounding(t.id) { m.lit } else { m.unlit }),
        };
        let mut out: HashMap<(u8, u8), (u8, u8)> = HashMap::new();
        for b in self.store.library.bindings.iter().filter(|b| b.enabled) {
            let (Source::Midi(src), Some(t)) = (&b.source, b.params.get("tile").and_then(|v| v.as_str())) else { continue };
            let (Some(note), Some(tile)) = (src.note, t.parse::<Uuid>().ok().and_then(|id| self.store.library.tile(id))) else { continue };
            let ch = src.channel.unwrap_or(1);
            // A pad bound to several tiles shows the one that plays.
            if !out.contains_key(&(ch, note)) || sounding(tile.id) {
                out.insert((ch, note), light(tile, ch));
            }
        }
        if let Some(base) = m.auto_map {
            let set = self.current_set();
            let ch = m.feedback_channel;
            let (first, count) = self.auto_pads(set);
            for k in 0..count {
                let Some(note) = (base as usize + k).try_into().ok().filter(|n: &u8| *n <= 127) else { break };
                let lit = match set.tiles.get(first + k) {
                    Some(t) => light(t, ch),
                    None => (m.palette.as_ref().and_then(|p| p.idle_channel).unwrap_or(ch), m.palette.as_ref().map_or(0, |p| p.empty)),
                };
                out.entry((ch, note)).or_insert(lit);
            }
        }
        out
    }

    /// The auto-mapped pads in `set`: the first tile's index and how many pads, which with
    /// a bank is the whole page (empty pads included) and without one every tile.
    fn auto_pads(&self, set: &SoundSet) -> (usize, usize) {
        let bank = self.store.library.midi.bank;
        if bank == 0 {
            return (0, set.tiles.len());
        }
        let page = self.pad_paging.page(set.id, crate::pads::pages(set.tiles.len(), bank));
        (page * bank as usize, bank as usize)
    }

    fn send_pad_lights(&mut self) {
        if !self.store.library.midi.feedback || self.midi_out.is_none() {
            self.leds.clear();
            return;
        }
        let want = self.led_state();
        // Everything again every few seconds: a controller plugged in late lights up.
        let full = self.leds_sent.is_none_or(|t| t.elapsed() > Duration::from_secs(5));
        if full {
            self.leds_sent = Some(Instant::now());
        }
        let tx = self.midi_out.as_ref().expect("checked");
        for (&(ch, note), &(send_ch, vel)) in &want {
            if full || self.leds.get(&(ch, note)) != Some(&(send_ch, vel)) {
                tx.send(vec![0x90 | (send_ch.clamp(1, 16) - 1), note & 0x7F, vel & 0x7F]).ok();
            }
        }
        self.leds = want;
    }

    /// Send each tile's start and stop messages (src/messages.rs), and show what failed.
    fn send_messages(&mut self) {
        let (started, stopped) = self.messages.transitions(&self.statuses);
        let lan = self.store.library.osc.lan;
        for (ids, starting) in [(started, true), (stopped, false)] {
            for id in ids {
                // A preview is for the operator's ears only.
                if self.messages.previews.contains(&id) {
                    if !starting {
                        self.messages.previews.remove(&id);
                    }
                    continue;
                }
                let Some(t) = self.store.library.tile(id) else { continue };
                let list = if starting { t.on_start.clone() } else { t.on_stop.clone() };
                for s in list {
                    if let Ok(a) = crate::messages::parse(&s) {
                        self.messages.send(a, lan);
                    }
                }
            }
        }
        if let Some(e) = self.messages.errors().pop() {
            self.notify_error(e);
        }
    }

    /// `onStart` / `onStop` for `tile.set`: one message, several (a list, or lines), or
    /// null or "" for none. OSC to another machine needs OSC's network setting (D-042).
    fn messages_param(&self, p: &P, key: &str) -> Result<Option<Vec<String>>, CmdError> {
        if !p.has(key) {
            return Ok(None);
        }
        let list: Vec<String> = match p.get(key) {
            None => Vec::new(),
            Some(Value::String(s)) => s.lines().map(str::to_owned).collect(),
            Some(Value::Array(a)) => a
                .iter()
                .map(|v| v.as_str().map(str::to_owned).ok_or_else(|| bad(format!("tile.set: {key} is a list of messages"))))
                .collect::<Result<_, _>>()?,
            Some(_) => return Err(bad(format!("tile.set: {key} is a send or a list of them"))),
        };
        let list: Vec<String> = list.iter().map(|s| s.trim()).filter(|s| !s.is_empty()).map(str::to_owned).collect();
        for s in &list {
            let a = crate::messages::parse(s).map_err(|e| bad(format!("tile.set: {key}: {e}")))?;
            if crate::messages::is_remote_literal(&a) && !self.store.library.osc.lan {
                return Err(bad(format!("tile.set: {key}: {s:?} is another machine; OSC messages stay on this one unless OSC may use the network (osc.enable --lan true)")));
            }
        }
        Ok(Some(list))
    }

    fn send_osc_feedback(&mut self) {
        let wanted = self.store.library.osc.feedback.clone().filter(|_| self.store.library.osc.enabled);
        if self.osc_feedback.as_ref().map(|f| &f.target) != wanted.as_ref() {
            self.osc_feedback = wanted.as_deref().and_then(crate::osc::Feedback::new);
        }
        if self.osc_feedback.is_some() {
            let state = self.osc_state();
            if let Some(f) = &mut self.osc_feedback {
                f.send(state);
            }
        }
    }

    /// Move the input's samples into the recording and the replay buffer.
    fn pump_capture(&mut self) {
        let Some(cap) = &self.capture else { return };
        let keep = self.store.library.replay_seconds as usize * cap.rate as usize * 2;
        while let Ok(block) = cap.rx.try_recv() {
            let stereo = to_stereo(&block, cap.channels);
            if let Some(r) = &mut self.recording {
                r.data.extend_from_slice(&stereo);
            }
            if keep > 0 {
                self.replay.extend(stereo);
                let over = self.replay.len().saturating_sub(keep);
                self.replay.drain(..over);
            }
        }
        self.replay_rate = cap.rate;
    }

    /// Seconds in the replay buffer.
    pub fn replay_seconds(&self) -> f64 {
        self.replay.len() as f64 / 2.0 / self.replay_rate.max(1) as f64
    }

    /// `replay.save`: the last seconds of input as a new tile.
    fn save_replay(&mut self, p: &P) -> Res {
        self.pump_capture();
        let lib_secs = self.store.library.replay_seconds;
        if lib_secs == 0 {
            return Err(CmdError::new("off", "Instant replay is off: turn it on with replay.enable or in Settings"));
        }
        let secs = p.f64("seconds")?.unwrap_or(lib_secs as f64).max(0.0);
        let n = ((secs * self.replay_rate as f64) as usize * 2).min(self.replay.len());
        if n < 2 * (self.replay_rate as usize / 10) {
            return Err(CmdError::new("failed", "There's nothing to replay yet"));
        }
        let data: Vec<f32> = self.replay.iter().skip(self.replay.len() - n).copied().collect();
        let si = self.set_param(p)?;
        let set = &mut self.store.library.sets[si];
        let k = set.tiles.iter().filter(|t| t.title.starts_with("Replay")).count() + 1;
        let title = p.str("title")?.map(str::to_owned).unwrap_or_else(|| format!("Replay {k}"));
        let t = Tile { title: title.clone(), color: TileColor::Orange, ..Default::default() };
        let id = t.id;
        set.tiles.push(t);
        let audio = crate::decode::DecodedAudio { sample_rate: self.replay_rate, data, art: None, native: None };
        let seconds = self.store.attach_recording(id, audio).map_err(|e| CmdError::new("failed", format!("{e:#}")))?;
        self.notify(format!("Saved the last {} as {title}", crate::logic::format_clock(seconds, true)));
        Ok(json!({"tile": id, "title": title, "seconds": (seconds * 10.0).round() / 10.0}))
    }

    /// The tiles' meters: each sounding tile's peak since the last tick, through the
    /// master meter's ballistics. A tile that stopped drops its meter.
    fn feed_levels(&mut self, dt: f32) {
        let peaks = self.engine.take_levels();
        self.levels.retain(|id, _| self.statuses.contains_key(id));
        for id in self.statuses.keys() {
            self.levels.entry(*id).or_default().feed(peaks.get(id).copied().unwrap_or(0.0), dt);
        }
    }

    /// Sample the tiles' levels for OSC feedback, every [`crate::osc::LEVEL_EVERY`]
    /// seconds of ticks, so a busy board sends each one ten times a second and not every
    /// frame. A tile that stopped drops out at once.
    fn sample_osc_levels(&mut self, dt: f64) {
        self.osc_levels.retain(|id, _| self.statuses.contains_key(id));
        self.osc_level_clock += dt;
        if self.osc_level_clock < crate::osc::LEVEL_EVERY {
            return;
        }
        self.osc_level_clock = 0.0;
        self.osc_levels = self.levels.iter().map(|(id, b)| (*id, crate::osc::coarse(crate::logic::meter_frac(b.level) as f64, 0.01))).collect();
    }

    /// The meter level of a tile in dB, as its meter shows it (the floor when silent).
    pub fn level_db(&self, id: Uuid) -> f32 {
        self.levels.get(&id).map_or(crate::logic::DB_FLOOR, |b| b.level)
    }

    /// A lost device closes only its own outputs, and every few seconds closed outputs
    /// whose device is back reopen (D-122); the status bar says what happened.
    fn watch_outputs(&mut self) {
        let mut changes = self.engine.check_streams();
        if self.last_reopen.elapsed() > Duration::from_secs(3) && self.engine.wants_reopen() {
            self.last_reopen = Instant::now();
            changes.extend(self.engine.reopen_closed());
        }
        if changes.is_empty() {
            return;
        }
        use crate::audio::OutputChange as C;
        let lost = changes.iter().any(|c| matches!(c, C::Lost { .. } | C::Main { from: Some(_), .. } | C::Main { to: None, .. }));
        let text: Vec<String> = changes
            .iter()
            .map(|c| match c {
                C::Lost { output, device } => format!("{output} lost {device}: its tiles play on Main"),
                C::Back { output, device } => format!("{output} is back on {device}"),
                C::Main { from: Some(from), to: Some(to) } => format!("Main lost {from}: playing on {to}"),
                C::Main { from: Some(from), to: None } => format!("Main lost {from}: no output device"),
                C::Main { from: None, to: Some(to) } => format!("Main is back on {to}"),
                C::Main { from: None, to: None } => "Main has no output device".into(),
            })
            .collect();
        let text = text.join("; ");
        if lost {
            self.notify_error(text);
        } else {
            self.notify(text);
        }
    }

    /// Advance time: decodes, a lost device, finished voices and auto-advance.
    pub fn tick(&mut self, dt: f64) {
        self.keep_input();
        self.pump_capture();
        if let Some(r) = &self.recording {
            if r.started.elapsed() > RECORD_MAX {
                if let Err(e) = self.finish_recording() {
                    self.report(&e);
                }
            }
        }
        if self.learning.as_ref().is_some_and(|l| Instant::now() > l.until) {
            self.learning = None;
            self.notify("Stopped waiting for a MIDI pad or key");
        }
        self.last_tick = Instant::now();
        self.engine.tick_without_device(dt);
        self.store.poll();
        self.poll_renders();
        self.poll_theme();
        crate::quip::clips::poll(self);
        crate::quip::watch::poll(self);
        self.watch_outputs();
        self.run_pending();
        self.statuses = self.engine.statuses();
        self.feed_levels(dt as f32);
        self.watch_hearing(dt);
        self.sample_osc_levels(dt);
        self.send_osc_feedback();
        self.send_pad_lights();
        self.send_messages();
        let finished = self.engine.take_finished();
        for &id in &finished {
            let Some((si, ti)) = self.store.library.locate(id) else { continue };
            let set = &self.store.library.sets[si];
            let tile = &set.tiles[ti];
            // A tile's own link wins over the set's auto-advance.
            match tile.link {
                CueLink::Follow => {
                    let wait = tile.post_wait;
                    self.link_next(id, wait);
                    continue;
                }
                CueLink::Continue => continue,
                CueLink::None => {}
            }
        }
        self.advance_lists(&finished);
        self.sync_list_selection();
        self.announce_changes();
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
            let c = PadKey::parse(k).ok_or_else(|| bad(format!("{r:?}: {KEY_HELP}")))?;
            return self.tile_for_key(c).ok_or_else(|| CmdError::new("not_found", format!("no tile has the hotkey {} in “{}”", c.label(), cur.name)));
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

    /// The tile an in-app key plays: the current set's, or with tile keys in every set
    /// on, the first in the other sets in sidebar order.
    pub fn tile_for_key(&self, key: PadKey) -> Option<Uuid> {
        let sets = &self.store.library.sets;
        let cur = self.set_idx();
        let others = (0..sets.len()).filter(|&i| i != cur).filter(|_| self.store.library.tile_keys_all_sets);
        std::iter::once(cur).chain(others).find_map(|si| tile_for_key(&sets[si].tiles, key)).map(|t| t.id)
    }

    /// The locked set a command would change, if it would change one (M22): the sets of
    /// the tiles it names, the set it names, or else the current set.
    fn locked_target(&self, id: &str, p: &P) -> Option<usize> {
        let edits = spec(id).is_some_and(|s| s.undo) || SET_SETTINGS.contains(&id);
        if !edits || LOCK_FREE.contains(&id) || (id == "record.toggle" && self.recording.is_some()) {
            return None;
        }
        let lib = &self.store.library;
        let mut tiles = match p.get("tiles") {
            Some(list @ Value::Array(_)) => self.tile_refs(list, id).unwrap_or_default(),
            _ => Vec::new(),
        };
        let tile = p.str("tile").ok().flatten();
        tiles.extend(tile.and_then(|r| self.resolve_tile(r).ok()));
        let mut sets: Vec<usize> = tiles.iter().filter_map(|t| lib.locate(*t)).map(|(si, _)| si).collect();
        match p.str("set").ok().flatten() {
            Some(r) => sets.extend(self.resolve_set(Some(r)).ok()),
            None if tile.is_none() && tiles.is_empty() => sets.push(self.set_idx()),
            None => {}
        }
        sets.into_iter().find(|&si| lib.sets[si].locked)
    }

    fn tile_param(&self, p: &P) -> Result<Uuid, CmdError> {
        self.resolve_tile(p.need_str("tile")?)
    }

    fn set_param(&self, p: &P) -> Result<usize, CmdError> {
        self.resolve_set(p.str("set")?)
    }

    // ── playback ────────────────────────────────────────────────────────────

    /// `toggle`: Enter on a focused pad (D-196): a press while it plays stops it even when
    /// it's One-shot, and one in its pre-wait cancels the wait.
    fn trigger(&mut self, id: Uuid, event: TriggerEvent, toggle: bool) -> Result<(), CmdError> {
        let Some(mut tile) = self.store.library.tile(id).cloned() else { return Ok(()) };
        if toggle {
            tile.press.one_shot = false;
        }
        // A release still goes through, so a held tile disabled mid-press stops.
        if event == TriggerEvent::Press {
            refuse_disabled(&tile)?;
        }
        // In its pre-wait: a press cancels it (One-shot starts the wait again), and so
        // does letting go of a Hold tile.
        if self.waiting(id).is_some() {
            match event {
                TriggerEvent::Press if tile.press.one_shot => self.start(&tile)?,
                TriggerEvent::Press => self.cancel_waits(id),
                TriggerEvent::Release if tile.press.hold => self.cancel_waits(id),
                TriggerEvent::Release => {}
            }
            return Ok(());
        }
        match resolve_press(tile.press, self.engine.state(id), self.engine.vamping(id), event) {
            TriggerAction::None => {}
            TriggerAction::Play => self.start(&tile)?,
            TriggerAction::Stop => self.stop_tile(id, tile.stop_fade),
            TriggerAction::Pause => self.engine.pause(id),
            TriggerAction::Resume => self.engine.resume(id),
            TriggerAction::PlayOn => {
                self.engine.play_on_vamp(id);
            }
        }
        Ok(())
    }

    fn start(&mut self, tile: &Tile) -> Result<(), CmdError> {
        self.start_as(tile, false)
    }

    /// Start a tile; a preview goes to the preview output (when there is one) and
    /// leaves the cue pointer alone. A tile with a pre-wait starts once it has passed
    /// (moving the pointer now, as GO does).
    fn start_as(&mut self, tile: &Tile, preview: bool) -> Result<(), CmdError> {
        self.cancel_pre_wait(tile.id);
        if !preview && tile.pre_wait > 0.0 {
            self.stop_all_fading = None;
            self.pending.push((Instant::now() + Duration::from_secs_f64(tile.pre_wait), Wait::PreWait(tile.id)));
            self.move_pointer_past(tile.id);
            return Ok(());
        }
        self.start_now(tile, preview)
    }

    /// Close the waveform editor, and forget its selection and any zoom it hasn't taken.
    fn close_editor(&mut self) {
        self.editor = None;
        self.editor_selection = None;
        self.editor_zoom = None;
    }

    /// The tile open in the editor and its length in seconds, for the editor's commands.
    fn editor_tile(&self, p: &P) -> Result<(Uuid, f64), CmdError> {
        let id = self.editor.ok_or_else(|| bad(format!("{}: open the waveform editor first (view.editor)", p.id)))?;
        let t = self.store.library.tile(id).expect("open");
        let dur = self.store.audio(t).map(|a| a.duration()).or(t.duration);
        dur.map(|d| (id, d)).ok_or_else(|| CmdError::new("loading", format!("“{}” is still loading…", t.title)))
    }

    /// The cue pointer after starting this tile: past it and every cue its links start.
    /// In a List set the tile is selected instead, as the list's current cue (D-197).
    /// Either way it has played now.
    fn move_pointer_past(&mut self, id: Uuid) {
        self.played.insert(id);
        if let Some((si, ti)) = self.store.library.locate(id) {
            let set = &self.store.library.sets[si];
            if set.layout == Layout::List {
                self.cue.insert(set.id, ti);
                self.list_current.insert(set.id, id);
            } else {
                self.cue.insert(set.id, crate::logic::pointer_after_chain(&set.tiles, ti));
            }
        }
    }

    /// The cue a List set plays next once its current cue ends: the next armed one after
    /// it (D-197). Queued shows it while the current cue sounds.
    pub fn list_queued(&self, si: usize) -> Option<usize> {
        let set = &self.store.library.sets[si];
        let cur = self.list_current.get(&set.id)?;
        let ti = set.tiles.iter().position(|t| t.id == *cur)?;
        Some(next_armed(&set.tiles, ti + 1, &self.played)).filter(|&n| n < set.tiles.len())
    }

    /// Each List set's current cue, once per tick (D-197): near its end in Continuous with
    /// a cross-fade, start the next armed cue fading in while it fades out; once it has
    /// ended, Continuous starts the next armed cue, and once it has ended or stopped,
    /// Queued selects it. A cue stopped in Continuous starts nothing.
    fn advance_lists(&mut self, finished: &[Uuid]) {
        let currents: Vec<(Uuid, Uuid)> = self.list_current.iter().map(|(s, t)| (*s, *t)).collect();
        for (sid, tid) in currents {
            let Some(si) = self.store.library.set_index(sid) else {
                self.list_current.remove(&sid);
                continue;
            };
            let set = &self.store.library.sets[si];
            let Some(ti) = set.tiles.iter().position(|t| t.id == tid).filter(|_| set.layout == Layout::List) else {
                self.list_current.remove(&sid);
                continue;
            };
            if self.waiting(tid).is_some() {
                continue;
            }
            let tile = &set.tiles[ti];
            let next = Some(next_armed(&set.tiles, ti + 1, &self.played)).filter(|&n| n < set.tiles.len());
            let continuous = set.auto_advance && tile.link == CueLink::None;
            // (A cue that ended since the voices were read counts as ended.)
            if let Some(s) = self.statuses.get(&tid).filter(|_| !finished.contains(&tid)) {
                // The cross-fade: at most half of either cue.
                let next_len = next.and_then(|n| self.store.audio(&set.tiles[n]).map(|a| a.duration()));
                if let (true, Some(n), Some(next_len)) = (continuous && set.crossfade > 0.0 && !tile.looping, next, next_len) {
                    let xf = (set.crossfade as f64).min(s.length / 2.0).min(next_len / 2.0);
                    if s.state == PlayState::Playing && !s.vamping && s.remaining() <= xf {
                        let left = s.remaining() as f32;
                        let mut next_tile = set.tiles[n].clone();
                        next_tile.fade_in = next_tile.fade_in.max(xf as f32);
                        self.engine.stop(tid, left);
                        if let Err(e) = self.start(&next_tile) {
                            self.report(&e);
                        }
                    }
                }
                continue;
            }
            self.list_current.remove(&sid);
            match next {
                Some(_) if continuous && finished.contains(&tid) => {
                    let next_tile = set.tiles[next.expect("matched")].clone();
                    if let Err(e) = self.start(&next_tile) {
                        self.report(&e);
                    }
                }
                Some(n) if !set.auto_advance => {
                    self.cue.insert(sid, n);
                }
                _ => {}
            }
        }
    }

    /// Play Selection in a List set (`cue.go`, Enter, D-197): the selected cue plays, and
    /// whatever else in the list is playing stops (over the cross-fade in Continuous, at
    /// once in Queued); pressed again while it plays, it stops (a Pausable cue pauses, and
    /// resumes after); a vamping cue plays on instead (D-155). The selection stays on it.
    fn play_selection(&mut self, si: usize) -> Result<Value, CmdError> {
        let set = &self.store.library.sets[si];
        let pointer = self.cue.get(&set.id).copied().unwrap_or(0);
        let Some(i) = next_cue(&set.tiles, pointer) else {
            return Err(CmdError::new("end_of_cues", "Nothing to play: select a cue in the list"));
        };
        let tile = set.tiles[i].clone();
        let reply = |what: &str| json!({"tile": tile.id, "title": tile.title, "position": i + 1, "action": what});
        if self.engine.vamping(tile.id) {
            self.engine.play_on_vamp(tile.id);
            let mut r = reply("played_on");
            r["played_on"] = json!(true);
            return Ok(r);
        }
        match self.engine.state(tile.id) {
            PlayState::Playing if tile.press.pausable => {
                self.engine.pause(tile.id);
                Ok(reply("paused"))
            }
            PlayState::Paused => {
                self.engine.resume(tile.id);
                Ok(reply("resumed"))
            }
            PlayState::Playing => {
                self.execute("tile.stop", json!({"tile": tile.id}))?;
                Ok(reply("stopped"))
            }
            PlayState::Stopped | PlayState::Stopping => {
                let fade = if set.auto_advance { set.crossfade } else { 0.0 };
                let others: Vec<Uuid> = set.tiles.iter().map(|t| t.id).filter(|id| *id != tile.id && self.engine.state(*id) != PlayState::Stopped).collect();
                for id in others {
                    self.cancel_pre_wait(id);
                    self.engine.stop(id, fade);
                }
                self.start(&tile)?;
                Ok(reply("played"))
            }
        }
    }

    /// In a List set the selection is the next cue (D-192): whichever of the two moved
    /// last, the other follows. Moving the pointer selects its cue (nothing at the end of
    /// the list); selecting a row of the set makes it the next cue.
    fn sync_list_selection(&mut self) {
        let set = self.current_set();
        if set.layout != Layout::List || self.search.is_some() {
            self.list_sync = None;
            return;
        }
        let pointer = self.cue.get(&set.id).copied().unwrap_or(0);
        let now = (set.id, pointer, self.selected_tile);
        match self.list_sync {
            Some(seen) if seen == now => return,
            Some((sid, p, _)) if sid == set.id && p != pointer => {
                let next = next_cue(&set.tiles, pointer).map(|i| set.tiles[i].id);
                self.selected_tile = next;
                self.selection = next.into_iter().collect();
            }
            _ => {
                if let Some(i) = self.selected_tile.and_then(|id| set.tiles.iter().position(|t| t.id == id)) {
                    self.cue.insert(set.id, i);
                }
            }
        }
        let set = self.current_set();
        self.list_sync = Some((set.id, self.cue.get(&set.id).copied().unwrap_or(0), self.selected_tile));
    }

    /// Start a tile at once (its pre-wait is over, or it has none).
    fn start_now(&mut self, tile: &Tile, preview: bool) -> Result<(), CmdError> {
        // An action tile acts on its targets; like a played cue, it moves the pointer on.
        if let Some(action) = &tile.action {
            if preview {
                return Err(CmdError::new("no_audio", format!("“{}” is an action tile: there's nothing to preview", tile.title)));
            }
            self.run_action(action);
            self.move_pointer_past(tile.id);
            // An action is over as soon as it has run, so a follow link goes on at once.
            if tile.link != CueLink::None {
                self.link_next(tile.id, tile.post_wait);
            }
            return Ok(());
        }
        if tile.pitch != 0.0 && matches!(self.store.media(tile), Some(Media::Ready { .. })) {
            if let Some(f) = &tile.file {
                self.store.request_pitch(f, tile.pitch);
            }
        }
        let playable = self.store.playable(tile);
        match self.store.media(tile) {
            Some(Media::Ready { .. }) if matches!(playable, Err(true)) => {
                Err(CmdError::new("loading", format!("“{}” is still being transposed…", tile.title)))
            }
            Some(Media::Ready { .. }) if preview && self.store.library.preview_output => {
                // Preview's device missing: nothing plays, never the house (D-136).
                self.engine.preview(tile, playable.expect("ready")).map_err(|e| CmdError::new("preview_unavailable", e))?;
                self.stop_all_fading = None;
                Ok(())
            }
            Some(Media::Ready { .. }) => {
                let audio = playable.expect("ready");
                self.stop_all_fading = None;
                self.engine.play(tile, audio);
                self.last_started = Some(tile.id);
                self.move_pointer_past(tile.id);
                if tile.link == CueLink::Continue {
                    self.link_next(tile.id, tile.post_wait);
                }
                Ok(())
            }
            Some(Media::Loading) => Err(CmdError::new("loading", format!("“{}” is still loading…", tile.title))),
            Some(Media::Failed(e)) => Err(CmdError::new("decode_failed", format!("Can't play “{}”: {e}", tile.title))),
            None => Err(CmdError::new("no_audio", format!("“{}” has no audio. Drop a file on it.", tile.title))),
        }
    }

    /// The tiles an action's targets name, sets expanded, in order and once each.
    pub fn action_targets(&self, a: &TileAction) -> Vec<Uuid> {
        let lib = &self.store.library;
        let mut out: Vec<Uuid> = Vec::new();
        for t in &a.targets {
            let ids: Vec<Uuid> = match (t.tile, t.set) {
                (Some(id), _) => lib.tile(id).map(|t| t.id).into_iter().collect(),
                (None, Some(sid)) => lib.set_index(sid).map(|si| lib.sets[si].tiles.iter().map(|t| t.id).collect()).unwrap_or_default(),
                _ => Vec::new(),
            };
            for id in ids {
                if !out.contains(&id) {
                    out.push(id);
                }
            }
        }
        out
    }

    /// Carry out an action tile: fade, stop, pause or play on its targets that are sounding.
    fn run_action(&mut self, a: &TileAction) {
        if a.kind == ActionKind::Play {
            self.run_group(a);
            return;
        }
        for id in self.action_targets(a) {
            let state = self.engine.state(id);
            if state == PlayState::Stopped {
                continue;
            }
            match a.kind {
                ActionKind::Fade => self.stop_tile(id, a.fade.max(0.0)),
                ActionKind::Stop => {
                    let fade = self.store.library.tile(id).map_or(0.0, |t| t.stop_fade);
                    self.stop_tile(id, fade);
                }
                ActionKind::Pause if state == PlayState::Playing => self.engine.pause(id),
                ActionKind::Pause => {}
                ActionKind::PlayOn => {
                    self.engine.play_on_vamp(id);
                }
                ActionKind::Play => unreachable!("groups run in run_group"),
            }
        }
    }

    /// A group: start each target now or after its offset. A set target starts every
    /// tile in it. Disabled and missing targets are skipped.
    fn run_group(&mut self, a: &TileAction) {
        if self.action_depth >= 4 {
            self.notify_error("A group fires itself (through other groups); stopped there");
            return;
        }
        for t in &a.targets {
            let one = TileAction { kind: ActionKind::Fade, targets: vec![ActionTarget { offset: 0.0, ..t.clone() }], fade: 0.0 };
            for id in self.action_targets(&one) {
                if t.offset > 0.0 {
                    self.pending.push((Instant::now() + Duration::from_secs_f64(t.offset), Wait::Target(id)));
                } else {
                    self.start_target(id);
                }
            }
        }
    }

    /// Start one of a group's targets, leaving the cue pointers where they were (the
    /// group's own cue moves its set's pointer on).
    fn start_target(&mut self, id: Uuid) {
        let Some(tile) = self.store.library.tile(id).cloned().filter(crate::logic::is_cue) else { return };
        let pointers = self.cue.clone();
        self.action_depth += 1;
        let result = self.start(&tile);
        self.action_depth -= 1;
        self.cue = pointers;
        if let Err(e) = result {
            self.report(&e);
        }
    }

    /// Run the waits whose time has come: group targets, pre-waits and cue links.
    fn run_pending(&mut self) {
        let now = Instant::now();
        let (due, later): (Vec<_>, Vec<_>) = std::mem::take(&mut self.pending).into_iter().partition(|(at, _)| *at <= now);
        self.pending = later;
        for (_, wait) in due {
            match wait {
                Wait::Target(id) => self.start_target(id),
                Wait::PreWait(id) => {
                    // The pointer moved when it was triggered; a disabled tile is skipped.
                    let Some(tile) = self.store.library.tile(id).cloned().filter(crate::logic::is_cue) else { continue };
                    let pointers = self.cue.clone();
                    let result = self.start_now(&tile, false);
                    self.cue = pointers;
                    if let Err(e) = result {
                        self.report(&e);
                    }
                }
                Wait::Next(id) => self.start_next_after(id),
            }
        }
    }

    /// A tile's link to the next cue: start it `post_wait` seconds from now. With no
    /// wait it starts at once, so linked cues start together.
    fn link_next(&mut self, id: Uuid, post_wait: f64) {
        if post_wait <= 0.0 && self.link_depth < 32 {
            self.link_depth += 1;
            self.start_next_after(id);
            self.link_depth -= 1;
        } else {
            self.pending.push((Instant::now() + Duration::from_secs_f64(post_wait.max(0.0)), Wait::Next(id)));
        }
    }

    /// Start the cue after `id` in its set (disabled tiles are skipped), leaving the
    /// pointer where GO put it: past the whole chain.
    fn start_next_after(&mut self, id: Uuid) {
        let Some((si, ti)) = self.store.library.locate(id) else { return };
        let tiles = &self.store.library.sets[si].tiles;
        let Some(next) = crate::logic::next_cue(tiles, ti + 1).map(|i| tiles[i].clone()) else { return };
        let pointers = self.cue.clone();
        let result = self.start(&next);
        self.cue = pointers;
        if let Err(e) = result {
            self.report(&e);
        }
    }

    /// Stop a tile the user (or an action) stopped: its pre-wait and the link it has
    /// waiting go too.
    fn stop_tile(&mut self, id: Uuid, fade: f32) {
        self.cancel_waits(id);
        self.engine.stop(id, fade);
    }

    fn cancel_pre_wait(&mut self, id: Uuid) {
        self.pending.retain(|(_, w)| *w != Wait::PreWait(id));
    }

    fn cancel_waits(&mut self, id: Uuid) {
        self.pending.retain(|(_, w)| *w != Wait::PreWait(id) && *w != Wait::Next(id));
    }

    /// Seconds left of the tile's pre-wait, while it's in one.
    pub fn waiting(&self, id: Uuid) -> Option<f64> {
        self.pending_for(Wait::PreWait(id))
    }

    /// Seconds until the cue after this tile starts through its link, while waiting.
    pub fn linking(&self, id: Uuid) -> Option<f64> {
        self.pending_for(Wait::Next(id))
    }

    fn pending_for(&self, wait: Wait) -> Option<f64> {
        let now = Instant::now();
        self.pending.iter().find(|(_, w)| *w == wait).map(|(at, _)| at.saturating_duration_since(now).as_secs_f64())
    }

    /// A tile's state for replies: `waiting` during its pre-wait.
    fn state_of(&self, id: Uuid) -> &'static str {
        match self.engine.state(id) {
            PlayState::Stopped if self.waiting(id).is_some() => "waiting",
            s => state_name(s),
        }
    }

    /// Action targets from tile refs, or `set:Name` for every tile in a set.
    fn parse_targets(&self, v: &Value, who: &str) -> Result<Vec<ActionTarget>, CmdError> {
        let list: Vec<&Value> = match v {
            Value::Array(a) => a.iter().collect(),
            one => vec![one],
        };
        list.into_iter()
            .map(|r| {
                let r = r.as_str().ok_or_else(|| bad(format!("{who}: targets are tiles, or \"set:Name\"")))?;
                // "Thunder@1.5": a group's offset.
                let (r, offset) = match r.rsplit_once('@').map(|(a, b)| (a, b.trim().parse::<f64>())) {
                    Some((a, Ok(o))) if o.is_finite() && (0.0..=3600.0).contains(&o) => (a, o),
                    Some((_, Ok(_))) => return Err(bad(format!("{who}: an offset is 0–3600 seconds"))),
                    _ => (r, 0.0),
                };
                match r.strip_prefix("set:") {
                    Some(s) => Ok(ActionTarget { set: Some(self.store.library.sets[self.resolve_set(Some(s))?].id), offset, ..Default::default() }),
                    None => Ok(ActionTarget { tile: Some(self.resolve_tile(r)?), offset, ..Default::default() }),
                }
            })
            .collect()
    }

    /// The action's targets as JSON, by name.
    pub fn action_json(&self, a: &TileAction) -> Value {
        let lib = &self.store.library;
        let targets: Vec<Value> = a
            .targets
            .iter()
            .map(|t| match (t.tile, t.set) {
                (Some(id), _) => json!({"tile": id, "title": lib.tile(id).map(|t| t.title.clone()), "offset": t.offset}),
                (None, Some(sid)) => json!({"set": sid, "name": lib.set_index(sid).map(|i| lib.sets[i].name.clone()), "offset": t.offset}),
                _ => Value::Null,
            })
            .collect();
        json!({"kind": enum_name(a.kind), "fade": a.fade, "targets": targets})
    }

    /// `tile.addAction` and `tile.action`.
    fn action_commands(&mut self, id: &str, p: &P) -> Res {
        let kind = p.named::<ActionKind>("kind", "play, fade, stop, pause, play_on")?;
        let targets = p.get("targets").map(|v| self.parse_targets(v, id)).transpose()?;
        let fade = match p.f64("fade")? {
            Some(f) if !(0.0..=60.0).contains(&f) => return Err(bad(format!("{id}: fade is 0–60 seconds"))),
            f => f.map(|f| f as f32),
        };
        if id == "tile.addAction" {
            let si = self.set_param(p)?;
            let mut action = TileAction { kind: kind.unwrap_or_default(), targets: targets.unwrap_or_default(), ..Default::default() };
            if let Some(f) = fade {
                action.fade = f;
            }
            let title = match p.str("title")?.map(str::trim).filter(|s| !s.is_empty()) {
                Some(t) => t.to_owned(),
                None => {
                    let what = self.action_json(&action)["targets"][0].clone();
                    let name = what["title"].as_str().or(what["name"].as_str()).unwrap_or("…").to_owned();
                    match action.kind {
                        ActionKind::Play => format!("Group: {name}"),
                        ActionKind::PlayOn => format!("Play On {name}"),
                        k => format!("{} {name}", k.label()),
                    }
                }
            };
            let t = Tile { title, color: TileColor::Indigo, action: Some(action), ..Default::default() };
            let new_id = t.id;
            let place = p.f64("at")?.map_or(Place::Append, |a| Place::Flat(a.max(1.0) as usize - 1));
            self.store.library.sets[si].insert_tiles(place, vec![t]);
            self.store.mark_dirty();
            // Open the inspector on it, to choose its targets.
            self.selected_tile = Some(new_id);
            self.selection = vec![new_id];
            return Ok(json!({"id": new_id}));
        }
        let tid = self.tile_param(p)?;
        let t = self.store.library.tile(tid).expect("resolved");
        if t.file.is_some() {
            return Err(bad(format!("tile.action: “{}” plays audio; an action tile has none (tile.addAction makes one)", t.title)));
        }
        let mut action = t.action.clone().unwrap_or_default();
        if let Some(k) = kind {
            action.kind = k;
        }
        if let Some(ts) = targets {
            action.targets = ts;
        }
        if let Some(f) = fade {
            action.fade = f;
        }
        let clear = p.bool("clear")? == Some(true);
        self.store.library.tile_mut(tid).expect("resolved").action = (!clear).then_some(action);
        self.store.mark_dirty();
        Ok(self.tile_json(tid))
    }

    fn play_next_cue(&mut self, set_idx: usize) -> Result<Value, CmdError> {
        if self.store.library.sets[set_idx].layout == Layout::List {
            return self.play_selection(set_idx);
        }
        let set = &self.store.library.sets[set_idx];
        let from = self.cue.get(&set.id).copied().unwrap_or(0);
        // A vamping tile in the set plays on and nothing else starts (D-155): the one
        // nearest at or above the pointer, else the nearest below it.
        let under = next_cue(&set.tiles, from);
        let vamping: Vec<usize> = (0..set.tiles.len()).filter(|&i| !set.tiles[i].disabled && self.engine.vamping(set.tiles[i].id)).collect();
        let limit = under.unwrap_or(set.tiles.len());
        if let Some(i) = vamping.iter().rev().find(|&&i| i <= limit).or(vamping.first()).copied() {
            let tile = set.tiles[i].clone();
            self.engine.play_on_vamp(tile.id);
            // GO on the vamping cue itself (stepped back to it) moves the pointer past it.
            if Some(i) == under {
                self.move_pointer_past(tile.id);
            }
            return Ok(json!({"tile": tile.id, "title": tile.title, "position": i + 1, "played_on": true}));
        }
        match under {
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
            (added, errors) = self.store.import(&sounds, si, crate::rows::Place::Append);
            self.report_import(added.len(), &errors);
        }
        Ok(json!({"added": added, "errors": errors, "picture": picture, "sets": sets}))
    }

    /// Files on their way into the library, conversions first.
    fn imports_json(&self) -> Value {
        let mut list: Vec<(&String, &crate::store::Importing)> = self.store.importing().collect();
        list.sort_by_key(|(name, i)| (!i.busy(), i.file_name(), (*name).clone()));
        json!(list
            .iter()
            .map(|(name, i)| {
                json!({"file": i.file_name(), "media": name, "converting": i.converting, "downloading": i.verb() == "Downloading", "progress": i.fraction()})
            })
            .collect::<Vec<_>>())
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
        let importing = self.store.import_of(t);
        let media = match self.store.media(t) {
            None if t.file.is_some() => "not_loaded",
            None => "none",
            Some(Media::Loading) if importing.is_some_and(|i| i.verb() == "Downloading") => "downloading",
            Some(Media::Loading) if importing.is_some_and(|i| i.converting) => "converting",
            Some(Media::Loading) => "loading",
            Some(Media::Ready { .. }) => "ready",
            Some(Media::Failed(_)) => "failed",
        };
        json!({
            "id": t.id,
            "title": t.title,
            "emoji": t.emoji,
            "set": lib.sets[si].name,
            "position": ti + 1,
            "row": lib.sets[si].row_of(ti).0 + 1,
            "slot": lib.sets[si].row_of(ti).1 + 1,
            "column": lib.sets[si].row_of(ti).1 + 1,
            "key": t.hotkey.map(PadKey::name),
            "keyByHand": t.hotkey_manual,
            "armed": !self.played.contains(&t.id),
            "color": enum_name(t.color),
            "hold": t.press.hold,
            "pausable": t.press.pausable,
            "one_shot": t.press.one_shot,
            "mode": enum_name(t.press.mode()),
            "volume": t.volume,
            "muted": t.muted,
            "gain_db": t.gain_db,
            "speed": t.speed,
            "pitch": t.pitch,
            "output": t.output(),
            "sends": t.routes().iter().map(|s| json!({"output": s.output, "gain_db": s.gain_db})).collect::<Vec<_>>(),
            "channels": if t.channel_mix.is_empty() { json!("auto") } else { json!(t.channel_mix) },
            "channel_layout": self.store.audio(t).and_then(|a| a.native.as_ref().map(|n| n.labels.clone())),
            "duck": enum_name(t.duck),
            "duck_level": t.duck_db.map_or(json!("default"), |db| json!(db)),
            "loop": t.looping,
            "solo": t.solo,
            "fade_in": t.fade_in,
            "fade_out": t.fade_out,
            "stop_fade": t.stop_fade,
            "pre_wait": t.pre_wait,
            "link": enum_name(t.link),
            "post_wait": t.post_wait,
            "fade_curve": enum_name(t.fade_curve),
            "trim_start": t.start,
            "trim_end": t.end,
            "vamp": t.vamp.map(|(a, b)| json!([a, b])),
            "markers": t.markers.iter().map(|m| json!({"name": m.name, "at": m.at})).collect::<Vec<_>>(),
            "duration": t.duration,
            "audio": media,
            "import_progress": importing.and_then(|i| i.fraction()),
            "picture": t.image.is_some(),
            "picture_layer": enum_name(t.image_layer),
            "picture_fit": enum_name(t.image_fit),
            "picture_opacity": t.image_opacity,
            "state": self.state_of(t.id),
            "on_start": t.on_start,
            "on_stop": t.on_stop,
            "notes": t.notes,
            "disabled": t.disabled,
            "action": t.action.as_ref().map(|a| self.action_json(a)),
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
                    "output": self.engine.output_of(*id),
                    "outputs": self.engine.outputs_of(*id),
                    "position": s.position,
                    "remaining": s.remaining(),
                    "length": s.length,
                    "level_db": (self.level_db(*id) * 10.0).round() / 10.0,
                })
            }).collect::<Vec<_>>(),
            "master": v,
            "master_db": if v <= 0.0 { Value::Null } else { json!(gain_to_db(v)) },
            "device": self.engine.device_name,
            "device_error": self.engine.error,
            "sample_rate": self.engine.sample_rate(),
            "set": set.name,
            "layout": enum_name(set.layout),
            "next_cue": next_cue(&set.tiles, pointer).map(|i| json!({"position": i + 1, "title": set.tiles[i].title, "notes": set.tiles[i].notes})),
            "ducking": self.engine.ducking(),
            "voice": self.engine.voice(),
            "input_level_db": self.input.as_ref().map(|i| i.shared.level_db()),
            "input_note": self.input_note(),
            "input": self.input_name(),
            "hears_board": self.hears_board.hears(),
            "recording": self.recording.as_ref().map(|r| r.tile),
            "recording_seconds": self.recording.as_ref().map(|r| (r.seconds() * 10.0).round() / 10.0),
            "replay_seconds": (self.replay_seconds() * 10.0).round() / 10.0,
            "duck_hold": self.engine.duck_hold(),
            "locked": set.locked,
            "sidebar": lib.show_sidebar,
            "inspector": lib.show_inspector,
            "inspector_tab": self.inspector_tab.name(),
            "selected": self.selected_tile,
            "selection": self.selected(),
            "theme": enum_name(lib.theme),
            "theme_name": self.look.id,
            "appearance": enum_name(lib.appearance),
            "zoom": lib.zoom,
            "loading": self.store.is_loading(),
            "importing": self.imports_json(),
            "quip_new": self.quip_watch.json(lib.quip_badge),
            "window": self.has_window,
        })
    }

    // ── execute ─────────────────────────────────────────────────────────────

    /// Run one command. `params` is a JSON object (or null for none).
    pub fn execute(&mut self, id: &str, params: Value) -> Res {
        let result = self.execute_step(id, params);
        self.sync_list_selection();
        result
    }

    /// A command as one undo step (when it's one that changes the library).
    fn execute_step(&mut self, id: &str, params: Value) -> Res {
        let Some(sp) = spec(id).filter(|s| s.undo) else { return self.run(id, params) };
        // One drag or one typed title is one step; anything else starts a new one.
        let target = match id {
            "tile.set" | "tile.action" => Some(format!("{id}:{}", params.get("tile").or(params.get("tiles")).unwrap_or(&Value::Null))),
            // Typing a set's notes, or its name in its settings, is one step too.
            "set.notes" | "set.rename" | "set.crossfade" => Some(format!("{id}:{}", params.get("set").unwrap_or(&Value::Null))),
            _ => None,
        };
        let sets = self.store.library.sets.clone();
        let selected = self.store.library.selected_set;
        // Hold today's clips too, so a removal can't delete what this step will refer to.
        let mut keep = self.history.media();
        keep.extend(self.store.library.media_files());
        self.store.hold_media(keep);
        let result = self.run(id, params);
        if result.is_err() || self.store.library.sets == sets {
            let held = self.history.media();
            self.store.hold_media(held);
        }
        let result = result?;
        if self.store.library.sets != sets {
            let h = &mut self.history;
            let now = Instant::now();
            let same = matches!((&h.last, &target), (Some((t, at)), Some(k)) if t == k && now - *at < COALESCE);
            if !same || h.undo.is_empty() {
                h.undo.push(Step { label: sp.label.trim_end_matches('…'), sets, selected });
                if h.undo.len() > UNDO_LIMIT {
                    h.undo.remove(0);
                }
            }
            h.last = target.map(|t| (t, now));
            h.redo.clear();
            let held = h.media();
            self.store.hold_media(held);
        }
        Ok(result)
    }

    /// One MIDI message from the input thread: finish a `bind.learn`, or run every
    /// binding it matches (and, with auto-map on, play the tile its note maps to).
    pub fn midi(&mut self, ev: &MidiEvent) {
        if !self.store.library.midi.enabled {
            return;
        }
        if ev.on && self.learning.as_ref().is_some_and(|l| !l.key) {
            let l = self.learning.take().expect("checked");
            let source = Source::Midi(MidiSource::from_event(ev));
            // A fader learned for a command that takes a level fills it.
            let value = l.value.or_else(|| matches!(ev.kind, MidiKind::Cc(_)).then(|| fader_param(&l.command)).flatten());
            self.learn_binding(Binding { source, command: l.command, params: l.params, value, ..Default::default() });
            return;
        }
        let edge = match ev.kind {
            MidiKind::Cc(c) => self.cc_state.insert((ev.port.clone(), ev.channel, c), ev.on) != Some(ev.on),
            _ => true,
        };
        let level = ev.value as f32 / 127.0;
        let matched: Vec<Binding> = self
            .store
            .library
            .bindings
            .iter()
            .filter(|b| b.enabled && matches!(&b.source, Source::Midi(m) if m.matches(ev)))
            .cloned()
            .collect();
        for b in &matched {
            let fire = match (ev.kind, &b.value) {
                (MidiKind::Cc(_), Some(_)) => Fire::Value(level),
                (MidiKind::Note(_), Some(_)) if ev.on => Fire::Value(level),
                (MidiKind::Note(_), Some(_)) => continue,
                (MidiKind::Cc(_), None) if !edge => continue,
                (MidiKind::Program(_), _) => Fire::Press,
                _ if ev.on => Fire::Press,
                _ => Fire::Release,
            };
            if let Some((cmd, params)) = b.invocation(fire) {
                self.run_binding(&cmd, params);
            }
        }
        if let (true, Some(base), MidiKind::Note(n)) = (matched.is_empty(), self.store.library.midi.auto_map, ev.kind) {
            let k = n.wrapping_sub(base) as usize;
            let set = self.current_set();
            let (first, count) = self.auto_pads(set);
            if n >= base && k < count && first + k < set.tiles.len() {
                let event = if ev.on { "press" } else { "release" };
                self.run_binding("tile.trigger", json!({"tile": format!("#{}", first + k + 1), "event": event}));
            }
        }
    }

    /// A system-wide key went down or up: run the key bindings for that combination.
    pub fn hotkey(&mut self, id: u32, pressed: bool) {
        let matched: Vec<Binding> = self
            .store
            .library
            .bindings
            .iter()
            .filter(|b| b.enabled && matches!(&b.source, Source::Key(k) if crate::hotkeys::id_of(k) == Some(id)))
            .cloned()
            .collect();
        for b in matched {
            if let Some((cmd, params)) = b.invocation(if pressed { Fire::Press } else { Fire::Release }) {
                self.run_binding(&cmd, params);
            }
        }
    }

    /// The desktop's global-shortcuts portal says a shortcut (a key binding's id) went
    /// down or up (Wayland, src/portal.rs).
    pub fn portal_key(&mut self, id: &str, pressed: bool) {
        let found = self.store.library.bindings.iter().find(|b| b.enabled && b.id.to_string() == id && matches!(b.source, Source::Key(_))).cloned();
        if let Some((cmd, params)) = found.and_then(|b| b.invocation(if pressed { Fire::Press } else { Fire::Release })) {
            self.run_binding(&cmd, params);
        }
    }

    /// The window caught the key combination a `bind.learn {from: key}` was waiting for.
    pub fn learn_key(&mut self, keys: &str) {
        if !self.learning.as_ref().is_some_and(|l| l.key) {
            return;
        }
        if let Err(e) = crate::hotkeys::parse(keys) {
            self.notify_error(format!("Can't use {e}"));
            return;
        }
        let l = self.learning.take().expect("checked");
        self.learn_binding(Binding { source: Source::Key(keys.to_owned()), command: l.command, params: l.params, value: None, ..Default::default() });
    }

    /// One OSC message from the listener: a built-in `/ls/...` address, or a binding.
    pub fn osc(&mut self, m: &OscMessage, _from: Option<std::net::SocketAddr>) {
        // TouchOSC-style buttons send 1 on press and 0 on release; no argument is a press.
        let on = |i: usize| m.num(i).is_none_or(|v| v >= 0.5);
        let event = |i: usize| if on(i) { "press" } else { "release" };
        let parts: Vec<&str> = m.addr.trim_start_matches('/').split('/').collect();
        let tile = m.str(0).map(str::to_owned);
        let builtin: Option<(String, Value)> = match parts.as_slice() {
            ["ls", "tile", n, "trigger"] => n.parse::<usize>().ok().map(|n| ("tile.trigger".into(), json!({"tile": format!("#{n}"), "event": event(0)}))),
            ["ls", "key", k] => Some(("tile.trigger".into(), json!({"tile": format!("key:{k}"), "event": event(0)}))),
            ["ls", "trigger"] => tile.map(|t| ("tile.trigger".into(), json!({"tile": t, "event": event(1)}))),
            ["ls", verb @ ("play" | "stop")] => tile.map(|t| (format!("tile.{verb}"), json!({"tile": t}))),
            // /ls/release is Play On's old name (D-156).
            ["ls", "playon" | "release"] => tile.map(|t| ("tile.playOn".into(), json!({"tile": t}))),
            ["ls", "jump"] => tile.map(|t| ("tile.jump".into(), json!({"tile": t, "marker": m.str(1).unwrap_or("#1")}))),
            ["ls", "go"] if on(0) => Some(("cue.go".into(), json!({}))),
            ["ls", "go"] => return,
            ["ls", "stopall"] => Some(("transport.stopAll".into(), json!({"immediate": m.num(0).is_some_and(|v| v >= 0.5)}))),
            ["ls", "master"] => m.num(0).map(|v| ("master.volume".into(), json!({"volume": v.clamp(0.0, 1.0)}))),
            ["ls", "duck"] => Some(("duck.hold".into(), json!({"on": on(0)}))),
            ["ls", "resync"] => {
                if let Some(f) = &mut self.osc_feedback {
                    f.resync();
                }
                self.send_osc_feedback();
                return;
            }
            ["ls", "cmd"] => m.str(0).map(|id| {
                let params = m.str(1).and_then(|js| serde_json::from_str(js).ok()).unwrap_or_else(|| json!({}));
                (id.to_owned(), params)
            }),
            _ => None,
        };
        if let Some((id, params)) = builtin {
            self.run_osc(&id, params);
            return;
        }
        let matched: Vec<Binding> = self
            .store
            .library
            .bindings
            .iter()
            .filter(|b| b.enabled && b.source == Source::Osc(m.addr.clone()))
            .cloned()
            .collect();
        if matched.is_empty() && m.addr.starts_with("/ls/") {
            self.notify_error(format!("OSC: no such address {}", m.addr));
        }
        for b in matched {
            let fire = match (&b.value, m.num(0)) {
                (Some(_), Some(v)) => Fire::Value(v.clamp(0.0, 1.0)),
                _ if on(0) => Fire::Press,
                _ => Fire::Release,
            };
            if let Some((cmd, params)) = b.invocation(fire) {
                self.run_osc(&cmd, params);
            }
        }
    }

    fn run_osc(&mut self, id: &str, params: Value) {
        // A remote doesn't get to point the app's own messages somewhere else.
        let sets_messages = id == "tile.set" && (params.get("onStart").is_some() || params.get("onStop").is_some());
        if crate::osc::refused(id) || sets_messages {
            self.notify_error(format!("OSC can't run {id}"));
            return;
        }
        self.run_binding(id, params);
    }

    fn run_binding(&mut self, cmd: &str, params: Value) {
        if let Err(e) = self.execute(cmd, params) {
            self.report(&e);
        }
    }

    /// Store a learned binding, replacing any other binding of the same source to the
    /// same command and parameters (learning a pad again moves it).
    fn learn_binding(&mut self, b: Binding) {
        let what = b.source.describe();
        let target = self.describe_invocation(&b.command, &b.params);
        let bindings = &mut self.store.library.bindings;
        bindings.retain(|x| !(x.source == b.source && x.command == b.command && x.params == b.params));
        bindings.push(b);
        self.store.mark_dirty();
        self.notify(format!("{what} now {target}"));
    }

    /// "plays Air Horn", "runs Stop All": for notices.
    fn describe_invocation(&self, cmd: &str, params: &Map<String, Value>) -> String {
        let tile = params.get("tile").and_then(|v| v.as_str()).and_then(|r| self.resolve_tile(r).ok()).and_then(|id| self.store.library.tile(id));
        match (cmd, tile) {
            ("tile.trigger" | "tile.play", Some(t)) => format!("plays {}", t.title),
            (_, Some(t)) => format!("runs {} on {}", spec(cmd).map_or(cmd, |s| s.label), t.title),
            _ => format!("runs {}", spec(cmd).map_or(cmd, |s| s.label.trim_end_matches('…'))),
        }
    }

    /// The bindings that act on one tile (the inspector's MIDI row).
    pub fn tile_bindings(&self, id: Uuid) -> Vec<&Binding> {
        let key = id.to_string();
        self.store.library.bindings.iter().filter(|b| b.params.get("tile").and_then(|v| v.as_str()) == Some(key.as_str())).collect()
    }

    fn binding_commands(&mut self, id: &str, p: &P) -> Res {
        match id {
            "bind.list" => Ok(Value::Array(self.store.library.bindings.iter().map(Binding::to_json).collect())),
            "bind.add" => {
                let source = Source::parse(p.get("source").ok_or_else(|| bad("bind.add: source is required"))?).map_err(bad)?;
                if let Source::Key(k) = &source {
                    crate::hotkeys::parse(k).map_err(|e| bad(format!("bind.add: {e}")))?;
                }
                let (command, params, value) = self.binding_target(p, "bind.add")?;
                let b = Binding { source, command, params, value, ..Default::default() };
                let out = b.to_json();
                self.store.library.bindings.push(b);
                self.store.mark_dirty();
                Ok(out)
            }
            "bind.remove" => {
                let before = self.store.library.bindings.len();
                if p.bool("all")? == Some(true) {
                    self.store.library.bindings.clear();
                } else if let Some(bid) = p.str("id")? {
                    self.store.library.bindings.retain(|b| b.id.to_string() != bid);
                } else if p.has("tile") {
                    let key = self.tile_param(p)?.to_string();
                    self.store.library.bindings.retain(|b| b.params.get("tile").and_then(|v| v.as_str()) != Some(key.as_str()));
                } else if let Some(src) = p.get("source") {
                    let source = Source::parse(src).map_err(bad)?;
                    self.store.library.bindings.retain(|b| b.source != source);
                } else {
                    return Err(bad("bind.remove: give id, tile, source or all"));
                }
                let removed = before - self.store.library.bindings.len();
                if removed > 0 {
                    self.store.mark_dirty();
                }
                Ok(json!({"removed": removed}))
            }
            "bind.enable" => {
                let bid = p.need_str("id")?;
                let b = self.store.library.bindings.iter_mut().find(|b| b.id.to_string() == bid).ok_or_else(|| CmdError::new("not_found", format!("no binding {bid}")))?;
                b.enabled = p.bool("on")?.unwrap_or(!b.enabled);
                let out = b.to_json();
                self.store.mark_dirty();
                Ok(out)
            }
            "bind.learn" => {
                if p.bool("cancel")? == Some(true) {
                    self.learning = None;
                    return Ok(json!({"learning": false}));
                }
                let key = match p.str("from")? {
                    None | Some("midi") => false,
                    Some("key") => {
                        self.needs_window("bind.learn", "a key pressed in the window")?;
                        true
                    }
                    Some(f) => return Err(bad(format!("bind.learn: from must be midi or key (got {f:?})"))),
                };
                let (command, params, value) = self.binding_target(p, "bind.learn")?;
                let target = self.describe_invocation(&command, &params);
                let will = target.replacen("plays", "play", 1).replacen("runs", "run", 1);
                self.learning = Some(Learn { key, command, params, value, until: Instant::now() + LEARN_FOR });
                self.notify(if key {
                    format!("Press a key combination (with Ctrl, Alt, Shift or Cmd, or an F-key): it will {will} from any app")
                } else {
                    format!("Press a pad or move a control on your MIDI controller: it will {will}")
                });
                Ok(json!({"learning": true, "from": if key { "key" } else { "midi" }, "for": target}))
            }
            "osc.enable" => {
                let o = &mut self.store.library.osc;
                o.enabled = p.bool("on")?.unwrap_or(true);
                if let Some(port) = p.f64("port")? {
                    if !(1024.0..=65535.0).contains(&port) {
                        return Err(bad("osc.enable: port is 1024–65535"));
                    }
                    o.port = port as u16;
                }
                if let Some(lan) = p.bool("lan")? {
                    o.lan = lan;
                }
                if p.has("feedback") {
                    o.feedback = p.str("feedback")?.filter(|s| !s.is_empty()).map(str::to_owned);
                    if let Some(f) = &o.feedback {
                        if crate::osc::feedback_addr(f).is_none() {
                            return Err(bad(format!("osc.enable: feedback {f:?} isn't host:port")));
                        }
                    }
                }
                if let Some(levels) = p.bool("levels")? {
                    o.levels = levels;
                }
                let out = json!({"on": o.enabled, "port": o.port, "lan": o.lan, "feedback": o.feedback, "levels": o.levels});
                self.store.mark_dirty();
                Ok(out)
            }
            "osc.status" => Ok(json!({
                "enabled": self.store.library.osc.enabled,
                "listening": self.osc_status.listening,
                "error": self.osc_status.error,
                "received": self.osc_status.received,
                "lan": self.store.library.osc.lan,
                "feedback": self.store.library.osc.feedback,
                "levels": self.store.library.osc.levels,
            })),
            "hotkey.status" => Ok(json!({
                "available": self.hotkey_status.available,
                "note": self.hotkey_status.note,
                "active": self.hotkey_status.active,
                "failed": self.hotkey_status.failed.iter().map(|(k, e)| json!({"keys": k, "error": e})).collect::<Vec<_>>(),
            })),
            "midi.ports" => Ok(json!({
                "enabled": self.store.library.midi.enabled,
                "ports": self.midi_status.ports,
                "connected": self.midi_status.connected,
                "error": self.midi_status.error,
                "auto_map": self.store.library.midi.auto_map,
                "bank": self.store.library.midi.bank,
                "colors": self.store.library.midi.palette.as_ref().map(|p| &p.name),
            })),
            "midi.enable" => {
                let m = &mut self.store.library.midi;
                m.enabled = p.bool("on")?.unwrap_or(true);
                if p.has("ports") {
                    m.ports = match p.get("ports").unwrap_or(&Value::Null) {
                        Value::Null => Vec::new(),
                        Value::String(s) => s.split(',').map(|s| s.trim().to_owned()).filter(|s| !s.is_empty()).collect(),
                        Value::Array(a) => a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect(),
                        _ => return Err(bad("midi.enable: ports is a list of names (or null for every port)")),
                    };
                }
                self.store.mark_dirty();
                ok()
            }
            "midi.feedback" => {
                let m = &mut self.store.library.midi;
                m.feedback = p.bool("on")?.unwrap_or(true);
                let vel = |k: &str| -> Result<Option<u8>, CmdError> {
                    match p.f64(k)? {
                        Some(v) if (0.0..=127.0).contains(&v) => Ok(Some(v as u8)),
                        Some(_) => Err(bad(format!("midi.feedback: {k} is a velocity, 0-127"))),
                        None => Ok(None),
                    }
                };
                if let Some(v) = vel("lit")? {
                    m.lit = v;
                }
                if let Some(v) = vel("unlit")? {
                    m.unlit = v;
                }
                if let Some(c) = p.f64("channel")? {
                    if !(1.0..=16.0).contains(&c) {
                        return Err(bad("midi.feedback: channel is 1-16"));
                    }
                    m.feedback_channel = c as u8;
                }
                if p.has("colors") {
                    let names = || crate::pads::builtin().iter().map(|p| p.name.clone()).collect::<Vec<_>>().join(", ");
                    m.palette = match p.get("colors") {
                        None => None,
                        Some(Value::String(s)) if s.is_empty() || s == "off" => None,
                        Some(Value::String(s)) if s.trim_start().starts_with('{') => Some(palette_table(s)?),
                        Some(Value::String(s)) => Some(crate::pads::find(s).ok_or_else(|| bad(format!("midi.feedback: no pad colours called {s:?} ({}, off, or a table)", names())))?),
                        Some(v) => Some(palette_table(&v.to_string())?),
                    };
                }
                let out = json!({"on": m.feedback, "lit": m.lit, "unlit": m.unlit, "channel": m.feedback_channel, "colors": m.palette.as_ref().map(|p| &p.name)});
                self.store.mark_dirty();
                self.leds_sent = None;
                Ok(out)
            }
            "midi.autoMap" => {
                // Without `base` (a bank change alone) the mapping stays as it is.
                let base = match p.m.get("base") {
                    None if p.has("bank") => self.store.library.midi.auto_map,
                    None | Some(Value::Null) => None,
                    Some(Value::String(s)) if s == "off" => None,
                    _ => match p.f64("base")? {
                        Some(n) if (0.0..=127.0).contains(&n) => Some(n as u8),
                        _ => return Err(bad("midi.autoMap: base is a note number 0–127, or null/off")),
                    },
                };
                if let Some(b) = p.f64("bank")? {
                    if !(0.0..=128.0).contains(&b) {
                        return Err(bad("midi.autoMap: bank is the pads per page, 0 (no pages) to 128"));
                    }
                    self.store.library.midi.bank = b as u8;
                }
                self.store.library.midi.auto_map = base;
                self.store.mark_dirty();
                Ok(json!({"auto_map": base, "bank": self.store.library.midi.bank}))
            }
            "midi.page" | "midi.pageUp" | "midi.pageDown" => {
                let bank = self.store.library.midi.bank;
                let set = self.current_set();
                let (set_id, pages) = (set.id, crate::pads::pages(set.tiles.len(), bank));
                let now = self.pad_paging.page(set_id, pages);
                let want = match id {
                    "midi.pageUp" => now + 1,
                    "midi.pageDown" => now.saturating_sub(1),
                    _ => match (p.f64("page")?, p.f64("delta")?) {
                        (Some(n), _) if n >= 1.0 => n as usize - 1,
                        (Some(_), _) => return Err(bad("midi.page: page counts from 1")),
                        (None, Some(d)) => (now as i64 + d as i64).max(0) as usize,
                        (None, None) => now,
                    },
                };
                let page = self.pad_paging.set_page(set_id, want, pages);
                Ok(json!({"page": page + 1, "pages": pages, "bank": bank}))
            }
            other => unreachable!("{other} isn't a binding command"),
        }
    }

    /// The command a binding runs: `command`, `params` (an object; a tile ref in it is
    /// stored as the tile's id, so renaming the tile keeps the binding) and `value`.
    fn binding_target(&self, p: &P, who: &str) -> Result<Target, CmdError> {
        let command = p.str("command")?.unwrap_or("tile.trigger").to_owned();
        let s = spec(&command).ok_or_else(|| bad(format!("{who}: no command {command:?}")))?;
        if command.starts_with("bind.") || command == "app.quit" {
            return Err(bad(format!("{who}: {command} can't be bound")));
        }
        let mut params = match p.get("params") {
            None | Some(Value::Null) => Map::new(),
            Some(Value::Object(m)) => m.clone(),
            Some(Value::String(js)) => match serde_json::from_str(js) {
                Ok(Value::Object(m)) => m,
                _ => return Err(bad(format!("{who}: params must be a JSON object"))),
            },
            Some(_) => return Err(bad(format!("{who}: params must be a JSON object"))),
        };
        // The tile can also be given directly: `bind.learn "Air Horn"`.
        if let Some(t) = p.str("tile")? {
            params.insert("tile".into(), json!(t));
        }
        if let Some(r) = params.get("tile").and_then(|v| v.as_str()).map(str::to_owned) {
            params.insert("tile".into(), json!(self.resolve_tile(&r)?.to_string()));
        }
        let tile_command = matches!(
            command.as_str(),
            "tile.trigger" | "tile.play" | "tile.stop" | "tile.pause" | "tile.resume" | "tile.playOn" | "tile.release" | "tile.preview" | "tile.enable" | "tile.disable"
        );
        if tile_command && !params.contains_key("tile") {
            return Err(bad(format!("{who}: {} needs a tile", s.id)));
        }
        let value = p.str("value")?.map(str::to_owned);
        Ok((command, params, value))
    }

    /// `record.start`: record from the input into `tile`, or into a new tile.
    fn start_recording(&mut self, p: &P) -> Res {
        if let Some(r) = &self.recording {
            let title = self.store.library.tile(r.tile).map(|t| t.title.clone()).unwrap_or_default();
            return Err(CmdError::new("busy", format!("Already recording into {title}")));
        }
        if self.engine.device_name.is_none() {
            return Err(CmdError::new("no_audio", "Recording needs the audio system, and there's no audio device"));
        }
        let (tile, made) = if p.has("tile") {
            (self.tile_param(p)?, false)
        } else {
            let si = self.set_param(p)?;
            (self.recording_tile(si, p.str("title")?), true)
        };
        self.recording = Some(Recording { tile, made, data: Vec::new(), rate: 48_000, started: Instant::now() });
        self.show_recording(tile, made);
        self.input_tried = None;
        self.keep_input();
        let title = self.store.library.tile(tile).map(|t| t.title.clone()).unwrap_or_default();
        if self.input.is_some() {
            self.recording_started();
        } else if !self.input_waiting() {
            let why = self.input_error.clone().unwrap_or_else(|| "the input didn't open".into());
            self.recording = None;
            if made {
                self.store.remove_tile(tile);
            }
            return Err(CmdError::new("failed", why));
        }
        // While the input opens on its own thread, the recording waits for it: it starts
        // (or is dropped, if the input can't open) in `poll_input`.
        Ok(json!({"tile": tile, "title": title, "recording": true, "waiting": self.input_waiting()}))
    }

    /// A new tile for a recording, at the end of set `si`: "Recording N" unless `title`.
    fn recording_tile(&mut self, si: usize, title: Option<&str>) -> Uuid {
        let set = &mut self.store.library.sets[si];
        let n = set.tiles.iter().filter(|t| t.title.starts_with("Recording")).count() + 1;
        let title = title.map(str::to_owned).unwrap_or_else(|| format!("Recording {n}"));
        let t = Tile { title, color: TileColor::Red, ..Default::default() };
        let id = t.id;
        set.tiles.push(t);
        self.store.mark_dirty();
        id
    }

    /// Put a recording where it can be seen and stopped (t-m20-record-ux, D-127): the
    /// board scrolls to its tile, and a new tile opens in the inspector, whose Stop
    /// Recording is at the top.
    fn show_recording(&mut self, tile: Uuid, made: bool) {
        self.effects.push(Effect::Reveal(tile));
        if made {
            self.selected_tile = Some(tile);
            self.selection = vec![tile];
            if self.editor.is_some() {
                self.close_editor();
            }
        }
    }

    /// Screenshots and demos (the `record:SECONDS` script step): a recording that only
    /// shows, `seconds` into a new tile, from a stand-in input. No input opens, and
    /// stopping it records nothing.
    pub fn stage_recording(&mut self, seconds: f64) {
        if self.recording.is_some() {
            return;
        }
        let tile = self.recording_tile(self.set_idx(), None);
        let data = vec![0.0; (seconds * 8000.0) as usize * 2];
        self.recording = Some(Recording { tile, made: true, data, rate: 8000, started: Instant::now() });
        self.staged_input = Some("Built-in Microphone".into());
        self.show_recording(tile, true);
    }

    /// The input opened for the recording: record at its rate, and say so.
    fn recording_started(&mut self) {
        let (Some(input), Some(r)) = (&self.input, &mut self.recording) else { return };
        r.rate = input.rate;
        let (tile, device) = (r.tile, input.device.clone());
        let title = self.store.library.tile(tile).map(|t| t.title.clone()).unwrap_or_default();
        self.notify(format!("Recording from {device} into {title}"));
    }

    /// `record.stop`: put what was recorded into the tile.
    fn finish_recording(&mut self) -> Res {
        self.pump_capture();
        let Some(mut r) = self.recording.take() else {
            return Err(CmdError::new("not_recording", "Nothing is recording"));
        };
        if self.staged_input.take().is_some() {
            r.data.clear();
        }
        self.keep_input();
        let title = self.store.library.tile(r.tile).map(|t| t.title.clone()).unwrap_or_default();
        if r.data.len() < 2 * (r.rate as usize / 20) {
            if r.made {
                self.store.remove_tile(r.tile);
            }
            return Err(CmdError::new("failed", "Nothing was recorded (no sound reached the input)"));
        }
        let seconds = r.seconds();
        let audio = crate::decode::DecodedAudio { sample_rate: r.rate, data: std::mem::take(&mut r.data), art: None, native: None };
        self.store.attach_recording(r.tile, audio).map_err(|e| CmdError::new("failed", format!("{e:#}")))?;
        self.notify(format!("Recorded {} into {title}", crate::logic::format_clock(seconds, true)));
        Ok(json!({"tile": r.tile, "title": title, "seconds": (seconds * 10.0).round() / 10.0}))
    }

    /// For tests: feed a recording as if the input had captured it.
    #[cfg(test)]
    fn fake_recording(&mut self, tile: Uuid, made: bool, data: Vec<f32>) {
        self.recording = Some(Recording { tile, made, data, rate: 8000, started: Instant::now() });
    }

    /// For tests: a tap the test feeds instead of an input.
    #[cfg(test)]
    fn fake_capture(&mut self, rate: u32, channels: usize) -> std::sync::mpsc::Sender<Vec<f32>> {
        let (tx, rx) = std::sync::mpsc::channel();
        self.capture = Some(Capture { rx, rate, channels });
        tx
    }

    /// `edit.undo` / `edit.redo`: put back the sets from the top of one stack, saving the
    /// current ones on the other.
    fn step_history(&mut self, redo: bool) -> Res {
        let h = &mut self.history;
        let (from, to) = if redo { (&mut h.redo, &mut h.undo) } else { (&mut h.undo, &mut h.redo) };
        let Some(step) = from.pop() else {
            return Err(CmdError::new("nothing_to_undo", if redo { "Nothing to redo" } else { "Nothing to undo" }));
        };
        to.push(Step { label: step.label, sets: self.store.library.sets.clone(), selected: self.store.library.selected_set });
        h.last = None;
        // Hold today's clips (now in the step just pushed) and the restored step's own
        // before the restore lets go of anything.
        let mut held = self.history.media();
        held.extend(step.sets.iter().flat_map(|s| &s.tiles).flat_map(|t| t.file.iter().chain(&t.image).cloned()));
        self.store.hold_media(held);
        self.store.restore_sets(step.sets, step.selected);
        // Tiles the step takes away stop, and leave the inspector.
        for id in self.engine.statuses().into_keys() {
            if self.store.library.tile(id).is_none() {
                self.engine.stop(id, 0.1);
            }
        }
        if self.selected_tile.is_some_and(|id| self.store.library.tile(id).is_none()) {
            self.selected_tile = None;
        }
        let held = self.history.media();
        self.store.hold_media(held);
        let verb = if redo { "Redid" } else { "Undid" };
        self.notify(format!("{verb} {}", step.label));
        Ok(json!({ if redo { "redid" } else { "undid" }: step.label }))
    }

    fn run(&mut self, id: &str, params: Value) -> Res {
        if spec(id).is_none() {
            return Err(CmdError::new("unknown_command", format!("no command {id:?}; `{} commands` lists them", crate::brand::CLI_NAME)));
        }
        let p = P::new(id, &params)?;
        if let Some(si) = self.locked_target(id, &p) {
            let name = &self.store.library.sets[si].name;
            return Err(CmdError::new("locked", format!("“{name}” is locked: unlock it first (set.unlock, or the lock in its settings)")));
        }
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
                self.trigger(id, event, p.bool("toggle")?.unwrap_or(false))?;
                Ok(json!({"tile": id, "state": self.state_of(id)}))
            }
            "tile.play" => {
                let id = self.tile_param(&p)?;
                refuse_disabled(self.store.library.tile(id).expect("resolved"))?;
                let state = self.engine.state(id);
                let restart = p.bool("restart")?.unwrap_or(false);
                if restart || !(matches!(state, PlayState::Playing | PlayState::Paused) || self.waiting(id).is_some()) {
                    let tile = self.store.library.tile(id).cloned().expect("resolved");
                    self.start(&tile)?;
                } else if state == PlayState::Paused {
                    self.engine.resume(id);
                }
                Ok(json!({"tile": id, "state": self.state_of(id)}))
            }
            "tile.stop" => {
                let id = self.tile_param(&p)?;
                let fade = match p.f64("fade")? {
                    Some(f) => f.max(0.0) as f32,
                    None => self.store.library.tile(id).map_or(0.0, |t| t.stop_fade),
                };
                self.stop_tile(id, fade);
                Ok(json!({"tile": id, "state": self.state_of(id)}))
            }
            "tile.jump" => {
                let id = self.tile_param(&p)?;
                let t = self.store.library.tile(id).cloned().expect("resolved");
                let m = find_marker(&t, p.need_str("marker")?)?;
                if !self.engine.jump(id, m.at) {
                    // Not sounding: start it, then move to the marker.
                    self.execute("tile.play", json!({"tile": id}))?;
                    self.engine.jump(id, m.at);
                }
                Ok(json!({"tile": id, "marker": m.name, "at": m.at}))
            }
            "tile.markerAdd" => {
                let id = self.tile_param(&p)?;
                let mut t = self.store.library.tile(id).cloned().expect("resolved");
                let at = match p.f64("at")? {
                    Some(a) => a,
                    // Live: where it's playing now.
                    None => match self.engine.statuses().get(&id) {
                        Some(st) => st.from + st.position * st.speed,
                        None => return Err(bad("tile.markerAdd: give at (seconds), or add it while the tile plays")),
                    },
                };
                if at < 0.0 || t.duration.is_some_and(|d| at > d) {
                    return Err(bad("tile.markerAdd: at is outside the audio"));
                }
                let name = p.str("name")?.map(str::to_owned).unwrap_or_else(|| format!("Marker {}", t.markers.len() + 1));
                if t.markers.iter().any(|m| m.name.eq_ignore_ascii_case(&name)) {
                    return Err(bad(format!("tile.markerAdd: {name:?} is taken")));
                }
                t.markers.push(Marker { name: name.clone(), at });
                t.markers.sort_by(|a, b| a.at.total_cmp(&b.at));
                if let Some(slot) = self.store.library.tile_mut(id) {
                    *slot = t;
                }
                self.store.mark_dirty();
                Ok(self.tile_json(id))
            }
            "tile.markerRemove" => {
                let id = self.tile_param(&p)?;
                let mut t = self.store.library.tile(id).cloned().expect("resolved");
                if p.bool("all")? == Some(true) {
                    t.markers.clear();
                } else {
                    let name = find_marker(&t, p.need_str("marker")?)?.name;
                    t.markers.retain(|m| m.name != name);
                }
                if let Some(slot) = self.store.library.tile_mut(id) {
                    *slot = t;
                }
                self.store.mark_dirty();
                Ok(self.tile_json(id))
            }
            "tile.markerMove" => {
                let id = self.tile_param(&p)?;
                let mut t = self.store.library.tile(id).cloned().expect("resolved");
                let old = find_marker(&t, p.need_str("marker")?)?.name;
                let at = p.f64("at")?;
                if at.is_some_and(|a| !a.is_finite() || a < 0.0 || t.duration.is_some_and(|d| a > d)) {
                    return Err(bad("tile.markerMove: at is outside the audio"));
                }
                let name = p.str("name")?.map(|n| n.trim().to_owned());
                if let Some(n) = &name {
                    if n.is_empty() {
                        return Err(bad("tile.markerMove: name can't be empty"));
                    }
                    if !n.eq_ignore_ascii_case(&old) && t.markers.iter().any(|m| m.name.eq_ignore_ascii_case(n)) {
                        return Err(bad(format!("tile.markerMove: {n:?} is taken")));
                    }
                }
                let m = t.markers.iter_mut().find(|m| m.name == old).expect("found");
                if let Some(a) = at {
                    m.at = a;
                }
                if let Some(n) = name {
                    m.name = n;
                }
                t.markers.sort_by(|a, b| a.at.total_cmp(&b.at));
                if let Some(slot) = self.store.library.tile_mut(id) {
                    *slot = t;
                }
                self.store.mark_dirty();
                Ok(self.tile_json(id))
            }
            // tile.release is Play On's old name (D-156).
            "tile.playOn" | "tile.release" => {
                let id = self.tile_param(&p)?;
                let played_on = self.engine.play_on_vamp(id);
                Ok(json!({"tile": id, "played_on": played_on, "released": played_on}))
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
            "tile.random" => self.play_random(&p),
            "tile.preview" => {
                let id = self.tile_param(&p)?;
                let (from, to) = (p.f64("from")?, p.f64("to")?);
                if self.engine.state(id) == PlayState::Stopped {
                    let mut tile = self.store.library.tile(id).cloned().expect("resolved");
                    if from.is_some() || to.is_some() {
                        // A stretch (the editor's Play Selection, D-104): just it, once.
                        let a = from.unwrap_or(tile.start).max(0.0);
                        let b = to.or(tile.end).or(tile.duration).unwrap_or(f64::MAX);
                        if b <= a {
                            return Err(bad("tile.preview: from must be before to"));
                        }
                        tile.start = a;
                        tile.end = Some(b);
                        tile.vamp = None;
                        tile.looping = false;
                        tile.solo = false;
                        tile.fade_in = 0.0;
                        tile.fade_out = 0.0;
                        tile.link = CueLink::None;
                    }
                    self.start_as(&tile, true)?;
                    self.messages.previews.insert(id);
                } else {
                    self.engine.stop(id, 0.05);
                }
                let mut out = json!({"tile": id, "state": state_name(self.engine.state(id))});
                if let (Some(a), Some(b)) = (from, to) {
                    out["from"] = json!(a);
                    out["to"] = json!(b);
                }
                Ok(out)
            }
            "transport.stopAll" => {
                // Two-stage (D-067): a plain Stop All while the last one is still fading
                // stops at once.
                let now = Instant::now();
                let second = !p.has("immediate") && !p.has("fade") && self.stop_all_fading.is_some_and(|until| now < until);
                let fade = if second || p.bool("immediate")?.unwrap_or(false) {
                    0.0
                } else {
                    p.f64("fade")?.map_or(self.store.library.stop_all_fade, |f| f.max(0.0) as f32)
                };
                self.engine.stop_all(fade);
                self.announce(if fade > 0.0 { "Stop All: fading out" } else { "Stop All: stopped" });
                self.stop_all_fading = (fade > 0.0).then(|| now + Duration::from_secs_f32(fade));
                self.pending.clear();
                self.effects.push(Effect::ReleaseHeld);
                Ok(json!({"fade": fade, "second": second}))
            }
            "cue.go" => {
                // With a tile: select it first (a double-click on a cue row).
                let si = match p.str("tile")? {
                    Some(r) => {
                        let tid = self.resolve_tile(r)?;
                        let (si, ti) = self.store.library.locate(tid).expect("resolved");
                        self.cue.insert(self.store.library.sets[si].id, ti);
                        si
                    }
                    None => self.set_param(&p)?,
                };
                self.play_next_cue(si)
            }
            "cue.set" | "cue.top" => {
                // A tile picks its own set and position: the cue list's select-without-play.
                let (si, index) = match (id, p.str("tile")?) {
                    ("cue.set", Some(r)) => {
                        let (si, ti) = self.store.library.locate(self.resolve_tile(r)?).expect("resolved");
                        (si, ti + 1)
                    }
                    _ => (self.set_param(&p)?, if id == "cue.top" { 1 } else { p.need_f64("index")? as usize }),
                };
                let set = &self.store.library.sets[si];
                if !(1..=set.tiles.len() + 1).contains(&index) {
                    return Err(bad(format!("cue.set: index must be 1–{} in “{}”", set.tiles.len() + 1, set.name)));
                }
                self.cue.insert(set.id, index - 1);
                Ok(json!({"set": set.name, "next": index}))
            }
            "cue.next" | "cue.prev" => {
                let si = self.set_param(&p)?;
                let set = &self.store.library.sets[si];
                let pointer = self.cue.get(&set.id).copied().unwrap_or(0);
                // A List set moves the selection one row, played or not, and stays at the
                // ends (D-197); a grid set's pointer goes past the next cue as before.
                let moved = match id {
                    "cue.next" if set.layout == Layout::List => next_cue(&set.tiles, pointer + 1).unwrap_or(pointer),
                    "cue.next" => pointer_after_next(&set.tiles, pointer),
                    _ => pointer_before_next(&set.tiles, pointer),
                };
                self.cue.insert(set.id, moved);
                let next = next_cue(&set.tiles, moved);
                Ok(json!({"set": set.name, "next": next.map(|i| i + 1), "title": next.map(|i| set.tiles[i].title.clone())}))
            }

            "cue.reset" => {
                // Reset List: every cue armed again, the first one selected; nothing stops.
                let si = self.set_param(&p)?;
                let set = &self.store.library.sets[si];
                for t in &set.tiles {
                    self.played.remove(&t.id);
                }
                self.cue.insert(set.id, 0);
                let next = next_cue(&set.tiles, 0);
                let armed = set.tiles.iter().filter(|t| crate::logic::is_cue(t)).count();
                let out = json!({"set": set.name, "next": next.map(|i| i + 1), "title": next.map(|i| set.tiles[i].title.clone())});
                self.announce(format!("List reset: {armed} cues armed"));
                Ok(out)
            }
            "cue.arm" => {
                let tid = self.resolve_tile(p.need_str("tile")?)?;
                let armed = p.bool("armed")?.unwrap_or(self.played.contains(&tid));
                if armed {
                    self.played.remove(&tid);
                } else {
                    // The selection stays where it is (D-197).
                    self.played.insert(tid);
                }
                let title = self.store.library.tile(tid).map(|t| t.title.clone()).unwrap_or_default();
                self.announce(format!("{title}: {}", if armed { "armed" } else { "played" }));
                Ok(json!({"tile": tid, "title": title, "armed": armed}))
            }
            "transport.pauseAll" => {
                // Pause everything that plays, pausable or not; with nothing playing and
                // something paused, resume it all.
                let statuses = self.engine.statuses();
                let ids = |state| statuses.iter().filter(move |(_, s)| s.state == state).map(|(id, _)| *id).collect::<Vec<_>>();
                let (playing, paused) = (ids(PlayState::Playing), ids(PlayState::Paused));
                if !playing.is_empty() {
                    for &id in &playing {
                        self.engine.pause(id);
                    }
                    self.announce("Pause All: paused");
                    Ok(json!({"paused": playing.len(), "resumed": 0}))
                } else {
                    for &id in &paused {
                        self.engine.resume(id);
                    }
                    if !paused.is_empty() {
                        self.announce("Pause All: resumed");
                    }
                    Ok(json!({"paused": 0, "resumed": paused.len()}))
                }
            }
            "transport.fadeAll" => {
                let fade = p.f64("fade")?.map_or(FADE_ALL, |f| f.clamp(0.0, 60.0));
                self.execute("transport.stopAll", json!({"fade": fade}))
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
            "bind.list" | "bind.add" | "bind.remove" | "bind.enable" | "bind.learn" | "hotkey.status" | "midi.ports" | "midi.enable"
            | "midi.autoMap" | "midi.feedback" | "osc.enable" | "osc.status" | "midi.page" | "midi.pageUp" | "midi.pageDown" => {
                self.binding_commands(id, &p)
            }
            "output.list" | "output.add" | "output.set" | "output.remove" | "settings.previewOutput" => self.output_commands(id, &p),
            "tile.send" | "tile.unsend" => self.send_commands(id, &p),
            "edit.undo" => self.step_history(false),
            "edit.redo" => self.step_history(true),
            "record.start" => self.start_recording(&p),
            "record.stop" => self.finish_recording(),
            "record.toggle" => {
                if self.recording.is_some() {
                    self.finish_recording()
                } else {
                    self.start_recording(&p)
                }
            }
            "replay.enable" => {
                let secs = match p.get("seconds") {
                    None if p.has("seconds") => 0.0,
                    Some(Value::Bool(false)) => 0.0,
                    Some(Value::String(s)) if s == "off" => 0.0,
                    _ => p.f64("seconds")?.unwrap_or(30.0),
                };
                if !(secs == 0.0 || (5.0..=REPLAY_MAX as f64).contains(&secs)) {
                    return Err(bad(format!("replay.enable: seconds is 5–{REPLAY_MAX}, or 0/off")));
                }
                self.store.library.replay_seconds = secs as u32;
                self.store.mark_dirty();
                self.keep_input();
                Ok(json!({"seconds": self.store.library.replay_seconds, "error": self.input_error}))
            }
            "replay.save" => self.save_replay(&p),
            "record.status" => Ok(match &self.recording {
                None => json!({"recording": false}),
                Some(r) => json!({
                    "recording": true,
                    "tile": r.tile,
                    "title": self.store.library.tile(r.tile).map(|t| t.title.clone()),
                    "seconds": (r.seconds() * 10.0).round() / 10.0,
                }),
            }),
            "duck.voice" => {
                let lib = &mut self.store.library;
                lib.voice_duck = p.bool("on")?.unwrap_or(true);
                if let Some(t) = p.f64("threshold")? {
                    if !(-80.0..=0.0).contains(&t) {
                        return Err(bad("duck.voice: threshold is a level in dBFS, -80 to 0"));
                    }
                    lib.voice_threshold_db = t as f32;
                }
                self.store.mark_dirty();
                self.keep_input();
                Ok(json!({"on": self.store.library.voice_duck, "threshold": self.store.library.voice_threshold_db, "error": self.input_error, "waiting": self.input_waiting()}))
            }
            "input.list" => Ok({
                self.list_inputs();
                json!({
                    "devices": self.input_devices,
                    "default": self.input_default,
                    "current": self.input.as_ref().map(|i| i.device.clone()),
                    "level_db": self.input.as_ref().map(|i| i.shared.level_db()),
                    "error": self.input_error,
                    "waiting": self.input_waiting(),
                })
            }),
            "settings.inputDevice" => {
                self.store.library.input_device = p.str("name")?.filter(|s| !s.is_empty()).map(str::to_owned);
                self.store.mark_dirty();
                self.keep_input();
                ok()
            }
            "duck.hold" => {
                let on = p.bool("on")?.unwrap_or(!self.engine.duck_hold());
                self.engine.set_duck_hold(on);
                Ok(json!({"on": on}))
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
                        "grid": enum_name(lib.grid),
                        "columns": s.columns,
                        "rows": s.row_lengths(),
                        "blanks": s.slots().iter().enumerate().flat_map(|(r, row)| row.iter().enumerate().filter(|(_, x)| x.is_none()).map(move |(a, _)| [r + 1, a + 1])).collect::<Vec<_>>(),
                        "auto_advance": s.auto_advance,
                        "list_mode": list_mode_name(s.auto_advance),
                        "crossfade": s.crossfade,
                        "notes": s.notes,
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
            "set.next" | "set.prev" => {
                let (si, n) = (self.set_idx(), self.store.library.sets.len());
                let to = if id == "set.next" { si + 1 } else { si.wrapping_sub(1) };
                if to >= n {
                    let which = if id == "set.next" { "last" } else { "first" };
                    return Err(CmdError::new("end_of_sets", format!("“{}” is the {which} set", self.current_set().name)));
                }
                let sid = self.store.library.sets[to].id;
                self.execute("set.select", json!({"set": sid}))
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
            "set.rows" => {
                let si = self.set_param(&p)?;
                let set = &mut self.store.library.sets[si];
                if let Some(n) = p.f64("perRow")? {
                    if !(1.0..=12.0).contains(&n) {
                        return Err(bad("set.rows: perRow must be 1–12"));
                    }
                    set.even_rows(n as u32);
                } else {
                    // A list, or text such as "3, 8".
                    let rows: Vec<usize> = match p.get("rows") {
                        Some(Value::Array(a)) => a.iter().map(|v| v.as_u64().filter(|&n| n > 0).map(|n| n as usize)).collect::<Option<_>>(),
                        Some(Value::String(s)) => s.split(',').map(|n| n.trim().parse().ok().filter(|&n: &usize| n > 0)).collect::<Option<_>>(),
                        _ => None,
                    }
                    .ok_or_else(|| bad("set.rows: give perRow, or rows as a list of tile counts (each at least 1)"))?;
                    let total: usize = rows.iter().sum();
                    if total > set.tiles.len() {
                        return Err(bad(format!("set.rows: those rows hold {total} tiles, and the set has {}", set.tiles.len())));
                    }
                    set.set_rows(&rows);
                }
                let rows = set.row_lengths();
                self.store.mark_dirty();
                Ok(json!({"rows": rows}))
            }
            "set.addBlank" | "set.removeBlank" => {
                let si = self.set_param(&p)?;
                let row = p.f64("row")?.ok_or_else(|| bad(format!("{id}: give row and slot (1-based)")))?.max(1.0) as usize - 1;
                let slot = p.f64("slot")?.ok_or_else(|| bad(format!("{id}: give row and slot (1-based)")))?.max(1.0) as usize - 1;
                let set = &mut self.store.library.sets[si];
                let rows = set.slots().len();
                if id == "set.addBlank" {
                    let count = p.f64("count")?.unwrap_or(1.0).clamp(1.0, 24.0) as usize;
                    if !set.insert_blanks(Place::InRow { row, at: slot }, count) {
                        return Err(bad(format!("set.addBlank: the set has {rows} row{}; blanks go into a row that has tiles", if rows == 1 { "" } else { "s" })));
                    }
                } else if !set.remove_blank(row, slot) {
                    return Err(CmdError::new("not_found", format!("set.removeBlank: slot {} of row {} isn't blank", slot + 1, row + 1)));
                }
                let slots: Vec<String> = set.slots().iter().map(|r| r.iter().map(|x| if x.is_some() { 'x' } else { '.' }).collect()).collect();
                self.store.mark_dirty();
                Ok(json!({"slots": slots}))
            }
            "set.listMode" => {
                let si = self.set_param(&p)?;
                let continuous = match p.need_str("mode")? {
                    "continuous" => true,
                    "queued" => false,
                    m => return Err(bad(format!("set.listMode: mode is continuous or queued, not {m:?}"))),
                };
                self.store.library.sets[si].auto_advance = continuous;
                self.store.mark_dirty();
                Ok(json!({"mode": list_mode_name(continuous)}))
            }
            "set.crossfade" => {
                let si = self.set_param(&p)?;
                let secs = p.need_f64("seconds")?.clamp(0.0, crate::model::CROSSFADE_MAX as f64);
                self.store.library.sets[si].crossfade = secs as f32;
                self.store.mark_dirty();
                Ok(json!({"seconds": secs}))
            }
            "set.autoAdvance" => {
                let si = self.set_param(&p)?;
                let set = &mut self.store.library.sets[si];
                set.auto_advance = p.bool("on")?.unwrap_or(!set.auto_advance);
                let on = set.auto_advance;
                self.store.mark_dirty();
                Ok(json!({"on": on}))
            }
            "set.notes" => {
                let si = self.set_param(&p)?;
                let notes = p.str("notes")?.unwrap_or_default().to_owned();
                self.store.library.sets[si].notes = notes.clone();
                self.store.mark_dirty();
                Ok(json!({"set": self.store.library.sets[si].name, "notes": notes}))
            }
            "set.lock" | "set.unlock" => {
                let si = self.set_param(&p)?;
                let on = id == "set.lock" && p.bool("on")?.unwrap_or(true);
                let set = &mut self.store.library.sets[si];
                set.locked = on;
                let name = set.name.clone();
                self.store.mark_dirty();
                Ok(json!({"set": name, "locked": on}))
            }
            "set.reassignKeys" => {
                let si = self.set_param(&p)?;
                // Keys set by hand go: every pad takes its slot's key (`Library::rekey`).
                for t in &mut self.store.library.sets[si].tiles {
                    t.hotkey_manual = false;
                }
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
                Some(path) if p.bool("ask")?.unwrap_or(false) => {
                    self.needs_window(id, "a path without ask")?;
                    let file = PathBuf::from(path);
                    if !file.is_file() && !(crate::farrago::is_export(&file) && file.is_dir()) {
                        return Err(CmdError::new("not_found", format!("No set file at {path}")));
                    }
                    let name = file.file_name().map_or_else(|| path.to_owned(), |n| n.to_string_lossy().into_owned());
                    self.announce(format!("Set file opened: {name}. Import it?"));
                    self.effects.push(Effect::ConfirmSetFile(file));
                    if p.bool("raise")?.unwrap_or(false) {
                        self.effects.push(Effect::Raise);
                    }
                    Ok(json!({"asked": true, "path": path}))
                }
                Some(path) => {
                    let use_theme = p.bool("useTheme")?;
                    let crate::setfile::Import { sets, theme, farrago } = crate::setfile::import(&mut self.store, path.as_ref())
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
                    let what = format!("{what} ({tiles} sound{})", if tiles == 1 { "" } else { "s" });
                    let theme = match theme {
                        None if farrago.is_some() => {
                            let report = farrago.as_ref().expect("checked");
                            let mut text = format!("Imported {what} from Farrago");
                            if !report.skipped.is_empty() {
                                let list: Vec<String> = report.skipped.iter().map(|s| format!("“{}” ({})", s.title, s.reason)).collect();
                                text += &format!("; skipped {}", list.join(", "));
                            }
                            if !report.dropped.is_empty() {
                                text += &format!("; not carried over: {}", report.dropped.join(", "));
                            }
                            if report.skipped.is_empty() {
                                self.notify(text);
                            } else {
                                self.notify_error(text);
                            }
                            Value::Null
                        }
                        None => {
                            self.notify(format!("Imported {what}"));
                            Value::Null
                        }
                        Some(Ok(t)) => {
                            self.notify(format!("Imported {what} and the theme {}", t.theme.name));
                            self.set_file_theme(t, use_theme)
                        }
                        Some(Err(e)) => {
                            self.notify_error(format!("Imported {what}, but not its theme: {e}"));
                            json!({"error": e})
                        }
                    };
                    let mut answer = json!({"sets": sets, "theme": theme});
                    if let Some(report) = farrago {
                        answer["farrago"] = json!(report);
                    }
                    Ok(answer)
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
                        let out = crate::setfile::export(&mut self.store, &sets, path.as_ref())
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
                    .filter(|t| query.as_ref().is_none_or(|q| matches_query(t, q)))
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
                let place = place_param(&p, "at")?.unwrap_or(Place::Append);
                match p.paths("paths")? {
                    None => {
                        self.needs_window(id, "paths")?;
                        self.effects.push(Effect::PickAudio { set: self.store.library.sets[si].id, place });
                        ok()
                    }
                    Some(paths) => {
                        let (added, errors) = self.store.import(&paths, si, place);
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
            "quip.connect" | "quip.disconnect" | "quip.list" | "quip.import" | "quip.pair" | "quip.pairing" | "quip.discover" | "quip.browse" | "quip.search" | "quip.new" | "quip.add" | "quip.preview" => {
                crate::quip::command(self, id, &params)
            }
            "import.cancel" => {
                let n = self.store.cancel_imports();
                if n > 0 {
                    self.notify(format!("Cancelled {n} import{}", if n == 1 { "" } else { "s" }));
                }
                Ok(json!({"cancelled": n}))
            }
            "tile.normalize" => self.tile_normalize(&p),
            "tile.render" | "set.render" | "render.status" | "render.cancel" => self.render_command(id, &p),
            "theme.list" | "theme.use" | "theme.import" | "theme.export" | "theme.remove" => self.theme_command(id, &p),
            "tile.addAction" | "tile.action" => self.action_commands(id, &p),
            "tile.enable" | "tile.disable" => {
                let id = self.tile_param(&p)?;
                let t = self.store.library.tile_mut(id).expect("resolved");
                t.disabled = p.id == "tile.disable";
                let disabled = t.disabled;
                self.store.mark_dirty();
                Ok(json!({"tile": id, "disabled": disabled}))
            }
            "tile.duplicate" => {
                let id = self.tile_param(&p)?;
                let (s, t) = self.store.library.locate(id).expect("resolved");
                let set = &mut self.store.library.sets[s];
                let copy = set.tiles[t].duplicate();
                let new_id = copy.id;
                // Beside the original, in its row.
                let (row, at) = set.row_of(t);
                set.insert_tiles(Place::InRow { row, at: at + 1 }, vec![copy]);
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
            "tile.move" => self.tile_move(&p),
            "tile.copy" => self.tile_copy(&p),
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
            "view.sidebar" | "view.inspector" => {
                let lib = &mut self.store.library;
                let shown = if id == "view.sidebar" { &mut lib.show_sidebar } else { &mut lib.show_inspector };
                // Choosing a tab shows the inspector; on its own the command toggles it.
                let tab = match p.str("tab")? {
                    Some(name) if id == "view.inspector" => Some(InspectorTab::named(name).ok_or_else(|| bad(format!("view.inspector: tab must be play, sound, cue or control (got {name:?})")))?),
                    _ => None,
                };
                *shown = p.bool("open")?.unwrap_or(tab.is_some() || !*shown);
                let open = *shown;
                self.store.mark_dirty();
                if let Some(tab) = tab {
                    self.inspector_tab = tab;
                }
                Ok(if id == "view.sidebar" { json!({"open": open}) } else { json!({"open": open, "tab": self.inspector_tab.name()}) })
            }
            // Perform and Edit mode went in M22 (D-147): kept for a release, as the
            // inspector's visibility; the old global lock locks the current set.
            "view.perform" => {
                self.selected_tile = None;
                self.execute("view.inspector", json!({"open": false}))
            }
            "view.edit" => self.execute("view.inspector", json!({"open": true})),
            "view.toggleEdit" => self.execute("view.inspector", json!({})),
            "view.lockEdit" => {
                let on = p.bool("on")?.unwrap_or(!self.current_set().locked);
                self.execute("set.lock", json!({"on": on}))
            }
            "view.select" => {
                let r = self.view_select(&p);
                // The editor follows the tile in the inspector, and closes with it.
                if self.editor.is_some() && self.editor != self.selected_tile {
                    self.close_editor();
                }
                r
            }
            "view.editor" => {
                let target = match p.str("tile")? {
                    Some(r) => Some(self.resolve_tile(r)?),
                    None => self.selected_tile,
                };
                // A named tile opens; on its own it toggles.
                let open = p.bool("open")?.unwrap_or(p.get("tile").is_some() || self.editor.is_none());
                if !open {
                    self.close_editor();
                    return Ok(json!({"open": false, "tile": null}));
                }
                let id = target.ok_or_else(|| bad("view.editor: give tile, or select one first"))?;
                let t = self.store.library.tile(id).expect("resolved");
                if t.action.is_some() || t.file.is_none() {
                    return Err(bad(format!("view.editor: {:?} has no sound to edit", t.title)));
                }
                self.selected_tile = Some(id);
                self.selection = vec![id];
                if self.editor != Some(id) {
                    self.close_editor();
                }
                self.editor = Some(id);
                Ok(json!({"open": true, "tile": id}))
            }
            "view.editorSelect" => {
                let (id, dur) = self.editor_tile(&p)?;
                if p.bool("clear")? == Some(true) {
                    self.editor_selection = None;
                } else {
                    match (p.f64("from")?, p.f64("to")?) {
                        (Some(a), Some(b)) => {
                            let (a, b) = (a.min(b).clamp(0.0, dur), a.max(b).clamp(0.0, dur));
                            if b <= a {
                                return Err(bad("view.editorSelect: from and to must be apart, inside the file"));
                            }
                            self.editor_selection = Some((a, b));
                        }
                        (None, None) => {}
                        _ => return Err(bad("view.editorSelect: give from and to, or clear: true")),
                    }
                }
                let sel = self.editor_selection.map(|(a, b)| json!({"from": a, "to": b}));
                Ok(json!({"tile": id, "selection": sel}))
            }
            "view.editorZoom" => {
                let (id, dur) = self.editor_tile(&p)?;
                let t = self.store.library.tile(id).expect("open");
                // A trim or a selection keeps a little either side in view, so its edges show.
                let padded = |a: f64, b: f64| {
                    let pad = (b - a) * 0.05;
                    EditorZoom::Span((a - pad).max(0.0), (b + pad).min(dur))
                };
                let zoom = if p.bool("fit")? == Some(true) {
                    EditorZoom::Fit
                } else if p.bool("trim")? == Some(true) {
                    let (s, e) = play_range(t, dur);
                    padded(s, e)
                } else if p.bool("selection")? == Some(true) {
                    let (a, b) = self.editor_selection.ok_or_else(|| bad("view.editorZoom: nothing is selected"))?;
                    padded(a, b)
                } else if let (Some(a), Some(b)) = (p.f64("from")?, p.f64("to")?) {
                    let (a, b) = (a.min(b).clamp(0.0, dur), a.max(b).clamp(0.0, dur));
                    if b <= a {
                        return Err(bad("view.editorZoom: from and to must be apart, inside the file"));
                    }
                    EditorZoom::Span(a, b)
                } else if let Some(by) = p.f64("by")? {
                    if !(by.is_finite() && by > 0.0) {
                        return Err(bad("view.editorZoom: by must be above 0 (2 zooms in, 0.5 out)"));
                    }
                    EditorZoom::By(by.clamp(1.0 / 1024.0, 1024.0))
                } else {
                    return Err(bad("view.editorZoom: give fit, trim, selection, from and to, or by"));
                };
                self.editor_zoom = Some(zoom);
                Ok(match zoom {
                    EditorZoom::Fit => json!({"tile": id, "fit": true}),
                    EditorZoom::Span(a, b) => json!({"tile": id, "from": a, "to": b}),
                    EditorZoom::By(by) => json!({"tile": id, "by": by}),
                })
            }
            "view.search" => self.view_search(&p),
            "view.waveforms" => {
                let lib = &mut self.store.library;
                lib.show_waveforms = p.bool("on")?.unwrap_or(!lib.show_waveforms);
                let on = lib.show_waveforms;
                self.store.mark_dirty();
                Ok(json!({"on": on}))
            }
            "view.meters" => {
                let lib = &mut self.store.library;
                lib.show_meters = p.bool("on")?.unwrap_or(!lib.show_meters);
                let on = lib.show_meters;
                self.store.mark_dirty();
                Ok(json!({"on": on}))
            }
            "view.theme" => {
                let k: ThemeKind = p.named("theme", "console, padd")?.ok_or_else(|| bad("view.theme: theme is required"))?;
                self.use_builtin(k);
                ok()
            }
            "view.appearance" => {
                // standard: the theme's own look (Panel's way out of high contrast, which
                // has no light or dark): Dark after High Contrast, otherwise unchanged.
                let a: Appearance = if p.str("appearance")?.is_some_and(|s| s.eq_ignore_ascii_case("standard")) {
                    match self.store.library.appearance {
                        Appearance::HighContrast => Appearance::Dark,
                        a => a,
                    }
                } else {
                    p.named("appearance", "dark, light, system, high_contrast, standard")?
                        .ok_or_else(|| bad("view.appearance: appearance is required"))?
                };
                self.store.library.appearance = a;
                self.store.mark_dirty();
                self.effects.push(Effect::ApplyLook);
                ok()
            }
            "view.zoom" => {
                let lib = &mut self.store.library;
                let zoom = match p.get("zoom") {
                    Some(Value::String(s)) if s.eq_ignore_ascii_case("in") => zoom_step(lib.zoom, true),
                    Some(Value::String(s)) if s.eq_ignore_ascii_case("out") => zoom_step(lib.zoom, false),
                    Some(Value::String(s)) if s.eq_ignore_ascii_case("reset") => 100,
                    Some(Value::String(s)) => s
                        .trim()
                        .trim_end_matches('%')
                        .parse::<f64>()
                        .map_err(|_| bad(format!("view.zoom: zoom must be in, out, reset or a percent (got {s:?})")))?
                        .round() as u32,
                    Some(v) => v.as_f64().ok_or_else(|| bad("view.zoom: zoom must be in, out, reset or a percent"))?.round() as u32,
                    None => return Err(bad("view.zoom: zoom is required")),
                };
                if !(ZOOM_MIN..=ZOOM_MAX).contains(&zoom) {
                    return Err(bad(format!("view.zoom: zoom must be {ZOOM_MIN} to {ZOOM_MAX} percent (got {zoom})")));
                }
                lib.zoom = zoom;
                self.store.mark_dirty();
                Ok(json!({"zoom": zoom}))
            }
            "view.settings" => {
                self.show_settings = p.bool("open")?.unwrap_or(true);
                if self.show_settings {
                    self.devices = Engine::output_devices();
                }
                ok()
            }
            "view.quip" => {
                self.needs_window(id, "a window")?;
                let want_new = p.bool("new")?.unwrap_or(false);
                let open = p.bool("open")?.unwrap_or(!self.show_quip || want_new || p.str("server")?.is_some() || p.str("folder")?.is_some() || p.str("query")?.is_some());
                self.show_quip = open;
                if !open {
                    return Ok(json!({"open": false}));
                }
                let mut server = match p.str("server")? {
                    Some(s) => Some(crate::quip::clips::server_for(&self.store.data_dir, Some(s))?.0),
                    None => None,
                };
                // The new clips: the server's, or the first server's with any.
                let mut new = None;
                if want_new {
                    server = server.or_else(|| self.quip_watch.first_with_new().map(str::to_owned));
                    if let Some(s) = &server {
                        new = Some(self.quip_watch.mark_seen(&self.store.data_dir, s).map_err(|e| CmdError::new("failed", e))?);
                    }
                }
                let (folder, query) = (p.str("folder")?.map(str::to_owned), p.str("query")?.map(str::to_owned));
                let count = new.as_ref().map(|l| l.clips.len());
                if server.is_some() || folder.is_some() || query.is_some() || new.is_some() {
                    self.quip_goto = Some(QuipGoto { server: server.clone(), folder, query, new });
                }
                let mut v = json!({"open": true});
                if want_new {
                    v["server"] = json!(server);
                    v["new"] = json!(count.unwrap_or(0));
                }
                Ok(v)
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
                let window = match p.str("window")? {
                    Some(r) => Some(self.open_window(id, Some(r))?),
                    None => None,
                };
                self.effects.push(Effect::Screenshot { path, window });
                ok()
            }
            "window.open" | "window.close" | "window.list" | "window.focus" | "window.move" | "window.gather" => self.window_command(id, &p),
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
            "settings.duck" => {
                let lib = &mut self.store.library;
                let range = |k: &str, lo: f64, hi: f64| -> Result<Option<f32>, CmdError> {
                    match p.f64(k)? {
                        Some(v) if !(lo..=hi).contains(&v) => Err(bad(format!("settings.duck: {k} must be {lo}…{hi}"))),
                        v => Ok(v.map(|v| v as f32)),
                    }
                };
                if let Some(v) = range("amount", -40.0, 0.0)? {
                    lib.duck_db = v;
                }
                if let Some(v) = range("attack", 0.0, 2.0)? {
                    lib.duck_attack = v;
                }
                if let Some(v) = range("release", 0.0, 5.0)? {
                    lib.duck_release = v;
                }
                self.engine.set_duck(lib.duck_db, lib.duck_attack, lib.duck_release);
                self.store.mark_dirty();
                ok()
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
            "app.checkForUpdates" => {
                // The window shows the answer, as the menu item does, unless asked not to.
                let show = p.bool("show")?.unwrap_or(true) && self.has_window;
                if p.bool("again")?.unwrap_or(true) {
                    self.updates.start(if show { crate::update::Show::Always } else { crate::update::Show::Never });
                }
                Ok(self.updates.json())
            }
            "settings.checkUpdates" => {
                let lib = &mut self.store.library;
                lib.check_updates = p.bool("on")?.unwrap_or(!lib.check_updates);
                let on = lib.check_updates;
                self.store.mark_dirty();
                Ok(json!({"on": on}))
            }
            "settings.stopAllHold" => {
                let lib = &mut self.store.library;
                lib.stop_all_hold = p.bool("on")?.unwrap_or(!lib.stop_all_hold);
                let on = lib.stop_all_hold;
                self.store.mark_dirty();
                Ok(json!({"on": on}))
            }
            "settings.gridLayout" => {
                let mode: GridMode = p.named("mode", "rows, auto, columns")?.ok_or_else(|| bad("settings.gridLayout: mode is required"))?;
                self.store.library.grid = mode;
                self.store.mark_dirty();
                Ok(json!({"mode": enum_name(mode)}))
            }
            "settings.speakCues" => {
                let lib = &mut self.store.library;
                lib.speak_cues = p.bool("on")?.unwrap_or(!lib.speak_cues);
                let on = lib.speak_cues;
                self.store.mark_dirty();
                let program = crate::speech::program();
                if on && program.is_none() {
                    self.notify_error("Speak Cue Names needs speech-dispatcher's spd-say, which isn't installed");
                }
                Ok(json!({"on": on, "voice": program, "routed": crate::speech::CAN_ROUTE}))
            }
            "settings.coloredPads" => {
                let lib = &mut self.store.library;
                lib.colored_pads = p.bool("on")?.unwrap_or(!lib.colored_pads);
                let on = lib.colored_pads;
                self.store.mark_dirty();
                Ok(json!({"on": on}))
            }
            "settings.tileKeysAllSets" => {
                let lib = &mut self.store.library;
                lib.tile_keys_all_sets = p.bool("on")?.unwrap_or(!lib.tile_keys_all_sets);
                let on = lib.tile_keys_all_sets;
                self.store.mark_dirty();
                Ok(json!({"on": on}))
            }
            "settings.quipBadge" => {
                let lib = &mut self.store.library;
                lib.quip_badge = p.bool("on")?.unwrap_or(!lib.quip_badge);
                let on = lib.quip_badge;
                self.store.mark_dirty();
                Ok(json!({"on": on}))
            }
            "settings.quipAskEachImport" => {
                let lib = &mut self.store.library;
                lib.quip_ask_each = p.bool("on")?.unwrap_or(!lib.quip_ask_each);
                let on = lib.quip_ask_each;
                self.store.mark_dirty();
                Ok(json!({"on": on}))
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

    /// Whether the current set has a tile `tile.random` can pick (one with a sound that
    /// isn't disabled): the Random button is greyed out otherwise.
    pub fn can_play_random(&self) -> bool {
        self.current_set().tiles.iter().any(|t| t.file.is_some() && crate::logic::is_cue(t))
    }

    /// `tile.random` (Play a Random Tile, and the Random button, D-171): play a tile from
    /// the pool, each once before any repeats (a shuffle bag, refilled when it runs out), so
    /// never the one it picked last.
    fn play_random(&mut self, p: &P) -> Res {
        let sets: Vec<usize> = match p.str("set")? {
            Some("*") => (0..self.store.library.sets.len()).collect(),
            r => vec![self.resolve_set(r)?],
        };
        let color = p.named::<TileColor>("color", "a tile colour (red, orange, … gray)")?;
        let lib = &self.store.library;
        let pool: Vec<Uuid> = sets
            .iter()
            .flat_map(|&si| &lib.sets[si].tiles)
            .filter(|t| t.file.is_some() && crate::logic::is_cue(t) && color.is_none_or(|c| t.color == c))
            .map(|t| t.id)
            .collect();
        if pool.is_empty() {
            return Err(CmdError::new("not_found", "No tile to pick from: none has audio and isn't disabled"));
        }
        let key = format!("{}:{}", p.str("set")?.map_or_else(|| lib.sets[sets[0]].id.to_string(), str::to_owned), enum_name(color));
        let (bag, last) = self.random.entry(key).or_default();
        // Tiles removed or disabled since leave the bag; new ones join at the next refill.
        bag.retain(|id| pool.contains(id));
        if bag.is_empty() {
            bag.clone_from(&pool);
        }
        let mut i = (Uuid::new_v4().as_u128() % bag.len() as u128) as usize;
        // A fresh bag doesn't open with the tile that closed the last one.
        if bag.len() > 1 && Some(bag[i]) == *last {
            i = (i + 1) % bag.len();
        }
        let pick = bag.swap_remove(i);
        *last = Some(pick);
        let left = bag.len();
        let tile = self.store.library.tile(pick).cloned().expect("in the pool");
        self.start(&tile)?;
        // Say which one it picked (D-171): in the status bar and, once, to screen readers
        // (not again as a tile that started), and with the system voice when cues are spoken.
        self.notify(format!("Random: playing {}", tile.title));
        self.heard.playing.insert(pick);
        if self.store.library.speak_cues {
            self.speak(&tile.title);
        }
        Ok(json!({"tile": pick, "title": tile.title, "left": left}))
    }

    /// The tiles matching a search, the current set's first, then the other sets in
    /// sidebar order. Nothing for an empty query.
    pub fn search_hits(&self, query: &str) -> Vec<Uuid> {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return Vec::new();
        }
        let cur = self.set_idx();
        let sets = &self.store.library.sets;
        std::iter::once(cur)
            .chain((0..sets.len()).filter(|&i| i != cur))
            .flat_map(|si| sets[si].tiles.iter())
            .filter(|t| matches_query(t, &q))
            .map(|t| t.id)
            .collect()
    }

    /// `view.search`: open, change or close the search box, or play its top hit.
    fn view_search(&mut self, p: &P) -> Res {
        if p.bool("close")? == Some(true) {
            self.search = None;
        } else if let Some(q) = p.str("query")? {
            self.search = Some(q.to_owned());
        } else if self.search.is_none() || !p.has("play") {
            self.search.get_or_insert_default();
            self.effects.push(Effect::FocusSearch);
        }
        let hits = self.search.as_deref().map(|q| self.search_hits(q)).unwrap_or_default();
        if p.bool("play")? == Some(true) {
            // The first hit that can play: disabled tiles and empty ones are passed over.
            let lib = &self.store.library;
            let Some(&top) = hits.iter().find(|id| lib.tile(**id).is_some_and(crate::logic::is_cue)) else {
                return Err(CmdError::new("not_found", "No playable tile matches the search"));
            };
            self.execute("tile.play", json!({"tile": top}))?;
            self.search = None;
        }
        let lib = &self.store.library;
        let hit = |id: &Uuid| {
            let (si, ti) = lib.locate(*id).expect("a hit");
            json!({"tile": id, "title": lib.sets[si].tiles[ti].title, "set": lib.sets[si].name})
        };
        Ok(json!({"open": self.search.is_some(), "query": self.search, "hits": hits.iter().map(hit).collect::<Vec<_>>()}))
    }

    /// `tile.normalize`: set the gain of one tile, or of every tile in a set, so its
    /// trimmed region reaches a peak or loudness level.
    fn tile_normalize(&mut self, p: &P) -> Res {
        let target = match p.str("target")? {
            None => level::Target::Loudness,
            Some(s) => level::Target::parse(s).ok_or_else(|| bad("tile.normalize: target must be peak or loudness"))?,
        };
        let goal = p.f64("level")?.map(|l| l as f32).unwrap_or(target.default_level());
        if !(-60.0..=0.0).contains(&goal) {
            return Err(bad(format!("tile.normalize: level must be −60…0 {}", target.unit())));
        }
        let ids: Vec<Uuid> = if p.has("tile") {
            vec![self.tile_param(p)?]
        } else {
            let si = self.set_param(p)?;
            self.store.library.sets[si].tiles.iter().filter(|t| t.file.is_some()).map(|t| t.id).collect()
        };
        let (mut done, mut skipped) = (Vec::new(), Vec::new());
        for id in ids {
            let t = self.store.library.tile(id).cloned().expect("resolved");
            // Use the decoded clip when it's in memory; otherwise (the offline CLI, or a
            // clip still loading) decode it here.
            let audio = match (self.store.audio(&t), &t.file) {
                (Some(a), _) => Some(a),
                (None, Some(f)) => crate::decode::decode_file(&self.store.media_dir.join(f)).ok().map(std::sync::Arc::new),
                (None, None) => None,
            };
            let measured = audio.as_ref().and_then(|a| {
                let (start, end) = play_range(&t, a.duration());
                level::measure(a, start, end, target)
            });
            let Some(measured) = measured else {
                skipped.push(t.title.clone());
                continue;
            };
            let gain = (level::gain_for(measured, goal) * 10.0).round() / 10.0;
            if let Some(slot) = self.store.library.tile_mut(id) {
                slot.gain_db = gain;
                self.engine.update_params(slot);
            }
            // One decimal, rounded in f64 so the JSON doesn't show 11.100000381469727.
            let tenth = |x: f32| (x as f64 * 10.0).round() / 10.0;
            done.push(json!({"tile": id, "title": t.title, "measured": tenth(measured), "gain_db": tenth(gain)}));
        }
        if !done.is_empty() {
            self.store.mark_dirty();
        }
        if !skipped.is_empty() && done.is_empty() {
            return Err(CmdError::new("failed", format!("Nothing to normalise: no audio or only silence in {}", skipped.join(", "))));
        }
        Ok(json!({"target": format!("{goal} {}", target.unit()), "tiles": done, "skipped": skipped}))
    }

    /// The selected tiles: several after ⌘/Shift-clicks, else the one in the inspector.
    pub fn selected(&self) -> Vec<Uuid> {
        let Some(primary) = self.selected_tile else { return Vec::new() };
        if !self.selection.contains(&primary) {
            return vec![primary];
        }
        self.selection.iter().copied().filter(|id| self.store.library.tile(*id).is_some()).collect()
    }

    pub fn is_selected(&self, id: Uuid) -> bool {
        self.selected_tile == Some(id) || (self.selected_tile.is_some_and(|p| self.selection.contains(&p)) && self.selection.contains(&id))
    }

    /// `view.select`: one tile, a ⌘-click toggle, a Shift-click rectangle (⌘-Shift adds
    /// it) or a list.
    fn view_select(&mut self, p: &P) -> Res {
        if let Some(list) = p.get("tiles") {
            let ids = self.tile_refs(list, "view.select")?;
            self.selected_tile = ids.last().copied();
            self.selection = ids;
        } else {
            match p.str("tile")? {
                None => {
                    self.selected_tile = None;
                    self.selection.clear();
                }
                Some(r) => {
                    let id = self.resolve_tile(r)?;
                    let lib = &self.store.library;
                    let anchor = self.selected_tile.and_then(|a| lib.locate(a)).zip(lib.locate(id)).filter(|((s1, _), (s2, _))| s1 == s2);
                    let add = p.bool("add")? == Some(true);
                    if p.bool("remove")? == Some(true) {
                        // Alt-click (D-229): out of the selection if it's in it; an
                        // unselected tile leaves the selection as it is.
                        let mut sel = self.selected();
                        if let Some(i) = sel.iter().position(|x| *x == id) {
                            sel.remove(i);
                            self.selected_tile = sel.last().copied();
                            self.selection = sel;
                        }
                    } else if let (Some(true), Some(((si, a), (_, b)))) = (p.bool("range")?, anchor) {
                        // The rectangle between the tile in the inspector (it stays the
                        // anchor) and this one (D-169); with add, on top of the selection.
                        let columns = p.f64("columns")?.map(|c| (c as usize).clamp(1, 64));
                        let set = &lib.sets[si];
                        let rect = set.rectangle(a, b, columns).into_iter().map(|i| set.tiles[i].id);
                        let mut sel = if add { self.selected() } else { Vec::new() };
                        for t in rect {
                            if !sel.contains(&t) {
                                sel.push(t);
                            }
                        }
                        self.selection = sel;
                    } else if add {
                        let mut sel = self.selected();
                        match sel.iter().position(|x| *x == id) {
                            Some(i) => {
                                sel.remove(i);
                            }
                            None => sel.push(id),
                        }
                        self.selected_tile = sel.last().copied();
                        self.selection = sel;
                    } else {
                        self.selected_tile = Some(id);
                        self.selection = vec![id];
                    }
                }
            }
        }
        Ok(json!({"selected": self.selected()}))
    }

    /// A JSON list of tile refs.
    fn tile_refs(&self, list: &Value, who: &str) -> Result<Vec<Uuid>, CmdError> {
        let refs = list.as_array().ok_or_else(|| bad(format!("{who}: tiles is a list of tiles")))?;
        refs.iter()
            .map(|r| r.as_str().ok_or_else(|| bad(format!("{who}: tiles is a list of tiles"))).and_then(|r| self.resolve_tile(r)))
            .collect()
    }

    /// `tile.move`: one tile, or with `tiles` several from one set together, as one run
    /// in the set's order (D-219); with `set`, into another set (at its end by default).
    fn tile_move(&mut self, p: &P) -> Res {
        let several = p.get("tiles");
        let ids = match several {
            Some(list) => self.tile_refs(list, "tile.move")?,
            None => vec![self.tile_param(p)?],
        };
        let lib = &self.store.library;
        let found: Vec<(usize, usize)> = ids.iter().map(|id| lib.locate(*id).expect("resolved")).collect();
        let Some(&(from_set, _)) = found.first() else { return Err(bad("tile.move: tiles is empty")) };
        if found.iter().any(|&(s, _)| s != from_set) {
            return Err(bad("tile.move: the tiles must all be in one set"));
        }
        let to_set = match p.str("set")? {
            Some(r) => self.resolve_set(Some(r))?,
            None => from_set,
        };
        let leave_blank = p.bool("leaveBlank")?.unwrap_or(false);
        let mut from: Vec<usize> = found.iter().map(|&(_, i)| i).collect();
        from.sort_unstable();
        from.dedup();
        let moved: Vec<Uuid> = from.iter().map(|&i| lib.sets[from_set].tiles[i].id).collect();
        if to_set == from_set {
            let place = place_param(p, "to")?.ok_or_else(|| bad("tile.move: give to, or row (with slot, at or newRow)"))?;
            self.store.library.sets[from_set].move_tiles(&from, place, leave_blank);
        } else {
            if leave_blank {
                return Err(bad("tile.move: leaveBlank is for a move inside the tiles' own set"));
            }
            let place = place_param(p, "to")?.unwrap_or(Place::Append);
            let sets = &mut self.store.library.sets;
            let mut tiles: Vec<Tile> = from.iter().rev().map(|&i| sets[from_set].remove_tile_at(i)).collect();
            tiles.reverse();
            sets[to_set].insert_tiles(place, tiles);
        }
        self.store.mark_dirty();
        let set = &self.store.library.sets[to_set];
        let at = |id: &Uuid| {
            let i = set.tiles.iter().position(|t| t.id == *id).unwrap_or(0);
            let (row, slot) = set.row_of(i);
            (i, row, slot)
        };
        if several.is_some() {
            let tiles: Vec<Value> = moved.iter().map(|id| {
                let (i, row, slot) = at(id);
                json!({"tile": id, "position": i + 1, "row": row + 1, "slot": slot + 1})
            }).collect();
            return Ok(json!({"set": set.name, "tiles": tiles}));
        }
        let (i, row, slot) = at(&moved[0]);
        let mut out = json!({"position": i + 1, "row": row + 1, "slot": slot + 1, "at": slot + 1});
        if to_set != from_set {
            out["set"] = json!(set.name);
        }
        Ok(out)
    }

    /// `tile.copy`: copies of one tile, or with `tiles` several from one set, dropped as
    /// one run in the set's order at a place on the board as it is (nothing comes out),
    /// or with `set` into another set, at its end by default; the originals stay where
    /// they are. What an Alt-drag does (D-229). A hand-set key stays only on a copy in
    /// another set that has no tile with that key (D-230).
    fn tile_copy(&mut self, p: &P) -> Res {
        let ids = match p.get("tiles") {
            Some(list) => self.tile_refs(list, "tile.copy")?,
            None => vec![self.tile_param(p)?],
        };
        let lib = &self.store.library;
        let found: Vec<(usize, usize)> = ids.iter().map(|id| lib.locate(*id).expect("resolved")).collect();
        let Some(&(from_set, _)) = found.first() else { return Err(bad("tile.copy: tiles is empty")) };
        if found.iter().any(|&(s, _)| s != from_set) {
            return Err(bad("tile.copy: the tiles must all be in one set"));
        }
        let to_set = match p.str("set")? {
            Some(r) => self.resolve_set(Some(r))?,
            None => from_set,
        };
        let mut from: Vec<usize> = found.iter().map(|&(_, i)| i).collect();
        from.sort_unstable();
        from.dedup();
        let sets = &mut self.store.library.sets;
        let copies: Vec<(Uuid, Tile)> = from.iter().map(|&i| {
            let t = &sets[from_set].tiles[i];
            let mut copy = t.duplicate();
            if to_set != from_set && t.hotkey_manual && t.hotkey.is_some_and(|k| tile_for_key(&sets[to_set].tiles, k).is_none()) {
                copy.hotkey = t.hotkey;
                copy.hotkey_manual = true;
            }
            (t.id, copy)
        }).collect();
        let place = place_param(p, "to")?.unwrap_or(Place::Append);
        let made: Vec<(Uuid, Uuid)> = copies.iter().map(|(of, c)| (*of, c.id)).collect();
        sets[to_set].insert_tiles(place, copies.into_iter().map(|(_, c)| c).collect());
        self.store.mark_dirty();
        let set = &self.store.library.sets[to_set];
        let tiles: Vec<Value> = made.iter().map(|(of, id)| {
            let i = set.tiles.iter().position(|t| t.id == *id).unwrap_or(0);
            let (row, slot) = set.row_of(i);
            json!({"tile": id, "copyOf": of, "position": i + 1, "row": row + 1, "slot": slot + 1})
        }).collect();
        Ok(json!({"set": set.name, "tiles": tiles}))
    }

    /// `tile.set`: one tile, or with `tiles` several at once. If any of them refuses a
    /// setting, none of them changes.
    fn tile_set(&mut self, p: &P) -> Res {
        let Some(list) = p.get("tiles") else {
            let id = self.tile_param(p)?;
            return self.tile_set_one(id, p);
        };
        let ids = self.tile_refs(list, "tile.set")?;
        if ids.is_empty() {
            return Err(bad("tile.set: tiles is empty"));
        }
        if ids.len() > 1 && p.has("hotkey") {
            return Err(bad("tile.set: a hotkey belongs to one tile, so it can't be set on several"));
        }
        let before = self.store.library.sets.clone();
        let mut out = Vec::new();
        for id in &ids {
            match self.tile_set_one(*id, p) {
                Ok(v) => out.push(v),
                Err(e) => {
                    self.store.library.sets = before;
                    for t in self.store.library.sets.iter().flat_map(|s| &s.tiles).filter(|t| ids.contains(&t.id)) {
                        self.engine.update_params(t);
                    }
                    return Err(e);
                }
            }
        }
        Ok(json!({"tiles": out}))
    }

    fn tile_set_one(&mut self, id: Uuid, p: &P) -> Res {
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
        // The emoji beside the title (D-253): emoji only; empty or null clears it.
        if p.has("emoji") {
            let e = p.str("emoji")?.map(str::trim).unwrap_or_default();
            if !e.is_empty() && !crate::model::is_emoji(e) {
                return Err(bad(format!("tile.set: emoji takes emoji only (got {e:?}); \"\" clears it")));
            }
            if e.chars().count() > EMOJI_MAX_CHARS {
                return Err(bad(format!("tile.set: emoji is at most {EMOJI_MAX_CHARS} characters")));
            }
            t.emoji = e.to_owned();
        }
        if let Some(c) = p.named::<TileColor>("color", "red, orange, yellow, lime, green, mint, teal, cyan, blue, indigo, purple, pink, rose, crimson, brown, gray")? {
            t.color = c;
        }
        // The old single mode, kept as an alias: it sets all three options (D-145).
        if let Some(m) = p.named::<PlayMode>("mode", "toggle, restart, hold, pause")? {
            t.press = PressOptions::from_mode(m);
        }
        let one_shot = if p.has("oneShot") { p.bool("oneShot")? } else { p.bool("oneshot")? };
        if let (Some(true), Some(true)) = (p.bool("pausable")?, one_shot) {
            return Err(bad("tile.set: pausable and oneShot can't both be on (a press either pauses it or starts it again)"));
        }
        if let Some(b) = p.bool("hold")? {
            t.press.hold = b;
        }
        if let Some(b) = p.bool("pausable")? {
            t.press.set_pausable(b);
        }
        if let Some(b) = one_shot {
            t.press.set_one_shot(b);
        }
        if let Some(v) = unit("volume", p.f64("volume")?)? {
            t.volume = v;
        }
        if let Some(c) = p.named::<FadeCurve>("fadeCurve", "gentle, linear, equal_power, exponential, s_curve")? {
            t.fade_curve = c;
        }
        if let Some(v) = p.m.get("sends") {
            t.sends = self.parse_sends(v)?;
        }
        if p.has("output") {
            // The tile's Output is its first send.
            let o = self.send_output("tile.set", p.str("output")?.unwrap_or(MAIN_OUTPUT))?;
            match t.sends.first_mut() {
                Some(first) => first.output = o,
                None => t.sends.push(TileSend::to(o)),
            }
        }
        tidy_sends(&mut t.sends);
        if let Some(v) = p.m.get("channels") {
            t.channel_mix = self.parse_channel_mix(&t, v)?;
            tidy_channel_mix(&mut t.channel_mix);
        }
        if let Some(d) = p.named::<Duck>("duck", "none, ducks, ducked")? {
            t.duck = d;
        }
        // A bed's own duck level (D-245): dB, or default (null) to follow the library's.
        if let Some(k) = ["duckLevel", "duck_level"].into_iter().find(|k| p.has(k)) {
            let default = p.get(k).is_none_or(|v| v.as_str().is_some_and(|s| s.trim().eq_ignore_ascii_case("default")));
            t.duck_db = if default {
                None
            } else {
                let db = p.f64(k).map_err(|_| bad(format!("tile.set: {k} must be dB ({DUCK_DB_MIN} to 0) or default")))?.unwrap_or_default();
                if !(DUCK_DB_MIN as f64..=0.0).contains(&db) {
                    return Err(bad(format!("tile.set: {k} must be {DUCK_DB_MIN} to 0 dB, or default")));
                }
                Some(db as f32)
            };
        }
        if let Some(st) = p.f64("pitch")? {
            if !(-crate::pitch::PITCH_MAX as f64..=crate::pitch::PITCH_MAX as f64).contains(&st) {
                return Err(bad(format!("tile.set: pitch must be −{0}…+{0} semitones", crate::pitch::PITCH_MAX)));
            }
            t.pitch = st as f32;
        }
        if let Some(sp) = p.f64("speed")? {
            if !(SPEED_MIN as f64..=SPEED_MAX as f64).contains(&sp) {
                return Err(bad(format!("tile.set: speed must be {SPEED_MIN}–{SPEED_MAX}")));
            }
            t.speed = sp as f32;
        }
        if let Some(db) = p.f64("gainDb")? {
            if !(-GAIN_DB_MAX as f64..=GAIN_DB_MAX as f64).contains(&db) {
                return Err(bad(format!("tile.set: gainDb must be −{GAIN_DB_MAX}…+{GAIN_DB_MAX}")));
            }
            t.gain_db = db as f32;
        }
        if let Some(v) = secs("fadeIn", p.f64("fadeIn")?)? {
            t.fade_in = v;
        }
        if let Some(v) = secs("fadeOut", p.f64("fadeOut")?)? {
            t.fade_out = v;
        }
        if let Some(v) = secs("stopFade", p.f64("stopFade")?)? {
            t.stop_fade = v;
        }
        for (key, slot) in [("preWait", &mut t.pre_wait), ("postWait", &mut t.post_wait)] {
            if let Some(v) = p.f64(key)? {
                if !(0.0..=WAIT_MAX).contains(&v) {
                    return Err(bad(format!("tile.set: {key} is 0–{WAIT_MAX} seconds")));
                }
                *slot = v;
            }
        }
        if let Some(l) = p.str("link")? {
            t.link = match l.to_ascii_lowercase().as_str() {
                "none" | "go" => CueLink::None,
                "continue" | "auto-continue" | "autocontinue" => CueLink::Continue,
                "follow" | "auto-follow" | "autofollow" => CueLink::Follow,
                other => return Err(bad(format!("tile.set: link is none, continue or follow (got {other:?})"))),
            };
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
        if p.has("vamp") && p.get("vamp").is_none() {
            t.vamp = None;
        }
        if p.has("vampStart") || p.has("vampEnd") {
            let (a0, b0) = t.vamp.unwrap_or((t.start, t.end.or(t.duration).unwrap_or(t.start + 1.0)));
            let a = if p.has("vampStart") { p.f64("vampStart")? } else { Some(a0) };
            let b = if p.has("vampEnd") { p.f64("vampEnd")? } else { Some(b0) };
            t.vamp = match (a, b) {
                (Some(a), Some(b)) if b - a >= 0.05 && a >= t.start && t.end.is_none_or(|e| b <= e) => Some((a, b)),
                (Some(_), Some(_)) => return Err(bad("tile.set: the vamp must lie inside the trim and last at least 0.05 s")),
                _ => None,
            };
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
        for (key, field) in [("onStart", &mut t.on_start), ("onStop", &mut t.on_stop)] {
            if let Some(list) = self.messages_param(p, key)? {
                *field = list;
            }
        }
        // Like the title, stored exactly as typed.
        if let Some(n) = p.str("notes")? {
            t.notes = n.to_owned();
        }
        if let Some(b) = p.bool("disabled")? {
            t.disabled = b;
        }
        // Mute (D-251): zero gain on every send; the volume stays as it was.
        if let Some(b) = p.bool("muted")? {
            t.muted = b;
        }
        // Some(None): automatic; Some(Some(None)): no key, by hand.
        let hotkey = if p.has("hotkey") {
            match p.str("hotkey")?.map(str::trim) {
                None | Some("") => Some(Some(None)),
                Some(k) if k.eq_ignore_ascii_case("auto") => Some(None),
                Some(k) => Some(Some(Some(PadKey::parse(k).ok_or_else(|| bad(format!("tile.set: {KEY_HELP}, or auto")))?))),
            }
        } else {
            None
        };
        let known = [
            "tile", "tiles", "title", "emoji", "color", "mode", "hold", "pausable", "oneShot", "oneshot", "volume", "gainDb", "speed", "pitch", "output", "sends", "channels", "duck", "duckLevel", "duck_level", "fadeCurve", "fadeIn", "fadeOut", "stopFade", "preWait", "postWait", "link", "loop", "solo", "trimStart", "trimEnd",
            "pictureLayer", "pictureFit", "pictureOpacity", "hotkey", "vamp", "vampStart", "vampEnd",
            "onStart", "onStop", "notes", "disabled", "muted",
        ];
        if let Some(k) = p.m.keys().find(|k| !known.contains(&k.as_str())) {
            return Err(bad(format!("tile.set: unknown setting {k:?}")));
        }
        if let (Some(f), true) = (&t.file, t.pitch != 0.0) {
            self.store.request_pitch(f, t.pitch);
        }
        self.engine.update_params(&t);
        let (si, _) = self.store.library.locate(id).expect("resolved");
        if let Some(slot) = self.store.library.tile_mut(id) {
            *slot = t;
        }
        match hotkey {
            Some(Some(key)) => set_hotkey(&mut self.store.library.sets[si].tiles, id, key),
            Some(None) => set_hotkey_auto(&mut self.store.library.sets[si].tiles, id),
            None => {}
        }
        self.store.mark_dirty();
        Ok(self.tile_json(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wav;
    use std::sync::Arc;

    fn controller() -> (Controller, PathBuf) {
        let root = std::env::temp_dir().join(format!("sb-ctl-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let store = Store::open(root.join("data")).unwrap();
        (Controller::new(store, Engine::disabled("test")), root)
    }

    /// `window.*` (D-224): the library keeps which sets are open in windows and where;
    /// the app is asked to raise, move and gather them.
    #[test]
    fn window_commands_open_close_and_ask_the_app() {
        let (mut c, _root) = controller();
        c.execute("set.create", json!({"name": "Cues"})).unwrap();
        c.execute("set.select", json!({"set": "#1"})).unwrap();
        c.effects.clear();
        let r = c.execute("window.open", json!({"set": "Cues", "x": 1920, "y": 0, "width": 100})).unwrap();
        assert_eq!(r["opened"], json!(true));
        let w = c.store.library.set_windows[0].clone();
        assert_eq!((w.x, w.y, w.width, w.height), (Some(1920.0), Some(0.0), Some(crate::model::SET_WINDOW_MIN[0]), None));
        assert!(c.effects.is_empty(), "a new window needs nothing more from the app");
        // Again: it's raised, not opened twice.
        let r = c.execute("window.open", json!({"set": "Cues"})).unwrap();
        assert_eq!(r["opened"], json!(false));
        assert_eq!(c.store.library.set_windows.len(), 1);
        let cues = w.set;
        assert_eq!(c.effects, vec![Effect::Window { window: Some(cues), op: WindowOp::Focus }]);
        c.effects.clear();
        // The current set by default.
        c.execute("window.open", json!({})).unwrap();
        assert_eq!(c.store.library.set_windows.len(), 2);

        c.execute("window.move", json!({"set": "Cues", "x": 10, "y": 20, "height": 300})).unwrap();
        assert_eq!(c.effects, vec![Effect::Window { window: Some(cues), op: WindowOp::Place { pos: Some([10.0, 20.0]), size: Some([0.0, 300.0]) } }]);
        c.effects.clear();
        c.execute("window.focus", json!({})).unwrap();
        assert_eq!(c.effects, vec![Effect::Window { window: None, op: WindowOp::Focus }]);
        assert_eq!(c.execute("window.gather", json!({})).unwrap()["moved"], json!(2));
        assert_eq!(c.execute("window.move", json!({"x": 10})).unwrap_err().code, "bad_params", "x without y");
        assert_eq!(c.execute("window.move", json!({"set": "Cues"})).unwrap_err().code, "bad_params", "nothing to do");
        assert_eq!(c.execute("window.open", json!({"x": "far"})).unwrap_err().code, "bad_params");
        c.effects.clear();
        let shot = c.execute("view.screenshot", json!({"path": "/tmp/w.png", "window": "Cues"}));
        assert!(shot.is_ok());
        assert_eq!(c.effects, vec![Effect::Screenshot { path: "/tmp/w.png".into(), window: Some(cues) }]);

        // window.list reads what the app last drew.
        c.windows = vec![
            WindowView { set: None, title: "LeadStinger".into(), pos: Some([0.0, 0.0]), size: Some([1280.0, 800.0]), focused: true },
            WindowView { set: Some(cues), title: "Cues · LeadStinger".into(), pos: None, size: Some([720.0, 520.0]), focused: false },
        ];
        let list = c.execute("window.list", json!({})).unwrap();
        assert_eq!(list["windows"][0]["window"], json!("main"));
        assert_eq!(list["windows"][1]["name"], json!("Cues"));
        assert!(list["windows"][1].get("x").is_none(), "no position where the compositor keeps it");

        assert_eq!(c.execute("window.close", json!({"set": "Cues"})).unwrap()["closed"], json!(["Cues"]));
        assert_eq!(c.windows.len(), 1, "window.list forgets it at once");
        assert_eq!(c.execute("window.close", json!({"set": "Cues"})).unwrap_err().code, "not_open");
        assert_eq!(c.execute("window.focus", json!({"set": "Cues"})).unwrap_err().code, "not_open");
        assert_eq!(c.execute("view.screenshot", json!({"path": "/tmp/w.png", "window": "Cues"})).unwrap_err().code, "not_open");
        c.execute("window.open", json!({"set": "Cues"})).unwrap();
        assert_eq!(c.execute("window.close", json!({"all": true})).unwrap()["closed"].as_array().unwrap().len(), 2);
        assert!(c.store.library.set_windows.is_empty());

        // Deleting a set closes its window; without a window there's nothing to ask.
        c.execute("window.open", json!({"set": "Cues"})).unwrap();
        c.execute("set.delete", json!({"set": "Cues"})).unwrap();
        assert!(c.store.library.set_windows.is_empty(), "its window went with it");
        c.has_window = false;
        assert_eq!(c.execute("window.open", json!({})).unwrap_err().code, "no_window");
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

    /// An import that ends before the next tick (a small file added from the CLI while the
    /// window is idle) is still announced: it was missed when ticks compared snapshots.
    #[test]
    fn an_import_that_ends_between_ticks_is_announced() {
        let (mut c, root) = controller();
        c.tick(0.0);
        let p = root.join("Quick.wav");
        wav::write_tone(&p, 8000, 0.5, &[330.0], 0.4).unwrap();
        c.execute("tile.add", json!({"paths": [p]})).unwrap();
        c.store.finish_imports();
        c.tick(0.0);
        assert_eq!(c.announcement.as_ref().map(|a| a.text.as_str()), Some("Imported Quick"));
        std::fs::remove_dir_all(root).ok();
    }

    /// Every registered command has an implementation: run each with empty parameters;
    /// anything but "unknown_command" or a panic counts.
    /// Check for Updates never runs unasked: it's off at launch by default, the command
    /// starts a check off the thread and the next call gives the answer (D-206).
    #[test]
    fn check_for_updates_answers_off_the_thread() {
        let (mut c, root) = controller();
        assert!(!c.store.library.check_updates);
        assert_eq!(c.execute("settings.checkUpdates", json!({})).unwrap()["on"], true);
        assert!(c.store.library.check_updates);
        assert_eq!(c.execute("settings.checkUpdates", json!({"on": false})).unwrap()["on"], false);

        c.updates = crate::update::Checker::with(|| {
            crate::update::compare("0.2.0", crate::update::Release { version: "9.0.0".into(), notes: "New".into(), url: "https://github.com/weckere/leadstinger/releases/tag/v9.0.0".into(), prerelease: false })
        });
        assert_eq!(c.execute("app.checkForUpdates", json!({"again": false})).unwrap()["status"], Value::Null);
        c.execute("app.checkForUpdates", json!({})).unwrap();
        let t = Instant::now();
        let v = loop {
            let v = c.execute("app.checkForUpdates", json!({"again": false})).unwrap();
            if v["checking"] != true {
                break v;
            }
            assert!(t.elapsed() < Duration::from_secs(5), "the check finishes");
            std::thread::sleep(Duration::from_millis(5));
        };
        assert_eq!((v["status"].as_str(), v["latest"].as_str()), (Some("newer"), Some("9.0.0")));
        // The menu's check is shown in the window; one without a window isn't.
        assert_eq!(c.updates.finished_since(0).unwrap().show, crate::update::Show::Always);
        std::fs::remove_dir_all(root).ok();
    }

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
    fn status_reports_each_playing_tiles_meter() {
        let (mut c, root) = controller();
        let ids = add(&mut c, &root, &["Tone"]);
        c.execute("tile.trigger", json!({"tile": "Tone"})).unwrap();
        c.tick(0.05);
        let db = c.execute("status", json!({})).unwrap()["playing"][0]["level_db"].as_f64().unwrap();
        assert!((-12.0..=-6.0).contains(&db), "a 0.4 tone reads about −8 dB: {db}");
        c.execute("master.volume", json!({"volume": 0.5})).unwrap();
        // After master: −6 dB, reached by falling at 26 dB/s.
        c.tick(0.05);
        assert!(c.level_db(ids[0]) > db as f32 - 2.0, "falls slowly");
        for _ in 0..5 {
            c.tick(0.05);
        }
        assert!((c.level_db(ids[0]) - (db as f32 - 6.0)).abs() < 1.0, "{} vs {db}", c.level_db(ids[0]));
        c.execute("transport.stopAll", json!({"immediate": true})).unwrap();
        c.tick(0.15);
        assert!(c.levels.is_empty(), "a stopped tile drops its meter");
        assert_eq!(c.execute("view.meters", json!({})).unwrap()["on"], false);
        assert!(!c.store.library.show_meters);
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
    fn stopping_uses_the_stop_fade_not_the_fade_out() {
        let (mut c, root) = controller();
        let id = add(&mut c, &root, &["Bed"])[0];
        // The end-of-clip fade no longer applies to a stop: no stop fade, so the 0.1 s
        // de-click (D-205).
        c.execute("tile.set", json!({"tile": id, "fadeOut": 2.0})).unwrap();
        c.execute("tile.play", json!({"tile": id})).unwrap();
        assert_eq!(c.execute("tile.stop", json!({"tile": id})).unwrap()["state"], "stopping", "the 0.1 s ramp");
        c.tick(0.08);
        assert_eq!(c.engine.state(id), PlayState::Stopping);
        c.tick(0.04);
        assert_eq!(c.engine.state(id), PlayState::Stopped);

        let r = c.execute("tile.set", json!({"tile": id, "stopFade": 0.3})).unwrap();
        assert_eq!(r["stop_fade"], 0.3f32 as f64);
        assert_eq!(c.execute("tile.set", json!({"tile": id, "stopFade": -1})).unwrap_err().code, "bad_params");
        c.execute("tile.play", json!({"tile": id})).unwrap();
        c.execute("tile.trigger", json!({"tile": id})).unwrap(); // toggles off, with the stop fade
        c.tick(0.1);
        assert_eq!(c.engine.state(id), PlayState::Stopping);
        c.tick(0.3);
        assert_eq!(c.engine.state(id), PlayState::Stopped);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn stop_all_is_two_stage_and_the_old_mode_commands_still_work() {
        let (mut c, root) = controller();
        let ids = add(&mut c, &root, &["A", "B"]);
        let play = |c: &mut Controller| {
            for id in &ids {
                c.execute("tile.play", json!({"tile": id})).unwrap();
            }
        };
        // Immediate by default: one press is enough, and a second is a plain first stage.
        // Immediate is the 0.1 s de-click (D-205), not a cut.
        play(&mut c);
        assert_eq!(c.execute("transport.stopAll", json!({})).unwrap(), json!({"fade": 0.0, "second": false}));
        c.tick(0.08);
        assert_eq!(c.engine.state(ids[0]), PlayState::Stopping, "de-clicking");
        c.tick(0.04);
        assert!(c.engine.statuses().is_empty());

        // With a fade: the first press fades, the second stops at once.
        c.execute("settings.stopAllFade", json!({"seconds": 2})).unwrap();
        play(&mut c);
        assert_eq!(c.execute("transport.stopAll", json!({})).unwrap()["fade"], 2.0);
        c.tick(0.1);
        assert_eq!(c.engine.state(ids[0]), PlayState::Stopping);
        assert_eq!(c.execute("transport.stopAll", json!({})).unwrap()["second"], true);
        c.tick(0.08);
        assert_eq!(c.engine.state(ids[0]), PlayState::Stopping, "the 0.1 s de-click from mid-fade");
        c.tick(0.04);
        assert!(c.engine.statuses().is_empty(), "stopped at once mid-fade");

        // Shift+Esc's immediate Stop All: the same 0.1 s, never the Stop All fade.
        play(&mut c);
        assert_eq!(c.execute("transport.stopAll", json!({"immediate": true})).unwrap()["fade"], 0.0);
        c.tick(0.08);
        assert_eq!(c.engine.state(ids[1]), PlayState::Stopping);
        c.tick(0.04);
        assert!(c.engine.statuses().is_empty());

        // Starting a tile between presses makes the next press a first stage again.
        play(&mut c);
        c.execute("transport.stopAll", json!({})).unwrap();
        c.execute("tile.play", json!({"tile": ids[0]})).unwrap();
        assert_eq!(c.execute("transport.stopAll", json!({})).unwrap()["second"], false);
        // An explicit fade or immediate is never a second stage.
        assert_eq!(c.execute("transport.stopAll", json!({"fade": 1})).unwrap()["second"], false);
        assert_eq!(c.execute("transport.stopAll", json!({"immediate": true})).unwrap()["fade"], 0.0);

        assert_eq!(c.execute("settings.stopAllHold", json!({})).unwrap()["on"], true);
        assert!(c.store.library.stop_all_hold);

        // Spoken cue names are off until turned on (D-092), and turn off again.
        assert!(!c.store.library.speak_cues);
        assert_eq!(c.execute("settings.speakCues", json!({"on": false})).unwrap()["on"], false);
        assert_eq!(c.execute("settings.speakCues", json!({})).unwrap()["routed"], crate::speech::CAN_ROUTE);
        assert!(c.store.library.speak_cues);
        c.execute("settings.speakCues", json!({})).unwrap();
        assert!(!c.store.library.speak_cues);

        // The old mode commands (D-147): the inspector's visibility, and the set lock.
        c.execute("view.select", json!({"tile": ids[0]})).unwrap();
        assert_eq!(c.execute("view.perform", json!({})).unwrap()["open"], false);
        assert_eq!((c.selected_tile, c.store.library.show_inspector), (None, false), "Perform hides the inspector and deselects");
        assert_eq!(c.execute("view.edit", json!({})).unwrap()["open"], true);
        assert_eq!(c.execute("view.toggleEdit", json!({})).unwrap()["open"], false);
        assert_eq!(c.execute("view.lockEdit", json!({})).unwrap()["locked"], true);
        assert!(c.current_set().locked, "Lock Edit Mode locks the current set");
        assert_eq!(c.execute("view.lockEdit", json!({"on": false})).unwrap()["locked"], false);
        std::fs::remove_dir_all(root).ok();
    }

    /// The set lock (M22, D-149): every change to a locked set is refused with code
    /// locked, from any command that would make it; its tiles still play and preview;
    /// other sets, Grid or List, the lock itself and copies of the set still work.
    #[test]
    fn a_locked_set_refuses_every_change_and_still_plays() {
        let (mut c, root) = controller();
        let ids = add(&mut c, &root, &["A", "B", "C"]);
        c.execute("tile.addAction", json!({"kind": "fade", "targets": [ids[0]]})).unwrap();
        let action = c.current_set().tiles[3].id;
        let extra = root.join("Extra.wav");
        wav::write_tone(&extra, 8000, 0.3, &[220.0], 0.4).unwrap();
        let lib = c.store.library.clone();
        assert_eq!(c.execute("set.lock", json!({})).unwrap(), json!({"set": c.current_set().name, "locked": true}));
        let (a, b) = (ids[0].to_string(), ids[1].to_string());
        let refused = [
            ("tile.set", json!({"tile": a, "volume": 0.5})),
            ("tile.set", json!({"tiles": [a, b], "loop": true})),
            ("tile.add", json!({"paths": [extra]})),
            ("tile.addAction", json!({"kind": "stop", "targets": [a]})),
            ("tile.move", json!({"tile": a, "to": 3})),
            ("tile.remove", json!({"tile": a})),
            ("tile.duplicate", json!({"tile": a})),
            ("tile.replaceAudio", json!({"tile": a, "path": extra})),
            ("tile.disable", json!({"tile": a})),
            ("tile.enable", json!({"tile": a})),
            ("tile.normalize", json!({"tile": a})),
            ("tile.normalize", json!({})),
            ("tile.markerAdd", json!({"tile": a, "at": 0.1})),
            ("tile.action", json!({"tile": action, "kind": "stop"})),
            ("tile.pictureClear", json!({"tile": a})),
            ("tile.send", json!({"tile": a, "output": "main"})),
            ("set.rename", json!({"name": "Renamed"})),
            ("set.columns", json!({"columns": 2})),
            ("set.rows", json!({"perRow": 2})),
            ("set.addBlank", json!({"row": 1, "slot": 1})),
            ("set.autoAdvance", json!({"on": true})),
            ("set.notes", json!({"notes": "changed"})),
            ("set.reassignKeys", json!({})),
            ("set.delete", json!({})),
            ("record.start", json!({})),
            ("record.start", json!({"tile": a})),
        ];
        for (id, params) in refused {
            let e = c.execute(id, params.clone()).expect_err(&format!("{id} {params} went through"));
            assert_eq!(e.code, "locked", "{id} {params}: {e}");
        }
        assert_eq!(c.store.library.sets, lib.sets.iter().map(|s| SoundSet { locked: true, ..s.clone() }).collect::<Vec<_>>(), "nothing changed");
        // Playing still works.
        c.execute("tile.trigger", json!({"tile": a})).unwrap();
        c.execute("tile.preview", json!({"tile": b})).unwrap();
        assert_eq!((c.engine.state(ids[0]), c.engine.state(ids[1])), (PlayState::Playing, PlayState::Playing));
        c.execute("transport.stopAll", json!({"immediate": true})).unwrap();
        // So do Grid or List, a copy (unlocked), and another set.
        c.execute("set.layout", json!({"layout": "list"})).unwrap();
        let copy = c.execute("set.duplicate", json!({})).unwrap()["id"].as_str().unwrap().to_owned();
        assert!(!c.current_set().locked, "the copy is unlocked");
        c.execute("set.rename", json!({"name": "Copy"})).unwrap();
        c.execute("tile.set", json!({"tile": "#1", "volume": 0.5})).unwrap();
        assert_eq!(c.current_set().id.to_string(), copy);
        // A locked set named from another set is still refused.
        let first = lib.sets[0].name.clone();
        assert_eq!(c.execute("set.notes", json!({"set": first, "notes": "x"})).unwrap_err().code, "locked");
        assert_eq!(c.execute("tile.set", json!({"tile": a, "title": "x"})).unwrap_err().code, "locked", "by id, from another set");
        // Unlocked, it changes again; status and set files carry the lock.
        c.execute("set.select", json!({"set": first})).unwrap();
        assert_eq!(c.status()["locked"], true);
        assert_eq!(c.execute("set.unlock", json!({})).unwrap()["locked"], false);
        c.execute("tile.set", json!({"tile": a, "volume": 0.5})).unwrap();
        // Undo goes back through the unlock, never past it.
        c.execute("edit.undo", json!({})).unwrap();
        c.execute("edit.undo", json!({})).unwrap();
        assert!(c.current_set().locked, "undoing the unlock locks it again");
        // A set's right-click menu names the set by id: it locks and unlocks that set,
        // not the one showing.
        let other = c.execute("set.create", json!({})).unwrap()["id"].as_str().unwrap().to_owned();
        c.execute("set.select", json!({"set": first})).unwrap();
        assert_eq!(c.execute("set.lock", json!({"set": other})).unwrap()["locked"], true);
        assert!(c.store.library.sets.iter().any(|s| s.id.to_string() == other && s.locked));
        c.execute("set.unlock", json!({"set": other})).unwrap();
        assert!(c.store.library.sets.iter().any(|s| s.id.to_string() == other && !s.locked));
        assert!(c.current_set().locked, "the showing set keeps its own lock");
        std::fs::remove_dir_all(root).ok();
    }

    /// The sidebar and inspector (M22): shown by default, toggled, and the inspector's tab.
    #[test]
    fn the_sidebar_and_inspector_show_and_hide() {
        let (mut c, root) = controller();
        assert!(c.store.library.show_sidebar && c.store.library.show_inspector);
        assert_eq!(c.execute("view.sidebar", json!({})).unwrap()["open"], false);
        assert_eq!(c.execute("view.sidebar", json!({"open": true})).unwrap()["open"], true);
        assert_eq!(c.execute("view.inspector", json!({})).unwrap(), json!({"open": false, "tab": "play"}));
        assert_eq!(c.execute("view.inspector", json!({"tab": "Cue"})).unwrap(), json!({"open": true, "tab": "cue"}), "a tab shows it");
        assert_eq!(c.execute("view.inspector", json!({"tab": "loud"})).unwrap_err().code, "bad_params");
        let s = c.status();
        assert_eq!((s["sidebar"].clone(), s["inspector"].clone(), s["inspector_tab"].clone()), (json!(true), json!(true), json!("cue")));
        std::fs::remove_dir_all(root).ok();
    }

    /// Settings ▸ Coloured pads (D-152): off by default, toggled or set, and saved.
    #[test]
    fn coloured_pads_setting_round_trips() {
        let (mut c, root) = controller();
        assert!(!c.store.library.colored_pads, "dark pads by default");
        assert_eq!(c.execute("settings.coloredPads", json!({})).unwrap(), json!({"on": true}), "no on: toggles");
        assert_eq!(c.execute("settings.coloredPads", json!({"on": true})).unwrap()["on"], true);
        c.store.save_now().unwrap();
        let again = Store::open(root.join("data")).unwrap();
        assert!(again.library.colored_pads, "saved in the library");
        assert_eq!(c.execute("settings.coloredPads", json!({"on": false})).unwrap()["on"], false);
        c.store.save_now().unwrap();
        assert!(!Store::open(root.join("data")).unwrap().library.colored_pads);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn cue_links_pre_wait_continue_and_follow() {
        let (mut c, root) = controller();
        let ids = add(&mut c, &root, &["A", "B", "C", "D"]);
        let playing = |c: &Controller, i: usize| c.engine.state(ids[i]) == PlayState::Playing;
        let pointer = |c: &Controller| c.cue.get(&c.current_set().id).copied().unwrap_or(0);
        // Continue with no post-wait: GO starts A and B together, and the pointer moves past
        // the whole chain.
        c.execute("tile.set", json!({"tile": "A", "link": "continue"})).unwrap();
        c.execute("cue.top", json!({})).unwrap();
        c.execute("cue.go", json!({})).unwrap();
        assert!(playing(&c, 0) && playing(&c, 1) && !playing(&c, 2));
        assert_eq!(pointer(&c), 2, "next is C");
        c.execute("transport.stopAll", json!({})).unwrap();

        // With a post-wait, B waits for it; Stop All drops the wait.
        c.execute("tile.set", json!({"tile": "A", "postWait": 0.15})).unwrap();
        c.execute("cue.top", json!({})).unwrap();
        c.execute("cue.go", json!({})).unwrap();
        assert!(playing(&c, 0) && !playing(&c, 1));
        assert!(c.linking(ids[0]).is_some_and(|s| s > 0.0 && s <= 0.15));
        std::thread::sleep(Duration::from_millis(200));
        c.tick(0.0);
        assert!(playing(&c, 1), "B started after the post-wait");
        c.execute("transport.stopAll", json!({"immediate": true})).unwrap();
        c.execute("cue.top", json!({})).unwrap();
        c.execute("cue.go", json!({})).unwrap();
        c.execute("transport.stopAll", json!({"immediate": true})).unwrap();
        assert!(c.pending.is_empty(), "Stop All drops the link");
        // Stopping the tile drops its link too.
        c.execute("tile.play", json!({"tile": "A"})).unwrap();
        c.execute("tile.stop", json!({"tile": "A"})).unwrap();
        assert!(c.linking(ids[0]).is_none());
        c.execute("tile.set", json!({"tile": "A", "link": "none", "postWait": 0})).unwrap();
        c.tick(0.01);

        // Follow: C starts when B ends; a disabled cue in between is skipped.
        c.execute("tile.set", json!({"tile": "B", "link": "follow"})).unwrap();
        c.execute("tile.disable", json!({"tile": "C"})).unwrap();
        c.execute("tile.play", json!({"tile": "B"})).unwrap();
        assert_eq!(pointer(&c), 4, "past B and D, the cue its link starts");
        c.tick(0.3);
        assert!(playing(&c, 1) && !playing(&c, 3));
        c.tick(0.3);
        assert!(!playing(&c, 1) && playing(&c, 3), "D followed B");
        c.execute("transport.stopAll", json!({"immediate": true})).unwrap();
        c.tick(0.15);

        // Pre-wait: the tile waits, a second press cancels it, and it starts when it's up.
        c.execute("tile.set", json!({"tile": "D", "preWait": 0.15})).unwrap();
        assert_eq!(c.execute("tile.play", json!({"tile": "D"})).unwrap()["state"], "waiting");
        assert_eq!(c.execute("tile.play", json!({"tile": "D"})).unwrap()["state"], "waiting", "play doesn't restart the wait");
        assert_eq!(c.execute("tile.trigger", json!({"tile": "D"})).unwrap()["state"], "stopped", "a press cancels it");
        c.execute("tile.trigger", json!({"tile": "D"})).unwrap();
        assert!(c.waiting(ids[3]).is_some());
        std::thread::sleep(Duration::from_millis(200));
        c.tick(0.0);
        assert!(playing(&c, 3));
        let t = c.execute("tile.get", json!({"tile": "D"})).unwrap();
        assert_eq!((t["pre_wait"].as_f64(), t["link"].as_str()), (Some(0.15), Some("none")));
        assert_eq!(c.execute("tile.set", json!({"tile": "D", "link": "sideways"})).unwrap_err().code, "bad_params");
        assert_eq!(c.execute("tile.set", json!({"tile": "D", "preWait": -1})).unwrap_err().code, "bad_params");
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn cue_notes_and_moving_the_pointer() {
        let (mut c, root) = controller();
        let ids = add(&mut c, &root, &["One", "Two", "Three"]);
        let r = c.execute("tile.set", json!({"tile": "Two", "notes": "Standby: door slam"})).unwrap();
        assert_eq!(r["notes"], "Standby: door slam");
        c.execute("set.notes", json!({"notes": "Act 1"})).unwrap();
        assert_eq!(c.execute("set.list", json!({})).unwrap()[0]["notes"], "Act 1");

        // Next and previous move the pointer and play nothing.
        assert_eq!(c.execute("cue.next", json!({})).unwrap()["title"], "Two");
        assert_eq!(c.execute("status", json!({})).unwrap()["next_cue"]["notes"], "Standby: door slam");
        assert_eq!(c.execute("cue.next", json!({})).unwrap()["next"], 3);
        assert_eq!(c.execute("cue.next", json!({})).unwrap()["next"], Value::Null, "past the last cue");
        assert_eq!(c.execute("cue.prev", json!({})).unwrap()["title"], "Three");
        assert_eq!(c.execute("cue.prev", json!({})).unwrap()["title"], "Two");
        assert!(c.engine.statuses().is_empty(), "nothing played");
        // Picking a cue by tile, without playing it; GO then plays it.
        assert_eq!(c.execute("cue.set", json!({"tile": "Three"})).unwrap()["next"], 3);
        assert!(c.engine.statuses().is_empty());
        assert_eq!(c.execute("cue.go", json!({})).unwrap()["title"], "Three");
        assert_eq!(c.engine.state(ids[2]), PlayState::Playing);

        // Typing notes is one undo step.
        for typed in ["S", "St", "Stand"] {
            c.execute("tile.set", json!({"tile": ids[0], "notes": typed})).unwrap();
        }
        for typed in ["A", "Ac"] {
            c.execute("set.notes", json!({"notes": typed})).unwrap();
        }
        c.execute("edit.undo", json!({})).unwrap();
        assert_eq!(c.current_set().notes, "Act 1");
        c.execute("edit.undo", json!({})).unwrap();
        assert_eq!(c.store.library.tile(ids[0]).unwrap().notes, "");
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn list_sets_play_the_selection_one_cue_at_a_time() {
        let (mut c, root) = controller();
        let ids = add(&mut c, &root, &["One", "Two", "Three", "Four"]);
        c.execute("set.layout", json!({"layout": "list"})).unwrap();
        let pointer = |c: &Controller| c.cue.get(&c.current_set().id).copied().unwrap_or(0);
        let armed = |c: &Controller, i: usize| !c.played.contains(&ids[i]);
        let playing = |c: &Controller, i: usize| c.engine.state(ids[i]) == PlayState::Playing;
        let go = |c: &mut Controller| c.execute("cue.go", json!({})).unwrap()["action"].as_str().unwrap_or_default().to_owned();
        // Selecting a row makes it the cue Enter plays; it stays selected while it plays.
        c.execute("view.select", json!({"tile": "Two"})).unwrap();
        assert_eq!(pointer(&c), 1);
        assert_eq!(go(&mut c), "played");
        assert!(playing(&c, 1) && !armed(&c, 1) && armed(&c, 0));
        assert_eq!(c.selected_tile, Some(ids[1]), "the selection stays on the playing cue");
        let rows = c.execute("tile.list", json!({})).unwrap();
        assert_eq!((rows[0]["armed"].as_bool(), rows[1]["armed"].as_bool()), (Some(true), Some(false)));
        // Again while it plays: it stops. Queued then selects the next armed cue.
        assert_eq!(go(&mut c), "stopped");
        c.tick(0.15);
        assert_eq!(c.selected_tile, Some(ids[2]));
        // One at a time: playing another row stops the one playing.
        go(&mut c);
        assert!(playing(&c, 2));
        c.execute("cue.go", json!({"tile": "One"})).unwrap();
        assert!(playing(&c, 0) && !playing(&c, 2), "Three stopped for One");
        assert_eq!(c.selected_tile, Some(ids[0]));
        // Queued: when One ends, the next armed cue (Four: Two and Three have played) is
        // selected and waits.
        c.tick(0.3);
        assert_eq!(c.list_queued(c.set_idx()), Some(3), "shown while One plays");
        c.tick(0.3);
        assert!((0..4).all(|i| !playing(&c, i)));
        assert_eq!((pointer(&c), c.selected_tile), (3, Some(ids[3])));
        assert_eq!(c.list_queued(c.set_idx()), None);
        // Marks change without moving the selection; Next stays on the last row, Previous
        // steps one row back, played or not.
        assert_eq!(c.execute("cue.arm", json!({"tile": "Four"})).unwrap()["armed"], false);
        assert_eq!(c.selected_tile, Some(ids[3]));
        assert_eq!(c.execute("cue.next", json!({})).unwrap()["title"], "Four");
        assert_eq!(c.execute("cue.prev", json!({})).unwrap()["title"], "Three");
        assert_eq!(c.execute("cue.next", json!({})).unwrap()["title"], "Four");

        // Reset List: everything armed, the first row selected; nothing stops.
        c.execute("tile.play", json!({"tile": "Four"})).unwrap();
        let r = c.execute("cue.reset", json!({})).unwrap();
        assert_eq!((r["next"].as_u64(), r["title"].as_str()), (Some(1), Some("One")));
        assert!((0..4).all(|i| armed(&c, i)) && playing(&c, 3));
        assert_eq!(c.selected_tile, Some(ids[0]));
        c.execute("transport.stopAll", json!({"immediate": true})).unwrap();
        c.tick(0.01);

        // Continuous: when a cue ends, the next armed one after it plays, wherever the
        // selection went meanwhile, and is selected; a stopped cue starts nothing.
        assert_eq!(c.execute("set.list", json!({})).unwrap()[0]["list_mode"], "queued");
        assert_eq!(c.execute("set.listMode", json!({"mode": "continuous"})).unwrap()["mode"], "continuous");
        c.execute("cue.reset", json!({})).unwrap();
        go(&mut c);
        c.execute("view.select", json!({"tile": "Four"})).unwrap();
        c.tick(0.3);
        c.tick(0.3);
        assert!(!playing(&c, 0) && playing(&c, 1), "on to Two");
        assert_eq!(c.selected_tile, Some(ids[1]));
        c.execute("transport.stopAll", json!({"immediate": true})).unwrap();
        c.tick(0.01);
        c.tick(0.01);
        assert!((0..4).all(|i| !playing(&c, i)) && c.selected_tile == Some(ids[1]), "stopped: nothing starts");
        // The cross-fade: the next cue starts that long before the end, while this one
        // fades out.
        assert_eq!(c.execute("set.crossfade", json!({"seconds": 0.2})).unwrap()["seconds"], 0.2);
        c.execute("cue.reset", json!({})).unwrap();
        go(&mut c);
        c.tick(0.35);
        c.tick(0.0);
        assert!(playing(&c, 1), "Two started 0.2 s early");
        assert_eq!(c.engine.state(ids[0]), PlayState::Stopping, "One fading out");
        c.execute("transport.stopAll", json!({"immediate": true})).unwrap();
        c.tick(0.01);
        assert_eq!(c.execute("set.listMode", json!({"mode": "loop"})).unwrap_err().code, "bad_params");
        c.execute("set.lock", json!({})).unwrap();
        assert!(c.execute("set.listMode", json!({"mode": "queued"})).is_err(), "a locked set keeps its mode");
        assert!(c.execute("set.crossfade", json!({"seconds": 1})).is_err());
        c.execute("set.unlock", json!({})).unwrap();

        // A Pausable cue pauses on the second press and resumes on the third.
        c.execute("tile.set", json!({"tile": "One", "pausable": true})).unwrap();
        c.execute("cue.set", json!({"tile": "One"})).unwrap();
        assert_eq!((go(&mut c), go(&mut c), go(&mut c)), ("played".into(), "paused".into(), "resumed".into()));
        c.execute("transport.stopAll", json!({"immediate": true})).unwrap();
        c.tick(0.01);

        // In a grid set, GO plays in order and moves on; played tiles aren't passed over.
        c.execute("set.layout", json!({"layout": "grid"})).unwrap();
        c.execute("cue.top", json!({})).unwrap();
        assert_eq!(c.execute("cue.next", json!({})).unwrap()["title"], "Two");
        assert_eq!(c.execute("cue.go", json!({})).unwrap()["title"], "Two");
        assert_eq!(pointer(&c), 2);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn pause_all_and_fade_all() {
        let (mut c, root) = controller();
        let ids = add(&mut c, &root, &["One", "Two"]);
        c.execute("tile.play", json!({"tile": "One"})).unwrap();
        c.execute("tile.play", json!({"tile": "Two"})).unwrap();
        let r = c.execute("transport.pauseAll", json!({})).unwrap();
        assert_eq!((r["paused"].as_u64(), r["resumed"].as_u64()), (Some(2), Some(0)), "pausable or not");
        assert!(ids.iter().all(|&id| c.engine.state(id) == PlayState::Paused));
        let r = c.execute("transport.pauseAll", json!({})).unwrap();
        assert_eq!(r["resumed"], 2);
        assert!(ids.iter().all(|&id| c.engine.state(id) == PlayState::Playing));
        assert_eq!(c.execute("transport.fadeAll", json!({})).unwrap()["fade"], 2.0);
        assert!(ids.iter().all(|&id| c.engine.state(id) == PlayState::Stopping));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn disabled_tiles_are_skipped() {
        let (mut c, root) = controller();
        let ids = add(&mut c, &root, &["One", "Two", "Three"]);
        assert_eq!(c.execute("tile.disable", json!({"tile": "Two"})).unwrap()["disabled"], true);
        assert_eq!(c.execute("tile.get", json!({"tile": "Two"})).unwrap()["disabled"], true);
        // Triggers and plays are refused (as information, not an error); GO skips it.
        let e = c.execute("tile.trigger", json!({"tile": "Two"})).unwrap_err();
        assert!(e.code == "disabled" && e.is_info(), "{e:?}");
        assert_eq!(c.execute("tile.play", json!({"tile": "Two"})).unwrap_err().code, "disabled");
        c.execute("bind.add", json!({"source": "midi:note:40", "tile": "Two"})).unwrap();
        c.midi(&crate::bindings::parse_midi("pads", &[0x90, 40, 100]).unwrap());
        assert_eq!(c.engine.state(ids[1]), PlayState::Stopped, "a pad doesn't play it");
        assert_eq!(c.execute("cue.go", json!({})).unwrap()["title"], "One");
        assert_eq!(c.execute("cue.go", json!({})).unwrap()["title"], "Three");
        // Preview still plays it, for checking it.
        c.execute("tile.preview", json!({"tile": "Two"})).unwrap();
        assert_eq!(c.engine.state(ids[1]), PlayState::Playing);
        c.execute("transport.stopAll", json!({"immediate": true})).unwrap();
        c.tick(0.0);
        // A held tile disabled while held still stops on release.
        c.execute("tile.set", json!({"tile": "One", "mode": "hold"})).unwrap();
        c.execute("tile.trigger", json!({"tile": "One"})).unwrap();
        c.execute("tile.set", json!({"tile": "One", "disabled": true})).unwrap();
        c.execute("tile.trigger", json!({"tile": "One", "event": "release"})).unwrap();
        assert_ne!(c.engine.state(ids[0]), PlayState::Playing);
        // Enable is a bindable command, and undoable.
        c.execute("bind.add", json!({"source": "midi:note:41", "command": "tile.enable", "tile": "Two"})).unwrap();
        c.midi(&crate::bindings::parse_midi("pads", &[0x90, 41, 100]).unwrap());
        assert!(!c.store.library.tile(ids[1]).unwrap().disabled);
        c.execute("edit.undo", json!({})).unwrap();
        assert!(c.store.library.tile(ids[1]).unwrap().disabled);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn search_across_sets() {
        let (mut c, root) = controller();
        add(&mut c, &root, &["Applause", "Rimshot"]);
        c.execute("set.create", json!({"name": "Act 2"})).unwrap();
        let other = add(&mut c, &root, &["Big Applause", "Door"]);
        c.execute("tile.set", json!({"tile": "Door", "notes": "applause on the slam"})).unwrap();
        c.execute("set.select", json!({"set": "#1"})).unwrap();
        // Opening asks the window for the keyboard; typing filters every set, the
        // current one first, by title or notes.
        c.execute("view.search", json!({})).unwrap();
        assert_eq!(c.effects.last(), Some(&Effect::FocusSearch));
        let r = c.execute("view.search", json!({"query": "APPLAUSE"})).unwrap();
        let titles: Vec<&str> = r["hits"].as_array().unwrap().iter().map(|h| h["title"].as_str().unwrap()).collect();
        assert_eq!(titles, ["Applause", "Big Applause", "Door"]);
        assert_eq!(c.execute("tile.list", json!({"set": "*", "query": "slam"})).unwrap()[0]["title"], "Door", "tile.list matches the same way");
        // Enter plays the top hit and closes the box.
        c.execute("set.select", json!({"set": "Act 2"})).unwrap();
        c.execute("view.search", json!({"query": "applause"})).unwrap();
        let r = c.execute("view.search", json!({"play": true})).unwrap();
        assert_eq!(r["open"], false);
        assert_eq!(c.engine.state(other[0]), PlayState::Playing, "the current set's hit comes first");
        c.execute("tile.disable", json!({"tile": "Applause"})).unwrap();
        c.execute("set.select", json!({"set": "#1"})).unwrap();
        c.execute("view.search", json!({"query": "applause"})).unwrap();
        c.execute("view.search", json!({"play": true})).unwrap();
        assert_eq!(c.engine.state(c.resolve_tile("Applause").unwrap()), PlayState::Stopped, "a disabled hit is passed over");
        c.execute("view.search", json!({"query": "zzz"})).unwrap();
        assert_eq!(c.execute("view.search", json!({"play": true})).unwrap_err().code, "not_found");
        c.execute("view.search", json!({"close": true})).unwrap();
        assert!(c.search.is_none());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn set_switching_and_keys_in_every_set() {
        let (mut c, root) = controller();
        let first = add(&mut c, &root, &["Horn", "Bed"]);
        c.execute("set.create", json!({"name": "Act 2"})).unwrap();
        let second = add(&mut c, &root, &["Thunder", "Rain", "Wind"]);
        c.execute("set.create", json!({"name": "Act 3"})).unwrap();
        assert_eq!(c.execute("set.next", json!({})).unwrap_err().code, "end_of_sets");
        assert_eq!(c.execute("set.prev", json!({})).unwrap()["name"], "Act 2");
        assert_eq!(c.execute("set.prev", json!({})).unwrap()["name"], "Set 1");
        let e = c.execute("set.prev", json!({})).unwrap_err();
        assert!(e.is_info(), "stopping at the end isn't a fault");

        // Keys: "3" is only in Act 2, "1" is in both.
        assert_eq!(c.tile_for_key(PadKey::plain('3')), None, "off: only the current set");
        c.execute("settings.tileKeysAllSets", json!({"on": true})).unwrap();
        assert_eq!(c.tile_for_key(PadKey::plain('3')), Some(second[2]));
        assert_eq!(c.tile_for_key(PadKey::plain('1')), Some(first[0]), "the current set wins");
        c.execute("set.next", json!({})).unwrap();
        assert_eq!(c.tile_for_key(PadKey::plain('1')), Some(second[0]));
        assert_eq!(c.resolve_tile("key:3").unwrap(), second[2], "key: refs follow the setting");
        assert_eq!(c.execute("settings.tileKeysAllSets", json!({})).unwrap()["on"], false, "no on: toggles");
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn random_tiles_play_each_once_before_repeating() {
        let (mut c, root) = controller();
        let ids = add(&mut c, &root, &["A", "B", "C", "D"]);
        c.execute("tile.disable", json!({"tile": "D"})).unwrap();
        for round in 0..3 {
            let mut seen = std::collections::HashSet::new();
            for _ in 0..3 {
                let r = c.execute("tile.random", json!({})).unwrap();
                assert!(seen.insert(r["tile"].as_str().unwrap().to_owned()), "round {round}: a repeat before all played");
            }
            assert!(!seen.contains(&ids[3].to_string()), "disabled tiles are left out");
        }
        // A colour narrows the pool; a pool of one plays that one every time.
        c.execute("tile.set", json!({"tile": "B", "color": "pink"})).unwrap();
        for _ in 0..3 {
            assert_eq!(c.execute("tile.random", json!({"color": "pink"})).unwrap()["title"], "B");
        }
        assert_eq!(c.execute("tile.random", json!({"color": "gray"})).unwrap_err().code, "not_found");
        // Bindable without a tile.
        c.execute("bind.add", json!({"source": "midi:note:50", "command": "tile.random"})).unwrap();
        std::fs::remove_dir_all(root).ok();
    }

    /// The Random button (D-171): never the last pick, even across rounds; disabled and
    /// empty tiles are skipped; a locked set still plays; it says what it played.
    #[test]
    fn random_never_repeats_the_last_pick_and_says_what_it_played() {
        let (mut c, root) = controller();
        let ids = add(&mut c, &root, &["A", "B", "C"]);
        c.execute("tile.disable", json!({"tile": "C"})).unwrap();
        // An empty tile (no sound yet) is left out too.
        c.store.library.sets[0].tiles.push(Tile { title: "Empty".into(), ..Tile::default() });
        c.execute("set.lock", json!({})).unwrap();
        let mut last = String::new();
        for _ in 0..40 {
            let r = c.execute("tile.random", json!({})).unwrap();
            let pick = r["tile"].as_str().unwrap().to_owned();
            assert_ne!(pick, last, "never the one it picked last time");
            assert!(pick == ids[0].to_string() || pick == ids[1].to_string(), "only enabled tiles with a sound: {}", r["title"]);
            let title = r["title"].as_str().unwrap();
            assert_eq!(c.notice.as_ref().map(|n| n.text.as_str()), Some(format!("Random: playing {title}").as_str()));
            assert_eq!(c.announcement.as_ref().map(|a| a.text.as_str()), Some(format!("Random: playing {title}").as_str()));
            last = pick;
        }
        assert!(!crate::osc::refused("tile.random"), "OSC can run it (/ls/cmd tile.random)");
        // A set with one playable tile plays it every time.
        c.execute("set.unlock", json!({})).unwrap();
        c.execute("tile.disable", json!({"tile": "B"})).unwrap();
        for _ in 0..3 {
            assert_eq!(c.execute("tile.random", json!({})).unwrap()["title"], "A");
        }
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn several_tiles_edit_as_one_step() {
        let (mut c, root) = controller();
        let ids = add(&mut c, &root, &["A", "B", "C", "D"]);
        // Click A, Shift-click C, ⌘-click D, ⌘-click B off again.
        c.execute("view.select", json!({"tile": "A"})).unwrap();
        assert_eq!(c.execute("view.select", json!({"tile": "C", "range": true})).unwrap()["selected"], json!([ids[0], ids[1], ids[2]]));
        c.execute("view.select", json!({"tile": "D", "add": true})).unwrap();
        let r = c.execute("view.select", json!({"tile": "B", "add": true})).unwrap();
        assert_eq!(r["selected"], json!([ids[0], ids[2], ids[3]]));
        assert!(c.is_selected(ids[3]) && !c.is_selected(ids[1]));

        // One tile.set over the selection is one undo step.
        let sel = c.selected();
        c.execute("tile.set", json!({"tiles": sel, "volume": 0.5, "loop": true})).unwrap();
        c.execute("tile.set", json!({"tiles": sel, "color": "teal"})).unwrap();
        for id in [ids[0], ids[2], ids[3]] {
            let t = c.store.library.tile(id).unwrap();
            assert_eq!((t.volume, t.looping, t.color), (0.5, true, TileColor::Teal));
        }
        assert_eq!(c.store.library.tile(ids[1]).unwrap().volume, 1.0, "B wasn't selected");
        c.execute("edit.undo", json!({})).unwrap();
        assert!(ids.iter().all(|id| c.store.library.tile(*id).unwrap().volume == 1.0), "the drag and the colour, one step");

        // All or nothing; no shared hotkey.
        assert_eq!(c.execute("tile.set", json!({"tiles": sel, "hotkey": "q"})).unwrap_err().code, "bad_params");
        c.execute("tile.set", json!({"tile": "C", "trimEnd": 0.3})).unwrap();
        assert_eq!(c.execute("tile.set", json!({"tiles": sel, "trimStart": 0.35})).unwrap_err().code, "bad_params");
        assert_eq!(c.store.library.tile(ids[0]).unwrap().start, 0.0, "A rolled back when C refused");
        // A plain click selects one again; another set's tile can't extend a range.
        c.execute("view.select", json!({"tile": "B"})).unwrap();
        assert_eq!(c.selected(), [ids[1]]);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn shift_click_selects_a_rectangle_from_the_cli() {
        let (mut c, root) = controller();
        let ids = add(&mut c, &root, &["A", "B", "C", "D", "E", "F", "G", "H", "I"]);
        // [A B C] / [D E F] / [G _ H I]
        c.execute("set.rows", json!({"rows": [3, 3, 3]})).unwrap();
        c.execute("set.addBlank", json!({"row": 3, "slot": 2})).unwrap();
        let titles = |c: &Controller| c.selected().iter().map(|id| c.store.library.tile(*id).unwrap().title.clone()).collect::<Vec<_>>().join(" ");
        // Shift-click: the rectangle between the selected tile and this one, not a run.
        c.execute("view.select", json!({"tile": "B"})).unwrap();
        c.execute("view.select", json!({"tile": "H", "range": true})).unwrap();
        assert_eq!(titles(&c), "B C E F H", "rows 1-3 x slots 2-3; the blank is skipped");
        assert_eq!(c.selected_tile, Some(ids[1]), "the anchor stays in the inspector");
        // From the other corner, and Shift-click again: a new rectangle from the same anchor.
        c.execute("view.select", json!({"tile": "D", "range": true})).unwrap();
        assert_eq!(titles(&c), "A B D E");
        // ⌘-click a new anchor, ⌘-Shift-click: that rectangle is added.
        c.execute("view.select", json!({"tile": "I", "add": true})).unwrap();
        c.execute("view.select", json!({"tile": "F", "add": true, "range": true})).unwrap();
        assert_eq!(titles(&c), "A B D E I F H", "F to I is F, H and I; I was in already");
        // In lines of 4 (Auto/Fixed columns: A B C D / E F G H / I), and one column (the run).
        c.execute("view.select", json!({"tile": "A"})).unwrap();
        c.execute("view.select", json!({"tile": "I", "range": true, "columns": 4})).unwrap();
        assert_eq!(titles(&c), "A E I");
        c.execute("view.select", json!({"tile": "C", "range": true, "columns": 1})).unwrap();
        assert_eq!(titles(&c), "A B C");
        // With nothing selected, Shift-click selects the one tile.
        c.execute("view.select", json!({})).unwrap();
        c.execute("view.select", json!({"tile": "E", "range": true})).unwrap();
        assert_eq!(titles(&c), "E");
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn action_tiles_fade_stop_pause_and_release() {
        let (mut c, root) = controller();
        let ids = add(&mut c, &root, &["Music", "Rain", "Bed"]);
        c.execute("set.create", json!({"name": "Act 2"})).unwrap();
        let act2 = add(&mut c, &root, &["Thunder"]);
        c.execute("set.select", json!({"set": "#1"})).unwrap();
        let fade = c.execute("tile.addAction", json!({"targets": ["Music", "Rain"], "fade": 2})).unwrap()["id"].as_str().unwrap().to_owned();
        let r = c.execute("tile.get", json!({"tile": fade})).unwrap();
        assert_eq!((r["title"].as_str(), r["action"]["kind"].as_str(), r["action"]["fade"].as_f64()), (Some("Fade out Music"), Some("fade"), Some(2.0)));
        assert_eq!(c.selected_tile.map(|i| i.to_string()), Some(fade.clone()), "the inspector opens on it");
        assert_eq!(c.execute("tile.preview", json!({"tile": fade})).unwrap_err().code, "no_audio");
        for t in ["Music", "Rain", "Bed"] {
            c.execute("tile.play", json!({"tile": t})).unwrap();
        }
        c.execute("tile.trigger", json!({"tile": fade})).unwrap();
        assert_eq!(c.engine.state(ids[0]), PlayState::Stopping, "fading out");
        assert_eq!(c.engine.state(ids[1]), PlayState::Stopping);
        assert_eq!(c.engine.state(ids[2]), PlayState::Playing, "not a target");

        // GO stops at action tiles too; this one stops every tile in another set.
        let stop = c.execute("tile.addAction", json!({"kind": "stop", "targets": "set:Act 2", "at": 1})).unwrap()["id"].as_str().unwrap().to_owned();
        assert_eq!(c.execute("tile.get", json!({"tile": stop})).unwrap()["title"], "Stop Act 2");
        c.execute("tile.play", json!({"tile": "Act 2/Thunder"})).unwrap();
        c.execute("cue.top", json!({})).unwrap();
        assert_eq!(c.execute("cue.go", json!({})).unwrap()["title"], "Stop Act 2");
        assert_ne!(c.engine.state(act2[0]), PlayState::Playing);
        assert_eq!(c.execute("cue.go", json!({})).unwrap()["title"], "Music", "on to the next cue");

        // Change it into a pause, and back to an empty tile.
        c.execute("tile.action", json!({"tile": stop, "kind": "pause", "targets": ["Bed"]})).unwrap();
        c.execute("tile.trigger", json!({"tile": stop})).unwrap();
        assert_eq!(c.engine.state(ids[2]), PlayState::Paused);
        assert_eq!(c.execute("tile.action", json!({"tile": "Bed", "kind": "stop"})).unwrap_err().code, "bad_params", "audio tiles can't");
        assert_eq!(c.execute("tile.action", json!({"tile": stop, "clear": true})).unwrap()["action"], Value::Null);
        assert_eq!(c.execute("tile.random", json!({})).unwrap()["tile"].as_str().map(|s| s != fade), Some(true), "random never picks an action");
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn group_tiles_fire_their_targets_with_offsets() {
        let (mut c, root) = controller();
        let ids = add(&mut c, &root, &["Thunder", "Rain", "Wind"]);
        let g = c.execute("tile.addAction", json!({"kind": "play", "targets": ["Thunder", "Rain@0.15", "Wind@60"]})).unwrap()["id"].as_str().unwrap().to_owned();
        let r = c.execute("tile.get", json!({"tile": g})).unwrap();
        assert_eq!((r["title"].as_str(), r["action"]["targets"][1]["offset"].as_f64()), (Some("Group: Thunder"), Some(0.15)));
        c.execute("tile.trigger", json!({"tile": g})).unwrap();
        assert_eq!(c.engine.state(ids[0]), PlayState::Playing, "no offset: at once");
        assert_eq!(c.engine.state(ids[1]), PlayState::Stopped, "waiting for its offset");
        assert_eq!(c.cue.get(&c.current_set().id), Some(&4), "the group's cue moved the pointer past it, not its targets");
        std::thread::sleep(Duration::from_millis(200));
        c.tick(0.0);
        assert_eq!(c.engine.state(ids[1]), PlayState::Playing);
        // Stop All drops what's still waiting.
        c.execute("transport.stopAll", json!({"immediate": true})).unwrap();
        assert!(c.pending.is_empty());
        // A group that contains itself stops instead of looping forever.
        c.execute("tile.action", json!({"tile": g, "targets": [g.clone()]})).unwrap();
        c.execute("tile.trigger", json!({"tile": g})).unwrap();
        assert!(c.notice.as_ref().is_some_and(|n| n.error && n.text.contains("fires itself")));
        assert_eq!(c.execute("tile.action", json!({"tile": g, "targets": ["Rain@-1"]})).unwrap_err().code, "bad_params");
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn gain_and_normalise() {
        let (mut c, root) = controller();
        let paths: Vec<String> = [("Loud", 0.8), ("Quiet", 0.1)]
            .iter()
            .map(|(n, amp)| {
                let p = root.join(format!("{n}.wav"));
                wav::write_tone(&p, 8000, 1.0, &[440.0], *amp).unwrap();
                p.to_string_lossy().into_owned()
            })
            .collect();
        c.execute("tile.add", json!({"paths": paths})).unwrap();
        wait_loaded(&mut c);

        assert_eq!(c.execute("tile.set", json!({"tile": "Loud", "gainDb": 30})).unwrap_err().code, "bad_params");
        assert_eq!(c.execute("tile.set", json!({"tile": "Loud", "gainDb": -3.5})).unwrap()["gain_db"], -3.5);

        // The whole set to −16 LUFS: the quiet clip gets about 18 dB more than the loud one.
        let r = c.execute("tile.normalize", json!({})).unwrap();
        let tiles = r["tiles"].as_array().unwrap();
        assert_eq!(tiles.len(), 2, "{r}");
        for t in tiles {
            let (m, g) = (t["measured"].as_f64().unwrap(), t["gain_db"].as_f64().unwrap());
            assert!((m + g + 16.0).abs() < 0.11, "{t}");
        }
        let gain = |c: &mut Controller, t: &str| c.execute("tile.get", json!({"tile": t})).unwrap()["gain_db"].as_f64().unwrap();
        let diff = gain(&mut c, "Quiet") - gain(&mut c, "Loud");
        assert!((diff - 18.06).abs() < 0.3, "{diff}");

        // One tile to a peak level.
        let r = c.execute("tile.normalize", json!({"tile": "Loud", "target": "peak", "level": -3})).unwrap();
        assert!((r["tiles"][0]["measured"].as_f64().unwrap() - 20.0 * 0.8f64.log10()).abs() < 0.2, "{r}");
        assert!((gain(&mut c, "Loud") - (-3.0 - 20.0 * 0.8f64.log10())).abs() < 0.2);
        assert_eq!(c.execute("tile.normalize", json!({"target": "rms"})).unwrap_err().code, "bad_params");
        assert_eq!(c.execute("tile.normalize", json!({"level": 3})).unwrap_err().code, "bad_params");
    }

    #[test]
    fn a_tiles_emoji_is_set_cleared_and_undone() {
        let (mut c, root) = controller();
        add(&mut c, &root, &["Dog", "Cat"]);
        assert_eq!(c.execute("tile.get", json!({"tile": "Dog"})).unwrap()["emoji"], "");
        assert_eq!(c.execute("tile.set", json!({"tile": "Dog", "emoji": " 🐕\u{200D}\u{1F9BA} "})).unwrap()["emoji"], "🐕\u{200D}\u{1F9BA}");
        assert_eq!(c.execute("tile.list", json!({})).unwrap()[0]["emoji"], "🐕\u{200D}\u{1F9BA}");
        assert_eq!(c.execute("tile.get", json!({"tile": "Dog"})).unwrap()["title"], "Dog", "the title is untouched");
        for bad in [json!("dog"), json!("🐕 dog"), json!("🐕".repeat(30)), json!(3)] {
            assert_eq!(c.execute("tile.set", json!({"tile": "Cat", "emoji": bad})).unwrap_err().code, "bad_params", "{bad}");
        }
        c.execute("edit.undo", json!({})).unwrap();
        assert_eq!(c.execute("tile.get", json!({"tile": "Dog"})).unwrap()["emoji"], "", "one undo step");
        c.execute("tile.set", json!({"tile": "Dog", "emoji": "🐕\u{200D}\u{1F9BA}"})).unwrap();
        assert_eq!(c.execute("tile.set", json!({"tile": "Dog", "emoji": ""})).unwrap()["emoji"], "");
        c.execute("tile.set", json!({"tile": "Dog", "emoji": "🐶"})).unwrap();
        assert_eq!(c.execute("tile.set", json!({"tile": "Dog", "emoji": null})).unwrap()["emoji"], "", "null clears it too");
    }

    #[test]
    fn mute_is_a_tile_setting_with_undo() {
        let (mut c, root) = controller();
        add(&mut c, &root, &["Bed", "Voice"]);
        c.execute("tile.set", json!({"tile": "Bed", "volume": 0.6})).unwrap();
        assert_eq!(c.execute("tile.get", json!({"tile": "Bed"})).unwrap()["muted"], false);
        let r = c.execute("tile.set", json!({"tile": "Bed", "muted": true})).unwrap();
        assert_eq!((r["muted"].as_bool(), r["volume"].as_f64().map(|v| (v * 10.0).round())), (Some(true), Some(6.0)), "the level is kept: {r}");
        assert_eq!(c.execute("tile.list", json!({})).unwrap()[0]["muted"], true);
        assert_eq!(c.execute("tile.set", json!({"tile": "Voice", "muted": "loud"})).unwrap_err().code, "bad_params");
        c.execute("edit.undo", json!({})).unwrap();
        assert_eq!(c.execute("tile.get", json!({"tile": "Bed"})).unwrap()["muted"], false, "one undo step");
        // A selection, as one step.
        c.execute("tile.set", json!({"tiles": ["Bed", "Voice"], "muted": true})).unwrap();
        assert!(c.execute("tile.list", json!({})).unwrap().as_array().unwrap().iter().all(|t| t["muted"] == true));
    }

    #[test]
    fn ducking_commands() {
        let (mut c, root) = controller();
        add(&mut c, &root, &["Bed", "Voice"]);
        assert_eq!(c.execute("tile.set", json!({"tile": "Bed", "duck": "ducked"})).unwrap()["duck"], "ducked");
        assert_eq!(c.execute("tile.set", json!({"tile": "Voice", "duck": "loud"})).unwrap_err().code, "bad_params");
        // A bed's own level (D-245): dB, a number as text from the CLI, or back to default.
        assert_eq!(c.execute("tile.get", json!({"tile": "Bed"})).unwrap()["duck_level"], "default");
        assert_eq!(c.execute("tile.set", json!({"tile": "Bed", "duckLevel": -20})).unwrap()["duck_level"], -20.0);
        assert_eq!(c.execute("tile.set", json!({"tile": "Bed", "duck_level": "-6.5"})).unwrap()["duck_level"], -6.5);
        assert_eq!(c.execute("tile.list", json!({})).unwrap()[0]["duck_level"], -6.5);
        for bad in [json!(-41), json!(3), json!("loud")] {
            assert_eq!(c.execute("tile.set", json!({"tile": "Bed", "duckLevel": bad})).unwrap_err().code, "bad_params", "{bad}");
        }
        assert_eq!(c.execute("tile.set", json!({"tile": "Bed", "duckLevel": "Default"})).unwrap()["duck_level"], "default");
        c.execute("tile.set", json!({"tile": "Bed", "duckLevel": -9})).unwrap();
        assert_eq!(c.execute("tile.set", json!({"tile": "Bed", "duckLevel": null})).unwrap()["duck_level"], "default");
        assert_eq!(c.execute("duck.hold", json!({"on": true})).unwrap()["on"], true);
        assert_eq!(c.execute("status", json!({})).unwrap()["duck_hold"], true);
        assert_eq!(c.execute("duck.hold", json!({})).unwrap()["on"], false, "no on: toggles");
        c.execute("settings.duck", json!({"amount": -18, "release": 1.5})).unwrap();
        let lib = &c.store.library;
        assert_eq!((lib.duck_db, lib.duck_attack, lib.duck_release), (-18.0, 0.05, 1.5));
        assert_eq!(c.execute("settings.duck", json!({"amount": 6})).unwrap_err().code, "bad_params");
    }

    #[test]
    fn undo_and_redo() {
        let (mut c, root) = controller();
        let ids = add(&mut c, &root, &["A", "B"]);
        let media = |c: &Controller, id: Uuid| c.store.media_dir.join(c.store.library.tile(id).unwrap().file.clone().unwrap());
        let file_a = media(&c, ids[0]);

        // Removing keeps the clip on disk (and decoded) while undo can bring it back.
        c.execute("tile.remove", json!({"tile": "A"})).unwrap();
        assert!(file_a.exists());
        assert_eq!(c.history.undo_label(), Some("Remove tile"));
        assert_eq!(c.execute("edit.undo", json!({})).unwrap()["undid"], "Remove tile");
        assert_eq!(c.current_set().tiles.iter().map(|t| t.title.as_str()).collect::<Vec<_>>(), ["A", "B"]);
        assert!(c.store.audio(c.store.library.tile(ids[0]).unwrap()).is_some(), "still decoded");
        c.execute("edit.redo", json!({})).unwrap();
        assert!(c.store.library.tile(ids[0]).is_none());
        c.execute("edit.undo", json!({})).unwrap();

        // A slider drag is one step; another tile or a pause starts a new one.
        for v in [0.9, 0.8, 0.7] {
            c.execute("tile.set", json!({"tile": "A", "volume": v})).unwrap();
        }
        c.execute("tile.set", json!({"tile": "B", "volume": 0.5})).unwrap();
        c.execute("edit.undo", json!({})).unwrap();
        assert_eq!(c.store.library.tile(ids[1]).unwrap().volume, 1.0);
        assert_eq!(c.store.library.tile(ids[0]).unwrap().volume, 0.7);
        c.execute("edit.undo", json!({})).unwrap();
        assert_eq!(c.store.library.tile(ids[0]).unwrap().volume, 1.0, "the whole drag undone at once");

        // Settings and the master volume aren't undone; a failed command records nothing.
        c.execute("master.volume", json!({"volume": 0.5})).unwrap();
        c.execute("tile.set", json!({"tile": "A", "volume": 7})).unwrap_err();
        c.execute("edit.redo", json!({})).unwrap();
        assert_eq!(c.store.library.master_volume, 0.5);
        assert_eq!(c.store.library.tile(ids[0]).unwrap().volume, 0.7);

        // A new change after undoing drops the redo steps.
        c.execute("tile.remove", json!({"tile": "B"})).unwrap();
        c.execute("edit.undo", json!({})).unwrap();
        c.execute("set.create", json!({"name": "New"})).unwrap();
        assert!(c.history.redo_label().is_none());
        assert!(media(&c, ids[1]).exists());
        assert_eq!(c.execute("edit.redo", json!({})).unwrap_err().code, "nothing_to_undo");
    }

    #[test]
    fn undone_additions_are_deleted_once_redo_is_gone() {
        let (mut c, root) = controller();
        let ids = add(&mut c, &root, &["A"]);
        let file = c.store.media_dir.join(c.store.library.tile(ids[0]).unwrap().file.clone().unwrap());
        c.execute("edit.undo", json!({})).unwrap();
        assert!(c.current_set().tiles.is_empty());
        assert!(file.exists(), "redo can still bring it back");
        c.execute("set.rename", json!({"name": "Renamed"})).unwrap();
        assert!(!file.exists(), "no step refers to it any more");
    }

    #[test]
    fn midi_bindings_learn_and_auto_map() {
        use crate::bindings::parse_midi;
        let (mut c, root) = controller();
        let ids = add(&mut c, &root, &["Horn", "Bed", "Third"]);
        let note = |n: u8, vel: u8| parse_midi("nanoPAD2", &[0x99, n, vel]).unwrap();
        let state = |c: &Controller, i: usize| c.engine.state(ids[i]);

        // Learn: the next pad becomes the binding, stored by tile id.
        c.execute("bind.learn", json!({"tile": "Horn"})).unwrap();
        c.midi(&note(40, 100));
        assert!(c.learning.is_none());
        let list = c.execute("bind.list", json!({})).unwrap();
        assert_eq!(list[0]["what"], "Note 40 (E2) · ch 10", "{list}");
        assert_eq!(list[0]["params"]["tile"], ids[0].to_string());
        assert_eq!(c.tile_bindings(ids[0]).len(), 1);
        // Note on plays, note off is a release (Play/Stop ignores it), the next press stops.
        c.midi(&note(40, 90));
        assert_eq!(state(&c, 0), PlayState::Playing);
        c.midi(&note(40, 0));
        assert_eq!(state(&c, 0), PlayState::Playing);
        c.midi(&note(40, 90));
        assert_ne!(state(&c, 0), PlayState::Playing);
        // Learning the pad again for the same tile doesn't duplicate it.
        c.execute("bind.learn", json!({"tile": "Horn"})).unwrap();
        c.midi(&note(40, 100));
        assert_eq!(c.store.library.bindings.len(), 1);

        // A hold-to-play tile follows the pad.
        c.execute("tile.set", json!({"tile": "Bed", "mode": "hold"})).unwrap();
        c.execute("bind.add", json!({"source": "midi:note:41", "tile": "Bed"})).unwrap();
        c.midi(&note(41, 100));
        assert_eq!(state(&c, 1), PlayState::Playing);
        c.midi(&note(41, 0));
        assert_ne!(state(&c, 1), PlayState::Playing);

        // A fader: CC 7 sets the master volume; a CC button fires once per push.
        c.execute("bind.add", json!({"source": "midi:cc:7", "command": "master.volume", "value": "volume"})).unwrap();
        c.midi(&parse_midi("x", &[0xB0, 7, 127]).unwrap());
        assert!((c.store.library.master_volume - 1.0).abs() < 1e-3);
        c.midi(&parse_midi("x", &[0xB0, 7, 0]).unwrap());
        assert_eq!(c.store.library.master_volume, 0.0);
        c.execute("bind.add", json!({"source": "midi:cc:20", "command": "duck.hold"})).unwrap();
        c.midi(&parse_midi("x", &[0xB0, 20, 127]).unwrap());
        assert!(c.engine.duck_hold());
        c.midi(&parse_midi("x", &[0xB0, 20, 100]).unwrap());
        assert!(c.engine.duck_hold(), "still pushed: no new fire");
        c.midi(&parse_midi("x", &[0xB0, 20, 0]).unwrap());
        assert!(!c.engine.duck_hold());

        // Auto-map: unbound notes from 36 play tiles by position.
        c.execute("midi.autoMap", json!({"base": 36})).unwrap();
        c.midi(&note(38, 100));
        assert_eq!(state(&c, 2), PlayState::Playing);
        c.midi(&note(60, 100)); // past the last tile: nothing
        c.execute("midi.enable", json!({"on": false})).unwrap();
        c.midi(&note(41, 100));
        assert_ne!(state(&c, 1), PlayState::Playing, "MIDI off");

        // Validation and removal.
        assert_eq!(c.execute("bind.add", json!({"source": "midi:note:300"})).unwrap_err().code, "bad_params");
        assert_eq!(c.execute("bind.add", json!({"source": "midi:note:1", "command": "tile.trigger"})).unwrap_err().code, "bad_params");
        assert_eq!(c.execute("bind.add", json!({"source": "midi:note:1", "command": "bind.list"})).unwrap_err().code, "bad_params");
        assert_eq!(c.execute("bind.remove", json!({"tile": "Horn"})).unwrap()["removed"], 1);
        assert_eq!(c.execute("bind.remove", json!({"all": true})).unwrap()["removed"], 3);
    }

    #[test]
    fn system_wide_key_bindings() {
        let (mut c, root) = controller();
        let ids = add(&mut c, &root, &["Horn", "Bed"]);
        let id = |k: &str| crate::hotkeys::id_of(k).unwrap();

        // Learning a key: the window hands over the combination it caught.
        c.execute("bind.learn", json!({"tile": "Horn", "from": "key"})).unwrap();
        c.midi(&crate::bindings::parse_midi("pads", &[0x90, 36, 100]).unwrap());
        assert!(c.learning.is_some(), "a MIDI pad doesn't finish a key learn");
        c.learn_key("Q");
        assert!(c.learning.is_some(), "a bare letter is refused and the learn keeps waiting");
        c.learn_key("Ctrl+Alt+1");
        assert!(c.learning.is_none());
        assert_eq!(c.execute("bind.list", json!({})).unwrap()[0]["what"], "Ctrl+Alt+1");

        c.hotkey(id("alt+ctrl+Digit1"), true);
        assert_eq!(c.engine.state(ids[0]), PlayState::Playing);
        c.hotkey(id("Ctrl+Alt+2"), true);
        assert_eq!(c.engine.state(ids[1]), PlayState::Stopped, "unbound combination");

        // Hold and the talk-over follow the key.
        c.execute("tile.set", json!({"tile": "Bed", "mode": "hold"})).unwrap();
        c.execute("bind.add", json!({"source": "key:F13", "tile": "Bed"})).unwrap();
        c.execute("bind.add", json!({"source": "key:Ctrl+Shift+T", "command": "duck.hold"})).unwrap();
        c.hotkey(id("F13"), true);
        c.hotkey(id("Ctrl+Shift+T"), true);
        assert_eq!(c.engine.state(ids[1]), PlayState::Playing);
        assert!(c.engine.duck_hold());
        c.hotkey(id("F13"), false);
        c.hotkey(id("Ctrl+Shift+T"), false);
        assert_ne!(c.engine.state(ids[1]), PlayState::Playing);
        assert!(!c.engine.duck_hold());

        assert_eq!(c.execute("bind.add", json!({"source": "key:Q", "tile": "Bed"})).unwrap_err().code, "bad_params");
        c.has_window = false;
        assert_eq!(c.execute("bind.learn", json!({"tile": "Horn", "from": "key"})).unwrap_err().code, "no_window");
        assert_eq!(c.execute("bind.learn", json!({"tile": "Horn", "from": "pedal"})).unwrap_err().code, "bad_params");
    }

    #[test]
    fn voice_ducking_settings() {
        let (mut c, _root) = controller();
        let r = c.execute("duck.voice", json!({"on": true, "threshold": -35})).unwrap();
        assert_eq!((r["on"].as_bool(), r["threshold"].as_f64()), (Some(true), Some(-35.0)));
        assert!(c.input.is_none(), "no audio device in tests, so the microphone stays closed");
        assert_eq!(c.execute("duck.voice", json!({"threshold": 5})).unwrap_err().code, "bad_params");
        c.execute("settings.inputDevice", json!({"name": "USB Mic"})).unwrap();
        assert_eq!(c.store.library.input_device.as_deref(), Some("USB Mic"));
        c.execute("duck.voice", json!({"on": false})).unwrap();
        assert!(!c.store.library.voice_duck);
    }

    /// t-m20-input-list: the input menu had only "System default input" until Refresh.
    /// The inputs are listed once something wants the input, never before.
    #[test]
    fn inputs_are_listed_once_the_input_is_wanted() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static LISTED: AtomicUsize = AtomicUsize::new(0);
        fn lister() -> (Vec<String>, Option<String>) {
            LISTED.fetch_add(1, Ordering::SeqCst);
            (vec!["Fake Mic".into(), "USB Mic".into()], Some("Fake Mic".into()))
        }
        let (mut c, root) = controller();
        c.input_lister = lister;
        c.input_opener = Arc::new(|device, voice| Ok(Input::fake("Fake Mic", device, voice)));
        // No audio device (the offline CLI, screenshots): nothing is listed.
        c.execute("duck.voice", json!({"on": true})).unwrap();
        c.list_inputs_once();
        assert!(c.input_devices.is_empty());
        c.execute("duck.voice", json!({"on": false})).unwrap();
        // At start-up and while nothing wants the input, nothing is listed either.
        c.engine.device_name = Some("Fake Speakers".into());
        c.tick(0.01);
        assert_eq!(LISTED.load(Ordering::SeqCst), 0);
        assert!(c.input_devices.is_empty());
        // Voice ducking on: listed at once, and only once.
        c.execute("duck.voice", json!({"on": true})).unwrap();
        assert_eq!(c.input_devices, ["Fake Mic", "USB Mic"]);
        c.tick(0.01);
        c.list_inputs_once();
        assert_eq!(LISTED.load(Ordering::SeqCst), 1);
        // Instant replay wants the input too, in a fresh controller.
        let (mut c2, root2) = controller();
        c2.input_lister = lister;
        c2.input_opener = Arc::new(|device, voice| Ok(Input::fake("Fake Mic", device, voice)));
        c2.engine.device_name = Some("Fake Speakers".into());
        c2.execute("replay.enable", json!({"seconds": 30})).unwrap();
        c2.tick(0.01);
        assert_eq!(c2.input_devices, ["Fake Mic", "USB Mic"]);
        // Refresh lists again.
        c.execute("input.list", json!({})).unwrap();
        assert_eq!(LISTED.load(Ordering::SeqCst), 3);
        std::fs::remove_dir_all(root).ok();
        std::fs::remove_dir_all(root2).ok();
    }

    #[test]
    fn a_recording_waits_for_the_input_to_open() {
        let (mut c, root) = controller();
        let ids = add(&mut c, &root, &["Old"]);
        c.engine.device_name = Some("Fake Speakers".into());
        let (answer, prompt) = std::sync::mpsc::channel::<bool>();
        let prompt = std::sync::Mutex::new(prompt);
        c.input_opener = Arc::new(move |device, voice| match prompt.lock().unwrap().recv() {
            Ok(true) => Ok(Input::fake("Fake Mic", device, voice)),
            _ => Err(anyhow::anyhow!("microphone access was denied")),
        });
        // Allowed: the recording starts once the input arrives, at its rate.
        let r = c.execute("record.start", json!({"tile": ids[0].to_string()})).unwrap();
        assert_eq!((r["recording"].as_bool(), r["waiting"].as_bool()), (Some(true), Some(true)));
        answer.send(true).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while c.input.is_none() {
            assert!(Instant::now() < deadline, "the input never arrived");
            std::thread::sleep(Duration::from_millis(10));
            c.tick(0.01);
        }
        assert_eq!(c.recording.as_ref().map(|r| r.rate), Some(48_000));
        assert!(c.notice.as_ref().is_some_and(|n| n.text == "Recording from Fake Mic into Old"));
        c.execute("record.stop", json!({})).unwrap_err();
        c.execute("replay.enable", json!({"seconds": 0})).ok();
        c.tick(0.01);
        assert!(c.input.is_none(), "closed again once nothing needs it");
        // Denied: the new tile it would have filled goes again, and the reason shows.
        let n = c.current_set().tiles.len();
        c.input_tried = None;
        assert_eq!(c.execute("record.start", json!({})).unwrap()["waiting"], true);
        assert_eq!(c.current_set().tiles.len(), n + 1);
        answer.send(false).unwrap();
        while c.input_waiting() {
            assert!(Instant::now() < deadline, "the open never finished");
            std::thread::sleep(Duration::from_millis(10));
            c.tick(0.01);
        }
        assert!(c.recording.is_none());
        assert_eq!(c.current_set().tiles.len(), n);
        assert_eq!(c.input_note().as_deref(), Some("Couldn't open the input: microphone access was denied"));
        std::fs::remove_dir_all(root).ok();
    }

    /// t-m20-record-ux (D-127): Record New Tile opens its tile in the inspector and has the
    /// board scroll to it, and the window can tell from which input it records.
    #[test]
    fn record_new_tile_is_selected_and_shown() {
        let (mut c, root) = controller();
        add(&mut c, &root, &["Old"]);
        c.engine.device_name = Some("Fake Speakers".into());
        c.input_lister = || (vec!["Fake Mic".into(), "USB Mic".into()], Some("Fake Mic".into()));
        c.input_opener = Arc::new(|device, voice| Ok(Input::fake(device.unwrap_or("Fake Mic"), device, voice)));
        assert_eq!(c.input_name(), "the system default input", "not listed yet");
        c.effects.clear();
        let id: Uuid = c.execute("record.start", json!({})).unwrap()["tile"].as_str().unwrap().parse().unwrap();
        assert_eq!((c.selected_tile, c.selected()), (Some(id), vec![id]));
        assert!(c.effects.iter().any(|e| matches!(e, Effect::Reveal(t) if *t == id)));
        let deadline = Instant::now() + Duration::from_secs(5);
        while c.input.is_none() {
            assert!(Instant::now() < deadline, "the input never arrived");
            std::thread::sleep(Duration::from_millis(10));
            c.tick(0.01);
        }
        let view = c.recording_view().unwrap();
        assert_eq!((view.tile, view.title.as_str(), view.input.as_deref()), (id, "Recording 1", Some("Fake Mic")));
        let status = c.execute("status", json!({})).unwrap();
        assert_eq!((status["input"].as_str(), status["recording_seconds"].as_f64()), (Some("Fake Mic"), Some(0.0)));
        assert_eq!(c.execute("input.list", json!({})).unwrap()["default"], "Fake Mic");
        // Recording into a tile already there scrolls to it but leaves the selection alone.
        c.execute("record.stop", json!({})).ok();
        c.execute("view.select", json!({})).unwrap();
        c.execute("settings.inputDevice", json!({"name": "USB Mic"})).unwrap();
        c.tick(0.01);
        assert_eq!(c.input_name(), "USB Mic", "the chosen input, before it opens");
        c.effects.clear();
        c.execute("record.start", json!({"tile": "Old"})).unwrap();
        assert_eq!(c.selected_tile, None);
        assert!(c.effects.iter().any(|e| matches!(e, Effect::Reveal(_))));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn recording_into_tiles() {
        let (mut c, root) = controller();
        let ids = add(&mut c, &root, &["Old"]);
        // The test engine has no device, so record.start refuses...
        assert_eq!(c.execute("record.start", json!({})).unwrap_err().code, "no_audio");
        assert_eq!(c.execute("record.stop", json!({})).unwrap_err().code, "not_recording");
        // ...so feed one in: half a second of a tone into the existing tile.
        let tone: Vec<f32> = (0..4000).flat_map(|i| {
            let s = 0.3 * (i as f32 * 0.2).sin();
            [s, s]
        }).collect();
        let old_file = c.store.library.tile(ids[0]).unwrap().file.clone().unwrap();
        c.fake_recording(ids[0], false, tone.clone());
        assert_eq!(c.execute("record.status", json!({})).unwrap()["seconds"], 0.5);
        let r = c.execute("record.toggle", json!({})).unwrap();
        assert_eq!(r["seconds"], 0.5);
        let t = c.store.library.tile(ids[0]).unwrap().clone();
        assert_ne!(t.file.as_deref(), Some(old_file.as_str()));
        assert!(c.store.media_dir.join(t.file.as_ref().unwrap()).exists(), "written as a WAV");
        assert!((c.store.audio(&t).unwrap().duration() - 0.5).abs() < 1e-6, "playable at once");
        assert_eq!(c.execute("tile.play", json!({"tile": "Old"})).unwrap()["state"], "playing");
        // The WAV decodes back to the same length.
        let back = crate::decode::decode_file(&c.store.media_dir.join(t.file.unwrap())).unwrap();
        assert!((back.duration() - 0.5).abs() < 1e-3);
        // An empty recording into a made tile removes the tile again.
        c.store.library.sets[0].tiles.push(Tile { title: "Recording 1".into(), ..Default::default() });
        let made = c.store.library.sets[0].tiles.last().unwrap().id;
        c.fake_recording(made, true, Vec::new());
        assert_eq!(c.execute("record.stop", json!({})).unwrap_err().code, "failed");
        assert!(c.store.library.tile(made).is_none());
    }

    #[test]
    fn instant_replay() {
        let (mut c, _root) = controller();
        assert_eq!(c.execute("replay.save", json!({})).unwrap_err().code, "off");
        c.execute("replay.enable", json!({"seconds": 5})).unwrap();
        assert_eq!(c.execute("replay.enable", json!({"seconds": 2})).unwrap_err().code, "bad_params");
        // A mono input at 1 kHz: 8 s of samples, of which 5 are kept.
        let tx = c.fake_capture(1000, 1);
        for i in 0..8 {
            tx.send(vec![i as f32 / 10.0; 1000]).unwrap();
        }
        c.pump_capture();
        assert!((c.replay_seconds() - 5.0).abs() < 1e-9);
        let r = c.execute("replay.save", json!({"seconds": 2})).unwrap();
        assert_eq!((r["title"].as_str(), r["seconds"].as_f64()), (Some("Replay 1"), Some(2.0)));
        let t = c.store.library.tile(r["tile"].as_str().unwrap().parse().unwrap()).unwrap().clone();
        let audio = c.store.audio(&t).unwrap();
        assert_eq!(audio.data.first().copied(), Some(0.6), "the last two seconds, mono doubled");
        assert_eq!(audio.data.last().copied(), Some(0.7));
        assert_eq!(c.execute("replay.save", json!({"title": "Callback"})).unwrap()["seconds"], 5.0);
        c.execute("replay.enable", json!({"seconds": "off"})).unwrap();
        assert_eq!(c.store.library.replay_seconds, 0);
    }

    #[test]
    fn vamp_settings_and_play_on() {
        let (mut c, root) = controller();
        add(&mut c, &root, &["Bed"]);
        let r = c.execute("tile.set", json!({"tile": "Bed", "vampStart": 0.1, "vampEnd": 0.3})).unwrap();
        assert_eq!(r["vamp"], json!([0.1, 0.3]));
        assert_eq!(c.execute("tile.set", json!({"tile": "Bed", "vampEnd": 0.12})).unwrap_err().code, "bad_params");
        c.execute("tile.set", json!({"tile": "Bed", "trimStart": 0.05})).unwrap();
        c.execute("tile.play", json!({"tile": "Bed"})).unwrap();
        assert_eq!(c.execute("tile.playOn", json!({"tile": "Bed"})).unwrap()["played_on"], true);
        assert_eq!(c.execute("tile.playOn", json!({"tile": "Bed"})).unwrap()["played_on"], false, "only once");
        // tile.release is its old name (D-156), with the old result too.
        c.execute("tile.play", json!({"tile": "Bed", "restart": true})).unwrap();
        let r = c.execute("tile.release", json!({"tile": "Bed"})).unwrap();
        assert_eq!((r["released"].as_bool(), r["played_on"].as_bool()), (Some(true), Some(true)));
        assert_eq!(c.execute("tile.release", json!({"tile": "Bed"})).unwrap()["released"], false);
        assert_eq!(c.execute("tile.set", json!({"tile": "Bed", "vamp": null})).unwrap()["vamp"], Value::Null);
    }

    /// D-153/D-154: a vamping tile's next press plays on, the press after stops it, for
    /// every option combination, through tile.trigger (what keys, pads, MIDI and OSC use).
    #[test]
    fn a_vamping_tile_plays_on_then_its_options_apply() {
        let (mut c, root) = controller();
        let ids = add(&mut c, &root, &["A"]);
        c.execute("tile.set", json!({"tile": "A", "vampStart": 0.1, "vampEnd": 0.3})).unwrap();
        // What the tile is doing: (state, vamping, played on); a fade-out counts as stopped.
        let look = |c: &Controller| match c.engine.state(ids[0]) {
            PlayState::Stopped | PlayState::Stopping => (PlayState::Stopped, false, false),
            s => (s, c.engine.vamping(ids[0]), c.engine.statuses()[&ids[0]].played_on),
        };
        use PlayState::{Paused, Playing, Stopped};
        const VAMPING: (bool, bool) = (true, false);
        const ON: (bool, bool) = (false, true);
        const OFF: (bool, bool) = (false, false);
        // An event, then the state and (vamping, played on) it leaves.
        type Step = (&'static str, PlayState, (bool, bool));
        #[rustfmt::skip]
        let table: [(&str, Value, &[Step]); 6] = [
            ("plain", json!({}), &[("press", Playing, VAMPING), ("release", Playing, VAMPING), ("press", Playing, ON), ("press", Stopped, OFF)]),
            ("pausable", json!({"pausable": true}), &[("press", Playing, VAMPING), ("press", Playing, ON), ("press", Paused, ON), ("press", Playing, ON)]),
            ("one-shot", json!({"oneShot": true}), &[("press", Playing, VAMPING), ("press", Playing, ON), ("press", Playing, VAMPING), ("press", Playing, ON)]),
            ("hold", json!({"hold": true}), &[("press", Playing, VAMPING), ("release", Playing, ON), ("press", Playing, ON), ("release", Stopped, OFF)]),
            ("hold+pausable", json!({"hold": true, "pausable": true}), &[("press", Playing, VAMPING), ("release", Playing, ON), ("press", Playing, ON), ("release", Paused, ON), ("press", Playing, ON)]),
            ("hold+one-shot", json!({"hold": true, "oneShot": true}), &[("press", Playing, VAMPING), ("release", Playing, ON), ("press", Playing, ON), ("release", Stopped, OFF)]),
        ];
        for (name, opts, steps) in table {
            c.execute("tile.stop", json!({"tile": "A", "fade": 0})).unwrap();
            let mut p = json!({"tile": "A", "hold": false, "pausable": false, "oneShot": false});
            p.as_object_mut().unwrap().extend(opts.as_object().unwrap().clone());
            c.execute("tile.set", p).unwrap();
            for (n, &(event, state, (vamping, played_on))) in steps.iter().enumerate() {
                c.execute("tile.trigger", json!({"tile": "A", "event": event})).unwrap();
                assert_eq!(look(&c), (state, vamping, played_on), "{name}, step {n} ({event})");
            }
        }
        std::fs::remove_dir_all(root).ok();
    }

    /// D-155: GO on a vamping cue plays it on and starts nothing; the next GO goes on.
    #[test]
    fn go_lets_a_vamping_cue_play_on() {
        let (mut c, root) = controller();
        let ids = add(&mut c, &root, &["Intro", "Song", "Outro"]);
        c.execute("tile.set", json!({"tile": "Intro", "vampStart": 0.1, "vampEnd": 0.3})).unwrap();
        assert_eq!(c.execute("cue.go", json!({})).unwrap()["title"], "Intro");
        assert!(c.engine.vamping(ids[0]));
        let r = c.execute("cue.go", json!({})).unwrap();
        assert_eq!((r["title"].as_str(), r["played_on"].as_bool()), (Some("Intro"), Some(true)));
        assert!(!c.engine.vamping(ids[0]) && c.engine.state(ids[0]) == PlayState::Playing, "played on");
        assert_eq!(c.engine.state(ids[1]), PlayState::Stopped, "the next cue waits for its own GO");
        assert_eq!(c.execute("status", json!({})).unwrap()["next_cue"]["title"], "Song", "the pointer stays on Song");
        assert_eq!(c.execute("cue.go", json!({})).unwrap()["title"], "Song");
        assert!(c.execute("cue.go", json!({})).unwrap().get("played_on").is_none());

        // Stepped back onto the vamping cue: GO plays it on (not again from the top) and
        // moves the pointer past it.
        c.execute("transport.stopAll", json!({"immediate": true})).unwrap();
        c.execute("tile.trigger", json!({"tile": "Intro"})).unwrap();
        c.execute("cue.set", json!({"tile": "Intro"})).unwrap();
        let r = c.execute("cue.go", json!({})).unwrap();
        assert_eq!((r["title"].as_str(), r["played_on"].as_bool()), (Some("Intro"), Some(true)));
        assert_eq!(c.execute("status", json!({})).unwrap()["next_cue"]["title"], "Song");
        // A disabled vamping tile is skipped, as GO skips it. (Intro is playing on: a press
        // stops it, the next starts it vamping.)
        c.execute("tile.trigger", json!({"tile": "Intro"})).unwrap();
        c.execute("tile.trigger", json!({"tile": "Intro"})).unwrap();
        assert!(c.engine.vamping(ids[0]), "{:?}", c.engine.state(ids[0]));
        c.execute("tile.disable", json!({"tile": "Intro"})).unwrap();
        assert_eq!(c.execute("cue.go", json!({})).unwrap()["title"], "Song");
        assert!(c.engine.vamping(ids[0]));
        std::fs::remove_dir_all(root).ok();
    }

    /// Play On's old names keep working: OSC /ls/release beside /ls/playon, and action
    /// tiles of kind release (the CLI, Library.json, set files) are Play On (D-156).
    #[test]
    fn play_on_aliases_and_old_release_actions() {
        use crate::osc::{OscArg, OscMessage};
        let (mut c, root) = controller();
        let ids = add(&mut c, &root, &["Bed", "Other"]);
        c.execute("tile.set", json!({"tile": "Bed", "vampStart": 0.1, "vampEnd": 0.3})).unwrap();
        for addr in ["/ls/playon", "/ls/release"] {
            c.execute("tile.play", json!({"tile": "Bed", "restart": true})).unwrap();
            assert!(c.engine.vamping(ids[0]));
            c.osc(&OscMessage { addr: addr.into(), args: vec![OscArg::Str("Bed".into())] }, None);
            assert!(!c.engine.vamping(ids[0]) && c.engine.state(ids[0]) == PlayState::Playing, "{addr}");
        }
        // The CLI takes the new and the old kind; both are play_on.
        for kind in ["play_on", "playOn", "release"] {
            let a = c.execute("tile.addAction", json!({"kind": kind, "targets": ["Bed"]})).unwrap()["id"].as_str().unwrap().to_owned();
            let r = c.execute("tile.get", json!({"tile": a})).unwrap();
            assert_eq!((r["action"]["kind"].as_str(), r["title"].as_str()), (Some("play_on"), Some("Play On Bed")), "{kind}");
            c.execute("tile.remove", json!({"tile": a})).unwrap();
        }
        // An older library's action tile says release, and still plays the vamp on.
        let mut lib = serde_json::to_value(&c.store.library).unwrap();
        let mut action = serde_json::to_value(Tile::default()).unwrap();
        action["title"] = json!("Let the bed go");
        action["action"] = json!({"kind": "release", "targets": [{"tile": ids[0]}]});
        lib["sets"][0]["tiles"].as_array_mut().unwrap().push(action);
        let old: crate::model::Library = serde_json::from_value(lib).unwrap();
        let t = old.sets[0].tiles.last().unwrap().clone();
        assert_eq!(t.action.as_ref().map(|a| a.kind), Some(ActionKind::PlayOn));
        assert_eq!(serde_json::to_value(t.action.as_ref().unwrap().kind).unwrap(), "play_on", "written under the new name");
        c.store.library.sets[0].tiles.push(t.clone());
        c.execute("tile.play", json!({"tile": "Bed", "restart": true})).unwrap();
        c.execute("tile.trigger", json!({"tile": t.id.to_string()})).unwrap();
        assert!(!c.engine.vamping(ids[0]) && c.engine.state(ids[0]) == PlayState::Playing, "the old action plays on");
        assert_eq!(c.engine.state(ids[1]), PlayState::Stopped);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn speed_setting() {
        let (mut c, root) = controller();
        add(&mut c, &root, &["Clip"]);
        assert_eq!(c.execute("tile.set", json!({"tile": "Clip", "speed": 1.5})).unwrap()["speed"], 1.5);
        assert_eq!(c.execute("tile.set", json!({"tile": "Clip", "speed": 3})).unwrap_err().code, "bad_params");
        c.execute("tile.play", json!({"tile": "Clip"})).unwrap();
        let st = c.execute("status", json!({})).unwrap();
        assert!((st["playing"][0]["length"].as_f64().unwrap() - 0.5 / 1.5).abs() < 1e-6, "{st}");
    }

    #[test]
    fn markers_and_jumps() {
        let (mut c, root) = controller();
        add(&mut c, &root, &["Song"]);
        c.execute("tile.markerAdd", json!({"tile": "Song", "at": 0.3, "name": "Outro"})).unwrap();
        let r = c.execute("tile.markerAdd", json!({"tile": "Song", "at": 0.1})).unwrap();
        assert_eq!(r["markers"], json!([{"name": "Marker 2", "at": 0.1}, {"name": "Outro", "at": 0.3}]), "kept in time order");
        assert_eq!(c.execute("tile.markerAdd", json!({"tile": "Song", "at": 0.2, "name": "outro"})).unwrap_err().code, "bad_params");
        assert_eq!(c.execute("tile.markerAdd", json!({"tile": "Song"})).unwrap_err().code, "bad_params", "not playing: needs at");
        // Jumping starts a stopped tile at the marker.
        let r = c.execute("tile.jump", json!({"tile": "Song", "marker": "outro"})).unwrap();
        assert_eq!(r["at"], 0.3);
        let pos = c.engine.statuses()[&c.resolve_tile("Song").unwrap()].position;
        assert!((pos - 0.3).abs() < 1e-3, "{pos}");
        c.execute("tile.jump", json!({"tile": "Song", "marker": "#1"})).unwrap();
        // A marker at the playhead.
        let r = c.execute("tile.markerAdd", json!({"tile": "Song", "name": "Here"})).unwrap();
        let here = r["markers"].as_array().unwrap().iter().find(|m| m["name"] == "Here").unwrap()["at"].as_f64().unwrap();
        assert!((0.1..0.15).contains(&here), "just after the marker it jumped to: {here}");
        assert_eq!(c.execute("tile.jump", json!({"tile": "Song", "marker": "Bridge"})).unwrap_err().code, "not_found");
        // Moving and renaming keep time order and unique names.
        let r = c.execute("tile.markerMove", json!({"tile": "Song", "marker": "Outro", "at": 0.05, "name": "Top"})).unwrap();
        assert_eq!(r["markers"][0]["name"], "Top");
        assert_eq!(c.execute("tile.markerMove", json!({"tile": "Song", "marker": "Top", "name": "here"})).unwrap_err().code, "bad_params");
        assert_eq!(c.execute("tile.markerMove", json!({"tile": "Song", "marker": "Top", "at": -1.0})).unwrap_err().code, "bad_params");
        c.execute("tile.markerMove", json!({"tile": "Song", "marker": "Top", "at": 0.3, "name": "Outro"})).unwrap();
        c.execute("tile.markerRemove", json!({"tile": "Song", "marker": "Outro"})).unwrap();
        assert_eq!(c.execute("tile.markerRemove", json!({"tile": "Song", "all": true})).unwrap()["markers"], json!([]));
    }

    #[test]
    fn editor_selection_zoom_and_stretch_preview() {
        let (mut c, root) = controller();
        add(&mut c, &root, &["Song", "Other"]);
        assert_eq!(c.execute("view.editorSelect", json!({"from": 0.1, "to": 0.2})).unwrap_err().code, "bad_params", "the editor isn't open");
        c.execute("view.editor", json!({"tile": "Song"})).unwrap();
        // Either way round, kept inside the file.
        let r = c.execute("view.editorSelect", json!({"from": 0.3, "to": 0.1})).unwrap();
        assert_eq!(r["selection"], json!({"from": 0.1, "to": 0.3}));
        assert_eq!(c.execute("view.editorSelect", json!({"from": 0.2, "to": 9.0})).unwrap()["selection"]["to"], 0.5);
        assert_eq!(c.execute("view.editorSelect", json!({"from": 0.2})).unwrap_err().code, "bad_params");
        assert_eq!(c.execute("view.editorSelect", json!({"from": 0.2, "to": 0.2})).unwrap_err().code, "bad_params");
        assert_eq!(c.execute("view.editorSelect", json!({})).unwrap()["selection"], json!({"from": 0.2, "to": 0.5}), "reports it");
        // Zoom requests wait for the window.
        let r = c.execute("view.editorZoom", json!({"selection": true})).unwrap();
        assert!((r["from"].as_f64().unwrap() - 0.185).abs() < 1e-9 && r["to"] == 0.5, "a little either side, inside the file: {r}");
        assert_eq!(c.editor_zoom, Some(EditorZoom::Span(0.2 - 0.015, 0.5)));
        c.execute("view.editorZoom", json!({"by": 2})).unwrap();
        assert_eq!(c.editor_zoom, Some(EditorZoom::By(2.0)));
        assert_eq!(c.execute("view.editorZoom", json!({"by": 0})).unwrap_err().code, "bad_params");
        assert_eq!(c.execute("view.editorZoom", json!({})).unwrap_err().code, "bad_params");
        c.execute("view.editorZoom", json!({"fit": true})).unwrap();
        assert_eq!(c.editor_zoom, Some(EditorZoom::Fit));
        // A stretch preview plays just it, and reports where it is in the file.
        c.execute("tile.set", json!({"tile": "Song", "vampStart": 0.1, "vampEnd": 0.3})).unwrap();
        let r = c.execute("tile.preview", json!({"tile": "Song", "from": 0.25, "to": 0.35})).unwrap();
        assert_eq!((r["from"].as_f64(), r["to"].as_f64()), (Some(0.25), Some(0.35)));
        let id = c.resolve_tile("Song").unwrap();
        let st = c.engine.statuses()[&id];
        assert!((st.from - 0.25).abs() < 1e-3 && (st.length - 0.1).abs() < 1e-3 && !st.vamping, "{st:?}");
        assert_eq!(c.store.library.tile(id).unwrap().vamp, Some((0.1, 0.3)), "the tile itself is unchanged");
        c.execute("tile.preview", json!({"tile": "Song"})).unwrap();
        assert_eq!(c.execute("tile.preview", json!({"tile": "Other", "from": 0.3, "to": 0.1})).unwrap_err().code, "bad_params");
        // Another tile, or closing, forgets the selection.
        c.execute("view.select", json!({"tile": "Other"})).unwrap();
        assert!(c.editor.is_none() && c.editor_selection.is_none());
        c.execute("view.editor", json!({"tile": "Song"})).unwrap();
        assert_eq!(c.execute("view.editorSelect", json!({})).unwrap()["selection"], Value::Null);
        assert_eq!(c.execute("view.editorZoom", json!({"selection": true})).unwrap_err().code, "bad_params", "nothing selected");
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn osc_addresses_bindings_and_refusals() {
        use crate::osc::{OscArg, OscMessage};
        let (mut c, root) = controller();
        let ids = add(&mut c, &root, &["Horn", "Bed", "Third"]);
        let msg = |addr: &str, args: Vec<OscArg>| OscMessage { addr: addr.into(), args };
        let n = |v: f32| OscArg::Num(v);
        let s = |v: &str| OscArg::Str(v.into());

        c.osc(&msg("/ls/tile/2/trigger", vec![n(1.0)]), None);
        assert_eq!(c.engine.state(ids[1]), PlayState::Playing);
        c.osc(&msg("/ls/stop", vec![s("Bed")]), None);
        assert_ne!(c.engine.state(ids[1]), PlayState::Playing);
        c.osc(&msg("/ls/trigger", vec![s("Horn")]), None);
        assert_eq!(c.engine.state(ids[0]), PlayState::Playing);
        c.osc(&msg("/ls/master", vec![n(0.5)]), None);
        assert_eq!(c.store.library.master_volume, 0.5);
        c.osc(&msg("/ls/duck", vec![n(1.0)]), None);
        assert!(c.engine.duck_hold());
        c.osc(&msg("/ls/duck", vec![n(0.0)]), None);
        assert!(!c.engine.duck_hold());
        c.osc(&msg("/ls/cmd", vec![s("tile.set"), s(r#"{"tile": "Third", "volume": 0.25}"#)]), None);
        assert_eq!(c.store.library.tile(ids[2]).unwrap().volume, 0.25);

        // Refused: destructive commands, quitting, files, bindings.
        c.osc(&msg("/ls/cmd", vec![s("tile.remove"), s(r#"{"tile": "Third"}"#)]), None);
        assert!(c.store.library.tile(ids[2]).is_some());
        assert!(c.notice.as_ref().is_some_and(|n| n.error && n.text.contains("can't run tile.remove")));

        // A binding on an address of your own, with a fader.
        c.execute("bind.add", json!({"source": "osc:/desk/fader1", "command": "master.volume", "value": "volume"})).unwrap();
        c.osc(&msg("/desk/fader1", vec![n(0.8)]), None);
        assert!((c.store.library.master_volume - 0.8).abs() < 1e-6);

        let r = c.execute("osc.enable", json!({"on": true, "port": 9100, "feedback": "127.0.0.1:9101"})).unwrap();
        assert_eq!((r["port"].as_u64(), r["lan"].as_bool()), (Some(9100), Some(false)), "localhost unless asked");
        assert_eq!(c.execute("osc.enable", json!({"port": 80})).unwrap_err().code, "bad_params");
    }

    #[test]
    fn osc_feedback_sends_changes() {
        let (mut c, root) = controller();
        add(&mut c, &root, &["Horn", "Bed"]);
        let rx = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        rx.set_read_timeout(Some(Duration::from_millis(500))).unwrap();
        let port = rx.local_addr().unwrap().port();
        c.execute("osc.enable", json!({"on": true, "port": 9200, "feedback": format!("127.0.0.1:{port}")})).unwrap();
        let read_all = |rx: &std::net::UdpSocket| {
            let mut got = Vec::new();
            let mut buf = [0u8; 2048];
            while let Ok((n, _)) = rx.recv_from(&mut buf) {
                got.extend(crate::osc::messages(rosc::decoder::decode_udp(&buf[..n]).unwrap().1));
                rx.set_read_timeout(Some(Duration::from_millis(100))).unwrap();
            }
            got
        };
        c.tick(0.0);
        let first = read_all(&rx);
        assert!(first.iter().any(|m| m.addr == "/ls/tile/2/title" && m.str(0) == Some("Bed")), "{first:?}");
        // Only changes after that: playing tile 1.
        c.execute("tile.play", json!({"tile": "Horn"})).unwrap();
        c.tick(0.0);
        let next = read_all(&rx);
        let addrs: Vec<(&str, Option<f32>)> = next.iter().map(|m| (m.addr.as_str(), m.num(0))).collect();
        assert!(addrs.contains(&("/ls/tile/1/state", Some(1.0))) && addrs.contains(&("/ls/cue/next", Some(2.0))), "{addrs:?}");
        assert!(!addrs.iter().any(|(a, _)| a.contains("title") && *a != "/ls/now/title" && *a != "/ls/cue/next/title" || a.starts_with("/ls/tile/2/")), "only what changed: {addrs:?}");
        let get = |ms: &[OscMessage], a: &str| ms.iter().find(|m| m.addr == a).cloned();
        assert_eq!(get(&next, "/ls/now/title").and_then(|m| m.str(0).map(str::to_owned)).as_deref(), Some("Horn"));
        assert_eq!(get(&next, "/ls/cue/current").and_then(|m| m.num(0)), Some(1.0));
        assert!(get(&next, "/ls/tile/1/remaining").and_then(|m| m.num(0)).is_some_and(|r| r > 0.0));
        assert!(get(&next, "/ls/tile/1/time").is_some_and(|m| m.str(0).is_some_and(|t| t.starts_with("0:"))));
        assert_eq!(get(&first, "/ls/tiles").and_then(|m| m.num(0)), Some(2.0));
        assert!(get(&first, "/ls/tile/1/rgb").is_some_and(|m| m.str(0).is_some_and(|c| c.len() == 7 && c.starts_with('#'))));
        assert!(get(&first, "/ls/tile/1/color").is_some_and(|m| m.str(0).is_some_and(|c| !c.is_empty())));

        // A controller asks for everything again.
        c.osc(&OscMessage { addr: "/ls/resync".into(), args: vec![] }, None);
        let again = read_all(&rx);
        assert!(get(&again, "/ls/tile/2/title").is_some(), "resync sends everything: {} messages", again.len());

        // A tile removed: its addresses go out once more, blank.
        c.execute("tile.remove", json!({"tile": "Bed"})).unwrap();
        c.tick(0.0);
        let after = read_all(&rx);
        assert_eq!(get(&after, "/ls/tile/2/title").and_then(|m| m.str(0).map(str::to_owned)).as_deref(), Some(""));
        assert_eq!(get(&after, "/ls/tiles").and_then(|m| m.num(0)), Some(1.0));
    }

    #[test]
    fn osc_feedback_sends_tile_levels_ten_times_a_second() {
        let (mut c, root) = controller();
        // Long enough to play through every step below.
        let paths: Vec<String> = ["Tone", "Quiet"]
            .iter()
            .map(|n| {
                let p = root.join(format!("{n}.wav"));
                wav::write_tone(&p, 8000, 5.0, &[330.0], 0.4).unwrap();
                p.to_string_lossy().into_owned()
            })
            .collect();
        c.execute("tile.add", json!({"paths": paths})).unwrap();
        wait_loaded(&mut c);
        let rx = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = rx.local_addr().unwrap().port();
        let r = c.execute("osc.enable", json!({"on": true, "port": 9300, "feedback": format!("127.0.0.1:{port}")})).unwrap();
        assert_eq!(r["levels"], true, "on with the feedback unless turned off");
        let read_all = |rx: &std::net::UdpSocket| {
            let mut got = Vec::new();
            let mut buf = [0u8; 4096];
            rx.set_read_timeout(Some(Duration::from_millis(300))).unwrap();
            while let Ok((n, _)) = rx.recv_from(&mut buf) {
                got.extend(crate::osc::messages(rosc::decoder::decode_udp(&buf[..n]).unwrap().1));
                rx.set_read_timeout(Some(Duration::from_millis(50))).unwrap();
            }
            got
        };
        let levels = |ms: &[OscMessage], a: &str| ms.iter().filter(|m| m.addr == a).filter_map(|m| m.num(0)).collect::<Vec<f32>>();
        c.tick(0.0);
        let first = read_all(&rx);
        assert_eq!(levels(&first, "/ls/tile/1/level"), [0.0], "a stopped tile's level is 0");

        c.execute("tile.trigger", json!({"tile": "Tone"})).unwrap();
        for _ in 0..10 {
            c.tick(0.02);
        }
        let playing = levels(&read_all(&rx), "/ls/tile/1/level");
        // A 0.4 tone is about −8 dB: 0.72 on the meter's curve.
        assert!(playing.last().is_some_and(|l| (0.65..=0.8).contains(l)), "{playing:?}");
        assert_eq!(levels(&read_all(&rx), "/ls/tile/2/level"), Vec::<f32>::new(), "a silent tile sends nothing new");

        // Falling at 26 dB/s, the level changes every frame; it goes out ten times a second.
        c.execute("master.volume", json!({"volume": 0.1})).unwrap();
        for _ in 0..30 {
            c.tick(0.02);
        }
        let falling = levels(&read_all(&rx), "/ls/tile/1/level");
        assert!((5..=7).contains(&falling.len()), "0.6 s of 50 fps ticks: {falling:?}");
        assert!(falling.windows(2).all(|w| w[1] < w[0]), "falling: {falling:?}");

        // A 0 the moment the tile stops (after its 0.1 s de-click, which the meter
        // follows down), without waiting for the next sample.
        c.execute("transport.stopAll", json!({"immediate": true})).unwrap();
        for _ in 0..200 {
            c.tick(0.001);
            if c.statuses.is_empty() {
                break;
            }
        }
        let stopped = read_all(&rx);
        assert_eq!(levels(&stopped, "/ls/tile/1/level").last(), Some(&0.0), "{stopped:?}");
        assert_eq!(levels(&stopped, "/ls/now/level").last(), Some(&0.0));
        assert!(stopped.last().is_some_and(|m| m.addr == "/ls/now/level"), "the 0 goes out as it stops: {stopped:?}");

        // Turned off: the level addresses go once more as 0, then not at all.
        c.execute("tile.trigger", json!({"tile": "Tone"})).unwrap();
        for _ in 0..10 {
            c.tick(0.02);
        }
        assert!(!levels(&read_all(&rx), "/ls/now/level").is_empty());
        assert_eq!(c.execute("osc.enable", json!({"levels": false})).unwrap()["levels"], false);
        c.tick(0.02);
        assert_eq!(levels(&read_all(&rx), "/ls/tile/1/level"), [0.0]);
        for _ in 0..10 {
            c.tick(0.02);
        }
        assert!(!read_all(&rx).iter().any(|m| m.addr.ends_with("/level")));
        assert_eq!(c.execute("osc.status", json!({})).unwrap()["levels"], false);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn tiles_send_messages_on_start_and_stop() {
        let (mut c, root) = controller();
        add(&mut c, &root, &["Horn", "Bed"]);
        let rx = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        rx.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let port = rx.local_addr().unwrap().port();
        let recv = |rx: &std::net::UdpSocket| {
            let mut buf = [0u8; 512];
            let (n, _) = rx.recv_from(&mut buf).expect("a send");
            crate::osc::messages(rosc::decoder::decode_udp(&buf[..n]).unwrap().1).remove(0)
        };
        let r = c
            .execute("tile.set", json!({"tile": "Horn", "onStart": format!("osc://127.0.0.1:{port}/lights/go 3\n\nosc://127.0.0.1:{port}/video/play \"intro\""), "onStop": [format!("osc://localhost:{port}/lights/out")]}))
            .unwrap();
        assert_eq!(r["on_start"].as_array().unwrap().len(), 2);
        assert_eq!(c.execute("tile.set", json!({"tile": "Horn", "onStart": "osc://h/x"})).unwrap_err().code, "bad_params");
        let far = c.execute("tile.set", json!({"tile": "Horn", "onStop": "osc://192.168.1.20:8000/x"})).unwrap_err();
        assert!(far.message.contains("network"), "{}", far.message);

        c.execute("tile.play", json!({"tile": "Horn"})).unwrap();
        c.tick(0.0);
        let m = recv(&rx);
        assert_eq!((m.addr.as_str(), m.num(0)), ("/lights/go", Some(3.0)));
        let m = recv(&rx);
        assert_eq!((m.addr.as_str(), m.str(0)), ("/video/play", Some("intro")));
        c.tick(0.0);
        c.execute("tile.stop", json!({"tile": "Horn"})).unwrap();
        for _ in 0..50 {
            c.tick(0.05);
            if !c.statuses.contains_key(&c.store.library.sets[0].tiles[0].id) {
                break;
            }
        }
        assert_eq!(recv(&rx).addr, "/lights/out");
        // A preview stays in the operator's headphones.
        let queued = c.messages.queued;
        c.execute("tile.preview", json!({"tile": "Horn"})).unwrap();
        c.tick(0.0);
        assert_eq!(c.messages.queued, queued, "a preview sends nothing");
        // OSC may not point the messages elsewhere.
        use crate::osc::{OscArg, OscMessage};
        let set = format!(r#"{{"tile": "Bed", "onStart": "osc://127.0.0.1:{port}/x"}}"#);
        c.osc(&OscMessage { addr: "/ls/cmd".into(), args: vec![OscArg::Str("tile.set".into()), OscArg::Str(set)] }, None);
        assert!(c.store.library.sets[0].tiles[1].on_start.is_empty());
        // A MIDI message to a port that isn't there: the error shows.
        c.execute("tile.set", json!({"tile": "Bed", "onStart": "midi:note:60 @nothing-called-this"})).unwrap();
        c.notice = None;
        c.execute("tile.play", json!({"tile": "Bed"})).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while !c.notice.as_ref().is_some_and(|n| n.error && n.text.contains("MIDI")) && Instant::now() < deadline {
            c.tick(0.0);
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(c.notice.as_ref().is_some_and(|n| n.error && n.text.contains("MIDI")), "{:?}", c.notice.as_ref().map(|n| &n.text));
    }

    /// The layouts in examples/ use only addresses the app accepts or sends.
    #[test]
    fn example_layouts_use_shipped_addresses() {
        let (mut c, root) = controller();
        add(&mut c, &root, &["Horn"]);
        let generic = |a: &str| {
            let parts: Vec<&str> = a.split('/').collect();
            match parts.as_slice() {
                ["", "ls", "tile", n, rest @ ..] if n.parse::<u32>().is_ok() => format!("/ls/tile/N/{}", rest.join("/")),
                _ => a.to_owned(),
            }
        };
        let mut known: Vec<String> = c.osc_state().into_iter().map(|(a, _)| generic(&a)).collect();
        known.extend(
            ["/ls/tile/N/trigger", "/ls/trigger", "/ls/play", "/ls/stop", "/ls/playon", "/ls/release", "/ls/jump", "/ls/go", "/ls/stopall", "/ls/master", "/ls/duck", "/ls/cmd", "/ls/resync"]
                .map(String::from),
        );
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
        let mut seen = 0;
        for f in ["companion/leadstinger.companionconfig", "open-stage-control/leadstinger.json", "make-layouts.py"] {
            let text = std::fs::read_to_string(dir.join(f)).unwrap();
            for (i, _) in text.match_indices("/ls/") {
                let addr: String = text[i..].chars().take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '/').collect();
                if addr.ends_with('/') {
                    continue; // an f-string base in the generator, such as "/ls/tile/{n}"
                }
                assert!(known.contains(&generic(&addr)), "{f} uses {addr}, which the app neither accepts nor sends");
                seen += 1;
            }
        }
        assert!(seen > 50, "found only {seen} addresses");
    }

    #[test]
    fn pad_lights_follow_their_tiles() {
        let (mut c, root) = controller();
        add(&mut c, &root, &["Horn", "Bed"]);
        let (tx, rx) = std::sync::mpsc::channel();
        c.midi_out = Some(tx);
        c.execute("bind.add", json!({"source": "midi:note:36:ch10", "tile": "Horn"})).unwrap();
        c.tick(0.0);
        assert!(rx.try_recv().is_err(), "off until turned on");
        c.execute("midi.feedback", json!({"on": true, "lit": 5, "unlit": 1})).unwrap();
        c.tick(0.0);
        assert_eq!(rx.try_iter().collect::<Vec<_>>(), [vec![0x99, 36, 1]], "every pad once, unlit");
        c.execute("tile.play", json!({"tile": "Horn"})).unwrap();
        c.tick(0.0);
        assert_eq!(rx.try_iter().collect::<Vec<_>>(), [vec![0x99, 36, 5]], "lit while it plays");
        c.tick(0.0);
        assert!(rx.try_recv().is_err(), "nothing new");
        c.execute("midi.autoMap", json!({"base": 48})).unwrap();
        c.tick(0.0);
        let mut got: Vec<Vec<u8>> = rx.try_iter().collect();
        got.sort();
        assert_eq!(got, [vec![0x90, 48, 5], vec![0x90, 49, 1]], "auto-mapped pads on channel 1");
        assert_eq!(c.execute("midi.feedback", json!({"lit": 200})).unwrap_err().code, "bad_params");
    }

    #[test]
    fn pad_pages_and_colours() {
        let (mut c, root) = controller();
        let ids = add(&mut c, &root, &["A", "B", "C", "D", "E"]);
        let (tx, rx) = std::sync::mpsc::channel();
        c.midi_out = Some(tx);
        let note = |n: u8| MidiEvent { port: "Pads".into(), channel: 1, kind: MidiKind::Note(n), value: 100, on: true };
        c.execute("midi.autoMap", json!({"base": 36, "bank": 2})).unwrap();
        // Page 1: notes 36, 37 play A, B; 38 is past the bank.
        c.midi(&note(37));
        assert_eq!(c.engine.state(ids[1]), PlayState::Playing);
        c.midi(&note(38));
        assert_eq!(c.engine.state(ids[2]), PlayState::Stopped);
        assert_eq!(c.execute("midi.pageUp", json!({})).unwrap()["page"], 2);
        c.midi(&note(36));
        assert_eq!(c.engine.state(ids[2]), PlayState::Playing, "page 2 starts at C");
        let r = c.execute("midi.page", json!({"page": 9})).unwrap();
        assert_eq!((r["page"].as_u64(), r["pages"].as_u64()), (Some(3), Some(3)), "clamped to the last page");
        c.midi(&note(37));
        assert_eq!(c.store.library.midi.auto_map, Some(36), "a bank change keeps the base");
        assert_eq!(c.execute("midi.page", json!({"delta": -1})).unwrap()["page"], 2);
        assert_eq!(c.execute("midi.pageDown", json!({})).unwrap()["page"], 1);
        c.execute("transport.stopAll", json!({"immediate": true})).unwrap();
        c.tick(0.0);

        // Colours: page 3 holds E and an empty pad.
        c.execute("midi.page", json!({"page": 3})).unwrap();
        c.execute("tile.set", json!({"tile": "E", "color": "red"})).unwrap();
        c.execute("midi.feedback", json!({"on": true, "colors": "launchpad"})).unwrap();
        c.tick(0.0);
        let mut got: Vec<Vec<u8>> = rx.try_iter().collect();
        got.sort();
        assert_eq!(got, [vec![0x90, 36, 5], vec![0x90, 37, 0]], "E red on its pad, the empty pad off");
        c.execute("tile.play", json!({"tile": "E"})).unwrap();
        c.tick(0.0);
        assert_eq!(rx.try_iter().collect::<Vec<_>>(), [vec![0x92, 36, 5]], "pulsing on channel 3 while it plays");
        c.execute("midi.feedback", json!({"colors": r#"{"idle": {"red": 60}, "playing": {"red": 61}}"#})).unwrap();
        c.tick(0.0);
        assert!(rx.try_iter().any(|m| m == [0x90, 36, 61]), "a table of one's own (sent with everything again)");
        assert_eq!(c.execute("midi.feedback", json!({"colors": "nope"})).unwrap_err().code, "bad_params");
        assert_eq!(c.execute("midi.feedback", json!({"colors": r#"{"idle": {"red": 300}}"#})).unwrap_err().code, "bad_params");
        let st = c.osc_state();
        let num = |a: &str| st.iter().find(|(k, _)| k == a).map(|(_, v)| v.clone());
        assert_eq!((num("/ls/midi/page"), num("/ls/midi/pages")), (Some(crate::osc::OscArg::Num(3.0)), Some(crate::osc::OscArg::Num(3.0))));
    }

    #[test]
    fn outputs_route_tiles_and_previews() {
        let (mut c, root) = controller();
        add(&mut c, &root, &["Horn", "Bed"]);
        let r = c.execute("output.add", json!({"name": "Phones", "device": "Headphones", "channel": 3})).unwrap();
        assert_eq!(r["outputs"][0]["channel"], 3);
        assert_eq!(c.execute("output.add", json!({"name": "main"})).unwrap_err().code, "bad_params");
        assert_eq!(c.execute("tile.set", json!({"tile": "Bed", "output": "Booth"})).unwrap_err().code, "bad_params");
        assert_eq!(c.execute("tile.set", json!({"tile": "Bed", "output": "phones"})).unwrap()["output"], "Phones");
        c.execute("tile.play", json!({"tile": "Bed"})).unwrap();
        let bed = c.resolve_tile("Bed").unwrap();
        // The test engine opens no devices, so a routed tile falls back to Main...
        assert_eq!(c.engine.output_of(bed).as_deref(), Some("Main"));
        assert_eq!(c.engine.routes()[1].0.channel, 2, "channel 3 is offset 2");
        // ...and the output's tiles follow a rename, then go back to Main when it's removed.
        c.execute("output.set", json!({"name": "Phones", "rename": "Cans"})).unwrap();
        assert_eq!(c.store.library.tile(bed).unwrap().output(), "Cans");
        c.execute("output.remove", json!({"name": "Cans"})).unwrap();
        assert!(c.store.library.tile(bed).unwrap().sends.is_empty());
        // Preview: on its own output, and it leaves the cue pointer alone.
        let r = c.execute("settings.previewOutput", json!({"on": true, "device": "Headphones"})).unwrap();
        assert_eq!(r["preview"]["on"], true);
        let set = c.current_set().id;
        let before = c.cue.get(&set).copied();
        c.execute("tile.preview", json!({"tile": "Horn"})).unwrap();
        assert_eq!(c.cue.get(&set).copied(), before);
        assert_eq!(c.engine.state(c.resolve_tile("Horn").unwrap()), PlayState::Playing);
    }

    #[test]
    fn a_preview_with_its_device_missing_plays_nowhere_and_says_so() {
        let (mut c, root) = controller();
        c.engine = Engine::fake(&["Speakers", "Headphones"]);
        add(&mut c, &root, &["Horn"]);
        let horn = c.resolve_tile("Horn").unwrap();
        c.execute("settings.previewOutput", json!({"on": true, "device": "Headphones"})).unwrap();
        c.engine.unplug("Headphones");
        c.tick(0.1);
        let set = c.current_set().id;
        let before = c.cue.get(&set).copied();
        for params in [json!({"tile": "Horn"}), json!({"tile": "Horn", "from": 0.0, "to": 0.5})] {
            let e = c.execute("tile.preview", params).unwrap_err();
            assert_eq!(e.code, "preview_unavailable");
            assert_eq!(e.message, "Preview's device Headphones isn't connected: nothing played");
            assert_eq!(c.engine.state(horn), PlayState::Stopped, "never on Main, the house");
        }
        assert_eq!(c.cue.get(&set).copied(), before);
        // The window shows why in the status bar.
        let e = c.execute("tile.preview", json!({"tile": "Horn"})).unwrap_err();
        c.report(&e);
        assert!(c.notice.as_ref().is_some_and(|n| n.error && n.text.contains("isn't connected: nothing played")));
        // Back: previews play in the headphones again.
        c.engine.plug("Headphones");
        c.last_reopen -= Duration::from_secs(4);
        c.tick(0.1);
        c.execute("tile.preview", json!({"tile": "Horn"})).unwrap();
        assert_eq!(c.engine.outputs_of(horn), ["Preview"]);
        c.execute("tile.stop", json!({"tile": "Horn"})).unwrap();
        // Preview to off: a preview plays where the tile goes, as before.
        c.execute("settings.previewOutput", json!({"on": false})).unwrap();
        c.engine.unplug("Headphones");
        c.tick(0.1);
        c.execute("tile.preview", json!({"tile": "Horn"})).unwrap();
        assert_eq!(c.engine.outputs_of(horn), ["Main"]);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn an_unplugged_output_closes_alone_and_reopens_when_it_is_back() {
        let (mut c, root) = controller();
        c.engine = Engine::fake(&["Speakers", "Headphones"]);
        add(&mut c, &root, &["Pad", "Cue"]);
        c.execute("output.add", json!({"name": "Phones", "device": "Headphones"})).unwrap();
        c.execute("tile.set", json!({"tile": "Pad", "sends": ["Main", {"output": "Phones", "gainDb": -6}]})).unwrap();
        c.execute("tile.set", json!({"tile": "Cue", "output": "Phones"})).unwrap();
        c.execute("tile.play", json!({"tile": "Pad"})).unwrap();
        c.execute("tile.play", json!({"tile": "Cue"})).unwrap();
        let (pad, cue) = (c.resolve_tile("Pad").unwrap(), c.resolve_tile("Cue").unwrap());
        assert_eq!(c.engine.outputs_of(pad), ["Main", "Phones"]);
        c.tick(0.2);

        c.engine.unplug("Headphones");
        c.tick(0.1);
        let n = c.notice.as_ref().map(|n| (n.error, n.text.clone())).unwrap();
        assert!(n.0 && n.1.contains("Phones lost Headphones"), "{}", n.1);
        assert_eq!(c.engine.state(pad), PlayState::Playing, "the pad plays on, on Main");
        assert_eq!(c.engine.outputs_of(pad), ["Main"]);
        assert_eq!(c.engine.outputs_of(cue), ["Main"], "a tile on Phones alone moves to Main");
        let list = c.execute("output.list", json!({})).unwrap();
        assert_eq!(list["outputs"][0]["open"], false);
        assert!(list["errors"][0]["error"].as_str().unwrap().contains("Headphones"), "{list}");

        // Back in: the next look at the devices (every 3 s) reopens it.
        c.engine.plug("Headphones");
        c.tick(0.1);
        assert_eq!(c.execute("output.list", json!({})).unwrap()["outputs"][0]["open"], false, "not before the next look");
        c.last_reopen -= Duration::from_secs(4);
        c.tick(0.1);
        let n = c.notice.as_ref().map(|n| (n.error, n.text.clone())).unwrap();
        assert_eq!(n, (false, "Phones is back on Headphones".to_string()));
        let list = c.execute("output.list", json!({})).unwrap();
        assert_eq!((&list["outputs"][0]["open"], &list["errors"]), (&json!(true), &json!([])));
        assert_eq!(c.engine.state(pad), PlayState::Playing);
        c.execute("tile.stop", json!({"tile": "Pad"})).unwrap();
        c.tick(1.0);
        c.execute("tile.play", json!({"tile": "Pad"})).unwrap();
        assert_eq!(c.engine.outputs_of(pad), ["Main", "Phones"], "its next start sends again");
    }

    #[test]
    fn sends_add_change_and_remove() {
        let (mut c, root) = controller();
        add(&mut c, &root, &["Bed"]);
        c.execute("output.add", json!({"name": "Stream"})).unwrap();
        c.execute("output.add", json!({"name": "Phones"})).unwrap();
        let sends = |c: &mut Controller| c.execute("tile.get", json!({"tile": "Bed"})).unwrap()["sends"].clone();
        assert_eq!(sends(&mut c), json!([{"output": "Main", "gain_db": 0.0}]));
        let r = c.execute("tile.send", json!({"tile": "Bed", "output": "stream", "gainDb": -6.0})).unwrap();
        assert_eq!(r["sends"], json!([{"output": "Main", "gain_db": 0.0}, {"output": "Stream", "gain_db": -6.0}]), "also sent, Main kept");
        c.execute("tile.send", json!({"tile": "Bed", "output": "Stream", "gainDb": -9.0})).unwrap();
        assert_eq!(sends(&mut c)[1]["gain_db"], -9.0, "a second send to it changes the level");
        assert_eq!(c.execute("tile.send", json!({"tile": "Bed", "output": "Booth"})).unwrap_err().code, "bad_params");
        assert_eq!(c.execute("tile.send", json!({"tile": "Bed", "output": "Stream", "gainDb": 40})).unwrap_err().code, "bad_params");
        // The Output is the first send.
        let r = c.execute("tile.set", json!({"tile": "Bed", "output": "Phones"})).unwrap();
        assert_eq!((&r["output"], &r["sends"][1]["output"]), (&json!("Phones"), &json!("Stream")));
        c.execute("tile.unsend", json!({"tile": "Bed", "output": "Phones"})).unwrap();
        assert_eq!(sends(&mut c), json!([{"output": "Stream", "gain_db": -9.0}]));
        assert_eq!(c.execute("tile.unsend", json!({"tile": "Bed", "output": "Phones"})).unwrap_err().code, "not_found");
        // The whole list at once, as the inspector and set files do; null is Main.
        let r = c.execute("tile.set", json!({"tile": "Bed", "sends": [{"output": "main", "gainDb": -2}, "Phones"]})).unwrap();
        assert_eq!(r["sends"], json!([{"output": "Main", "gain_db": -2.0}, {"output": "Phones", "gain_db": 0.0}]));
        c.execute("tile.set", json!({"tile": "Bed", "sends": "Stream,Phones"})).unwrap();
        assert_eq!(c.store.library.tile(c.resolve_tile("Bed").unwrap()).unwrap().sends.len(), 2);
        c.execute("tile.set", json!({"tile": "Bed", "sends": null})).unwrap();
        assert_eq!(sends(&mut c), json!([{"output": "Main", "gain_db": 0.0}]));
        // Removing an output drops its sends only.
        c.execute("tile.set", json!({"tile": "Bed", "sends": ["Stream", "Phones"]})).unwrap();
        c.execute("output.remove", json!({"name": "Stream"})).unwrap();
        assert_eq!(sends(&mut c), json!([{"output": "Phones", "gain_db": 0.0}]));
        c.execute("tile.play", json!({"tile": "Bed"})).unwrap();
        assert_eq!(c.execute("status", json!({})).unwrap()["playing"][0]["outputs"], json!(["Main"]), "Phones didn't open: on Main");
    }

    #[test]
    fn pitch_renders_a_copy_then_plays_it() {
        let (mut c, root) = controller();
        let ids = add(&mut c, &root, &["Voice"]);
        assert_eq!(c.execute("tile.set", json!({"tile": "Voice", "pitch": 13})).unwrap_err().code, "bad_params");
        assert_eq!(c.execute("tile.set", json!({"tile": "Voice", "pitch": -3})).unwrap()["pitch"], -3.0);
        // The render runs on the decode workers; wait for it, then the copy plays.
        let deadline = Instant::now() + Duration::from_secs(10);
        let tile = c.store.library.tile(ids[0]).unwrap().clone();
        while c.store.playable(&tile).is_err() && Instant::now() < deadline {
            c.store.poll();
            std::thread::sleep(Duration::from_millis(10));
        }
        let copy = c.store.playable(&tile).unwrap();
        let original = c.store.audio(&tile).unwrap();
        assert_eq!(copy.data.len(), original.data.len());
        assert!(!Arc::ptr_eq(&copy, &original));
        assert_eq!(c.execute("tile.play", json!({"tile": "Voice"})).unwrap()["state"], "playing");
    }

    #[test]
    fn pad_options_from_the_cli_and_how_presses_play() {
        let (mut c, root) = controller();
        let ids = add(&mut c, &root, &["A", "B"]);
        let state = |c: &Controller| c.engine.state(ids[0]);
        fn opts(r: &Value) -> (Option<bool>, Option<bool>, Option<bool>, Option<&str>) {
            (r["hold"].as_bool(), r["pausable"].as_bool(), r["one_shot"].as_bool(), r["mode"].as_str())
        }
        let r = c.execute("tile.set", json!({"tile": "A", "hold": true, "pausable": true})).unwrap();
        assert_eq!(opts(&r), (Some(true), Some(true), Some(false), Some("hold")), "mode: the nearest old one, for old scripts");
        // One-shot and Pausable turn each other off, and can't both be asked for.
        let r = c.execute("tile.set", json!({"tile": "A", "oneShot": true})).unwrap();
        assert_eq!(opts(&r), (Some(true), Some(false), Some(true), Some("hold")));
        let r = c.execute("tile.set", json!({"tile": "A", "oneshot": false, "pausable": true})).unwrap();
        assert_eq!(opts(&r), (Some(true), Some(true), Some(false), Some("hold")), "--oneshot works as well as --one-shot");
        assert_eq!(c.execute("tile.set", json!({"tile": "A", "pausable": true, "oneShot": true})).unwrap_err().code, "bad_params");
        // The deprecated mode sets all three.
        for (mode, want) in [("restart", (false, false, true)), ("pause", (false, true, false)), ("toggle", (false, false, false)), ("hold", (true, false, false))] {
            let r = c.execute("tile.set", json!({"tile": "A", "mode": mode})).unwrap();
            assert_eq!(opts(&r), (Some(want.0), Some(want.1), Some(want.2), Some(mode)), "{mode}");
        }
        assert_eq!(c.execute("tile.list", json!({})).unwrap()[0]["hold"], true, "tile.list reports them");
        // Hold + Pausable: letting go pauses, holding again resumes.
        c.execute("tile.set", json!({"tile": "A", "pausable": true})).unwrap();
        let event = |c: &mut Controller, e: &str| c.execute("tile.trigger", json!({"tile": "A", "event": e})).unwrap();
        event(&mut c, "press");
        assert_eq!(state(&c), PlayState::Playing);
        event(&mut c, "release");
        assert_eq!(state(&c), PlayState::Paused);
        event(&mut c, "press");
        assert_eq!(state(&c), PlayState::Playing);
        // Hold + One-shot: as plain Hold, and each hold starts from the beginning.
        c.execute("tile.set", json!({"tile": "A", "oneShot": true})).unwrap();
        event(&mut c, "release");
        assert_ne!(state(&c), PlayState::Playing);
        event(&mut c, "press");
        assert_eq!(state(&c), PlayState::Playing);
        // Pausable alone: a press pauses, the next resumes; the release does nothing.
        c.execute("tile.set", json!({"tile": "A", "hold": false, "pausable": true})).unwrap();
        event(&mut c, "release");
        assert_eq!(state(&c), PlayState::Playing);
        event(&mut c, "press");
        assert_eq!(state(&c), PlayState::Paused);
        event(&mut c, "press");
        assert_eq!(state(&c), PlayState::Playing);
        // None: a press while playing stops.
        c.execute("tile.set", json!({"tile": "A", "pausable": false})).unwrap();
        event(&mut c, "press");
        assert_ne!(state(&c), PlayState::Playing);
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
        assert_eq!(r["keyByHand"], true);
        // Taking its key sends Ding back to its slot's key (D-188).
        c.execute("tile.set", json!({"tile": "B", "hotkey": "q"})).unwrap();
        let ding = c.execute("tile.get", json!({"tile": "Ding"})).unwrap();
        assert_eq!((ding["key"].as_str(), &ding["keyByHand"]), (Some("1"), &json!(false)));
        // Option keys, by hand; none by hand; back to automatic.
        assert_eq!(c.execute("tile.set", json!({"tile": "B", "hotkey": "Alt+1"})).unwrap()["key"], "alt+1");
        assert_eq!(c.resolve_tile("key:alt+1").unwrap(), c.resolve_tile("B").unwrap());
        assert_eq!(c.execute("tile.set", json!({"tile": "B", "hotkey": "ctrl+1"})).unwrap_err().code, "bad_params");
        let none = c.execute("tile.set", json!({"tile": "B", "hotkey": null})).unwrap();
        assert_eq!((&none["key"], &none["keyByHand"]), (&Value::Null, &json!(true)));
        assert_eq!(c.execute("tile.set", json!({"tile": "B", "hotkey": "auto"})).unwrap()["key"], "2");
        c.execute("tile.set", json!({"tile": "Ding", "hotkey": "z"})).unwrap();
        c.execute("set.reassignKeys", json!({})).unwrap();
        assert_eq!(c.execute("tile.get", json!({"tile": "Ding"})).unwrap()["key"], "1", "reassigning drops the keys set by hand");
        assert_eq!(c.execute("tile.set", json!({"tile": "B", "trimStart": 0.3, "trimEnd": 0.2})).unwrap_err().code, "bad_params");
        assert_eq!(c.execute("tile.set", json!({"tile": "B", "colour": "red"})).unwrap_err().code, "bad_params");
        // Typing in the inspector: spaces and an empty field survive each keystroke.
        let b = c.resolve_tile("B").unwrap().to_string();
        for typed in ["Rim", "", "Rimshot"] {
            assert_eq!(c.execute("tile.set", json!({"tile": b, "title": typed})).unwrap()["title"], typed);
        }
        std::fs::remove_dir_all(root).ok();
    }

    /// Grid rows from the CLI (D-113, D-116): moving into rows and new rows, adding into a
    /// row, duplicates beside their original, evened-out rows, and the grid layout.
    #[test]
    fn blank_slots_from_the_cli() {
        let (mut c, root) = controller();
        let ids = add(&mut c, &root, &["a", "b", "c", "d", "e", "f", "g"]);
        let shape = |c: &mut Controller| c.execute("set.addBlank", json!({"row": 9, "slot": 1})).unwrap_err();
        assert_eq!(shape(&mut c).code, "bad_params", "no row 9");
        // [a _ _ b c d e] [f g]: two blanks before row 1's second slot.
        let r = c.execute("set.addBlank", json!({"row": 1, "slot": 2, "count": 2})).unwrap();
        assert_eq!(r["slots"], json!(["x..xxxx", "xx"]));
        let sets = c.execute("set.list", json!({})).unwrap();
        assert_eq!(sets[0]["blanks"], json!([[1, 2], [1, 3]]));
        // Reading order skips them: b is still tile 2, and says it sits in slot 4.
        let g = c.execute("tile.get", json!({"tile": "#2"})).unwrap();
        assert_eq!((g["id"].clone(), g["row"].as_u64(), g["slot"].as_u64()), (json!(ids[1]), Some(1), Some(4)));
        // tile.move into a blank slot fills it; nothing else in that row moves.
        let r = c.execute("tile.move", json!({"tile": ids[6], "row": 1, "slot": 3})).unwrap();
        assert_eq!((r["position"].as_u64(), r["slot"].as_u64()), (Some(2), Some(3)));
        let slots = |c: &Controller| c.current_set().slots().iter().map(|r| r.iter().map(|x| x.map_or('.', |i| c.current_set().tiles[i].title.chars().next().unwrap())).collect::<String>()).collect::<Vec<_>>();
        assert_eq!(slots(&c), ["a.gbcde", "f"]);
        // tile.add into a slot holding a tile: it goes in before it.
        let p = root.join("h.wav");
        wav::write_tone(&p, 8000, 0.5, &[330.0], 0.4).unwrap();
        c.execute("tile.add", json!({"paths": [p], "row": 1, "slot": 3})).unwrap();
        assert_eq!(slots(&c), ["a.hgbcde", "f"]);
        // Removing: only a blank.
        assert_eq!(c.execute("set.removeBlank", json!({"row": 1, "slot": 1})).unwrap_err().code, "not_found");
        assert_eq!(c.execute("set.removeBlank", json!({"row": 1, "slot": 2})).unwrap()["slots"], json!(["xxxxxxx", "x"]));
        // Undo brings it back; evening rows out clears blanks.
        c.execute("edit.undo", json!({})).unwrap();
        assert_eq!(slots(&c), ["a.hgbcde", "f"]);
        c.execute("set.rows", json!({"perRow": 4})).unwrap();
        assert!(c.current_set().blanks.is_empty());
        // leaveBlank: a blank stays in its old slot, nothing else moves (D-202).
        assert_eq!(slots(&c), ["ahgb", "cdef"]);
        let r = c.execute("tile.move", json!({"tile": ids[6], "row": 1, "slot": 5, "leaveBlank": true})).unwrap();
        assert_eq!((r["position"].as_u64(), slots(&c)), (Some(4), vec!["ah.bg".to_owned(), "cdef".to_owned()]));
        std::fs::remove_dir_all(root).ok();
    }

    /// Several tiles moved together (D-219): one run in the set's order, one undo step,
    /// into another set too, and never into or out of a locked set.
    #[test]
    fn several_tiles_move_together() {
        let (mut c, root) = controller();
        let ids = add(&mut c, &root, &["a", "b", "c", "d", "e", "f", "g"]);
        let slots = |c: &Controller| c.current_set().slots().iter().map(|r| r.iter().map(|x| x.map_or('.', |i| c.current_set().tiles[i].title.chars().next().unwrap())).collect::<String>()).collect::<Vec<_>>();
        assert_eq!(slots(&c), ["abcde", "fg"]);
        // f and b (in any order) to the front of row 1: b first, as in the set.
        let r = c.execute("tile.move", json!({"tiles": [ids[5], ids[1]], "row": 1, "at": 1})).unwrap();
        assert_eq!(slots(&c), ["bfacde", "g"]);
        assert_eq!(r["tiles"][0], json!({"tile": ids[1], "position": 1, "row": 1, "slot": 1}));
        assert_eq!(r["tiles"][1]["position"], 2);
        // One undo step puts both back.
        c.execute("edit.undo", json!({})).unwrap();
        assert_eq!(slots(&c), ["abcde", "fg"]);
        // A new row of their own; with leaveBlank, their old slots stay.
        c.execute("tile.move", json!({"tiles": [ids[0], ids[2]], "row": 3, "newRow": true})).unwrap();
        assert_eq!(slots(&c), ["bde", "fg", "ac"]);
        c.execute("tile.move", json!({"tiles": [ids[5], ids[6]], "row": 1, "leaveBlank": true})).unwrap();
        // (Row 2 is left with blanks only, so it goes.)
        assert_eq!(slots(&c), ["bdefg", "ac"]);
        // Into another set: at its end by default, out of this one, one undo step.
        let other = c.execute("set.create", json!({"name": "Other"})).unwrap()["id"].as_str().unwrap().to_owned();
        c.execute("set.select", json!({"set": "#1"})).unwrap();
        let r = c.execute("tile.move", json!({"tiles": [ids[3], ids[4]], "set": other})).unwrap();
        assert_eq!((r["set"].as_str(), r["tiles"][1]["position"].as_u64()), (Some("Other"), Some(2)));
        assert_eq!(slots(&c), ["bfg", "ac"]);
        let moved: Vec<Uuid> = c.store.library.sets[1].tiles.iter().map(|t| t.id).collect();
        assert_eq!(moved, [ids[3], ids[4]]);
        c.execute("edit.undo", json!({})).unwrap();
        assert_eq!((slots(&c), c.store.library.sets[1].tiles.len()), (vec!["bdefg".to_owned(), "ac".to_owned()], 0));
        // One tile, with a place in the other set.
        c.execute("tile.move", json!({"tiles": [ids[3], ids[4]], "set": other})).unwrap();
        let r = c.execute("tile.move", json!({"tile": ids[0], "set": other, "to": 1})).unwrap();
        assert_eq!((r["set"].as_str(), r["position"].as_u64()), (Some("Other"), Some(1)));
        // Refused: tiles from two sets, leaveBlank across sets, an empty list.
        let e = |c: &mut Controller, p: Value| c.execute("tile.move", p).unwrap_err().code;
        assert_eq!(e(&mut c, json!({"tiles": [ids[0], ids[1]], "to": 1})), "bad_params");
        assert_eq!(e(&mut c, json!({"tiles": [ids[1]], "set": other, "leaveBlank": true})), "bad_params");
        assert_eq!(e(&mut c, json!({"tiles": [], "to": 1})), "bad_params");
        // A locked set: nothing moves into it, out of it or inside it.
        c.execute("set.lock", json!({"set": other})).unwrap();
        assert_eq!(e(&mut c, json!({"tiles": [ids[1], ids[2]], "set": other})), "locked");
        assert_eq!(e(&mut c, json!({"tiles": [ids[3], ids[4]], "set": "#1"})), "locked");
        assert_eq!(e(&mut c, json!({"tiles": [ids[3], ids[4]], "to": 1})), "locked");
        std::fs::remove_dir_all(root).ok();
    }

    /// tile.copy (D-229, D-230): copies at a place on the board as it is, the originals
    /// untouched, one undo step; into another set at its end, a hand-set key going along
    /// only where it's free; never into or out of a locked set.
    #[test]
    fn tiles_copy_and_the_originals_stay() {
        let (mut c, root) = controller();
        let ids = add(&mut c, &root, &["a", "b", "c", "d", "e", "f", "g"]);
        let slots = |c: &Controller| c.current_set().slots().iter().map(|r| r.iter().map(|x| x.map_or('.', |i| c.current_set().tiles[i].title.chars().next().unwrap())).collect::<String>()).collect::<Vec<_>>();
        assert_eq!(slots(&c), ["abcde", "fg"]);
        c.execute("tile.set", json!({"tile": ids[1], "color": "red", "volume": 0.5, "loop": true})).unwrap();
        // b and f (any order) into row 2's first gap: b first, as in the set.
        let r = c.execute("tile.copy", json!({"tiles": [ids[5], ids[1]], "row": 2, "at": 1})).unwrap();
        assert_eq!(slots(&c), ["abcde", "bffg"]);
        let copy = r["tiles"][0]["tile"].as_str().unwrap().parse::<Uuid>().unwrap();
        assert_eq!(r["tiles"][0]["copyOf"], json!(ids[1]));
        assert_eq!((r["tiles"][0]["row"].as_u64(), r["tiles"][0]["slot"].as_u64()), (Some(2), Some(1)));
        let (orig, dup) = (c.store.library.tile(ids[1]).unwrap().clone(), c.store.library.tile(copy).unwrap().clone());
        assert_eq!(Tile { id: orig.id, hotkey: orig.hotkey, ..dup }, orig, "a full duplicate, apart from its id and slot key");
        assert!(ids.iter().all(|id| c.store.library.tile(*id).is_some()), "the originals stay");
        c.execute("edit.undo", json!({})).unwrap();
        assert_eq!(slots(&c), ["abcde", "fg"], "one undo step");
        // A blank filled, a slot past a row's end, a new row.
        c.execute("tile.copy", json!({"tile": ids[0], "row": 2, "slot": 4})).unwrap();
        assert_eq!(slots(&c), ["abcde", "fg.a"]);
        c.execute("tile.copy", json!({"tile": ids[1], "row": 2, "slot": 3})).unwrap();
        assert_eq!(slots(&c), ["abcde", "fgba"]);
        c.execute("tile.copy", json!({"tile": ids[2], "row": 3, "newRow": true})).unwrap();
        assert_eq!(slots(&c), ["abcde", "fgba", "c"]);
        // Into another set: at its end; a hand-set key goes along where it's free.
        let other = c.execute("set.create", json!({"name": "Other"})).unwrap()["id"].as_str().unwrap().to_owned();
        c.execute("set.select", json!({"set": "#1"})).unwrap();
        c.execute("tile.set", json!({"tile": ids[3], "hotkey": "alt+z"})).unwrap();
        let r = c.execute("tile.copy", json!({"tiles": [ids[3], ids[4]], "set": other})).unwrap();
        assert_eq!((r["set"].as_str(), r["tiles"][1]["position"].as_u64()), (Some("Other"), Some(2)));
        let o = &c.store.library.sets[1].tiles;
        assert_eq!((o[0].title.as_str(), o[0].hotkey.map(|k| k.name()), o[0].hotkey_manual), ("d", Some("alt+z".to_owned()), true));
        // A second copy there: the key is taken, so it doesn't go along.
        let r = c.execute("tile.copy", json!({"tile": ids[3], "set": other})).unwrap();
        let again = r["tiles"][0]["tile"].as_str().unwrap().parse::<Uuid>().unwrap();
        assert!(!c.store.library.tile(again).unwrap().hotkey_manual);
        assert_eq!(c.store.library.tile(ids[3]).unwrap().hotkey.map(|k| k.name()), Some("alt+z".to_owned()), "the original keeps it");
        // Refused: tiles from two sets, an empty list; a locked set either way.
        let e = |c: &mut Controller, p: Value| c.execute("tile.copy", p).unwrap_err().code;
        let there = c.store.library.sets[1].tiles[0].id;
        assert_eq!(e(&mut c, json!({"tiles": [ids[0], there]})), "bad_params");
        assert_eq!(e(&mut c, json!({"tiles": []})), "bad_params");
        c.execute("set.lock", json!({"set": other})).unwrap();
        assert_eq!(e(&mut c, json!({"tile": ids[0], "set": other})), "locked");
        assert_eq!(e(&mut c, json!({"tile": there, "set": "#1"})), "locked");
        std::fs::remove_dir_all(root).ok();
    }

    /// view.select remove (Alt-click, D-229): out of the selection if it's in it.
    #[test]
    fn view_select_remove_takes_a_tile_out() {
        let (mut c, root) = controller();
        let ids = add(&mut c, &root, &["a", "b", "c", "d"]);
        c.execute("view.select", json!({"tiles": [ids[0], ids[1], ids[2]]})).unwrap();
        let r = c.execute("view.select", json!({"tile": ids[1], "remove": true})).unwrap();
        assert_eq!(r["selected"], json!([ids[0], ids[2]]));
        let r = c.execute("view.select", json!({"tile": ids[3], "remove": true})).unwrap();
        assert_eq!(r["selected"], json!([ids[0], ids[2]]), "an unselected tile changes nothing");
        c.execute("view.select", json!({"tile": ids[0], "remove": true})).unwrap();
        let r = c.execute("view.select", json!({"tile": ids[2], "remove": true})).unwrap();
        assert_eq!(r["selected"], json!([]), "the last one out: nothing selected");
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn rows_commands() {
        let (mut c, root) = controller();
        let names = ["a", "b", "c", "d", "e", "f", "g"];
        let ids = add(&mut c, &root, &names);
        let rows = |c: &Controller| c.current_set().row_lengths();
        let titles = |c: &Controller| c.current_set().tiles.iter().map(|t| t.title.clone()).collect::<Vec<_>>().join("");
        assert_eq!(rows(&c), vec![5, 2], "a new set: rows of its columns");
        assert_eq!(c.execute("set.rows", json!({"rows": "3, 4"})).unwrap()["rows"], json!([3, 4]));
        assert_eq!(c.execute("set.rows", json!({"rows": [3, 5]})).unwrap_err().code, "bad_params", "more than the tiles");
        assert_eq!(c.execute("set.rows", json!({"rows": [0, 2]})).unwrap_err().code, "bad_params");
        // Into row 2 at place 1; the reply says where it went.
        let r = c.execute("tile.move", json!({"tile": ids[0], "row": 2, "at": 1})).unwrap();
        assert_eq!((r["position"].as_u64(), r["row"].as_u64(), r["at"].as_u64()), (Some(3), Some(2), Some(1)));
        assert_eq!((titles(&c), rows(&c)), ("bcadefg".into(), vec![2, 5]));
        // A new row at the end, then a new first row.
        c.execute("tile.move", json!({"tile": ids[6], "row": 3, "newRow": true})).unwrap();
        c.execute("tile.move", json!({"tile": ids[5], "row": 1, "newRow": true})).unwrap();
        assert_eq!((titles(&c), rows(&c)), ("fbcadeg".into(), vec![1, 2, 3, 1]));
        // The old flat `to` still lands at that position.
        assert_eq!(c.execute("tile.move", json!({"tile": ids[5], "to": 4})).unwrap()["position"], 4);
        assert_eq!((titles(&c), rows(&c)), ("bcafdeg".into(), vec![2, 4, 1]), "its row went; it joined the row of the tile it displaced");
        assert_eq!(c.execute("tile.move", json!({"tile": ids[5]})).unwrap_err().code, "bad_params");
        // tile.get and tile.list say where a tile is.
        let g = c.execute("tile.get", json!({"tile": ids[6]})).unwrap();
        assert_eq!((g["row"].as_u64(), g["column"].as_u64()), (Some(3), Some(1)));
        // A duplicate sits beside its original, in its row.
        let dup = c.execute("tile.duplicate", json!({"tile": ids[1]})).unwrap()["id"].clone();
        assert_eq!((rows(&c), c.current_set().tiles[1].id), (vec![3, 4, 1], serde_json::from_value(dup).unwrap()));
        // Removing a row's only tile removes the row.
        c.execute("tile.remove", json!({"tile": ids[6]})).unwrap();
        assert_eq!(rows(&c), vec![3, 4]);
        // Files added into a row, or as a new row.
        let p = root.join("h.wav");
        wav::write_tone(&p, 8000, 0.5, &[330.0], 0.4).unwrap();
        c.execute("tile.add", json!({"paths": [p], "row": 1, "at": 1})).unwrap();
        c.execute("tile.add", json!({"paths": [p], "row": 2, "newRow": true})).unwrap();
        assert_eq!((rows(&c), c.current_set().tiles[0].title.as_str(), c.current_set().tiles[4].title.as_str()), (vec![4, 1, 4], "h", "h"));
        // Evening out, and the list.
        c.execute("set.rows", json!({"perRow": 3})).unwrap();
        let sets = c.execute("set.list", json!({})).unwrap();
        assert_eq!((sets[0]["rows"].clone(), sets[0]["columns"].as_u64(), sets[0]["grid"].as_str()), (json!([3, 3, 3]), Some(3), Some("rows")));
        assert_eq!(c.execute("settings.gridLayout", json!({"mode": "auto"})).unwrap()["mode"], "auto");
        assert_eq!(c.store.library.grid, GridMode::Auto);
        assert_eq!(c.execute("settings.gridLayout", json!({"mode": "wide"})).unwrap_err().code, "bad_params");
        // Undo puts rows back with the tiles.
        c.execute("tile.move", json!({"tile": ids[2], "row": 4, "newRow": true})).unwrap();
        c.execute("edit.undo", json!({})).unwrap();
        assert_eq!(rows(&c), vec![3, 3, 3]);
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
        c.execute("view.appearance", json!({"appearance": "high_contrast"})).unwrap();
        assert_eq!(c.store.library.appearance, Appearance::HighContrast);
        // Standard leaves high contrast (Panel's way out), and otherwise changes nothing.
        c.execute("view.appearance", json!({"appearance": "standard"})).unwrap();
        assert_eq!(c.store.library.appearance, Appearance::Dark);
        c.execute("view.appearance", json!({"appearance": "light"})).unwrap();
        c.execute("view.appearance", json!({"appearance": "standard"})).unwrap();
        assert_eq!(c.store.library.appearance, Appearance::Light);
        assert_eq!(c.execute("view.zoom", json!({"zoom": "in"})).unwrap()["zoom"], 110);
        assert_eq!(c.execute("view.zoom", json!({"zoom": 150})).unwrap()["zoom"], 150);
        assert_eq!(c.execute("view.zoom", json!({"zoom": "out"})).unwrap()["zoom"], 125);
        assert_eq!(c.execute("view.zoom", json!({"zoom": "200%"})).unwrap()["zoom"], 200);
        assert_eq!(c.execute("view.zoom", json!({"zoom": 301})).unwrap_err().code, "bad_params");
        assert_eq!(c.execute("view.zoom", json!({"zoom": "huge"})).unwrap_err().code, "bad_params");
        assert_eq!(c.execute("view.zoom", json!({"zoom": "reset"})).unwrap()["zoom"], 100);
        for _ in 0..20 {
            c.execute("view.zoom", json!({"zoom": "in"})).unwrap();
        }
        assert_eq!(c.store.library.zoom, ZOOM_MAX, "in stops at the top");
        c.execute("view.zoom", json!({"zoom": 100})).unwrap();
        c.execute("view.toggleEdit", json!({})).unwrap();
        assert!(!c.store.library.show_inspector);
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
        assert_eq!(c.effects, [Effect::PickAudio { set, place: Place::Append }, Effect::PickPicture(ids[0]), Effect::PickReplacement(ids[0])]);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn channel_mix_for_multichannel_files() {
        let (mut c, root) = controller();
        add(&mut c, &root, &["Stereo"]);
        let path = root.join("surround.wav");
        // L R C LFE Ls Rs, 0.5 s.
        wav::write_multichannel(&path, 8000, 0x60F, &[0.1f32, 0.2, 0.3, 0.4, 0.05, 0.06].repeat(4000)).unwrap();
        c.execute("tile.add", json!({"paths": [path]})).unwrap();
        wait_loaded(&mut c);
        let r = c.execute("tile.get", json!({"tile": "surround"})).unwrap();
        assert_eq!((&r["channels"], &r["channel_layout"]), (&json!("auto"), &json!(["L", "R", "C", "LFE", "Ls", "Rs"])));
        let r = c.execute("tile.set", json!({"tile": "surround", "channels": "left, right, both:-3, mute, left:-6, right:-6"})).unwrap();
        assert_eq!(r["channels"][2], json!({"to": "both", "gain_db": -3.0}));
        assert_eq!(r["channels"][3]["to"], "mute");
        let r = c.execute("tile.set", json!({"tile": "surround", "channels": [{"to": "right"}, "l", "mute", "mute", "mute", "mute"]})).unwrap();
        assert_eq!((&r["channels"][0]["to"], &r["channels"][1]["to"]), (&json!("right"), &json!("left")));
        assert_eq!(c.execute("tile.set", json!({"tile": "surround", "channels": "left,right"})).unwrap_err().code, "bad_params", "one per channel");
        assert_eq!(c.execute("tile.set", json!({"tile": "surround", "channels": "up,down"})).unwrap_err().code, "bad_params");
        assert_eq!(c.execute("tile.set", json!({"tile": "Stereo", "channels": "left,right"})).unwrap_err().code, "bad_params", "stereo has no mix");
        c.execute("tile.play", json!({"tile": "surround"})).unwrap();
        assert_eq!(c.execute("tile.set", json!({"tile": "surround", "channels": "auto"})).unwrap()["channels"], "auto");
    }
}
