//! Pure, UI- and device-free rules: what a press does, hotkey assignment, fade
//! envelopes, trim ranges and time formatting.

use crate::model::{PlayMode, Tile};
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
}

pub fn resolve_trigger(mode: PlayMode, state: PlayState, event: TriggerEvent) -> TriggerAction {
    use PlayState::*;
    use TriggerAction as A;
    match (mode, event) {
        (PlayMode::Toggle, TriggerEvent::Press) => match state {
            Stopped | Stopping => A::Play,
            Playing => A::Stop,
            Paused => A::Resume,
        },
        (PlayMode::Restart, TriggerEvent::Press) => A::Play,
        (PlayMode::Hold, TriggerEvent::Press) => match state {
            Playing => A::None,
            _ => A::Play,
        },
        (PlayMode::Hold, TriggerEvent::Release) => match state {
            Playing | Paused => A::Stop,
            _ => A::None,
        },
        (PlayMode::Pause, TriggerEvent::Press) => match state {
            Stopped | Stopping => A::Play,
            Playing => A::Pause,
            Paused => A::Resume,
        },
        (_, TriggerEvent::Release) => A::None,
    }
}

/// Default hotkey order, following the physical keyboard rows.
pub const KEY_LAYOUT: &str = "1234567890qwertyuiopasdfghjkl;zxcvbnm,./";

/// Keys accepted as tile hotkeys: those in [`KEY_LAYOUT`] (letters case-folded).
pub fn normalize_key(c: char) -> Option<char> {
    let c = c.to_ascii_lowercase();
    KEY_LAYOUT.contains(c).then_some(c)
}

pub fn first_free_key(tiles: &[Tile]) -> Option<char> {
    KEY_LAYOUT.chars().find(|k| !tiles.iter().any(|t| t.hotkey == Some(*k)))
}

/// Give every tile the hotkey matching its position (tiles past the 40th get none).
pub fn assign_keys_by_position(tiles: &mut [Tile]) {
    let mut keys = KEY_LAYOUT.chars();
    for t in tiles {
        t.hotkey = keys.next();
    }
}

/// Set (or clear) a tile's hotkey, taking it away from any other tile that had it.
pub fn set_hotkey(tiles: &mut [Tile], id: Uuid, key: Option<char>) {
    let key = key.and_then(normalize_key);
    for t in tiles.iter_mut() {
        if t.id == id {
            t.hotkey = key;
        } else if key.is_some() && t.hotkey == key {
            t.hotkey = None;
        }
    }
}

pub fn tile_for_key(tiles: &[Tile], key: char) -> Option<&Tile> {
    let key = normalize_key(key)?;
    tiles.iter().find(|t| t.hotkey == Some(key))
}

/// Perceptual-ish fade curve (quadratic in amplitude) for t in 0..=1.
pub fn fade_curve(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t
}

/// Gain multiplier from the tile's own fades.
/// `elapsed`: seconds since playback started (pauses excluded);
/// `position`: seconds into the trimmed region; `length`: trimmed region length.
/// Looping tiles fade in once and only fade out when stopped.
pub fn envelope_gain(elapsed: f64, position: f64, length: f64, fade_in: f32, fade_out: f32, looping: bool) -> f32 {
    let mut g = 1.0;
    if fade_in > 0.0 && elapsed < fade_in as f64 {
        g *= fade_curve((elapsed / fade_in as f64) as f32);
    }
    if !looping && fade_out > 0.0 {
        let remaining = length - position;
        if remaining < fade_out as f64 {
            g *= fade_curve((remaining / fade_out as f64) as f32);
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

/// The first playable cue at or after `from`.
pub fn next_cue(tiles: &[Tile], from: usize) -> Option<usize> {
    (from..tiles.len()).find(|&i| tiles[i].file.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use PlayState::*;
    use TriggerAction as A;
    use TriggerEvent::*;

    #[test]
    fn toggle_mode() {
        assert_eq!(resolve_trigger(PlayMode::Toggle, Stopped, Press), A::Play);
        assert_eq!(resolve_trigger(PlayMode::Toggle, Playing, Press), A::Stop);
        assert_eq!(resolve_trigger(PlayMode::Toggle, Stopping, Press), A::Play);
        assert_eq!(resolve_trigger(PlayMode::Toggle, Playing, Release), A::None);
    }

    #[test]
    fn restart_mode_always_plays() {
        for s in [Stopped, Playing, Paused, Stopping] {
            assert_eq!(resolve_trigger(PlayMode::Restart, s, Press), A::Play);
        }
    }

    #[test]
    fn hold_mode() {
        assert_eq!(resolve_trigger(PlayMode::Hold, Stopped, Press), A::Play);
        assert_eq!(resolve_trigger(PlayMode::Hold, Playing, Press), A::None);
        assert_eq!(resolve_trigger(PlayMode::Hold, Playing, Release), A::Stop);
        assert_eq!(resolve_trigger(PlayMode::Hold, Stopped, Release), A::None);
    }

    #[test]
    fn pause_mode() {
        assert_eq!(resolve_trigger(PlayMode::Pause, Stopped, Press), A::Play);
        assert_eq!(resolve_trigger(PlayMode::Pause, Playing, Press), A::Pause);
        assert_eq!(resolve_trigger(PlayMode::Pause, Paused, Press), A::Resume);
    }

    fn tiles(n: usize) -> Vec<Tile> {
        (0..n).map(|_| Tile::default()).collect()
    }

    #[test]
    fn hotkeys_fill_first_free_slot() {
        let mut ts = tiles(3);
        ts[0].hotkey = Some('1');
        ts[1].hotkey = Some('3');
        assert_eq!(first_free_key(&ts), Some('2'));
    }

    #[test]
    fn hotkeys_by_position_and_overflow() {
        let mut ts = tiles(42);
        assign_keys_by_position(&mut ts);
        assert_eq!(ts[0].hotkey, Some('1'));
        assert_eq!(ts[10].hotkey, Some('q'));
        assert_eq!(ts[39].hotkey, Some('/'));
        assert_eq!(ts[40].hotkey, None);
        assert_eq!(first_free_key(&ts), None);
    }

    #[test]
    fn set_hotkey_steals_and_normalizes() {
        let mut ts = tiles(2);
        ts[0].hotkey = Some('q');
        let id = ts[1].id;
        set_hotkey(&mut ts, id, Some('Q'));
        assert_eq!(ts[0].hotkey, None);
        assert_eq!(ts[1].hotkey, Some('q'));
        set_hotkey(&mut ts, id, Some('!'));
        assert_eq!(ts[1].hotkey, None, "keys outside the layout are rejected");
        assert_eq!(tile_for_key(&ts, 'q').map(|t| t.id), None);
    }

    #[test]
    fn envelope_fades() {
        // 10 s clip, 2 s fades.
        assert_eq!(envelope_gain(0.0, 0.0, 10.0, 2.0, 2.0, false), 0.0);
        assert!((envelope_gain(1.0, 1.0, 10.0, 2.0, 2.0, false) - 0.25).abs() < 1e-6);
        assert_eq!(envelope_gain(5.0, 5.0, 10.0, 2.0, 2.0, false), 1.0);
        assert!((envelope_gain(9.0, 9.0, 10.0, 2.0, 2.0, false) - 0.25).abs() < 1e-6);
        // Looping: no tail fade, fade-in only on the first pass.
        assert_eq!(envelope_gain(29.0, 9.0, 10.0, 2.0, 2.0, true), 1.0);
        assert_eq!(envelope_gain(5.0, 5.0, 10.0, 0.0, 0.0, false), 1.0);
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
    fn cue_pointer_skips_empty_tiles() {
        let mut ts = tiles(4);
        ts[0].file = Some("a".into());
        ts[3].file = Some("b".into());
        assert_eq!(next_cue(&ts, 0), Some(0));
        assert_eq!(next_cue(&ts, 1), Some(3));
        assert_eq!(next_cue(&ts, 4), None);
    }
}
