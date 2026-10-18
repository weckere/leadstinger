//! The native macOS menu bar, built with `muda` from the shared menu model.
//!
//! The menu structure is built once. Check marks and enabled states are synced from
//! `menu::State` whenever it changes. Clicks and key equivalents arrive as
//! `MenuEvent`s, which are mapped back to registry commands ([`Cmd`]); the handler also wakes egui, so a
//! menu works even while the app is idle.

use crate::commands::Cmd;
use crate::menu::{self, Entry, State};
use eframe::egui::{self, Key, KeyboardShortcut};
use muda::accelerator::{Accelerator, Code, Modifiers};
use muda::{AboutMetadata, CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu};
use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver};
use std::sync::Mutex;

pub struct NativeMenu {
    _menu: Menu,
    commands: HashMap<MenuId, Cmd>,
    checks: Vec<(Cmd, CheckMenuItem)>,
    items: Vec<(Cmd, MenuItem)>,
    events: Receiver<MenuId>,
    last: Option<State>,
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

        let mut this = NativeMenu { _menu: Menu::new(), commands: HashMap::new(), checks: Vec::new(), items: Vec::new(), events, last: None };
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
                for e in &m.entries {
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
                    this.append_entry(&sub, e);
                }
            } else {
                for e in &m.entries {
                    this.append_entry(&sub, e);
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
        this.last = Some(*state);
        this
    }

    fn append_entry(&mut self, parent: &Submenu, e: &Entry) {
        match e {
            Entry::Separator => {
                let _ = parent.append(&PredefinedMenuItem::separator());
            }
            Entry::Submenu(label, entries) => {
                let sub = Submenu::new(*label, true);
                for e in entries {
                    self.append_entry(&sub, e);
                }
                let _ = parent.append(&sub);
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
                        self.items.push((it.cmd, m));
                    }
                }
            }
        }
    }

    /// Bring check marks and enabled states up to date with `state`.
    pub fn sync(&mut self, state: &State) {
        if self.last.as_ref() == Some(state) {
            return;
        }
        self.last = Some(*state);
        let model = menu::menus(state, true);
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
        for (cmd, item) in &self.items {
            if let Some((_, enabled)) = flat.get(cmd) {
                item.set_enabled(*enabled);
            }
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
