//! The native macOS menu bar, built with `muda` from the shared menu model.
//!
//! The menu structure is built once. Check marks, enabled states and the items whose
//! command changes with the state (Record New Tile ↔ Stop Recording, found again by their
//! slot in the model) are synced from `menu::State` whenever it changes, and View ▸ Theme
//! is rebuilt when the installed themes change (an import, a removal, a renamed theme).
//! `menu::shape` must not depend on the state; a test there holds it to that (D-124). Clicks and key equivalents arrive as
//! `MenuEvent`s, which are mapped back to registry commands ([`Cmd`]); the handler also wakes egui, so a
//! menu works even while the app is idle.

use crate::commands::Cmd;
use crate::menu::{self, Entry, State};
use eframe::egui::{self, Key, KeyboardShortcut};
use muda::accelerator::{Accelerator, Code, Modifiers};
use muda::{AboutMetadata, CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu};
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Mutex;

pub struct NativeMenu {
    _menu: Menu,
    commands: HashMap<MenuId, Cmd>,
    checks: Vec<(Cmd, CheckMenuItem)>,
    /// Plain items by their slot (`menu::slots`), with the command and label they show.
    items: Vec<Slot>,
    /// View ▸ Theme and its slot, and the installed themes it lists.
    themes: Option<(Submenu, Vec<usize>)>,
    listed: Vec<(String, String)>,
    events: Receiver<MenuId>,
    last: Option<State>,
}

struct Slot {
    at: Vec<usize>,
    cmd: Cmd,
    label: &'static str,
    item: MenuItem,
}

fn code(key: Key) -> Option<Code> {
    Some(match key {
        Key::A => Code::KeyA,
        Key::E => Code::KeyE,
        Key::N => Code::KeyN,
        Key::O => Code::KeyO,
        Key::Q => Code::KeyQ,
        Key::Num1 => Code::Digit1,
        Key::Num2 => Code::Digit2,
        Key::Num0 => Code::Digit0,
        Key::Equals => Code::Equal,
        Key::Minus => Code::Minus,
        Key::Comma => Code::Comma,
        Key::Period => Code::Period,
        Key::ArrowUp => Code::ArrowUp,
        Key::ArrowDown => Code::ArrowDown,
        _ => return None,
    })
}

fn accelerator(s: Option<KeyboardShortcut>) -> Option<Accelerator> {
    let s = s?;
    let mut m = Modifiers::empty();
    if s.modifiers.command || s.modifiers.mac_cmd {
        m |= Modifiers::META;
    }
    if s.modifiers.shift {
        m |= Modifiers::SHIFT;
    }
    if s.modifiers.alt {
        m |= Modifiers::ALT;
    }
    if s.modifiers.ctrl && !s.modifiers.command {
        m |= Modifiers::CONTROL;
    }
    Some(Accelerator::new(m, code(s.logical_key)?))
}

impl NativeMenu {
    /// Build the menu bar and install it as the app's main menu. Call on the main
    /// thread, after the event loop has started (winit installs its own default menu
    /// when the app finishes launching, and this replaces it).
    pub fn install(ctx: &egui::Context, state: &State) -> NativeMenu {
        // A single-window app: no "Show Tab Bar" / "Show All Tabs" in the View menu.
        if let Some(mtm) = objc2::MainThreadMarker::new() {
            objc2_app_kit::NSWindow::setAllowsAutomaticWindowTabbing(false, mtm);
        }
        let (tx, events) = channel();
        let tx = Mutex::new(tx);
        let repaint = ctx.clone();
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
            if let Ok(tx) = tx.lock() {
                let _ = tx.send(e.id().clone());
            }
            repaint.request_repaint();
        }));

        let mut this = NativeMenu { _menu: Menu::new(), commands: HashMap::new(), checks: Vec::new(), items: Vec::new(), themes: None, listed: state.user_themes.clone(), events, last: None };
        let bar = Menu::new();
        let model = menu::menus(state, true);
        for (i, m) in model.iter().enumerate() {
            let sub = Submenu::new(m.title, true);
            if i == 0 {
                // The application menu: About, then the model's items, with the
                // standard Services / Hide group before Quit.
                let about = AboutMetadata {
                    name: Some(crate::app::APP_NAME.to_owned()),
                    version: Some(env!("CARGO_PKG_VERSION").to_owned()),
                    comments: Some("A live soundboard. Design after SoundCraft by the ArtCraft team; Inter by The Inter Project Authors; Antonio by The Antonio Project Authors.".to_owned()),
                    ..Default::default()
                };
                let _ = sub.append(&PredefinedMenuItem::about(Some(&format!("About {}", crate::app::APP_NAME)), Some(about)));
                let _ = sub.append(&PredefinedMenuItem::separator());
                for (n, e) in m.entries.iter().enumerate() {
                    if let Entry::Item(it) = e {
                        if it.cmd.id == "app.quit" {
                            let _ = sub.append(&PredefinedMenuItem::services(None));
                            let _ = sub.append(&PredefinedMenuItem::separator());
                            let _ = sub.append(&PredefinedMenuItem::hide(None));
                            let _ = sub.append(&PredefinedMenuItem::hide_others(None));
                            let _ = sub.append(&PredefinedMenuItem::show_all(None));
                            let _ = sub.append(&PredefinedMenuItem::separator());
                        }
                    }
                    this.append_entry(&sub, e, vec![i, n]);
                }
            } else {
                for (n, e) in m.entries.iter().enumerate() {
                    this.append_entry(&sub, e, vec![i, n]);
                }
                if m.title == "View" {
                    let _ = sub.append(&PredefinedMenuItem::separator());
                    let _ = sub.append(&PredefinedMenuItem::fullscreen(None));
                }
            }
            if m.title == "Help" {
                // Window comes before Help, as on every Mac app.
                let window = Submenu::new("Window", true);
                let _ = window.append(&PredefinedMenuItem::minimize(None));
                let _ = window.append(&PredefinedMenuItem::maximize(Some("Zoom")));
                let _ = window.append(&PredefinedMenuItem::separator());
                let _ = window.append(&PredefinedMenuItem::bring_all_to_front(None));
                let _ = bar.append(&window);
                window.set_as_windows_menu_for_nsapp();
                sub.set_as_help_menu_for_nsapp();
            }
            let _ = bar.append(&sub);
        }
        bar.init_for_nsapp();
        this._menu = bar;
        this.last = Some(state.clone());
        this
    }

    /// Add `e`, which sits at slot `at` in the model.
    fn append_entry(&mut self, parent: &Submenu, e: &Entry, at: Vec<usize>) {
        match e {
            Entry::Separator => {
                let _ = parent.append(&PredefinedMenuItem::separator());
            }
            Entry::Submenu(label, entries) => {
                let sub = Submenu::new(*label, true);
                for (n, e) in entries.iter().enumerate() {
                    self.append_entry(&sub, e, [&at[..], &[n]].concat());
                }
                let _ = parent.append(&sub);
                if *label == "Theme" {
                    self.themes = Some((sub, at));
                }
            }
            Entry::Item(it) => {
                let accel = accelerator(it.shortcut);
                match it.checked {
                    Some(checked) => {
                        let c = CheckMenuItem::new(it.label, it.enabled, checked, accel);
                        self.commands.insert(c.id().clone(), it.cmd);
                        let _ = parent.append(&c);
                        self.checks.push((it.cmd, c));
                    }
                    None => {
                        let m = MenuItem::new(it.label, it.enabled, accel);
                        self.commands.insert(m.id().clone(), it.cmd);
                        let _ = parent.append(&m);
                        self.items.push(Slot { at, cmd: it.cmd, label: it.label, item: m });
                    }
                }
            }
        }
    }

    /// Bring check marks, enabled states and swapped items up to date with `state`.
    pub fn sync(&mut self, state: &State) {
        if self.last.as_ref() == Some(state) {
            return;
        }
        let model = menu::menus(state, true);
        if self.listed != state.user_themes {
            self.listed = state.user_themes.clone();
            self.rebuild_themes(&model);
        }
        self.last = Some(state.clone());
        let mut flat: HashMap<Cmd, (Option<bool>, bool)> = HashMap::new();
        fn walk(entries: &[Entry], out: &mut HashMap<Cmd, (Option<bool>, bool)>) {
            for e in entries {
                match e {
                    Entry::Item(i) => {
                        out.insert(i.cmd, (i.checked, i.enabled));
                    }
                    Entry::Submenu(_, sub) => walk(sub, out),
                    Entry::Separator => {}
                }
            }
        }
        for m in &model {
            walk(&m.entries, &mut flat);
        }
        for (cmd, item) in &self.checks {
            if let Some((checked, enabled)) = flat.get(cmd) {
                item.set_checked(checked.unwrap_or(false));
                item.set_enabled(*enabled);
            }
        }
        let slots: HashMap<Vec<usize>, &menu::Item> = menu::slots(&model).into_iter().collect();
        for slot in &mut self.items {
            let Some(i) = slots.get(&slot.at) else { continue };
            if (i.cmd, i.label) != (slot.cmd, slot.label) {
                slot.item.set_text(i.label);
                let _ = slot.item.set_accelerator(accelerator(i.shortcut));
                self.commands.insert(slot.item.id().clone(), i.cmd);
                (slot.cmd, slot.label) = (i.cmd, i.label);
            }
            slot.item.set_enabled(i.enabled);
        }
    }

    /// Fill View ▸ Theme again from the model, dropping its old items.
    fn rebuild_themes(&mut self, model: &[menu::Menu]) {
        let Some((sub, at)) = self.themes.clone() else { return };
        let Some(entries) = model.iter().flat_map(|m| &m.entries).find_map(|e| match e {
            Entry::Submenu("Theme", entries) => Some(entries),
            _ => None,
        }) else {
            return;
        };
        let mut gone = HashSet::new();
        while let Some(old) = sub.remove_at(0) {
            gone.insert(old.id().clone());
        }
        self.commands.retain(|id, _| !gone.contains(id));
        self.checks.retain(|(_, c)| !gone.contains(c.id()));
        self.items.retain(|s| !gone.contains(s.item.id()));
        for (n, e) in entries.iter().enumerate() {
            self.append_entry(&sub, e, [&at[..], &[n]].concat());
        }
    }

    /// Cmds chosen from the menu bar since the last call.
    pub fn take_commands(&mut self) -> Vec<Cmd> {
        let mut out = Vec::new();
        while let Ok(id) = self.events.try_recv() {
            if let Some(cmd) = self.commands.get(&id) {
                out.push(*cmd);
            }
        }
        if !out.is_empty() {
            // AppKit toggles a check item when it's clicked; resync to the model.
            self.last = None;
        }
        out
    }
}
