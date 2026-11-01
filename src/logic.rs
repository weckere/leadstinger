//! Pure, UI- and device-free rules: what a press does, hotkey assignment, fade
//! envelopes, trim ranges and time formatting.

use crate::model::{FadeCurve, GridMode, Layout, PressOptions, SoundSet, Tile};
use std::collections::HashSet;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayState {
    Stopped,
    Playing,
    Paused,
    /// Fading out after a stop; a new press restarts it.
    Stopping,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TriggerEvent {
    Press,
    Release,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TriggerAction {
    None,
    /// Start from the trim start (restarting if already sounding).
    Play,
    /// Stop with the tile's fade-out.
    Stop,
    Pause,
    Resume,
    /// Let the vamp go: play on to the end (D-153).
    PlayOn,
}

/// What a press or a release does to a pad that may be looping its vamp (D-154): the
/// first trigger that would stop, pause or restart a vamping tile plays on instead;
/// after that its options apply as usual.
pub fn resolve_press(press: PressOptions, state: PlayState, vamping: bool, event: TriggerEvent) -> TriggerAction {
    use TriggerAction as A;
    match resolve_trigger(press, state, event) {
        A::Stop | A::Pause | A::Play if vamping && state == PlayState::Playing => A::PlayOn,
        a => a,
    }
}

/// What a press or a release does to a pad with these options in this state (D-145).
pub fn resolve_trigger(press: PressOptions, state: PlayState, event: TriggerEvent) -> TriggerAction {
    use PlayState::*;
    use TriggerAction as A;
    match (event, state) {
        (TriggerEvent::Press, Stopped | Stopping) => A::Play,
        // Held again after a release paused it: on from that spot.
        (TriggerEvent::Press, Paused) if press.pausable || !press.one_shot && !press.hold => A::Resume,
        (TriggerEvent::Press, Paused) => A::Play,
        // Already held (a second key, or key repeat).
        (TriggerEvent::Press, Playing) if press.hold => A::None,
        (TriggerEvent::Press, Playing) if press.one_shot => A::Play,
        (TriggerEvent::Press, Playing) if press.pausable => A::Pause,
        (TriggerEvent::Press, Playing) => A::Stop,
        (TriggerEvent::Release, _) if !press.hold => A::None,
        (TriggerEvent::Release, Playing) if press.pausable => A::Pause,
        (TriggerEvent::Release, Playing) => A::Stop,
        (TriggerEvent::Release, Paused) if !press.pausable => A::Stop,
        (TriggerEvent::Release, _) => A::None,
    }
}

/// The keyboard's four rows, as pad keys take them (D-188): a board row takes the
/// keyboard row of its number, slot by slot; rows 5–8 take the same four with Option
/// (Alt) held.
pub const KEY_ROWS: [&str; 4] = ["1234567890-=", "qwertyuiop[]", "asdfghjkl;'", "zxcvbnm,./"];

/// How many board rows have automatic keys: the four keyboard rows, then the same with
/// Option.
pub const KEYED_ROWS: usize = 2 * KEY_ROWS.len();

/// A pad's key: one on the keyboard's four rows ([`KEY_ROWS`]), plain or with Option
/// (Alt) held. Stored as `"q"` or `"alt+q"`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PadKey {
    pub key: char,
    pub alt: bool,
}

impl PadKey {
    pub const fn plain(key: char) -> PadKey {
        PadKey { key, alt: false }
    }

    /// The automatic key of slot `at` (counting blanks) in board row `row`, both 0-based:
    /// none past the eighth row or past the end of its keyboard row.
    pub fn for_slot(row: usize, at: usize) -> Option<PadKey> {
        (row < KEYED_ROWS).then(|| KEY_ROWS[row % KEY_ROWS.len()].chars().nth(at)).flatten().map(|key| PadKey { key, alt: row >= KEY_ROWS.len() })
    }

    /// The `i`th key in board order for Auto columns, whose rows change with the window
    /// (D-189): each keyboard row filled before the next, then the four again with Option.
    pub fn nth(i: usize) -> Option<PadKey> {
        let mut i = i;
        for row in 0..KEYED_ROWS {
            let len = KEY_ROWS[row % KEY_ROWS.len()].chars().count();
            if i < len {
                return PadKey::for_slot(row, i);
            }
            i -= len;
        }
        None
    }

    /// Read a key as the CLI, a set file or a library writes it: `q`, `Q`, `alt+1`,
    /// `Option+1`, `opt+1`, `⌥1`. Anything off the four rows is refused.
    pub fn parse(s: &str) -> Option<PadKey> {
        let lower: String = s.to_lowercase().split_whitespace().collect();
        let (alt, rest) = ["alt+", "option+", "opt+", "⌥"]
            .iter()
            .find_map(|p| lower.strip_prefix(p).map(|r| (true, r)))
            .unwrap_or((false, lower.as_str()));
        let mut chars = rest.trim().chars();
        let key = chars.next().filter(|_| chars.next().is_none())?;
        KEY_ROWS.iter().any(|r| r.contains(key)).then_some(PadKey { key, alt })
    }

    /// The stored and scripted form: `q`, `alt+q`.
    pub fn name(self) -> String {
        if self.alt { format!("alt+{}", self.key) } else { self.key.to_string() }
    }

    /// As the pad's key cap and the inspector show it: `Q`, and `⌥Q` on macOS or
    /// `Alt+Q` elsewhere.
    pub fn label(self) -> String {
        let key = self.key.to_ascii_uppercase();
        match (self.alt, cfg!(target_os = "macos")) {
            (false, _) => key.to_string(),
            (true, true) => format!("⌥{key}"),
            (true, false) => format!("Alt+{key}"),
        }
    }

    /// As a screen reader says it: `Q`, `Option Q`, `Alt Q`.
    pub fn spoken(self) -> String {
        let key = self.key.to_ascii_uppercase();
        match (self.alt, cfg!(target_os = "macos")) {
            (false, _) => key.to_string(),
            (true, true) => format!("Option {key}"),
            (true, false) => format!("Alt {key}"),
        }
    }
}

impl serde::Serialize for PadKey {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.name())
    }
}

impl<'de> serde::Deserialize<'de> for PadKey {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        PadKey::parse(&s).ok_or_else(|| serde::de::Error::custom(format!("{s:?} isn't a pad key")))
    }
}

/// A stored key that isn't a pad key reads as none, rather than failing the library.
pub fn lenient_key<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<PadKey>, D::Error> {
    let s: Option<String> = serde::Deserialize::deserialize(d)?;
    Ok(s.as_deref().and_then(PadKey::parse))
}

/// The automatic key of each of the set's tiles, by index (D-188, D-189, D-192): the Rows
/// grid by its rows and slots (blank slots count), Fixed columns by rows of its column
/// count, Auto columns in board order, and none in a List set, which is stepped through
/// with GO and Enter (a key set by hand still works there, D-195).
pub fn auto_keys(set: &SoundSet, grid: GridMode) -> Vec<Option<PadKey>> {
    let n = set.tiles.len();
    match (set.layout, grid) {
        (Layout::Grid, GridMode::Rows) => {
            let mut keys = vec![None; n];
            for (r, row) in set.slots().iter().enumerate() {
                for (at, slot) in row.iter().enumerate() {
                    if let Some(i) = *slot {
                        keys[i] = PadKey::for_slot(r, at);
                    }
                }
            }
            keys
        }
        (Layout::Grid, GridMode::Columns) => {
            let cols = set.columns.max(1) as usize;
            (0..n).map(|i| PadKey::for_slot(i / cols, i % cols)).collect()
        }
        (Layout::Grid, GridMode::Auto) => (0..n).map(PadKey::nth).collect(),
        (Layout::List, _) => vec![None; n],
    }
}

/// Give every tile its key: a key set by hand stays (the first tile keeps one that two
/// were given), and every other tile takes its slot's automatic key unless a hand-set
/// one has it.
pub fn rekey(set: &mut SoundSet, grid: GridMode) {
    let auto = auto_keys(set, grid);
    let mut taken = HashSet::new();
    for t in set.tiles.iter_mut().filter(|t| t.hotkey_manual) {
        if t.hotkey.is_some_and(|k| !taken.insert(k)) {
            t.hotkey_manual = false;
        }
    }
    for (t, key) in set.tiles.iter_mut().zip(auto) {
        if !t.hotkey_manual {
            t.hotkey = key.filter(|k| !taken.contains(k));
        }
    }
}

/// Set (or clear) a tile's key by hand: it stays with the tile wherever it moves, and a
/// tile that had it goes back to its automatic key ([`rekey`] gives it).
pub fn set_hotkey(tiles: &mut [Tile], id: Uuid, key: Option<PadKey>) {
    for t in tiles.iter_mut() {
        if t.id == id {
            t.hotkey = key;
            t.hotkey_manual = true;
        } else if key.is_some() && t.hotkey == key {
            t.hotkey = None;
            t.hotkey_manual = false;
        }
    }
}

/// Give a tile back its automatic key.
pub fn set_hotkey_auto(tiles: &mut [Tile], id: Uuid) {
    if let Some(t) = tiles.iter_mut().find(|t| t.id == id) {
        t.hotkey_manual = false;
    }
}

pub fn tile_for_key(tiles: &[Tile], key: PadKey) -> Option<&Tile> {
    tiles.iter().find(|t| t.hotkey == Some(key))
}

/// Before D-188 keys ran straight through the keyboard (`1`–`0`, `Q`–`P`, `A`–`;`,
/// `Z`–`/`) in tile order, and a library didn't say which were set by hand.
const LEGACY_KEY_ORDER: &str = "1234567890qwertyuiopasdfghjkl;zxcvbnm,./";

/// Mark which of an older library's (or set file's) keys were set by hand (D-193): a
/// key the old order would have given the tile, by its place or as the first key free
/// of the tiles before it, was automatic; any other key was set by hand. A tile with no
/// key takes an automatic one.
pub fn guess_hand_set(tiles: &mut [Tile]) -> usize {
    let mut kept = 0;
    for i in 0..tiles.len() {
        let by_place = LEGACY_KEY_ORDER.chars().nth(i).map(PadKey::plain);
        let first_free = LEGACY_KEY_ORDER.chars().map(PadKey::plain).find(|k| !tiles[..i].iter().any(|t| t.hotkey == Some(*k)));
        let key = tiles[i].hotkey;
        let manual = key.is_some() && key != by_place && key != first_free;
        tiles[i].hotkey_manual = manual;
        kept += usize::from(manual);
    }
    kept
}

/// The default fade curve (quadratic in amplitude) for t in 0..=1.
pub fn fade_curve(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t
}

/// Gain multiplier from the tile's own fades.
/// `elapsed`: seconds since playback started (pauses excluded);
/// `position`: seconds into the trimmed region; `length`: trimmed region length.
/// Looping tiles fade in once and only fade out when stopped.
pub fn envelope_gain(elapsed: f64, position: f64, length: f64, fade_in: f32, fade_out: f32, looping: bool, curve: FadeCurve) -> f32 {
    let mut g = 1.0;
    if fade_in > 0.0 && elapsed < fade_in as f64 {
        g *= curve.gain((elapsed / fade_in as f64) as f32);
    }
    if !looping && fade_out > 0.0 {
        let remaining = length - position;
        if remaining < fade_out as f64 {
            g *= curve.gain((remaining / fade_out as f64) as f32);
        }
    }
    g
}

/// The (start, end) seconds actually played for a file of `duration` seconds.
/// Out-of-range or inverted trims fall back to the file bounds.
pub fn play_range(tile: &Tile, duration: f64) -> (f64, f64) {
    let dur = duration.max(0.0);
    let start = tile.start.clamp(0.0, dur);
    let end = tile.end.map(|e| e.clamp(0.0, dur)).unwrap_or(dur);
    if end > start {
        (start, end)
    } else {
        (start, dur.max(start))
    }
}

/// The vamp section actually looped for a file of `duration` seconds: inside the trim
/// and at least 50 ms long, or `None`.
pub fn vamp_range(tile: &Tile, duration: f64) -> Option<(f64, f64)> {
    let (s, e) = play_range(tile, duration);
    let (a, b) = tile.vamp?;
    let (a, b) = (a.clamp(s, e), b.clamp(s, e));
    (b - a >= 0.05).then_some((a, b))
}

/// `m:ss`, `h:mm:ss`, optionally with tenths (`m:ss.t`).
pub fn format_clock(seconds: f64, tenths: bool) -> String {
    let secs = if seconds.is_finite() { seconds.max(0.0) } else { 0.0 };
    let (whole, frac) = if tenths {
        let t = (secs * 10.0 + 1e-6).floor() as u64;
        (t / 10, Some(t % 10))
    } else {
        ((secs + 1e-6).floor() as u64, None)
    };
    let (h, m, s) = (whole / 3600, (whole / 60) % 60, whole % 60);
    let mut out = if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") };
    if let Some(f) = frac {
        out.push_str(&format!(".{f}"));
    }
    out
}

/// `m:ss.mmm`, or `h:mm:ss.mmm` from an hour up: the editor's millisecond fields.
pub fn format_timecode(seconds: f64) -> String {
    let secs = if seconds.is_finite() { seconds.max(0.0) } else { 0.0 };
    let ms = (secs * 1000.0 + 1e-6).round() as u64;
    let (h, m, s, milli) = (ms / 3_600_000, (ms / 60_000) % 60, (ms / 1000) % 60, ms % 1000);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}.{milli:03}")
    } else {
        format!("{m}:{s:02}.{milli:03}")
    }
}

/// `N smp`: a position as a whole number of frames at `rate`.
pub fn format_samples(seconds: f64, rate: u32) -> String {
    let frames = if seconds.is_finite() {
        (seconds * f64::from(rate)).round().max(0.0) as u64
    } else {
        0
    };
    format!("{frames} smp")
}

/// Parse what someone types into a time field, to seconds.
///
/// Accepts `h:mm:ss` / `m:ss` (fraction allowed on the seconds), plain seconds like
/// `12.5`, milliseconds (`250ms`) and sample counts (`12345smp`); anything else is
/// `None`. Numbers are checked character by character, so `1e3`, `inf` and signs never
/// sneak through `f64` parsing.
pub fn parse_timecode(text: &str, rate: u32) -> Option<f64> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    if let Some(num) = strip_suffix_word(text, "smp") {
        if rate == 0 {
            return None;
        }
        let frames = parse_uint(num)?;
        return Some(frames as f64 / f64::from(rate));
    }
    if let Some(num) = strip_suffix_word(text, "ms") {
        return Some(parse_decimal(num)? / 1000.0);
    }
    if text.contains(':') {
        return parse_colon_time(text);
    }
    parse_decimal(text)
}

/// `text` minus a trailing ASCII word like `ms`/`smp` (case-insensitive), with any
/// whitespace between the number and the word trimmed.
fn strip_suffix_word<'a>(text: &'a str, word: &str) -> Option<&'a str> {
    // `get`, not `split_at`: the cut may fall inside a multi-byte character.
    let cut = text.len().checked_sub(word.len())?;
    let tail = text.get(cut..)?;
    tail.eq_ignore_ascii_case(word).then(|| text[..cut].trim_end())
}

/// Non-empty ASCII digits as `u64`.
fn parse_uint(text: &str) -> Option<u64> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// ASCII digits with at most one `.` (`12`, `12.5`, `.5`, `12.`); never signs,
/// exponents or `inf`/`nan`, which `f64` parsing would otherwise accept.
fn parse_decimal(text: &str) -> Option<f64> {
    if text.is_empty() {
        return None;
    }
    let mut seen_dot = false;
    let mut seen_digit = false;
    for b in text.bytes() {
        match b {
            b'.' if !seen_dot => seen_dot = true,
            b'0'..=b'9' => seen_digit = true,
            _ => return None,
        }
    }
    if !seen_digit {
        return None;
    }
    let v: f64 = text.parse().ok()?;
    v.is_finite().then_some(v)
}

/// `h:mm:ss` or `m:ss`, with an optional fraction on the seconds.
fn parse_colon_time(text: &str) -> Option<f64> {
    let parts: Vec<&str> = text.split(':').collect();
    let (h, m, s) = match parts.as_slice() {
        [mins, secs] => (0.0, parse_uint(mins)? as f64, parse_seconds(secs)?),
        [first, mins, secs] => (
            parse_uint(first)? as f64,
            clock_field(mins)? as f64,
            parse_seconds(secs)?,
        ),
        _ => return None,
    };
    // A fraction hundreds of digits long would make the seconds NaN.
    let t = h * 3600.0 + m * 60.0 + s;
    t.is_finite().then_some(t)
}

/// A 1-2 digit field below 60 (`mm` or whole `ss`).
fn clock_field(text: &str) -> Option<u64> {
    if text.len() > 2 {
        return None;
    }
    let v = parse_uint(text)?;
    if v >= 60 {
        return None;
    }
    Some(v)
}

/// Seconds with an optional fraction: `05`, `05.25`, `05.`.
fn parse_seconds(text: &str) -> Option<f64> {
    let (int, frac) = match text.split_once('.') {
        Some((i, f)) => (i, f),
        None => (text, ""),
    };
    let secs = clock_field(int)? as f64;
    let mut scale = 1.0;
    let mut frac_val = 0.0;
    for b in frac.bytes() {
        if !b.is_ascii_digit() {
            return None;
        }
        scale *= 10.0;
        frac_val = frac_val * 10.0 + f64::from(b - b'0');
    }
    Some(secs + frac_val / scale)
}

/// Quietest level shown, in dB; gain 0 maps here.
pub const DB_FLOOR: f32 = -80.0;

pub fn gain_to_db(g: f32) -> f32 {
    if g <= 0.0 {
        DB_FLOOR
    } else {
        (20.0 * g.log10()).max(DB_FLOOR)
    }
}

pub fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// SoundCraft's meter curve: −80…−40 dB fills the bottom 10 %, −40…−20 the next 20 %,
/// −20…0 the top 70 %. The window's meters and OSC's `/ls/tile/N/level` use it.
pub fn meter_frac(db: f32) -> f32 {
    let db = db.clamp(DB_FLOOR, 0.0);
    if db < -40.0 {
        (db + 80.0) / 40.0 * 0.1
    } else if db < -20.0 {
        0.1 + (db + 40.0) / 20.0 * 0.2
    } else {
        0.3 + (db + 20.0) / 20.0 * 0.7
    }
}

/// One meter channel's display level in dB, with SoundCraft-style ballistics: instant
/// attack, a 26 dB/s fall and a 2 s peak hold. The master meter and the tiles' meters
/// share it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ballistics {
    pub level: f32,
    pub hold: f32,
    /// Seconds since `hold` was last set.
    held: f32,
}

impl Default for Ballistics {
    fn default() -> Self {
        Ballistics { level: DB_FLOOR, hold: DB_FLOOR, held: 0.0 }
    }
}

impl Ballistics {
    /// Show `peak` (linear) after `dt` seconds.
    pub fn feed(&mut self, peak: f32, dt: f32) {
        self.level = gain_to_db(peak).max(self.level - 26.0 * dt);
        self.held += dt;
        if self.level >= self.hold || self.held > 2.0 {
            self.hold = self.level;
            self.held = 0.0;
        }
    }

    /// Fallen to the floor, hold included.
    pub fn idle(&self) -> bool {
        self.level.max(self.hold) <= DB_FLOOR + 0.5
    }
}

/// Search: the title or the notes contain `query`, ignoring case. `query` is lower case.
pub fn matches_query(t: &Tile, query: &str) -> bool {
    t.title.to_lowercase().contains(query) || t.notes.to_lowercase().contains(query)
}

/// Whether GO stops at this tile: it has audio or an action, and isn't disabled.
pub fn is_cue(t: &Tile) -> bool {
    (t.file.is_some() || t.action.is_some()) && !t.disabled
}

/// The first playable cue at or after `from`.
pub fn next_cue(tiles: &[Tile], from: usize) -> Option<usize> {
    (from..tiles.len()).find(|&i| is_cue(&tiles[i]))
}

/// The pointer after the cue at `i` starts: past it and every cue its links (continue
/// or follow, through the following cues' own links) will start.
pub fn pointer_after_chain(tiles: &[Tile], i: usize) -> usize {
    let mut k = i;
    while tiles.get(k).is_some_and(|t| t.link != crate::model::CueLink::None) {
        match next_cue(tiles, k + 1) {
            Some(n) => k = n,
            None => return tiles.len(),
        }
    }
    k + 1
}

/// `cue.next`: the pointer once the next cue is skipped without playing it (past the
/// last cue, the end of the list).
pub fn pointer_after_next(tiles: &[Tile], pointer: usize) -> usize {
    next_cue(tiles, pointer).map_or(tiles.len(), |i| i + 1)
}

/// In a List set: the first cue at or after `from` that is still armed (it hasn't played
/// since the list was reset), else the end of the list.
pub fn next_armed(tiles: &[Tile], from: usize, played: &std::collections::HashSet<Uuid>) -> usize {
    (from..tiles.len()).find(|&i| is_cue(&tiles[i]) && !played.contains(&tiles[i].id)).unwrap_or(tiles.len())
}

/// `cue.prev`: the pointer on the cue before the next one. At the first cue it stays.
pub fn pointer_before_next(tiles: &[Tile], pointer: usize) -> usize {
    let cur = next_cue(tiles, pointer).unwrap_or(tiles.len()).min(tiles.len());
    (0..cur).rev().find(|&i| is_cue(&tiles[i])).unwrap_or(pointer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use PlayState::*;
    use TriggerAction as A;
    use TriggerEvent::*;

    #[test]
    fn meter_ballistics_attack_at_once_fall_slowly_and_hold() {
        let mut b = Ballistics::default();
        assert!(b.idle());
        b.feed(0.5, 0.02);
        assert!((b.level - -6.02).abs() < 0.01, "instant attack: {}", b.level);
        // Silence: down 26 dB a second, the hold stays put for 2 s.
        b.feed(0.0, 0.5);
        assert!((b.level - -19.02).abs() < 0.01, "{}", b.level);
        assert!((b.hold - -6.02).abs() < 0.01);
        for _ in 0..4 {
            b.feed(0.0, 0.5);
        }
        assert!(b.hold < -6.5, "hold released after 2 s: {}", b.hold);
        for _ in 0..10 {
            b.feed(0.0, 0.5);
        }
        assert!(b.idle());
    }

    /// Every option combination (D-145) against every state, for a press and a release.
    #[test]
    fn every_option_combination() {
        let o = |hold, pausable, one_shot| PressOptions { hold, pausable, one_shot };
        // Columns: Stopped, Playing, Paused, Stopping.
        #[rustfmt::skip]
        let table = [
            // none: play, stop (Play / Stop).
            (o(false, false, false), [A::Play, A::Stop, A::Resume, A::Play], [A::None; 4]),
            // Pausable: play, pause, resume (Play / Pause).
            (o(false, true, false), [A::Play, A::Pause, A::Resume, A::Play], [A::None; 4]),
            // One-shot: every press starts again from the beginning (Always Restart).
            (o(false, false, true), [A::Play; 4], [A::None; 4]),
            // Hold: plays while held, stops on release.
            (o(true, false, false), [A::Play, A::None, A::Play, A::Play], [A::None, A::Stop, A::Stop, A::None]),
            // Hold + Pausable: release pauses, holding again resumes from that spot.
            (o(true, true, false), [A::Play, A::None, A::Resume, A::Play], [A::None, A::Pause, A::None, A::None]),
            // Hold + One-shot: plain Hold, each hold from the beginning.
            (o(true, false, true), [A::Play, A::None, A::Play, A::Play], [A::None, A::Stop, A::Stop, A::None]),
        ];
        for (opts, press, release) in table {
            for (i, state) in [Stopped, Playing, Paused, Stopping].into_iter().enumerate() {
                assert_eq!(resolve_trigger(opts, state, Press), press[i], "{opts:?} pressed while {state:?}");
                assert_eq!(resolve_trigger(opts, state, Release), release[i], "{opts:?} released while {state:?}");
            }
        }
    }

    #[test]
    fn hold_with_pausable_pauses_on_release_and_resumes_on_the_next_hold() {
        let opts = PressOptions { hold: true, pausable: true, one_shot: false };
        let mut state = Stopped;
        for (event, want, next) in [(Press, A::Play, Playing), (Release, A::Pause, Paused), (Press, A::Resume, Playing), (Release, A::Pause, Paused)] {
            assert_eq!(resolve_trigger(opts, state, event), want);
            state = next;
        }
    }

    /// A vamping tile through presses and releases, for every option combination
    /// (D-154): the trigger that would end it plays on, then the options apply.
    #[test]
    fn a_vamp_plays_on_then_the_options_apply() {
        let o = |hold, pausable, one_shot| PressOptions { hold, pausable, one_shot };
        // (vamping, played on) after each action, from the tile's point of view.
        fn step(state: &mut (PlayState, bool), a: A) {
            *state = match a {
                A::None => *state,
                A::Play => (Playing, true),
                A::Stop => (Stopped, false),
                A::Pause => (Paused, state.1),
                A::Resume => (Playing, state.1),
                A::PlayOn => (Playing, false),
            };
        }
        #[rustfmt::skip]
        let table: [(PressOptions, &[(TriggerEvent, A)]); 6] = [
            // Plain: play (vamping), play on, stop.
            (o(false, false, false), &[(Press, A::Play), (Release, A::None), (Press, A::PlayOn), (Press, A::Stop)]),
            // Pausable: play, play on, pause, resume.
            (o(false, true, false), &[(Press, A::Play), (Press, A::PlayOn), (Press, A::Pause), (Press, A::Resume)]),
            // One-shot: play, play on, then from the top (vamping again), play on.
            (o(false, false, true), &[(Press, A::Play), (Press, A::PlayOn), (Press, A::Play), (Press, A::PlayOn)]),
            // Hold: vamps while held, letting go plays on; a hold and let-go stops it.
            (o(true, false, false), &[(Press, A::Play), (Press, A::None), (Release, A::PlayOn), (Press, A::None), (Release, A::Stop)]),
            // Hold + Pausable: letting go plays on; then a let-go pauses, a hold resumes.
            (o(true, true, false), &[(Press, A::Play), (Release, A::PlayOn), (Press, A::None), (Release, A::Pause), (Press, A::Resume)]),
            // Hold + One-shot: as Hold.
            (o(true, false, true), &[(Press, A::Play), (Release, A::PlayOn), (Press, A::None), (Release, A::Stop)]),
        ];
        for (opts, steps) in table {
            let mut state = (Stopped, false);
            for (n, &(event, want)) in steps.iter().enumerate() {
                let got = resolve_press(opts, state.0, state.1, event);
                assert_eq!(got, want, "{opts:?}, step {n}: {event:?} while {state:?}");
                step(&mut state, got);
            }
        }
        // A paused vamping tile resumes, still vamping; not vamping, nothing changes.
        assert_eq!(resolve_press(o(false, true, false), Paused, true, Press), A::Resume);
        assert_eq!(resolve_press(o(false, false, false), Playing, false, Press), A::Stop);
        assert_eq!(resolve_press(o(false, false, false), Stopping, true, Press), A::Play);
    }

    fn tiles(n: usize) -> Vec<Tile> {
        (0..n).map(|_| Tile::default()).collect()
    }

    fn key(s: &str) -> Option<PadKey> {
        PadKey::parse(s)
    }

    #[test]
    fn pad_keys_parse_and_name() {
        assert_eq!(key("Q"), Some(PadKey::plain('q')));
        assert_eq!(key("alt+1"), Some(PadKey { key: '1', alt: true }));
        for s in ["Alt+1", "option+1", "Opt+1", "⌥1", " alt + 1 "] {
            assert_eq!(key(s), key("alt+1"), "{s:?}");
        }
        for s in ["", "!", "qq", "alt+", "ctrl+1", "`", "\\"] {
            assert_eq!(key(s), None, "{s:?}");
        }
        assert_eq!(key("alt+'").map(PadKey::name).as_deref(), Some("alt+'"));
        assert_eq!(key("=").map(PadKey::name).as_deref(), Some("="));
        let label = key("alt+q").unwrap().label();
        assert_eq!(label, if cfg!(target_os = "macos") { "⌥Q" } else { "Alt+Q" });
        assert_eq!(serde_json::to_string(&key("alt+q")).unwrap(), "\"alt+q\"");
        assert_eq!(serde_json::from_str::<PadKey>("\"q\"").unwrap(), PadKey::plain('q'));
    }

    #[test]
    fn slots_take_their_row_s_keys() {
        let k = |r, a| PadKey::for_slot(r, a).map(PadKey::name);
        assert_eq!(k(0, 0).as_deref(), Some("1"));
        assert_eq!(k(0, 11).as_deref(), Some("="));
        assert_eq!(k(0, 12), None, "past the end of the keyboard row");
        assert_eq!(k(1, 11).as_deref(), Some("]"));
        assert_eq!(k(2, 10).as_deref(), Some("'"));
        assert_eq!(k(2, 11), None);
        assert_eq!(k(3, 9).as_deref(), Some("/"));
        assert_eq!(k(4, 0).as_deref(), Some("alt+1"));
        assert_eq!(k(7, 9).as_deref(), Some("alt+/"));
        assert_eq!(k(8, 0), None, "past the eighth row");
        // Board order: 12 + 12 + 11 + 10 plain, then the same with Option.
        assert_eq!(PadKey::nth(11).map(PadKey::name).as_deref(), Some("="));
        assert_eq!(PadKey::nth(12).map(PadKey::name).as_deref(), Some("q"));
        assert_eq!(PadKey::nth(44).map(PadKey::name).as_deref(), Some("/"));
        assert_eq!(PadKey::nth(45).map(PadKey::name).as_deref(), Some("alt+1"));
        assert_eq!(PadKey::nth(89).map(PadKey::name).as_deref(), Some("alt+/"));
        assert_eq!(PadKey::nth(90), None);
    }

    fn set(n: usize) -> SoundSet {
        SoundSet { tiles: tiles(n), ..Default::default() }
    }

    fn names(set: &SoundSet) -> Vec<String> {
        set.tiles.iter().map(|t| t.hotkey.map_or_else(|| "-".into(), PadKey::name)).collect()
    }

    #[test]
    fn rows_give_keys_by_slot_and_blanks_count() {
        let mut s = set(7);
        s.rows = vec![3, 4];
        s.blanks = vec![[1, 0]];
        rekey(&mut s, GridMode::Rows);
        assert_eq!(names(&s), ["1", "2", "3", "w", "e", "r", "t"], "the blank slot is q's");
        // Fixed columns: rows of the column count; Auto and the List in board order.
        let mut s = set(14);
        s.columns = 4;
        rekey(&mut s, GridMode::Columns);
        assert_eq!(names(&s)[3..6], ["4", "q", "w"]);
        rekey(&mut s, GridMode::Auto);
        assert_eq!(names(&s)[11..14], ["=", "q", "w"]);
        // A List set has no automatic keys (D-192); one set by hand still works.
        let id = s.tiles[3].id;
        set_hotkey(&mut s.tiles, id, key("alt+q"));
        s.layout = Layout::List;
        rekey(&mut s, GridMode::Columns);
        assert_eq!(names(&s)[2..5], ["-", "alt+q", "-"]);
        assert!(names(&s).iter().filter(|k| *k != "-").count() == 1);
    }

    #[test]
    fn a_hand_set_key_stays_and_is_taken_from_the_slot() {
        let mut s = set(3);
        s.rows = vec![3];
        rekey(&mut s, GridMode::Rows);
        let (a, c) = (s.tiles[0].id, s.tiles[2].id);
        // C takes 1 by hand: A, whose slot gives 1, has no key now.
        set_hotkey(&mut s.tiles, c, key("1"));
        rekey(&mut s, GridMode::Rows);
        assert_eq!(names(&s), ["-", "2", "1"]);
        // C moves to the front: its key goes with it, and A's slot gives A 2.
        let moved = s.tiles.remove(2);
        s.tiles.insert(0, moved);
        rekey(&mut s, GridMode::Rows);
        assert_eq!(names(&s), ["1", "2", "3"]);
        set_hotkey(&mut s.tiles, a, key("alt+z"));
        rekey(&mut s, GridMode::Rows);
        assert_eq!(names(&s), ["1", "alt+z", "3"]);
        // Taking a hand-set key from another tile sends that one back to its slot's key.
        set_hotkey(&mut s.tiles, c, key("alt+z"));
        rekey(&mut s, GridMode::Rows);
        assert_eq!(names(&s), ["alt+z", "2", "3"]);
        assert!(!s.tiles[1].hotkey_manual);
        // Cleared by hand: no key, until it goes back to automatic.
        set_hotkey(&mut s.tiles, a, None);
        rekey(&mut s, GridMode::Rows);
        assert_eq!(names(&s), ["alt+z", "-", "3"]);
        set_hotkey_auto(&mut s.tiles, a);
        rekey(&mut s, GridMode::Rows);
        assert_eq!(names(&s), ["alt+z", "2", "3"]);
        assert_eq!(tile_for_key(&s.tiles, PadKey::plain('2')).map(|t| t.id), Some(a));
    }

    #[test]
    fn old_keys_in_the_old_order_were_automatic() {
        let mut ts = tiles(6);
        // By place, first free (after a tile with none), set by hand, none.
        for (t, k) in ts.iter_mut().zip(["1", "2", "", "3", "r", ""]) {
            t.hotkey = key(k);
        }
        assert_eq!(guess_hand_set(&mut ts), 1);
        assert_eq!(ts.iter().map(|t| t.hotkey_manual).collect::<Vec<_>>(), [false, false, false, false, true, false]);
    }

    #[test]
    fn envelope_fades() {
        // 10 s clip, 2 s fades.
        assert_eq!(envelope_gain(0.0, 0.0, 10.0, 2.0, 2.0, false, FadeCurve::Gentle), 0.0);
        assert!((envelope_gain(1.0, 1.0, 10.0, 2.0, 2.0, false, FadeCurve::Gentle) - 0.25).abs() < 1e-6);
        assert_eq!(envelope_gain(5.0, 5.0, 10.0, 2.0, 2.0, false, FadeCurve::Gentle), 1.0);
        assert!((envelope_gain(9.0, 9.0, 10.0, 2.0, 2.0, false, FadeCurve::Gentle) - 0.25).abs() < 1e-6);
        // Looping: no tail fade, fade-in only on the first pass.
        assert_eq!(envelope_gain(29.0, 9.0, 10.0, 2.0, 2.0, true, FadeCurve::Gentle), 1.0);
        assert_eq!(envelope_gain(5.0, 5.0, 10.0, 0.0, 0.0, false, FadeCurve::Gentle), 1.0);
        // Other shapes, halfway through a 2 s fade-in.
        let half = |c| envelope_gain(1.0, 1.0, 10.0, 2.0, 2.0, false, c);
        assert!((half(FadeCurve::Linear) - 0.5).abs() < 1e-6);
        assert!((half(FadeCurve::EqualPower) - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-3);
        assert!((half(FadeCurve::SCurve) - 0.5).abs() < 1e-6);
        assert!((half(FadeCurve::Exponential) - 0.0316).abs() < 1e-3, "-30 dB");
        for c in FadeCurve::ALL {
            assert_eq!(c.gain(0.0), 0.0, "{c:?}");
            assert!((c.gain(1.0) - 1.0).abs() < 1e-6, "{c:?}");
        }
    }

    #[test]
    fn play_range_clamps() {
        let mut t = Tile::default();
        assert_eq!(play_range(&t, 10.0), (0.0, 10.0));
        t.start = 2.0;
        t.end = Some(5.0);
        assert_eq!(play_range(&t, 10.0), (2.0, 5.0));
        t.end = Some(1.0); // inverted
        assert_eq!(play_range(&t, 10.0), (2.0, 10.0));
        t.start = 50.0;
        t.end = None;
        assert_eq!(play_range(&t, 10.0), (10.0, 10.0));
    }

    #[test]
    fn clock_format() {
        assert_eq!(format_clock(0.0, false), "0:00");
        assert_eq!(format_clock(65.4, false), "1:05");
        assert_eq!(format_clock(65.4, true), "1:05.4");
        assert_eq!(format_clock(3661.0, false), "1:01:01");
        assert_eq!(format_clock(-3.0, false), "0:00");
        assert_eq!(format_clock(f64::NAN, false), "0:00");
        assert_eq!(format_clock(2.9999999, false), "0:03", "float noise rounds up");
        assert_eq!(format_clock(2.95, false), "0:02");
    }

    #[test]
    fn timecode_format() {
        assert_eq!(format_timecode(0.0), "0:00.000");
        assert_eq!(format_timecode(65.25), "1:05.250");
        assert_eq!(format_timecode(3661.5), "1:01:01.500");
        assert_eq!(format_timecode(59.9996), "1:00.000", "carries into the minute");
        assert_eq!(format_timecode(3599.9996), "1:00:00.000", "carries into the hour");
        assert_eq!(format_timecode(2.9999999), "0:03.000", "float noise rounds up");
        assert_eq!(format_timecode(1.2344), "0:01.234");
        assert_eq!(format_timecode(0.0015), "0:00.002");
        assert_eq!(format_timecode(-3.0), "0:00.000");
        assert_eq!(format_timecode(f64::NAN), "0:00.000");
        assert_eq!(format_timecode(f64::INFINITY), "0:00.000");
    }

    #[test]
    fn samples_format() {
        assert_eq!(format_samples(1.0, 48_000), "48000 smp");
        assert_eq!(format_samples(0.0, 48_000), "0 smp");
        assert_eq!(format_samples(1.5, 44_100), "66150 smp");
        assert_eq!(format_samples(-2.0, 48_000), "0 smp");
        assert_eq!(format_samples(f64::NAN, 48_000), "0 smp");
        assert_eq!(format_samples(f64::INFINITY, 48_000), "0 smp");
    }

    #[test]
    fn timecode_parse_accepts() {
        let rate = 48_000;
        let ok = |text: &str| parse_timecode(text, rate).unwrap();
        assert_eq!(ok("1:05"), 65.0);
        assert_eq!(ok("1:5"), 65.0);
        assert_eq!(ok("1:05.25"), 65.25);
        assert_eq!(ok("1:01:05.250"), 3665.25);
        assert_eq!(ok("0:00.000"), 0.0);
        assert_eq!(ok("10:00"), 600.0);
        assert_eq!(ok("12"), 12.0);
        assert_eq!(ok("12.5"), 12.5);
        assert_eq!(ok(".5"), 0.5);
        assert_eq!(ok("12."), 12.0);
        assert_eq!(ok("  12.5  "), 12.5, "surrounding whitespace is trimmed");
        assert_eq!(ok("250ms"), 0.25);
        assert_eq!(ok("250 ms"), 0.25);
        assert_eq!(ok("250MS"), 0.25, "suffix is case-insensitive");
        assert_eq!(ok("1.5ms"), 0.0015);
        assert_eq!(ok("12345smp"), 12345.0 / 48_000.0);
        assert_eq!(ok("12345 smp"), 12345.0 / 48_000.0);
        assert_eq!(ok("48000SMP"), 1.0);
        assert_eq!(parse_timecode("12345smp", 0), None, "samples need a rate");
    }

    #[test]
    fn timecode_parse_rejects() {
        let rate = 48_000;
        for bad in [
            "", "   ", "-1", "+1", "1e3", "inf", "nan", "INF", "NaN", "1:60", "1:2:3:4",
            "1::2", "abc", "1,5", "1:05:06:07", ":", "1:", ":05", "1:05:60", "1:005", ".",
            "12..5", "1.2.3", "ms", "smp", "12x", "--1", "1éé", "5 μs", "２",
        ] {
            assert_eq!(parse_timecode(bad, rate), None, "input {bad:?} must not parse");
        }
        let long = format!("1:05.{}", "9".repeat(400));
        assert_eq!(parse_timecode(&long, rate), None, "a 400-digit fraction is not a time");
    }

    #[test]
    fn timecode_round_trip() {
        let rate = 48_000;
        // Awkward step so the fractional milliseconds land all over the place.
        let mut x = 0.0;
        while x < 4000.0 {
            let back = parse_timecode(&format_timecode(x), rate).unwrap();
            assert!(
                (back - x).abs() <= 0.0005 + 1e-9,
                "timecode round trip: x={x} back={back}"
            );
            let smp = parse_timecode(&format_samples(x, rate), rate).unwrap();
            assert_eq!(
                smp,
                (x * f64::from(rate)).round() / f64::from(rate),
                "samples round trip: x={x}"
            );
            x += 7.0 / 13.0;
        }
    }

    #[test]
    fn cue_pointer_skips_empty_tiles() {
        let mut ts = tiles(4);
        ts[0].file = Some("a".into());
        ts[3].file = Some("b".into());
        assert_eq!(next_cue(&ts, 0), Some(0));
        assert_eq!(next_cue(&ts, 1), Some(3));
        assert_eq!(next_cue(&ts, 4), None);
    }

    #[test]
    fn cue_prev_and_next_move_between_cues() {
        let mut ts = tiles(5);
        for i in [0, 2, 3] {
            ts[i].file = Some(format!("{i}"));
        }
        // Next skips the cue under the pointer, past the empty tile between.
        assert_eq!(pointer_after_next(&ts, 0), 1);
        assert_eq!(next_cue(&ts, pointer_after_next(&ts, 0)), Some(2));
        assert_eq!(pointer_after_next(&ts, 3), 4);
        assert_eq!(pointer_after_next(&ts, 4), 5, "past the last cue: the end of the list");
        // Previous lands on the cue before the next one, from anywhere.
        assert_eq!(pointer_before_next(&ts, 3), 2);
        assert_eq!(pointer_before_next(&ts, 1), 0, "1 shows cue 2 as next; previous is cue 0");
        assert_eq!(pointer_before_next(&ts, 5), 3, "from the end, the last cue");
        assert_eq!(pointer_before_next(&ts, 0), 0, "at the top it stays");
        // A disabled cue is skipped both ways.
        ts[2].disabled = true;
        assert_eq!(next_cue(&ts, 1), Some(3));
        assert_eq!(pointer_before_next(&ts, 3), 0);
    }
}
