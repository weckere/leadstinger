//! One menu model, two renderers (the pattern PhotoCraft uses): the native macOS menu
//! bar (`native_menu.rs`) and an egui menu bar inside the window on Linux and Windows.
//! Keyboard shortcuts come from the same model, so a menu and its key never disagree.
//! Every item runs a registry command ([`crate::commands`]); its label and shortcut come
//! from the registry unless the item gives its own (items that pass an argument).

use crate::commands::{intern, Arg, Cmd};
use crate::model::{Appearance, GridMode, Layout, ThemeKind};
use crate::theme::Tokens;
use eframe::egui::{self, Key, KeyboardShortcut, Modifiers};

/// What the menus need to know to show check marks, disabled items and the installed
/// themes.
#[derive(Clone, Debug, PartialEq)]
pub struct State {
    /// The sets sidebar is shown (`view.sidebar`).
    pub sidebar: bool,
    /// The inspector is shown (`view.inspector`).
    pub inspector: bool,
    /// The current set is locked (`set.lock`): the items that would change it are off.
    pub locked: bool,
    pub layout: Layout,
    /// The current set's tiles per row.
    pub columns: u32,
    /// Settings > Grid layout.
    pub grid: GridMode,
    /// The row size when the current set's rows are even.
    pub even_rows: Option<u32>,
    pub waveforms: bool,
    pub meters: bool,
    pub appearance: Appearance,
    pub theme: ThemeKind,
    /// The id of the theme in use: `console`, `panel` or an installed theme's.
    pub theme_id: String,
    /// The installed themes as (id, name), in the order View ▸ Theme lists them.
    pub user_themes: Vec<(String, String)>,
    pub can_delete_set: bool,
    pub has_next_cue: bool,
    pub anything_playing: bool,
    /// The talk-over is holding ducked tiles down.
    pub duck_hold: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    pub recording: bool,
    /// The input a recording would use, by name (Record New Tile's hover text).
    pub input: String,
    /// Files are being copied or converted into the library.
    pub importing: bool,
    /// Instant replay is keeping input.
    pub replay: bool,
    /// UI zoom in percent.
    pub zoom: u32,
    /// The waveform editor is open (`view.editor`).
    pub editor: bool,
    /// The selected tile has a sound the editor can open.
    pub can_edit_wave: bool,
}

pub struct Item {
    pub cmd: Cmd,
    pub label: &'static str,
    pub shortcut: Option<KeyboardShortcut>,
    /// `Some` for items with a check mark.
    pub checked: Option<bool>,
    pub enabled: bool,
    /// While disabled: what makes it available (its hover text in the in-window menu).
    pub why: &'static str,
    /// Hover text of its own, in place of [`help`]'s (Record New Tile names the input).
    pub tip: Option<String>,
}

pub enum Entry {
    Item(Item),
    Separator,
    Submenu(&'static str, Vec<Entry>),
}

pub struct Menu {
    pub title: &'static str,
    pub entries: Vec<Entry>,
}

const fn sc(modifiers: Modifiers, key: Key) -> Option<KeyboardShortcut> {
    Some(KeyboardShortcut::new(modifiers, key))
}

const CMD: Modifiers = Modifiers::COMMAND;
/// settings.gridLayout's name for a mode.
fn grid_mode_name(m: GridMode) -> &'static str {
    match m {
        GridMode::Rows => "rows",
        GridMode::Auto => "auto",
        GridMode::Columns => "columns",
    }
}

const COLUMN_LABELS: [&str; 12] = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12"];
const CMD_SHIFT: Modifiers = Modifiers { shift: true, ..Modifiers::COMMAND };
/// View ▸ Zoom's presets, in percent, and their labels.
const ZOOM_PRESETS: [(u32, &str); 8] =
    [(50, "50 %"), (75, "75 %"), (100, "100 %"), (125, "125 %"), (150, "150 %"), (200, "200 %"), (250, "250 %"), (300, "300 %")];

/// A plain command, labelled and keyed as the registry says.
fn item(id: &'static str) -> Entry {
    let c = Cmd::new(id);
    let spec = c.spec();
    Entry::Item(Item { cmd: c, label: spec.label, shortcut: spec.shortcut, checked: None, enabled: true, why: "", tip: None })
}

/// A command with an argument, or a label of its own.
fn item_as(cmd: Cmd, label: &'static str, shortcut: Option<KeyboardShortcut>) -> Entry {
    Entry::Item(Item { cmd, label, shortcut, checked: None, enabled: true, why: "", tip: None })
}

fn checked(entry: Entry, on: bool) -> Entry {
    match entry {
        Entry::Item(i) => Entry::Item(Item { checked: Some(on), ..i }),
        e => e,
    }
}

/// Enabled only when `on`; `why` says what makes it available.
fn enabled(entry: Entry, on: bool, why: &'static str) -> Entry {
    match entry {
        Entry::Item(i) => Entry::Item(Item { enabled: on, why, ..i }),
        Entry::Submenu(label, sub) => Entry::Submenu(label, sub.into_iter().map(|e| enabled(e, on, why)).collect()),
        e => e,
    }
}

/// The menus, in order. `mac` puts About, Settings and Quit in the application menu
/// (rendered natively, along with Services, Hide, Window and Full Screen items);
/// elsewhere they live in File and Help.
pub fn menus(s: &State, mac: bool) -> Vec<Menu> {
    let mut out = Vec::new();
    if mac {
        out.push(Menu {
            title: crate::app::APP_NAME,
            entries: vec![item("view.settings"), Entry::Separator, item("app.quit")],
        });
    }
    let open = |e: Entry| enabled(e, !s.locked, LOCKED_WHY);
    let mut file = vec![
        item("set.create"),
        open(item("tile.add")),
        open(item("tile.addAction")),
        open(item_as(Cmd::with("tile.addAction", Arg::Str("play")), "New Group Tile", None)),
        // No accelerator: ⌘/Ctrl+V must keep pasting text into fields. The app catches
        // the shortcut itself when no field has focus.
        open(item("tile.paste")),
        enabled(item("import.cancel"), s.importing, "Nothing is being imported"),
        Entry::Separator,
        item("view.quip"),
        item("set.import"),
        item("set.export"),
        item("tile.render"),
        Entry::Separator,
        open(item("set.rename")),
        item("set.duplicate"),
        // One slot whichever way it goes: the native menu is built once.
        checked(item_as(Cmd::new(if s.locked { "set.unlock" } else { "set.lock" }), "Lock Set", None), s.locked),
        // With no tile, tile.normalize evens out the whole current set.
        open(item_as(Cmd::new("tile.normalize"), "Normalise Set Loudness", None)),
        match (s.can_delete_set, s.locked) {
            (false, _) => enabled(item("set.delete"), false, "The last set can't be deleted"),
            (true, locked) => enabled(item("set.delete"), !locked, LOCKED_WHY),
        },
        Entry::Separator,
        item("library.reveal"),
    ];
    if !mac {
        file.extend([Entry::Separator, item("view.settings"), Entry::Separator, item("app.quit")]);
    }
    out.push(Menu { title: "File", entries: file });
    // No accelerators, like Paste: ⌘/Ctrl+Z must keep undoing typing in text fields. The
    // app catches the keys itself when no field has focus.
    let (undo, redo) = if mac { ("Undo  (⌘Z)", "Redo  (Shift+⌘Z)") } else { ("Undo  (Ctrl+Z)", "Redo  (Ctrl+Shift+Z)") };
    out.push(Menu {
        title: "Edit",
        entries: vec![
            enabled(item_as(Cmd::new("edit.undo"), undo, None), s.can_undo, "Nothing to undo yet"),
            enabled(item_as(Cmd::new("edit.redo"), redo, None), s.can_redo, "Nothing has been undone"),
        ],
    });
    let theme = |name: &'static str, label: &'static str| checked(item_as(Cmd::with("view.theme", Arg::Str(name)), label, None), s.theme_id == name);
    let mut themes = vec![theme("console", "Console"), theme("panel", crate::brand::PANEL_THEME)];
    if !s.user_themes.is_empty() {
        themes.push(Entry::Separator);
        themes.extend(s.user_themes.iter().map(|(id, name)| {
            checked(item_as(Cmd::with("theme.use", Arg::Str(intern(id))), intern(name), None), s.theme_id == *id)
        }));
    }
    themes.extend([Entry::Separator, item("theme.import"), item("theme.export")]);
    // Light/dark applies to Console; PANEL is always on black, so its first item is
    // Standard, the way out of high contrast (t-m22-panel-hc). High contrast is both's.
    let console = s.theme == ThemeKind::Console;
    let look = |a: Appearance, name: &'static str, label| {
        let (name, label, on) = match a {
            Appearance::Dark if !console => ("standard", "Standard", s.appearance != Appearance::HighContrast),
            _ => (name, label, s.appearance == a),
        };
        enabled(
            checked(item_as(Cmd::with("view.appearance", Arg::Str(name)), label, None), on),
            console || a == Appearance::HighContrast || a == Appearance::Dark,
            "Light and dark are for the Console theme; Panel is always dark",
        )
    };
    let mut zoom = vec![
        enabled(item_as(Cmd::with("view.zoom", Arg::Str("in")), "Zoom In", sc(CMD, Key::Equals)), s.zoom < crate::model::ZOOM_MAX, "Already at the largest, 300 %"),
        enabled(item_as(Cmd::with("view.zoom", Arg::Str("out")), "Zoom Out", sc(CMD, Key::Minus)), s.zoom > crate::model::ZOOM_MIN, "Already at the smallest, 50 %"),
        item_as(Cmd::with("view.zoom", Arg::Str("reset")), "Actual Size", sc(CMD, Key::Num0)),
        Entry::Separator,
    ];
    zoom.extend(ZOOM_PRESETS.iter().map(|&(z, label)| checked(item_as(Cmd::with("view.zoom", Arg::Num(z)), label, None), s.zoom == z)));
    out.push(Menu {
        title: "View",
        entries: vec![
            checked(item("view.sidebar"), s.sidebar),
            checked(item("view.inspector"), s.inspector),
            Entry::Separator,
            checked(item_as(Cmd::with("set.layout", Arg::Str("grid")), "Grid", sc(CMD, Key::Num1)), s.layout == Layout::Grid),
            checked(item_as(Cmd::with("set.layout", Arg::Str("list")), "Cue List", sc(CMD, Key::Num2)), s.layout == Layout::List),
            item("set.next"),
            item("set.prev"),
            // Also a toolbar control, but the PANEL toolbar drops it in narrow windows.
            Entry::Submenu(
                "Columns",
                (1..=12u32)
                    .map(|n| {
                        // In Rows mode the count evens the rows out (D-114).
                        let (cmd, on) = match s.grid {
                            GridMode::Rows => ("set.rows", s.even_rows == Some(n)),
                            _ => ("set.columns", s.columns == n),
                        };
                        let entry = checked(item_as(Cmd::with(cmd, Arg::Num(n)), COLUMN_LABELS[n as usize - 1], None), on);
                        match s.grid {
                            GridMode::Auto => enabled(entry, false, "Auto columns fit every tile in the window by themselves (Settings > Grid layout)"),
                            _ if s.locked => enabled(entry, false, LOCKED_WHY),
                            _ => enabled(entry, s.layout == Layout::Grid, "Columns are for the grid; switch to Grid first"),
                        }
                    })
                    .collect(),
            ),
            Entry::Submenu(
                "Grid Layout",
                GridMode::ALL
                    .iter()
                    .map(|&m| checked(item_as(Cmd::with("settings.gridLayout", Arg::Str(grid_mode_name(m))), m.label(), None), s.grid == m))
                    .collect(),
            ),
            Entry::Separator,
            item("view.search"),
            enabled(checked(item("view.editor"), s.editor), s.editor || s.can_edit_wave, "Select a tile with a sound first"),
            checked(item("view.waveforms"), s.waveforms),
            checked(item("view.meters"), s.meters),
            Entry::Submenu("Theme", themes),
            Entry::Submenu(
                "Appearance",
                vec![
                    look(Appearance::Dark, "dark", "Dark"),
                    look(Appearance::Light, "light", "Light"),
                    look(Appearance::System, "system", "Match System"),
                    look(Appearance::HighContrast, "high_contrast", "High Contrast"),
                ],
            ),
            Entry::Submenu("Zoom", zoom),
        ],
    });
    out.push(Menu {
        title: "Playback",
        entries: vec![
            enabled(item_as(Cmd::new("cue.go"), "Play Selection  (Enter)", None), s.has_next_cue && s.layout == Layout::List, CUE_WHY),
            enabled(item_as(Cmd::new("cue.prev"), "Previous Cue  (Up)", None), s.layout == Layout::List, "Only in the cue list (View > Cue List)"),
            enabled(item_as(Cmd::new("cue.next"), "Next Cue  (Down)", None), s.has_next_cue && s.layout == Layout::List, CUE_WHY),
            item("cue.reset"),
            item("tile.random"),
            Entry::Separator,
            enabled(item_as(Cmd::new("transport.stopAll"), "Stop All  (Esc)", sc(CMD, Key::Period)), s.anything_playing, "Nothing is playing"),
            enabled(
                item_as(Cmd::with("transport.stopAll", Arg::Bool(true)), "Stop All Immediately", sc(CMD_SHIFT, Key::Period)),
                s.anything_playing,
                "Nothing is playing",
            ),
            enabled(item("transport.fadeAll"), s.anything_playing, "Nothing is playing"),
            enabled(item_as(Cmd::new("transport.pauseAll"), "Pause All  (Space)", None), s.anything_playing, "Nothing is playing or paused"),
            Entry::Separator,
            // A toggle here; binds and the CLI send on/off as a press/release pair.
            checked(item_as(Cmd::new("duck.hold"), "Duck Beds (Talk-Over)", None), s.duck_hold),
            if s.recording {
                item("record.stop")
            } else {
                match enabled(item("record.start"), !s.locked, LOCKED_WHY) {
                    Entry::Item(i) => Entry::Item(Item { tip: Some(format!("Record a new tile from {} (Settings > Voice chooses the input)", s.input)), ..i }),
                    e => e,
                }
            },
            match (s.replay, s.locked) {
                (false, _) => enabled(item("replay.save"), false, "Turn on instant replay first: Settings > Replay"),
                (true, locked) => enabled(item("replay.save"), !locked, LOCKED_WHY),
            },
            Entry::Separator,
            item("master.up"),
            item("master.down"),
        ],
    });
    let mut help = vec![item("view.shortcuts"), Entry::Separator, item("app.checkForUpdates")];
    if !mac {
        help.extend([Entry::Separator, item("view.about")]);
    }
    out.push(Menu { title: "Help", entries: help });
    out
}

/// Why GO and Skip are unavailable.
const CUE_WHY: &str = "Only in the cue list (View > Cue List), while a cue is left to play";

/// What a menu item does, for its hover text in the in-window menu bar.
/// Play a Random Tile's hover text, the menu item's and the Random button's (D-171).
pub const RANDOM_HELP: &str =
    "Play a random tile from this set: any with a sound that isn't disabled, never the one it played last (each plays once before any repeats)";

pub fn help(cmd: Cmd) -> &'static str {
    match (cmd.id, cmd.arg) {
        ("view.settings", _) => "Open Settings: outputs, keys, MIDI, OSC, guards, theme",
        ("app.quit", _) => "Save the library and quit",
        ("set.create", _) => "Add an empty set below the others",
        ("tile.add", _) => "Choose sound or video files to add as tiles to this set",
        ("view.quip", _) => "Browse a connected QuipClipper's clips: search, preview and add them to this set",
        ("tile.addAction", Arg::Str("play")) => "Add a group tile, which plays several tiles at once (or one after another)",
        ("tile.addAction", _) => "Add a tile that fades, stops, pauses or plays on other tiles",
        ("tile.paste", _) => "Add files copied in a file manager as tiles",
        ("import.cancel", _) => "Stop the files being copied or converted; their tiles stay, marked cancelled",
        ("set.import", _) => "Add a set from a set file someone exported",
        ("set.export", _) => "Save this set, with its sounds, as one file to share or back up",
        ("tile.render", _) => "Write the selected tiles, or the whole set, to WAV or FLAC files with their settings applied",
        ("set.rename", _) => "Rename this set in the setlist",
        ("set.duplicate", _) => "Copy this set, tiles and all",
        ("tile.normalize", _) => "Set every tile's gain so the set plays at an even loudness (−16 LUFS)",
        ("set.delete", _) => "Delete this set and its tiles (Undo brings it back)",
        ("library.reveal", _) => "Open the folder that holds the library and its sounds",
        ("edit.undo", _) => "Undo the last change to sets or tiles",
        ("edit.redo", _) => "Redo the last change undone",
        ("view.sidebar", _) => "Show or hide the setlist: the sets, on the left",
        ("view.inspector", _) => "Show or hide the inspector on the right: the selected tiles' settings, or the set's",
        ("set.lock", _) | ("set.unlock", _) => "Lock this set so nothing in it can be changed by accident; its tiles still play",
        ("set.layout", Arg::Str("grid")) => "Show this set as a grid of tiles",
        ("set.layout", _) => "Show this set as a cue list, played in order with GO",
        ("set.next", _) => "Go to the set below this one in the setlist",
        ("set.prev", _) => "Go to the set above this one in the setlist",
        ("set.columns", _) => "Tiles per row in this set's grid",
        ("set.rows", _) => "Even this set's rows out to this many tiles each (a fuller row's tiles narrow to fit)",
        ("settings.gridLayout", Arg::Str("rows")) => "Each row holds the tiles put in it; a fuller row's tiles narrow to fit",
        ("settings.gridLayout", Arg::Str("auto")) => "As many columns as show every tile without scrolling; all tiles narrow together",
        ("settings.gridLayout", _) => "The set's column count, wrapping onto new rows",
        ("view.search", _) => "Find tiles in every set by title or notes; Enter plays the first",
        ("view.editor", _) => "Edit the selected tile's trim, vamp and markers on a big waveform you can zoom into",
        ("view.waveforms", _) => "Draw each tile's waveform on it",
        ("view.meters", _) => "Draw a level meter beside each playing tile's time",
        ("view.theme", _) => "Change the whole look of the window",
        ("theme.use", _) => "Use this imported theme (theme.remove takes it away again)",
        ("theme.import", _) => "Add a theme: a .lstheme folder or zip, whose tokens.json is a FigCraft variables export",
        ("theme.export", _) => "Save the theme in use as a .lstheme, to start a theme from or to share",
        ("view.appearance", Arg::Str("high_contrast")) => "White on black with outlined controls, in either theme",
        ("view.appearance", Arg::Str("standard")) => "Panel's own colours: pastel blocks on black",
        ("view.appearance", _) => "Light or dark, or follow the system",
        ("view.zoom", Arg::Str("in")) => "Make everything in the window bigger",
        ("view.zoom", Arg::Str("out")) => "Make everything in the window smaller",
        ("view.zoom", Arg::Str("reset")) => "Show everything at its normal size (100 %)",
        ("view.zoom", _) => "Show everything in the window at this size",
        ("cue.go", _) => "Play the selected cue and stop the rest of the list; again while it plays, stop it",
        ("cue.top", _) => "Move the cue pointer back to the first cue, without playing",
        ("cue.reset", _) => "Arm every cue again and select the first; nothing stops",
        ("cue.prev", _) => "Select the cue above, without playing",
        ("cue.next", _) => "Select the cue below, without playing",
        ("transport.pauseAll", _) => "Pause everything that's playing, pausable or not; again to carry on (Space)",
        ("transport.fadeAll", _) => "Fade everything out over 2 seconds",
        ("tile.random", _) => RANDOM_HELP,
        ("transport.stopAll", Arg::Bool(true)) => "Stop everything at once, with no fade",
        ("transport.stopAll", _) => "Stop everything, with the Stop All fade if one is set; again while fading stops at once",
        ("duck.hold", _) => "Hold beds down, as if you were talking over them, until this is turned off",
        ("record.start", _) => "Record a new tile from the input chosen under Voice in Settings",
        ("record.stop", _) => "Stop recording; the recording becomes the tile's sound",
        ("replay.save", _) => "Make a tile from the last seconds of the input",
        ("master.up", _) => "Raise the master volume by 3 dB",
        ("master.down", _) => "Lower the master volume by 3 dB",
        ("view.shortcuts", _) => "List every keyboard shortcut",
        ("view.about", _) => "Version and credits",
        ("app.checkForUpdates", _) => "Ask GitHub whether a newer version is out; nothing is downloaded unless you choose to",
        _ => "",
    }
}

/// Why an item that would change the current set is off while it's locked.
const LOCKED_WHY: &str = "This set is locked: File > Lock Set unlocks it";

/// What each menu and submenu holds, for its hover text.
pub fn menu_help(title: &str) -> &'static str {
    match title {
        "File" => "New sets and tiles, import and export, and the library",
        "Edit" => "Undo and redo changes to sets and tiles",
        "View" => "The setlist and inspector, grid or cue list, search, theme",
        "Playback" => "The cue list, Stop All, talk-over, recording and the master volume",
        "Help" => "Keyboard shortcuts, Check for Updates and About",
        "Columns" => "Tiles per row in this set's grid",
        "Theme" => "Console, Panel or a theme you imported",
        "Appearance" => "Light or dark for the Console theme, or high contrast for either",
        "Zoom" => "Make everything in the window bigger or smaller",
        _ if title == crate::app::APP_NAME => "Settings and Quit",
        _ => "",
    }
}

/// Every (shortcut, command) in the model, most specific first, so Shift+⌘+. is
/// tried before ⌘+. (egui matches modifiers loosely).
pub fn shortcuts(menus: &[Menu]) -> Vec<(KeyboardShortcut, Cmd)> {
    fn walk(entries: &[Entry], out: &mut Vec<(KeyboardShortcut, Cmd)>) {
        for e in entries {
            match e {
                Entry::Item(i) => {
                    if let (Some(s), true) = (i.shortcut, i.enabled) {
                        out.push((s, i.cmd));
                    }
                }
                Entry::Submenu(_, sub) => walk(sub, out),
                Entry::Separator => {}
            }
        }
    }
    let mut out = Vec::new();
    for m in menus {
        walk(&m.entries, &mut out);
    }
    let weight = |m: &Modifiers| m.shift as u8 + m.alt as u8 + m.ctrl as u8;
    out.sort_by_key(|(s, _)| std::cmp::Reverse(weight(&s.modifiers)));
    out
}

/// Every item with its slot: the indexes of the menu and of each entry down to it.
/// The native macOS menu is built once and finds its items again by slot, so it can
/// follow an item whose command and label change with the state (Record New Tile and
/// Stop Recording share one slot).
pub fn slots(menus: &[Menu]) -> Vec<(Vec<usize>, &Item)> {
    fn walk<'a>(entries: &'a [Entry], path: &mut Vec<usize>, out: &mut Vec<(Vec<usize>, &'a Item)>) {
        for (n, e) in entries.iter().enumerate() {
            path.push(n);
            match e {
                Entry::Item(i) => out.push((path.clone(), i)),
                Entry::Submenu(_, sub) => walk(sub, path, out),
                Entry::Separator => {}
            }
            path.pop();
        }
    }
    let mut out = Vec::new();
    for (n, m) in menus.iter().enumerate() {
        walk(&m.entries, &mut vec![n], &mut out);
    }
    out
}

/// What the native menu can't change once built: the menus, and where each separator,
/// submenu, item and check item sits. It follows everything else in place (check marks,
/// enabled states, a plain item's command, label and shortcut), and refills View ▸
/// Theme, whose contents aren't part of the shape.
pub fn shape(menus: &[Menu]) -> Vec<String> {
    fn walk(entries: &[Entry], at: &str, out: &mut Vec<String>) {
        for (n, e) in entries.iter().enumerate() {
            match e {
                Entry::Separator => out.push(format!("{at}/{n} separator")),
                Entry::Item(i) if i.checked.is_some() => out.push(format!("{at}/{n} check item")),
                Entry::Item(_) => out.push(format!("{at}/{n} item")),
                Entry::Submenu(label, sub) => {
                    out.push(format!("{at}/{n} submenu {label}"));
                    if *label != "Theme" {
                        walk(sub, &format!("{at}/{label}"), out);
                    }
                }
            }
        }
    }
    let mut out = Vec::new();
    for m in menus {
        out.push(format!("menu {}", m.title));
        walk(&m.entries, m.title, &mut out);
    }
    out
}

/// Each command's menu label (without hints in parentheses), for the shortcuts window.
pub fn label(menus: &[Menu], cmd: Cmd) -> &'static str {
    fn find(entries: &[Entry], cmd: Cmd) -> Option<&'static str> {
        entries.iter().find_map(|e| match e {
            Entry::Item(i) if i.cmd == cmd => Some(i.label.split("  (").next().unwrap_or(i.label)),
            Entry::Submenu(_, sub) => find(sub, cmd),
            _ => None,
        })
    }
    menus.iter().find_map(|m| find(&m.entries, cmd)).unwrap_or("")
}

/// The in-window menu bar (Linux and Windows). Returns the command picked, if any.
pub fn menu_bar(ui: &mut egui::Ui, menus: &[Menu]) -> Option<Cmd> {
    let mut picked = None;
    egui::MenuBar::new().ui(ui, |ui| {
        for m in menus {
            let r = ui.menu_button(m.title, |ui| {
                ui.set_min_width(220.0);
                if let Some(c) = entries_ui(ui, &m.entries) {
                    picked = Some(c);
                }
            });
            crate::theme::hint(r.response, menu_help(m.title));
        }
    });
    picked
}

fn entries_ui(ui: &mut egui::Ui, entries: &[Entry]) -> Option<Cmd> {
    let t = Tokens::get(ui.ctx());
    let mut picked = None;
    for e in entries {
        match e {
            Entry::Separator => {
                ui.separator();
            }
            Entry::Submenu(label, sub) => {
                let r = ui.menu_button(*label, |ui| {
                    if let Some(c) = entries_ui(ui, sub) {
                        picked = Some(c);
                    }
                });
                crate::theme::hint(r.response, menu_help(label));
            }
            Entry::Item(i) => {
                let mark = match i.checked {
                    Some(true) => "✔  ",
                    Some(false) => "     ",
                    None => "",
                };
                let mut button = egui::Button::new(format!("{mark}{}", i.label)).frame(false);
                if let Some(s) = i.shortcut {
                    button = button.shortcut_text(egui::RichText::new(ui.ctx().format_shortcut(&s)).color(t.text_dim));
                }
                let resp = ui.add_enabled(i.enabled, button);
                // Read without the drawn check mark, as a menu item that's checked or not.
                let role = if i.checked.is_some() { egui::accesskit::Role::MenuItemCheckBox } else { egui::accesskit::Role::MenuItem };
                let resp = crate::theme::role(resp, role, i.label.split("  (").next().unwrap_or(i.label), i.checked);
                let tip = i.tip.as_deref().unwrap_or(help(i.cmd));
                let resp = match i.shortcut {
                    Some(s) => crate::theme::hint_or(resp, format!("{tip} — {}", ui.ctx().format_shortcut(&s)), i.why),
                    None => crate::theme::hint_or(resp, tip, i.why),
                };
                if resp.clicked() {
                    picked = Some(i.cmd);
                }
            }
        }
    }
    picked
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> State {
        State {
            sidebar: true,
            inspector: true,
            locked: false,
            layout: Layout::Grid,
            columns: 4,
            grid: GridMode::Rows,
            even_rows: Some(4),
            waveforms: true,
            meters: true,
            appearance: Appearance::Dark,
            theme: ThemeKind::Console,
            theme_id: "console".into(),
            user_themes: vec![("harbor".into(), "Harbor".into())],
            can_delete_set: true,
            has_next_cue: true,
            anything_playing: true,
            duck_hold: false,
            can_undo: true,
            can_redo: false,
            recording: false,
            input: "Fake Mic".into(),
            importing: false,
            replay: false,
            zoom: 100,
            editor: false,
            can_edit_wave: true,
        }
    }

    fn titles(m: &[Menu]) -> Vec<&str> {
        m.iter().map(|m| m.title).collect()
    }

    #[test]
    fn mac_puts_settings_and_quit_in_the_app_menu() {
        let mac = menus(&state(), true);
        assert_eq!(titles(&mac), [crate::app::APP_NAME, "File", "Edit", "View", "Playback", "Help"]);
        let linux = menus(&state(), false);
        assert_eq!(titles(&linux), ["File", "Edit", "View", "Playback", "Help"]);
        // Both platforms reach every command that has a shortcut, with the same keys.
        let keys = |m: &[Menu]| {
            let mut v: Vec<_> = shortcuts(m).into_iter().map(|(s, c)| (format!("{s:?}"), c)).collect();
            v.sort_by(|a, b| a.0.cmp(&b.0));
            v
        };
        assert_eq!(keys(&mac), keys(&linux));
    }

    /// Help ▸ Check for Updates… on both platforms (D-206).
    #[test]
    fn help_has_check_for_updates() {
        for mac in [true, false] {
            let model = menus(&state(), mac);
            let help = &model.iter().find(|m| m.title == "Help").unwrap().entries;
            let item = help.iter().find_map(|e| match e {
                Entry::Item(i) if i.cmd.id == "app.checkForUpdates" => Some(i),
                _ => None,
            });
            assert_eq!(item.map(|i| i.label), Some("Check for Updates…"), "mac: {mac}");
        }
    }

    #[test]
    fn shortcuts_are_unique_and_specific_first() {
        let list = shortcuts(&menus(&state(), false));
        let mut seen = std::collections::HashSet::new();
        for (s, _) in &list {
            assert!(seen.insert(format!("{s:?}")), "duplicate shortcut {s:?}");
        }
        let pos = |c: Cmd| list.iter().position(|(_, x)| *x == c).unwrap();
        assert!(pos(Cmd::with("transport.stopAll", Arg::Bool(true))) < pos(Cmd::new("transport.stopAll")));
    }

    #[test]
    fn disabled_items_have_no_live_shortcut() {
        let quiet = State { anything_playing: false, ..state() };
        let list = shortcuts(&menus(&quiet, false));
        assert!(!list.iter().any(|(_, c)| c.id == "transport.stopAll"));
    }

    /// Installed themes are listed under the built-ins, checked when in use, and run
    /// theme.use with their id; the built-ins aren't checked then.
    #[test]
    fn view_theme_lists_installed_themes() {
        let s = State {
            theme_id: "harbor".into(),
            user_themes: vec![("harbor".into(), "Harbor".into()), ("night-shift".into(), "Night Shift".into())],
            ..state()
        };
        let model = menus(&s, false);
        let view = &model.iter().find(|m| m.title == "View").unwrap().entries;
        let Some(Entry::Submenu(_, theme)) = view.iter().find(|e| matches!(e, Entry::Submenu("Theme", _))) else { panic!("no Theme menu") };
        let items: Vec<(&str, Option<bool>, Cmd)> =
            theme.iter().filter_map(|e| if let Entry::Item(i) = e { Some((i.label, i.checked, i.cmd)) } else { None }).collect();
        assert_eq!(
            items,
            [
                ("Console", Some(false), Cmd::with("view.theme", Arg::Str("console"))),
                (crate::brand::PANEL_THEME, Some(false), Cmd::with("view.theme", Arg::Str("panel"))),
                ("Harbor", Some(true), Cmd::with("theme.use", Arg::Str("harbor"))),
                ("Night Shift", Some(false), Cmd::with("theme.use", Arg::Str("night-shift"))),
                ("Import Theme…", None, Cmd::new("theme.import")),
                ("Export Theme…", None, Cmd::new("theme.export")),
            ]
        );
        assert_eq!(theme.iter().filter(|e| matches!(e, Entry::Separator)).count(), 2);
        // Without installed themes there's no empty group.
        let plain = menus(&State { user_themes: Vec::new(), ..state() }, false);
        let view = &plain.iter().find(|m| m.title == "View").unwrap().entries;
        let Some(Entry::Submenu(_, theme)) = view.iter().find(|e| matches!(e, Entry::Submenu("Theme", _))) else { panic!() };
        assert_eq!(theme.len(), 5);
    }

    /// The native macOS menu is built once, so no state may add, remove or move an
    /// entry, or turn an item into a check item; it can only change what sits in a slot
    /// (t-m20-record-menu: Playback kept saying Record New Tile while recording).
    #[test]
    fn the_menu_shape_never_depends_on_the_state() {
        let base = state();
        let mut states = vec![
            State { sidebar: false, inspector: false, ..base.clone() },
            State { locked: true, ..base.clone() },
            State { locked: true, can_delete_set: false, ..base.clone() },
            State { layout: Layout::List, ..base.clone() },
            State { columns: 12, ..base.clone() },
            State { even_rows: None, ..base.clone() },
            State { waveforms: false, meters: false, ..base.clone() },
            State { theme: ThemeKind::Panel, theme_id: "panel".into(), ..base.clone() },
            State { theme_id: "harbor".into(), ..base.clone() },
            State { user_themes: Vec::new(), ..base.clone() },
            State { can_delete_set: false, has_next_cue: false, anything_playing: false, ..base.clone() },
            State { duck_hold: true, can_undo: false, can_redo: true, ..base.clone() },
            State { recording: true, ..base.clone() },
            State { input: "USB Mic".into(), ..base.clone() },
            State { importing: true, replay: true, ..base.clone() },
            State { zoom: crate::model::ZOOM_MIN, ..base.clone() },
            State { zoom: crate::model::ZOOM_MAX, ..base.clone() },
            State { editor: true, can_edit_wave: false, ..base.clone() },
        ];
        states.extend(GridMode::ALL.iter().map(|&grid| State { grid, ..base.clone() }));
        states.extend([Appearance::Light, Appearance::System, Appearance::HighContrast].map(|appearance| State { appearance, ..base.clone() }));
        for mac in [true, false] {
            let want = shape(&menus(&base, mac));
            for s in &states {
                assert_eq!(shape(&menus(s, mac)), want, "the menu changes shape in {s:?}");
            }
        }
    }

    /// In Panel, View ▸ Appearance's first item is Standard, enabled and checked unless
    /// High Contrast is on, so high contrast can be left without going back to Console.
    #[test]
    fn panel_can_leave_high_contrast_from_the_menu() {
        let look = |s: &State| {
            let model = menus(s, true);
            slots(&model).into_iter().filter(|(_, i)| i.cmd.id == "view.appearance").map(|(_, i)| (i.label, i.enabled, i.checked)).collect::<Vec<_>>()
        };
        let panel = State { theme: ThemeKind::Panel, theme_id: "panel".into(), ..state() };
        let hc = State { appearance: Appearance::HighContrast, ..panel.clone() };
        assert_eq!(look(&hc)[0], ("Standard", true, Some(false)));
        assert_eq!(look(&hc)[3], ("High Contrast", true, Some(true)));
        assert_eq!(look(&panel)[0], ("Standard", true, Some(true)));
        assert!(!look(&panel)[1].1 && !look(&panel)[2].1, "Light and System stay Console's");
        assert_eq!(look(&state())[0], ("Dark", true, Some(true)), "Console keeps Dark");
    }

    /// Playback's record item is Stop Recording while recording, in the slot Record New
    /// Tile had, which is where the native menu looks for it.
    #[test]
    fn stop_recording_takes_record_new_tiles_slot() {
        let find = |s: &State, id: &str| {
            let model = menus(s, true);
            slots(&model).into_iter().find(|(_, i)| i.cmd.id == id).map(|(at, i)| (at, i.label))
        };
        let (idle_at, idle) = find(&state(), "record.start").expect("Record New Tile");
        let (busy_at, busy) = find(&State { recording: true, ..state() }, "record.stop").expect("Stop Recording");
        assert_eq!((idle, busy), ("Record New Tile", "Stop Recording"));
        assert_eq!(idle_at, busy_at);
        assert!(find(&State { recording: true, ..state() }, "record.start").is_none());
    }

    /// Every item runs a registered command, with its argument in the main parameter,
    /// and a plain item's shortcut is the one the registry gives it.
    #[test]
    fn every_menu_item_is_a_registry_command() {
        fn walk(entries: &[Entry], n: &mut usize) {
            for e in entries {
                match e {
                    Entry::Item(i) => {
                        let spec = crate::commands::spec(i.cmd.id).unwrap_or_else(|| panic!("{} isn't registered", i.cmd.id));
                        let _ = i.cmd.params(); // panics if the argument has nowhere to go
                        assert!(!help(i.cmd).is_empty(), "{} has no hover text", i.cmd.id);
                        assert!(i.enabled || !i.why.is_empty(), "{} is disabled without a reason", i.cmd.id);
                        if i.cmd.arg == Arg::None && i.label == spec.label {
                            assert_eq!(i.shortcut, spec.shortcut, "{}", i.cmd.id);
                        }
                        *n += 1;
                    }
                    Entry::Submenu(_, sub) => walk(sub, n),
                    Entry::Separator => {}
                }
            }
        }
        for mac in [true, false] {
            let mut n = 0;
            for m in menus(&state(), mac) {
                walk(&m.entries, &mut n);
            }
            assert!(n > 20);
        }
    }
}
