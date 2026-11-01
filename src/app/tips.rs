//! Hover text shared by the Console and Panel layouts, so both say the same thing, and the
//! tile's hover card. The style (D-087): what the control does, in plain words, then its
//! keys after a dash; a disabled control says what makes it available on a second line.
//! Everything goes through `theme::hint`, `hint_keys` or `hint_or`.

use super::*;

/// The bottom bar's Setlist button, by whether the setlist is shown (D-216).
pub(super) fn setlist(shown: bool) -> (&'static str, &'static str) {
    (if shown { "Setlist: hide the sets, on the left" } else { "Setlist: show the sets, on the left" }, "⌘/Ctrl+B")
}
/// The bottom bar's Inspector button, by whether the inspector is shown (D-216).
pub(super) fn inspector(shown: bool) -> (&'static str, &'static str) {
    (
        if shown { "Inspector: hide the selected tiles' settings, or the set's, on the right" } else { "Inspector: show the selected tiles' settings, or the set's, on the right" },
        "⌘/Ctrl+E",
    )
}
/// Why a control that would change the set is off while the set is locked.
pub(super) const LOCKED_WHY: &str = "This set is locked: unlock it with a right-click on the set in the setlist > Unlock Set, in the set's settings, or File > Lock Set";
pub(super) const PANEL_STANDARD: &str = "Panel's own colours: pastel blocks on black";
pub(super) const HIGH_CONTRAST: &str = "White on black with outlined controls, in either theme";
pub(super) const LOCK: &str = "Lock this set so nothing in it can be changed by accident: its tiles, their order and its settings. Its tiles still play";
pub(super) const UNLOCK: &str = "Unlock this set so its tiles and settings can be changed again";
pub(super) const GRID: (&str, &str) = ("Grid: show this set as a grid of tiles", "⌘/Ctrl+1");
pub(super) const LIST: (&str, &str) = ("List: show this set as a cue list, played in order, one cue at a time", "⌘/Ctrl+2");
pub(super) const ADD: (&str, &str) = ("Add sound or video files to this set as tiles", "⌘/Ctrl+O, or drop files on the window");
pub(super) const SETTINGS: (&str, &str) = ("Settings: outputs, keys, MIDI, OSC, guards and theme", "⌘/Ctrl+,");
pub(super) const NEW_SET: (&str, &str) = ("Add an empty set", "⌘/Ctrl+Shift+N");
pub(super) const CUE_PREV: (&str, &str) = ("Select the cue above, without playing", "Up arrow");
pub(super) const CUE_NEXT: (&str, &str) = ("Select the cue below, without playing", "Down arrow");
pub(super) const RESET_LIST: &str = "Arm every cue again and select the first; nothing stops";
pub(super) const CONTINUOUS: &str = "Continuous: when a cue ends on its own, the next armed cue after it plays";
pub(super) const QUEUED: &str = "Queued: when a cue ends or stops, the next armed cue after it is selected and waits for Enter";
pub(super) const CROSSFADE: &str = "Continuous: how long each cue overlaps the end of the one before, one fading out as the other fades in (at most half of either)";
pub(super) const CROSSFADE_WHY: &str = "Only Continuous cross-fades: choose Continuous first";
pub(super) const CUE_ARM: &str = "Armed: the list plays this cue when it gets to it, and the mark clears once it has played. Click to pass it over, or to arm it again";
pub(super) const PAUSE_ALL: (&str, &str) = ("Pause everything that's playing, pausable or not; press again to carry on", "Space");
pub(super) const RESUME_ALL: (&str, &str) = ("Carry on with everything that's paused", "Space");
pub(super) const PAUSE_ALL_WHY: &str = "Nothing is playing or paused";
pub(super) const FADE_ALL: &str = "Fade everything out over 2 seconds";
/// The Random button (D-171): the same hover text as Playback ▸ Play a Random Tile, and
/// why it's greyed out.
pub(super) const RANDOM: &str = crate::menu::RANDOM_HELP;
pub(super) const RANDOM_WHY: &str = "No pad in this set has a sound (or every one is disabled)";
pub(super) const ADD_SLOT: (&str, &str) = ("Add sounds to this set", "⌘/Ctrl+O, or drop files here");

/// Stop All's hover text, which depends on the hold guard.
pub(super) fn stop_all(hold: bool) -> String {
    let keys = if hold {
        "hold Esc for a moment (the guard is on); Shift+Esc stops at once"
    } else {
        "Esc fades out (with a Stop All fade set); a second Esc or Shift+Esc stops at once"
    };
    format!("Stop everything that's playing — {keys}")
}

/// The master meter's hover text, with the volume now.
pub(super) fn master(volume: f32) -> String {
    let db = if volume <= 0.0 { "silent".to_owned() } else { format!("{:+.1} dB", gain_to_db(volume)).replace('-', "−") };
    format!(
        "Master output level, and the master volume ({db}). Drag to set the volume; click the clip light to clear it \
         — ⌘/Ctrl+Up and ⌘/Ctrl+Down change it by 3 dB"
    )
}

/// A set in the sidebar.
pub(super) fn set_row(name: &str, tiles: usize, playing: bool, locked: bool) -> String {
    let live = if playing { ", playing" } else { "" };
    let lock = if locked { ", locked" } else { "" };
    format!(
        "{name}: {tiles} tile{}{live}{lock}. Click to show it and its settings in the inspector, double-click to rename, \
         right-click for more — ⌘/Ctrl+[ and ⌘/Ctrl+] move between sets",
        if tiles == 1 { "" } else { "s" }
    )
}

pub(super) const RENAME_FIELD: &str = "The set's new name: Enter keeps it, Esc cancels";

impl StingerApp {
    /// A tile's hover card: its title, how it plays, its key and length, where it plays,
    /// its notes, what starts the next cue, and what a click does. `set` names the set it
    /// lives in, for search hits.
    pub(super) fn tile_tip(&self, tile: &Tile, set: Option<&str>) -> String {
        let mut lines = vec![if tile.title.trim().is_empty() { "Untitled tile".to_owned() } else { tile.title.clone() }];
        if let Some(set) = set {
            lines.push(format!("In “{set}”"));
        }
        let mut how = Vec::new();
        match &tile.action {
            Some(a) => how.push(a.kind.label().to_owned()),
            // A plain pad (press plays, press again stops) says nothing.
            // The fade badge's hover text: how long the end fade-out is (D-168).
            None => how.extend(super::pad_options::option_words(tile).into_iter().map(|w| match w {
                "fades out" => format!("fades out over its last {}", super::wait_text(tile.fade_out as f64)),
                w => w.to_owned(),
            })),
        }
        how.push(match tile.hotkey {
            Some(k) => format!("key {}", k.spoken()),
            None => "no key".into(),
        });
        if tile.action.is_none() {
            match self.ctl.store.audio(tile) {
                Some(a) => {
                    let (s, e) = play_range(tile, a.duration());
                    how.push(format_clock((e - s) / tile.speed.clamp(0.5, 2.0) as f64, true));
                }
                None if tile.file.is_none() => how.push("no sound yet".into()),
                None => {}
            }
        }
        lines.push(how.join(" · "));
        if let Some(c) = self.action_caption(tile) {
            lines.push(c);
        } else if tile.file.is_some() {
            let sends: Vec<String> = tile
                .routes()
                .iter()
                .map(|s| if s.gain_db == 0.0 { s.output.clone() } else { format!("{} ({:+.1} dB)", s.output, s.gain_db).replace('-', "−") })
                .collect();
            lines.push(format!("Plays on {}", sends.join(", ")));
        }
        if let Some(link) = link_text(tile, None) {
            lines.push(link);
        }
        if tile.disabled {
            lines.push("Disabled: keys, pads and GO skip it".into());
        }
        let notes = tile.notes.trim();
        if !notes.is_empty() {
            let short: String = notes.chars().take(240).collect();
            lines.push(if short.len() < notes.len() { format!("{short}…") } else { short });
        }
        let locked = self.ctl.store.library.locate(tile.id).is_some_and(|(si, _)| self.ctl.store.library.sets[si].locked);
        lines.push(
            if locked {
                "Click to select it, double-click to play it (on a Hold tile, keep the second click down); its set is locked, \
                 so it can't be moved or changed — focused: Enter plays or stops it (Space is Pause All)"
            } else {
                "Click to select it, double-click to play it (on a Hold tile, keep the second click down), drag to move it \
                 (selected, the whole selection moves; onto a set in the sidebar, into that set); ⌘/Ctrl-click adds it to \
                 the selection; right-click for more — focused: Enter plays or stops it (Space is \
                 Pause All), ⌘/Ctrl+Up/Down moves it"
            }
            .into(),
        );
        lines.join("\n")
    }
}

pub(super) const HOTKEY_TIP: &str = "The key that plays this tile while the window has focus. A pad takes its row's key (row 1: 1 to =, row 2: Q to ], row 3: A to ', row 4: Z to /, rows 5 to 8: the same with Option or Alt; a cue list has none) and it changes when the pad moves. Click, then press a key from those rows, Option or Alt too, to set one by hand that stays with the tile (Backspace clears, Esc keeps it)";
pub(super) const HOTKEY_AUTO_TIP: &str = "Give the tile back its slot's key, which follows it as it moves";
pub(super) const DUCK_TIP: &str = "Whether this tile dips beds while it plays, or is a bed that dips";
pub(super) const FADE_IN_TIP: &str = "Seconds to fade in when it starts";
pub(super) const FADE_OUT_TIP: &str = "Seconds to fade out at the end of the clip";
pub(super) const STOP_FADE_TIP: &str = "Seconds to fade out when it's stopped (a click, its key, an action, Solo); 0 stops at once, with a 0.1 s de-click so it doesn't click or pop";
pub(super) const FADE_SHAPE_TIP: &str = "The curve every fade on this tile follows";

/// "1.2 seconds", "1:12" — a time for a screen reader.
/// A tile's meter in the tile's label (D-110): in 3 dB steps, so it doesn't change with
/// every frame, and "silent" below −60 dB.
fn spoken_level(db: f32) -> String {
    if db < -60.0 {
        "level silent".into()
    } else {
        format!("level {} dB", ((db / 3.0).round() * 3.0) as i32).replace('-', "−")
    }
}

fn spoken_secs(s: f64) -> String {
    if s < 60.0 {
        format!("{s:.1} seconds")
    } else {
        format_clock(s, false)
    }
}

/// A set in the sidebar, for a screen reader: "Game Night, set 1 of 2, 14 tiles, playing".
pub(super) fn set_label(name: &str, index: usize, sets: usize, tiles: usize, playing: bool, locked: bool) -> String {
    let mut out = format!("{name}, set {} of {sets}, {tiles} tile{}", index + 1, if tiles == 1 { "" } else { "s" });
    if playing {
        out.push_str(", playing");
    }
    if locked {
        out.push_str(", locked");
    }
    out
}

impl StingerApp {
    /// A tile or cue row, for a screen reader, live: "Air Horn, tile 5, key 5, hold,
    /// playing, 1.2 seconds left". A cue row starts with its number and adds whether it
    /// has played (its armed mark), its notes and link; a search hit names its set; a grid tile in Rows mode its `row`
    /// ("tile 5, row 2"; the number stays the one pads, OSC and the cue list use).
    pub(super) fn tile_label(&self, tile: &Tile, index: usize, set: Option<&str>, cue: Option<bool>, row: Option<usize>) -> String {
        let title = if tile.title.trim().is_empty() { "Untitled" } else { tile.title.as_str() };
        let mut parts = vec![match cue {
            Some(_) => format!("Cue {}, {title}", index + 1),
            None => format!("{title}, tile {}", index + 1),
        }];
        if let Some(row) = row {
            parts.push(format!("row {}", row + 1));
        }
        if let Some(set) = set {
            parts.push(format!("in {set}"));
        }
        if let Some(k) = tile.hotkey {
            parts.push(format!("key {}", k.spoken()));
        }
        // What the badges and the Solo border show (D-145).
        match &tile.action {
            Some(a) => parts.push(a.kind.label().to_owned()),
            None => parts.extend(super::pad_options::option_words(tile).into_iter().map(String::from)),
        }
        if tile.vamp.is_some() {
            parts.push("vamp".into());
        }
        if tile.disabled {
            parts.push("disabled".into());
        }
        if cue == Some(true) {
            parts.push("next cue".into());
        }
        if cue.is_some() && self.ctl.played.contains(&tile.id) {
            parts.push("played".into());
        }
        let status = self.ctl.statuses.get(&tile.id);
        if let Some(left) = self.ctl.waiting(tile.id) {
            parts.push(format!("waiting, starts in {}", spoken_secs(left)));
        } else if let Some(r) = self.ctl.recording_view().filter(|r| r.tile == tile.id) {
            parts.push(match r.input {
                Some(input) => format!("recording from {input}, {}", spoken_secs(r.seconds)),
                None => "recording, waiting for the input".into(),
            });
        } else if let Some(s) = status {
            parts.push(match s.state {
                PlayState::Paused => format!("paused, {} left", spoken_secs(s.remaining())),
                PlayState::Stopping => "fading out".into(),
                _ if s.vamping => format!("playing, vamping, the next press plays on, {} left", spoken_secs(s.remaining())),
                _ if s.played_on => format!("playing on, {} left", spoken_secs(s.remaining())),
                _ => format!("playing, {} left", spoken_secs(s.remaining())),
            });
            if let Some(b) = self.tile_meter(tile.id).filter(|_| s.state == PlayState::Playing) {
                parts.push(spoken_level(b.level));
            }
        } else if tile.action.is_none() {
            match self.ctl.store.media(tile) {
                None => parts.push("no sound".into()),
                Some(Media::Loading) => parts.push("loading".into()),
                Some(Media::Failed(_)) => parts.push("can't be read".into()),
                Some(Media::Ready { audio, .. }) => {
                    let (s, e) = play_range(tile, audio.duration());
                    parts.push(spoken_secs((e - s) / tile.speed.clamp(0.5, 2.0) as f64));
                }
            }
        }
        if cue.is_some() {
            let notes = tile.notes.trim();
            if !notes.is_empty() {
                parts.push(format!("notes: {}", notes.chars().take(160).collect::<String>()));
            }
            if let Some(link) = link_text(tile, self.ctl.linking(tile.id)) {
                parts.push(link);
            }
        }
        parts.join(", ")
    }

    /// The transport counter, for a screen reader: "Air Horn, playing, 1.2 seconds left,
    /// 2 playing", or "Nothing playing".
    pub(super) fn counter_label(&self) -> String {
        let rec = self.ctl.recording_view().map(|r| format!("{}, {}", super::rec_title(&r.title), spoken_secs(r.seconds)));
        let Some((id, s)) = self.focus_voice() else { return rec.unwrap_or_else(|| "Nothing playing".into()) };
        let title = self.ctl.store.library.tile(id).map_or("Untitled", |t| t.title.as_str());
        let state = if s.state == PlayState::Paused { "paused" } else { "playing" };
        let mut out = format!("{title}, {state}, {} left", spoken_secs(s.remaining()));
        let sounding = self.ctl.statuses.values().filter(|s| s.state != PlayState::Paused).count();
        if sounding > 1 {
            out.push_str(&format!(", {sounding} playing"));
        }
        if let Some(rec) = rec {
            out.push_str(&format!(", {rec}"));
        }
        out
    }

    /// The master meter, for a screen reader: (label, value in dB, value as words).
    pub(super) fn master_info(&self) -> (f64, String) {
        let v = self.ctl.store.library.master_volume;
        let db = if v <= 0.0 { -80.0 } else { gain_to_db(v) as f64 };
        let level = self.meter.ch.iter().map(|b| b.level).fold(METER_FLOOR_DB, f32::max);
        let mut text = if v <= 0.0 { "silent".to_owned() } else { format!("{db:+.1} dB").replace('-', "−") };
        if level > METER_FLOOR_DB + 0.5 {
            text.push_str(&format!(", output level {:.0} dB", level).replace('-', "−"));
        }
        if self.meter.clip {
            text.push_str(", clipped");
        }
        (db, text)
    }

    /// A screen reader's +/− on the master meter: 3 dB a step, as ⌘/Ctrl+Up and Down.
    pub(super) fn master_steps(&mut self, steps: i32) {
        for _ in 0..steps.unsigned_abs() {
            self.run(Invocation::new(if steps > 0 { "master.up" } else { "master.down" }, json!({})));
        }
    }
}

/// Left/Right (or Down/Up) on a focused custom slider: −1, +1 or 0 steps.
pub(super) fn arrow_steps(resp: &egui::Response) -> i32 {
    if !resp.has_focus() {
        return 0;
    }
    theme::lock_arrows(resp);
    resp.ctx.input_mut(|i| {
        let up = i.consume_key(Modifiers::NONE, Key::ArrowRight) as i32;
        let down = i.consume_key(Modifiers::NONE, Key::ArrowLeft) as i32;
        up - down
    })
}

/// Why the column count is off in Auto columns.
pub(super) const AUTO_COLUMNS: &str = "Auto columns fit every tile in the window by themselves (Settings > Grid layout)";

/// A Grid layout choice in Settings: what it does, and how it differs (D-114).
pub(super) fn grid_mode(mode: GridMode) -> (&'static str, &'static str) {
    match mode {
        GridMode::Rows => ("Each row holds the tiles you put in it.", "Drag a sound into a row and its tiles narrow to fit; drop between rows to start a new one."),
        GridMode::Auto => ("One column count for the whole grid, as many as show every tile without scrolling.", "All tiles narrow together as sounds are added."),
        GridMode::Columns => ("The set's column count, wrapping onto new rows.", "Fewer columns when tiles would get too narrow."),
    }
}
