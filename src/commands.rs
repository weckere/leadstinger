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

const BASE: Spec = Spec { id: "", label: "", main: None, params: "", kind: Kind::Query, destructive: false, shortcut: None };

/// Tile refs: a UUID, a title, `Set/Title`, `key:q` (hotkey in the current set) or `#3`
/// (position in the current set, from 1). Set refs: a UUID, a name or `#2`.
pub static COMMANDS: &[Spec] = &[
    // ── status ──────────────────────────────────────────────────────────────
    spec!("status", "Show what's playing", Query, None,
        "→ {playing: [{tile, title, set, state, position, remaining, length}], master, master_db, device, sample_rate, set, cue, edit_mode, theme}"),
    spec!("library.get", "Show the whole library", Query, None, "→ the library as stored in Library.json"),
    spec!("library.path", "Show where the library is", Query, None, "→ {library, data_dir}"),
    // ── transport ───────────────────────────────────────────────────────────
    spec!("tile.trigger", "Trigger a tile", Live, Some("tile"),
        "{tile, event?: press|release (default press)}  as its hotkey would, following its play mode"),
    spec!("tile.play", "Play a tile", Live, Some("tile"), "{tile, restart? (default false: a playing tile keeps going)}"),
    spec!("tile.stop", "Stop a tile", Live, Some("tile"), "{tile, fade? (seconds; default the tile's fade-out)}"),
    spec!("tile.pause", "Pause a tile", Live, Some("tile"), "{tile}"),
    spec!("tile.resume", "Resume a paused tile", Live, Some("tile"), "{tile}"),
    spec!("tile.preview", "Preview a tile", Live, Some("tile"), "{tile}  play it, or stop it quickly if it's sounding (the context menu's Play/Stop)"),
    spec!("transport.stopAll", "Stop All", Live, Some("immediate"),
        "{immediate? (default false), fade? (seconds; default the Stop All fade setting)}", shortcut = sc(CMD, Key::Period)),
    spec!("cue.go", "Play Next Cue", Live, Some("set"), "{set? (default current)}  the GO button: play the cue under the pointer and move it on"),
    spec!("cue.set", "Set the next cue", Live, Some("index"), "{index (1-based position), set?}"),
    spec!("cue.top", "Back to Top of Cue List", Live, Some("set"), "{set?}"),
    // ── master ──────────────────────────────────────────────────────────────
    spec!("master.volume", "Set the master volume", Library, Some("volume"), "{volume (0-1) | db (≤ 0)}"),
    spec!("master.up", "Master Volume Up", Library, None, "{}  +3 dB", shortcut = sc(CMD, Key::ArrowUp)),
    spec!("master.down", "Master Volume Down", Library, None, "{}  −3 dB; below −60 dB goes silent", shortcut = sc(CMD, Key::ArrowDown)),
    spec!("meter.clearClip", "Clear the clip light", Live, None, "{}"),
    // ── sets ────────────────────────────────────────────────────────────────
    spec!("set.list", "List sets", Query, None, "→ [{id, name, tiles, layout, columns, auto_advance, current}]"),
    spec!("set.create", "New Set", Library, Some("name"), "{name? (default \"Set N\"; the app then lets you type one)}  → {id}",
        shortcut = sc(CMD_SHIFT, Key::N)),
    spec!("set.rename", "Rename Set", Library, Some("name"), "{name? (without it the app edits the name in place), set? (default current)}"),
    spec!("set.duplicate", "Duplicate Set", Library, Some("set"), "{set?}  → {id}"),
    spec!("set.delete", "Delete Set", Library, Some("set"), "{set?}  the last set can't be deleted", destructive = true),
    spec!("set.select", "Select a set", Library, Some("set"), "{set}"),
    spec!("set.layout", "Set the layout", Library, Some("layout"), "{layout: grid|list, set?}"),
    spec!("set.columns", "Set the grid columns", Library, Some("columns"), "{columns (1-12), set?}"),
    spec!("set.autoAdvance", "Auto-advance cues", Library, Some("on"), "{on? (default: toggle), set?}"),
    spec!("set.reassignKeys", "Reassign hotkeys by position", Library, Some("set"), "{set?}  1–0, Q–P, A–;, Z–/ in tile order"),
    // ── tiles ───────────────────────────────────────────────────────────────
    spec!("tile.list", "List tiles", Query, Some("set"), "{set? (default current; \"*\" for all), query? (title contains)}  → [{id, title, set, position, key, mode, duration, ...}]"),
    spec!("tile.get", "Show a tile", Query, Some("tile"), "{tile}  → every setting of the tile"),
    spec!("tile.add", "Add Sounds…", Library, Some("paths"),
        "{paths? ([file] or file; without it the app opens a file picker), set?, at? (1-based position)}  → {added: [id], errors}",
        shortcut = sc(CMD, Key::O)),
    spec!("tile.paste", "Paste Sounds", Live, None, "{}  import files copied in a file manager (also ⌘/Ctrl+V outside text fields)"),
    spec!("tile.replaceAudio", "Replace Audio…", Library, Some("path"), "{tile, path? (without it the app opens a file picker)}"),
    spec!("tile.duplicate", "Duplicate tile", Library, Some("tile"), "{tile}  → {id}"),
    spec!("tile.remove", "Remove tile", Library, Some("tile"), "{tile}", destructive = true),
    spec!("tile.move", "Move tile", Library, Some("tile"), "{tile, to (1-based position in its set)}"),
    spec!("tile.set", "Change tile settings", Library, Some("tile"),
        "{tile, title?, color? (red|orange|yellow|lime|green|mint|teal|cyan|blue|indigo|purple|pink|rose|crimson|brown|gray), hotkey? (one key, or null), mode? (toggle|restart|hold|pause), volume? (0-1), fadeIn?, fadeOut? (s), loop?, solo?, trimStart?, trimEnd? (s; null = to the end), pictureLayer? (underlay|overlay), pictureFit? (fill|fit), pictureOpacity? (0-1)}"),
    spec!("tile.picture", "Choose Picture…", Library, Some("path"), "{tile, path? (an image; without it the app opens a file picker)}"),
    spec!("tile.pictureClear", "Remove Picture", Library, Some("tile"), "{tile}"),
    // ── view ────────────────────────────────────────────────────────────────
    spec!("view.perform", "Perform Mode", Live, None, "{}  clicking a tile plays it"),
    spec!("view.edit", "Edit Mode", Live, None, "{}  clicking a tile selects it; drag to reorder"),
    spec!("view.toggleEdit", "Toggle Edit Mode", Live, None, "{}", shortcut = sc(CMD, Key::E)),
    spec!("view.select", "Select a tile", Live, Some("tile"), "{tile? (none closes the inspector)}"),
    spec!("view.waveforms", "Show Waveforms", Library, Some("on"), "{on? (default: toggle)}"),
    spec!("view.theme", "Theme", Library, Some("theme"), "{theme: console|panel}"),
    spec!("view.appearance", "Appearance", Library, Some("appearance"), "{appearance: dark|light|system}  (Console theme)"),
    spec!("view.settings", "Settings…", Live, Some("open"), "{open? (default true)}", shortcut = sc(CMD, Key::Comma)),
    spec!("view.shortcuts", "Keyboard Shortcuts", Live, None, "{}"),
    spec!("view.about", "About", Live, None, "{}"),
    spec!("view.screenshot", "Save a screenshot", Live, Some("path"), "{path (a .png)}  the next frame of the window"),
    spec!("app.quit", "Quit", Live, None, "{}  saves first", shortcut = sc(CMD, Key::Q)),
    // ── settings ────────────────────────────────────────────────────────────
    spec!("device.list", "List output devices", Live, None, "→ {devices: [name], current}"),
    spec!("device.set", "Choose the output device", Library, Some("name"), "{name (null = system default)}"),
    spec!("device.reconnect", "Reconnect the output device", Live, None, "{}"),
    spec!("settings.stopAllFade", "Set the Stop All fade", Library, Some("seconds"), "{seconds (0-5)}"),
    spec!("settings.windowSystem", "Choose the window system (Linux)", Library, Some("x11"), "{x11: bool}  applies at the next launch"),
    spec!("library.reveal", "Show Library Folder", Live, None, "{}  opens the data folder in the file manager"),
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
    }
}
