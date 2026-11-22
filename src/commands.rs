//! The command registry: one entry for everything the app can do (docs/CLI.md).
//!
//! The menus, the keyboard shortcuts, the Keyboard Shortcuts window, the GUI's widgets
//! and (from M7) the CLI (`leadstinger-cli`) all name commands by these ids, and the
//! [`Controller`](crate::controller::Controller) executes them. A command is called
//! with a JSON object of parameters; `main` names the one a menu item or a positional
//! CLI argument fills in.

use eframe::egui::{Key, KeyboardShortcut, Modifiers};
use serde_json::{json, Map, Value};

/// What a command needs in order to run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Reads state; changes nothing.
    Query,
    /// Changes the library; works without the app running (offline CLI).
    Library,
    /// Needs the running app: playback, voices, cue pointers or the window.
    Live,
}

#[derive(Debug)]
pub struct Spec {
    pub id: &'static str,
    /// The menu label, and the CLI's one-line description.
    pub label: &'static str,
    /// The parameter a menu item or the CLI's positional argument fills in.
    pub main: Option<&'static str>,
    /// Figcraft-style parameter summary, shown by `commands` and in docs/CLI.md.
    pub params: &'static str,
    pub kind: Kind,
    /// Deletes something; refused by `leadstinger-cli --safe`.
    pub destructive: bool,
    /// Changes the library's sets and tiles, so the app records an undo step for it.
    /// (Settings, the master volume, the view and playback aren't undone.)
    pub undo: bool,
    pub shortcut: Option<KeyboardShortcut>,
}

const CMD: Modifiers = Modifiers::COMMAND;
const CMD_SHIFT: Modifiers = Modifiers { shift: true, ..Modifiers::COMMAND };

const fn sc(m: Modifiers, k: Key) -> Option<KeyboardShortcut> {
    Some(KeyboardShortcut::new(m, k))
}

macro_rules! spec {
    ($id:literal, $label:literal, $kind:ident, $main:expr, $params:literal $(, $field:ident = $value:expr)* $(,)?) => {
        Spec {
            id: $id,
            label: $label,
            main: $main,
            params: $params,
            kind: Kind::$kind,
            $($field: $value,)*
            ..BASE
        }
    };
}

const BASE: Spec = Spec { id: "", label: "", main: None, params: "", kind: Kind::Query, destructive: false, undo: false, shortcut: None };

/// Tile refs: a UUID, a title, `Set/Title`, `key:q` or `key:alt+q` (hotkey in the current
/// set, or in any set with settings.tileKeysAllSets) or `#3`
/// (position in the current set, from 1). Set refs: a UUID, a name or `#2`.
pub static COMMANDS: &[Spec] = &[
    // ── status ──────────────────────────────────────────────────────────────
    spec!("status", "Show what's playing", Query, None,
        "→ {playing: [{tile, title, set, state, position, remaining, length, level_db (the tile's meter)}], master, master_db, device, sample_rate, set, cue, input_note (why the input is closed, or that it's still opening), input (the input recording and ducking use), hears_board (the input follows the board, D-127), recording, recording_seconds, locked (the current set), sidebar, inspector, inspector_tab, theme}"),
    spec!("library.get", "Show the whole library", Query, None, "→ the library as stored in Library.json"),
    spec!("library.path", "Show where the library is", Query, None, "→ {library, data_dir}"),
    spec!("batch.run", "Run a script", Library, None,
        "{steps ([{command, params?}], at most 2000), atomic? (default true: when a step fails, the sets and tiles go back as they were and nothing is recorded), dryRun? (default false: true runs the steps, reports what they would change and then puts everything back; only queries and changes that undo can take back are allowed)}  runs the steps one after another with nothing else acting in between, as one undo step (Script); as with undo, settings, the view and playback aren't put back, and a background job a step starts (an export, a transcription, an import) isn't waited for → {ok, steps: [{step, command, ok, result | error}], failed (the step that failed, or null), rolledBack, dryRun, undo (Script, or null when nothing changed or it was put back), changes: {sets: {added, removed, changed: [{set, name, fields}]}, tiles: {added, removed, changed: [{tile, title, set, fields}]}}}"),
    // ── transport ───────────────────────────────────────────────────────────
    spec!("tile.trigger", "Trigger a tile", Live, Some("tile"),
        "{tile, event?: press|release (default press), toggle? (true: a press while it plays stops it even when it's One-shot, as Enter on a focused pad does)}  as its hotkey would, following its options (hold, pausable, oneShot)"),
    spec!("tile.play", "Play a tile", Live, Some("tile"), "{tile, restart? (default false: a playing tile keeps going)}"),
    spec!("tile.stop", "Stop a tile", Live, Some("tile"), "{tile, fade? (seconds; default the tile's fade-out)}"),
    spec!("tile.jump", "Jump to a marker", Live, Some("tile"),
        "{tile, marker (name or #n)}  a playing tile moves there with a short crossfade (past the vamp, it plays on); a stopped one starts there → {tile, marker, at}"),
    spec!("tile.markerAdd", "Add a marker", Library, Some("tile"),
        "{tile, at? (seconds into the file; default: where it's playing now), name? (default \"Marker N\")}", undo = true),
    spec!("tile.markerMove", "Move or rename a marker", Library, Some("tile"),
        "{tile, marker (name or #n), at? (seconds into the file), name? (a new name)}", undo = true),
    spec!("tile.subtitles", "Show a tile's subtitles", Query, Some("tile"),
        "{tile}  its subtitle lines (QuipClipper's cues), in seconds into its audio file whatever its trim → {tile, title, cues: [{start, end, text, words? ([{start, end, text}]), aligned? (true: the words were placed by hand), added? (true: the line was added in the editor)}], cues_source ({kind, lang?, ...} or null), heard (how many lines overlap the trim), trim_start, trim_end}"),
    spec!("tile.subtitlesSet", "Set a tile's subtitles", Library, Some("tile"),
        "{tile, cues ([{start, end, text, words?, aligned?, added?}] as tile.subtitles gives them, in seconds into the audio file), cues_source? ({kind, lang?, ...}), keepAligned? (default true: a line aligned by hand whose text matches a new line overlapping it stays as it is)}  lines that can't be read are skipped (at most 2000 lines of 500 characters, 200 words each) → {tile, lines, skipped, skipped_words, kept_aligned}", undo = true),
    // Transcription (D-277 to D-285): only when asked, one tile at a time in the
    // background; each tile's result is one undo step of its own, recorded when it lands.
    spec!("tile.transcribe", "Transcribe", Library, Some("tile"),
        "{tile | tiles (default the selection), engine? (auto|whisper|quip|apple; default settings.transcribe's), lang? (a language code, e.g. en; default the setting, else the engine's own), gaps? (default true: also add what's heard between a tile's subtitle lines)}  listens to each tile's whole sound: a tile without subtitles (or with only machine-made ones) gets the lines heard, marked machine-made {kind: machine, engine, model, lang}; a tile with subtitles keeps its text, and its lines without word timings get them from what was heard. With gaps, speech heard where none of its lines is spoken (a stretch of 0.5 s or more; a line's first or last word heard up to 0.25 s outside it is still that line's) becomes new lines, each marked machine: {kind: machine, engine, model, lang}, never over a known line; a later run replaces the gap lines it added, except ones changed by hand since. Each word's start is then moved onto the sound it starts on (up to 0.15 s); lines aligned by hand stay as they are. Runs in the background, one tile at a time; the CLI waits for it. With quip, the sound is sent to the connected QuipClipper (settings.transcribe's quipServer, else the first by address that takes uploads), which keeps none of it. Refused for a locked set, and with code unavailable when no engine can run (the reason says what to install) → {job, tiles, engine, sendsTo (\"QuipClipper at …\" when the audio leaves this computer, else null), skipped: [{tile, title, reason}]}; each tile's result (transcribe.status) has gaps: {added, replaced, kept}"),
    spec!("set.transcribe", "Transcribe Tiles Without Subtitles…", Library, Some("set"),
        "{set? (default current), engine?, lang?, gaps? (default false: also the tiles with subtitles from elsewhere, for the lines heard between theirs), ask? (the window asks first, naming the engine, where the audio is sent and, with a box to tick, how many tiles have subtitles whose gaps it could fill; then → {asked, set, tiles, subtitled, engine, sendsTo})}  tile.transcribe for every tile in the set that has a sound and no subtitles (with gaps, and every one whose subtitles aren't machine-made) → as tile.transcribe"),
    spec!("tile.alignWords", "Align Words", Library, Some("tile"),
        "{tile, line? (from 1: listen to that line, a little either side, and time its words again even if it has some; default every line without word timings), engine?, lang?}  the line's text stays; its words take the times the engine heard them at, the ones it missed are spread between, and Reset puts the old timing back. A line aligned by hand is refused → as tile.transcribe"),
    spec!("transcribe.status", "Show transcriptions", Query, Some("job"),
        "{job? (default every one)}  → {jobs: [{job, total, done, finished, cancelled, current ({tile, title, engine, fraction (0-1, or null while unknown)} or null), queued, results: [{tile, title, engine, model, lang, heard: {lines, words}, replaced, lines, kept_aligned, aligned: {lines, matched, placed, missed}, refined: {words, moved, mean_ms, max_ms}, changed}], errors: [{tile, title, error}]}], busy}"),
    spec!("transcribe.cancel", "Cancel Transcription", Library, Some("job"), "{job? (default every one)}  stops the tile being transcribed and drops the ones waiting; nothing is changed → {cancelled (how many tiles)}"),
    spec!("transcribe.engines", "List transcription engines", Query, None,
        "→ {setting (auto or an engine), uses (the engine tile.transcribe would use, or null), sendsTo (where it sends the audio, or null), reason (why none can run, with what to install), engines: [{engine, label, available, reason, servers? (quip: [{server, name, transcribes, reason, upload: {maxBytes, maxSeconds}, used, why}])}], apple ({permission: not-asked|denied|restricted|allowed (null outside the app), locale, language, onDevice, canAsk} on macOS and iPadOS, else null; asking it never prompts)}  in auto's order: Apple Speech, whisper-cli, QuipClipper (only the ones this version has). Asks each connected QuipClipper whether it transcribes (GET /api/config) if it hasn't been asked yet"),
    spec!("transcribe.allowSpeech", "Allow Speech Recognition…", Live, None,
        "{}  macOS and iPadOS, in the app only: shows the system's Speech Recognition permission prompt if it hasn't been answered (the only thing that ever shows it, D-288), else changes nothing. Recognition stays on the device → {asked, permission (as it was), apple (as transcribe.engines), reason (why Apple Speech still can't run, or null)}"),
    spec!("tile.subtitlesClear", "Remove a tile's subtitles", Library, Some("tile"), "{tile}  → {tile, removed (how many lines)}", undo = true),
    // Aligning subtitle words with the audio in the waveform editor (D-267). Lines and
    // words are numbered from 1 in time order; each returns the line as it is now.
    spec!("tile.subtitleEdge", "Move a subtitle word's edge", Library, Some("tile"),
        "{tile, line (from 1), word (from 1), edge: start|end, at (seconds into the file), split? (default false: a boundary shared with the next or previous word moves that word's edge too; true leaves it)}  words keep their order and at least 20 ms; marks the line aligned → {tile, line, start, end, text, aligned, provisional, words: [{start, end, text}]}", undo = true),
    spec!("tile.subtitleNudge", "Nudge a subtitle word", Library, Some("tile"),
        "{tile, line, word, grip: start|end|word, by (seconds; the editor's arrow keys send 0.01, with Shift 0.1)}  moves its start edge, its end edge (a shared boundary moves the neighbour's edge too) or the whole word → the line", undo = true),
    spec!("tile.subtitleWordMove", "Move a subtitle word", Library, Some("tile"),
        "{tile, line, word, at (its new start) | by (seconds), split?}  keeps its length; shared boundaries take the neighbours' edges along (unless split), gaps stop it → the line", undo = true),
    spec!("tile.subtitleTap", "Start a subtitle word here", Library, Some("tile"),
        "{tile, line, word, at (seconds into the file, e.g. where it's playing)}  the word's start moves there, or the whole word when that's past its end: tapping a line in while it plays → the line", undo = true),
    spec!("tile.subtitleLineMove", "Move a subtitle line", Library, Some("tile"),
        "{tile, line, at (its new start) | by (seconds)}  all its words with it, inside the audio → the line (and its new number: lines stay in time order)", undo = true),
    spec!("tile.subtitleAutoAlign", "Auto-align a subtitle line", Library, Some("tile"),
        "{tile, line}  a suggestion: each word starts where a sound rises inside the line (the first and strongest rises when there are more than words; the even split when there are fewer); not marked aligned → the line, method: onsets|strongest|even", undo = true),
    spec!("tile.subtitleReset", "Reset a subtitle line", Library, Some("tile"),
        "{tile, line}  its start, end and words back as the source gave them (or as they were before its first edit here) → the line, reset (false: it was never changed)", undo = true),
    spec!("tile.subtitleWordText", "Retype a subtitle word", Library, Some("tile"),
        "{tile, line, word, text}  fixes its spelling; text with spaces splits it, its time shared out by length → the line", undo = true),
    spec!("tile.subtitleWordJoin", "Join two subtitle words", Library, Some("tile"),
        "{tile, line, word}  the word and the next become one, spanning both → the line", undo = true),
    spec!("tile.subtitleText", "Split a subtitle line again", Library, Some("tile"),
        "{tile, line, text? (new text for the line; a line break is \\n)}  drops its word timings, so its words are spread evenly over it until they're aligned again → the line", undo = true),
    spec!("tile.subtitleAdd", "Add a subtitle line", Library, Some("tile"),
        "{tile, text (a line break is \\n), start? (seconds into the file, with end; or give at), end? (seconds into the file), at? (instead of start and end: its start; it lasts about 0.3 s a word, 0.8 to 6 s, stopping before the next line), words? (onsets (default): each starts where a sound rises inside it, as Auto-Align does | even: spread evenly)}  a line made here, marked added: a new copy of the subtitles from their source keeps it. In the open editor it becomes the selected line → the line, method: onsets|strongest|even, lines (how many now)", undo = true),
    spec!("tile.subtitleDelete", "Delete a subtitle line", Library, Some("tile"),
        "{tile, line}  removes it; a new copy from the source brings a line it has back → {tile, line, removed: {start, end, text, added}, lines (how many are left)}", undo = true),
    spec!("tile.markerRemove", "Remove a marker", Library, Some("tile"), "{tile, marker (name or #n) | all: true}", undo = true),
    spec!("tile.playOn", "Play On", Live, Some("tile"),
        "{tile}  a tile looping its vamp section lets it go and plays on to the end (a press on a vamping tile does the same) → {tile, played_on}"),
    spec!("tile.release", "Play On (deprecated)", Live, Some("tile"), "{tile}  deprecated (D-156): the old name of tile.playOn → {tile, played_on, released}"),
    spec!("tile.pause", "Pause a tile", Live, Some("tile"), "{tile}"),
    spec!("tile.resume", "Resume a paused tile", Live, Some("tile"), "{tile}"),
    spec!("tile.random", "Play a Random Tile", Live, Some("set"),
        "{set? (default current; \"*\" for every set), color? (only tiles of this colour)}  every tile in the pool plays once before any repeats, so never the last pick; disabled, empty and action tiles are left out; the Random button (D-171); the status bar says what it played → {tile, title, left}"),
    spec!("tile.preview", "Preview a tile", Live, Some("tile"),
        "{tile, from?, to? (seconds into the file: just that stretch, once, without its vamp, loop, fades or link; the editor's Play Selection)}  play it, or stop it quickly if it's sounding (the context menu's Play/Stop); with Preview to on and its device not open, nothing plays (code preview_unavailable), never on Main"),
    spec!("transport.stopAll", "Stop All", Live, Some("immediate"),
        "{immediate? (default false), fade? (seconds; default the Stop All fade setting)}  without either, a Stop All while the last one is still fading stops at once (Esc's second press); at once is a 0.1 s de-click → {fade, second}", shortcut = sc(CMD, Key::Period)),
    spec!("cue.go", "Play Next Cue", Live, Some("set"), "{set? (default current), tile? (select this cue first)}  in a List set, Play Selection (Enter): play the selected cue and stop the rest of the list (over the cross-fade in Continuous); again while it plays, stop it (pause or resume a Pausable cue); a vamping cue plays on (D-155). In a grid set, the GO button: play the cue under the pointer and move it on; while a tile loops its vamp, let it play on instead → {tile, title, position, action?, played_on?}"),
    spec!("cue.set", "Set the next cue", Live, Some("index"),
        "{index (1-based position) | tile (in its own set), set?}  moves the pointer there without playing → {set, next}"),
    spec!("cue.top", "Back to Top of Cue List", Live, Some("set"), "{set?}"),
    spec!("cue.next", "Skip to Next Cue", Live, Some("set"), "{set?}  move the pointer past the next cue without playing it → {set, next, title}"),
    spec!("cue.prev", "Back to Previous Cue", Live, Some("set"), "{set?}  move the pointer back one cue without playing → {set, next, title}"),
    spec!("cue.reset", "Reset List", Live, Some("set"), "{set?}  arm every cue again and make the first one next; nothing stops → {set, next, title}"),
    spec!("cue.arm", "Arm or Disarm a Cue", Live, Some("tile"), "{tile, armed? (default: toggle)}  a List row's armed marker: an armed cue is played when the list reaches it, a played one is passed over → {tile, title, armed}"),
    spec!("transport.pauseAll", "Pause All", Live, None, "pause everything playing, pausable or not; with nothing playing, resume what's paused → {paused, resumed}"),
    spec!("transport.fadeAll", "Fade All", Live, Some("fade"), "{fade? (seconds, default 2)}  fade everything out and stop it → {fade, second}"),
    // ── edit ────────────────────────────────────────────────────────────────
    spec!("edit.undo", "Undo", Live, None, "{}  the last change to sets or tiles (⌘/Ctrl+Z outside text fields) → {undid}"),
    spec!("edit.redo", "Redo", Live, None, "{}  the last undone change (⌘/Ctrl+Shift+Z) → {redid}"),
    // ── bindings and MIDI ───────────────────────────────────────────────────
    spec!("bind.list", "List bindings", Query, None, "→ [{id, source, what, command, params, value, enabled}]  pads, keys and OSC addresses mapped to commands"),
    spec!("bind.add", "Add a binding", Library, Some("source"),
        "{source (midi:note:36[:ch10][:port], midi:cc:7, midi:program:2, key:Ctrl+Alt+1, osc:/address, or an object), command? (default tile.trigger), tile?, params? (object), value? (the parameter a fader fills, e.g. volume)}  → the binding"),
    spec!("bind.remove", "Remove bindings", Library, Some("id"), "{id | tile | source | all: true}  → {removed}"),
    spec!("bind.enable", "Turn a binding on or off", Library, Some("id"), "{id, on? (default: toggle)}"),
    spec!("bind.learn", "Learn a MIDI binding", Live, Some("tile"),
        "{tile?, command? (default tile.trigger), params?, value?, from? (midi|key; default midi), cancel?}  the next pad or control on a MIDI controller, or the next key combination pressed in the window (within 30 s), becomes the binding; a fader learned for master.volume or tile.set fills volume"),
    spec!("hotkey.status", "Show system-wide keys", Live, None, "→ {available, note, active, failed}  key bindings work from any app on macOS and in X11 sessions; on Wayland use the compositor's binds"),
    spec!("osc.enable", "Turn OSC on or off", Library, Some("on"),
        "{on? (default true), port? (default 9000), lan? (listen to other machines too; default false: this machine only), feedback? (host:port to send state to, or null; see OSC above), levels? (send each playing tile's level with the feedback, ten times a second; default true)}  OSC has no password: keep lan off unless the network is yours"),
    spec!("osc.status", "Show OSC", Live, None, "→ {enabled, listening, error, received, lan, feedback, levels}"),
    spec!("midi.ports", "List MIDI inputs", Live, None, "→ {enabled, ports, connected, error, auto_map, bank, colors}"),
    spec!("midi.enable", "Turn MIDI input on or off", Library, Some("on"), "{on? (default true), ports? (names or parts of names; null = every port)}"),
    spec!("midi.feedback", "Light MIDI pads", Library, Some("on"),
        "{on? (default true), lit? (velocity while the tile plays, 0-127; default 127), unlit? (default 0), channel? (for auto-mapped pads, 1-16), colors? (pads in their tile's colour: launchpad, apc-mini, apc-mini-mk2, a JSON table {idle: {red: 5, ...}, playing?, idle_channel?, playing_channel?, empty?}, or off)}  pads bound to tiles light up while their tile plays (on the controller's matching output)"),
    spec!("midi.autoMap", "Map MIDI notes to tiles", Library, Some("base"),
        "{base (the note that plays tile #1, e.g. 36; null or off to stop), bank? (pads per page for midi.page; 0 = no pages)}  notes without a binding of their own play the current set's tiles by position"),
    spec!("midi.page", "Pad Page", Live, Some("page"), "{page? (from 1) | delta? (+1 or -1)}  which tiles auto-mapped pads play, a bank at a time → {page, pages, bank}"),
    spec!("midi.pageUp", "Next Pad Page", Live, None, "{}  auto-mapped pads play the next bank of tiles → {page, pages, bank}"),
    spec!("midi.pageDown", "Previous Pad Page", Live, None, "{}  → {page, pages, bank}"),
    // ── master ──────────────────────────────────────────────────────────────
    spec!("master.volume", "Set the master volume", Library, Some("volume"), "{volume (0-1) | db (≤ 0)}"),
    spec!("master.up", "Master Volume Up", Library, None, "{}  +3 dB", shortcut = sc(CMD, Key::ArrowUp)),
    spec!("master.down", "Master Volume Down", Library, None, "{}  −3 dB; below −60 dB goes silent", shortcut = sc(CMD, Key::ArrowDown)),
    spec!("record.start", "Record New Tile", Live, Some("tile"),
        "{tile? (record into this tile, replacing its audio), set?, title?}  records from the input (Settings ▸ Voice chooses it) into a new tile at the end of the set, or into the tile → {tile, title, recording, waiting (the input is still opening: the recording starts when it does)}", undo = true),
    spec!("record.stop", "Stop Recording", Live, None, "{}  the recording becomes the tile's audio (a WAV in the library), playable at once → {tile, title, seconds}", undo = true),
    spec!("record.toggle", "Record / Stop", Live, Some("tile"), "{tile?, set?, title?}  record.start, or record.stop while recording: one pad or key for both", undo = true),
    spec!("replay.enable", "Instant replay", Library, Some("seconds"),
        "{seconds? (5-300, default 30; 0 or off turns it off)}  keep the last seconds of the input, ready for replay.save → {seconds, error}"),
    spec!("replay.save", "Save Instant Replay", Live, Some("seconds"),
        "{seconds? (default: all that's kept), title?, set?}  the last seconds of the input as a new tile, playable at once → {tile, title, seconds}", undo = true),
    spec!("record.status", "Show the recording", Query, None, "→ {recording, tile?, title?, seconds?}"),
    spec!("duck.voice", "Duck under my voice", Library, Some("on"),
        "{on? (default true), threshold? (dBFS, -80 to 0; default -40)}  beds dip while the microphone is above the threshold → {on, threshold, error, waiting}"),
    spec!("input.list", "List inputs", Live, None, "→ {devices, default (the system default input's name), current, level_db, error, waiting (the input is still opening; on macOS, maybe asking for the microphone)}"),
    spec!("duck.hold", "Duck Beds (Talk-Over)", Live, Some("on"),
        "{on? (default: toggle)}  hold every ducked tile down, as a press/release pair from a key bind or pad → {on}"),
    spec!("meter.clearClip", "Clear the clip light", Live, None, "{}"),
    // ── sets ────────────────────────────────────────────────────────────────
    spec!("set.list", "List sets", Query, None, "→ [{id, name, tiles, layout, grid (Settings > Grid layout), columns, rows (tiles per grid row, top to bottom), auto_advance, current}]"),
    spec!("set.create", "New Set", Library, Some("name"), "{name? (default \"Set N\"; the app then lets you type one)}  → {id}",
        shortcut = sc(CMD_SHIFT, Key::N), undo = true),
    spec!("set.rename", "Rename Set", Library, Some("name"), "{name? (without it the app edits the name in place), set? (default current)}", undo = true),
    spec!("set.duplicate", "Duplicate Set", Library, Some("set"), "{set?}  → {id}", undo = true),
    spec!("set.delete", "Delete Set", Library, Some("set"), "{set?}  the last set can't be deleted", destructive = true, undo = true),
    spec!("set.select", "Select a set", Library, Some("set"), "{set}"),
    spec!("set.next", "Next Set", Library, None, "{}  the set below the current one in the sidebar (stops at the last) → {id, name}",
        shortcut = sc(CMD, Key::CloseBracket)),
    spec!("set.prev", "Previous Set", Library, None, "{}  the set above the current one (stops at the first) → {id, name}",
        shortcut = sc(CMD, Key::OpenBracket)),
    spec!("set.layout", "Set the layout", Library, Some("layout"), "{layout: grid|list, set?}"),
    spec!("set.columns", "Set the grid columns", Library, Some("columns"), "{columns (1-12), set?}  Fixed columns: the column count; Rows: the row size (how wide tiles get, and how many new sounds fill a row)"),
    spec!("set.rows", "Even out rows", Library, Some("perRow"),
        "{perRow (1-12) | rows ([3, 8, …] or \"3,8\": tiles per row, top to bottom; tiles left over fill rows of the set's columns), set?}  perRow evens every row out and sets columns; both clear the blank slots → {rows}",
        undo = true),
    spec!("set.addBlank", "Insert a blank slot", Library, Some("row"),
        "{row, slot (1-based: the blank goes before this slot; slots + 1 for the row's end), count? (default 1), set?}  an empty slot in a grid row, which stays put as tiles come and go around it; reading order skips it → {slots: [\"xx.x\", …] (x a tile, . a blank)}",
        undo = true),
    spec!("set.removeBlank", "Remove a blank slot", Library, Some("row"),
        "{row, slot (1-based), set?}  the slots after it move up one → {slots}", undo = true),
    spec!("set.listMode", "List Playback Mode", Library, Some("mode"), "{mode: continuous|queued, set?}  Continuous: a cue that ends on its own starts the next armed one; Queued: it waits for GO / Enter → {mode}"),
    spec!("set.crossfade", "List Cross-Fade", Library, Some("seconds"), "{seconds (0–5), set?}  Continuous: how long the next cue overlaps the end of the one before, one fading out as the other fades in (at most half of either) → {seconds}"),
    spec!("set.autoAdvance", "Auto-advance cues", Library, Some("on"), "{on? (default: toggle), set?}  the same switch as set.listMode (on: continuous)"),
    spec!("set.notes", "Set notes", Library, Some("notes"), "{notes (text; empty clears), set?}  shown above the cue list", undo = true),
    spec!("set.lock", "Lock Set", Library, Some("set"),
        "{set? (default current), on? (default true)}  a locked set can't be changed: its tiles, their order and its settings stay as they are (every change is refused with code locked) and its tiles still play → {set, locked}", undo = true),
    spec!("set.unlock", "Unlock Set", Library, Some("set"), "{set? (default current)}  → {set, locked}", undo = true),
    spec!("set.reassignKeys", "Reassign hotkeys by row", Library, Some("set"), "{set?}  drops the keys set by hand: every pad takes its slot's key again (row 1: 1–=, row 2: q–], row 3: a–', row 4: z–/, rows 5–8: the same with alt+)", undo = true),
    spec!("set.import", "Import Set…", Library, Some("path"),
        "{path? (a .lsset set file, a .zip, a pack folder, or a Farrago export: a .farragoset folder or its zip; without it the app opens a file picker), ask? (the window asks first, as for a set file opened from the file manager; then → {asked, path}), raise? (with ask: bring the window to the front), useTheme? (a set file carrying a theme: true uses it now, false doesn't; without it the window asks)}  adds its sets after yours, and installs the theme it carries as a user theme, replacing one of the same name → {sets: [{id, name, tiles}], theme (null, or {id, name, base, path, replaced, used, asked, warnings}, or {error}), farrago? (a Farrago export: {imported: [title], skipped: [{title, reason}], dropped: [setting (n tiles)], warnings})}; missing or unreadable sounds are skipped, not fatal", undo = true),
    spec!("set.export", "Export Set…", Library, Some("path"),
        "{path? (a .lsset file to write; without it the app asks where), set? (default current), all? (every set)}  clips, pictures and every setting in one file, and the user theme in use → {path, sets, tiles, bytes, skipped, theme (its id, or null)}"),
    // ── rendered export (D-077, D-078–D-080) ────────────────────────────────
    spec!("tile.render", "Export Audio…", Query, Some("path"),
        "{tile? | tiles? (default the selection), path? (the file to write, or a folder for several tiles; without it the app opens the Export Audio window), format? (wav|flac; default the path's extension, else wav), rate? (Hz, 8000-192000; default the clip's)}  the tile with trim, fades, volume, gain, speed, pitch and channel mix applied, once through (24-bit, stereo); runs in the background and the CLI waits for it; the library is untouched → {job, format, files, skipped}"),
    spec!("set.render", "Export Set Audio…", Query, Some("folder"),
        "{folder? (made if missing; without it the app opens the Export Audio window), set? (default current), format?, rate?}  every tile with audio, as \"NN Title.wav\" in set order, never over an existing file → {job, format, files, skipped}"),
    spec!("render.status", "Show audio exports", Query, Some("job"),
        "{job? (default every one)}  → {renders: [{job, format, files, done, total, fraction, current, finished, cancelled, written, errors}]}"),
    spec!("render.cancel", "Cancel Export", Live, Some("job"), "{job? (default every one under way)}  stops rendering; a half-written file is removed → {cancelled}"),
    // ── tiles ───────────────────────────────────────────────────────────────
    spec!("tile.list", "List tiles", Query, Some("set"), "{set? (default current; \"*\" for all), query? (the words a search finds, as tile.search matches them)}  → [{id, title, set, position, row, slot (counting blanks; column is the same), key, loop, solo, pausable, one_shot, hold, disabled, duration, ...}]"),
    spec!("tile.search", "Search tiles", Query, Some("query"),
        "{query (words; every one must be found, in any order, ignoring case and accents), set? (default every set, the current one first)}  searches each tile's title, the name of the file it was made from, its notes and its subtitles → [{tile, title, set, position, matched: [{field: title|file|notes|subtitles, text (the line that matched)}]}]"),
    spec!("tile.get", "Show a tile", Query, Some("tile"), "{tile}  → every setting of the tile"),
    spec!("tile.add", "Add Sounds…", Library, Some("paths"),
        "{paths? ([file] or file; without it the app opens a file picker), set?, at? (1-based position, joining the row of the tile there) | row, slot? (that slot of the row: a blank one fills, one past the row's end opens blanks up to it, otherwise the slots from there move along) | at? (the gap before this slot; default the row's end), newRow? (true: a new row at row)}  → {added: [id], errors}; files are copied in (AC-3, DTS and video converted with ffmpeg) in the background",
        shortcut = sc(CMD, Key::O), undo = true),
    spec!("tile.paste", "Paste Sounds", Live, None, "{}  import files copied in a file manager (also ⌘/Ctrl+V outside text fields)", undo = true),
    spec!("tile.replaceAudio", "Replace Audio…", Library, Some("path"), "{tile, path? (without it the app opens a file picker)}", undo = true),
    spec!("import.cancel", "Cancel Import", Live, None,
        "{}  stop the copies and ffmpeg conversions under way; their tiles stay, marked cancelled  → {cancelled}"),
    spec!("tile.duplicate", "Duplicate tile", Library, Some("tile"), "{tile}  → {id}", undo = true),
    spec!("tile.remove", "Remove tile", Library, Some("tile"), "{tile}", destructive = true, undo = true),
    spec!("tile.move", "Move tile", Library, Some("tile"), "{tile | tiles ([tile], all from one set: they move together as one run in the set's order, one undo step; what dragging a selected tile does), to (1-based position in its set; it joins the row of the tile it displaces; with tiles, where the first ends up) | row (1-based, as the rows are before the move), slot? (the slot it ends up in: a blank one fills, one past the row's end opens blanks up to it, otherwise the slots from there move along) | at? (the gap it goes into, numbered with it already out: where it ends up; default the row's end), newRow? (true: a new row of its own at row; rows + 1 for the end), leaveBlank? (true: a blank stays in its old slot, the slots keep the numbers they have now and nothing else moves; what dragging a tile onto a blank of its own row or past the row's end does), set? (another set to move it into, at its end unless to or row says where; what dropping on a set in the sidebar does)}  otherwise its old row closes up; a row it leaves with no tiles goes; never into or out of a locked set → {position, row, slot, at, set? (when it changed sets)}, or with tiles {set, tiles: [{tile, position, row, slot}]}", undo = true),
    spec!("tile.copy", "Copy tiles", Library, Some("tile"), "{tile | tiles ([tile], all from one set: copied as one run in the set's order, one undo step), to (1-based position in the set it goes into; the copies join the row of the tile there) | row (1-based, as the rows are now: the originals stay), slot? (the slot the first copy goes into: a blank one fills, one past the row's end opens blanks up to it, otherwise the slots from there move along) | at? (the gap they go into; default the row's end), newRow? (true: a new row of their own at row; rows + 1 for the end), set? (another set to copy them into, at its end unless to or row says where; what Alt-dropping on a set in the sidebar does)}  full copies (settings, colour, picture, options) that leave the originals where they are; a hand-set key goes with a copy only into another set where no tile has it; what Alt-dragging tiles does; never into or out of a locked set → {set, tiles: [{tile (the copy), copyOf, position, row, slot}]}", undo = true),
    spec!("tile.set", "Change tile settings", Library, Some("tile"),
        "{tile | tiles ([tile]: change them all as one undo step; not hotkey), title?, emoji? (an emoji or a few, drawn before the title on the pad; emoji only; \"\" or null clears it), color? (red|orange|yellow|lime|green|mint|teal|cyan|blue|indigo|purple|pink|rose|crimson|brown|gray), hotkey? (a key on the keyboard's four rows, 1–= q–] a–' z–/, or with alt+ (Option on macOS) e.g. alt+1; set by hand, it stays with the tile wherever it moves; null: no key; auto: its slot's key again, which follows it), hold? (plays while held, stops on release; with pausable, pauses), pausable? (a press while it plays pauses it, the next resumes), oneShot? (a press while it plays starts it again from the beginning; pausable and oneShot exclude each other: turning one on turns the other off), mode? (deprecated alias for the three: toggle = none, pause = pausable, restart = oneShot, hold = hold), volume? (0-1), gainDb? (-24 to +24), output? (an output's name, or main: where it plays first), sends? ([{output, gainDb?}] or \"Main,Stream\": every output it plays on, each at its own level; null = Main only), channels? (files with more than two channels: auto (the stereo downmix), or one route per channel in file order, as \"left,right,both:-3,mute,left:-3,right:-3\" or [{to: left|right|both|mute, gainDb?}]), speed? (0.5-2; the pitch follows), pitch? (semitones, -12 to +12, keeping the speed; rendered once in the background), duck? (none|ducks|ducked), duckLevel? (a bed's own duck level, dB below its level, -40 to 0; default or null: the library's duck amount), fadeIn?, fadeOut? (s: the fade at the end of the clip), stopFade? (s: the fade when it's stopped; 0 = at once, with the 0.1 s de-click every stop gets), preWait? (s between triggering it and its start), link? (none|continue|follow: what starts the next cue in the set without GO: continue = postWait s after this one starts, follow = postWait s after it ends), postWait? (s), fadeCurve? (gentle|linear|equal_power|exponential|s_curve), loop?, solo?, trimStart?, trimEnd? (s; null = to the end), vampStart?, vampEnd? (s, inside the trim: a section that loops until the next press, GO or tile.playOn lets it play on), vamp? (null clears it), pictureLayer? (underlay|overlay), pictureFit? (fill|fit), pictureOpacity? (0-1), onStart?, onStop? (an OSC or MIDI message, or a list of them, sent when the tile starts or stops: osc://host:port/address args, midi:note:60:ch1:vel100 @port, midi:noteoff:60, midi:cc:20:val127, midi:program:5; null clears), notes? (text shown in the cue list), disabled? (triggers and GO skip it), muted? (plays at zero gain on every send, keeping its volume, timing, cues, ducking and Solo)}", undo = true),
    spec!("tile.enable", "Enable tile", Library, Some("tile"), "{tile}  triggers, bindings and GO play it again", undo = true),
    spec!("tile.disable", "Disable tile", Library, Some("tile"),
        "{tile}  triggers, bindings and GO skip it (a playing tile plays on; Preview still works); it draws dimmed", undo = true),
    spec!("tile.addAction", "New Action Tile", Library, Some("kind"),
        "{kind? (play|fade|stop|pause|play_on; release is play_on's old name; default fade), targets? ([tile, or \"set:Name\" for every tile in a set; add @seconds for a group's offset, e.g. \"Thunder@1.5\"]), fade? (seconds, 0-60; default 3), title?, set?, at? (1-based position)}  a tile with no audio that acts on other tiles when triggered or GO'd, as theatre fade and stop cues do; kind play makes a group that fires its targets, each after its offset → {id}",
        undo = true),
    spec!("tile.action", "Change a tile's action", Library, Some("tile"),
        "{tile, kind? (play|fade|stop|pause|play_on), targets? ([tile | \"set:Name\", each with @seconds for a group's offset]), fade? (seconds), clear? (make it an empty tile again)}  only for tiles without audio", undo = true),
    spec!("tile.normalize", "Normalise", Library, Some("tile"),
        "{tile? (default: every tile in the set), set?, target? (loudness|peak; default loudness), level? (default −16 LUFS or −1 dBFS)}  sets gainDb so the trimmed clip reaches the level → {target, tiles: [{tile, title, measured, gain_db}], skipped}", undo = true),
    spec!("tile.picture", "Choose Picture…", Library, Some("path"), "{tile, path? (an image; without it the app opens a file picker)}", undo = true),
    spec!("tile.pictureClear", "Remove Picture", Library, Some("tile"), "{tile}", undo = true),
    // ── view ────────────────────────────────────────────────────────────────
    spec!("view.sidebar", "Setlist", Library, Some("open"), "{open? (default: toggle)}  show or hide the setlist (the sets sidebar) → {open}", shortcut = sc(CMD, Key::B)),
    spec!("view.inspector", "Inspector", Library, Some("open"),
        "{open? (default: toggle, or true with tab), tab? (play|sound|cue|control: the tile inspector's tab)}  show or hide the inspector: the selected tiles' settings, or with none selected the current set's → {open, tab}",
        shortcut = sc(CMD, Key::E)),
    spec!("view.perform", "Perform Mode (deprecated)", Live, None, "{}  deprecated (D-147): there are no modes; hides the inspector and deselects → {open}"),
    spec!("view.edit", "Edit Mode (deprecated)", Live, None, "{}  deprecated (D-147): shows the inspector → {open}"),
    spec!("view.toggleEdit", "Toggle Edit Mode (deprecated)", Live, None, "{}  deprecated (D-147): view.inspector → {open}"),
    spec!("view.lockEdit", "Lock Edit Mode (deprecated)", Library, Some("on"), "{on? (default: toggle)}  deprecated (D-147): locks or unlocks the current set (set.lock) → {set, locked}"),
    spec!("view.select", "Select a tile", Live, Some("tile"),
        "{tile? (none closes the inspector), add? (⌘-click: add it to the selection, or take it out), remove? (Alt-click: take it out of the selection; an unselected tile leaves the selection as it is), range? (Shift-click: every tile in the rectangle of slots, rows by columns, between the selected one and it, in its set; with add, ⌘-Shift-click: add the rectangle to the selection), columns? (with range: lay the set out in lines of this many instead of its rows, as Auto/Fixed columns show it; 1 = the run in set order), tiles? ([tile]: select these)}  several selected tiles edit together in the inspector → {selected}"),
    spec!("view.search", "Find…", Live, Some("query"),
        "{query? (the search box's text; the board dims the tiles it doesn't find), clear? (empty the box; close is the old name), go? (go to hit #index: its set, the tile selected, its pad focused; the box empties), play? (play hit #index, or the first that can play; the box keeps its text), index? (from 1, default 1)}  without parameters it puts the keyboard in the search box → {open, query, hits: [{tile, title, set, position, matched: [{field, text}]}], went?, played?}",
        shortcut = sc(CMD, Key::F)),
    spec!("view.editor", "Edit Waveform", Live, Some("tile"),
        "{tile? (default: the selected tile), open? (default: true with tile, else toggle)}  a full-window editor for the tile's trim, vamp and markers, in place of the board; selecting another tile closes it → {open, tile}",
        shortcut = sc(CMD_SHIFT, Key::E)),
    spec!("view.editorWord", "Select a Subtitle Word", Live, None,
        "{line? (with word), word? (from 1), grip? (start|end|word: what the arrow keys move; default start, or the grip already chosen; given alone, the selected word's), clear? (true: select no word)}  the word selected in the waveform editor's subtitle lane; with nothing, it reports → {tile, line, word, grip, text, start, end} | {tile, selected: null}"),
    spec!("view.editorSelect", "Select in the Editor", Live, None,
        "{from, to (seconds into the file) | clear: true}  the waveform editor's selection (dragging across the waveform), which Trim to Selection (tile.set trimStart/trimEnd), Vamp Selection (vampStart/vampEnd), Play Selection (tile.preview from/to) and view.editorZoom selection act on; with neither, it reports it; closing the editor clears it → {tile, selection: {from, to} | null}"),
    spec!("view.editorZoom", "Zoom the Editor", Live, None,
        "{fit: true | trim: true | selection: true | from, to (seconds into the file) | by (2 zooms in, 0.5 out, around the cursor or the middle)}  what the waveform editor shows; trim and selection leave a little room either side → {tile, fit | from, to | by}"),
    spec!("view.waveforms", "Show Waveforms", Library, Some("on"), "{on? (default: toggle)}"),
    spec!("view.meters", "Show Level Meters", Library, Some("on"), "{on? (default: toggle)}"),
    spec!("view.theme", "Theme", Library, Some("theme"), "{theme: console|panel}  the built-in themes; theme.use picks any theme"),
    spec!("theme.list", "List themes", Live, None,
        "→ {themes: [{id, name, base (console|panel), builtin, path (an installed theme's folder; edit its files there and the window reloads them), current}], current, warnings (where the theme in use is below WCAG AA)}"),
    spec!("theme.use", "Use a theme", Library, Some("theme"),
        "{theme (an id or name from theme.list; console and panel are the built-ins)}  View ▸ Theme lists the installed ones too → {theme, name, base, warnings}"),
    spec!("theme.import", "Import Theme…", Library, Some("path"),
        "{path? (a .lstheme folder or zip, or its theme.json; without it the app opens a file picker), use? (default true)}  copies it into the data folder's themes, replacing one with the same name; a theme below WCAG AA is still imported, with warnings → {theme: {id, name, base, path}, replaced, used, warnings}"),
    spec!("theme.export", "Export Theme…", Live, Some("path"),
        "{path? (ending .lstheme or .zip for a zip, else a folder; without it the app asks where), theme? (default: the one in use)}  a built-in is written as theme.json and tokens.json, a FigCraft variables export to start a theme from → {path}"),
    spec!("theme.remove", "Remove a theme", Library, Some("theme"), "{theme}  an installed theme; the built-in it was based on takes over if it was in use → {removed, current}"),
    spec!("view.appearance", "Appearance", Library, Some("appearance"),
        "{appearance: dark|light|system|high_contrast|standard}  dark, light and system are the Console theme's; high_contrast applies to both themes; standard is the theme's own look (after high_contrast, dark; otherwise no change)"),
    spec!("view.zoom", "Zoom", Library, Some("zoom"),
        "{zoom: in|out|reset or a percent (50–300)}  in and out step through 50, 67, 75, 80, 90, 100, 110, 125, 150, 175, 200, 250, 300 → {zoom}"),
    spec!("view.settings", "Settings…", Live, Some("open"), "{open? (default true)}", shortcut = sc(CMD, Key::Comma)),
    spec!("view.shortcuts", "Keyboard Shortcuts", Live, None, "{}"),
    spec!("view.quip", "Browse QuipClipper…", Live, Some("open"),
        "{open? (default: toggle, or open when anything else is given), server? (a connected one), folder?, query?, new? (true: list the clips new since last time, on server or the first with any, and count them as seen)}  the QuipClipper window: a connected server's clips library, to preview and add from (quip.browse, quip.preview, quip.add) → {open, server?, new?}"),
    spec!("view.about", "About", Live, None, "{}"),
    spec!("app.checkForUpdates", "Check for Updates…", Query, None,
        "{again? (default true: ask GitHub now; false: only the last answer), show? (default true: the window shows the answer, as the menu item does)}  asks GitHub for the project's latest published release (drafts and pre-releases never count) and compares it with this build; nothing is downloaded. Returns at once with {checking: true}; the CLI waits for the answer → {checking, current, status: newer|latest|failed, latest?, url? (the release page), notes?, error?}"),
    spec!("view.screenshot", "Save a screenshot", Live, Some("path"),
        "{path (a .png), window? (a set open in a window of its own: that window instead of the main one)}  the window once it has settled; the file appears a moment later (up to a second when the window is hidden)"),
    // ── windows ─────────────────────────────────────────────────────────────
    spec!("window.open", "Open Set in New Window", Live, Some("set"),
        "{set? (default current), x?, y? (where its top-left corner goes, in the desktop's points), width?, height? (its inside)}  the set's board in a window of its own, to put on another monitor or workspace; one already open is raised. It stays open across launches, where it was (on Wayland the compositor places it) → {set, name, opened (false: it was open already)}"),
    spec!("window.close", "Close Set Window", Live, Some("set"), "{set? (default current) | all: true}  the main window stays → {closed: [name]}"),
    spec!("window.list", "List windows", Live, None,
        "→ {windows: [{window (main, or the set's id), set?, name, title, x?, y? (absent where the compositor keeps positions to itself), width?, height?, focused}]}"),
    spec!("window.focus", "Bring a window to the front", Live, Some("set"), "{set? (none: the main window)}  raises it and gives it the keyboard → {window}"),
    spec!("window.move", "Move or resize a window", Live, Some("set"),
        "{set? (none: the main window), x?, y?, width?, height?}  in the desktop's points; Wayland compositors ignore positions → {window}"),
    spec!("window.gather", "Gather Windows", Live, None, "{}  every set window back beside the main window, one below another: the way back to a window left on a monitor that's gone → {moved}"),
    spec!("app.quit", "Quit", Live, None, "{}  saves first", shortcut = sc(CMD, Key::Q)),
    // ── settings ────────────────────────────────────────────────────────────
    spec!("device.list", "List output devices", Live, None, "→ {devices: [name], current}"),
    spec!("device.set", "Choose the output device", Library, Some("name"), "{name (null = system default)}"),
    spec!("output.list", "List outputs", Live, None, "→ {outputs: [{name, device, channel, open}], errors, preview: {on, device, open}, devices}"),
    spec!("output.add", "Add an output", Library, Some("name"),
        "{name? (default \"Output N\"), device? (null = system default), channel? (first of its pair, from 1; default 1)}  tiles can then be routed to it with tile.set output"),
    spec!("output.set", "Change an output", Library, Some("name"), "{name, device?, channel?, rename?}"),
    spec!("output.remove", "Remove an output", Library, Some("name"), "{name}  sends to it go; a tile with none left plays on Main"),
    spec!("tile.send", "Also send to an output", Library, Some("tile"),
        "{tile, output (an output's name, or main), gainDb? (-24 to +24, on top of the tile's level; default 0)}  adds the send, or changes its level; the tile plays there too from its next start → the tile", undo = true),
    spec!("tile.unsend", "Stop sending to an output", Library, Some("tile"), "{tile, output}  a tile with no sends left plays on Main", undo = true),
    spec!("device.reconnect", "Reconnect the output device", Live, None, "{}"),
    spec!("settings.stopAllFade", "Set the Stop All fade", Library, Some("seconds"), "{seconds (0-5)}  Esc's first press fades over it; 0 (the default) stops at once (a 0.1 s de-click)"),
    spec!("settings.checkUpdates", "Check for updates at launch", Library, Some("on"), "{on? (default: toggle)}  off by default: then the app never contacts GitHub unless app.checkForUpdates is run; on, it checks once at launch and speaks up only about a newer release → {on}"),
    spec!("settings.stopAllHold", "Hold Esc for Stop All", Library, Some("on"), "{on? (default: toggle)}  Esc must be held for 0.3 s before it stops everything (Shift+Esc and the CLI don't wait)"),
    spec!("settings.inputDevice", "Choose the input (microphone)", Library, Some("name"), "{name (null = system default)}"),
    spec!("settings.previewOutput", "Preview to another output", Library, Some("on"),
        "{on? (default true), device? (null = system default)}  the inspector's Preview and tile.preview play there (headphones) and leave the cue pointer alone; while that device isn't open, previews play nowhere"),
    spec!("settings.duck", "Set the ducking", Library, Some("amount"),
        "{amount? (dB below the bed's level, -40 to 0; default -12), attack? (s, 0-2), release? (s, 0-5)}"),
    spec!("settings.gridLayout", "Grid Layout", Library, Some("mode"),
        "{mode: rows|auto|columns}  rows: each row holds the tiles put in it and a fuller row's tiles narrow (the default); auto: as many columns as show every tile without scrolling; columns: each set's column count, wrapping"),
    spec!("settings.speakCues", "Speak Cue Names", Library, Some("on"),
        "{on? (default: toggle)}  the system voice says a tile's title when it starts (macOS say, on the Preview output's device when Preview to another output is on; Linux spd-say, speech-dispatcher's own output); off by default → {on, voice, routed}"),
    spec!("settings.coloredPads", "Coloured pads", Library, Some("on"),
        "{on? (default: toggle)}  Console pads in their tile's colour all over; off (the default), a pad has a dark body with its colour on the title, the waveform and the border. Panel's blocks don't change → {on}"),
    spec!("settings.tileKeysAllSets", "Tile keys work in every set", Library, Some("on"),
        "{on? (default: toggle)}  a tile's key plays it from any set: the current set's tile first, then the other sets in sidebar order → {on}"),
    spec!("settings.quipBadge", "Show new QuipClipper clips", Library, Some("on"),
        "{on? (default: toggle)}  the status bar shows how many clips are new on the connected QuipClipper servers, asking them every minute while the QuipClipper window is closed; on by default (status.quip_new has the counts) → {on}"),
    spec!("settings.quipAskEachImport", "Ask before each QuipClipper import", Library, Some("on"),
        "{on? (default: toggle)}  a QuipClipper link the system opens asks \"Import …?\" before importing, even from a connected server (links from a server that isn't connected always ask); off by default → {on}"),
    spec!("settings.transcribe", "Transcription settings", Library, None,
        "{engine? (auto|whisper|quip|apple), lang? (a language code like en; auto or \"\" for the engine's own), whisperModel? (a ggml model file for whisper-cli, e.g. ggml-base.en.bin; \"\" clears it; LEADSTINGER_WHISPER_MODEL wins), quipServer? (a connected QuipClipper to transcribe on; \"\" for the first, by address, that takes uploads)}  the app downloads no model and needs none until you transcribe → {engine, lang, whisperModel, quipServer}"),
    spec!("settings.windowSystem", "Choose the window system (Linux)", Library, Some("x11"), "{x11: bool}  applies at the next launch"),
    spec!("library.reveal", "Show Library Folder", Live, None, "{}  opens the data folder in the file manager"),
    // ── QuipClipper (docs/QUIPCLIPPER.md) ───────────────────────────────────
    spec!("quip.connect", "Connect to QuipClipper", Library, Some("url"),
        "{url (the server's base URL, e.g. https://quip.example), token? (its API token, QC_API_TOKEN; - reads it from stdin; none: a server with no password, asked first and sent no credentials), check? (default true: try the token on the server first)}  keeps the token in quip-servers.json in the data folder, readable only by you, never in the library or set files; the server's links then open → {server, checked}"),
    spec!("quip.disconnect", "Disconnect from QuipClipper", Library, Some("url"), "{url}  forgets the server's token (a paired one is revoked on the server too); its links are refused again → {server, removed, revoked}"),
    spec!("quip.list", "List QuipClipper servers", Query, None, "→ [{server, added (Unix time), token (has one), paired}]  the connected servers, never their tokens"),
    spec!("quip.pair", "Pair with QuipClipper", Library, Some("url"),
        "{url, cancel? (true: stop waiting), ask? (the window asks first, as a pair link does)}  asks the server for a pairing and returns at once; QuipClipper shows the code with Allow and Deny, and on Allow gives this app a token of its own (revocable there). A server without pairing that has no password connects without a token → {server, status: starting|waiting|connected|denied|expired|cancelled|failed, code?, approve_url?, message?}; the CLI waits for the answer, showing the code"),
    spec!("quip.pairing", "QuipClipper pairings", Query, Some("url"), "{url?}  → [{server, status, code?, approve_url?, expires_in?, token?, message?}]  the pairings started since the app started"),
    spec!("quip.discover", "Find QuipClipper on the network", Library, None,
        "{again? (default true: look again)}  listens a few seconds for QuipClipper's mDNS advertisement (_quipclipper._tcp; QuipClipper's QC_ADVERTISE) and checks each answer is QuipClipper; connected servers aren't listed → {looking, found: [{server, name, address, protected}], error}; the CLI waits for the look to finish"),
    spec!("quip.import", "Import from QuipClipper", Library, Some("link"),
        "{link (a leadstinger://quip?… link, or a clip URL <server>/api/clips/download/<clip>, which may end ?as=flac), title?, start?, end? (s: the tile's trim), set? (by name, created if missing; default current), notes? (the tile's notes), as? (flac: the server converts the clip to FLAC first, every channel kept), raise? (bring the window to the front, as opened links do), ask? (as an opened link: with a window, a server that isn't connected asks to connect first, and with settings.quipAskEachImport every link asks)}  downloads the clip from a connected server in the background (Cancel Import stops it) into a new tile at the end of the set → {tile, title, set, server, clip, trimStart, trimEnd, flac}, or {asked, server, connected} when the window asks; a server that isn't connected is otherwise refused with code not_connected and nothing is fetched", undo = true),
    spec!("quip.browse", "Browse QuipClipper's clips", Query, Some("folder"),
        "{server? (default: the only one connected), folder? (in the clips library; default its top), query? (words every clip must have), kind? (audio|video|gif|set), recursive? (default false: only the folder's own clips), sort? (newest|name; default newest)}  → {server, folder, folders, clips: [{clip, kind, title, source, episode, duration, channels, trim, size, modified, flac}], count, cursor}"),
    spec!("quip.search", "Search QuipClipper's clips", Query, Some("query"),
        "{query, server?, folder? (default: the whole library), kind?}  every clip under the folder with all the words in its line, file name, source, show or episode, newest first → as quip.browse"),
    spec!("quip.new", "New in QuipClipper", Library, Some("server"),
        "{server?, mark? (default false: true counts them as seen)}  the clips added since they were last seen (the cursor is kept beside the token, never in the library); the first look at a server is the baseline and finds none → as quip.browse, plus {since, baseline, marked}"),
    spec!("quip.add", "Add from QuipClipper", Library, Some("clip"),
        "{clip (its path in the clips library), server?, set? (by name, created if missing; default current), title? (default: the line)}  quip.import with the clip's metadata: as FLAC when the server offers it, trimmed to the line when its ends are known, notes saying where it's from; a set file (.lsset) is imported as a set → as quip.import, or {set, downloading} for a set file", undo = true),
    spec!("quip.preview", "Preview a QuipClipper clip", Live, Some("clip"),
        "{clip, server?, stop? (true: stop the preview), title?}  fetches the clip into memory (never saved) and plays it on the Preview output, or on Main when Preview to is off, as tile.preview does (Preview to on with its device not open: refused, code preview_unavailable); again, or another clip, stops it first → {clip, title, loading} or {stopped}"),
];

pub fn spec(id: &str) -> Option<&'static Spec> {
    COMMANDS.iter().find(|s| s.id == id)
}

impl Kind {
    pub fn describe(self) -> &'static str {
        match self {
            Kind::Query => "read-only",
            Kind::Library => "library (works with the app closed)",
            Kind::Live => "running app",
        }
    }
}

impl Spec {
    /// The shortcut as `⌘O / Ctrl+O`, or "".
    pub fn shortcut_text(&self) -> String {
        use eframe::egui::ModifierNames;
        self.shortcut
            .map(|s| format!("{} / {}", s.format(&ModifierNames::SYMBOLS, true), s.format(&ModifierNames::NAMES, false)))
            .unwrap_or_default()
    }
}

/// The area a command belongs to: the part of its id before the dot.
pub fn area(id: &str) -> &str {
    id.split('.').next().unwrap_or(id)
}

/// The commands grouped by area, areas in order of first appearance.
pub fn by_area() -> Vec<(&'static str, Vec<&'static Spec>)> {
    let mut out: Vec<(&'static str, Vec<&'static Spec>)> = Vec::new();
    for s in COMMANDS {
        let a = area(s.id);
        match out.iter_mut().find(|(x, _)| *x == a) {
            Some((_, v)) => v.push(s),
            None => out.push((a, vec![s])),
        }
    }
    out
}

/// docs/CLI.md, generated from the registry (`leadstinger-cli commands --markdown`); a test
/// keeps the checked-in file equal to this.
pub fn markdown() -> String {
    let mut out = String::from(include_str!("../docs/CLI.head.md"));
    for (a, specs) in by_area() {
        out.push_str(&format!("\n### {a}\n\n| Command | What it does | Parameters | Needs |\n|---|---|---|---|\n"));
        for s in specs {
            let mut what = s.label.trim_end_matches('…').to_owned();
            if s.destructive {
                what.push_str(" **(destructive)**");
            }
            let keys = s.shortcut_text();
            if !keys.is_empty() {
                what.push_str(&format!(" · {keys}"));
            }
            let main = s.main.map(|m| format!(" (positional: `{m}`)")).unwrap_or_default();
            out.push_str(&format!(
                "| `{}` | {} | `{}`{} | {} |\n",
                s.id,
                what,
                s.params.replace('|', "\\|"),
                main,
                s.kind.describe()
            ));
        }
    }
    out
}

/// A static argument a menu item or widget passes as the command's main parameter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Arg {
    None,
    Str(&'static str),
    Bool(bool),
    Num(u32),
}

/// `s` kept for the rest of the run, once per distinct string, so a menu item can carry
/// a name only known at run time (an installed theme's id and name) while [`Cmd`] and
/// the menu labels stay `&'static` (D-119). Only for short names that rarely change.
pub fn intern(s: &str) -> &'static str {
    static KEPT: std::sync::Mutex<std::collections::BTreeSet<&'static str>> = std::sync::Mutex::new(std::collections::BTreeSet::new());
    let mut kept = KEPT.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(&k) = kept.get(s) {
        return k;
    }
    let k: &'static str = Box::leak(s.to_owned().into_boxed_str());
    kept.insert(k);
    k
}

/// A command with an optional fixed main argument: what a menu item or shortcut runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Cmd {
    pub id: &'static str,
    pub arg: Arg,
}

impl Cmd {
    pub const fn new(id: &'static str) -> Cmd {
        Cmd { id, arg: Arg::None }
    }

    pub const fn with(id: &'static str, arg: Arg) -> Cmd {
        Cmd { id, arg }
    }

    pub fn spec(&self) -> &'static Spec {
        spec(self.id).unwrap_or_else(|| panic!("unknown command {:?}", self.id))
    }

    /// The parameters this invocation sends.
    pub fn params(&self) -> Value {
        let mut m = Map::new();
        let main = self.spec().main;
        match (self.arg, main) {
            (Arg::None, _) => {}
            (Arg::Str(s), Some(k)) => {
                m.insert(k.into(), json!(s));
            }
            (Arg::Bool(b), Some(k)) => {
                m.insert(k.into(), json!(b));
            }
            (Arg::Num(n), Some(k)) => {
                m.insert(k.into(), json!(n));
            }
            (_, None) => panic!("{} takes no main parameter", self.id),
        }
        Value::Object(m)
    }
}

/// A command to run, with its parameters: what the GUI's widgets queue.
#[derive(Clone, Debug, PartialEq)]
pub struct Invocation {
    pub id: &'static str,
    pub params: Value,
}

impl Invocation {
    pub fn new(id: &'static str, params: Value) -> Invocation {
        debug_assert!(spec(id).is_some(), "unknown command {id:?}");
        Invocation { id, params }
    }
}

impl From<Cmd> for Invocation {
    fn from(c: Cmd) -> Invocation {
        Invocation { id: c.id, params: c.params() }
    }
}

/// Shorthand for `Invocation::new(id, json!(...))`.
#[macro_export]
macro_rules! inv {
    ($id:literal) => {
        $crate::commands::Invocation::new($id, serde_json::json!({}))
    };
    ($id:literal, $($json:tt)+) => {
        $crate::commands::Invocation::new($id, serde_json::json!($($json)+))
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_well_formed() {
        let mut seen = std::collections::HashSet::new();
        for s in COMMANDS {
            assert!(seen.insert(s.id), "duplicate command {}", s.id);
            assert!(s.id.chars().all(|c| c.is_ascii_alphanumeric() || c == '.'), "bad id {}", s.id);
            assert!(!s.label.is_empty() && !s.params.is_empty(), "{} needs a label and params", s.id);
            if let Some(m) = s.main {
                assert!(s.params.contains(m), "{}: main parameter {m} isn't in its params", s.id);
            }
        }
    }

    #[test]
    fn undoable_commands_change_the_library() {
        for s in COMMANDS.iter().filter(|s| s.undo) {
            assert!(s.kind == Kind::Library || s.id == "tile.paste" || s.id.starts_with("record.") || s.id == "replay.save", "{} records undo but doesn't change the library", s.id);
            assert!(!s.id.starts_with("edit.") && !s.id.starts_with("settings.") && !s.id.starts_with("master."), "{}", s.id);
        }
    }

    #[test]
    fn shortcuts_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for s in COMMANDS.iter().filter_map(|s| s.shortcut) {
            assert!(seen.insert(format!("{s:?}")), "shortcut {s:?} used twice");
        }
    }

    #[test]
    fn docs_cli_md_is_up_to_date() {
        let doc = include_str!("../docs/CLI.md");
        assert!(
            doc == markdown(),
            "docs/CLI.md is stale: run `cargo run --bin leadstinger-cli -- commands --markdown > docs/CLI.md`"
        );
    }

    #[test]
    fn menu_args_fill_the_main_parameter() {
        assert_eq!(Cmd::with("view.theme", Arg::Str("panel")).params(), json!({"theme": "panel"}));
        assert_eq!(Cmd::with("transport.stopAll", Arg::Bool(true)).params(), json!({"immediate": true}));
        assert_eq!(Cmd::with("set.columns", Arg::Num(5)).params(), json!({"columns": 5}));
        assert_eq!(Cmd::new("cue.go").params(), json!({}));
        let id = intern(&String::from("harbor"));
        assert!(std::ptr::eq(id, intern("harbor")), "kept once");
        assert_eq!(Cmd::with("theme.use", Arg::Str(id)).params(), json!({"theme": "harbor"}));
    }
}
