//! The egui front end. UI code collects [`Action`]s while drawing; they are applied
//! after the frame's panels are laid out, which keeps borrows simple.

use crate::audio::{Engine, VoiceStatus};
use crate::decode::AUDIO_EXTENSIONS;
use crate::logic::{
    db_to_gain, format_clock, gain_to_db, next_cue, play_range, tile_for_key, PlayState,
};
use crate::model::{Appearance, ImageFit, ImageLayer, Layout, PlayMode, ThemeKind, Tile, TileColor};
use crate::picture::{is_image_path, IMAGE_EXTENSIONS};
use crate::store::{Media, PictureState, Store};
use crate::commands::{Cmd, Invocation};
use crate::control::{self, Lender};
use crate::controller::{Controller, Effect};
use std::sync::Arc;
use crate::inv;
use crate::menu;
use serde_json::{json, Value};
use crate::theme::{self, Tokens};
use eframe::egui::{
    self, pos2, vec2, Align, Align2, Color32, CornerRadius, FontId, Key, Modifiers, Rect, RichText, Sense,
    Stroke, StrokeKind, Ui,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use uuid::Uuid;

mod panel;

/// Shown in the window title, the toolbar logo and the menus (see brand.rs).
pub use crate::brand::APP_NAME;

/// Startup options, mostly for automated screenshots and demos.
#[derive(Default)]
pub struct Options {
    /// Save a screenshot here after `snapshot_frames` frames, then quit.
    pub snapshot: Option<PathBuf>,
    pub snapshot_frames: u64,
    /// Comma-separated UI steps run on startup: `play:N`, `select:N`, `edit`, `list`,
    /// `grid`, `set:N`, `settings`, `import:PATH`, `paste`, `picture:N:PATH`, `overlay:N`, `fit:N`,
    /// and `wait:FRAMES` to pause (e.g. until imported audio has decoded).
    pub script: Vec<String>,
    /// Frame before which the next script step must not run.
    pub script_resume_frame: u64,
    /// Draw the menu bar inside the window even on macOS (`LEADSTINGER_IN_WINDOW_MENU=1`).
    pub in_window_menu: bool,
    /// The window runs on X11 or XWayland rather than native Wayland (Linux).
    pub x11: bool,
}

impl Options {
    pub fn from_env() -> Options {
        Options {
            snapshot: crate::brand::env("SNAPSHOT").map(PathBuf::from),
            snapshot_frames: crate::brand::env_string("SNAPSHOT_FRAMES").and_then(|v| v.parse().ok()).unwrap_or(40),
            script: crate::brand::env_string("SCRIPT")
                .map(|s| s.split(',').map(|p| p.trim().to_owned()).filter(|p| !p.is_empty()).collect())
                .unwrap_or_default(),
            script_resume_frame: 0,
            in_window_menu: crate::brand::env("IN_WINDOW_MENU").is_some(),
            x11: false,
        }
    }
}

/// What a widget asks for. Almost everything is a registry command (`Run`); the rest is
/// GUI-only state: a held mouse button, hotkey capture and editing a set name in place.
enum Action {
    Run(Invocation),
    /// Mouse down on a tile: trigger it, and release it when the button comes up.
    Press(Uuid),
    CaptureHotkey,
    StartRename(Uuid),
    CommitRename,
    CancelRename,
}

impl From<Invocation> for Action {
    fn from(i: Invocation) -> Action {
        Action::Run(i)
    }
}

/// The Controller while the window has it, for one frame at a time (see control.rs).
/// Derefs to it, so the UI code reads `self.ctl.store` and so on.
struct Lent(Option<Controller>);

impl std::ops::Deref for Lent {
    type Target = Controller;
    fn deref(&self) -> &Controller {
        self.0.as_ref().expect("the window uses the Controller only during a frame")
    }
}

impl std::ops::DerefMut for Lent {
    fn deref_mut(&mut self) -> &mut Controller {
        self.0.as_mut().expect("the window uses the Controller only during a frame")
    }
}

pub struct StingerApp {
    /// Library, engine, cue pointers and view state; every command runs here. Shared
    /// with the control socket, which runs the CLI (`leadstinger-cli`) commands between frames.
    lender: Arc<Lender>,
    ctl: Lent,
    /// The control socket; dropping it removes the socket file.
    _control: Option<control::Server>,
    renaming: Option<(Uuid, String, bool)>, // set id, draft, focus requested
    mouse_held: Option<Uuid>,
    keys_held: HashMap<Key, Uuid>,
    capturing_hotkey: bool,
    tile_rects: Vec<(Uuid, usize, Rect)>,
    /// Tile pictures uploaded to the GPU, by media file name.
    textures: HashMap<String, egui::TextureHandle>,
    meter: Meter,
    /// When ⌘/Ctrl+V last imported (or tried to), so one keystroke imports once.
    last_paste: Option<Instant>,
    /// Screenshots asked for (`view.screenshot`, or `LEADSTINGER_SNAPSHOT`), saved from the
    /// next Screenshot event; `true` quits afterwards.
    shots: Vec<(PathBuf, bool)>,
    #[cfg(target_os = "macos")]
    native_menu: Option<crate::native_menu::NativeMenu>,
    last_frame: Instant,
    frame: u64,
    opts: Options,
    snapshot_requested: bool,
}

impl StingerApp {
    pub fn new(cc: &eframe::CreationContext<'_>, store: Store, engine: Engine, opts: Options) -> Self {
        theme::apply(&cc.egui_ctx, store.library.appearance, store.library.theme);
        let data_dir = store.data_dir.clone();
        let mut ctl = Controller::new(store, engine);
        ctl.x11 = opts.x11;
        let lender = Lender::new(ctl);
        let control = match control::serve(lender.clone(), &data_dir, Some(cc.egui_ctx.clone())) {
            Ok(server) => Some(server),
            Err(e) => {
                lender.with(|c| c.notify_error(format!("{} can't reach this window: {e}", crate::brand::CLI_NAME)));
                None
            }
        };
        StingerApp {
            lender,
            ctl: Lent(None),
            _control: control,
            renaming: None,
            mouse_held: None,
            keys_held: HashMap::new(),
            capturing_hotkey: false,
            tile_rects: Vec::new(),
            textures: HashMap::new(),
            meter: Meter::new(),
            last_paste: None,
            shots: Vec::new(),
            #[cfg(target_os = "macos")]
            native_menu: None,
            last_frame: Instant::now(),
            frame: 0,
            opts,
            snapshot_requested: false,
        }
    }

    /// Run a command for the GUI; failures go to the status bar.
    fn run(&mut self, inv: Invocation) {
        if inv.id == "view.select" {
            self.capturing_hotkey = false;
        }
        if let Err(e) = self.ctl.execute(inv.id, inv.params) {
            self.ctl.report(&e);
        }
    }

    /// Carry out what commands asked of the window (pickers, renames, restyling, …).
    fn handle_effects(&mut self, ctx: &egui::Context) {
        while !self.ctl.effects.is_empty() {
            for effect in std::mem::take(&mut self.ctl.effects) {
                match effect {
                    Effect::ApplyLook => theme::apply_look(ctx, self.ctl.store.library.appearance, self.ctl.store.library.theme),
                    Effect::PickAudio { set, at } => {
                        let paths = Self::pick_audio_files(true);
                        if !paths.is_empty() {
                            self.run(inv!("tile.add", {"paths": paths, "set": set, "at": at.map(|a| a + 1)}));
                        }
                    }
                    Effect::PickReplacement(id) => {
                        if let Some(path) = Self::pick_audio_files(false).pop() {
                            self.run(inv!("tile.replaceAudio", {"tile": id, "path": path}));
                        }
                    }
                    Effect::PickPicture(id) => {
                        if let Some(path) = Self::pick_image_file() {
                            self.run(inv!("tile.picture", {"tile": id, "path": path}));
                        }
                    }
                    Effect::StartRename(id) => {
                        if let Some(i) = self.ctl.store.library.set_index(id) {
                            self.renaming = Some((id, self.ctl.store.library.sets[i].name.clone(), false));
                        }
                    }
                    Effect::Screenshot(path) => {
                        self.shots.push((path, false));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
                    }
                    Effect::ClearClip => self.meter.clip = false,
                    Effect::ReleaseHeld => {
                        self.mouse_held = None;
                        self.keys_held.clear();
                    }
                    Effect::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                }
            }
        }
    }

    // ── input ───────────────────────────────────────────────────────────────

    fn menu_state(&self) -> menu::State {
        let set = self.ctl.current_set();
        let pointer = self.ctl.cue.get(&set.id).copied().unwrap_or(0);
        menu::State {
            edit_mode: self.ctl.edit_mode,
            layout: set.layout,
            columns: set.columns,
            waveforms: self.ctl.store.library.show_waveforms,
            appearance: self.ctl.store.library.appearance,
            theme: self.ctl.store.library.theme,
            can_delete_set: self.ctl.store.library.sets.len() > 1,
            has_next_cue: next_cue(&set.tiles, pointer).is_some(),
            anything_playing: !self.ctl.statuses.is_empty(),
        }
    }

    /// Keyboard shortcuts, from the menu model so they always match the menus.
    fn handle_shortcuts(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let table = menu::shortcuts(&menu::menus(&self.menu_state(), cfg!(target_os = "macos")));
        let hits: Vec<Cmd> = ctx.input_mut(|i| table.iter().filter(|(s, _)| i.consume_shortcut(s)).map(|(_, c)| *c).collect());
        actions.extend(hits.into_iter().map(|c| Action::Run(c.into())));
    }

    fn about_window(&mut self, ctx: &egui::Context) {
        let t = *Tokens::get(ctx);
        egui::Window::new(format!("About {APP_NAME}")).open(&mut self.ctl.show_about).collapsible(false).resizable(false).show(ctx, |ui| {
            ui.label(RichText::new(APP_NAME).font(theme::bold(20.0)).color(t.text));
            ui.label(RichText::new(format!("Version {}", env!("CARGO_PKG_VERSION"))).color(t.text_dim));
            ui.add_space(6.0);
            ui.label("A live soundboard for macOS and Linux.");
            ui.label(RichText::new("Design after SoundCraft by the ArtCraft team. Inter by The Inter Project Authors (OFL).").small().color(t.text_dim));
        });
    }

    fn shortcuts_window(&mut self, ctx: &egui::Context) {
        let t = *Tokens::get(ctx);
        let menus = menu::menus(&self.menu_state(), cfg!(target_os = "macos"));
        let table = menu::shortcuts(&menus);
        let mut open = self.ctl.show_shortcuts;
        egui::Window::new("Keyboard Shortcuts").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
            egui::Grid::new("shortcut-grid").num_columns(2).spacing([24.0, 6.0]).show(ui, |ui| {
                let mut row = |k: String, what: &str| {
                    ui.label(RichText::new(k).font(theme::semibold(13.0)).color(t.text));
                    ui.label(RichText::new(what).color(t.text_dim));
                    ui.end_row();
                };
                row("1–0, Q–P, A–;, Z–/".into(), "Trigger the tile with that hotkey");
                row("Space".into(), "Play the next cue (cue list)");
                row("Esc / Shift+Esc".into(), "Stop All with a fade / at once");
                row(ctx.format_shortcut(&egui::KeyboardShortcut::new(Modifiers::COMMAND, Key::V)), "Paste sounds copied in a file manager");
                for (s, cmd) in &table {
                    row(ctx.format_shortcut(s), menu::label(&menus, *cmd));
                }
            });
        });
        self.ctl.show_shortcuts = open;
    }

    fn handle_hotkeys(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        // Leave keys alone while typing or while a menu is open.
        if ctx.wants_keyboard_input() || ctx.memory(|m| m.any_popup_open()) {
            return;
        }
        let events = ctx.input(|i| i.events.clone());
        for ev in events {
            let egui::Event::Key { key, pressed, repeat, modifiers, .. } = ev else { continue };
            if modifiers.command || modifiers.ctrl || modifiers.alt || modifiers.mac_cmd || (pressed && repeat) {
                continue;
            }
            if self.capturing_hotkey && pressed {
                self.capturing_hotkey = false;
                if let Some(id) = self.ctl.selected_tile {
                    match key {
                        Key::Escape => {}
                        Key::Backspace | Key::Delete => actions.push(inv!("tile.set", {"tile": id, "hotkey": null}).into()),
                        k => match key_char(k) {
                            Some(c) => actions.push(inv!("tile.set", {"tile": id, "hotkey": c.to_string()}).into()),
                            None => self.ctl.notify("That key can't be a hotkey. Use a letter, a digit or ; , . /"),
                        },
                    }
                }
                continue;
            }
            match (key, pressed) {
                (Key::Escape, true) => actions.push(inv!("transport.stopAll", {"immediate": modifiers.shift}).into()),
                (Key::Space, true) if self.ctl.current_set().layout == Layout::List => {
                    // Consume it so a focused button (e.g. GO after a click) doesn't also fire.
                    ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Space));
                    actions.push(inv!("cue.go").into());
                }
                (k, true) => {
                    let found = key_char(k).and_then(|c| tile_for_key(&self.ctl.current_set().tiles, c)).map(|t| t.id);
                    if let Some(id) = found {
                        self.keys_held.insert(k, id);
                        actions.push(inv!("tile.trigger", {"tile": id, "event": "press"}).into());
                    }
                }
                (k, false) => {
                    if let Some(id) = self.keys_held.remove(&k) {
                        actions.push(inv!("tile.trigger", {"tile": id, "event": "release"}).into());
                    }
                }
            }
        }
    }

    /// Files dropped on the window: pictures onto the tile under the pointer, one sound
    /// onto a tile replaces its audio, anything else becomes new tiles there.
    fn handle_file_drop(&mut self, ctx: &egui::Context) {
        let (dropped, pos) = ctx.input(|i| (i.raw.dropped_files.clone(), i.pointer.latest_pos()));
        let paths: Vec<PathBuf> = dropped.into_iter().filter_map(|f| f.path).collect();
        if paths.is_empty() {
            return;
        }
        let target = pos.and_then(|p| self.tile_rects.iter().find(|(_, _, r)| r.contains(p)).map(|(id, i, _)| (*id, *i)));
        // Pictures go onto the tile they're dropped on; they're never imported as sounds.
        let (images, paths): (Vec<PathBuf>, Vec<PathBuf>) = paths.into_iter().partition(|p| is_image_path(p));
        if let Some(img) = images.first() {
            match target {
                Some((id, _)) => self.run(inv!("tile.picture", {"tile": id, "path": img})),
                None => self.ctl.notify("Drop a picture onto a tile to use it as that tile's picture."),
            }
        }
        match target {
            _ if paths.is_empty() => {}
            Some((id, _)) if paths.len() == 1 => self.run(inv!("tile.replaceAudio", {"tile": id, "path": paths[0]})),
            _ => self.run(inv!("tile.add", {"paths": paths, "at": target.map(|(_, i)| i + 1)})),
        }
    }

    /// ⌘/Ctrl+V outside a text field imports files copied in a file manager. egui sends
    /// a Paste event only when the clipboard also holds text, so the key's release is
    /// watched too (its press never reaches the app).
    fn handle_paste(&mut self, ctx: &egui::Context) {
        if ctx.wants_keyboard_input() || ctx.memory(|m| m.any_popup_open()) {
            return;
        }
        let (text, released) = ctx.input(|i| {
            let text = i.events.iter().find_map(|e| match e {
                egui::Event::Paste(t) => Some(t.clone()),
                _ => None,
            });
            let released = i.events.iter().any(|e| {
                matches!(e, egui::Event::Key { key: Key::V, pressed: false, modifiers, .. } if modifiers.command)
            });
            (text, released)
        });
        if text.is_none() && !released {
            return;
        }
        if self.last_paste.is_some_and(|t| t.elapsed() < Duration::from_millis(800)) {
            return;
        }
        self.last_paste = Some(Instant::now());
        match self.ctl.execute("tile.paste", json!({"text": text})) {
            // Ordinary text pasted outside a field isn't worth a message.
            Err(e) if e.code == "nothing_to_paste" && text.is_some() => {}
            Err(e) => self.ctl.report(&e),
            Ok(_) => {}
        }
    }

    fn pick_image_file() -> Option<PathBuf> {
        rfd::FileDialog::new().set_title("Choose a Picture").add_filter("Images", IMAGE_EXTENSIONS).pick_file()
    }

    /// Upload any decoded pictures the visible tiles (and the inspector) need.
    /// Painting then only reads `textures`.
    fn prepare_textures(&mut self, ctx: &egui::Context) {
        let mut wanted: Vec<&Tile> = self.ctl.current_set().tiles.iter().collect();
        if let Some(t) = self.ctl.selected_tile.and_then(|id| self.ctl.store.library.tile(id)) {
            wanted.push(t);
        }
        let mut uploads = Vec::new();
        for t in wanted {
            let Some(name) = &t.image else { continue };
            if self.textures.contains_key(name) {
                continue;
            }
            if let Some(PictureState::Ready(p)) = self.ctl.store.picture_state(t) {
                uploads.push((name.clone(), p.clone()));
            }
        }
        for (name, p) in uploads {
            let img = egui::ColorImage::from_rgba_unmultiplied([p.width, p.height], &p.rgba);
            let tex = ctx.load_texture(format!("tile-picture-{name}"), img, egui::TextureOptions::LINEAR);
            self.textures.insert(name, tex);
        }
    }

    fn tile_texture(&self, tile: &Tile) -> Option<&egui::TextureHandle> {
        tile.image.as_ref().and_then(|n| self.textures.get(n))
    }

    fn pick_audio_files(multiple: bool) -> Vec<PathBuf> {
        let dialog = rfd::FileDialog::new().set_title("Add Sounds").add_filter("Audio", AUDIO_EXTENSIONS);
        if multiple {
            dialog.pick_files().unwrap_or_default()
        } else {
            dialog.pick_file().into_iter().collect()
        }
    }

    // ── actions ─────────────────────────────────────────────────────────────

    fn apply(&mut self, action: Action) {
        match action {
            Action::Run(inv) => self.run(inv),
            Action::Press(id) => {
                self.mouse_held = Some(id);
                self.run(inv!("tile.trigger", {"tile": id, "event": "press"}));
            }
            Action::CaptureHotkey => self.capturing_hotkey = true,
            Action::StartRename(id) => {
                if let Some(i) = self.ctl.store.library.set_index(id) {
                    self.renaming = Some((id, self.ctl.store.library.sets[i].name.clone(), false));
                }
            }
            Action::CommitRename => {
                if let Some((id, name, _)) = self.renaming.take() {
                    if !name.trim().is_empty() {
                        self.run(inv!("set.rename", {"set": id, "name": name}));
                    }
                }
            }
            Action::CancelRename => self.renaming = None,
        }
    }

    /// `LEADSTINGER_SCRIPT` steps, for automated screenshots. Each step is one command, so
    /// scripts exercise the same paths as the GUI and (from M7) the CLI.
    fn run_script(&mut self) {
        if self.frame < 3.max(self.opts.script_resume_frame) || self.opts.script.is_empty() {
            return;
        }
        while !self.opts.script.is_empty() {
            let step = self.opts.script.remove(0);
            let (cmd, arg) = step.split_once(':').unwrap_or((step.as_str(), ""));
            let n: usize = arg.parse().unwrap_or(0);
            let tile = format!("#{}", n + 1); // steps count tiles from 0
            let inv = match cmd {
                "play" => inv!("tile.trigger", {"tile": tile}),
                "select" => inv!("view.select", {"tile": tile}),
                "edit" => inv!("view.edit"),
                "list" | "grid" => inv!("set.layout", {"layout": cmd}),
                "set" => inv!("set.select", {"set": tile}),
                "settings" => inv!("view.settings"),
                "import" => inv!("tile.add", {"paths": [arg]}),
                "picture" => {
                    // picture:N:/path/to/image
                    let (idx, path) = arg.split_once(':').unwrap_or(("", ""));
                    let n: usize = idx.parse().unwrap_or(0);
                    inv!("tile.picture", {"tile": format!("#{}", n + 1), "path": path})
                }
                // Change a tile's picture layer or fit, to exercise both paths.
                "overlay" => inv!("tile.set", {"tile": tile, "pictureLayer": "overlay", "pictureOpacity": 0.9}),
                "fit" => inv!("tile.set", {"tile": tile, "pictureFit": "fit"}),
                "paste" => inv!("tile.paste"),
                "padd" | "panel" | "console" => inv!("view.theme", {"theme": cmd}),
                "light" | "dark" => inv!("view.appearance", {"appearance": cmd}),
                "wait" => {
                    self.opts.script_resume_frame = self.frame + n as u64;
                    break;
                }
                other => {
                    eprintln!("{}: unknown script step {other:?}", crate::brand::SLUG);
                    continue;
                }
            };
            if let Err(e) = self.ctl.execute(inv.id, inv.params) {
                eprintln!("{}: script step {step:?}: {e}", crate::brand::SLUG);
                self.ctl.report(&e);
            }
        }
    }

    // ── panels ──────────────────────────────────────────────────────────────

    fn toolbar(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let t = *Tokens::get(ui.ctx());
        ui.horizontal(|ui| {
            ui.set_height(48.0);
            ui.spacing_mut().item_spacing.x = 10.0;
            // Narrow (e.g. tiled) windows first shrink Add Sounds to its icon, then the
            // wordmark to its tile mark, so Stop All and Settings always have room. The
            // full row needs about 800 px.
            let room = ui.available_width();
            let (compact_add, compact_mark) = (room < 810.0, room < 720.0);
            paint_wordmark(ui, compact_mark);

            theme::group_frame(&t).show(ui, |ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                if theme::key(ui, "Perform", vec2(74.0, 26.0), !self.ctl.edit_mode).on_hover_text("Click tiles to play them").clicked() {
                    actions.push(inv!("view.perform").into());
                }
                if theme::key(ui, "Edit", vec2(64.0, 26.0), self.ctl.edit_mode)
                    .on_hover_text("Click tiles to edit them, drag to reorder (⌘/Ctrl+E)")
                    .clicked()
                {
                    actions.push(inv!("view.edit").into());
                }
            });

            let si = self.ctl.set_idx();
            let mut layout = self.ctl.store.library.sets[si].layout;
            let mut columns = self.ctl.store.library.sets[si].columns;
            theme::group_frame(&t).show(ui, |ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                if theme::key(ui, "Grid", vec2(54.0, 26.0), layout == Layout::Grid).on_hover_text("⌘/Ctrl+1").clicked() {
                    layout = Layout::Grid;
                }
                if theme::key(ui, "List", vec2(54.0, 26.0), layout == Layout::List).on_hover_text("⌘/Ctrl+2").clicked() {
                    layout = Layout::List;
                }
                if layout == Layout::Grid {
                    ui.add_space(4.0);
                    ui.add(egui::DragValue::new(&mut columns).range(1..=12).speed(0.05))
                        .on_hover_text("Tiles per row (drag or type)");
                }
            });
            let set = &self.ctl.store.library.sets[si];
            if set.layout != layout {
                actions.push(inv!("set.layout", {"layout": layout}).into());
            }
            if set.columns != columns {
                actions.push(inv!("set.columns", {"columns": columns}).into());
            }

            theme::group_frame(&t).show(ui, |ui| {
                let (label, width) = if compact_add { ("✚", 34.0) } else { ("✚  Add Sounds", 118.0) };
                if theme::key(ui, label, vec2(width, 26.0), false)
                    .on_hover_text("Add Sounds: ⌘/Ctrl+O, or drop files on the window")
                    .clicked()
                {
                    actions.push(inv!("tile.add").into());
                }
            });

            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                if theme::key(ui, "⚙", vec2(34.0, 34.0), self.ctl.show_settings).on_hover_text("Settings (⌘/Ctrl+,)").clicked() {
                    actions.push(inv!("view.settings", {"open": !self.ctl.show_settings}).into());
                }
                let stop = theme::key_with(ui, "⏹  Stop All", vec2(112.0, 34.0), t.stop, Color32::from_rgb(0xFF, 0xD6, 0xD2), theme::semibold(13.0));
                if stop.on_hover_text("Esc fades out, Shift+Esc stops at once").clicked() {
                    actions.push(inv!("transport.stopAll").into());
                }
                // Narrow windows drop the counter, then the meter, rather than overlap
                // the left-hand groups.
                if ui.available_width() > 230.0 {
                    self.master_section(ui, &t);
                }
                let room = ui.available_width() - 12.0;
                if room >= 190.0 {
                    self.counter(ui, &t, room.min(240.0));
                }
            });
        });
    }

    /// The most relevant sounding tile: the last one started, if still going.
    fn focus_voice(&self) -> Option<(Uuid, VoiceStatus)> {
        if let Some(id) = self.ctl.last_started {
            if let Some(s) = self.ctl.statuses.get(&id) {
                return Some((id, *s));
            }
        }
        self.ctl.statuses
            .iter()
            .filter(|(_, s)| s.state == PlayState::Playing)
            .min_by(|a, b| a.1.remaining().total_cmp(&b.1.remaining()))
            .map(|(id, s)| (*id, *s))
    }

    /// Green-on-black countdown for the focus tile, like a DAW's main counter.
    fn counter(&self, ui: &mut Ui, t: &Tokens, width: f32) {
        let (rect, _) = ui.allocate_exact_size(vec2(width, 46.0), Sense::hover());
        let p = ui.painter();
        p.rect_filled(rect, 4.0, t.counter_bg);
        p.rect_stroke(rect, 4.0, Stroke::new(1.0_f32, t.border_light), StrokeKind::Inside);
        let inner = rect.shrink2(vec2(12.0, 6.0));
        let sounding = self.ctl.statuses.values().filter(|s| s.state != PlayState::Paused).count();
        let right_label = if sounding > 0 { format!("{sounding} PLAYING") } else { "READY".to_owned() };
        p.text(inner.right_top(), Align2::RIGHT_TOP, right_label, theme::semibold(10.0), t.text_dim);
        match self.focus_voice() {
            Some((id, s)) => {
                let title = self.ctl.store.library.tile(id).map(|t| t.title.to_uppercase()).unwrap_or_default();
                let color = if s.state == PlayState::Paused { t.solo } else { t.counter_text };
                let galley = ui.fonts(|f| {
                    let mut job = egui::text::LayoutJob::simple_singleline(title, theme::semibold(10.0), t.text_dim);
                    job.wrap.max_width = inner.width() - 80.0;
                    job.wrap.max_rows = 1;
                    f.layout_job(job)
                });
                p.galley(inner.left_top(), galley, t.text_dim);
                let big = format!("−{}", format_clock(s.remaining(), true));
                theme::paint_tabular(p, inner.left_bottom() + vec2(0.0, 2.0), Align2::LEFT_BOTTOM, &big, theme::semibold(22.0), color);
                let small = format!("{} / {}", format_clock(s.position, true), format_clock(s.length, true));
                theme::paint_tabular(p, inner.right_bottom() + vec2(0.0, -3.0), Align2::RIGHT_BOTTOM, &small, theme::regular(11.5), theme::shade(color, 0.62));
            }
            None => {
                p.text(inner.left_top(), Align2::LEFT_TOP, "NOTHING PLAYING", theme::semibold(10.0), t.text_dim);
                theme::paint_tabular(p, inner.left_bottom() + vec2(0.0, 2.0), Align2::LEFT_BOTTOM, "0:00.0", theme::semibold(22.0), theme::shade(t.counter_text, 0.35));
            }
        }
    }

    /// Stereo master meter with the master volume as a draggable marker on its scale.
    fn master_section(&mut self, ui: &mut Ui, t: &Tokens) {
        let (rect, resp) = ui.allocate_exact_size(vec2(220.0, 46.0), Sense::click_and_drag());
        let resp = resp.on_hover_text("Master output level. Drag to set the master volume; click the clip light to clear it.");
        let bars = Rect::from_min_size(rect.min + vec2(10.0, 22.0), vec2(150.0, 13.0));
        let clip_light = Rect::from_center_size(pos2(bars.right() + 9.0, bars.center().y), vec2(6.0, 13.0));
        if let Some(pos) = resp.interact_pointer_pos() {
            if resp.clicked() && clip_light.expand(4.0).contains(pos) {
                self.meter.clip = false;
            } else if resp.dragged() || resp.clicked() {
                let frac = ((pos.x - bars.left()) / bars.width()).clamp(0.0, 1.0);
                let v = if frac <= 0.0 { 0.0 } else { db_to_gain(meter_db(frac)) };
                self.run(inv!("master.volume", {"volume": v}));
            }
        }
        let p = ui.painter();
        p.rect_filled(rect, 4.0, t.group);
        p.rect_stroke(rect, 4.0, Stroke::new(1.0_f32, t.border), StrokeKind::Inside);
        p.text(rect.left_top() + vec2(10.0, 6.0), Align2::LEFT_TOP, "MASTER", theme::semibold(10.0), t.text_dim);
        let green_end = meter_frac(-12.0);
        let yellow_end = meter_frac(-3.0);
        for (ch, level) in self.meter.level.iter().enumerate() {
            let row = Rect::from_min_size(bars.min + vec2(0.0, ch as f32 * 8.0), vec2(bars.width(), 5.0));
            p.rect_filled(row, 1.0, t.meter_off);
            // Unlit zones at 18%, as on SoundCraft's meters.
            for (from, to, c) in [(0.0, green_end, t.meter_green), (green_end, yellow_end, t.meter_yellow), (yellow_end, 1.0, t.meter_red)] {
                let zone = Rect::from_x_y_ranges(row.left() + from * row.width()..=row.left() + to * row.width(), row.y_range());
                p.rect_filled(zone, 0.0, theme::with_alpha(c, 0.18));
                let lit = meter_frac(*level).clamp(0.0, 1.0);
                if lit > from {
                    let end = lit.min(to);
                    p.rect_filled(Rect::from_x_y_ranges(row.left() + from * row.width()..=row.left() + end * row.width(), row.y_range()), 0.0, c);
                }
            }
            let hold = meter_frac(self.meter.hold[ch]);
            if hold > 0.0 {
                let c = if hold >= yellow_end { t.meter_red } else if hold >= green_end { t.meter_yellow } else { t.meter_green };
                p.rect_filled(Rect::from_min_size(pos2(row.left() + hold * row.width() - 1.0, row.top()), vec2(2.0, 5.0)), 0.0, c);
            }
        }
        p.rect_filled(clip_light, 1.5, if self.meter.clip { t.meter_red } else { theme::shade(t.meter_red, 0.25) });
        // Volume marker on the same scale.
        let v = self.ctl.store.library.master_volume;
        let vx = bars.left() + meter_frac(gain_to_db(v)) * bars.width();
        p.line_segment([pos2(vx, bars.top() - 3.0), pos2(vx, bars.bottom() + 1.0)], Stroke::new(2.0_f32, t.text));
        p.add(egui::Shape::convex_polygon(
            vec![pos2(vx - 4.0, bars.top() - 7.0), pos2(vx + 4.0, bars.top() - 7.0), pos2(vx, bars.top() - 2.0)],
            t.text,
            Stroke::NONE,
        ));
        let db = if v <= 0.0 { "−∞".to_owned() } else { format!("{:.1}", gain_to_db(v)).replace('-', "−") };
        theme::paint_tabular(p, pos2(rect.right() - 8.0, bars.center().y), Align2::RIGHT_CENTER, &db, theme::regular(12.0), t.text);
    }

    fn sets_panel(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let t = *Tokens::get(ui.ctx());
        ui.add_space(10.0);
        ui.label(RichText::new("SETS").font(theme::semibold(10.0)).color(t.text_dim));
        ui.add_space(4.0);
        let selected = self.ctl.store.library.selected_set;
        let n_sets = self.ctl.store.library.sets.len();
        egui::ScrollArea::vertical().auto_shrink([false, true]).max_height(ui.available_height() - 48.0).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            for set in self.ctl.store.library.sets.iter() {
                if let Some((id, draft, focused)) = &mut self.renaming {
                    if *id == set.id {
                        let resp = ui.add(egui::TextEdit::singleline(draft).desired_width(f32::INFINITY));
                        if !*focused {
                            resp.request_focus();
                            *focused = true;
                        }
                        if resp.lost_focus() {
                            let cancel = ui.input(|i| i.key_pressed(Key::Escape));
                            actions.push(if cancel { Action::CancelRename } else { Action::CommitRename });
                        }
                        continue;
                    }
                }
                let is_sel = Some(set.id) == selected;
                let playing = set.tiles.iter().any(|tile| self.ctl.statuses.contains_key(&tile.id));
                let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::click());
                let p = ui.painter();
                if is_sel {
                    p.rect_filled(rect, 4.0, t.selected);
                } else if resp.hovered() {
                    p.rect_filled(rect, 4.0, theme::with_alpha(t.text, 0.06));
                }
                if playing {
                    p.circle_filled(pos2(rect.left() + 14.0, rect.center().y), 4.0, t.counter_text);
                }
                let font = if is_sel { theme::semibold(13.0) } else { theme::regular(13.0) };
                let name = ui.fonts(|f| {
                    let mut job = egui::text::LayoutJob::simple_singleline(set.name.clone(), font, t.text);
                    job.wrap.max_width = rect.width() - 64.0;
                    job.wrap.max_rows = 1;
                    f.layout_job(job)
                });
                p.galley(pos2(rect.left() + 26.0, rect.center().y - name.size().y / 2.0), name, t.text);
                theme::paint_tabular(p, pos2(rect.right() - 10.0, rect.center().y), Align2::RIGHT_CENTER, &set.tiles.len().to_string(), theme::regular(11.5), t.text_dim);
                let resp = resp.on_hover_text("Double-click to rename");
                if resp.clicked() {
                    actions.push(inv!("set.select", {"set": set.id}).into());
                }
                if resp.double_clicked() {
                    actions.push(Action::StartRename(set.id));
                }
                resp.context_menu(|ui| {
                    if ui.button("Rename").clicked() {
                        actions.push(Action::StartRename(set.id));
                        ui.close_menu();
                    }
                    if ui.button("Duplicate").clicked() {
                        actions.push(inv!("set.duplicate", {"set": set.id}).into());
                        ui.close_menu();
                    }
                    if ui.add_enabled(n_sets > 1, egui::Button::new("Delete")).clicked() {
                        actions.push(inv!("set.delete", {"set": set.id}).into());
                        ui.close_menu();
                    }
                });
            }
        });
        ui.with_layout(egui::Layout::bottom_up(Align::LEFT), |ui| {
            ui.add_space(10.0);
            if theme::key(ui, "✚  New Set", vec2(104.0, 26.0), false).on_hover_text("⌘/Ctrl+Shift+N").clicked() {
                actions.push(inv!("set.create").into());
            }
        });
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        let t = *Tokens::get(ui.ctx());
        ui.horizontal(|ui| {
            ui.set_height(22.0);
            let playing = self.ctl.statuses.values().filter(|s| s.state != PlayState::Paused).count();
            let paused = self.ctl.statuses.len() - playing;
            let (rect, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
            let dot = if playing > 0 { t.counter_text } else if paused > 0 { t.solo } else { t.text_dim };
            ui.painter().circle_filled(rect.center(), 4.0, dot);
            let mut status = if playing > 0 { format!("{playing} playing") } else { "Idle".to_owned() };
            if paused > 0 {
                status.push_str(&format!(" · {paused} paused"));
            }
            ui.label(RichText::new(status).color(t.text));
            if self.ctl.store.is_loading() {
                ui.add_space(8.0);
                ui.spinner();
                ui.label(RichText::new("Loading audio…").color(t.text_dim));
            }
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                match (&self.ctl.engine.device_name, &self.ctl.engine.error) {
                    (_, Some(err)) => {
                        ui.colored_label(theme::ERROR, err);
                    }
                    (Some(name), None) => {
                        let khz = self.ctl.engine.sample_rate() as f32 / 1000.0;
                        ui.label(RichText::new(format!("Output: {name} · {khz} kHz")).color(t.text_dim));
                    }
                    (None, None) => {
                        ui.label(RichText::new("No output").color(t.text_dim));
                    }
                }
                if let Some(n) = &self.ctl.notice {
                    if n.at.elapsed() < Duration::from_secs(if n.error { 10 } else { 4 }) {
                        ui.add_space(16.0);
                        let color = if n.error { theme::ERROR } else { t.text };
                        ui.label(RichText::new(&n.text).color(color));
                    }
                }
            });
        });
    }

    fn settings_window(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let mut open = self.ctl.show_settings;
        egui::Window::new("Settings").open(&mut open).collapsible(false).resizable(false).default_width(420.0).show(ctx, |ui| {
            egui::Grid::new("settings-grid").num_columns(2).spacing([14.0, 10.0]).show(ui, |ui| {
                ui.label("Output device");
                let current = self.ctl.store.library.output_device.clone();
                egui::ComboBox::from_id_salt("device")
                    .width(260.0)
                    .selected_text(current.clone().unwrap_or_else(|| "System default".into()))
                    .show_ui(ui, |ui| {
                        if ui.selectable_label(current.is_none(), "System default").clicked() {
                            actions.push(inv!("device.set", {"name": null}).into());
                        }
                        for d in &self.ctl.devices {
                            if ui.selectable_label(current.as_deref() == Some(d), d).clicked() {
                                actions.push(inv!("device.set", {"name": d}).into());
                            }
                        }
                    });
                ui.end_row();

                ui.label("");
                ui.horizontal(|ui| {
                    if ui.button("Refresh list").clicked() {
                        actions.push(inv!("device.list").into());
                    }
                    if ui.button("Reconnect").clicked() {
                        actions.push(inv!("device.reconnect").into());
                    }
                });
                ui.end_row();

                ui.label("Stop All fade");
                let mut fade = self.ctl.store.library.stop_all_fade;
                if ui.add(egui::Slider::new(&mut fade, 0.0..=5.0).suffix(" s").step_by(0.1)).changed() {
                    actions.push(inv!("settings.stopAllFade", {"seconds": fade}).into());
                }
                ui.end_row();

                ui.label("Theme");
                ui.horizontal(|ui| {
                    let mut kind = self.ctl.store.library.theme;
                    ui.selectable_value(&mut kind, ThemeKind::Console, "Console");
                    ui.selectable_value(&mut kind, ThemeKind::Panel, crate::brand::PANEL_THEME);
                    if kind != self.ctl.store.library.theme {
                        actions.push(inv!("view.theme", {"theme": kind}).into());
                    }
                });
                ui.end_row();

                ui.label("Appearance");
                ui.horizontal(|ui| {
                    let mut appearance = self.ctl.store.library.appearance;
                    for (a, label) in [(Appearance::Dark, "Dark"), (Appearance::Light, "Light"), (Appearance::System, "System")] {
                        ui.selectable_value(&mut appearance, a, label);
                    }
                    if appearance != self.ctl.store.library.appearance {
                        actions.push(inv!("view.appearance", {"appearance": appearance}).into());
                    }
                });
                ui.end_row();

                ui.label("Tiles");
                let mut waveforms = self.ctl.store.library.show_waveforms;
                if ui.checkbox(&mut waveforms, "Show waveforms on tiles").changed() {
                    actions.push(inv!("view.waveforms", {"on": waveforms}).into());
                }
                ui.end_row();

                if cfg!(target_os = "linux") {
                    ui.label("Window system");
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            let mut x11 = self.ctl.store.library.use_x11;
                            ui.selectable_value(&mut x11, false, "Wayland");
                            ui.selectable_value(&mut x11, true, "X11 (XWayland)");
                            if x11 != self.ctl.store.library.use_x11 {
                                actions.push(inv!("settings.windowSystem", {"x11": x11}).into());
                            }
                        });
                        let now = if self.opts.x11 { "X11" } else { "Wayland" };
                        ui.label(
                            RichText::new(format!(
                                "Running on {now}; a change applies on the next launch. X11 lets you drag files in \
                                 from a file manager; Wayland is sharper with fractional scaling. Pasting \
                                 copied files (⌘/Ctrl+V) works on both."
                            ))
                            .small()
                            .color(Tokens::get(ui.ctx()).text_dim),
                        );
                    });
                    ui.end_row();
                }

                ui.label("Hotkeys");
                if ui.button("Reassign this set by position").on_hover_text("1–0, Q–P, A–;, Z–/ in tile order").clicked() {
                    actions.push(inv!("set.reassignKeys").into());
                }
                ui.end_row();

                ui.label("Library");
                ui.vertical(|ui| {
                    ui.add(egui::Label::new(RichText::new(self.ctl.store.library_path().display().to_string()).monospace().small()).wrap());
                    if ui.button("Open Folder").clicked() {
                        actions.push(inv!("library.reveal").into());
                    }
                });
                ui.end_row();
            });
        });
        self.ctl.show_settings = open;
    }

    // ── board ───────────────────────────────────────────────────────────────

    fn tile_interaction(&self, ui: &Ui, resp: &egui::Response, tile: &Tile, index: usize, actions: &mut Vec<Action>) {
        if self.ctl.edit_mode {
            if resp.clicked() {
                actions.push(inv!("view.select", {"tile": tile.id}).into());
            }
            resp.dnd_set_drag_payload(tile.id);
            if let Some(src) = resp.dnd_release_payload::<Uuid>() {
                if *src != tile.id {
                    actions.push(inv!("tile.move", {"tile": *src, "to": index + 1}).into());
                }
            }
        } else if resp.is_pointer_button_down_on() && ui.input(|i| i.pointer.primary_pressed()) {
            actions.push(Action::Press(tile.id));
        }
        resp.context_menu(|ui| {
            if ui.button("Edit…").clicked() {
                actions.push(inv!("view.select", {"tile": tile.id}).into());
                ui.close_menu();
            }
            let previewing = self.ctl.statuses.contains_key(&tile.id);
            if ui.add_enabled(tile.file.is_some(), egui::Button::new(if previewing { "Stop" } else { "Play" })).clicked() {
                actions.push(inv!("tile.preview", {"tile": tile.id}).into());
                ui.close_menu();
            }
            if self.ctl.current_set().layout == Layout::List && ui.button("Set as Next Cue").clicked() {
                actions.push(inv!("cue.set", {"index": index + 1}).into());
                ui.close_menu();
            }
            ui.separator();
            if ui.button("Replace Audio…").clicked() {
                actions.push(inv!("tile.replaceAudio", {"tile": tile.id}).into());
                ui.close_menu();
            }
            let picture_label = if tile.image.is_some() { "Change Picture…" } else { "Choose Picture…" };
            if ui.button(picture_label).on_hover_text("Or drop an image file onto the tile").clicked() {
                actions.push(inv!("tile.picture", {"tile": tile.id}).into());
                ui.close_menu();
            }
            if tile.image.is_some() && ui.button("Remove Picture").clicked() {
                actions.push(inv!("tile.pictureClear", {"tile": tile.id}).into());
                ui.close_menu();
            }
            if ui.button("Duplicate").clicked() {
                actions.push(inv!("tile.duplicate", {"tile": tile.id}).into());
                ui.close_menu();
            }
            if ui.button(RichText::new("Remove").color(theme::ERROR)).clicked() {
                actions.push(inv!("tile.remove", {"tile": tile.id}).into());
                ui.close_menu();
            }
        });
    }

    fn time_label(&self, tile: &Tile, status: Option<&VoiceStatus>) -> String {
        if let Some(s) = status {
            // Count down in whole seconds, rounding up so a sounding tile never shows 0:00.
            return format!("−{}", format_clock((s.remaining() - 1e-3).max(0.0).ceil(), false));
        }
        match self.ctl.store.media(tile) {
            None => "Drop audio here".into(),
            Some(Media::Loading) => "Loading…".into(),
            Some(Media::Failed(_)) => "⚠ Unreadable".into(),
            Some(Media::Ready { audio, .. }) => {
                let (s, e) = play_range(tile, audio.duration());
                // Short clips get tenths so they never read as 0:00.
                format_clock(e - s, e - s < 10.0)
            }
        }
    }

    fn grid(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let set = self.ctl.current_set();
        let gap = 10.0;
        let mut rects = Vec::new();
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            let avail = ui.available_width() - 2.0;
            let is_panel = theme::is_panel(ui.ctx());
            let min_w = if is_panel { panel::TILE_MIN_WIDTH } else { TILE_MIN_WIDTH };
            let cols = fit_columns(set.columns.max(1) as usize, avail, min_w, gap);
            let w = ((avail - gap * (cols as f32 - 1.0)) / cols as f32).floor().max(60.0);
            let h = if is_panel { panel::tile_height(w) } else { (w / 1.5).clamp(58.0, 170.0) };
            ui.spacing_mut().item_spacing = vec2(gap, gap);
            let count = set.tiles.len() + 1;
            for row in 0..count.div_ceil(cols) {
                ui.horizontal(|ui| {
                    for c in 0..cols {
                        let i = row * cols + c;
                        if i >= count {
                            break;
                        }
                        let (rect, resp) = ui.allocate_exact_size(vec2(w, h), Sense::click_and_drag());
                        if let Some(tile) = set.tiles.get(i) {
                            rects.push((tile.id, i, rect));
                            self.tile_interaction(ui, &resp, tile, i, actions);
                            let status = self.ctl.statuses.get(&tile.id);
                            let drop_hover = self.ctl.edit_mode && resp.dnd_hover_payload::<Uuid>().is_some();
                            let peaks = if self.ctl.store.library.show_waveforms { self.ctl.store.peaks(tile) } else { None };
                            let duration = self.ctl.store.audio(tile).map(|a| a.duration());
                            let paint = if is_panel { panel::paint_tile } else { paint_tile };
                            paint(ui, rect, tile, status, &self.time_label(tile, status), TileFlags {
                                selected: self.ctl.selected_tile == Some(tile.id),
                                drop_hover,
                                hovered: resp.hovered(),
                                failed: matches!(self.ctl.store.media(tile), Some(Media::Failed(_))),
                                waveform: peaks.as_deref().map(|p| p.as_slice()).zip(duration),
                                picture: self.tile_texture(tile),
                            });
                        } else {
                            if is_panel {
                                panel::paint_add_slot(ui, rect, resp.hovered());
                            } else {
                                paint_add_slot(ui, rect, resp.hovered());
                            }
                            if resp.clicked() {
                                actions.push(inv!("tile.add").into());
                            }
                            if let Some(src) = resp.dnd_release_payload::<Uuid>() {
                                actions.push(inv!("tile.move", {"tile": *src, "to": u32::MAX}).into());
                            }
                        }
                    }
                });
            }
        });
        self.tile_rects = rects;
    }

    fn list(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let t = *Tokens::get(ui.ctx());
        let si = self.ctl.set_idx();
        let set = &self.ctl.store.library.sets[si];
        let pointer = self.ctl.cue.get(&set.id).copied().unwrap_or(0);
        let next = next_cue(&set.tiles, pointer);
        let mut auto_advance = set.auto_advance;

        let is_panel = theme::is_panel(ui.ctx());
        if is_panel {
            let next_title = next.map(|i| set.tiles[i].title.clone());
            panel::go_header(ui, next_title, &mut auto_advance, actions);
        } else {
            ui.horizontal(|ui| {
                ui.add_enabled_ui(next.is_some(), |ui| {
                    let go = theme::key_with(ui, "GO", vec2(120.0, 48.0), t.playing, Color32::from_rgb(0x78, 0xFF, 0x8C), theme::bold(24.0));
                    if go.on_hover_text("Space").clicked() {
                        actions.push(inv!("cue.go").into());
                    }
                });
                ui.add_space(6.0);
                ui.vertical(|ui| {
                    ui.add_space(4.0);
                    ui.label(RichText::new("NEXT CUE").font(theme::semibold(10.0)).color(t.text_dim));
                    let title = next.map(|i| format!("{}.  {}", i + 1, set.tiles[i].title)).unwrap_or_else(|| "End of list".into());
                    ui.label(RichText::new(title).font(theme::semibold(18.0)).color(t.text));
                });
                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    if theme::key(ui, "⏮  Back to Top", vec2(124.0, 26.0), false).clicked() {
                        actions.push(inv!("cue.top").into());
                    }
                    ui.checkbox(&mut auto_advance, "Auto-advance").on_hover_text("When a cue finishes on its own, start the next one");
                });
            });
        }
        ui.add_space(10.0);

        let mut rects = Vec::new();
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            for (i, tile) in set.tiles.iter().enumerate() {
                let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::click_and_drag());
                rects.push((tile.id, i, rect));
                self.tile_interaction(ui, &resp, tile, i, actions);
                let status = self.ctl.statuses.get(&tile.id);
                let is_next = Some(i) == next;
                if is_panel {
                    let peaks = if self.ctl.store.library.show_waveforms { self.ctl.store.peaks(tile) } else { None };
                    let duration = self.ctl.store.audio(tile).map(|a| a.duration());
                    panel::paint_list_row(ui, rect, i, tile, status, &self.time_label(tile, status), is_next, TileFlags {
                        selected: self.ctl.selected_tile == Some(tile.id),
                        drop_hover: self.ctl.edit_mode && resp.dnd_hover_payload::<Uuid>().is_some(),
                        hovered: resp.hovered(),
                        failed: matches!(self.ctl.store.media(tile), Some(Media::Failed(_))),
                        waveform: peaks.as_deref().map(|p| p.as_slice()).zip(duration),
                        picture: self.tile_texture(tile),
                    });
                    continue;
                }
                let p = ui.painter();
                let base = theme::tile_fill(tile.color);
                let bg = if status.is_some() {
                    theme::shade(base, 0.34)
                } else if resp.hovered() {
                    theme::lighten(t.panel2, 0.04)
                } else {
                    t.panel2
                };
                p.rect_filled(rect, 5.0, bg);
                p.rect_filled(
                    Rect::from_min_size(rect.min, vec2(6.0, rect.height())),
                    CornerRadius { nw: 5, sw: 5, ne: 0, se: 0 },
                    theme::shade(base, 0.9),
                );
                let cy = rect.center().y;
                if is_next {
                    p.text(pos2(rect.left() + 14.0, cy), Align2::LEFT_CENTER, "▶", theme::bold(12.0), t.accent);
                }
                theme::paint_tabular(p, pos2(rect.left() + 48.0, cy), Align2::RIGHT_CENTER, &(i + 1).to_string(), theme::semibold(14.0), t.text_dim);
                let mut x = rect.left() + 58.0;
                if let Some(k) = tile.hotkey {
                    let cap = Rect::from_min_size(pos2(x, cy - 10.0), vec2(20.0, 20.0));
                    p.rect_filled(cap, 3.0, t.key);
                    p.rect_stroke(cap, 3.0, Stroke::new(1.0_f32, t.key_border), StrokeKind::Inside);
                    p.text(cap.center(), Align2::CENTER_CENTER, k.to_ascii_uppercase(), theme::bold(11.0), t.text);
                }
                x += 30.0;
                if let Some(tex) = self.tile_texture(tile) {
                    let thumb = Rect::from_min_size(pos2(x, cy - 15.0), vec2(30.0, 30.0));
                    let [w, h] = tex.size();
                    let uv = cover_uv(w as f32 / h.max(1) as f32, 1.0);
                    p.add(egui::epaint::RectShape::filled(thumb, 4.0, Color32::WHITE).with_texture(tex.id(), uv));
                    x += 38.0;
                }
                // Right side: time, then badges, then a mini waveform between title and badges.
                let time = self.time_label(tile, status);
                let time_rect = theme::paint_tabular(p, pos2(rect.right() - 14.0, cy), Align2::RIGHT_CENTER, &time, theme::semibold(13.0), t.text);
                let mut right = time_rect.left() - 16.0;
                let badges = mode_badge_text(tile);
                if !badges.is_empty() {
                    let g = p.layout_no_wrap(badges, theme::bold(9.0), t.text_dim);
                    right -= g.size().x;
                    p.galley(pos2(right, cy - g.size().y / 2.0), g, t.text_dim);
                    right -= 16.0;
                }
                let title_w = (rect.width() * 0.32).min(right - x);
                let title = ui.fonts(|f| {
                    let mut job = egui::text::LayoutJob::simple_singleline(tile.title.clone(), theme::semibold(14.0), t.text);
                    job.wrap.max_width = title_w.max(10.0);
                    job.wrap.max_rows = 1;
                    f.layout_job(job)
                });
                p.galley(pos2(x, cy - title.size().y / 2.0), title, t.text);
                let wave = Rect::from_min_max(pos2(x + title_w + 16.0, rect.top() + 8.0), pos2(right, rect.bottom() - 8.0));
                if let (true, Some(peaks), Some(audio)) = (self.ctl.store.library.show_waveforms, self.ctl.store.peaks(tile), self.ctl.store.audio(tile)) {
                    let played_x = status.map(|s| wave.left() + wave.width() * s.progress());
                    let rest = theme::with_alpha(theme::lighten(base, 0.35), if status.is_some() { 0.55 } else { 0.7 });
                    paint_waveform(p, wave, tile, &peaks, audio.duration(), played_x, theme::lighten(base, 0.75), rest);
                    if let Some(px) = played_x {
                        p.line_segment([pos2(px, wave.top()), pos2(px, wave.bottom())], Stroke::new(1.0_f32, t.playhead));
                    }
                }
                let outline = if self.ctl.selected_tile == Some(tile.id) || (self.ctl.edit_mode && resp.dnd_hover_payload::<Uuid>().is_some()) {
                    Some(Stroke::new(2.0_f32, t.accent))
                } else if status.is_some_and(|s| s.state == PlayState::Paused) {
                    Some(Stroke::new(2.0_f32, t.solo))
                } else if status.is_some() {
                    Some(Stroke::new(2.0_f32, t.counter_text))
                } else if is_next {
                    Some(Stroke::new(1.0_f32, t.accent))
                } else {
                    None
                };
                if let Some(s) = outline {
                    p.rect_stroke(rect, 5.0, s, StrokeKind::Inside);
                }
            }
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::click());
            if is_panel {
                panel::paint_add_slot(ui, rect, resp.hovered());
            } else {
                paint_add_slot(ui, rect, resp.hovered());
            }
            if resp.clicked() {
                actions.push(inv!("tile.add").into());
            }
            if let Some(src) = resp.dnd_release_payload::<Uuid>() {
                actions.push(inv!("tile.move", {"tile": *src, "to": u32::MAX}).into());
            }
        });
        self.tile_rects = rects;
        if auto_advance != self.ctl.store.library.sets[si].auto_advance {
            actions.push(inv!("set.autoAdvance", {"on": auto_advance}).into());
        }
    }

    // ── inspector ───────────────────────────────────────────────────────────

    fn inspector(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let Some(id) = self.ctl.selected_tile else { return };
        let Some(orig) = self.ctl.store.library.tile(id).cloned() else {
            actions.push(inv!("view.select").into());
            return;
        };
        let mut t = orig.clone();
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("TILE").small().strong().weak());
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                if ui.small_button("🗙").on_hover_text("Close").clicked() {
                    actions.push(inv!("view.select").into());
                }
            });
        });
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            ui.add(egui::TextEdit::singleline(&mut t.title).font(FontId::proportional(16.0)).desired_width(f32::INFINITY));

            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 5.0;
                for c in TileColor::ALL {
                    let (rect, resp) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::click());
                    ui.painter().circle_filled(rect.center(), 8.0, theme::tile_fill(c));
                    if t.color == c {
                        ui.painter().circle_stroke(rect.center(), 9.5, Stroke::new(2.0_f32, Color32::WHITE));
                    }
                    if resp.on_hover_text(format!("{c:?}")).clicked() {
                        t.color = c;
                    }
                }
            });

            ui.separator();
            egui::Grid::new("inspector-grid").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
                ui.label("Hotkey");
                ui.horizontal(|ui| {
                    let label = if self.capturing_hotkey {
                        "Press a key…".to_owned()
                    } else {
                        t.hotkey.map(|c| c.to_ascii_uppercase().to_string()).unwrap_or_else(|| "None".into())
                    };
                    let btn = egui::Button::new(RichText::new(label).monospace()).min_size(vec2(90.0, 0.0));
                    if ui.add(btn).on_hover_text("Click, then press a key").clicked() {
                        actions.push(Action::CaptureHotkey);
                    }
                    if t.hotkey.is_some() && ui.small_button("Clear").clicked() {
                        actions.push(inv!("tile.set", {"tile": id, "hotkey": null}).into());
                    }
                });
                ui.end_row();

                ui.label("Mode");
                egui::ComboBox::from_id_salt("mode").width(160.0).selected_text(t.mode.label()).show_ui(ui, |ui| {
                    for m in PlayMode::ALL {
                        ui.selectable_value(&mut t.mode, m, m.label()).on_hover_text(m.help());
                    }
                });
                ui.end_row();
                ui.label("");
                ui.add(egui::Label::new(RichText::new(t.mode.help()).small().weak()).wrap());
                ui.end_row();

                ui.label("Options");
                ui.vertical(|ui| {
                    ui.checkbox(&mut t.looping, "Loop");
                    ui.checkbox(&mut t.solo, "Solo").on_hover_text("Starting this tile fades out all others");
                });
                ui.end_row();

                ui.label("Volume");
                ui.add(egui::Slider::new(&mut t.volume, 0.0..=1.0).custom_formatter(|v, _| {
                    if v <= 0.0 {
                        "−∞ dB".into()
                    } else {
                        format!("{:.1} dB", 20.0 * v.log10())
                    }
                }));
                ui.end_row();

                ui.label("Fade in");
                ui.add(egui::Slider::new(&mut t.fade_in, 0.0..=10.0).step_by(0.1).suffix(" s"));
                ui.end_row();
                ui.label("Fade out");
                ui.add(egui::Slider::new(&mut t.fade_out, 0.0..=10.0).step_by(0.1).suffix(" s"));
                ui.end_row();
            });

            ui.separator();
            ui.label(RichText::new("TRIM").small().strong().weak());
            self.waveform_editor(ui, &mut t);
            if let Some(dur) = self.ctl.store.audio(&t).map(|a| a.duration()) {
                let (s, e) = play_range(&t, dur);
                let (mut start, mut end) = (s, e);
                ui.horizontal(|ui| {
                    ui.label("Start");
                    ui.add(egui::DragValue::new(&mut start).range(0.0..=dur).speed(0.01).max_decimals(2).suffix(" s"));
                    ui.label("End");
                    ui.add(egui::DragValue::new(&mut end).range(0.0..=dur).speed(0.01).max_decimals(2).suffix(" s"));
                    if ui.small_button("Reset").clicked() {
                        start = 0.0;
                        end = dur;
                    }
                });
                if start != s || end != e {
                    t.start = start.min(end - 0.01).max(0.0);
                    t.end = if (dur - end).abs() < 1e-3 { None } else { Some(end.max(t.start + 0.01)) };
                }
                ui.weak(format!("Plays {} of {}", format_clock(e - s, true), format_clock(dur, true)));
            }

            ui.separator();
            ui.label(RichText::new("PICTURE").small().strong().weak());
            self.picture_editor(ui, &mut t, actions);

            ui.separator();
            ui.horizontal(|ui| {
                let playing = self.ctl.statuses.contains_key(&id);
                if ui.add_enabled(t.file.is_some(), egui::Button::new(if playing { "⏹ Stop" } else { "▶ Preview" })).clicked() {
                    actions.push(inv!("tile.preview", {"tile": id}).into());
                }
                if ui.button("Replace Audio…").clicked() {
                    actions.push(inv!("tile.replaceAudio", {"tile": id}).into());
                }
            });
            if let Some(Media::Failed(e)) = self.ctl.store.media(&t) {
                ui.colored_label(theme::ERROR, format!("Can't decode this file: {e}"));
            }
            ui.add_space(8.0);
            if ui.button(RichText::new("Remove Tile").color(theme::ERROR)).clicked() {
                actions.push(inv!("tile.remove", {"tile": id}).into());
            }
        });
        if t != orig {
            actions.push(Action::Run(tile_changes(&orig, &t)));
        }
    }

    fn picture_editor(&self, ui: &mut Ui, t: &mut Tile, actions: &mut Vec<Action>) {
        let id = t.id;
        if t.image.is_none() {
            ui.weak("No picture. Choose one, or drop an image file onto the tile.");
            if ui.button("Choose Picture…").clicked() {
                actions.push(inv!("tile.picture", {"tile": id}).into());
            }
            return;
        }
        ui.horizontal(|ui| {
            // A small preview at the tile's own aspect, so Fill vs Fit is visible.
            let (rect, _) = ui.allocate_exact_size(vec2(96.0, 64.0), Sense::hover());
            let p = ui.painter_at(rect);
            p.rect_filled(rect, 6.0, theme::tile_fill(t.color));
            match (self.tile_texture(t), self.ctl.store.picture_state(t)) {
                (Some(tex), _) => paint_picture(&p, rect, 6.0, t, tex),
                (None, Some(PictureState::Failed(e))) => {
                    p.text(rect.center(), Align2::CENTER_CENTER, "⚠", FontId::proportional(18.0), theme::ERROR);
                    ui.colored_label(theme::ERROR, format!("Can't show this picture: {e}"));
                }
                _ => {
                    p.text(rect.center(), Align2::CENTER_CENTER, "…", FontId::proportional(16.0), Color32::WHITE);
                }
            }
            ui.vertical(|ui| {
                if t.image_from_art {
                    ui.weak("Cover art from the audio file");
                }
                ui.horizontal(|ui| {
                    if ui.button("Choose…").clicked() {
                        actions.push(inv!("tile.picture", {"tile": id}).into());
                    }
                    if ui.button("Remove").clicked() {
                        actions.push(inv!("tile.pictureClear", {"tile": id}).into());
                    }
                });
            });
        });
        egui::Grid::new("picture-grid").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            ui.label("Layer");
            ui.horizontal(|ui| {
                ui.selectable_value(&mut t.image_layer, ImageLayer::Underlay, "Under")
                    .on_hover_text("Behind the waveform and progress");
                ui.selectable_value(&mut t.image_layer, ImageLayer::Overlay, "Over")
                    .on_hover_text("On top of the waveform and progress (the title stays on top)");
            });
            ui.end_row();
            ui.label("Fit");
            ui.horizontal(|ui| {
                ui.selectable_value(&mut t.image_fit, ImageFit::Fill, "Fill").on_hover_text("Cover the whole tile");
                ui.selectable_value(&mut t.image_fit, ImageFit::Fit, "Fit").on_hover_text("Show the whole picture; good for icons");
            });
            ui.end_row();
            ui.label("Opacity");
            ui.add(egui::Slider::new(&mut t.image_opacity, 0.0..=1.0).custom_formatter(|v, _| format!("{:.0}%", v * 100.0)));
            ui.end_row();
        });
    }

    fn waveform_editor(&self, ui: &mut Ui, t: &mut Tile) {
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 84.0), Sense::click_and_drag());
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 6.0, Color32::from_black_alpha(90));
        let (Some(peaks), Some(audio)) = (self.ctl.store.peaks(t), self.ctl.store.audio(t)) else {
            let msg = if t.file.is_none() { "No audio" } else { "Loading waveform…" };
            p.text(rect.center(), Align2::CENTER_CENTER, msg, FontId::proportional(13.0), ui.visuals().weak_text_color());
            return;
        };
        let dur = audio.duration().max(1e-6);
        let color = theme::lighten(theme::tile_fill(t.color), 0.25);
        let mid = rect.center().y;
        let half = rect.height() / 2.0 - 4.0;
        let width = rect.width().max(1.0) as usize;
        for x in 0..width {
            let b = x * peaks.len() / width;
            let h = (peaks[b] * half).max(0.5);
            let px = rect.left() + x as f32 + 0.5;
            p.line_segment([pos2(px, mid - h), pos2(px, mid + h)], Stroke::new(1.0_f32, color));
        }
        let (s, e) = play_range(t, dur);
        let x_of = |sec: f64| rect.left() + (sec / dur) as f32 * rect.width();
        let shade = Color32::from_black_alpha(150);
        p.rect_filled(Rect::from_x_y_ranges(rect.left()..=x_of(s), rect.y_range()), 0.0, shade);
        p.rect_filled(Rect::from_x_y_ranges(x_of(e)..=rect.right(), rect.y_range()), 0.0, shade);
        for x in [x_of(s), x_of(e)] {
            p.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], Stroke::new(2.0_f32, theme::TRIM));
            p.rect_filled(Rect::from_center_size(pos2(x, rect.top() + 6.0), vec2(8.0, 12.0)), 2.0, theme::TRIM);
        }
        if let Some(st) = self.ctl.statuses.get(&t.id) {
            let x = x_of(s + st.position);
            p.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], Stroke::new(1.5_f32, Color32::WHITE));
        }

        // Drag whichever handle is nearer to where the drag started.
        let handle_id = resp.id.with("handle");
        if let Some(pos) = resp.interact_pointer_pos() {
            if resp.drag_started() || resp.clicked() {
                let start_nearer = (pos.x - x_of(s)).abs() <= (pos.x - x_of(e)).abs();
                ui.ctx().data_mut(|d| d.insert_temp(handle_id, start_nearer));
            }
            if resp.dragged() || resp.clicked() {
                let start_handle = ui.ctx().data(|d| d.get_temp::<bool>(handle_id)).unwrap_or(true);
                let sec = (((pos.x - rect.left()) / rect.width()) as f64 * dur).clamp(0.0, dur);
                if start_handle {
                    t.start = sec.min(e - 0.01).max(0.0);
                } else {
                    let end = sec.max(s + 0.01);
                    t.end = if dur - end < 1e-3 { None } else { Some(end) };
                }
            }
        }
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
    }

    fn console_panels(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let t = *Tokens::get(ctx);
        let panel = |fill: Color32, margin: egui::Margin| egui::Frame::new().fill(fill).inner_margin(margin);
        egui::TopBottomPanel::top("toolbar")
            .frame(panel(t.panel, egui::Margin { left: 14, right: 14, top: 8, bottom: 8 }).stroke(Stroke::new(1.0_f32, t.border)))
            .show(ctx, |ui| self.toolbar(ui, actions));
        egui::TopBottomPanel::bottom("status")
            .frame(panel(t.bar, egui::Margin { left: 12, right: 12, top: 2, bottom: 2 }).stroke(Stroke::new(1.0_f32, t.border)))
            .show(ctx, |ui| self.status_bar(ui));
        egui::SidePanel::left("sets")
            .frame(panel(t.panel2, egui::Margin { left: 10, right: 10, top: 0, bottom: 0 }).stroke(Stroke::new(1.0_f32, t.border)))
            .resizable(true)
            .default_width(208.0)
            .width_range(150.0..=320.0)
            .show(ctx, |ui| self.sets_panel(ui, actions));
        if self.ctl.selected_tile.is_some() {
            egui::SidePanel::right("inspector")
                .frame(panel(t.panel2, egui::Margin { left: 12, right: 12, top: 0, bottom: 0 }).stroke(Stroke::new(1.0_f32, t.border)))
                .resizable(true)
                .default_width(320.0)
                .width_range(260.0..=480.0)
                .show(ctx, |ui| self.inspector(ui, actions));
        }
        egui::CentralPanel::default().frame(panel(t.window, egui::Margin::same(16))).show(ctx, |ui| match self.ctl.current_set().layout {
            Layout::Grid => self.grid(ui, actions),
            Layout::List => self.list(ui, actions),
        });
    }

    /// The Panel layout: a fixed frame, with the panels sized to fit inside it.
    fn panel_panels(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let area = ctx.available_rect();
        panel::paint_frame(ctx, area, crate::brand::PANEL_SUBTITLE);
        let none = egui::Frame::NONE;
        egui::TopBottomPanel::top("toolbar")
            .frame(none)
            .exact_height(panel::TOP_H)
            .show_separator_line(false)
            .show(ctx, |ui| self.panel_toolbar(ui, area, actions));
        egui::TopBottomPanel::bottom("status")
            .frame(none)
            .exact_height(panel::BOTTOM_H)
            .show_separator_line(false)
            .show(ctx, |ui| self.panel_status(ui, area));
        egui::SidePanel::left("sets")
            .frame(none)
            .exact_width(panel::LEFT_W)
            .resizable(false)
            .show_separator_line(false)
            .show(ctx, |ui| self.panel_sets(ui, area, actions));
        if self.ctl.selected_tile.is_some() {
            egui::SidePanel::right("inspector")
                .frame(none.inner_margin(egui::Margin { left: 14, right: 12, top: 4, bottom: 0 }).stroke(Stroke::new(2.0_f32, theme::PANEL.lilac)))
                .resizable(true)
                .default_width(320.0)
                .width_range(260.0..=480.0)
                .show(ctx, |ui| self.inspector(ui, actions));
        }
        egui::CentralPanel::default()
            .frame(none.inner_margin(egui::Margin { left: 8, right: 16, top: 6, bottom: 8 }))
            .show(ctx, |ui| match self.ctl.current_set().layout {
                Layout::Grid => self.grid(ui, actions),
                Layout::List => self.list(ui, actions),
            });
    }

    fn drop_overlay(&self, ctx: &egui::Context) {
        let hovering = ctx.input(|i| !i.raw.hovered_files.is_empty());
        if !hovering {
            return;
        }
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("drop-overlay")));
        let screen = ctx.screen_rect();
        painter.rect_filled(screen, 0.0, Color32::from_black_alpha(140));
        painter.rect_stroke(screen.shrink(12.0), 12.0, Stroke::new(3.0_f32, theme::ACCENT), StrokeKind::Inside);
        painter.text(
            screen.center(),
            Align2::CENTER_CENTER,
            "Drop audio to add tiles\n(drop one file on a tile to replace its sound)",
            FontId::proportional(22.0),
            Color32::WHITE,
        );
    }

    /// `LEADSTINGER_SNAPSHOT`: after `snapshot_frames` frames, save a screenshot and quit.
    /// `view.screenshot` saves one without quitting. Both arrive as Screenshot events.
    fn snapshot(&mut self, ctx: &egui::Context) {
        if let Some(path) = self.opts.snapshot.clone() {
            ctx.request_repaint();
            if self.frame >= self.opts.snapshot_frames && !self.snapshot_requested {
                self.snapshot_requested = true;
                self.shots.push((path, true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            }
        }
        if self.shots.is_empty() {
            return;
        }
        ctx.request_repaint();
        let shot = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(img) = shot {
            let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
            let mut quit = false;
            for (path, then_quit) in std::mem::take(&mut self.shots) {
                match image::save_buffer(&path, &bytes, img.size[0] as u32, img.size[1] as u32, image::ColorType::Rgba8) {
                    Ok(()) => eprintln!("{}: saved screenshot to {}", crate::brand::SLUG, path.display()),
                    Err(e) => self.ctl.notify_error(format!("Couldn't save the screenshot: {e}")),
                }
                quit |= then_quit;
            }
            if quit {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }
}

impl eframe::App for StingerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.ctl.0 = Some(self.lender.borrow());
        self.frame_update(ctx);
        self.lender.give_back(self.ctl.0.take().expect("borrowed for this frame"));
    }
}

impl StingerApp {
    fn frame_update(&mut self, ctx: &egui::Context) {
        self.frame += 1;
        let now = Instant::now();
        let dt = (now - self.last_frame).as_secs_f64();
        self.last_frame = now;
        self.ctl.tick_now();
        self.meter.feed(self.ctl.engine.take_peaks(), dt as f32);
        self.prepare_textures(ctx);
        self.run_script();
        self.handle_effects(ctx);

        let mut actions = Vec::new();
        let menu_state = self.menu_state();
        let mut menu_commands = Vec::new();
        #[cfg(target_os = "macos")]
        if !self.opts.in_window_menu {
            // Installed after the first frames: winit sets up its default menu once the
            // app has finished launching, and ours must replace it.
            if self.native_menu.is_none() && self.frame >= 2 {
                self.native_menu = Some(crate::native_menu::NativeMenu::install(ctx, &menu_state));
            }
            if let Some(nm) = &mut self.native_menu {
                nm.sync(&menu_state);
                menu_commands.extend(nm.take_commands());
            }
        }
        self.handle_shortcuts(ctx, &mut actions);
        self.handle_hotkeys(ctx, &mut actions);
        self.handle_file_drop(ctx);
        self.handle_paste(ctx);

        let t = *Tokens::get(ctx);
        let panel = |fill: Color32, margin: egui::Margin| egui::Frame::new().fill(fill).inner_margin(margin);
        if !cfg!(target_os = "macos") || self.opts.in_window_menu {
            // Linux and Windows: the menu bar lives at the top of the window.
            let menus = menu::menus(&menu_state, false);
            egui::TopBottomPanel::top("menubar")
                .frame(panel(t.bar, egui::Margin { left: 8, right: 8, top: 2, bottom: 2 }))
                .show(ctx, |ui| {
                    if let Some(c) = menu::menu_bar(ui, &menus) {
                        menu_commands.push(c);
                    }
                });
        }
        if theme::is_panel(ctx) {
            self.panel_panels(ctx, &mut actions);
        } else {
            self.console_panels(ctx, &mut actions);
        }
        if self.ctl.show_settings {
            self.settings_window(ctx, &mut actions);
        }
        if self.ctl.show_about {
            self.about_window(ctx);
        }
        if self.ctl.show_shortcuts {
            self.shortcuts_window(ctx);
        }
        actions.extend(menu_commands.into_iter().map(|c: Cmd| Action::Run(c.into())));
        self.drop_overlay(ctx);

        // Drag ghost while reordering.
        if let (Some(id), Some(pos)) = (egui::DragAndDrop::payload::<Uuid>(ctx), ctx.pointer_hover_pos()) {
            if let Some(tile) = self.ctl.store.library.tile(*id) {
                let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("drag-ghost")));
                let r = Rect::from_min_size(pos + vec2(8.0, 8.0), vec2(150.0, 32.0));
                painter.rect_filled(r, 6.0, theme::tile_fill(tile.color).gamma_multiply(0.9));
                painter.text(r.left_center() + vec2(8.0, 0.0), Align2::LEFT_CENTER, &tile.title, FontId::proportional(14.0), Color32::WHITE);
            }
        }

        // A tile pressed with the mouse is released when the button comes up anywhere.
        if let Some(id) = self.mouse_held {
            if !ctx.input(|i| i.pointer.primary_down()) {
                self.mouse_held = None;
                actions.push(inv!("tile.trigger", {"tile": id, "event": "release"}).into());
            }
        }

        for a in actions {
            self.apply(a);
        }
        self.handle_effects(ctx);

        if ctx.input(|i| i.viewport().close_requested()) {
            if let Err(e) = self.ctl.store.save_now() {
                eprintln!("{}: failed to save library: {e:#}", crate::brand::SLUG);
            }
        } else if let Err(e) = self.ctl.store.save_if_due() {
            self.ctl.notify_error(format!("Couldn't save the library: {e:#}"));
        }

        if !self.ctl.statuses.is_empty() || self.mouse_held.is_some() || !self.meter.idle() {
            ctx.request_repaint_after(Duration::from_millis(16));
        } else if self.ctl.store.is_loading() || self.ctl.store.has_unsaved() || self.ctl.notice.is_some() {
            ctx.request_repaint_after(Duration::from_millis(200));
        }
        self.snapshot(ctx);
    }
}

/// Floor of the meter scale.
const METER_FLOOR_DB: f32 = -80.0;

/// SoundCraft's meter curve: −80…−40 dB fills the bottom 10 %, −40…−20 the next 20 %,
/// −20…0 the top 70 %.
fn meter_frac(db: f32) -> f32 {
    let db = db.clamp(METER_FLOOR_DB, 0.0);
    if db < -40.0 {
        (db + 80.0) / 40.0 * 0.1
    } else if db < -20.0 {
        0.1 + (db + 40.0) / 20.0 * 0.2
    } else {
        0.3 + (db + 20.0) / 20.0 * 0.7
    }
}

/// Inverse of [`meter_frac`].
fn meter_db(frac: f32) -> f32 {
    let f = frac.clamp(0.0, 1.0);
    if f < 0.1 {
        f / 0.1 * 40.0 - 80.0
    } else if f < 0.3 {
        (f - 0.1) / 0.2 * 20.0 - 40.0
    } else {
        (f - 0.3) / 0.7 * 20.0 - 20.0
    }
}



/// Master meter display state, with SoundCraft-style ballistics: instant attack,
/// ~26 dB/s release, a 2 s peak hold and a clip light that stays lit until clicked.
struct Meter {
    level: [f32; 2],
    hold: [f32; 2],
    hold_at: [Instant; 2],
    clip: bool,
}

impl Meter {
    fn new() -> Self {
        Meter { level: [METER_FLOOR_DB; 2], hold: [METER_FLOOR_DB; 2], hold_at: [Instant::now(); 2], clip: false }
    }

    #[allow(clippy::needless_range_loop)] // one index across four per-channel arrays
    fn feed(&mut self, peaks: [f32; 2], dt: f32) {
        for ch in 0..2 {
            let db = gain_to_db(peaks[ch]);
            self.level[ch] = db.max(self.level[ch] - 26.0 * dt);
            if self.level[ch] >= self.hold[ch] || self.hold_at[ch].elapsed() > Duration::from_secs(2) {
                self.hold[ch] = self.level[ch];
                self.hold_at[ch] = Instant::now();
            }
            if peaks[ch] > 1.0 {
                self.clip = true;
            }
        }
    }

    fn idle(&self) -> bool {
        self.level.iter().chain(self.hold.iter()).all(|db| *db <= METER_FLOOR_DB + 0.5)
    }
}

/// The app's mark: a small 3×2 grid of tile colours beside the name.
/// The tile mark and the app's name; `compact` draws the mark alone.
fn paint_wordmark(ui: &mut Ui, compact: bool) {
    let t = Tokens::get(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(if compact { 22.0 } else { 150.0 }, 28.0), Sense::hover());
    let p = ui.painter();
    let colors = [TileColor::Red, TileColor::Orange, TileColor::Green, TileColor::Blue, TileColor::Purple, TileColor::Teal];
    for (i, c) in colors.iter().enumerate() {
        let r = Rect::from_min_size(rect.left_center() + vec2((i % 3) as f32 * 7.0, -9.0 + (i / 3) as f32 * 10.0), vec2(6.0, 8.0));
        p.rect_filled(r, 1.5, theme::tile_fill(*c));
    }
    if compact {
        return;
    }
    // The logo's three parts (src/brand.rs), the middle one in the accent colour.
    let (a, colon, b) = crate::brand::LOGO;
    let mut x = rect.left() + 28.0;
    for (part, color) in [(a, t.text), (colon, t.accent), (b, t.text)] {
        let r = p.text(pos2(x, rect.center().y), Align2::LEFT_CENTER, part, theme::bold(17.0), color);
        x = r.right() + if part == colon { 0.5 } else { 0.0 };
    }
}

fn key_char(key: Key) -> Option<char> {
    let c = match key {
        Key::Num0 => '0',
        Key::Num1 => '1',
        Key::Num2 => '2',
        Key::Num3 => '3',
        Key::Num4 => '4',
        Key::Num5 => '5',
        Key::Num6 => '6',
        Key::Num7 => '7',
        Key::Num8 => '8',
        Key::Num9 => '9',
        Key::Semicolon => ';',
        Key::Comma => ',',
        Key::Period => '.',
        Key::Slash => '/',
        k => {
            let name = k.name();
            let mut chars = name.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) if c.is_ascii_alphabetic() => c.to_ascii_lowercase(),
                _ => return None,
            }
        }
    };
    Some(c)
}

/// The inspector's edits as one `tile.set`, naming only the settings that changed.
fn tile_changes(orig: &Tile, t: &Tile) -> Invocation {
    let mut m = serde_json::Map::new();
    m.insert("tile".into(), json!(t.id));
    let mut put = |k: &str, changed: bool, v: Value| {
        if changed {
            m.insert(k.into(), v);
        }
    };
    put("title", t.title != orig.title, json!(t.title));
    put("color", t.color != orig.color, json!(t.color));
    put("mode", t.mode != orig.mode, json!(t.mode));
    put("volume", t.volume != orig.volume, json!(t.volume));
    put("fadeIn", t.fade_in != orig.fade_in, json!(t.fade_in));
    put("fadeOut", t.fade_out != orig.fade_out, json!(t.fade_out));
    put("loop", t.looping != orig.looping, json!(t.looping));
    put("solo", t.solo != orig.solo, json!(t.solo));
    put("trimStart", t.start != orig.start, json!(t.start));
    put("trimEnd", t.end != orig.end, json!(t.end));
    put("pictureLayer", t.image_layer != orig.image_layer, json!(t.image_layer));
    put("pictureFit", t.image_fit != orig.image_fit, json!(t.image_fit));
    put("pictureOpacity", t.image_opacity != orig.image_opacity, json!(t.image_opacity));
    Invocation::new("tile.set", Value::Object(m))
}

struct TileFlags<'a> {
    selected: bool,
    drop_hover: bool,
    hovered: bool,
    failed: bool,
    /// Waveform peaks and the file's duration, when the waveform should be drawn.
    waveform: Option<(&'a [f32], f64)>,
    picture: Option<&'a egui::TextureHandle>,
}

/// Texture coordinates that crop a picture of `img_aspect` (w/h) to fill a box of
/// `box_aspect`, centred.
fn cover_uv(img_aspect: f32, box_aspect: f32) -> Rect {
    if img_aspect > box_aspect {
        let u = box_aspect / img_aspect;
        Rect::from_min_max(pos2((1.0 - u) / 2.0, 0.0), pos2((1.0 + u) / 2.0, 1.0))
    } else {
        let v = img_aspect / box_aspect;
        Rect::from_min_max(pos2(0.0, (1.0 - v) / 2.0), pos2(1.0, (1.0 + v) / 2.0))
    }
}

/// Draw `tex` into `rect` at the tile's picture settings. Fill covers the area
/// (cropped, following `corners`); Fit shows the whole picture centred.
fn paint_picture(p: &egui::Painter, rect: Rect, corners: impl Into<CornerRadius>, tile: &Tile, tex: &egui::TextureHandle) {
    let [w, h] = tex.size();
    if w == 0 || h == 0 {
        return;
    }
    // Premultiplied in gamma space, as egui blends (see theme::with_alpha).
    let tint = theme::with_alpha(Color32::WHITE, tile.image_opacity);
    let img_aspect = w as f32 / h as f32;
    match tile.image_fit {
        ImageFit::Fill => {
            let uv = cover_uv(img_aspect, rect.width() / rect.height().max(1.0));
            p.add(egui::epaint::RectShape::filled(rect, corners, tint).with_texture(tex.id(), uv));
        }
        ImageFit::Fit => {
            let inner = rect.shrink(8.0);
            let scale = (inner.width() / w as f32).min(inner.height() / h as f32);
            let r = Rect::from_center_size(inner.center(), vec2(w as f32 * scale, h as f32 * scale));
            p.image(tex.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), tint);
        }
    }
}

/// The trimmed region's waveform, mirrored, as one anti-aliased mesh with a column
/// per point (SoundCraft draws waveforms the same way). Columns left of `played_x`
/// use `played`; the rest use `rest`.
#[allow(clippy::too_many_arguments)]
fn paint_waveform(
    p: &egui::Painter,
    area: Rect,
    tile: &Tile,
    peaks: &[f32],
    duration: f64,
    played_x: Option<f32>,
    played: Color32,
    rest: Color32,
) {
    if peaks.is_empty() || duration <= 0.0 || area.width() < 2.0 {
        return;
    }
    let (s, e) = play_range(tile, duration);
    let mid = area.center().y;
    let half = area.height() / 2.0 * 0.92;
    let columns = area.width().floor() as usize;
    let mut mesh = egui::Mesh::default();
    for i in 0..=columns {
        let x = area.left() + i as f32;
        let t = s + (e - s) * i as f64 / columns as f64;
        let b = ((t / duration) * peaks.len() as f64) as usize;
        let h = (peaks[b.min(peaks.len() - 1)] * half).max(0.6);
        let c = if played_x.is_some_and(|px| x <= px) { played } else { rest };
        mesh.colored_vertex(pos2(x, mid - h), c);
        mesh.colored_vertex(pos2(x, mid + h), c);
        if i > 0 {
            let v = (i * 2) as u32;
            mesh.add_triangle(v - 2, v - 1, v);
            mesh.add_triangle(v - 1, v + 1, v);
        }
    }
    p.add(mesh);
}

/// Badge text for the name bar, e.g. "HOLD · LOOP".
fn mode_badge_text(tile: &Tile) -> String {
    let mut parts = Vec::new();
    match tile.mode {
        PlayMode::Restart => parts.push("RESTART"),
        PlayMode::Hold => parts.push("HOLD"),
        PlayMode::Pause => parts.push("PAUSE"),
        PlayMode::Toggle => {}
    }
    if tile.looping {
        parts.push("LOOP");
    }
    if tile.solo {
        parts.push("SOLO");
    }
    parts.join(" · ")
}

/// The narrowest Console tile the grid will draw; below it the grid shows fewer columns.
const TILE_MIN_WIDTH: f32 = 140.0;

/// How many grid columns to draw: the set's `columns`, or fewer when tiles that wide
/// would be narrower than `min_w`. The saved setting is left alone, so widening the
/// window brings the columns back.
fn fit_columns(want: usize, avail: f32, min_w: f32, gap: f32) -> usize {
    let fit = ((avail + gap) / (min_w + gap)).floor().max(1.0) as usize;
    want.clamp(1, fit)
}

/// A tile drawn like a DAW clip: a coloured name bar (colour ×0.9) over a dark body
/// (colour ×0.34) holding the waveform tinted toward white. While it plays, the played
/// part of the waveform is brighter and a red playhead marks the position.
fn paint_tile(ui: &Ui, rect: Rect, tile: &Tile, status: Option<&VoiceStatus>, time: &str, f: TileFlags) {
    let t = Tokens::get(ui.ctx());
    let p = ui.painter();
    let radius = 6.0;
    let r = radius as u8;
    let sounding = status.is_some_and(|s| matches!(s.state, PlayState::Playing | PlayState::Stopping));
    let paused = status.is_some_and(|s| s.state == PlayState::Paused);
    let mut base = theme::tile_fill(tile.color);
    if tile.file.is_none() {
        base = theme::shade(base, 0.6);
    }
    let bar_h = 28.0_f32.min(rect.height() * 0.32);
    let bar = Rect::from_min_size(rect.min, vec2(rect.width(), bar_h));
    let body = Rect::from_min_max(pos2(rect.left(), bar.bottom()), rect.max);
    let body_corners = CornerRadius { nw: 0, ne: 0, sw: r, se: r };

    p.rect_filled(rect, radius, theme::shade(base, if f.hovered && !sounding { 0.40 } else { 0.34 }));
    if let (Some(tex), ImageLayer::Underlay) = (f.picture, tile.image_layer) {
        paint_picture(p, body, body_corners, tile, tex);
    }
    let wave = Rect::from_min_max(pos2(body.left() + 8.0, body.top() + 6.0), pos2(body.right() - 8.0, body.bottom() - 26.0));
    let played_x = status.map(|s| wave.left() + wave.width() * s.progress());
    if let Some(px) = played_x {
        let wash = Rect::from_min_max(body.min, pos2(px, body.bottom()));
        p.rect_filled(wash, CornerRadius { nw: 0, ne: 0, sw: r, se: 0 }, theme::with_alpha(Color32::WHITE, 0.06));
    }
    match f.waveform {
        Some((peaks, duration)) => {
            let rest = theme::with_alpha(theme::lighten(base, 0.35), if status.is_some() { 0.55 } else { 0.85 });
            paint_waveform(p, wave, tile, peaks, duration, played_x, theme::lighten(base, 0.75), rest);
        }
        None if tile.file.is_none() => {
            p.text(wave.center(), Align2::CENTER_CENTER, "Drop audio here", theme::regular(12.0), theme::lighten(base, 0.45));
        }
        None => {}
    }
    if let Some(px) = played_x {
        p.line_segment([pos2(px, body.top() + 2.0), pos2(px, body.bottom() - 2.0)], Stroke::new(1.0_f32, t.playhead));
    }
    if let (Some(tex), ImageLayer::Overlay) = (f.picture, tile.image_layer) {
        paint_picture(p, body, body_corners, tile, tex);
    }

    // Name bar: hotkey cap, title, mode badges.
    p.rect_filled(bar, CornerRadius { nw: r, ne: r, sw: 0, se: 0 }, theme::shade(base, 0.9));
    let ink = Color32::from_rgb(0x0C, 0x0C, 0x0C);
    let mut x = bar.left() + 6.0;
    if let Some(k) = tile.hotkey {
        let cap = Rect::from_min_size(pos2(x, bar.center().y - 9.0), vec2(18.0, 18.0));
        p.rect_filled(cap, 3.0, theme::with_alpha(Color32::BLACK, 0.35));
        p.text(cap.center(), Align2::CENTER_CENTER, k.to_ascii_uppercase(), theme::bold(11.0), Color32::WHITE);
        x = cap.right() + 6.0;
    }
    // A narrow tile drops the badges rather than squeeze the title to nothing.
    let badges = mode_badge_text(tile);
    let badge = (!badges.is_empty()).then(|| p.layout_no_wrap(badges, theme::bold(9.0), theme::with_alpha(ink, 0.6)));
    let badge_w = match badge {
        Some(g) if bar.right() - 16.0 - g.size().x - x >= 64.0 => {
            let w = g.size().x;
            p.galley(pos2(bar.right() - 8.0 - w, bar.center().y - g.size().y / 2.0), g, ink);
            w + 8.0
        }
        _ => 0.0,
    };
    let title = ui.fonts(|fonts| {
        let mut job = egui::text::LayoutJob::simple_singleline(tile.title.clone(), theme::semibold(13.0), ink);
        job.wrap.max_width = (bar.right() - 8.0 - badge_w - x).max(10.0);
        job.wrap.max_rows = 1;
        fonts.layout_job(job)
    });
    p.galley(pos2(x, bar.center().y - title.size().y / 2.0), title, ink);

    // Footer: state on the left, time on the right.
    let foot = body.bottom() - 13.0;
    if sounding || paused {
        let (icon, c) = if paused { ("⏸", t.solo) } else { ("▶", t.counter_text) };
        p.text(pos2(body.left() + 10.0, foot), Align2::LEFT_CENTER, icon, theme::bold(11.0), c);
    }
    let time_color = if f.failed {
        theme::ERROR
    } else if status.is_some() {
        Color32::WHITE
    } else {
        theme::lighten(base, 0.6)
    };
    theme::paint_tabular(p, pos2(body.right() - 10.0, foot), Align2::RIGHT_CENTER, time, theme::semibold(13.0), time_color);

    let outline = if f.drop_hover || f.selected {
        Stroke::new(2.0_f32, t.accent)
    } else if sounding {
        Stroke::new(2.0_f32, t.counter_text)
    } else if paused {
        Stroke::new(2.0_f32, t.solo)
    } else {
        Stroke::new(1.0_f32, Color32::from_rgb(0x0A, 0x0A, 0x0A))
    };
    p.rect_stroke(rect, radius, outline, StrokeKind::Inside);
}

fn paint_add_slot(ui: &Ui, rect: Rect, hovered: bool) {
    let t = Tokens::get(ui.ctx());
    let p = ui.painter();
    let color = if hovered { t.text } else { t.text_dim };
    if hovered {
        p.rect_filled(rect, 6.0, theme::with_alpha(t.text, 0.04));
    }
    // Dashed outline.
    let r = rect.shrink(1.0);
    let stroke = Stroke::new(1.0_f32, if hovered { t.text_dim } else { t.border_light });
    for (a, b) in [(r.left_top(), r.right_top()), (r.right_top(), r.right_bottom()), (r.right_bottom(), r.left_bottom()), (r.left_bottom(), r.left_top())] {
        p.extend(egui::Shape::dashed_line(&[a, b], stroke, 6.0, 4.0));
    }
    p.text(rect.center(), Align2::CENTER_CENTER, "✚  Add Sounds", theme::regular(13.0), color);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_drops_columns_only_when_tiles_get_too_narrow() {
        // 4 × 250 px fits in 1030 px with 10 px gaps: keep the set's 4.
        assert_eq!(fit_columns(4, 1030.0, 150.0, 10.0), 4);
        // A 720 px window leaves ~470 px: 3 tiles of 150 px.
        assert_eq!(fit_columns(4, 470.0, 150.0, 10.0), 3);
        // 940 px with the inspector leaves ~380 px: 2 tiles.
        assert_eq!(fit_columns(4, 380.0, 150.0, 10.0), 2);
        // Never fewer than one column, never more than asked for.
        assert_eq!(fit_columns(4, 40.0, 150.0, 10.0), 1);
        assert_eq!(fit_columns(2, 2000.0, 150.0, 10.0), 2);
        assert_eq!(fit_columns(0, 2000.0, 150.0, 10.0), 1);
    }

    #[test]
    fn maps_layout_keys() {
        assert_eq!(key_char(Key::A), Some('a'));
        assert_eq!(key_char(Key::Num0), Some('0'));
        assert_eq!(key_char(Key::Semicolon), Some(';'));
        assert_eq!(key_char(Key::Escape), None);
        assert_eq!(key_char(Key::F1), None);
        for c in crate::logic::KEY_LAYOUT.chars() {
            let found = Key::ALL.iter().any(|k| key_char(*k) == Some(c));
            assert!(found, "no egui key produces {c:?}");
        }
    }
}

#[cfg(test)]
mod glyph_tests {
    use eframe::egui::{self, FontId};

    /// Every symbol the UI draws must exist in egui's bundled fonts, or it renders as a box.
    #[test]
    fn ui_glyphs_exist_in_bundled_fonts() {
        let source = [include_str!("app.rs"), include_str!("menu.rs")].concat();
        let ctx = egui::Context::default();
        let _ = ctx.run(Default::default(), |_| {});
        let mut missing: Vec<char> = source
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .flat_map(|l| l.chars())
            .filter(|c| !c.is_ascii() && !c.is_alphabetic())
            .filter(|c| !ctx.fonts(|f| f.has_glyphs(&FontId::proportional(14.0), &c.to_string())))
            .collect();
        missing.dedup();
        assert!(missing.is_empty(), "glyphs missing from egui fonts: {missing:?}");
    }
}
