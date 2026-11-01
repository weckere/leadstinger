//! The persisted library: sets of tiles. Every struct is `#[serde(default)]` so files
//! written by older or newer versions still load.

use crate::bindings::{Binding, MidiSettings};
use crate::logic::PadKey;
use serde::{Deserialize, Deserializer, Serialize};
use uuid::Uuid;

/// 2: `stop_fade` split from `fade_out`, and Stop All immediate by default (D-067).
/// 3: grid sets store their rows (D-113).
/// 4: grid rows are rows of slots, which may be blank (D-140); tiles have the pad options
///    Hold, Pausable and One-shot in place of `mode`, which is still written (D-145).
/// 5: pad keys follow the board's rows and may have Option (`alt+q`); a tile says when
///    its key was set by hand (D-188, D-193).
pub const LIBRARY_VERSION: u32 = 5;

/// The longest pre-wait or post-wait, in seconds (an hour).
pub const WAIT_MAX: f64 = 3600.0;

/// How long Esc is held for Stop All with the hold guard on.
pub const STOP_ALL_HOLD: std::time::Duration = std::time::Duration::from_millis(300);

/// The shortest fade any stop gets (a pad's stop with no stop fade, Stop All at once, a
/// restart's old voice, a pause), so nothing clicks or pops (D-205; 5 ms before).
pub const DECLICK_SECONDS: f32 = 0.1;

/// The single play mode tiles had before the pad options (D-145). Still read from older
/// libraries, set files and starter packs, written beside the options for older builds,
/// and taken by `tile.set mode` as a deprecated alias.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlayMode {
    #[default]
    Toggle,
    Restart,
    Hold,
    Pause,
}

/// What a press does to a pad: the options Hold, Pausable and One-shot (D-145). With
/// none, a press plays it and a press while it plays stops it. Pausable and One-shot
/// exclude each other; Hold goes with either.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(from = "PressFile", into = "PressFile")]
pub struct PressOptions {
    /// It plays while held and stops on release (with Pausable, pauses).
    pub hold: bool,
    /// A press while it plays pauses it, and the next resumes.
    pub pausable: bool,
    /// A press while it plays starts it again from the beginning.
    pub one_shot: bool,
}

impl PressOptions {
    pub const fn from_mode(mode: PlayMode) -> PressOptions {
        let off = PressOptions { hold: false, pausable: false, one_shot: false };
        match mode {
            PlayMode::Toggle => off,
            PlayMode::Restart => PressOptions { one_shot: true, ..off },
            PlayMode::Hold => PressOptions { hold: true, ..off },
            PlayMode::Pause => PressOptions { pausable: true, ..off },
        }
    }

    /// The nearest old mode, for older builds (Hold with Pausable reads as Hold there).
    pub fn mode(self) -> PlayMode {
        if self.hold {
            PlayMode::Hold
        } else if self.pausable {
            PlayMode::Pause
        } else if self.one_shot {
            PlayMode::Restart
        } else {
            PlayMode::Toggle
        }
    }

    /// The options as a file gives them: any of the three that's there wins, and a file
    /// with only the old `mode` (from before D-145) gets its mapping.
    pub fn read(mode: Option<PlayMode>, hold: Option<bool>, pausable: Option<bool>, one_shot: Option<bool>) -> PressOptions {
        if hold.is_none() && pausable.is_none() && one_shot.is_none() {
            return PressOptions::from_mode(mode.unwrap_or_default());
        }
        let mut p = PressOptions { hold: hold.unwrap_or(false), ..Default::default() };
        // Both ticked (a file edited by hand): One-shot, as the old Always Restart.
        p.set_pausable(pausable.unwrap_or(false));
        p.set_one_shot(one_shot.unwrap_or(false));
        p
    }

    /// Tick or untick Pausable; ticking it unticks One-shot.
    pub fn set_pausable(&mut self, on: bool) {
        self.pausable = on;
        if on {
            self.one_shot = false;
        }
    }

    /// Tick or untick One-shot; ticking it unticks Pausable.
    pub fn set_one_shot(&mut self, on: bool) {
        self.one_shot = on;
        if on {
            self.pausable = false;
        }
    }
}

/// How [`PressOptions`] is stored: the options, plus the old `mode` for older builds
/// (read as [`PressOptions::read`] says).
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct PressFile {
    mode: Option<PlayMode>,
    hold: Option<bool>,
    pausable: Option<bool>,
    one_shot: Option<bool>,
}

impl From<PressFile> for PressOptions {
    fn from(f: PressFile) -> PressOptions {
        PressOptions::read(f.mode, f.hold, f.pausable, f.one_shot)
    }
}

impl From<PressOptions> for PressFile {
    fn from(p: PressOptions) -> PressFile {
        PressFile { mode: Some(p.mode()), hold: Some(p.hold), pausable: Some(p.pausable), one_shot: Some(p.one_shot) }
    }
}

/// An extra output tiles can be routed to: a device and a channel pair on it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OutputDef {
    pub name: String,
    /// Device by name; `None` is the system default.
    pub device: Option<String>,
    /// The first of its two channels, from 1 (3 means channels 3 and 4).
    pub channel: u16,
}

impl Default for OutputDef {
    fn default() -> Self {
        OutputDef { name: "Output".into(), device: None, channel: 1 }
    }
}

/// The built-in output every library has.
pub const MAIN_OUTPUT: &str = "Main";

/// One place a tile plays: an output, by name ([`MAIN_OUTPUT`] is Main), at its own
/// level on top of the tile's. A tile has one voice per send, all reading the same
/// decoded audio.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TileSend {
    pub output: String,
    /// dB on top of the tile's volume and gain, ±GAIN_DB_MAX.
    #[serde(alias = "gainDb")]
    pub gain_db: f32,
}

impl Default for TileSend {
    fn default() -> Self {
        TileSend { output: MAIN_OUTPUT.into(), gain_db: 0.0 }
    }
}

impl TileSend {
    pub fn to(output: impl Into<String>) -> Self {
        TileSend { output: output.into(), gain_db: 0.0 }
    }

    pub fn is_main(&self) -> bool {
        self.output.eq_ignore_ascii_case(MAIN_OUTPUT)
    }
}

/// Keep one send per output (the first), with levels in range, and store the plain
/// "Main at 0 dB" as no sends at all, the default.
pub fn tidy_sends(sends: &mut Vec<TileSend>) {
    let mut seen: Vec<String> = Vec::new();
    sends.retain_mut(|s| {
        if s.is_main() {
            s.output = MAIN_OUTPUT.into();
        }
        s.gain_db = if s.gain_db.is_finite() { s.gain_db.clamp(-GAIN_DB_MAX, GAIN_DB_MAX) } else { 0.0 };
        let key = s.output.to_lowercase();
        let fresh = !s.output.trim().is_empty() && !seen.contains(&key);
        seen.push(key);
        fresh
    });
    if sends.len() == 1 && sends[0].is_main() && sends[0].gain_db == 0.0 {
        sends.clear();
    }
}

/// A tile's `sends`, or (libraries from before sends) its single `output`: a name, or
/// null for Main.
fn sends_or_output<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<TileSend>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Either {
        Sends(Vec<TileSend>),
        Output(Option<String>),
    }
    let mut sends = match Either::deserialize(d)? {
        Either::Sends(s) => s,
        Either::Output(o) => o.map(TileSend::to).into_iter().collect(),
    };
    tidy_sends(&mut sends);
    Ok(sends)
}

/// Where one channel of a multichannel file goes in a tile's own channel mix.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelTo {
    Mute,
    #[serde(alias = "l")]
    Left,
    #[serde(alias = "r")]
    Right,
    #[default]
    Both,
}

impl ChannelTo {
    pub const ALL: [ChannelTo; 4] = [ChannelTo::Left, ChannelTo::Right, ChannelTo::Both, ChannelTo::Mute];

    pub fn label(self) -> &'static str {
        match self {
            ChannelTo::Mute => "Mute",
            ChannelTo::Left => "L",
            ChannelTo::Right => "R",
            ChannelTo::Both => "L+R",
        }
    }
}

/// One source channel in a tile's channel mix: where it goes and at what level.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChannelRoute {
    pub to: ChannelTo,
    /// dB, ±GAIN_DB_MAX.
    #[serde(alias = "gainDb")]
    pub gain_db: f32,
}

impl ChannelRoute {
    /// The (left, right) linear gains it adds this channel at.
    pub fn gains(&self) -> (f32, f32) {
        let g = crate::logic::db_to_gain(self.gain_db);
        match self.to {
            ChannelTo::Mute => (0.0, 0.0),
            ChannelTo::Left => (g, 0.0),
            ChannelTo::Right => (0.0, g),
            ChannelTo::Both => (g, g),
        }
    }

    /// The route nearest to a downmix gain pair, to start a custom mix from Auto.
    pub fn from_gains((l, r): (f32, f32)) -> Self {
        let db = |g: f32| ((20.0 * g.log10()) * 10.0).round() / 10.0;
        match (l > 0.0, r > 0.0) {
            (true, true) => ChannelRoute { to: ChannelTo::Both, gain_db: db(l.max(r)) },
            (true, false) => ChannelRoute { to: ChannelTo::Left, gain_db: db(l) },
            (false, true) => ChannelRoute { to: ChannelTo::Right, gain_db: db(r) },
            (false, false) => ChannelRoute { to: ChannelTo::Mute, gain_db: 0.0 },
        }
    }
}

/// Keep a channel mix's levels in range.
pub fn tidy_channel_mix(mix: &mut [ChannelRoute]) {
    for c in mix {
        c.gain_db = if c.gain_db.is_finite() { c.gain_db.clamp(-GAIN_DB_MAX, GAIN_DB_MAX) } else { 0.0 };
    }
}

/// A tile's channel mix as (left, right) gains per source channel, for a file with
/// `channels` channels; `None` is Auto (the BS.775 downmix): no mix, or one made for a
/// file with a different number of channels.
pub fn channel_gains(mix: &[ChannelRoute], channels: usize) -> Option<Vec<(f32, f32)>> {
    (!mix.is_empty() && mix.len() == channels).then(|| mix.iter().map(ChannelRoute::gains).collect())
}

/// A named point in a tile's audio to jump to (`tile.jump`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Marker {
    pub name: String,
    /// Seconds into the file.
    pub at: f64,
}

/// The shape of a tile's fades (in, out, and when it's stopped).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FadeCurve {
    /// Gain follows t²: eases in, the long-standing default.
    #[default]
    Gentle,
    Linear,
    /// Constant power across a crossfade: sin(t·π/2).
    EqualPower,
    /// Even in decibels (a 60 dB ramp): sounds steady on long music fades.
    Exponential,
    /// Slow at both ends.
    SCurve,
}

impl FadeCurve {
    pub const ALL: [FadeCurve; 5] = [FadeCurve::Gentle, FadeCurve::Linear, FadeCurve::EqualPower, FadeCurve::Exponential, FadeCurve::SCurve];

    pub fn label(self) -> &'static str {
        match self {
            FadeCurve::Gentle => "Gentle",
            FadeCurve::Linear => "Linear",
            FadeCurve::EqualPower => "Equal power",
            FadeCurve::Exponential => "Exponential (dB)",
            FadeCurve::SCurve => "S-curve",
        }
    }

    /// Gain for `t` in 0..=1 (0 silent, 1 full).
    pub fn gain(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            FadeCurve::Gentle => t * t,
            FadeCurve::Linear => t,
            FadeCurve::EqualPower => (t * std::f32::consts::FRAC_PI_2).sin(),
            FadeCurve::Exponential if t <= 0.0 => 0.0,
            FadeCurve::Exponential => 10f32.powf(-60.0 * (1.0 - t) / 20.0),
            FadeCurve::SCurve => (1.0 - (t * std::f32::consts::PI).cos()) / 2.0,
        }
    }
}

/// A tile's part in ducking: while any `Ducks` tile plays (or the duck is held), every
/// `Ducked` tile dips by the library's duck amount.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Duck {
    #[default]
    None,
    /// A voice or stinger: beds dip while it plays.
    Ducks,
    /// A bed: dips under `Ducks` tiles and the talk-over.
    Ducked,
}

impl Duck {
    pub const ALL: [Duck; 3] = [Duck::None, Duck::Ducks, Duck::Ducked];

    pub fn label(self) -> &'static str {
        match self {
            Duck::None => "No ducking",
            Duck::Ducks => "Ducks beds",
            Duck::Ducked => "Bed (gets ducked)",
        }
    }

    pub fn help(self) -> &'static str {
        match self {
            Duck::None => "Plays at its own level.",
            Duck::Ducks => "While this plays, tiles marked as beds dip.",
            Duck::Ducked => "Dips under tiles that duck beds, and under the talk-over.",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TileColor {
    Red,
    Orange,
    Yellow,
    Lime,
    Green,
    Mint,
    Teal,
    Cyan,
    #[default]
    Blue,
    Indigo,
    Purple,
    Pink,
    Rose,
    Crimson,
    Brown,
    Gray,
}

impl TileColor {
    /// Gray stays last: `nth` skips it.
    pub const ALL: [TileColor; 16] = [
        TileColor::Red,
        TileColor::Orange,
        TileColor::Yellow,
        TileColor::Lime,
        TileColor::Green,
        TileColor::Mint,
        TileColor::Teal,
        TileColor::Cyan,
        TileColor::Blue,
        TileColor::Indigo,
        TileColor::Purple,
        TileColor::Pink,
        TileColor::Rose,
        TileColor::Crimson,
        TileColor::Brown,
        TileColor::Gray,
    ];

    /// Cycle through the palette so newly added tiles get varied colours.
    pub fn nth(n: usize) -> TileColor {
        // Skip gray in the rotation; it reads as "disabled".
        TileColor::ALL[n % (TileColor::ALL.len() - 1)]
    }
}

/// Where a tile's picture sits relative to its waveform and progress fill.
/// The title and badges are always drawn on top.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageLayer {
    #[default]
    Underlay,
    Overlay,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageFit {
    /// Cover the whole tile, cropping the picture if needed.
    #[default]
    Fill,
    /// Show the whole picture centred inside the tile (good for icons).
    Fit,
}

pub const DEFAULT_IMAGE_OPACITY: f32 = 0.35;

/// A tile's gain goes from −GAIN_DB_MAX to +GAIN_DB_MAX dB.
pub const GAIN_DB_MAX: f32 = 24.0;

/// The longest instant-replay buffer, in seconds.
pub const REPLAY_MAX: u32 = 300;

/// The input level, dBFS, above which a voice ducks the beds by default.
pub const VOICE_THRESHOLD_DEFAULT: f32 = -40.0;

/// The slowest and fastest a tile plays.
pub const SPEED_MIN: f32 = 0.5;
pub const SPEED_MAX: f32 = 2.0;

/// How far ducked tiles dip by default, in dB.
pub const DUCK_DB_DEFAULT: f32 = -12.0;

/// The app's visual design. Console is the SoundCraft-style mixing desk; Panel is a retro
/// sci-fi control-panel look (always on black).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeKind {
    #[default]
    Console,
    /// Shown as [`crate::brand::PANEL_THEME`]. Stored as
    /// "padd", so libraries stay readable by older builds; "panel" is accepted too.
    #[serde(rename = "padd", alias = "panel")]
    Panel,
}

/// Light or dark UI. Dark is the default, as on a mixing desk.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Appearance {
    #[default]
    Dark,
    Light,
    /// Follow the operating system.
    System,
    /// White on black with outlined controls (M20, D-085); the Panel theme has its own
    /// high-contrast variant.
    HighContrast,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Layout {
    #[default]
    Grid,
    List,
}

/// How grid sets lay out their tiles (Settings > Grid layout, D-114).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GridMode {
    /// Each row holds the tiles put in it; a fuller row's tiles narrow (D-111).
    #[default]
    Rows,
    /// One column count for the whole grid, as many as fit everything without scrolling.
    Auto,
    /// The set's `columns`, wrapping (fewer when tiles would get too narrow).
    Columns,
}

impl GridMode {
    pub const ALL: [GridMode; 3] = [GridMode::Rows, GridMode::Auto, GridMode::Columns];

    pub fn label(self) -> &'static str {
        match self {
            GridMode::Rows => "Rows",
            GridMode::Auto => "Auto columns",
            GridMode::Columns => "Fixed columns",
        }
    }
}

/// What starts the cue after this one in its set, without a GO (theatre's cue linking).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CueLink {
    /// The next cue waits for GO.
    #[default]
    None,
    /// Auto-continue: the next cue starts `post_wait` seconds after this one starts.
    Continue,
    /// Auto-follow: the next cue starts when this one ends (plus `post_wait`).
    Follow,
}

impl CueLink {
    pub const ALL: [CueLink; 3] = [CueLink::None, CueLink::Continue, CueLink::Follow];

    pub fn label(self) -> &'static str {
        match self {
            CueLink::None => "Waits for GO",
            CueLink::Continue => "Starts with this one (after the post-wait)",
            CueLink::Follow => "Starts when this one ends",
        }
    }
}

/// What an action tile does to its targets (theatre's fade and stop cues).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    /// Fade the targets out over the action's seconds, then stop them.
    #[default]
    Fade,
    /// Stop them with their own fade-out.
    Stop,
    Pause,
    /// Let a vamping target play on to its end. Called Release before D-156, and
    /// still read under that name.
    #[serde(alias = "release", alias = "playon", alias = "play-on")]
    PlayOn,
    /// A group: play every target, each after its offset.
    Play,
}

impl ActionKind {
    pub const ALL: [ActionKind; 5] = [ActionKind::Play, ActionKind::Fade, ActionKind::Stop, ActionKind::Pause, ActionKind::PlayOn];

    pub fn label(self) -> &'static str {
        match self {
            ActionKind::Fade => "Fade out",
            ActionKind::Stop => "Stop",
            ActionKind::Pause => "Pause",
            ActionKind::PlayOn => "Play on (a vamp)",
            ActionKind::Play => "Play (a group)",
        }
    }

    /// The badge on the tile.
    pub fn badge(self) -> &'static str {
        match self {
            ActionKind::Fade => "FADE",
            ActionKind::Stop => "STOP",
            ActionKind::Pause => "PAUSE",
            ActionKind::PlayOn => "PLAY ON",
            ActionKind::Play => "GROUP",
        }
    }
}

/// One tile, or every tile in a set, that an action acts on.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Target {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tile: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub set: Option<Uuid>,
    /// Seconds after the trigger that a group plays this target.
    #[serde(skip_serializing_if = "is_zero")]
    pub offset: f64,
}

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

/// An action tile plays no audio: triggering it (a click, a key, GO) acts on other tiles.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TileAction {
    pub kind: ActionKind,
    pub targets: Vec<Target>,
    /// Seconds, for a fade.
    pub fade: f32,
}

impl Default for TileAction {
    fn default() -> Self {
        TileAction { kind: ActionKind::Fade, targets: Vec::new(), fade: 3.0 }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Tile {
    pub id: Uuid,
    pub title: String,
    /// File name inside the library's `Media/` folder.
    pub file: Option<String>,
    pub color: TileColor,
    /// Its key: the slot's automatic one (`logic::rekey`), or one set by hand.
    #[serde(deserialize_with = "crate::logic::lenient_key")]
    pub hotkey: Option<PadKey>,
    /// The key (or no key) was set by hand: it stays with the tile wherever it moves.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub hotkey_manual: bool,
    /// The options Hold, Pausable and One-shot, stored flat beside the old `mode`.
    #[serde(flatten)]
    pub press: PressOptions,
    /// Linear gain, 0.0–1.0.
    pub volume: f32,
    /// Extra gain in dB (±GAIN_DB_MAX) on top of `volume`; set by hand or by
    /// `tile.normalize`, so quiet clips can be raised.
    pub gain_db: f32,
    pub duck: Duck,
    /// Playback speed, 0.5–2; the pitch follows (like tape).
    pub speed: f32,
    /// Transposition in semitones (±12) that keeps the speed; rendered once, offline.
    pub pitch: f32,
    pub looping: bool,
    /// Starting this tile fades out every other playing tile.
    pub solo: bool,
    /// Seconds.
    pub fade_in: f32,
    /// Seconds: the fade at the end of the clip (not used when it's stopped).
    pub fade_out: f32,
    /// Seconds to fade out when the tile is stopped (a click, a key, an action, Solo);
    /// 0 is no fade of its own: the stop still gets the 0.1 s de-click ramp.
    pub stop_fade: f32,
    pub fade_curve: FadeCurve,
    /// Trim start, seconds into the file.
    pub start: f64,
    /// Trim end, seconds into the file; `None` means the end of the file.
    pub end: Option<f64>,
    /// A section (seconds into the file, inside the trim) that loops until the tile is
    /// released (`tile.release`), then plays on to the trim end: a vamp.
    pub vamp: Option<(f64, f64)>,
    /// Named points to jump to, in time order.
    pub markers: Vec<Marker>,
    /// Cached file duration in seconds, filled in once decoded.
    pub duration: Option<f64>,
    /// Picture file inside the library's `Media/` folder.
    pub image: Option<String>,
    pub image_layer: ImageLayer,
    pub image_fit: ImageFit,
    /// 0.0–1.0.
    pub image_opacity: f32,
    /// The picture is cover art taken from the audio file (not chosen by the user).
    pub image_from_art: bool,
    /// The audio file has been checked for embedded cover art. Stays true after the
    /// user removes the art, so it isn't applied again.
    pub art_checked: bool,
    /// Where it plays, each at its own level; the first is its Output. Empty is Main at
    /// 0 dB. Older libraries' single `output` reads as one send.
    #[serde(alias = "output", deserialize_with = "sends_or_output")]
    pub sends: Vec<TileSend>,
    /// For a file with more than two channels: each source channel's route, in file
    /// order. Empty is Auto, the BS.775 stereo downmix.
    pub channel_mix: Vec<ChannelRoute>,
    /// OSC or MIDI messages sent when the tile starts and when it stops (src/messages.rs).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub on_start: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub on_stop: Vec<String>,
    /// The operator's notes for this cue ("standby: door slam"), shown in the cue list
    /// and the inspector.
    pub notes: String,
    /// Triggers, bindings and GO skip it (it still previews), and it draws dimmed.
    pub disabled: bool,
    /// An action tile: what it does to other tiles instead of playing audio.
    pub action: Option<TileAction>,
    /// Seconds between triggering the tile and its start.
    #[serde(skip_serializing_if = "is_zero")]
    pub pre_wait: f64,
    /// What starts the next cue in the set without a GO.
    #[serde(skip_serializing_if = "is_default")]
    pub link: CueLink,
    /// Seconds after this tile starts (Continue) or ends (Follow) before the next cue
    /// starts.
    #[serde(skip_serializing_if = "is_zero")]
    pub post_wait: f64,
}

fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}

impl Default for Tile {
    fn default() -> Self {
        Tile {
            id: Uuid::new_v4(),
            title: "Untitled".into(),
            file: None,
            color: TileColor::default(),
            hotkey: None,
            hotkey_manual: false,
            press: PressOptions::default(),
            volume: 1.0,
            gain_db: 0.0,
            duck: Duck::None,
            speed: 1.0,
            pitch: 0.0,
            looping: false,
            solo: false,
            fade_in: 0.0,
            fade_out: 0.0,
            stop_fade: 0.0,
            fade_curve: FadeCurve::Gentle,
            start: 0.0,
            end: None,
            vamp: None,
            markers: Vec::new(),
            duration: None,
            image: None,
            image_layer: ImageLayer::default(),
            image_fit: ImageFit::default(),
            image_opacity: DEFAULT_IMAGE_OPACITY,
            image_from_art: false,
            art_checked: false,
            sends: Vec::new(),
            channel_mix: Vec::new(),
            on_start: Vec::new(),
            on_stop: Vec::new(),
            notes: String::new(),
            disabled: false,
            action: None,
            pre_wait: 0.0,
            link: CueLink::None,
            post_wait: 0.0,
        }
    }
}

impl Tile {
    /// Every place it plays: its sends, or Main at 0 dB when it has none.
    pub fn routes(&self) -> Vec<TileSend> {
        if self.sends.is_empty() {
            vec![TileSend::default()]
        } else {
            self.sends.clone()
        }
    }

    /// The output it plays on first, by name (the inspector's Output).
    pub fn output(&self) -> &str {
        self.sends.first().map_or(MAIN_OUTPUT, |s| s.output.as_str())
    }

    /// A copy with a fresh id, and its slot's automatic key (a key stays unique within a
    /// set).
    pub fn duplicate(&self) -> Tile {
        Tile { id: Uuid::new_v4(), hotkey: None, hotkey_manual: false, ..self.clone() }
    }

    /// The linear gain the mixer applies: volume times the dB gain.
    pub fn gain(&self) -> f32 {
        self.volume * crate::logic::db_to_gain(self.gain_db)
    }
}

/// The longest cross-fade between a cue list's cues, in seconds.
pub const CROSSFADE_MAX: f32 = 5.0;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SoundSet {
    pub id: Uuid,
    pub name: String,
    pub layout: Layout,
    /// Grid columns, 1–12: in Rows mode the row size (how wide tiles get, and how many
    /// new sounds fill a row), in Fixed columns mode the column count.
    pub columns: u32,
    /// How many tiles each grid row holds, top to bottom (D-113; `rows.rs`). Tiles beyond
    /// them fill rows of `columns`, so empty is the plain N-column wrap.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub rows: Vec<u32>,
    /// The blank slots, as [row, slot] (0-based, slots counting blanks), in reading
    /// order (D-140). Older builds ignore them and close the rows up.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub blanks: Vec<[u32; 2]>,
    /// List view: Continuous (on) starts the next armed cue when one ends on its own;
    /// Queued (off) selects it and waits.
    pub auto_advance: bool,
    /// List view, Continuous: seconds the next cue overlaps the end of the one before,
    /// one fading out as the other fades in (0–`CROSSFADE_MAX`, at most half of either).
    pub crossfade: f32,
    pub tiles: Vec<Tile>,
    /// Notes for the whole set (an act, a scene), shown above the cue list.
    pub notes: String,
    /// Locked: nothing in the set can be changed (its tiles, their order, its own
    /// settings) until it's unlocked; its tiles still play (`set.lock`, M22).
    pub locked: bool,
}

impl Default for SoundSet {
    fn default() -> Self {
        SoundSet {
            id: Uuid::new_v4(),
            name: "Untitled Set".into(),
            layout: Layout::default(),
            columns: 5,
            rows: Vec::new(),
            blanks: Vec::new(),
            auto_advance: false,
            crossfade: 0.0,
            tiles: Vec::new(),
            notes: String::new(),
            locked: false,
        }
    }
}

impl SoundSet {
    pub fn named(name: impl Into<String>) -> Self {
        SoundSet { name: name.into(), ..Default::default() }
    }

    /// The empty set a new library starts with.
    pub fn first() -> Self {
        SoundSet::named("Set 1")
    }

    /// Still exactly as [`SoundSet::first`] made it: nothing added, renamed or changed.
    pub fn is_untouched_first(&self) -> bool {
        *self == SoundSet { id: self.id, ..SoundSet::first() }
    }

    /// A deep copy with fresh ids for the set and every tile; hotkeys are kept because
    /// they are unique per set. The copy is unlocked, to be changed.
    pub fn duplicate(&self) -> SoundSet {
        SoundSet {
            id: Uuid::new_v4(),
            name: format!("{} copy", self.name),
            locked: false,
            tiles: self.tiles.iter().map(|t| Tile { id: Uuid::new_v4(), ..t.clone() }).collect(),
            ..self.clone()
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Library {
    /// Files from before versioning read as 1.
    #[serde(default = "version_one")]
    pub version: u32,
    pub sets: Vec<SoundSet>,
    pub selected_set: Option<Uuid>,
    pub master_volume: f32,
    /// Output device name; `None` follows the system default.
    pub output_device: Option<String>,
    /// Seconds used by Stop All (Esc's first press); 0 is immediate.
    pub stop_all_fade: f32,
    /// Esc must be held for [`STOP_ALL_HOLD`] before Stop All runs (an accident guard).
    pub stop_all_hold: bool,
    /// Before M22: Edit mode couldn't be entered until unlocked. Read only, to lock every
    /// set instead (`normalize`).
    #[serde(skip_serializing)]
    pub edit_locked: bool,
    /// The sets sidebar is shown (`view.sidebar`).
    pub show_sidebar: bool,
    /// The inspector is shown (`view.inspector`): the selected tiles, or the current set.
    pub show_inspector: bool,
    /// How ducked tiles dip, in dB below their level (−40 to 0).
    pub duck_db: f32,
    /// Seconds to dip, and to come back up.
    pub duck_attack: f32,
    pub duck_release: f32,
    /// Draw each tile's waveform behind its label.
    pub show_waveforms: bool,
    /// How grid sets lay out (Settings > Grid layout).
    pub grid: GridMode,
    /// Draw a level meter on each playing tile.
    pub show_meters: bool,
    /// Console pads in their tile's colour all over (D-146) rather than the default dark
    /// body with a coloured title, waveform and border (`settings.coloredPads`; off by
    /// default, D-152). Panel draws its own blocks either way.
    pub colored_pads: bool,
    pub appearance: Appearance,
    /// The design in use; for a user theme, its base (so older builds open in that).
    pub theme: ThemeKind,
    /// The user theme in use: its folder in the data directory's `themes` (D-105).
    /// `None` is the built-in `theme`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme_name: Option<String>,
    /// Linux: open the window through X11 (XWayland on a Wayland desktop) rather than
    /// native Wayland. Read at launch; `LEADSTINGER_X11` overrides it.
    pub use_x11: bool,
    /// Microphone or other input by name; `None` follows the system default.
    pub input_device: Option<String>,
    /// Duck the beds while the input is above `voice_threshold_db`.
    pub voice_duck: bool,
    pub voice_threshold_db: f32,
    /// Seconds of input kept for instant replay (`replay.save`); 0 is off.
    pub replay_seconds: u32,
    /// MIDI pads, global keys and OSC addresses mapped to commands (D-041).
    pub bindings: Vec<Binding>,
    pub midi: MidiSettings,
    pub osc: crate::osc::OscSettings,
    /// Outputs beyond Main that tiles can be routed to.
    pub outputs: Vec<OutputDef>,
    /// Previews (the inspector's Preview, `tile.preview`) play on their own output.
    pub preview_output: bool,
    /// Its device; `None` is the system default.
    pub preview_device: Option<String>,
    /// In-app tile keys play a tile in any set: the current set's first, then the
    /// others in sidebar order.
    pub tile_keys_all_sets: bool,
    /// Ask GitHub for a newer release at launch (Settings > Updates; off by default, so
    /// the app makes no network call unless asked, D-206).
    pub check_updates: bool,
    /// UI zoom in percent, [`ZOOM_MIN`] to [`ZOOM_MAX`] (`view.zoom`).
    pub zoom: u32,
    /// The system voice says a tile's title when it starts (`settings.speakCues`;
    /// off by default, D-092).
    pub speak_cues: bool,
    /// The status bar shows how many clips are new on the connected QuipClipper servers,
    /// asking them now and then while the QuipClipper window is closed
    /// (`settings.quipBadge`; on by default, D-103).
    pub quip_badge: bool,
    /// A QuipClipper link the system opens asks before importing, even from a connected
    /// server (`settings.quipAskEachImport`; off by default, D-135).
    pub quip_ask_each: bool,
}

/// UI zoom limits and the steps ⌘/Ctrl + and − move through, in percent.
pub const ZOOM_MIN: u32 = 50;
pub const ZOOM_MAX: u32 = 300;
pub const ZOOM_STEPS: [u32; 13] = [50, 67, 75, 80, 90, 100, 110, 125, 150, 175, 200, 250, 300];

/// The next zoom step above (`up`) or below `from`.
pub fn zoom_step(from: u32, up: bool) -> u32 {
    if up {
        ZOOM_STEPS.iter().copied().find(|&z| z > from).unwrap_or(ZOOM_MAX)
    } else {
        ZOOM_STEPS.iter().rev().copied().find(|&z| z < from).unwrap_or(ZOOM_MIN)
    }
}

impl Default for Library {
    fn default() -> Self {
        Library {
            version: LIBRARY_VERSION,
            sets: Vec::new(),
            selected_set: None,
            master_volume: 1.0,
            output_device: None,
            stop_all_fade: 0.0,
            stop_all_hold: false,
            edit_locked: false,
            show_sidebar: true,
            show_inspector: true,
            duck_db: DUCK_DB_DEFAULT,
            duck_attack: 0.05,
            duck_release: 0.4,
            show_waveforms: true,
            grid: GridMode::default(),
            show_meters: true,
            colored_pads: false,
            appearance: Appearance::default(),
            theme: ThemeKind::default(),
            theme_name: None,
            use_x11: false,
            input_device: None,
            voice_duck: false,
            voice_threshold_db: VOICE_THRESHOLD_DEFAULT,
            replay_seconds: 0,
            bindings: Vec::new(),
            midi: MidiSettings::default(),
            osc: crate::osc::OscSettings::default(),
            outputs: Vec::new(),
            preview_output: false,
            preview_device: None,
            tile_keys_all_sets: false,
            check_updates: false,
            zoom: 100,
            speak_cues: false,
            quip_badge: true,
            quip_ask_each: false,
        }
    }
}

fn version_one() -> u32 {
    1
}

impl Library {
    /// Bring a library written by an older version up to [`LIBRARY_VERSION`]. Returns
    /// what changed, for the notice, or `None` when nothing needed to.
    pub fn migrate(&mut self) -> Option<String> {
        if self.version >= LIBRARY_VERSION {
            return None;
        }
        let mut notes = Vec::new();
        if self.version < 2 {
            // A tile's fade-out used to apply when it was stopped too: keep that.
            let mut kept = 0;
            for t in self.sets.iter_mut().flat_map(|s| s.tiles.iter_mut()) {
                if t.fade_out > 0.0 && t.stop_fade == 0.0 {
                    t.stop_fade = t.fade_out;
                    kept += 1;
                }
            }
            if kept > 0 {
                notes.push(format!("{kept} tile{} with a fade-out now fade{} the same way when stopped", if kept == 1 { "" } else { "s" }, if kept == 1 { "s" } else { "" }));
            }
            // Every library stored 1.0 s because it was the default (D-067).
            if self.stop_all_fade > 0.0 {
                self.stop_all_fade = 0.0;
                notes.push("Stop All is now immediate (Settings > Stop All fade sets a fade for Esc's first press)".into());
            }
        }
        if self.version < 3 {
            // Today's N-column wrap becomes rows of N: the grid looks the same.
            for set in &mut self.sets {
                set.rows = set.row_lengths().into_iter().map(|r| r as u32).collect();
            }
            if self.sets.iter().any(|s| s.layout == Layout::Grid && !s.tiles.is_empty()) {
                notes.push("grid rows can now hold more tiles: drag a sound into a row and its tiles narrow to fit (Settings > Grid layout has auto columns)".into());
            }
        }
        if self.version < 4 && self.sets.iter().flat_map(|s| &s.tiles).any(|t| t.press != PressOptions::default()) {
            // Read from `mode` already (PressOptions's From<PressFile>).
            notes.push("the Mode menu is now the pad options: Play / Pause is Pausable, Always Restart is One-shot, Hold to Play is Hold".into());
        }
        if self.version < 4 && self.sets.iter().any(|s| s.layout == Layout::Grid && !s.tiles.is_empty()) {
            // Nothing to convert: v3 rows are rows of slots with no blanks.
            notes.push("the grid has slots now: drag a sound between two others, into a blank slot or below the last row, and the board shows where it goes; files dropped on the board go in the same way (Replace Audio… in the inspector replaces a sound)".into());
        }
        if self.version < 5 && self.sets.iter().any(|s| !s.tiles.is_empty()) {
            // Keys in the old straight-through order were automatic; any other was set by
            // hand and stays (D-193). The rest follow the rows from now on (`rekey`).
            let kept: usize = self.sets.iter_mut().map(|s| crate::logic::guess_hand_set(&mut s.tiles)).sum();
            let hand = match kept {
                0 => String::new(),
                1 => " (the 1 key set by hand stays)".into(),
                n => format!(" (the {n} keys set by hand stay)"),
            };
            notes.push(format!("pad keys now follow the board's rows: row 1 takes 1 to =, row 2 Q to ], row 3 A to ', row 4 Z to /, and rows 5 to 8 the same with {}{hand}", if cfg!(target_os = "macos") { "Option" } else { "Alt" }));
        }
        self.version = LIBRARY_VERSION;
        Some(notes.join("; "))
    }

    pub fn starter() -> Library {
        let set = SoundSet::first();
        Library { selected_set: Some(set.id), sets: vec![set], ..Default::default() }
    }

    /// Repair invariants after loading: at least one set, a valid selection, sane ranges.
    pub fn normalize(&mut self) {
        if self.sets.is_empty() {
            self.sets.push(SoundSet::first());
        }
        if !self.sets.iter().any(|s| Some(s.id) == self.selected_set) {
            self.selected_set = Some(self.sets[0].id);
        }
        self.master_volume = self.master_volume.clamp(0.0, 1.0);
        self.stop_all_fade = self.stop_all_fade.clamp(0.0, 10.0);
        let finite = |v: f32, d: f32| if v.is_finite() { v } else { d };
        self.duck_db = finite(self.duck_db, DUCK_DB_DEFAULT).clamp(-40.0, 0.0);
        self.duck_attack = finite(self.duck_attack, 0.05).clamp(0.0, 5.0);
        self.duck_release = finite(self.duck_release, 0.4).clamp(0.0, 5.0);
        self.replay_seconds = self.replay_seconds.min(REPLAY_MAX);
        self.voice_threshold_db = finite(self.voice_threshold_db, VOICE_THRESHOLD_DEFAULT).clamp(-80.0, 0.0);
        // The old Lock Edit Mode guarded every set: each set keeps that guard as its lock.
        if std::mem::take(&mut self.edit_locked) {
            for set in &mut self.sets {
                set.locked = true;
            }
        }
        for set in &mut self.sets {
            set.columns = set.columns.clamp(1, 12);
            for t in &mut set.tiles {
                t.volume = t.volume.clamp(0.0, 1.0);
                t.pitch = if t.pitch.is_finite() { t.pitch.clamp(-crate::pitch::PITCH_MAX, crate::pitch::PITCH_MAX) } else { 0.0 };
                t.speed = if t.speed.is_finite() { t.speed.clamp(SPEED_MIN, SPEED_MAX) } else { 1.0 };
                t.gain_db = if t.gain_db.is_finite() { t.gain_db.clamp(-GAIN_DB_MAX, GAIN_DB_MAX) } else { 0.0 };
                t.fade_in = t.fade_in.max(0.0);
                t.fade_out = t.fade_out.max(0.0);
                t.stop_fade = if t.stop_fade.is_finite() { t.stop_fade.clamp(0.0, 10.0) } else { 0.0 };
                t.pre_wait = if t.pre_wait.is_finite() { t.pre_wait.clamp(0.0, WAIT_MAX) } else { 0.0 };
                t.post_wait = if t.post_wait.is_finite() { t.post_wait.clamp(0.0, WAIT_MAX) } else { 0.0 };
                t.start = t.start.max(0.0);
                t.image_opacity = t.image_opacity.clamp(0.0, 1.0);
            }
        }
        self.rekey();
    }

    /// Give every set's tiles their keys (`logic::rekey`): after any change, since a
    /// tile's automatic key follows its slot.
    pub fn rekey(&mut self) {
        for set in &mut self.sets {
            crate::logic::rekey(set, self.grid);
        }
    }

    pub fn set_index(&self, id: Uuid) -> Option<usize> {
        self.sets.iter().position(|s| s.id == id)
    }

    pub fn selected_index(&self) -> usize {
        self.selected_set.and_then(|id| self.set_index(id)).unwrap_or(0)
    }

    pub fn locate(&self, tile: Uuid) -> Option<(usize, usize)> {
        self.sets.iter().enumerate().find_map(|(si, s)| {
            s.tiles.iter().position(|t| t.id == tile).map(|ti| (si, ti))
        })
    }

    pub fn tile(&self, id: Uuid) -> Option<&Tile> {
        self.locate(id).map(|(s, t)| &self.sets[s].tiles[t])
    }

    pub fn tile_mut(&mut self, id: Uuid) -> Option<&mut Tile> {
        self.locate(id).map(move |(s, t)| &mut self.sets[s].tiles[t])
    }

    /// Every media file (audio and pictures) the library's tiles use.
    pub fn media_files(&self) -> impl Iterator<Item = String> + '_ {
        self.sets.iter().flat_map(|s| &s.tiles).flat_map(|t| t.file.iter().chain(&t.image).cloned())
    }

    /// True if any tile uses `file` as its audio or its picture.
    pub fn file_in_use(&self, file: &str) -> bool {
        self.sets
            .iter()
            .flat_map(|s| &s.tiles)
            .any(|t| t.file.as_deref() == Some(file) || t.image.as_deref() == Some(file))
    }

    /// Tiles playing `audio_file` that haven't been checked for embedded art yet.
    pub fn needs_art(&self, audio_file: &str) -> bool {
        self.sets.iter().flat_map(|s| &s.tiles).any(|t| t.file.as_deref() == Some(audio_file) && !t.art_checked)
    }

    /// Record the result of checking `audio_file` for cover art: tiles without a
    /// picture get `art_file` as a transparent underlay. Every tile playing the file is
    /// marked checked, so removed art never comes back. Returns true if anything changed.
    pub fn apply_embedded_art(&mut self, audio_file: &str, art_file: Option<&str>) -> bool {
        let mut changed = false;
        for t in self.sets.iter_mut().flat_map(|s| s.tiles.iter_mut()) {
            if t.file.as_deref() != Some(audio_file) || t.art_checked {
                continue;
            }
            t.art_checked = true;
            changed = true;
            if let (Some(art), None) = (art_file, &t.image) {
                t.image = Some(art.to_owned());
                t.image_from_art = true;
                t.image_layer = ImageLayer::Underlay;
                t.image_fit = ImageFit::Fill;
                t.image_opacity = DEFAULT_IMAGE_OPACITY;
            }
        }
        changed
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn panel_theme_stays_readable_by_older_builds() {
        assert_eq!(serde_json::to_value(ThemeKind::Panel).unwrap(), serde_json::json!("padd"));
        assert_eq!(serde_json::from_value::<ThemeKind>(serde_json::json!("panel")).unwrap(), ThemeKind::Panel);
        assert_eq!(serde_json::from_value::<ThemeKind>(serde_json::json!("padd")).unwrap(), ThemeKind::Panel);
    }

    use super::*;

    #[test]
    fn round_trips_through_json() {
        let mut lib = Library::starter();
        lib.sets[0].tiles.push(Tile {
            title: "Airhorn".into(),
            file: Some("a.wav".into()),
            hotkey: Some(PadKey { key: ';', alt: true }),
            hotkey_manual: true,
            press: PressOptions { hold: true, pausable: true, one_shot: false },
            end: Some(2.5),
            ..Default::default()
        });
        let json = serde_json::to_string(&lib).unwrap();
        let back: Library = serde_json::from_str(&json).unwrap();
        assert_eq!(lib, back);
    }

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let lib: Library =
            serde_json::from_str(r#"{"sets":[{"name":"Show","tiles":[{"title":"Hit"}]}],"future_field":1}"#)
                .unwrap();
        let tile = &lib.sets[0].tiles[0];
        assert_eq!(tile.title, "Hit");
        assert_eq!(tile.volume, 1.0);
        assert_eq!(tile.gain_db, 0.0);
        assert_eq!(tile.press, PressOptions::default());
        assert_eq!(lib.sets[0].columns, 5);
        assert_eq!(lib.stop_all_fade, 0.0);
        assert_eq!(lib.version, 1, "no version field: from before versioning");
    }

    /// Lock Edit Mode (before M22) becomes a lock on every set; it's no longer written,
    /// and a set's lock is.
    #[test]
    fn the_old_edit_lock_locks_every_set() {
        let mut lib: Library = serde_json::from_value(serde_json::json!({
            "version": 4, "edit_locked": true, "sets": [{"name": "A"}, {"name": "B"}]
        }))
        .unwrap();
        lib.normalize();
        assert!(lib.sets.iter().all(|s| s.locked) && !lib.edit_locked);
        let saved = serde_json::to_value(&lib).unwrap();
        assert!(saved.get("edit_locked").is_none());
        assert_eq!((saved["sets"][0]["locked"].clone(), saved["show_inspector"].clone()), (serde_json::json!(true), serde_json::json!(true)));
        let mut plain: Library = serde_json::from_value(serde_json::json!({"sets": [{"name": "A"}]})).unwrap();
        plain.normalize();
        assert!(!plain.sets[0].locked && plain.show_sidebar);
        assert!(!lib.sets[0].duplicate().locked, "a copy is unlocked");
    }

    /// Release was Play On's name (D-156): libraries, set files and starter packs that say
    /// release still read as Play On (set files and packs read the kind as this does), and
    /// it's written as play_on.
    #[test]
    fn a_release_action_is_play_on() {
        for old in ["release", "play_on", "playon", "play-on"] {
            assert_eq!(serde_json::from_value::<ActionKind>(serde_json::json!(old)).unwrap(), ActionKind::PlayOn, "{old}");
        }
        let a: TileAction = serde_json::from_str(r#"{"kind": "release", "fade": 0}"#).unwrap();
        assert_eq!(a.kind, ActionKind::PlayOn);
        assert_eq!(serde_json::to_value(a.kind).unwrap(), "play_on");
        assert_eq!((ActionKind::PlayOn.label(), ActionKind::PlayOn.badge()), ("Play on (a vamp)", "PLAY ON"));
    }

    #[test]
    fn old_modes_become_pad_options_and_are_still_written() {
        // Libraries from before D-145 have only `mode`.
        let old = r#"{"version": 4, "sets": [{"name": "Show", "tiles": [
            {"title": "A"}, {"title": "B", "mode": "toggle"}, {"title": "C", "mode": "pause"},
            {"title": "D", "mode": "restart"}, {"title": "E", "mode": "hold"}]}]}"#;
        let lib: Library = serde_json::from_str(old).unwrap();
        let got: Vec<PressOptions> = lib.sets[0].tiles.iter().map(|t| t.press).collect();
        let off = PressOptions::default();
        assert_eq!(
            got,
            [off, off, PressOptions { pausable: true, ..off }, PressOptions { one_shot: true, ..off }, PressOptions { hold: true, ..off }],
            "Play/Stop → none, Play/Pause → Pausable, Always Restart → One-shot, Hold to Play → Hold"
        );
        // Written: the options, and the nearest old mode for older builds.
        let both = Tile { press: PressOptions { hold: true, pausable: true, one_shot: false }, ..Default::default() };
        let v = serde_json::to_value(&both).unwrap();
        assert_eq!((v["hold"].as_bool(), v["pausable"].as_bool(), v["one_shot"].as_bool(), v["mode"].as_str()), (Some(true), Some(true), Some(false), Some("hold")));
        assert_eq!(serde_json::from_value::<Tile>(v).unwrap().press, both.press, "the options win over the mode written beside them");
        // The options alone (no mode), and a hand-edited file with both exclusive ones.
        let t: Tile = serde_json::from_str(r#"{"pausable": true}"#).unwrap();
        assert_eq!(t.press, PressOptions { pausable: true, ..off });
        let t: Tile = serde_json::from_str(r#"{"pausable": true, "one_shot": true, "mode": "pause"}"#).unwrap();
        assert_eq!(t.press, PressOptions { one_shot: true, ..off }, "never both: One-shot, as Always Restart was");
    }

    #[test]
    fn pausable_and_one_shot_exclude_each_other() {
        let mut p = PressOptions { hold: true, ..Default::default() };
        p.set_pausable(true);
        p.set_one_shot(true);
        assert_eq!(p, PressOptions { hold: true, pausable: false, one_shot: true });
        p.set_pausable(true);
        assert_eq!(p, PressOptions { hold: true, pausable: true, one_shot: false });
        p.set_one_shot(false);
        assert_eq!(p, PressOptions { hold: true, pausable: true, one_shot: false }, "unticking one leaves the other");
        for mode in [PlayMode::Toggle, PlayMode::Restart, PlayMode::Hold, PlayMode::Pause] {
            assert_eq!(PressOptions::from_mode(mode).mode(), mode);
        }
    }

    #[test]
    fn normalize_repairs_selection_and_ranges() {
        let mut lib = Library { sets: vec![], master_volume: 4.0, ..Default::default() };
        lib.normalize();
        assert_eq!(lib.sets.len(), 1);
        assert_eq!(lib.selected_set, Some(lib.sets[0].id));
        assert_eq!(lib.master_volume, 1.0);

        let mut set = SoundSet::named("A");
        set.tiles.push(Tile { gain_db: 60.0, ..Default::default() });
        set.tiles.push(Tile { gain_db: f32::NAN, ..Default::default() });
        let mut lib = Library { sets: vec![set], ..Default::default() };
        lib.normalize();
        assert_eq!(lib.sets[0].tiles[0].gain_db, GAIN_DB_MAX);
        assert_eq!(lib.sets[0].tiles[1].gain_db, 0.0);
    }

    #[test]
    fn gain_combines_volume_and_db() {
        let t = Tile { volume: 0.5, gain_db: 6.0206, ..Default::default() };
        assert!((t.gain() - 1.0).abs() < 1e-3);
    }

    #[test]
    fn duplicate_set_gets_fresh_ids() {
        let mut set = SoundSet::named("A");
        set.tiles.push(Tile::default());
        let copy = set.duplicate();
        assert_ne!(copy.id, set.id);
        assert_ne!(copy.tiles[0].id, set.tiles[0].id);
        assert_eq!(copy.name, "A copy");
    }

    #[test]
    fn embedded_art_applies_once_and_respects_user_pictures() {
        let mut lib = Library::starter();
        let plain = Tile { file: Some("a.mp3".into()), ..Default::default() };
        let chosen = Tile { file: Some("a.mp3".into()), image: Some("mine.png".into()), ..Default::default() };
        let other = Tile { file: Some("b.mp3".into()), ..Default::default() };
        let (p, c, o) = (plain.id, chosen.id, other.id);
        lib.sets[0].tiles.extend([plain, chosen, other]);

        assert!(lib.needs_art("a.mp3"));
        assert!(lib.apply_embedded_art("a.mp3", Some("art.jpg")));
        let t = lib.tile(p).unwrap();
        assert_eq!(t.image.as_deref(), Some("art.jpg"));
        assert!(t.image_from_art && t.art_checked);
        assert_eq!(t.image_layer, ImageLayer::Underlay);
        assert_eq!(lib.tile(c).unwrap().image.as_deref(), Some("mine.png"), "a chosen picture wins");
        assert!(!lib.tile(o).unwrap().art_checked, "other files untouched");
        assert!(lib.file_in_use("art.jpg"));

        // The user removes the art; a later check must not bring it back.
        lib.tile_mut(p).unwrap().image = None;
        assert!(!lib.needs_art("a.mp3"));
        assert!(!lib.apply_embedded_art("a.mp3", Some("art.jpg")));
        assert_eq!(lib.tile(p).unwrap().image, None);

        // A file without art still gets marked checked.
        assert!(lib.apply_embedded_art("b.mp3", None));
        assert!(lib.tile(o).unwrap().art_checked);
        assert_eq!(lib.tile(o).unwrap().image, None);
    }

    #[test]
    fn older_libraries_load_with_picture_defaults() {
        let lib: Library = serde_json::from_str(r#"{"sets":[{"tiles":[{"title":"Hit"}]}]}"#).unwrap();
        let t = &lib.sets[0].tiles[0];
        assert_eq!(t.image, None);
        assert!(!t.art_checked, "existing tiles get checked for art on next load");
        assert_eq!(t.image_opacity, DEFAULT_IMAGE_OPACITY);
        assert!(lib.show_waveforms);
        assert!(lib.show_meters, "meters are on in older libraries");
        assert!(!lib.colored_pads, "older libraries get the dark pads (D-152)");
    }

    #[test]
    fn locate_finds_tiles_across_sets() {
        let mut lib = Library::starter();
        let mut second = SoundSet::named("B");
        let tile = Tile::default();
        let id = tile.id;
        second.tiles.push(tile);
        lib.sets.push(second);
        assert_eq!(lib.locate(id), Some((1, 0)));
    }

    #[test]
    fn a_single_output_migrates_to_one_send() {
        let read = |v: serde_json::Value| serde_json::from_value::<Tile>(v).unwrap().sends;
        assert_eq!(read(serde_json::json!({"output": "Phones"})), vec![TileSend::to("Phones")]);
        assert_eq!(read(serde_json::json!({"output": null})), vec![], "null was Main");
        assert_eq!(read(serde_json::json!({"output": "main"})), vec![], "so is Main by name");
        assert_eq!(read(serde_json::json!({})), vec![]);
        let both = serde_json::json!({"sends": [{"output": "Main", "gain_db": -3.0}, {"output": "Stream", "gainDb": -6.0}, {"output": "stream"}]});
        assert_eq!(
            read(both),
            vec![TileSend { output: "Main".into(), gain_db: -3.0 }, TileSend { output: "Stream".into(), gain_db: -6.0 }],
            "one send per output, the first kept"
        );
        // Written back as sends, which read the same.
        let t = Tile { sends: vec![TileSend::to("Phones"), TileSend { output: "Main".into(), gain_db: -6.0 }], ..Default::default() };
        let json = serde_json::to_value(&t).unwrap();
        assert!(json.get("output").is_none());
        assert_eq!(serde_json::from_value::<Tile>(json).unwrap(), t);
        assert_eq!(t.output(), "Phones");
        assert_eq!(Tile::default().routes(), vec![TileSend::default()]);
    }

    #[test]
    fn channel_mixes_start_from_the_downmix() {
        let k = std::f32::consts::FRAC_1_SQRT_2;
        assert_eq!(ChannelRoute::from_gains((1.0, 0.0)), ChannelRoute { to: ChannelTo::Left, gain_db: 0.0 });
        assert_eq!(ChannelRoute::from_gains((k, k)), ChannelRoute { to: ChannelTo::Both, gain_db: -3.0 });
        assert_eq!(ChannelRoute::from_gains((0.0, k)), ChannelRoute { to: ChannelTo::Right, gain_db: -3.0 });
        assert_eq!(ChannelRoute::from_gains((0.0, 0.0)).to, ChannelTo::Mute);
        let mix = vec![ChannelRoute { to: ChannelTo::Both, gain_db: -6.0 }, ChannelRoute::default()];
        let g = channel_gains(&mix, 2).unwrap();
        assert!((g[0].0 - 0.501).abs() < 1e-3 && g[0].0 == g[0].1 && g[1] == (1.0, 1.0));
        assert_eq!(channel_gains(&mix, 6), None, "another channel count: Auto");
        assert_eq!(channel_gains(&[], 6), None);
        // Stored with the tile; older libraries have none (Auto).
        let t = Tile { channel_mix: mix.clone(), ..Default::default() };
        let back: Tile = serde_json::from_value(serde_json::to_value(&t).unwrap()).unwrap();
        assert_eq!(back.channel_mix, mix);
        let read: Vec<ChannelRoute> = serde_json::from_value(serde_json::json!([{"to": "l", "gainDb": -3}, {"to": "mute"}])).unwrap();
        assert_eq!(read[0], ChannelRoute { to: ChannelTo::Left, gain_db: -3.0 });
    }

    /// Version 4 → 5 (D-188, D-193): keys in the old straight-through order were automatic
    /// and now follow the rows; any other key was set by hand and stays with its tile.
    #[test]
    fn version_4_libraries_re_key_by_row_keeping_hand_set_keys() {
        let keys = ["1", "2", "3", "4", "5", "z", "7", "8"];
        let tiles: Vec<_> = keys.iter().enumerate().map(|(i, k)| serde_json::json!({"title": format!("T{i}"), "hotkey": k})).collect();
        let json = serde_json::json!({"version": 4, "sets": [{"columns": 4, "rows": [4, 4], "tiles": tiles}]});
        let mut lib: Library = serde_json::from_value(json).unwrap();
        let note = lib.migrate().expect("migrated");
        assert!(note.contains("pad keys now follow the board's rows") && note.contains("the 1 key set by hand stays"), "{note}");
        lib.normalize();
        let got: Vec<_> = lib.sets[0].tiles.iter().map(|t| t.hotkey.map(PadKey::name).unwrap_or_default()).collect();
        assert_eq!(got, ["1", "2", "3", "4", "q", "z", "e", "r"], "row 2 is Q W E R; Z stays, and W is free");
        assert!(lib.sets[0].tiles[5].hotkey_manual);
        // Saved and read again: the same keys, and nothing to migrate.
        let back: Library = serde_json::from_value(serde_json::to_value(&lib).unwrap()).unwrap();
        assert_eq!(back.sets[0].tiles, lib.sets[0].tiles);
        // A key no build knows reads as none instead of failing the library.
        let odd: Tile = serde_json::from_value(serde_json::json!({"hotkey": "§"})).unwrap();
        assert_eq!(odd.hotkey, None);
    }

    #[test]
    fn version_1_libraries_migrate_stop_fades_and_stop_all() {
        let json = serde_json::json!({
            "version": 1,
            "stop_all_fade": 1.0,
            "sets": [{"tiles": [
                {"title": "Bed", "fade_out": 2.0, "looping": true},
                {"title": "Hit"},
            ]}],
        });
        let mut lib: Library = serde_json::from_value(json).unwrap();
        let note = lib.migrate().expect("migrated");
        assert!(note.contains("1 tile") && note.contains("Stop All"), "{note}");
        assert_eq!(lib.version, LIBRARY_VERSION);
        assert_eq!(lib.stop_all_fade, 0.0, "D-067: the stored 1.0 s goes to immediate");
        let (bed, hit) = (&lib.sets[0].tiles[0], &lib.sets[0].tiles[1]);
        assert_eq!((bed.fade_out, bed.stop_fade), (2.0, 2.0), "stopping still fades as before");
        assert_eq!(hit.stop_fade, 0.0, "no fade-out: no stop fade of its own");
        assert_eq!(lib.migrate(), None, "only once");

        // A version 2 library keeps a deliberate Stop All fade and separate stop fades.
        let mut v2 = Library { stop_all_fade: 1.0, ..Library::starter() };
        v2.sets[0].tiles.push(Tile { fade_out: 2.0, ..Default::default() });
        let mut back: Library = serde_json::from_value(serde_json::to_value(&v2).unwrap()).unwrap();
        assert_eq!(back.migrate(), None);
        assert_eq!((back.stop_all_fade, back.sets[0].tiles[0].stop_fade), (1.0, 0.0));
        assert_eq!(Library::default().stop_all_fade, 0.0, "new libraries stop at once");
    }
}
