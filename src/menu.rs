//! One menu model, two renderers (the pattern PhotoCraft uses): the native macOS menu
//! bar (`native_menu.rs`) and an egui menu bar inside the window on Linux and Windows.
//! Keyboard shortcuts come from the same model, so a menu and its key never disagree.
//! Every item runs a registry command ([`crate::commands`]); its label and shortcut come
//! from the registry unless the item gives its own (items that pass an argument).

use crate::commands::{Arg, Cmd};
use crate::model::{Appearance, Layout, ThemeKind};
use crate::theme::Tokens;
use eframe::egui::{self, Key, KeyboardShortcut, Modifiers};

/// What the menus need to know to show check marks and disabled items.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct State {
    pub edit_mode: bool,
    pub layout: Layout,
    /// The current set's tiles per row.
    pub columns: u32,
    pub waveforms: bool,
    pub appearance: Appearance,
    pub theme: ThemeKind,
    pub can_delete_set: bool,
    pub has_next_cue: bool,
    pub anything_playing: bool,
}

pub struct Item {
    pub cmd: Cmd,
    pub label: &'static str,
    pub shortcut: Option<KeyboardShortcut>,
    /// `Some` for items with a check mark.
    pub checked: Option<bool>,
    pub enabled: bool,
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
const COLUMN_LABELS: [&str; 12] = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12"];
const CMD_SHIFT: Modifiers = Modifiers { shift: true, ..Modifiers::COMMAND };

/// A plain command, labelled and keyed as the registry says.
fn item(id: &'static str) -> Entry {
    let c = Cmd::new(id);
    let spec = c.spec();
    Entry::Item(Item { cmd: c, label: spec.label, shortcut: spec.shortcut, checked: None, enabled: true })
}

/// A command with an argument, or a label of its own.
fn item_as(cmd: Cmd, label: &'static str, shortcut: Option<KeyboardShortcut>) -> Entry {
    Entry::Item(Item { cmd, label, shortcut, checked: None, enabled: true })
}

fn checked(entry: Entry, on: bool) -> Entry {
    match entry {
        Entry::Item(i) => Entry::Item(Item { checked: Some(on), ..i }),
        e => e,
    }
}

fn enabled(entry: Entry, on: bool) -> Entry {
    match entry {
        Entry::Item(i) => Entry::Item(Item { enabled: on, ..i }),
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
    let mut file = vec![
        item("set.create"),
        item("tile.add"),
        // No accelerator: ⌘/Ctrl+V must keep pasting text into fields. The app catches
        // the shortcut itself when no field has focus.
        item("tile.paste"),
        Entry::Separator,
        item("set.import"),
        item("set.export"),
        Entry::Separator,
        item("set.rename"),
        item("set.duplicate"),
        enabled(item("set.delete"), s.can_delete_set),
        Entry::Separator,
        item("library.reveal"),
    ];
    if !mac {
        file.extend([Entry::Separator, item("view.settings"), Entry::Separator, item("app.quit")]);
    }
    out.push(Menu { title: "File", entries: file });
    let theme = |k: ThemeKind, name: &'static str, label| {
        checked(item_as(Cmd::with("view.theme", Arg::Str(name)), label, None), s.theme == k)
    };
    // Light/dark applies to Console; PANEL is always on black.
    let look = |a: Appearance, name: &'static str, label| {
        enabled(
            checked(item_as(Cmd::with("view.appearance", Arg::Str(name)), label, None), s.appearance == a),
            s.theme == ThemeKind::Console,
        )
    };
    out.push(Menu {
        title: "View",
        entries: vec![
            checked(item("view.perform"), !s.edit_mode),
            checked(item_as(Cmd::new("view.toggleEdit"), "Edit Mode", sc(CMD, Key::E)), s.edit_mode),
            Entry::Separator,
            checked(item_as(Cmd::with("set.layout", Arg::Str("grid")), "Grid", sc(CMD, Key::Num1)), s.layout == Layout::Grid),
            checked(item_as(Cmd::with("set.layout", Arg::Str("list")), "Cue List", sc(CMD, Key::Num2)), s.layout == Layout::List),
            // Also a toolbar control, but the PANEL toolbar drops it in narrow windows.
            Entry::Submenu(
                "Columns",
                (1..=12u32)
                    .map(|n| {
                        let entry = item_as(Cmd::with("set.columns", Arg::Num(n)), COLUMN_LABELS[n as usize - 1], None);
                        enabled(checked(entry, s.columns == n), s.layout == Layout::Grid)
                    })
                    .collect(),
            ),
            Entry::Separator,
            checked(item("view.waveforms"), s.waveforms),
            Entry::Submenu(
                "Theme",
                vec![theme(ThemeKind::Console, "console", "Console"), theme(ThemeKind::Panel, "panel", crate::brand::PANEL_THEME)],
            ),
            Entry::Submenu(
                "Appearance",
                vec![
                    look(Appearance::Dark, "dark", "Dark"),
                    look(Appearance::Light, "light", "Light"),
                    look(Appearance::System, "system", "Match System"),
                ],
            ),
        ],
    });
    out.push(Menu {
        title: "Playback",
        entries: vec![
            enabled(item_as(Cmd::new("cue.go"), "Play Next Cue  (Space)", None), s.has_next_cue && s.layout == Layout::List),
            item("cue.top"),
            Entry::Separator,
            enabled(item_as(Cmd::new("transport.stopAll"), "Stop All  (Esc)", sc(CMD, Key::Period)), s.anything_playing),
            enabled(
                item_as(Cmd::with("transport.stopAll", Arg::Bool(true)), "Stop All Immediately", sc(CMD_SHIFT, Key::Period)),
                s.anything_playing,
            ),
            Entry::Separator,
            item("master.up"),
            item("master.down"),
        ],
    });
    let mut help = vec![item("view.shortcuts")];
    if !mac {
        help.extend([Entry::Separator, item("view.about")]);
    }
    out.push(Menu { title: "Help", entries: help });
    out
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
    egui::menu::bar(ui, |ui| {
        for m in menus {
            ui.menu_button(m.title, |ui| {
                ui.set_min_width(220.0);
                if let Some(c) = entries_ui(ui, &m.entries) {
                    picked = Some(c);
                    ui.close_menu();
                }
            });
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
                ui.menu_button(*label, |ui| {
                    if let Some(c) = entries_ui(ui, sub) {
                        picked = Some(c);
                        ui.close_menu();
                    }
                });
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
                if ui.add_enabled(i.enabled, button).clicked() {
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
            edit_mode: false,
            layout: Layout::Grid,
            columns: 4,
            waveforms: true,
            appearance: Appearance::Dark,
            theme: ThemeKind::Console,
            can_delete_set: true,
            has_next_cue: true,
            anything_playing: true,
        }
    }

    fn titles(m: &[Menu]) -> Vec<&str> {
        m.iter().map(|m| m.title).collect()
    }

    #[test]
    fn mac_puts_settings_and_quit_in_the_app_menu() {
        let mac = menus(&state(), true);
        assert_eq!(titles(&mac), [crate::app::APP_NAME, "File", "View", "Playback", "Help"]);
        let linux = menus(&state(), false);
        assert_eq!(titles(&linux), ["File", "View", "Playback", "Help"]);
        // Both platforms reach every command that has a shortcut, with the same keys.
        let keys = |m: &[Menu]| {
            let mut v: Vec<_> = shortcuts(m).into_iter().map(|(s, c)| (format!("{s:?}"), c)).collect();
            v.sort_by(|a, b| a.0.cmp(&b.0));
            v
        };
        assert_eq!(keys(&mac), keys(&linux));
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
