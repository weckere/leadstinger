//! The egui front end. UI code collects [`Action`]s while drawing; they are applied
//! after the frame's panels are laid out, which keeps borrows simple.

use crate::audio::{Engine, VoiceStatus};
use crate::decode::AUDIO_EXTENSIONS;
use crate::logic::{
    db_to_gain, format_clock, gain_to_db, meter_frac, next_cue, play_range, Ballistics, PadKey, PlayState,
};
use crate::model::{ZOOM_MAX, ZOOM_MIN, ActionKind, CueLink, SPEED_MAX, SPEED_MIN, STOP_ALL_HOLD, Appearance, Duck, FadeCurve, GridMode, ImageFit, ImageLayer, Layout, ThemeKind, Tile, TileColor, DUCK_DB_MIN, GAIN_DB_MAX};
use crate::picture::{is_image_path, IMAGE_EXTENSIONS};
use crate::store::{Media, PictureState, Store};
use crate::commands::{Cmd, Invocation};
use eframe::egui::accesskit::Role as AxRole;
use crate::control::{self, Lender};
use crate::controller::{Controller, Effect};
use std::sync::Arc;
use crate::inv;
use crate::rows::{self, Grid, Place};
use crate::menu;
use serde_json::{json, Value};
use crate::theme::{self, Tokens};
use eframe::egui::{
    self, pos2, vec2, Align, Align2, Color32, CornerRadius, FontId, Key, KeyboardShortcut, Modifiers, Pos2, Rect, RichText, Sense,
    Stroke, StrokeKind, Ui, Vec2,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use uuid::Uuid;

mod panel;
mod routing;
mod messages_ui;
mod pads_ui;
mod export_ui;
mod quip_ui;
mod quip_browser;
mod quip_connect;
mod update_ui;
mod tips;
mod grid;
mod pad_options;
use pad_options::PadOption;
mod editor;
mod inspector;
mod windows;
#[cfg(target_os = "ios")]
mod touch;
#[cfg(test)]
mod a11y_tests;

/// Shown in the window title, the toolbar logo and the menus (see brand.rs).
pub use crate::brand::APP_NAME;

/// How long `view.screenshot` lets the ui run before capturing: at least this many ui
/// frames and this much time. While the window is hidden (egui 0.34+), a single frame
/// would show windows opened in the meantime still invisible (egui sizes a window on its
/// first frame) and closed ones half faded; 150 ms covers the fades.
/// The smallest window (main.rs), in points: the layouts are made to fit it, so when a
/// zoomed-in window holds fewer points the layout keeps this size and scrolls.
pub const LAYOUT_MIN: [f32; 2] = [720.0, 480.0];
/// The inspector's width when the app opens (egui doesn't keep a dragged width across
/// launches), and how far it can be dragged (D-203, D-211). Every row fits 240 in both
/// looks; narrower, the Panel look's Play tab no longer would.
const INSPECTOR_WIDTH: f32 = 260.0;
const INSPECTOR_MIN_WIDTH: f32 = 240.0;
const INSPECTOR_MAX_WIDTH: f32 = 520.0;

const SHOT_SETTLE: (u32, Duration) = (2, Duration::from_millis(150));

/// A captured frame and the paths it's saved to (`true`: quit once saved).
type Shot = (Vec<(PathBuf, bool)>, Arc<egui::ColorImage>);
/// Where captured frames go.
type ShotSender = std::sync::mpsc::Sender<Shot>;

/// Startup options, mostly for automated screenshots and demos.
#[derive(Default)]
pub struct Options {
    /// Save a screenshot here after `snapshot_frames` frames, then quit.
    pub snapshot: Option<PathBuf>,
    pub snapshot_frames: u64,
    /// Comma-separated UI steps run on startup: `play:N`, `select:N`, `also:N` (⌘-click), `focus:N`, `list`,
    /// `grid`, `set:N`, `settings`, `import:PATH`, `paste`, `find:TEXT`, `picture:N:PATH`, `overlay:N`, `fit:N`,
    /// `editor:N`, `vamp:N`, `marker:N`, `ezoom:DOUBLINGS`, `record:SECONDS` (a recording that only shows), `scroll:POINTS[@X.Y]` (over that point, else the board),
    /// `blank:ROW.SLOT` (1-based), `drag:N@X.Y` (tile N held over that point, still down; `altdrag:` with Alt held), `files:K@X.Y` (K sound
    /// files held over it), `tab:NAME` (the inspector's tab), `hide:sidebar|inspector`, `lock`, `unlock`, and
    /// `wait:FRAMES` to pause (e.g. until imported audio has decoded).
    pub script: Vec<String>,
    /// Frame before which the next script step must not run.
    pub script_resume_frame: u64,
    /// Draw the menu bar inside the window even on macOS (`LEADSTINGER_IN_WINDOW_MENU=1`).
    pub in_window_menu: bool,
    /// The window runs on X11 or XWayland rather than native Wayland (Linux).
    pub x11: bool,
    /// Tests: no control socket, MIDI, OSC or system-wide keys (src/app/a11y_tests.rs).
    pub headless: bool,
    /// Out of the way of whoever is at the computer (src/background.rs): never takes the
    /// focus, and keeps painting while hidden so screenshots are fresh.
    pub background: bool,
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
            headless: false,
            background: crate::background::on(),
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
    /// Enter or Space on a focused tile: trigger it, and release it when the key comes
    /// up, as its hotkey would, except that on the grid it stops a playing One-shot pad
    /// (D-196).
    KeyPress(Uuid, Key),
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
    /// iPadOS: the touches down, each with the tile it plays, and the one egui emulates
    /// the pointer with (app/touch.rs).
    #[cfg(target_os = "ios")]
    touches: touch::Touches,
    /// Where the board (the central panel) is; windows first open just inside it, clear
    /// of the toolbar and side panels.
    board_rect: Rect,
    /// The inspector's width last frame (0 while it's hidden), for the tests (D-211).
    inspector_width: f32,
    keys_held: HashMap<Key, Uuid>,
    /// With the hold guard on: when Esc went down, and whether Stop All has run for it.
    esc_down: Option<(Instant, bool)>,
    capturing_hotkey: bool,
    /// `view.search` opened the search box: focus it on the next frame it's drawn.
    search_focus: bool,
    tile_rects: Vec<(Uuid, usize, Rect)>,
    /// The grid as drawn in Rows mode, for working out where files and tiles land.
    row_grid: Option<(grid::Layout, Grid<usize>)>,
    /// Files are being dragged over the grid in Rows mode (the board shows where).
    file_marker: bool,
    /// Where each pad and ghost of a Rows grid is drawn, while they glide to a drag
    /// preview's places (D-141).
    slot_anim: HashMap<egui::Id, Rect>,
    /// Until when the board keeps gliding after a drag (the drop settling).
    anim_until: Option<Instant>,
    /// The pointer was on the status bar's notice last frame: it stays while it's read.
    notice_hovered: bool,
    /// Where the pointer took hold of the pad being dragged, from its corner.
    drag_grab: Option<Vec2>,
    /// A tile the board scrolls to on its next draw (a recording started in it).
    reveal: Option<Uuid>,
    /// The `scroll:POINTS[@X.Y]` script step: a wheel turn the next frame gets, and where.
    script_scroll: Option<(f32, Pos2)>,
    /// The `drag:`/`files:` script steps: (files?, tile or file count, point, frame).
    script_drag: Option<(bool, usize, Pos2, u32)>,
    script_drag_from: Option<Pos2>,
    /// The drag is an `altdrag:` (Alt held).
    script_drag_alt: bool,
    /// Tile pictures uploaded to the GPU, by media file name.
    textures: HashMap<String, egui::TextureHandle>,
    meter: Meter,
    /// When ⌘/Ctrl+V last imported (or tried to), so one keystroke imports once.
    last_paste: Option<Instant>,
    /// Screenshots asked for (`view.screenshot`, or `LEADSTINGER_SNAPSHOT`) and not yet
    /// requested from egui; `true` quits afterwards.
    shots: Vec<(PathBuf, bool)>,
    /// Captures requested and not yet saved.
    shots_pending: usize,
    /// Captures whose saving ends a `LEADSTINGER_SNAPSHOT` run, still to come: it quits
    /// once the last is saved.
    quit_after: usize,
    /// `view.screenshot {window}` shots of set windows, waiting for the ui to settle.
    window_shots: Vec<(Uuid, PathBuf)>,
    /// A `view.screenshot` waiting for the ui to settle: the ui frames run since it was
    /// asked for, and when. See [`SHOT_SETTLE`].
    shot_settle: Option<(u32, Instant)>,
    /// Captured frames with the paths they go to, sent by egui's screenshot callback once
    /// the frame (of whichever window) is rendered.
    shot_tx: ShotSender,
    shot_rx: std::sync::mpsc::Receiver<Shot>,
    #[cfg(target_os = "macos")]
    native_menu: Option<crate::native_menu::NativeMenu>,
    last_frame: Instant,
    frame: u64,
    opts: Options,
    snapshot_requested: bool,
    /// System-wide keys (macOS, X11), registered from this thread.
    hotkeys: Option<crate::hotkeys::Hotkeys>,
    /// The library revision the hotkeys were last synced at.
    hotkeys_synced: u64,
    /// Wayland: system-wide keys through the desktop's global-shortcuts portal.
    #[cfg(target_os = "linux")]
    portal: Option<crate::portal::PortalKeys>,
    /// Native Wayland: file drags, which winit doesn't deliver there (D-108).
    wayland_drops: Option<crate::wayland_drop::WaylandDrops>,
    /// Where a Finder drag is (macOS).
    mac_drops: Option<crate::mac_drop::MacDrops>,
    /// File ▸ Export Audio…, while it's open.
    export: Option<export_ui::ExportDialog>,
    /// Settings ▸ QuipClipper's form and server list.
    quip: quip_ui::QuipSettings,
    /// The QuipClipper window's folder, search and listing (`view.quip`).
    quip_browser: quip_browser::QuipBrowser,
    /// The tile or cue row with keyboard focus last frame (M20, D-090).
    focused_tile: Option<Uuid>,
    /// A `focus:N` script step: give that tile keyboard focus on the next frame.
    focus_script: Option<Uuid>,
    /// A `setmenu:N` script step: hold that set's right-click menu open (screenshots).
    set_menu_script: Option<Uuid>,
    /// The editor's waveform, which takes − and = (zoom) while it has the focus.
    editor_wave: Option<egui::Id>,
    /// The Settings zoom slider's value while it's dragged (applied when the drag ends).
    zoom_draft: Option<u32>,
    /// Set files opened from the file manager, waiting for Import or Cancel; the first is
    /// asked about.
    confirm_sets: Vec<PathBuf>,
    /// Themes set files brought, waiting for Use it or Not now: (id, name).
    confirm_themes: Vec<(String, String)>,
    /// The QuipClipper first-contact prompt, while it's up (src/app/quip_connect.rs).
    quip_prompt: Option<quip_connect::QuipPrompt>,
    /// QuipClipper servers Cancel was clicked for since the app started: their links are
    /// refused instead of asking again.
    quip_declined: Vec<String>,
    /// The waveform editor's zoom, scroll and peak pyramid, while it's open.
    editor_view: Option<editor::EditorView>,
    /// The last Check for Updates answer the window took (src/app/update_ui.rs).
    update_seen: u64,
    /// Its dialog, while open: a newer release, or "latest" after a menu check.
    update_dialog: Option<crate::update::Outcome>,
    /// The sets open in windows of their own, while they're open (src/app/windows.rs).
    set_wins: HashMap<Uuid, windows::SetWin>,
    /// The app icon, for set windows.
    window_icon: Arc<egui::IconData>,
}

impl StingerApp {
    pub fn new(cc: &eframe::CreationContext<'_>, store: Store, engine: Engine, opts: Options) -> Self {
                // ⌘/Ctrl + and − run view.zoom, which saves the zoom in the library; egui's own
        // zoom keys would change it without saving.
        cc.egui_ctx.options_mut(|o| o.zoom_with_keyboard = false);
        cc.egui_ctx.set_zoom_factor(store.library.zoom as f32 / 100.0);
        let data_dir = store.data_dir.clone();
        let appearance = store.library.appearance;
        let mut ctl = Controller::new(store, engine);
        theme::apply(&cc.egui_ctx, appearance, &ctl.look);
        ctl.x11 = opts.x11;
        let lender = Lender::new(ctl);
        crate::quip::set_waker(cc.egui_ctx.clone());
        let control = if opts.headless {
            None
        } else {
            match control::serve(lender.clone(), &data_dir, Some(cc.egui_ctx.clone())) {
                Ok(server) => Some(server),
                Err(e) => {
                    lender.with(|c| c.notify_error(format!("{} can't reach this window: {e}", crate::brand::CLI_NAME)));
                    None
                }
            }
        };
        // The launch check, only when Settings turned it on (D-206): never in screenshot runs.
        if opts.snapshot.is_none() && !opts.headless {
            lender.with(|c| {
                if c.store.library.check_updates {
                    c.updates.start(crate::update::Show::IfNewer);
                }
            });
        }
        // Screenshot runs stay away from the system's MIDI ports and keys.
        let mut hotkeys = None;
        #[cfg(target_os = "linux")]
        let mut portal = None;
        if opts.snapshot.is_none() && !opts.headless {
            crate::midi::start(lender.clone(), Some(cc.egui_ctx.clone()));
            crate::osc::start(lender.clone(), Some(cc.egui_ctx.clone()));
            match crate::hotkeys::Hotkeys::start(lender.clone(), Some(cc.egui_ctx.clone())) {
                Ok(h) => hotkeys = Some(h),
                Err(note) => {
                    lender.with(|c| c.hotkey_status = crate::hotkeys::HotkeyStatus { note: Some(note), ..Default::default() });
                    // Wayland: ask the desktop's global-shortcuts portal instead.
                    #[cfg(target_os = "linux")]
                    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
                        portal = Some(crate::portal::PortalKeys::start(lender.clone(), Some(cc.egui_ctx.clone())));
                    }
                }
            }
        }
        if opts.background {
            crate::background::hide_window(cc);
        }
        let (shot_tx, shot_rx) = std::sync::mpsc::channel();
        StingerApp {
            lender,
            ctl: Lent(None),
            _control: control,
            renaming: None,
            mouse_held: None,
            #[cfg(target_os = "ios")]
            touches: Default::default(),
            board_rect: Rect::ZERO,
            inspector_width: 0.0,
            keys_held: HashMap::new(),
            esc_down: None,
            capturing_hotkey: false,
            search_focus: false,
            hotkeys,
            hotkeys_synced: u64::MAX,
            #[cfg(target_os = "linux")]
            portal,
            wayland_drops: crate::wayland_drop::WaylandDrops::install(cc),
            mac_drops: crate::mac_drop::MacDrops::install(cc),
            tile_rects: Vec::new(),
            row_grid: None,
            file_marker: false,
            slot_anim: HashMap::new(),
            anim_until: None,
            notice_hovered: false,
            drag_grab: None,
            reveal: None,
            script_scroll: None,
            script_drag: None,
            script_drag_from: None,
            script_drag_alt: false,
            textures: HashMap::new(),
            meter: Meter::new(),
            last_paste: None,
            shots: Vec::new(),
            shots_pending: 0,
            quit_after: 0,
            window_shots: Vec::new(),
            shot_settle: None,
            shot_tx,
            shot_rx,
            #[cfg(target_os = "macos")]
            native_menu: None,
            last_frame: Instant::now(),
            frame: 0,
            opts,
            snapshot_requested: false,
            export: None,
            quip: Default::default(),
            quip_browser: Default::default(),
            focused_tile: None,
            focus_script: None,
            set_menu_script: None,
            editor_wave: None,
            zoom_draft: None,
            confirm_sets: Vec::new(),
            confirm_themes: Vec::new(),
            quip_prompt: None,
            quip_declined: Vec::new(),
            editor_view: None,
            update_seen: 0,
            update_dialog: None,
            set_wins: HashMap::new(),
            window_icon: Arc::new(theme::app_icon()),
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
                    Effect::ApplyLook => theme::apply_look(ctx, self.ctl.store.library.appearance, &self.ctl.look),
                    Effect::PickAudio { set, place } => {
                        let paths = Self::pick_audio_files(true);
                        if !paths.is_empty() {
                            let mut params = grid::place_json(place);
                            params["paths"] = json!(paths);
                            params["set"] = json!(set);
                            self.run(Invocation::new("tile.add", params));
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
                    Effect::PickSetFile => {
                        let file = rfd::FileDialog::new()
                            .set_title("Import Set")
                            .add_filter("Set files", &[crate::setfile::EXTENSION, "zip"])
                            .add_filter("Farrago set", &[crate::farrago::EXTENSION, "zip"])
                            .pick_file();
                        if let Some(path) = file {
                            self.run(inv!("set.import", {"path": path}));
                        }
                    }
                    Effect::PickTheme => {
                        let file = rfd::FileDialog::new()
                            .set_title("Import Theme")
                            .add_filter("Themes", &[crate::themes::EXTENSION, "zip", "json"])
                            .pick_file();
                        if let Some(path) = file {
                            self.run(inv!("theme.import", {"path": path}));
                        }
                    }
                    Effect::SaveTheme(id) => {
                        let name = if self.ctl.look.id == id { self.ctl.look.name.clone() } else { id.clone() };
                        let file = rfd::FileDialog::new()
                            .set_title("Export Theme")
                            .set_file_name(format!("{}.{}", name.replace(['/', '\\', ':'], "-"), crate::themes::EXTENSION))
                            .add_filter("Themes", &[crate::themes::EXTENSION])
                            .save_file();
                        if let Some(path) = file {
                            self.run(inv!("theme.export", {"path": path, "theme": id}));
                        }
                    }
                    Effect::SaveSetFile(set) => {
                        let lib = &self.ctl.store.library;
                        let name = set.and_then(|id| lib.set_index(id)).map_or_else(|| crate::app::APP_NAME.to_owned(), |i| lib.sets[i].name.clone());
                        let file = rfd::FileDialog::new()
                            .set_title("Export Set")
                            .set_file_name(format!("{}.{}", name.replace(['/', '\\', ':'], "-"), crate::setfile::EXTENSION))
                            .add_filter("Set files", &[crate::setfile::EXTENSION])
                            .save_file();
                        if let Some(path) = file {
                            match set {
                                Some(id) => self.run(inv!("set.export", {"path": path, "set": id})),
                                None => self.run(inv!("set.export", {"path": path, "all": true})),
                            }
                        }
                    }
                    Effect::StartRename(id) => {
                        if let Some(i) = self.ctl.store.library.set_index(id) {
                            self.renaming = Some((id, self.ctl.store.library.sets[i].name.clone(), false));
                        }
                    }
                    Effect::Screenshot { path, window } => {
                        match window {
                            Some(set) => self.window_shots.push((set, path)),
                            None => self.shots.push((path, false)),
                        }
                        self.shot_settle = Some((0, Instant::now()));
                    }
                    Effect::Window { window, op } => self.window_effect(ctx, window, op),
                    Effect::ClearClip => self.meter.clip = false,
                    Effect::FocusSearch => self.search_focus = true,
                    Effect::ExportAudio { tiles, set } => self.export = Some(export_ui::ExportDialog::new(tiles, set)),
                    Effect::ConfirmSetFile(path) => {
                        if !self.confirm_sets.contains(&path) {
                            self.confirm_sets.push(path);
                        }
                    }
                    Effect::ConfirmTheme { id, name } => {
                        self.confirm_themes.retain(|(i, _)| *i != id);
                        self.confirm_themes.push((id, name));
                    }
                    Effect::ConfirmQuip(ask) => self.offer_quip(*ask),
                    Effect::Reveal(id) => {
                        self.reveal = Some(id);
                        ctx.request_repaint();
                    }
                    Effect::Raise => {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                        if !self.opts.background {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                        }
                    }
                    Effect::ReleaseHeld => {
                        self.mouse_held = None;
                        self.keys_held.clear();
                        self.release_held_in_windows();
                    }
                    Effect::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                }
            }
        }
    }

    // ── input ───────────────────────────────────────────────────────────────

    fn menu_state(&mut self) -> menu::State {
        let user_themes = self.ctl.installed_themes();
        let set = self.ctl.current_set();
        let pointer = self.ctl.cue.get(&set.id).copied().unwrap_or(0);
        menu::State {
            sidebar: self.ctl.store.library.show_sidebar,
            inspector: self.ctl.store.library.show_inspector,
            locked: set.locked,
            layout: set.layout,
            columns: set.columns,
            grid: self.ctl.store.library.grid,
            even_rows: even_rows(&set.row_lengths()),
            waveforms: self.ctl.store.library.show_waveforms,
            meters: self.ctl.store.library.show_meters,
            appearance: self.ctl.store.library.appearance,
            theme: self.ctl.store.library.theme,
            theme_id: self.ctl.look.id.clone(),
            user_themes,
            can_delete_set: self.ctl.store.library.sets.len() > 1,
            has_next_cue: next_cue(&set.tiles, pointer).is_some(),
            anything_playing: !self.ctl.statuses.is_empty(),
            duck_hold: self.ctl.engine.duck_hold(),
            recording: self.ctl.recording.is_some(),
            input: self.ctl.input_name(),
            importing: self.ctl.store.importing().next().is_some(),
            replay: self.ctl.store.library.replay_seconds > 0,
            can_undo: self.ctl.history.undo_label().is_some(),
            can_redo: self.ctl.history.redo_label().is_some(),
            zoom: self.ctl.store.library.zoom,
            editor: self.ctl.editor.is_some(),
            can_edit_wave: self.ctl.selected_tile.and_then(|id| self.ctl.store.library.tile(id)).is_some_and(|t| t.action.is_none() && t.file.is_some()),
            set_window: self.ctl.store.library.set_windows.iter().any(|w| w.set == set.id),
            windows_open: !self.ctl.store.library.set_windows.is_empty(),
        }
    }

    /// Keyboard shortcuts, from the menu model so they always match the menus.
    fn handle_shortcuts(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let mut table = menu::shortcuts(&menu::menus(&self.menu_state(), cfg!(target_os = "macos")));
        // ⌘/Ctrl+= is Zoom In's menu key; a keypad or layout that types + zooms in too.
        table.push((KeyboardShortcut::new(Modifiers::COMMAND, Key::Plus), Cmd::with("view.zoom", crate::commands::Arg::Str("in"))));
        let hits: Vec<Cmd> = ctx.input_mut(|i| table.iter().filter(|(s, _)| i.consume_shortcut(s)).map(|(_, c)| *c).collect());
        actions.extend(hits.into_iter().map(|c| Action::Run(c.into())));
    }

    /// ⌘/Ctrl+Up/Down (or Left/Right) on a focused tile move it one place earlier or
    /// later, instead of dragging it; the master volume keeps those keys otherwise, and
    /// in a locked set.
    fn handle_move_keys(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let Some(id) = self.focused_tile.filter(|id| !self.tile_locked(*id) && self.ctl.search.is_none()) else { return };
        if ctx.memory(|m| m.focused()) != Some(egui::Id::unique(("tile", id))) && ctx.memory(|m| m.focused()) != Some(egui::Id::unique(("cue-row", id))) {
            return;
        }
        let Some((si, ti)) = self.ctl.store.library.locate(id) else { return };
        let set = &self.ctl.store.library.sets[si];
        if set.layout == Layout::Grid && self.ctl.store.library.grid == GridMode::Rows {
            let keys = [Key::ArrowLeft, Key::ArrowRight, Key::ArrowUp, Key::ArrowDown];
            let Some(key) = ctx.input_mut(|i| keys.into_iter().find(|&k| i.consume_key(Modifiers::COMMAND, k))) else { return };
            if let Some(place) = row_move(&set.slots(), ti, key) {
                let mut params = grid::place_json(place);
                params["tile"] = json!(id);
                actions.push(Invocation::new("tile.move", params).into());
            }
            return;
        }
        let n = set.tiles.len();
        let step = ctx.input_mut(|i| {
            if i.consume_key(Modifiers::COMMAND, Key::ArrowUp) || i.consume_key(Modifiers::COMMAND, Key::ArrowLeft) {
                -1
            } else if i.consume_key(Modifiers::COMMAND, Key::ArrowDown) || i.consume_key(Modifiers::COMMAND, Key::ArrowRight) {
                1
            } else {
                0
            }
        });
        let to = ti as i64 + step; // 0-based place it goes to
        if step != 0 && (0..n as i64).contains(&to) {
            actions.push(inv!("tile.move", {"tile": id, "to": to + 1}).into());
        }
    }

    /// Where a window first opens: where egui put windows before 0.34, when panels still
    /// shrank the context's free area.
    fn window_pos(&self) -> Pos2 {
        self.board_rect.min + vec2(16.0, 16.0)
    }

    fn about_window(&mut self, ctx: &egui::Context) {
        let t = Tokens::get(ctx);
        let pos = self.window_pos();
        egui::Window::new(format!("About {APP_NAME}")).open(&mut self.ctl.show_about).default_pos(pos).collapsible(false).resizable(false).show(ctx, |ui| {
            ui.label(RichText::new(APP_NAME).font(theme::bold(20.0)).color(t.text));
            ui.label(RichText::new(format!("Version {}", env!("CARGO_PKG_VERSION"))).color(t.text_dim));
            ui.add_space(6.0);
            ui.label("A live soundboard for macOS and Linux.");
            ui.label(RichText::new("Design after SoundCraft by the ArtCraft team. Inter by The Inter Project Authors (OFL).").small().color(t.text_dim));
        });
    }

    /// A set file opened from the file manager (or passed on by a second launch): import
    /// it only when asked to, since a double-click on a download shouldn't change the
    /// library unseen (D-091). Import has the focus, so Enter imports and Esc cancels.
    fn confirm_set_window(&mut self, ctx: &egui::Context) {
        let Some(path) = self.confirm_sets.first().cloned() else { return };
        let t = Tokens::get(ctx);
        let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
        let folder = path.parent().map(|p| p.display().to_string()).unwrap_or_default();
        let mut answer = None;
        let modal = egui::Modal::new(egui::Id::unique("confirm-set-file")).show(ctx, |ui| {
            ui.set_max_width(380.0);
            theme::name_dialog(ui, "Import this set file?");
            ui.label(RichText::new("Import this set file?").font(theme::bold(16.0)).color(t.text));
            ui.add_space(6.0);
            ui.label(format!("“{name}” adds its sets after yours. Nothing you have changes."));
            ui.label(RichText::new(format!("In {folder}")).small().color(t.text_dim));
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                let import = theme::hint_keys(ui.button("Import"), "Add the set file's sets after yours", "Enter");
                if ui.memory(|m| m.focused().is_none()) {
                    import.request_focus();
                }
                if import.clicked() {
                    answer = Some(true);
                }
                if theme::hint_keys(ui.button("Cancel"), "Don't import it", "Esc").clicked() {
                    answer = Some(false);
                }
            });
        });
        if answer.is_none() && modal.should_close() {
            answer = Some(false);
        }
        if let Some(import) = answer {
            self.confirm_sets.remove(0);
            if import {
                self.run(inv!("set.import", {"path": path}));
            }
        }
    }

    /// A set file brought a theme, now installed (D-173): use it now, or leave it in
    /// Settings ▸ Theme. Use it has the focus, so Enter uses it and Esc leaves it.
    fn confirm_theme_window(&mut self, ctx: &egui::Context) {
        if !self.confirm_sets.is_empty() {
            return;
        }
        // Chosen some other way meanwhile (Settings, the CLI): nothing left to ask.
        let current = self.ctl.look.id.clone();
        self.confirm_themes.retain(|(id, _)| *id != current);
        let Some((id, name)) = self.confirm_themes.first().cloned() else { return };
        let t = Tokens::get(ctx);
        let mut answer = None;
        let modal = egui::Modal::new(egui::Id::unique("confirm-theme")).show(ctx, |ui| {
            ui.set_max_width(380.0);
            theme::name_dialog(ui, "Use its theme?");
            ui.label(RichText::new("Use its theme?").font(theme::bold(16.0)).color(t.text));
            ui.add_space(6.0);
            ui.label(format!("The set file brings the theme “{name}”. Use it now?"));
            ui.label(RichText::new("It's installed either way: Settings ▸ Theme lists it.").small().color(t.text_dim));
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                let yes = theme::hint_keys(ui.button("Use It"), &format!("Switch to {name}"), "Enter");
                if ui.memory(|m| m.focused().is_none()) {
                    yes.request_focus();
                }
                if yes.clicked() {
                    answer = Some(true);
                }
                if theme::hint_keys(ui.button("Not Now"), "Keep the theme you have", "Esc").clicked() {
                    answer = Some(false);
                }
            });
        });
        if answer.is_none() && modal.should_close() {
            answer = Some(false);
        }
        if let Some(yes) = answer {
            self.confirm_themes.remove(0);
            if yes {
                self.run(inv!("theme.use", {"theme": id}));
            }
        }
    }

    /// The live region (t-m20-announce, D-093): an unseen status node whose label is the
    /// latest announcement. AccessKit has VoiceOver and Orca read a live node's label
    /// whenever it changes, wherever their focus is. A repeated message gets a no-break
    /// space added or removed, so it still changes.
    fn live_region(&mut self, ui: &mut Ui) {
        let rect = Rect::from_min_size(ui.max_rect().left_bottom() - vec2(0.0, 1.0), vec2(1.0, 1.0));
        let id = egui::Id::unique("announcements");
        let resp = ui.interact(rect, id, Sense::hover());
        let Some(a) = &self.ctl.announcement else { return };
        let label = if a.seq % 2 == 0 { format!("{}\u{a0}", a.text) } else { a.text.clone() };
        ui.ctx().accesskit_node_builder(resp.id, |node| {
            node.set_role(egui::accesskit::Role::Status);
            node.set_live(egui::accesskit::Live::Polite);
            node.set_label(label);
        });
    }

    fn shortcuts_window(&mut self, ctx: &egui::Context) {
        let t = Tokens::get(ctx);
        let menus = menu::menus(&self.menu_state(), cfg!(target_os = "macos"));
        let table = menu::shortcuts(&menus);
        let mut open = self.ctl.show_shortcuts;
        egui::Window::new("Keyboard Shortcuts").open(&mut open).default_pos(self.window_pos()).collapsible(false).resizable(false).show(ctx, |ui| {
            egui::Grid::new("shortcut-grid").num_columns(2).spacing([24.0, 6.0]).show(ui, |ui| {
                let mut row = |k: String, what: &str| {
                    ui.label(RichText::new(k).font(theme::semibold(13.0)).color(t.text));
                    ui.label(RichText::new(what).color(t.text_dim));
                    ui.end_row();
                };
                let alt = if cfg!(target_os = "macos") { "⌥" } else { "Alt+" };
                row(format!("1–=, Q–], A–', Z–/, and with {alt}"), "Trigger the tile with that hotkey: rows 1 to 4 of the board, then 5 to 8 with Option or Alt");
                row("Space".into(), "Play the next cue (cue list)");
                row("Up / Down".into(), "Previous / next cue, without playing (cue list)");
                row("Esc".into(), "Stop All (the Stop All fade, if one is set; press again to stop at once)");
                row("Shift+Esc".into(), "Stop All at once");
                row("Tab / Shift+Tab".into(), "Move the keyboard focus: menus, toolbar, sets, inspector, board");
                row("Arrow keys".into(), "Move the focus between tiles, or between cues in the list");
                row("Enter or Space".into(), "Play the focused tile (Edit mode: select it); in the cue list Space stays GO");
                row("Shift+Enter".into(), "Make the focused cue the next one, without playing it");
                row(
                    format!("{} / {}", ctx.format_shortcut(&egui::KeyboardShortcut::new(Modifiers::COMMAND, Key::ArrowUp)), ctx.format_shortcut(&egui::KeyboardShortcut::new(Modifiers::COMMAND, Key::ArrowDown))),
                    "Edit mode: move the focused tile one place earlier or later (otherwise: master volume)",
                );
                row("Left / Right".into(), "Nudge a focused trim or vamp handle (Shift: 0.1 s), or the master volume");
                row(ctx.format_shortcut(&egui::KeyboardShortcut::new(Modifiers::COMMAND, Key::V)), "Paste sounds copied in a file manager");
                row(ctx.format_shortcut(&egui::KeyboardShortcut::new(Modifiers::COMMAND, Key::Z)), "Undo a change to sets or tiles");
                row(
                    ctx.format_shortcut(&egui::KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z)),
                    "Redo",
                );
                for (s, cmd) in &table {
                    row(ctx.format_shortcut(s), menu::label(&menus, *cmd));
                }
            });
        });
        self.ctl.show_shortcuts = open;
    }

    fn handle_hotkeys(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        // Leave keys alone while typing or while a menu is open. Any other focused control
        // (a clicked or Tab-focused pad, a button, the inspector's tabs) keeps the arrows,
        // Space and Enter, but tile keys and Esc still work (SPEC: only a text field
        // stops them).
        if ctx.text_edit_focused() || egui::Popup::is_any_open(ctx) || self.asking() {
            return;
        }
        let focused = ctx.memory(|m| m.focused());
        let control_focused = focused.is_some();
        let wave_focused = focused.is_some() && focused == self.editor_wave;
        let events = ctx.input(|i| i.events.clone());
        for ev in events {
            let egui::Event::Key { key, physical_key, pressed, repeat, modifiers } = ev else { continue };
            // `bind.learn {from: key}`: the next combination becomes a system-wide key.
            if pressed && !repeat && self.ctl.learning.as_ref().is_some_and(|l| l.key) {
                if key == Key::Escape && !modifiers.any() {
                    actions.push(inv!("bind.learn", {"cancel": true}).into());
                } else {
                    self.ctl.learn_key(&key_combo(key, modifiers));
                }
                continue;
            }
            // Option (Alt) is for the pad rows 5–8 (D-188). With it held, macOS types
            // another character (⌥1 is ¡), so the key is read where it is on the keyboard.
            if modifiers.command || modifiers.ctrl || modifiers.mac_cmd || (pressed && repeat) {
                continue;
            }
            let key = if modifiers.alt { physical_key.unwrap_or(key) } else { key };
            // The focused waveform zooms with − and =.
            if wave_focused && matches!(key, Key::Minus | Key::Equals | Key::Plus) {
                continue;
            }
            // The QuipClipper window keeps its keys (Enter adds, Space previews); Esc
            // still stops everything.
            if key != Key::Escape && quip_browser::QuipBrowser::has_focus(ctx) {
                continue;
            }
            if self.capturing_hotkey && pressed {
                self.capturing_hotkey = false;
                if let Some(id) = self.ctl.selected_tile {
                    match key {
                        Key::Escape => {}
                        Key::Backspace | Key::Delete => actions.push(inv!("tile.set", {"tile": id, "hotkey": null}).into()),
                        k => match key_char(k) {
                            Some(c) => actions.push(inv!("tile.set", {"tile": id, "hotkey": PadKey { key: c, alt: modifiers.alt }.name()}).into()),
                            None => self.ctl.notify("That key can't be a hotkey. Use a key from the keyboard's four rows (1 to =, Q to ], A to ', Z to /), with or without Option"),
                        },
                    }
                }
                continue;
            }
            match (key, pressed) {
                // Shift+Esc always stops at once. Plain Esc leaves the stage to the
                // controller (a second press while it fades stops at once), or with the
                // hold guard on waits until it has been held (below).
                (Key::Escape, true) if modifiers.shift => actions.push(inv!("transport.stopAll", {"immediate": true}).into()),
                (Key::Escape, true) if self.ctl.store.library.stop_all_hold => self.esc_down = Some((Instant::now(), false)),
                (Key::Escape, true) => actions.push(inv!("transport.stopAll").into()),
                (Key::Escape, false) => {
                    if let Some((_, false)) = self.esc_down.take() {
                        self.ctl.notify("Hold Esc to stop everything (Shift+Esc stops at once)");
                    }
                }
                // Space is Pause All (Resume All) in every set, on a focused pad or row too
                // (D-199); a focused button or tab keeps it. In a cue list Enter is Play
                // Selection (a focused row takes it itself, in `list`).
                (Key::Space, true) if !control_focused || self.focused_tile.is_some() => {
                    // Consumed so a focused row or button doesn't also take it.
                    ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Space));
                    actions.push(inv!("transport.pauseAll").into());
                }
                (Key::Enter, true) if self.ctl.current_set().layout == Layout::List && !control_focused => {
                    ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter));
                    actions.push(inv!("cue.go").into());
                }
                // Move the pointer without playing; consumed so egui doesn't move focus.
                // With a cue row focused, the arrows move the focus instead (egui does that).
                (Key::ArrowUp | Key::ArrowDown, true) if self.ctl.current_set().layout == Layout::List && !control_focused => {
                    ctx.input_mut(|i| i.consume_key(Modifiers::NONE, key));
                    actions.push(Invocation::new(if key == Key::ArrowUp { "cue.prev" } else { "cue.next" }, json!({})).into());
                }
                (k, true) => {
                    let found = key_char(k).and_then(|c| self.ctl.tile_for_key(PadKey { key: c, alt: modifiers.alt }));
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
        // The hold guard: Stop All once Esc has been down long enough (a release that
        // never arrived, say while a menu was open, counts as let go).
        if let Some((since, false)) = self.esc_down {
            if !ctx.input(|i| i.key_down(Key::Escape)) {
                self.esc_down = None;
            } else if since.elapsed() >= STOP_ALL_HOLD {
                self.esc_down = Some((since, true));
                actions.push(inv!("transport.stopAll").into());
            } else {
                ctx.request_repaint_after(STOP_ALL_HOLD - since.elapsed());
            }
        }
    }

    /// Files dropped on the window: pictures onto the tile under the pointer, sounds as
    /// new tiles where they're dropped (Replace Audio… replaces a tile's sound, D-142).
    fn handle_file_drop(&mut self, ctx: &egui::Context) {
        let (dropped, pos) = ctx.input(|i| (i.raw.dropped_files.clone(), i.pointer.latest_pos()));
        let paths: Vec<PathBuf> = dropped.iter().map(|f| f.path().to_path_buf()).collect();
        if paths.is_empty() {
            return;
        }
        // Set files become new sets, wherever they're dropped.
        let (set_files, paths): (Vec<PathBuf>, Vec<PathBuf>) = paths.into_iter().partition(|p| crate::setfile::is_set_file(p));
        for f in set_files {
            self.run(inv!("set.import", {"path": f}));
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
        if paths.is_empty() {
            return;
        }
        // Rows mode: into the gap, blank slot or new row the board showed, as dragged pads
        // go (D-142); elsewhere after the tile they're dropped on, or at the end.
        let place = match pos.zip(self.row_grid.as_ref()) {
            Some((p, (l, slots))) if self.board_rect.contains(p) => grid::target_at(l, slots.len(), |r, a| slots[r][a].is_none(), p),
            _ => target.map_or(Place::Append, |(_, i)| Place::Flat(i + 1)),
        };
        let mut params = grid::place_json(place);
        params["paths"] = json!(paths);
        self.run(Invocation::new("tile.add", params));
    }

    /// ⌘/Ctrl+V outside a text field imports files copied in a file manager. egui sends
    /// a Paste event only when the clipboard also holds text, so the key's release is
    /// watched too (its press never reaches the app).
    fn handle_paste(&mut self, ctx: &egui::Context) {
        if ctx.text_edit_focused() || egui::Popup::is_any_open(ctx) || self.asking() {
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

    /// "Air Horn" for a tile trigger, else the command's label (and its tile).
    fn binding_target_label(&self, b: &crate::bindings::Binding) -> String {
        let tile = b.params.get("tile").and_then(|v| v.as_str()).and_then(|r| r.parse::<Uuid>().ok()).and_then(|id| self.ctl.store.library.tile(id));
        let label = crate::commands::spec(&b.command).map_or(b.command.as_str(), |s| s.label.trim_end_matches('…'));
        match (b.command.as_str(), tile) {
            ("tile.trigger", Some(t)) => t.title.clone(),
            (_, Some(t)) => format!("{label}: {}", t.title),
            ("tile.trigger", None) => "a removed tile".to_owned(),
            _ => label.to_owned(),
        }
    }

    /// ⌘/Ctrl+Z and ⌘/Ctrl+Shift+Z outside a text field undo and redo library changes;
    /// inside one, egui undoes the typing.
    fn handle_undo_keys(&mut self, ctx: &egui::Context) {
        if ctx.text_edit_focused() || egui::Popup::is_any_open(ctx) || self.asking() {
            return;
        }
        let redo_key = egui::KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
        let undo_key = egui::KeyboardShortcut::new(Modifiers::COMMAND, Key::Z);
        // Shift first: egui matches modifiers loosely.
        let cmd = ctx.input_mut(|i| {
            if i.consume_shortcut(&redo_key) {
                Some("edit.redo")
            } else if i.consume_shortcut(&undo_key) {
                Some("edit.undo")
            } else {
                None
            }
        });
        if let Some(id) = cmd {
            if let Err(e) = self.ctl.execute(id, json!({})) {
                self.ctl.report(&e);
            }
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
        let exts: Vec<&str> = AUDIO_EXTENSIONS.iter().chain(crate::transcode::FFMPEG_EXTENSIONS).copied().collect();
        let dialog = rfd::FileDialog::new().set_title("Add Sounds").add_filter("Audio and video", &exts);
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
            // A double-click on a cue row is Play Selection on it (D-197): one cue at a time.
            Action::Press(id) if self.ctl.search.is_none() && self.ctl.store.library.locate(id).is_some_and(|(si, _)| si == self.ctl.set_idx()) && self.ctl.current_set().layout == Layout::List => {
                self.run(inv!("cue.go", {"tile": id}));
            }
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
            Action::KeyPress(id, key) => {
                self.keys_held.insert(key, id);
                // On the grid a focused pad's Enter toggles it, One-shot too (D-196).
                let toggle = self.ctl.current_set().layout == Layout::Grid;
                self.run(inv!("tile.trigger", {"tile": id, "event": "press", "toggle": toggle}));
            }
        }
    }

    /// `LEADSTINGER_SCRIPT` steps, for automated screenshots. Each step is one command, so
    /// scripts exercise the same paths as the GUI and (from M7) the CLI.
    fn run_script(&mut self, ctx: &egui::Context) {
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
                "playon" => inv!("tile.playOn", {"tile": tile}),
                "select" => inv!("view.select", {"tile": tile}),
                "also" => inv!("view.select", {"tile": tile, "add": true}),
                // Shift-click tile N: the rectangle from the selected one (D-169).
                "range" => inv!("view.select", {"tile": tile, "range": true}),
                // The inspector's tab (tab:sound), hiding a side (hide:sidebar), the set lock.
                "tab" => inv!("view.inspector", {"tab": arg}),
                "hide" => Invocation::new(if arg == "sidebar" { "view.sidebar" } else { "view.inspector" }, json!({"open": false})),
                "lock" | "unlock" => Invocation::new(if cmd == "lock" { "set.lock" } else { "set.unlock" }, json!({})),
                // Cue links on tile N: continue after 2 s, follow, a 1.5 s pre-wait.
                "continue" => inv!("tile.set", {"tile": tile, "link": "continue", "postWait": 2.0}),
                "follow" => inv!("tile.set", {"tile": tile, "link": "follow"}),
                "prewait" => inv!("tile.set", {"tile": tile, "preWait": 1.5}),
                "list" | "grid" => inv!("set.layout", {"layout": cmd}),
                // The cue list: GO, pass cue N over, Reset List, the playback mode.
                "go" => inv!("cue.go"),
                "disarm" => inv!("cue.arm", {"tile": tile, "armed": false}),
                "reset" => inv!("cue.reset"),
                "continuous" | "queued" => inv!("set.listMode", {"mode": cmd}),
                "set" => inv!("set.select", {"set": tile}),
                // A set (from 0) in a window of its own.
                "window" => inv!("window.open", {"set": format!("#{}", n + 1)}),
                "settings" => inv!("view.settings"),
                "updates" => inv!("app.checkForUpdates"),
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
                "find" => inv!("view.search", {"query": arg}),
                // The QuipClipper window, and a search in it.
                "quip" => inv!("view.quip", {"open": true}),
                "quipfind" => inv!("view.quip", {"query": arg}),
                // A link as the system would open it: the first-contact prompt (D-132).
                "quiplink" => {
                    let (id, params) = crate::quip::opened_command(arg);
                    crate::commands::Invocation::new(id, params)
                }
                "focus" => {
                    self.focus_script = self.ctl.current_set().tiles.get(n).map(|t| t.id);
                    continue;
                }
                // The keyboard focus on set N in Panel's sets column, as a click leaves it.
                "setfocus" => {
                    if let Some(set) = self.ctl.store.library.sets.get(n) {
                        let id = egui::Id::unique(("panel-set", set.id));
                        ctx.memory_mut(|m| m.request_focus(id));
                    }
                    continue;
                }
                "setmenu" => {
                    self.set_menu_script = self.ctl.store.library.sets.get(n).map(|s| s.id);
                    continue;
                }
                "normalize" => inv!("tile.normalize"),
                "export" => inv!("tile.render"),
                // The waveform editor on tile N, a vamp and a marker on it, and zooming
                // in N doublings around the cursor or the middle.
                "editor" => inv!("view.editor", {"tile": tile}),
                "vamp" => inv!("tile.set", {"tile": tile, "vampStart": 0.25, "vampEnd": 0.6}),
                "marker" => inv!("tile.markerAdd", {"tile": tile, "at": 0.15, "name": "Hit"}),
                // A selection in the open editor, from N s for 3 s.
                "eselect" => inv!("view.editorSelect", {"from": n as f64, "to": n as f64 + 3.0}),
                "ezoom" => {
                    if let Some(v) = self.editor_view.as_mut() {
                        v.zoom_in(n as u32);
                    }
                    continue;
                }
                "padd" | "panel" | "console" => inv!("view.theme", {"theme": cmd}),
                // theme:<id or name> uses a theme; themeimport:<path> imports and uses one.
                "theme" => inv!("theme.use", {"theme": arg}),
                "themeimport" => inv!("theme.import", {"path": arg}),
                // setimport:<path> imports a set file; one carrying a theme asks to use it.
                "setimport" => inv!("set.import", {"path": arg}),
                "light" | "dark" => inv!("view.appearance", {"appearance": cmd}),
                "contrast" => inv!("view.appearance", {"appearance": "high_contrast"}),
                // Settings ▸ Coloured pads on (D-152).
                "colored" => inv!("settings.coloredPads", {"on": true}),
                "disable" => inv!("tile.disable", {"tile": tile}),
                // An end fade-out on tile N (its fade badge, D-168), and the Random button's
                // command (D-171).
                "fadeout" => inv!("tile.set", {"tile": tile, "fadeOut": 1.5}),
                "random" => inv!("tile.random"),
                // opt:N:hold+pausable+solo turns those pad options on (D-145); title:N:text
                // renames tile N, for the face's long titles.
                "opt" | "title" => {
                    let (idx, rest) = arg.split_once(':').unwrap_or((arg, ""));
                    let mut params = serde_json::Map::new();
                    params.insert("tile".into(), json!(format!("#{}", idx.parse::<usize>().unwrap_or(0) + 1)));
                    if cmd == "title" {
                        params.insert("title".into(), json!(rest));
                    } else {
                        for key in rest.split('+').filter(|k| !k.is_empty()) {
                            params.insert(key.to_owned(), json!(true));
                        }
                    }
                    crate::commands::Invocation::new("tile.set", Value::Object(params))
                }
                // zoom:200 sets the UI zoom in percent.
                "zoom" => inv!("view.zoom", {"zoom": n}),
                // rows:3+8 sets the current set's rows, columns:3 its row size; layout:auto the
                // grid layout.
                "rows" => inv!("set.rows", {"rows": arg.replace('+', ",")}),
                "columns" => inv!("set.columns", {"columns": n}),
                "layout" => inv!("settings.gridLayout", {"mode": arg}),
                // record:12 shows a recording 12 s into a new tile, with no input open.
                "record" => {
                    self.ctl.stage_recording(n as f64);
                    continue;
                }
                // scroll:300 turns the wheel 300 points down at (600, 400), e.g. over Settings;
                // scroll:300@1260.600 does it there (the inspector, at 1280 wide).
                "scroll" => {
                    let (points, at) = arg.split_once('@').unwrap_or((arg, "600.400"));
                    let at = at.split_once('.').and_then(|(x, y)| Some(pos2(x.parse().ok()?, y.parse().ok()?))).unwrap_or(pos2(600.0, 400.0));
                    self.script_scroll = Some((points.parse().unwrap_or(0.0), at));
                    self.opts.script_resume_frame = self.frame + 2;
                    break;
                }
                "wait" => {
                    self.opts.script_resume_frame = self.frame + n as u64;
                    break;
                }
                // drag:3@600.420 holds tile 3 over (600, 420), button down (altdrag: with
                // Alt down too, a copy, D-229); files:2@600.420 holds two sound files there
                // (D-141 previews).
                "drag" | "altdrag" | "files" | "blank" => {
                    let (what, at) = arg.split_once('@').unwrap_or((arg, ""));
                    let num = |t: &str| t.split_once('.').and_then(|(a, b)| Some((a.parse::<f32>().ok()?, b.parse::<f32>().ok()?)));
                    if cmd == "blank" {
                        let Some((row, slot)) = num(what) else { continue };
                        inv!("set.addBlank", {"row": row, "slot": slot})
                    } else {
                        let (Ok(k), Some((x, y))) = (what.parse::<usize>(), num(at)) else { continue };
                        self.script_drag = Some((cmd == "files", k, pos2(x, y), 0));
                        self.script_drag_alt = cmd == "altdrag";
                        continue;
                    }
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

    /// The top bar: the wordmark, then the counter, the master meter, Stop All and Settings.
    /// What changes the board (Sidebar, Add Sound, Grid | List, Inspector) is in the bottom
    /// bar (M22).
    fn toolbar(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let t = Tokens::get(ui.ctx());
        ui.horizontal(|ui| {
            ui.set_height(48.0);
            ui.spacing_mut().item_spacing.x = 10.0;
            // The top strip on macOS: clear of the traffic lights (D-228).
            if !self.opts.in_window_menu {
                ui.add_space((crate::titlebar::inset(ui.ctx()).x - 14.0).max(0.0));
            }
            // A narrow (e.g. tiled) window shrinks the wordmark to its tile mark, so Stop
            // All and Settings always have room.
            paint_wordmark(ui, ui.available_width() < 720.0);

            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                let key = theme::role(theme::key(ui, "⚙", vec2(34.0, 34.0), self.ctl.show_settings), AxRole::Button, "Settings", Some(self.ctl.show_settings));
                if theme::hint_keys(key, tips::SETTINGS.0, tips::SETTINGS.1).clicked() {
                    actions.push(inv!("view.settings", {"open": !self.ctl.show_settings}).into());
                }
                let stop = theme::key_with(ui, "⏹  Stop All", vec2(112.0, 34.0), t.stop, theme::inks(ui.ctx()).stop, theme::semibold(13.0));
                let stop = theme::role(stop, AxRole::Button, "Stop All", None);
                if theme::hint(stop, tips::stop_all(self.ctl.store.library.stop_all_hold)).clicked() {
                    actions.push(inv!("transport.stopAll").into());
                }
                self.transport_keys(ui, actions);
                // Narrow windows drop the counter, then the meter, rather than overlap
                // the left-hand groups.
                if ui.available_width() > 230.0 {
                    self.master_section(ui, &t);
                }
                // Random (D-171), left of the meter; it stays when the counter goes.
                if ui.available_width() > 110.0 {
                    self.random_key(ui, actions);
                }
                let room = ui.available_width() - 12.0;
                if room >= 190.0 {
                    self.counter(ui, &t, room.min(300.0));
                }
            });
        });
    }

    /// Beside Stop All, right to left: Fade All, Pause All (Resume All while everything
    /// is paused) and, in a List set, Previous and Next (D-197).
    fn transport_keys(&self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let fade = theme::role(theme::key(ui, "Fade All", vec2(80.0, 34.0), false), AxRole::Button, "Fade All", None);
        if theme::hint(fade, tips::FADE_ALL).clicked() {
            actions.push(inv!("transport.fadeAll").into());
        }
        let (label, tip, any) = self.pause_all_state();
        let pause = ui.add_enabled_ui(any, |ui| theme::key(ui, label, vec2(100.0, 34.0), false)).inner;
        if theme::hint_or(theme::role(pause, AxRole::Button, label.trim_start_matches(['⏸', '▶', ' ']), None), tip, tips::PAUSE_ALL_WHY).clicked() {
            actions.push(inv!("transport.pauseAll").into());
        }
        if self.ctl.current_set().layout == Layout::List {
            if theme::hint_keys(theme::role(step_key(ui, false), AxRole::Button, "Next cue", None), tips::CUE_NEXT.0, tips::CUE_NEXT.1).clicked() {
                actions.push(inv!("cue.next").into());
            }
            if theme::hint_keys(theme::role(step_key(ui, true), AxRole::Button, "Previous cue", None), tips::CUE_PREV.0, tips::CUE_PREV.1).clicked() {
                actions.push(inv!("cue.prev").into());
            }
        }
    }

    /// Pause All's label and hover text, and whether there's anything for it to do.
    fn pause_all_state(&self) -> (&'static str, String, bool) {
        let playing = self.ctl.statuses.values().any(|s| s.state == PlayState::Playing);
        let paused = self.ctl.statuses.values().any(|s| s.state == PlayState::Paused);
        if !playing && paused {
            ("▶  Resume All", format!("{} — {}", tips::RESUME_ALL.0, tips::RESUME_ALL.1), true)
        } else {
            ("⏸  Pause All", format!("{} — {}", tips::PAUSE_ALL.0, tips::PAUSE_ALL.1), playing)
        }
    }

    /// The Random button (D-171): a die and the word, greyed out when the set has nothing
    /// to pick. It runs `tile.random`, so it plays in a locked set too.
    fn random_key(&self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let any = self.ctl.can_play_random();
        let resp = ui
            .add_enabled_ui(any, |ui| {
                let resp = theme::key(ui, "", vec2(100.0, 34.0), false);
                let ink = Tokens::get(ui.ctx()).text;
                let p = ui.painter();
                let g = p.layout_no_wrap("Random".into(), theme::semibold(13.0), ink);
                let (icon, gap) = (15.0, 7.0);
                let left = resp.rect.center().x - (icon + gap + g.size().x) / 2.0;
                let cy = resp.rect.center().y;
                pad_options::paint_die(p, pos2(left + icon / 2.0, cy), icon, ink);
                p.galley(pos2(left + icon + gap, cy - g.size().y / 2.0), g, ink);
                resp
            })
            .inner;
        if theme::hint_or(theme::role(resp, AxRole::Button, "Random", None), tips::RANDOM, tips::RANDOM_WHY).clicked() {
            actions.push(inv!("tile.random").into());
        }
    }

    /// The bottom bar (M22): Setlist, Add Sound, Grid | List with the columns, and
    /// Inspector at the far end, under the panels they show and hide. Setlist and
    /// Inspector carry an arrow the way a click moves their panel; Grid and List are
    /// icons, their words in the hover text (D-216).
    fn bottom_bar(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let si = self.ctl.set_idx();
        let set = &self.ctl.store.library.sets[si];
        let (layout, mut columns, locked) = (set.layout, set.columns, set.locked);
        let (sidebar, inspector) = (self.ctl.store.library.show_sidebar, self.ctl.store.library.show_inspector);
        ui.horizontal(|ui| {
            ui.set_height(32.0);
            ui.spacing_mut().item_spacing.x = 4.0;
            let key = theme::role(arrow_key(ui, "Setlist", 84.0, sidebar, sidebar), AxRole::Button, "Setlist", Some(sidebar));
            let tip = tips::setlist(sidebar);
            if theme::hint_keys(key, tip.0, tip.1).clicked() {
                actions.push(inv!("view.sidebar").into());
            }
            ui.add_space(10.0);
            let add = ui.add_enabled_ui(!locked, |ui| theme::key(ui, "✚  Add Sound", vec2(112.0, 26.0), false)).inner;
            if theme::hint_or(theme::role(add, AxRole::Button, "Add Sound", None), format!("{} — {}", tips::ADD.0, tips::ADD.1), tips::LOCKED_WHY).clicked() {
                actions.push(inv!("tile.add").into());
            }
            ui.add_space(10.0);
            let key = theme::role(icon_key(ui, layout == Layout::Grid, pad_options::paint_grid_icon), AxRole::Button, "Grid", Some(layout == Layout::Grid));
            if theme::hint_keys(key, tips::GRID.0, tips::GRID.1).clicked() && layout != Layout::Grid {
                actions.push(inv!("set.layout", {"layout": "grid"}).into());
            }
            let key = theme::role(icon_key(ui, layout == Layout::List, pad_options::paint_list_icon), AxRole::Button, "Cue list", Some(layout == Layout::List));
            if theme::hint_keys(key, tips::LIST.0, tips::LIST.1).clicked() && layout != Layout::List {
                actions.push(inv!("set.layout", {"layout": "list"}).into());
            }
            if layout == Layout::Grid {
                ui.add_space(4.0);
                let grid = self.ctl.store.library.grid;
                let field = ui.add_enabled(grid != GridMode::Auto && !locked, egui::DragValue::new(&mut columns).range(1..=12).speed(0.05));
                let tip = match grid {
                    GridMode::Rows => "Tiles per row, 1–12: evens this set's rows out (with a fuller row, every pad narrows so it fits)",
                    _ => "Tiles per row in this set's grid, 1–12: drag sideways or type a number",
                };
                theme::hint_or(theme::name(field, "Columns"), tip, if locked { tips::LOCKED_WHY } else { tips::AUTO_COLUMNS });
                if columns != set.columns {
                    match grid {
                        GridMode::Rows => actions.push(inv!("set.rows", {"perRow": columns}).into()),
                        _ => actions.push(inv!("set.columns", {"columns": columns}).into()),
                    }
                }
            }
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                let key = theme::role(arrow_key(ui, "Inspector", 98.0, inspector, !inspector), AxRole::Button, "Inspector", Some(inspector));
                let tip = tips::inspector(inspector);
                if theme::hint_keys(key, tip.0, tip.1).clicked() {
                    actions.push(inv!("view.inspector").into());
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
        let (rect, resp) = ui.allocate_exact_size(vec2(width, 46.0), Sense::hover());
        theme::role(resp, egui::accesskit::Role::Label, self.counter_label(), None);
        let p = ui.painter();
        p.rect_filled(rect, 4.0, t.counter_bg);
        p.rect_stroke(rect, 4.0, Stroke::new(1.0_f32, t.border_light), StrokeKind::Inside);
        let inner = rect.shrink2(vec2(12.0, 6.0));
        let sounding = self.ctl.statuses.values().filter(|s| s.state != PlayState::Paused).count();
        let rec = self.ctl.recording_view();
        let (right_label, right_color) = match &rec {
            Some(r) => (format!("● REC {}", format_clock(r.seconds, false)), t.error),
            None if sounding > 0 => (format!("{sounding} PLAYING"), t.text_dim),
            None => ("READY".to_owned(), t.text_dim),
        };
        p.text(inner.right_top(), Align2::RIGHT_TOP, right_label, theme::semibold(10.0), right_color);
        match self.focus_voice() {
            Some((id, s)) => {
                let title = self.ctl.store.library.tile(id).map(|t| t.title.to_uppercase()).unwrap_or_default();
                let color = if s.state == PlayState::Paused { t.solo } else { t.counter_text };
                let galley = ui.fonts_mut(|f| {
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
            None if rec.is_some() => {
                let r = rec.as_ref().expect("recording");
                let title = rec_title(&r.title).to_uppercase();
                let from = r.input.as_deref().map_or_else(|| title.clone(), |i| format!("{title} · {}", i.to_uppercase()));
                let galley = ui.fonts_mut(|f| {
                    let mut job = egui::text::LayoutJob::simple_singleline(from, theme::semibold(10.0), t.text_dim);
                    job.wrap.max_width = inner.width() - 80.0;
                    job.wrap.max_rows = 1;
                    f.layout_job(job)
                });
                p.galley(inner.left_top(), galley, t.text_dim);
                theme::paint_tabular(p, inner.left_bottom() + vec2(0.0, 2.0), Align2::LEFT_BOTTOM, &format_clock(r.seconds, true), theme::semibold(22.0), t.error);
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
        let (value, text) = self.master_info();
        let (resp, steps) = theme::slider_info(resp, "Master volume", value, text);
        let steps = steps + tips::arrow_steps(&resp);
        self.master_steps(steps);
        let resp = theme::hint(resp, tips::master(self.ctl.store.library.master_volume));
        let bars = Rect::from_min_size(rect.min + vec2(10.0, 22.0), vec2(150.0, 13.0));
        let clip_light = Rect::from_center_size(pos2(bars.right() + 9.0, bars.center().y), vec2(6.0, 13.0));
        if let Some(pos) = resp.interact_pointer_pos() {
            if resp.clicked() && clip_light.expand2(vec2(9.0, 6.0)).contains(pos) {
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
        for (ch, b) in self.meter.ch.iter().enumerate() {
            let row = Rect::from_min_size(bars.min + vec2(0.0, ch as f32 * 8.0), vec2(bars.width(), 5.0));
            p.rect_filled(row, 1.0, t.meter_off);
            // Unlit zones at 18%, as on SoundCraft's meters.
            for (from, to, c) in [(0.0, green_end, t.meter_green), (green_end, yellow_end, t.meter_yellow), (yellow_end, 1.0, t.meter_red)] {
                let zone = Rect::from_x_y_ranges(row.left() + from * row.width()..=row.left() + to * row.width(), row.y_range());
                p.rect_filled(zone, 0.0, theme::with_alpha(c, 0.18));
                let lit = meter_frac(b.level).clamp(0.0, 1.0);
                if lit > from {
                    let end = lit.min(to);
                    p.rect_filled(Rect::from_x_y_ranges(row.left() + from * row.width()..=row.left() + end * row.width(), row.y_range()), 0.0, c);
                }
            }
            let hold = meter_frac(b.hold);
            if hold > 0.0 {
                let c = if hold >= yellow_end { t.meter_red } else if hold >= green_end { t.meter_yellow } else { t.meter_green };
                p.rect_filled(Rect::from_min_size(pos2(row.left() + hold * row.width() - 1.0, row.top()), vec2(2.0, 5.0)), 0.0, c);
            }
        }
        p.rect_filled(clip_light, 1.5, if self.meter.clip { t.meter_red } else { theme::shade(t.meter_red, 0.25) });
        if self.meter.clip {
            // Said in words too: a lit light differs from an unlit one only in brightness.
            p.text(pos2(clip_light.right(), rect.top() + 6.0), Align2::RIGHT_TOP, "CLIP", theme::semibold(10.0), t.text);
        }
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
        let t = Tokens::get(ui.ctx());
        ui.add_space(10.0);
        ui.label(RichText::new("SETLIST").font(theme::semibold(10.0)).color(t.text_dim));
        ui.add_space(4.0);
        let selected = self.ctl.store.library.selected_set;
        let n_sets = self.ctl.store.library.sets.len();
        egui::ScrollArea::vertical().auto_shrink([false, true]).max_height(ui.available_height() - 48.0).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            for set in self.ctl.store.library.sets.iter() {
                if let Some((id, draft, focused)) = &mut self.renaming {
                    if *id == set.id {
                        let field = theme::name(theme::text_field(ui, egui::TextEdit::singleline(draft).desired_width(f32::INFINITY)), "Set name");
                        let resp = theme::hint(field, tips::RENAME_FIELD);
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
                // Pads dragged over it move there when let go (D-219).
                let drop = self.drop_on_set(&resp, set.id, actions);
                let p = ui.painter();
                if is_sel {
                    p.rect_filled(rect, 4.0, t.selected);
                    // A bar on the left edge marks the current set as well as its fill.
                    p.rect_filled(Rect::from_min_size(rect.min, vec2(3.0, rect.height())), CornerRadius { nw: 4, sw: 4, ne: 0, se: 0 }, t.accent);
                } else if resp.hovered() || drop {
                    p.rect_filled(rect, 4.0, theme::with_alpha(t.text, 0.06));
                }
                if drop {
                    p.rect_stroke(rect, 4.0, Stroke::new(1.5_f32, t.accent), egui::StrokeKind::Inside);
                }
                if playing {
                    p.text(pos2(rect.left() + 14.0, rect.center().y), Align2::CENTER_CENTER, "▶", theme::bold(10.0), t.counter_text);
                }
                let font = if is_sel { theme::semibold(13.0) } else { theme::regular(13.0) };
                let name = ui.fonts_mut(|f| {
                    let mut job = egui::text::LayoutJob::simple_singleline(set.name.clone(), font, t.text);
                    let windowed = self.ctl.store.library.set_windows.iter().any(|w| w.set == set.id);
                    job.wrap.max_width = rect.width() - 64.0 - if set.locked { 20.0 } else { 0.0 } - if windowed { 20.0 } else { 0.0 };
                    job.wrap.max_rows = 1;
                    f.layout_job(job)
                });
                p.galley(pos2(rect.left() + 26.0, rect.center().y - name.size().y / 2.0), name, t.text);
                let count = theme::paint_tabular(p, pos2(rect.right() - 10.0, rect.center().y), Align2::RIGHT_CENTER, &set.tiles.len().to_string(), theme::regular(11.5), t.text_dim);
                let mut left = count.left();
                if set.locked {
                    pad_options::paint_lock(p, pos2(left - 11.0, rect.center().y), 14.0, true, t.text_dim);
                    left -= 20.0;
                }
                // Open in a window of its own (D-224).
                let windowed = self.ctl.store.library.set_windows.iter().any(|w| w.set == set.id);
                if windowed {
                    pad_options::paint_window_mark(p, pos2(left - 11.0, rect.center().y), 13.0, t.text_dim);
                }
                let index = self.ctl.store.library.set_index(set.id).unwrap_or(0);
                let resp = theme::role(resp, AxRole::Button, tips::set_label(&set.name, index, n_sets, set.tiles.len(), playing, set.locked, windowed), Some(is_sel));
                let resp = theme::hint(resp, tips::set_row(&set.name, set.tiles.len(), playing, set.locked, windowed));
                if resp.clicked() {
                    actions.extend(self.show_set(set.id));
                }
                if resp.double_clicked() {
                    actions.push(Action::StartRename(set.id));
                }
                self.set_row_menu(&resp, set.id, n_sets, set.locked, windowed, actions);
            }
        });
        ui.with_layout(egui::Layout::bottom_up(Align::LEFT), |ui| {
            ui.add_space(10.0);
            let key = theme::role(theme::key(ui, "✚  New Set", vec2(104.0, 26.0), false), AxRole::Button, "New Set", None);
            if theme::hint_keys(key, tips::NEW_SET.0, tips::NEW_SET.1).clicked() {
                actions.push(inv!("set.create").into());
            }
        });
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        let t = Tokens::get(ui.ctx());
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
            if let Some(rec) = self.ctl.recording_view() {
                ui.add_space(8.0);
                self.recording_status(ui, &t, &rec);
            }
            if let Some(what) = import_summary(&self.ctl.store) {
                ui.add_space(8.0);
                ui.spinner();
                ui.label(RichText::new(what).color(t.text_dim));
                if theme::hint(theme::small_button(ui, "Cancel"), "Stop importing; the tiles stay, marked cancelled").clicked() {
                    self.run(inv!("import.cancel"));
                }
            } else if let Some(what) = self.ctl.render_summary() {
                ui.add_space(8.0);
                ui.spinner();
                ui.label(RichText::new(what).color(t.text_dim));
                if theme::hint(theme::small_button(ui, "Cancel"), "Stop exporting; a half-written file is removed").clicked() {
                    self.run(inv!("render.cancel"));
                }
            } else if self.ctl.store.is_loading() {
                ui.add_space(8.0);
                ui.spinner();
                ui.label(RichText::new("Loading audio…").color(t.text_dim));
            }
            if let Some(badge) = self.quip_badge() {
                ui.add_space(8.0);
                // A narrow bar says just "3 new"; the name and hover text say where.
                let text = if ui.available_width() > 460.0 { badge.name() } else { format!("{} new", badge.n) };
                let button = theme::small(egui::Button::new(RichText::new(text).small().strong().color(theme::ink_on(t.accent))).fill(t.accent));
                let resp = theme::name(ui.add(button), &badge.name());
                if theme::hint(resp, badge.tip).clicked() {
                    self.run(inv!("view.quip", {"new": true}));
                }
            }
            // The right side: the output, outputs that aren't open, and the last notice,
            // each cut short with "…" (the full text on hover) so nothing runs over the
            // left side (D-191). While a notice shows, the output gives way if the notice
            // would get less than NOTICE_MIN of room.
            let notice = self.ctl.notice.as_ref().filter(|n| self.notice_hovered || n.at.elapsed() < Duration::from_secs(if n.error { 10 } else { 4 }));
            let output = match (&self.ctl.engine.device_name, &self.ctl.engine.error) {
                (_, Some(err)) => RichText::new(err).color(t.error),
                (Some(name), None) => {
                    let khz = self.ctl.engine.sample_rate() as f32 / 1000.0;
                    RichText::new(format!("Output: {name} · {khz} kHz")).color(t.text_dim)
                }
                (None, None) => RichText::new("No output").color(t.text_dim),
            };
            let mut hovered = false;
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                let output_w = egui::WidgetText::from(output.clone()).into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, egui::TextStyle::Body).size().x;
                if notice.is_none() || ui.available_width() - output_w - 16.0 >= NOTICE_MIN {
                    ui.add(egui::Label::new(output).truncate());
                }
                // Outputs that aren't open (a device unplugged), until they reopen.
                let closed = &self.ctl.engine.output_errors;
                if !closed.is_empty() {
                    let names: Vec<&str> = closed.iter().map(|(n, _)| n.as_str()).collect();
                    let why: Vec<String> = closed.iter().map(|(n, e)| format!("{n}: {e}")).collect();
                    let resp = ui.add(egui::Label::new(RichText::new(format!("{} not open ·", names.join(", "))).color(t.error)).truncate().show_tooltip_when_elided(false));
                    theme::hint(resp, why.join("\n"));
                }
                if let Some(n) = notice {
                    ui.add_space(16.0);
                    let color = if n.error { t.error } else { t.text };
                    hovered = ui.add(egui::Label::new(RichText::new(&n.text).color(color)).truncate()).hovered();
                }
            });
            self.notice_hovered = hovered;
        });
    }

    /// The status bar while recording (D-127): a pulsing dot, "Recording 0:12 from <input>",
    /// the input's level and a Stop button.
    fn recording_status(&mut self, ui: &mut Ui, t: &Tokens, rec: &crate::controller::RecordingView) {
        let (dot, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
        paint_rec_dot(ui.painter(), dot.center(), 5.0, t, rec_pulse(ui.ctx()));
        let text = recording_text(rec);
        ui.label(RichText::new(&text).color(t.error).strong());
        if let Some(db) = rec.level_db {
            let (bar, resp) = ui.allocate_exact_size(vec2(48.0, 6.0), Sense::hover());
            paint_input_level(ui.painter(), bar, db, t);
            theme::hint(resp, "The input's level");
        }
        let stop = theme::small_button(ui, "⏹ Stop");
        if theme::hint(theme::name(stop, "Stop Recording"), format!("Stop recording; it becomes the sound of {}", rec.title)).clicked() {
            self.run(inv!("record.stop"));
        }
    }

    fn settings_window(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let mut open = self.ctl.show_settings;
        // Scrolls, so a short or tiled window still reaches every setting.
        let max_height = (ctx.content_rect().height() - 120.0).max(200.0);
        // Zoomed in, the window can be narrower than Settings: it fits and scrolls sideways too.
        let max_width = (ctx.content_rect().width() - 40.0).max(200.0);
        egui::Window::new("Settings").open(&mut open).default_pos(self.window_pos()).collapsible(false).resizable(false).default_width(420.0_f32.min(max_width)).max_width(max_width).default_height(settings_height(ctx)).show(ctx, |ui| {
            egui::ScrollArea::both().max_height(max_height).max_width(max_width).min_scrolled_width(max_width.min(720.0)).show(ui, |ui| {
                egui::Grid::new("settings-grid").num_columns(2).spacing([14.0, 10.0]).show(ui, |ui| {
                    ui.label("Output device");
                    let current = self.ctl.store.library.output_device.clone();
                    let combo = egui::ComboBox::from_id_salt("device")
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
                    theme::hint(theme::name(combo.response, "Output device"), "The sound card or interface tiles play on: the Main output");
                    ui.end_row();

                    ui.label("");
                    ui.horizontal(|ui| {
                        if theme::hint(ui.button("Refresh list"), "Look again for sound cards and interfaces, e.g. after plugging one in").clicked() {
                            actions.push(inv!("device.list").into());
                        }
                        if theme::hint(ui.button("Reconnect"), "Reopen the output device, e.g. after it dropped out").clicked() {
                            actions.push(inv!("device.reconnect").into());
                        }
                    });
                    ui.end_row();

                    ui.label("Outputs");
                    ui.vertical(|ui| {
                        let lib = &self.ctl.store.library;
                        let devices = self.ctl.devices.clone();
                        let device_combo = |ui: &mut Ui, salt: String, current: &Option<String>, tip: &str| -> Option<Option<String>> {
                            let mut pick = None;
                            let combo = egui::ComboBox::from_id_salt(salt)
                                .width(170.0)
                                .selected_text(current.clone().unwrap_or_else(|| "System default".into()))
                                .show_ui(ui, |ui| {
                                    if ui.selectable_label(current.is_none(), "System default").clicked() {
                                        pick = Some(None);
                                    }
                                    for d in &devices {
                                        if ui.selectable_label(current.as_deref() == Some(d), d).clicked() {
                                            pick = Some(Some(d.clone()));
                                        }
                                    }
                                });
                            theme::hint(theme::name(combo.response, tip), tip);
                            pick
                        };
                        let routes = self.ctl.engine.routes();
                        for o in lib.outputs.clone() {
                            ui.horizontal(|ui| {
                                ui.label(&o.name);
                                if let Some(d) = device_combo(ui, format!("out-dev-{}", o.name), &o.device, &format!("The device the {} output plays on", o.name)) {
                                    actions.push(inv!("output.set", {"name": o.name, "device": d}).into());
                                }
                                let mut ch = o.channel;
                                if theme::hint(theme::name(ui.add(egui::DragValue::new(&mut ch).range(1..=63).prefix("ch ")), &format!("{} channel", o.name)), "The first of the two device channels this output uses").changed() {
                                    actions.push(inv!("output.set", {"name": o.name, "channel": ch}).into());
                                }
                                if !routes.iter().any(|(r, ok)| r.name == o.name && *ok) {
                                    ui.label(RichText::new("not open").small().color(Tokens::get(ui.ctx()).error));
                                }
                                if theme::hint(theme::name(theme::small_button(ui, "🗙"), &format!("Remove {}", o.name)), format!("Remove the {} output (sends to it go; tiles with none left play on Main)", o.name)).clicked() {
                                    actions.push(inv!("output.remove", {"name": o.name}).into());
                                }
                            });
                        }
                        ui.horizontal(|ui| {
                            if theme::hint(ui.button("Add Output"), "Add another output, e.g. headphones or a stream; then send tiles to it in their inspector").clicked() {
                                actions.push(inv!("output.add").into());
                                actions.push(inv!("device.list").into());
                            }
                            let mut preview = lib.preview_output;
                            if theme::hint(theme::checkbox(ui, &mut preview, "Preview to"), "Play the inspector's Preview on its own device, e.g. headphones, not on Main").changed() {
                                actions.push(inv!("settings.previewOutput", {"on": preview}).into());
                            }
                            if preview {
                                if let Some(d) = device_combo(ui, "preview-dev".into(), &lib.preview_device, "The device Preview plays on") {
                                    actions.push(inv!("settings.previewOutput", {"on": true, "device": d}).into());
                                }
                            }
                        });
                        for (n, e) in &self.ctl.engine.output_errors {
                            ui.label(RichText::new(format!("{n}: {e}")).small().color(Tokens::get(ui.ctx()).error));
                        }
                    });
                    ui.end_row();

                    ui.label("Tile keys");
                    let mut all = self.ctl.store.library.tile_keys_all_sets;
                    if theme::hint(theme::checkbox(ui, &mut all, "Work in every set"), "A tile's key plays it from any set: the current set's tile first, then the other sets in sidebar order")
                        .changed()
                    {
                        actions.push(inv!("settings.tileKeysAllSets", {"on": all}).into());
                    }
                    ui.end_row();

                    ui.label("Stop All fade");
                    let mut fade = self.ctl.store.library.stop_all_fade;
                    let fade_resp = ui.add(egui::Slider::new(&mut fade, 0.0..=5.0).step_by(0.1).custom_formatter(|x, _| if x <= 0.0 { "Immediate".into() } else { format!("{x:.1} s") }));
                    if theme::hint(theme::name(fade_resp, "Stop All fade"), "How long Stop All fades out: Esc's first press fades over this; a second press while it fades stops at once — Shift+Esc never uses it. At once is a 0.1 s de-click, so nothing clicks")
                        .changed()
                    {
                        actions.push(inv!("settings.stopAllFade", {"seconds": fade}).into());
                    }
                    ui.end_row();

                    ui.label("Guards");
                    ui.vertical(|ui| {
                        let lib = &self.ctl.store.library;
                        let mut hold = lib.stop_all_hold;
                        if theme::hint(
                            theme::checkbox(ui, &mut hold, "Hold Esc for Stop All"),
                            "Esc stops everything only after it's held for 0.3 s, so a stray tap does nothing. Shift+Esc and the Stop All button don't wait",
                        )
                        .changed()
                        {
                            actions.push(inv!("settings.stopAllHold", {"on": hold}).into());
                        }
                    });
                    ui.end_row();

                    ui.label("Updates");
                    let mut check = self.ctl.store.library.check_updates;
                    if theme::hint(
                        theme::checkbox(ui, &mut check, "Check for updates at launch"),
                        "Ask GitHub for a newer version each time the app starts, and say so only if there is one. Off, the app never goes online unless you choose Help > Check for Updates…",
                    )
                    .changed()
                    {
                        actions.push(inv!("settings.checkUpdates", {"on": check}).into());
                    }
                    ui.end_row();

                    ui.label("Speech");
                    let mut speak = self.ctl.store.library.speak_cues;
                    let voice = crate::speech::program();
                    let tip = if crate::speech::CAN_ROUTE {
                        "The system voice says a tile's title when it starts, on the Preview output's device when Preview to another output is on (otherwise the system's default output)"
                    } else {
                        "The system voice (speech-dispatcher) says a tile's title when it starts, through speech-dispatcher's own output"
                    };
                    let resp = ui.add_enabled_ui(voice.is_some() || speak, |ui| theme::checkbox(ui, &mut speak, "Speak cue names")).inner;
                    if theme::hint_or(resp, tip, "Install speech-dispatcher (spd-say) to hear cue names").changed() {
                        actions.push(inv!("settings.speakCues", {"on": speak}).into());
                    }
                    ui.end_row();

                    ui.label("Ducking");
                    ui.vertical(|ui| {
                        let lib = &self.ctl.store.library;
                        let (mut amount, mut attack, mut release) = (lib.duck_db, lib.duck_attack, lib.duck_release);
                        let mut changed = theme::hint(ui.add(egui::Slider::new(&mut amount, -40.0..=0.0).step_by(1.0).suffix(" dB").text("dip")), "How far beds dip while ducked").changed();
                        changed |= theme::hint(ui.add(egui::Slider::new(&mut attack, 0.0..=2.0).step_by(0.01).suffix(" s").text("down")), "How quickly beds dip").changed();
                        changed |= theme::hint(ui.add(egui::Slider::new(&mut release, 0.0..=5.0).step_by(0.05).suffix(" s").text("back up")), "How quickly beds come back up afterwards").changed();
                        if changed {
                            actions.push(inv!("settings.duck", {"amount": amount, "attack": attack, "release": release}).into());
                        }
                        ui.label(RichText::new("Beds dip under tiles set to duck them, and while Duck Beds (Playback menu) is on.").small().weak());
                    });
                    ui.end_row();

                    ui.label("Voice");
                    // The input menu below lists what this finds (t-m20-input-list).
                    self.ctl.list_inputs_once();
                    ui.vertical(|ui| {
                        // Shown whether or not ducking is on: recording and instant replay
                        // use this input too (t-m20-record-ux).
                        let lib = &self.ctl.store.library;
                        let current = lib.input_device.clone();
                        let default = match &self.ctl.input_default {
                            Some(d) => format!("System default input ({d})"),
                            None => "System default input".to_owned(),
                        };
                        ui.horizontal(|ui| {
                            let combo = egui::ComboBox::from_id_salt("input-device")
                                .width(240.0)
                                .selected_text(current.clone().unwrap_or_else(|| default.clone()))
                                .show_ui(ui, |ui| {
                                    if ui.selectable_label(current.is_none(), &default).clicked() {
                                        actions.push(inv!("settings.inputDevice", {"name": null}).into());
                                    }
                                    for d in &self.ctl.input_devices {
                                        if ui.selectable_label(current.as_deref() == Some(d), d).clicked() {
                                            actions.push(inv!("settings.inputDevice", {"name": d}).into());
                                        }
                                    }
                                });
                            theme::hint(theme::name(combo.response, "Input device"), "The microphone or input that voice ducking listens to, and that recording and instant replay record");
                            if theme::hint(theme::small_button(ui, "Refresh"), "Look again for inputs").clicked() {
                                actions.push(inv!("input.list").into());
                            }
                        });
                        let mut voice = lib.voice_duck;
                        if theme::hint(theme::checkbox(ui, &mut voice, "Duck beds while I talk (microphone)"), "Listen to the input and dip beds whenever it's louder than the threshold").changed() {
                            actions.push(inv!("duck.voice", {"on": voice}).into());
                        }
                        if voice {
                            let mut threshold = lib.voice_threshold_db;
                            let level = self.ctl.input.as_ref().map(|i| i.shared.level_db());
                            let resp = theme::hint(
                                ui.add(egui::Slider::new(&mut threshold, -80.0..=0.0).step_by(1.0).suffix(" dB").text("threshold")),
                                "Beds dip while the input is louder than this; the bar under it shows the input now",
                            );
                            if resp.changed() {
                                actions.push(inv!("duck.voice", {"on": true, "threshold": threshold}).into());
                            }
                            // The input level under the threshold slider's track, so the
                            // threshold can be set by talking.
                            let (rect, _) = ui.allocate_exact_size(vec2(resp.rect.width(), 6.0), Sense::hover());
                            let x = |db: f32| rect.left() + rect.width() * ((db + 80.0) / 80.0).clamp(0.0, 1.0);
                            ui.painter().rect_filled(rect, 3.0, ui.visuals().extreme_bg_color);
                            if let Some(db) = level {
                                let on = self.ctl.engine.voice();
                                let tokens = Tokens::get(ui.ctx());
                                let fill = if on { tokens.input_voice } else { tokens.input_level };
                                ui.painter().rect_filled(Rect::from_min_max(rect.min, egui::pos2(x(db), rect.max.y)), 3.0, fill);
                                ui.ctx().request_repaint_after(Duration::from_millis(50));
                            }
                            ui.painter().vline(x(threshold), rect.y_range(), Stroke::new(2.0_f32, ui.visuals().strong_text_color()));
                            let note = match (self.ctl.input_note(), &self.ctl.input) {
                                (Some(note), _) => note,
                                (None, Some(i)) => format!("Listening to {}. Set the threshold just under your speaking level.", i.device),
                                (None, None) => crate::controller::INPUT_WAITING.into(),
                            };
                            ui.label(RichText::new(note).small().weak());
                            // A microphone that hears the speakers ducks beds at their own
                            // sound, and they pump (D-127): say so when it's heard, and how
                            // to avoid it always.
                            if self.ctl.hears_board.hears() {
                                ui.label(RichText::new(crate::controller::HEARS_BOARD).small().color(Tokens::get(ui.ctx()).error));
                            } else {
                                ui.label(RichText::new("Use headphones or a close mic: a microphone that hears the speakers ducks beds at their own sound.").small().weak());
                            }
                        } else {
                            ui.label(RichText::new("Recording and instant replay use this input too.").small().weak());
                        }
                    });
                    ui.end_row();

                    ui.label("Replay");
                    ui.vertical(|ui| {
                        let mut secs = self.ctl.store.library.replay_seconds;
                        let mut on = secs > 0;
                        let mut changed = false;
                        ui.horizontal(|ui| {
                            changed |= theme::hint(theme::checkbox(ui, &mut on, "Keep the last"), "Keep recording the input, so Save Instant Replay can make a tile of what just happened").changed();
                            if secs == 0 {
                                secs = 30;
                            }
                            let keep = ui.add_enabled(on, egui::DragValue::new(&mut secs).range(5..=300).suffix(" s"));
                            changed |= theme::hint_or(theme::name(keep, "Seconds of input to keep"), "How many seconds of the input to keep, 5–300", "Turn on Keep the last first").changed();
                            ui.label("of the input for instant replay");
                        });
                        if changed {
                            actions.push(inv!("replay.enable", {"seconds": if on { secs } else { 0 }}).into());
                        }
                        if on {
                            let kept = self.ctl.replay_seconds();
                            ui.label(RichText::new(format!("{} kept. Save Instant Replay in the Playback menu (or a pad bound to replay.save) makes it a tile.", format_clock(kept, false))).small().weak());
                        }
                    });
                    ui.end_row();

                    ui.label("Pads and keys");
                    ui.vertical(|ui| {
                        let midi = self.ctl.store.library.midi.clone();
                        let status = &self.ctl.midi_status;
                        let mut on = midi.enabled;
                        if theme::hint(theme::checkbox(ui, &mut on, "Listen to MIDI controllers"), "Let pads, keys and faders on MIDI controllers play tiles and run commands").changed() {
                            actions.push(inv!("midi.enable", {"on": on}).into());
                        }
                        let ports = if let Some(e) = &status.error {
                            e.clone()
                        } else if status.connected.is_empty() {
                            "No MIDI controller connected".to_owned()
                        } else {
                            status.connected.join(", ")
                        };
                        ui.label(RichText::new(ports).small().weak());
                        let mut auto = midi.auto_map.is_some();
                        let mut base = midi.auto_map.unwrap_or(36);
                        ui.horizontal(|ui| {
                            let mut changed = theme::hint(theme::checkbox(ui, &mut auto, "Notes play tiles in order from"), "Notes with no binding of their own play this set's tiles by position").changed();
                            let note = ui.add_enabled(auto, egui::DragValue::new(&mut base).range(0..=127).custom_formatter(|n, _| crate::bindings::note_name(n as u8)));
                            changed |= theme::hint_or(theme::name(note, "First note"), "The note that plays the first tile; drag or type", "Turn on Notes play tiles first").changed();
                            if changed {
                                actions.push(inv!("midi.autoMap", {"base": if auto { json!(base) } else { Value::Null }}).into());
                            }
                        });
                        let mut lights = midi.feedback;
                        if theme::hint(
                            theme::checkbox(ui, &mut lights, "Light pads while their tile plays"),
                            "Send each bound note back to the controller; many pads read the velocity as a colour (leadstinger-cli midi.feedback --lit N --unlit N)",
                        )
                        .changed()
                        {
                            actions.push(inv!("midi.feedback", {"on": lights}).into());
                        }
                        self.pad_settings(ui, actions);
                        let bindings = self.ctl.store.library.bindings.clone();
                        if !bindings.is_empty() {
                            egui::Grid::new("bindings-grid").num_columns(3).spacing([10.0, 4.0]).show(ui, |ui| {
                                for b in &bindings {
                                    let mut enabled = b.enabled;
                                    let toggle = ui.checkbox(&mut enabled, RichText::new(b.source.describe()).small().monospace());
                                    if theme::hint(toggle, format!("Turn this binding on or off: {} runs {}", b.source.describe(), self.binding_target_label(b))).changed() {
                                        actions.push(inv!("bind.enable", {"id": b.id, "on": enabled}).into());
                                    }
                                    ui.label(RichText::new(self.binding_target_label(b)).small());
                                    if theme::hint(theme::small_button(ui, "Remove"), format!("Remove the binding for {}", b.source.describe())).clicked() {
                                        actions.push(inv!("bind.remove", {"id": b.id}).into());
                                    }
                                    ui.end_row();
                                }
                            });
                        }
                        let keys = &self.ctl.hotkey_status;
                        let keys_line = match (&keys.note, keys.failed.first()) {
                            (Some(note), _) => note.clone(),
                            (None, Some((k, e))) => format!("System-wide keys: {k} isn't available ({e})"),
                            (None, None) if keys.active.is_empty() => "System-wide keys: none yet".to_owned(),
                            (None, None) => format!("System-wide keys: {}", keys.active.join(", ")),
                        };
                        ui.label(RichText::new(keys_line).small().weak());
                        ui.label(RichText::new("Bind a pad or key to a tile from the tile's inspector, or any command with leadstinger-cli bind.add.").small().weak());
                    });
                    ui.end_row();

                    ui.label("OSC");
                    ui.vertical(|ui| {
                        let o = self.ctl.store.library.osc.clone();
                        let (mut on, mut port, mut lan) = (o.enabled, o.port, o.lan);
                        let mut changed = false;
                        ui.horizontal(|ui| {
                            changed |= theme::hint(theme::checkbox(ui, &mut on, "Listen for OSC on port"), "Let OSC messages (e.g. from QLab, Companion or TouchOSC) play tiles and run commands").changed();
                            changed |= theme::hint(theme::name(ui.add(egui::DragValue::new(&mut port).range(1024..=65535)), "OSC port"), "The UDP port to listen on, 1024–65535").changed();
                        });
                        changed |= theme::hint(
                            theme::checkbox(ui, &mut lan, "from other machines too"),
                            "Accept OSC from other machines as well as this one. OSC has no password: anyone on the network could fire sounds",
                        )
                        .changed();
                        if changed {
                            actions.push(inv!("osc.enable", {"on": on, "port": port, "lan": lan}).into());
                        }
                        let st = &self.ctl.osc_status;
                        let line = match (&st.listening, &st.error) {
                            (_, Some(e)) => e.clone(),
                            (Some(a), None) => format!("Listening on {a}; {} messages so far. Addresses: /ls/tile/1/trigger, /ls/go, /ls/stopall…", st.received),
                            (None, None) if on => "Starting…".into(),
                            (None, None) => "Off".into(),
                        };
                        ui.label(RichText::new(line).small().weak());
                    });
                    ui.end_row();

                    ui.label("QuipClipper");
                    ui.vertical(|ui| self.quip_settings(ui, actions));
                    ui.end_row();

                    ui.label("Theme");
                    let choices = self.ctl.theme_choices();
                    ui.vertical(|ui| {
                        ui.horizontal_wrapped(|ui| {
                            for (id, name) in choices {
                                let tip = match id.as_str() {
                                    "console" => "The Console look: a mixing-desk window, light or dark".to_owned(),
                                    "panel" => format!("The {} look: bold blocks and pills on black", crate::brand::PANEL_THEME),
                                    _ => format!("Your theme {name}, from the themes folder; edit its files and the window follows"),
                                };
                                let on = id == self.ctl.look.id;
                                if theme::hint(ui.selectable_label(on, &name), tip).clicked() && !on {
                                    actions.push(inv!("theme.use", {"theme": id}).into());
                                }
                            }
                        });
                        ui.horizontal(|ui| {
                            if theme::hint(theme::small_button(ui, "Import…"), "Add a theme: a .lstheme folder or zip, whose tokens.json is a FigCraft variables export").clicked() {
                                actions.push(inv!("theme.import").into());
                            }
                            if theme::hint(theme::small_button(ui, "Export…"), "Save the theme in use as a .lstheme, to start a theme from or to share").clicked() {
                                actions.push(inv!("theme.export").into());
                            }
                        });
                        // A theme below WCAG AA still shows (D-107); say where it falls short.
                        let warnings = &self.ctl.look_warnings;
                        for w in warnings.iter().take(3) {
                            ui.add(egui::Label::new(RichText::new(format!("⚠ {w}")).small().color(Tokens::get(ui.ctx()).error)).wrap());
                        }
                        if warnings.len() > 3 {
                            ui.label(RichText::new(format!("…and {} more (theme.list lists them)", warnings.len() - 3)).small().weak());
                        }
                    });
                    ui.end_row();

                    ui.label("Appearance");
                    ui.horizontal(|ui| {
                        let mut appearance = self.ctl.store.library.appearance;
                        if self.ctl.store.library.theme == ThemeKind::Console {
                            for (a, label, tip) in [
                                (Appearance::Dark, "Dark", "Light text on a dark window"),
                                (Appearance::Light, "Light", "Dark text on a light window"),
                                (Appearance::System, "System", "Follow the system's light or dark setting"),
                            ] {
                                theme::hint(ui.selectable_value(&mut appearance, a, label), tip);
                            }
                        } else {
                            // Panel is always dark: Standard or High Contrast (t-m22-panel-hc).
                            let standard = appearance != Appearance::HighContrast;
                            if theme::hint(ui.selectable_label(standard, "Standard"), tips::PANEL_STANDARD).clicked() && !standard {
                                actions.push(inv!("view.appearance", {"appearance": "standard"}).into());
                            }
                        }
                        theme::hint(ui.selectable_value(&mut appearance, Appearance::HighContrast, "High Contrast"), tips::HIGH_CONTRAST);
                        if appearance != self.ctl.store.library.appearance {
                            actions.push(inv!("view.appearance", {"appearance": appearance}).into());
                        }
                    });
                    ui.end_row();

                    ui.label("Zoom");
                    ui.horizontal(|ui| {
                        let mut zoom = self.zoom_draft.unwrap_or(self.ctl.store.library.zoom);
                        let slider = egui::Slider::new(&mut zoom, ZOOM_MIN..=ZOOM_MAX).step_by(5.0).suffix(" %");
                        let resp = theme::hint_keys(theme::name(ui.add(slider), "Zoom"), "The size of everything in the window", "⌘/Ctrl + and − step it, ⌘/Ctrl+0 resets it");
                        // A drag applies when it ends, so the window doesn't rescale under the pointer.
                        if resp.dragged() {
                            self.zoom_draft = Some(zoom);
                        } else {
                            let zoom = self.zoom_draft.take().unwrap_or(zoom);
                            if zoom != self.ctl.store.library.zoom {
                                actions.push(inv!("view.zoom", {"zoom": zoom}).into());
                            }
                        }
                        let reset = ui.add_enabled(self.ctl.store.library.zoom != 100, egui::Button::new("Reset"));
                        if theme::hint_or(theme::name(reset, "Reset zoom"), "Back to 100 % — ⌘/Ctrl+0", "Already at 100 %").clicked() {
                            actions.push(inv!("view.zoom", {"zoom": "reset"}).into());
                        }
                    });
                    ui.end_row();

                    ui.label("Tiles");
                    let mut waveforms = self.ctl.store.library.show_waveforms;
                    if theme::hint(theme::checkbox(ui, &mut waveforms, "Show waveforms on tiles"), "Draw each tile's waveform on it (View > Show Waveforms)").changed() {
                        actions.push(inv!("view.waveforms", {"on": waveforms}).into());
                    }
                    ui.end_row();

                    ui.label("");
                    let mut meters = self.ctl.store.library.show_meters;
                    if theme::hint(theme::checkbox(ui, &mut meters, "Show level meters on playing tiles"), "A thin meter beside each playing tile's time (View > Show Level Meters)").changed() {
                        actions.push(inv!("view.meters", {"on": meters}).into());
                    }
                    ui.end_row();

                    ui.label("");
                    let mut colored = self.ctl.store.library.colored_pads;
                    let tip = "Console: each pad in its tile's colour all over. Off, a pad is dark with its colour on the title, the waveform and the border. Panel's blocks don't change";
                    if theme::hint(theme::checkbox(ui, &mut colored, "Coloured pads"), tip).changed() {
                        actions.push(inv!("settings.coloredPads", {"on": colored}).into());
                    }
                    ui.end_row();

                    ui.label("Grid layout");
                    let mut grid = self.ctl.store.library.grid;
                    ui.horizontal(|ui| {
                        for mode in GridMode::ALL {
                            let (tip, why) = tips::grid_mode(mode);
                            let r = theme::role(ui.radio_value(&mut grid, mode, mode.label()), AxRole::RadioButton, mode.label(), Some(grid == mode));
                            theme::hint(r, format!("{tip} {why}"));
                        }
                    });
                    if grid != self.ctl.store.library.grid {
                        actions.push(inv!("settings.gridLayout", {"mode": grid}).into());
                    }
                    ui.end_row();

                    if cfg!(target_os = "linux") {
                        ui.label("Window system");
                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                let mut x11 = self.ctl.store.library.use_x11;
                                theme::hint(ui.selectable_value(&mut x11, false, "Wayland"), "Open the window through Wayland: sharper with fractional scaling");
                                theme::hint(ui.selectable_value(&mut x11, true, "X11 (XWayland)"), "Open the window through X11 (XWayland), for desktops where native Wayland gives trouble");
                                if x11 != self.ctl.store.library.use_x11 {
                                    actions.push(inv!("settings.windowSystem", {"x11": x11}).into());
                                }
                            });
                            let now = if self.opts.x11 { "X11" } else { "Wayland" };
                            ui.label(
                                RichText::new(format!(
                                    "Running on {now}; a change applies on the next launch. Wayland is sharper with \
                                     fractional scaling. Dropping files from a file manager and pasting copied \
                                     ones (⌘/Ctrl+V) work on both."
                                ))
                                .small()
                                .color(Tokens::get(ui.ctx()).text_dim),
                            );
                        });
                        ui.end_row();
                    }

                    ui.label("Hotkeys");
                    if theme::hint(ui.button("Reassign this set by row"), "Drop the keys set by hand in this set: every pad takes its row's key again (row 1: 1 to =, row 2: Q to ], row 3: A to ', row 4: Z to /, rows 5 to 8: the same with Option or Alt)").clicked() {
                        actions.push(inv!("set.reassignKeys").into());
                    }
                    ui.end_row();

                    ui.label("Library");
                    ui.vertical(|ui| {
                        ui.add(egui::Label::new(RichText::new(self.ctl.store.library_path().display().to_string()).monospace().small()).wrap());
                        if theme::hint(ui.button("Open Folder"), "Open the folder that holds the library and its sounds").clicked() {
                            actions.push(inv!("library.reveal").into());
                        }
                    });
                    ui.end_row();
                });
            });
        });
        self.ctl.show_settings = open;
    }

    // ── board ───────────────────────────────────────────────────────────────

    /// `drops`: a tile dropped on this one moves to its place (the grid handles drops
    /// itself in Rows mode). `slot`: its row and slot in a Rows grid, for the menu's
    /// blank slots. `columns`: the lines the board shows its tiles in when they aren't the
    /// set's rows (Auto/Fixed columns, the cue list's one), for Shift-click's rectangle.
    #[allow(clippy::too_many_arguments)]
    fn tile_interaction(&self, ui: &Ui, resp: &egui::Response, tile: &Tile, index: usize, drops: bool, slot: Option<(usize, usize)>, columns: Option<usize>, actions: &mut Vec<Action>) {
        // No modes (M22, D-147): a click selects (⌘-click adds or takes out, Shift-click
        // selects the rectangle from the selected tile, ⌘-Shift adds it, D-169; Alt-click
        // takes it out, D-229); the second press of a double-click plays, as a key would,
        // so a Hold tile plays while that press is held. Keys, Enter and Space always play.
        let m = ui.input(|i| i.modifiers);
        let locked = self.tile_locked(tile.id);
        if resp.clicked() && m.alt && !(m.command || m.shift) {
            actions.push(inv!("view.select", {"tile": tile.id, "remove": true}).into());
        } else if resp.clicked() {
            let mut params = json!({"tile": tile.id, "add": m.command, "range": m.shift});
            if let Some(c) = columns.filter(|_| m.shift) {
                params["columns"] = json!(c);
            }
            actions.push(Invocation::new("view.select", params).into());
        }
        if resp.is_pointer_button_down_on() && ui.input(|i| i.pointer.primary_pressed()) {
            // iPadOS: a finger down plays, as a key would (several fingers: app/touch.rs).
            if !(m.command || m.shift || m.alt) && (cfg!(target_os = "ios") || second_press(ui, tile.id)) {
                actions.push(Action::Press(tile.id));
            }
        } else if let Some(key) = activation_key(ui, resp) {
            actions.push(Action::KeyPress(tile.id, key));
        }
        // A tile clicked or dragged takes the keyboard focus, so the arrows and
        // ⌘/Ctrl+arrows act on it next (not the master volume). Not an Alt-click: the
        // focus would select the tile it just took out (`focus_selects`).
        if (resp.clicked() && !m.alt) || resp.drag_started() {
            resp.request_focus();
        }
        if !locked {
            resp.dnd_set_drag_payload(tile.id);
            if let Some(src) = drops.then(|| resp.dnd_release_payload::<Uuid>()).flatten() {
                if !self.drag_group(*src).contains(&tile.id) {
                    actions.push(self.drag_move(ui.ctx(), *src, json!({"to": index + 1})));
                }
            }
        }
        resp.context_menu(|ui| {
            // Edit… opens the waveform editor, as the inspector's Edit… does (read-only in
            // a locked set); a click on the tile already shows it in the inspector.
            let why = if tile.action.is_some() { "An action tile has no sound to edit" } else { "This tile has no sound yet" };
            let edit = ui.add_enabled(tile.action.is_none() && tile.file.is_some(), egui::Button::new("Edit…"));
            if theme::hint_or(theme::name(edit, "Edit waveform"), editor::OPEN_TIP, why).clicked() {
                actions.push(inv!("view.editor", {"tile": tile.id, "open": true}).into());
            }
            let previewing = self.ctl.statuses.contains_key(&tile.id);
            let preview = ui.add_enabled(tile.file.is_some(), egui::Button::new(if previewing { "Stop" } else { "Play" }));
            let (tip, why) = (if previewing { "Stop this tile now" } else { "Play this tile, whatever its play mode" }, "This tile has no sound yet");
            if theme::hint_or(preview, tip, why).clicked() {
                actions.push(inv!("tile.preview", {"tile": tile.id}).into());
            }
            if self.ctl.current_set().layout == Layout::List {
                let played = self.ctl.played.contains(&tile.id);
                if theme::hint(ui.button(if played { "Arm Cue" } else { "Mark as Played" }), tips::CUE_ARM).clicked() {
                    actions.push(inv!("cue.arm", {"tile": tile.id, "armed": played}).into());
                }
            }
            ui.separator();
            // A locked set's tile only plays: the rest of the menu would change it.
            ui.add_enabled_ui(!locked, |ui| self.tile_edit_menu(ui, tile, slot, actions));
        });
    }

    /// The tile menu's items that change the tile or its set.
    fn tile_edit_menu(&self, ui: &mut Ui, tile: &Tile, slot: Option<(usize, usize)>, actions: &mut Vec<Action>) {
        let hint = |r: egui::Response, tip: &str| theme::hint_or(r, tip, tips::LOCKED_WHY);
        {
            if hint(ui.button("Replace Audio…"), "Choose another sound file for this tile, keeping its settings").clicked() {
                actions.push(inv!("tile.replaceAudio", {"tile": tile.id}).into());
            }
            let picture_label = if tile.image.is_some() { "Change Picture…" } else { "Choose Picture…" };
            if hint(ui.button(picture_label), "Choose a picture to show on the tile, or drop an image file onto it").clicked() {
                actions.push(inv!("tile.picture", {"tile": tile.id}).into());
            }
            if tile.image.is_some() && hint(ui.button("Remove Picture"), "Take the picture off this tile").clicked() {
                actions.push(inv!("tile.pictureClear", {"tile": tile.id}).into());
            }
            let mute_tip = if tile.muted { "Let this tile be heard again, at its volume" } else { PadOption::Muted.tip() };
            if hint(ui.button(if tile.muted { "Unmute" } else { "Mute" }), mute_tip).clicked() {
                actions.push(inv!("tile.set", {"tile": tile.id, "muted": !tile.muted}).into());
            }
            let toggle_tip = if tile.disabled { "Let keys, pads and GO play this tile again" } else { "Make keys, pads and GO skip this tile" };
            if hint(ui.button(if tile.disabled { "Enable" } else { "Disable" }), toggle_tip).clicked() {
                actions.push(Invocation::new(if tile.disabled { "tile.enable" } else { "tile.disable" }, json!({"tile": tile.id})).into());
            }
            if tile.file.is_some() {
                let set = self.ctl.store.library.locate(tile.id).map(|(si, _)| self.ctl.store.library.sets[si].id);
                if hint(ui.button("Add a Fade-Out Cue for It"), "Add a tile at the end of the set that fades this one out").clicked() {
                    actions.push(inv!("tile.addAction", {"kind": "fade", "targets": [tile.id], "set": set}).into());
                }
                if hint(ui.button("Add a Stop Cue for It"), "Add a tile at the end of the set that stops this one").clicked() {
                    actions.push(inv!("tile.addAction", {"kind": "stop", "targets": [tile.id], "set": set}).into());
                }
            }
            if let Some((row, at)) = slot {
                if hint(ui.button("Insert Blank Before"), "Leave an empty slot before this tile in its row").clicked() {
                    actions.push(inv!("set.addBlank", {"row": row + 1, "slot": at + 1}).into());
                }
                if hint(ui.button("Insert Blank After"), "Leave an empty slot after this tile in its row").clicked() {
                    actions.push(inv!("set.addBlank", {"row": row + 1, "slot": at + 2}).into());
                }
            }
            if hint(ui.button("Duplicate"), "Copy this tile, right after it").clicked() {
                actions.push(inv!("tile.duplicate", {"tile": tile.id}).into());
            }
            if hint(ui.button(RichText::new("Remove").color(Tokens::get(ui.ctx()).error)), "Remove this tile (Undo brings it back)").clicked() {
                actions.push(inv!("tile.remove", {"tile": tile.id}).into());
            }
        }
    }

    /// The set this tile is in is locked.
    fn tile_locked(&self, id: Uuid) -> bool {
        self.ctl.store.library.locate(id).is_some_and(|(si, _)| self.ctl.store.library.sets[si].locked)
    }

    /// Clicking a set in the sidebar: show it, and its settings in the inspector (M22),
    /// which a selected tile of that set would otherwise keep.
    fn show_set(&self, id: Uuid) -> Vec<Action> {
        let mut out = vec![inv!("set.select", {"set": id}).into()];
        if self.ctl.current_set().id == id && self.ctl.selected_tile.is_some() {
            out.push(inv!("view.select").into());
        }
        out
    }

    /// A tile's meter, when meters are shown and it has one (it's sounding or paused).
    fn tile_meter(&self, id: Uuid) -> Option<Ballistics> {
        self.ctl.store.library.show_meters.then(|| self.ctl.levels.get(&id).copied()).flatten()
    }

    /// The recording mark for this tile, while it's being recorded into.
    fn rec_mark(&self, ctx: &egui::Context, id: Uuid) -> Option<RecMark> {
        let r = self.ctl.recording_view().filter(|r| r.tile == id)?;
        Some(RecMark { level_db: r.level_db, pulse: rec_pulse(ctx) })
    }

    /// What a tile being recorded into says where its waveform goes: the input.
    fn rec_caption(&self, id: Uuid) -> Option<String> {
        let r = self.ctl.recording_view().filter(|r| r.tile == id)?;
        Some(match r.input {
            Some(input) => format!("From {input}"),
            None => crate::controller::INPUT_WAITING.into(),
        })
    }

    /// What an action tile does, in a few words, for the tile itself.
    fn action_caption(&self, tile: &Tile) -> Option<String> {
        let a = tile.action.as_ref()?;
        let targets = self.ctl.action_json(a)["targets"].clone();
        let names: Vec<String> = targets
            .as_array()
            .into_iter()
            .flatten()
            .map(|t| match (t["title"].as_str(), t["name"].as_str()) {
                (Some(title), _) => title.to_owned(),
                (None, Some(set)) => format!("all of {set}"),
                _ => "a removed tile".to_owned(),
            })
            .collect();
        if names.is_empty() {
            return Some("No targets yet: choose them in the inspector".into());
        }
        let verb = match a.kind {
            ActionKind::Fade => "Fades",
            ActionKind::Stop => "Stops",
            ActionKind::Pause => "Pauses",
            ActionKind::PlayOn => "Plays on",
            ActionKind::Play => "Plays",
        };
        let shown = names.iter().take(3).cloned().collect::<Vec<_>>().join(", ");
        let more = names.len().saturating_sub(3);
        Some(if more > 0 { format!("{verb} {shown} and {more} more") } else { format!("{verb} {shown}") })
    }

    fn time_label(&self, tile: &Tile, status: Option<&VoiceStatus>) -> String {
        if let Some(left) = self.ctl.waiting(tile.id) {
            return format!("Wait {left:.1}");
        }
        if let Some(a) = &tile.action {
            let last = a.targets.iter().map(|t| t.offset).fold(0.0, f64::max);
            return match a.kind {
                ActionKind::Fade => format!("{:.1} s", a.fade),
                ActionKind::Play if last > 0.0 => format!("+{last:.1} s"),
                _ => String::new(),
            };
        }
        if let Some(r) = self.ctl.recording.as_ref().filter(|r| r.tile == tile.id) {
            return format!("REC {}", format_clock(r.seconds(), false));
        }
        if let Some(s) = status {
            // Count down in whole seconds, rounding up so a sounding tile never shows 0:00.
            return format!("−{}", format_clock((s.remaining() - 1e-3).max(0.0).ceil(), false));
        }
        match self.ctl.store.media(tile) {
            None => "Drop audio here".into(),
            Some(Media::Loading) => match self.ctl.store.import_of(tile) {
                Some(i) if i.busy() => match i.fraction() {
                    Some(f) => format!("{} {:.0}%", i.verb(), f * 100.0),
                    None => format!("{}…", i.verb()),
                },
                _ => "Loading…".into(),
            },
            Some(Media::Failed(e)) if e == crate::transcode::CANCELLED => "Cancelled".into(),
            Some(Media::Failed(_)) => "⚠ Unreadable".into(),
            Some(Media::Ready { audio, .. }) => {
                let (s, e) = play_range(tile, audio.duration());
                // Short clips get tenths so they never read as 0:00.
                let heard = (e - s) / tile.speed.clamp(0.5, 2.0) as f64;
                format_clock(heard, heard < 10.0)
            }
        }
    }

    /// The central panel: the search box when it's open, then the grid or cue list.
    fn board(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        // Only a Rows grid drawn this frame says where files land.
        self.row_grid = None;
        self.file_marker = false;
        if self.ctl.editor.is_some() {
            self.editor(ui, actions);
            return;
        }
        self.editor_view = None;
        if self.ctl.search.is_some() {
            self.search_bar(ui, actions);
        }
        match self.ctl.current_set().layout {
            Layout::Grid => self.grid(ui, actions),
            Layout::List => self.list(ui, actions),
        }
        self.reveal = None;
    }

    /// ⌘/Ctrl+F: a box that filters the board across every set. Enter plays the top hit,
    /// Esc (while typing) or ✕ closes it.
    fn search_bar(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let t = Tokens::get(ui.ctx());
        let mut query = self.ctl.search.clone().unwrap_or_default();
        let hits = self.ctl.search_hits(&query);
        ui.horizontal(|ui| {
            let resp = theme::text_field(
                ui,
                egui::TextEdit::singleline(&mut query).hint_text("Find tiles in every set, by title or notes").desired_width(360.0),
            );
            let resp = theme::hint_keys(theme::name(resp, "Find"), "Find tiles in every set by title or notes; the board shows only the matches", "Enter plays the first, Esc closes");
            if std::mem::take(&mut self.search_focus) {
                resp.request_focus();
            }
            if resp.changed() {
                actions.push(inv!("view.search", {"query": query}).into());
            }
            if resp.lost_focus() {
                if ui.input(|i| i.key_pressed(Key::Enter)) {
                    actions.push(inv!("view.search", {"play": true}).into());
                } else if ui.input(|i| i.key_pressed(Key::Escape)) {
                    actions.push(inv!("view.search", {"close": true}).into());
                }
            }
            let sets = hits.iter().filter_map(|id| self.ctl.store.library.locate(*id)).map(|(s, _)| s).collect::<std::collections::HashSet<_>>().len();
            let summary = match (query.trim().is_empty(), hits.len()) {
                (true, _) => "Type to filter; Enter plays the first tile".to_owned(),
                (false, 0) => "No tiles match".to_owned(),
                (false, n) => format!("{n} tile{} in {sets} set{}; Enter plays the first", if n == 1 { "" } else { "s" }, if sets == 1 { "" } else { "s" }),
            };
            ui.label(RichText::new(summary).color(t.text_dim));
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                if theme::hint_keys(theme::name(theme::small_button(ui, "🗙"), "Close search"), "Close the search and show the whole set again", "Esc in the box").clicked() {
                    actions.push(inv!("view.search", {"close": true}).into());
                }
            });
        });
        ui.add_space(10.0);
    }

    fn grid(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let set = self.ctl.current_set();
        // Fixed ids carry the set: a set window's board is drawn in the same pass (D-223).
        let set_id = set.id;
        let lib = &self.ctl.store.library;
        // While searching, the board holds the hits from every set and no add slot.
        let hits = self.ctl.search.as_deref().map(|q| self.ctl.search_hits(q));
        let tiles: Vec<&Tile> = match &hits {
            Some(h) => h.iter().filter_map(|id| lib.tile(*id)).collect(),
            None => set.tiles.iter().collect(),
        };
        // Search hits come from several sets, so they have no rows (D-114).
        let mode = if hits.is_some() { GridMode::Columns } else { lib.grid };
        let rows_mode = mode == GridMode::Rows;
        let slots = set.slots();
        let view_h = ui.available_height();
        let ctx = ui.ctx().clone();
        // What's being dragged over a Rows grid: a pad or files (never into a locked set).
        let locked = set.locked && hits.is_none();
        let from = rows_mode
            .then(|| egui::DragAndDrop::payload::<Uuid>(&ctx))
            .flatten()
            .filter(|_| !locked)
            .and_then(|id| set.tiles.iter().position(|t| t.id == *id));
        // The pads it moves: the selection it's part of, or itself (D-219).
        let group: Vec<usize> = from.map_or_else(Vec::new, |f| {
            let ids = self.drag_group(set.tiles[f].id);
            set.tiles.iter().enumerate().filter(|(_, t)| ids.contains(&t.id)).map(|(i, _)| i).collect()
        });
        let (files, any_files, pos, released) = ctx.input(|i| {
            let sounds = i.raw.hovered_files.iter().filter(|f| f.path.as_deref().is_none_or(|p| !is_image_path(p) && !crate::setfile::is_set_file(p))).count() * usize::from(!locked);
            (sounds, !i.raw.hovered_files.is_empty(), i.pointer.latest_pos(), i.pointer.any_released())
        });
        let mut anim = std::mem::take(&mut self.slot_anim);
        let mut grab = self.drag_grab;
        let mut rects = Vec::new();
        let mut focused = None;
        let mut row_grid = None;
        let mut file_marker = false;
        let mut moving = false;
        // Rows never wrap (D-170): a row too wide for the board at the smallest pads
        // scrolls sideways.
        egui::ScrollArea::both().auto_shrink(false).show(ui, |ui| {
            let is_panel = theme::is_panel(ui.ctx());
            // Panel's outlines sit outside its pads, so the board keeps room for them
            // round the edge; the scroll area would cut them off (t-m22-panel-polish).
            let reach = if is_panel { panel::OUTLINE_REACH } else { 0.0 };
            let view_h = view_h - 2.0 * reach;
            let avail = ui.available_width() - 2.0 - 2.0 * reach;
            let height: fn(f32) -> f32 = if is_panel { panel::tile_height } else { tile_height };
            let min_h = if is_panel { panel::ROW_MIN_HEIGHT } else { ROW_MIN_HEIGHT };
            let auto_sizing = grid::Sizing { min_w: if is_panel { grid::PANEL_ROW_MIN_WIDTH } else { grid::ROW_MIN_WIDTH }, height, min_h };
            let rows_sizing = grid::Sizing { min_w: grid::PAD_MIN_WIDTH, ..auto_sizing };
            // A locked set has no add slot.
            let count = tiles.len() + usize::from(hits.is_none() && !locked);
            let origin = ui.cursor().min + vec2(reach, reach);
            let columns = set.columns as usize;
            let fill = Some((view_h - 1.0).floor());
            // The lines Shift-click's rectangle follows when they aren't the set's rows
            // (search hits, from several sets: the run in the set's order).
            let (base, shown_columns) = match mode {
                GridMode::Rows => (grid::rows(origin, &rows::slot_counts(&slots), !locked, columns, avail, rows_sizing, fill), None),
                GridMode::Auto => {
                    let c = grid::auto_columns(count, avail, view_h, auto_sizing);
                    (grid::rows(origin, &grid::uniform(count, c), false, c, avail, auto_sizing, None), Some(c))
                }
                GridMode::Columns => {
                    let min_w = if is_panel { panel::TILE_MIN_WIDTH } else { TILE_MIN_WIDTH };
                    let c = fit_columns(set.columns.max(1) as usize, avail, min_w, grid::GAP);
                    let lines = if hits.is_some() { 1 } else { c };
                    (grid::rows(origin, &grid::uniform(count, c), false, c, avail, grid::Sizing { min_w, height, min_h }, None), Some(lines))
                }
            };
            // The drop under the pointer, worked out on the board as it is (D-141).
            let over = pos.filter(|p| ui.clip_rect().contains(*p) && (from.is_some() || files > 0));
            let target = over.map(|p| grid::target_at(&base, slots.len(), |r, a| slots[r][a].is_none(), p));
            let active = rows_mode && target.is_some();
            // Alt held: the drop makes copies and the pads stay (D-229), so the board
            // shows them going in as files do.
            let copying = from.is_some() && files == 0 && drag_copies(&ctx);
            // What each slot shows: the drop's result while dragging, else the set.
            let shown: Grid<grid::Show> = if active && copying {
                grid::preview(&slots, &[], group.len(), target, columns)
            } else if active {
                grid::preview(&slots, &group, files, target, columns)
            } else {
                slots.iter().map(|r| r.iter().map(|s| s.map(grid::Show::Pad)).collect()).collect()
            };
            // A phantom last row takes the add slot's place while it shows.
            let add = !matches!(target, Some(Place::NewRow(r)) if r >= slots.len());
            let layout = if active { grid::rows(origin, &rows::slot_counts(&shown), add && !locked, columns, avail, rows_sizing, fill) } else { base.clone() };
            ui.allocate_rect(Rect::from_min_max(origin - vec2(reach, reach), pos2(origin.x + layout.width + reach, layout.bottom + reach)), Sense::hover());
            // A row number for screen readers once there's more than one (D-116).
            let speak_rows = rows_mode && slots.len() > 1;
            let t = Tokens::get(ui.ctx());
            let animate = rows_mode && (active || self.anim_until.is_some_and(|u| Instant::now() < u)) && ui.style().animation_time > 0.0;
            let dt = ui.input(|i| i.stable_dt).min(0.1);
            let mut glide = |key: egui::Id, to: Rect| -> Rect {
                let r = match anim.get(&key) {
                    Some(&from) if animate => {
                        let k = 1.0 - (-dt / 0.06).exp();
                        let r = Rect::from_min_max(from.min + (to.min - from.min) * k, from.max + (to.max - from.max) * k);
                        if (r.min - to.min).length() + (r.max - to.max).length() < 1.0 { to } else { r }
                    }
                    _ => to,
                };
                moving |= r != to;
                anim.insert(key, r);
                r
            };
            // A phantom row: outlined, with the ghost in it.
            if let (true, Some(Place::NewRow(r))) = (active, target) {
                if let Some(b) = layout.bands.get(r) {
                    let band = Rect::from_min_max(pos2(layout.left, b.top), pos2(layout.left + layout.width, b.bottom)).expand(4.0);
                    dashed_outline(ui.painter(), band, Stroke::new(1.5_f32, t.accent));
                }
            }
            let mut flat = 0;
            let mut held = false;
            for cell in &layout.cells {
                let rect = cell.rect;
                // The tile in this cell: by slot in Rows mode, by order in the others.
                let i = if cell.add {
                    None
                } else if rows_mode {
                    match shown.get(cell.row).and_then(|r| r.get(cell.at)).copied().flatten() {
                        Some(grid::Show::Pad(i)) => Some(i),
                        Some(grid::Show::Ghost) => {
                            let ghost = glide(egui::Id::unique(("ghost", cell.row, cell.at)), rect);
                            paint_ghost(ui.painter(), ghost, if copying { t.meter_green } else { t.accent });
                            // The dragged pad keeps its widget (on the first ghost of
                            // several), so its drag goes on; a copy's pad is still there.
                            if let Some(src) = from.and_then(|f| tiles.get(f)).filter(|_| files == 0 && !held && !copying) {
                                ui.interact(ghost, egui::Id::unique(("tile", src.id)), Sense::click_and_drag());
                                held = true;
                            }
                            continue;
                        }
                        None => {
                            self.blank_slot(ui, rect, cell.row, cell.at, active, locked, actions);
                            continue;
                        }
                    }
                } else {
                    flat += 1;
                    Some(flat - 1)
                };
                // A tile keeps its id wherever it moves, so keyboard focus follows it.
                let id = i.and_then(|i| tiles.get(i)).map_or(egui::Id::unique(("add-slot", set_id)), |t| egui::Id::unique(("tile", t.id)));
                let rect = if rows_mode { glide(id, rect) } else { rect };
                let resp = ui.interact(rect, id, Sense::click_and_drag());
                if let Some(&tile) = i.and_then(|i| tiles.get(i)) {
                    let i = i.unwrap_or_default();
                    if resp.has_focus() {
                        focused = Some(tile.id);
                    }
                    // A pad the keyboard moves to comes into view, sideways too (D-170);
                    // a clicked one doesn't move, so a double-click stays on it.
                    if resp.has_focus() && self.focused_tile != Some(tile.id) && !ui.input(|i| i.pointer.any_down() || i.pointer.any_released()) {
                        resp.scroll_to_me(None);
                    }
                    if resp.drag_started() {
                        grab = resp.interact_pointer_pos().map(|p| p - rect.min);
                    }
                    // A hit's place is the one in its own set.
                    let (set_name, i) = match lib.locate(tile.id) {
                        Some((si, ti)) if hits.is_some() => (Some(lib.sets[si].name.as_str()), ti),
                        _ => (None, i),
                    };
                    let on = Some(self.ctl.is_selected(tile.id));
                    let row = speak_rows.then_some(cell.row);
                    let resp = theme::role(resp, AxRole::Button, self.tile_label(tile, i, set_name, None, row), on);
                    let resp = theme::hint(resp, self.tile_tip(tile, set_name));
                    rects.push((tile.id, i, rect));
                    // In Rows mode the grid works out where a dragged tile lands (below).
                    let slot = (rows_mode && hits.is_none()).then_some((cell.row, cell.at));
                    self.tile_interaction(ui, &resp, tile, i, !rows_mode, slot, shown_columns, actions);
                    let drop_hover = !rows_mode && !locked && resp.dnd_hover_payload::<Uuid>().is_some();
                    let paint = if is_panel { panel::paint_tile } else { paint_tile };
                    let peaks = self.shown_peaks(tile);
                    let status = self.ctl.statuses.get(&tile.id);
                    paint(ui, rect, tile, status, &self.time_label(tile, status), self.tile_flags(ui, tile, peaks.as_deref().map(|p| p.as_slice()), &resp, drop_hover));
                    if self.reveal == Some(tile.id) {
                        ui.scroll_to_rect(rect, Some(Align::Center));
                    }
                } else {
                    if is_panel {
                        panel::paint_add_slot(ui, rect, resp.hovered());
                    } else {
                        paint_add_slot(ui, rect, resp.hovered());
                    }
                    let resp = theme::role(resp, AxRole::Button, "Add sounds", None);
                    let resp = theme::hint_keys(resp, tips::ADD_SLOT.0, tips::ADD_SLOT.1);
                    if resp.clicked() {
                        actions.push(inv!("tile.add").into());
                    }
                    if !rows_mode && !locked {
                        if let Some(src) = resp.dnd_release_payload::<Uuid>() {
                            actions.push(self.drag_move(ui.ctx(), *src, json!({"to": u32::MAX})));
                        }
                    }
                }
            }
            // A click on the board between its tiles clears the selection, so the
            // inspector shows the set's settings (D-148).
            let clicked = ui.input(|i| i.pointer.primary_clicked().then(|| i.pointer.interact_pos()).flatten());
            if let Some(p) = clicked.filter(|p| ui.clip_rect().contains(*p) && !layout.cells.iter().any(|c| c.rect.contains(*p))) {
                if self.ctl.selected_tile.is_some() && ui.ctx().layer_id_at(p) == Some(ui.layer_id()) && !egui::Popup::is_any_open(ui.ctx()) {
                    actions.push(inv!("view.select").into());
                }
            }
            if rows_mode {
                // The dragged pad follows the pointer, over everything.
                if let (Some(tile), Some(p)) = (from.and_then(|f| tiles.get(f)).copied(), pos.filter(|_| files == 0)) {
                    let size = base.cells.iter().find(|c| !c.add && slots.get(c.row).and_then(|r| r.get(c.at)) == Some(&from)).map_or(vec2(160.0, 100.0), |c| c.rect.size());
                    let r = Rect::from_min_size(p - grab.unwrap_or(size / 2.0), size);
                    let paint = if is_panel { panel::paint_tile } else { paint_tile };
                    let peaks = self.shown_peaks(tile);
                    let flags = TileFlags { selected: true, ..self.tile_flags_plain(tile, peaks.as_deref().map(|p| p.as_slice())) };
                    let count = group.len();
                    egui::Area::new(egui::Id::unique("dragged-pad")).order(egui::Order::Tooltip).fixed_pos(r.min).interactable(false).show(ui.ctx(), |ui| {
                        ui.multiply_opacity(0.85);
                        paint(ui, r, tile, None, &self.time_label(tile, None), flags);
                        // Several pads dragged together: how many, on its corner.
                        if count > 1 {
                            let c = r.right_top() + vec2(-4.0, 4.0);
                            ui.painter().circle_filled(c, 13.0, t.accent);
                            ui.painter().text(c, Align2::CENTER_CENTER, count.to_string(), theme::bold(13.0), theme::ink_on(t.accent));
                        }
                        if copying {
                            paint_copy_badge(ui.painter(), r.left_top() + vec2(4.0, 4.0), t.meter_green);
                        }
                    });
                }
                // A pad let go over the grid moves there; with Alt, its copies go in
                // there, on the board as it is.
                if let (Some(_), true, Some(place)) = (from.filter(|_| released), active, target) {
                    let src = egui::DragAndDrop::take_payload::<Uuid>(ui.ctx()).map(|s| *s);
                    if let (Some(src), true) = (src, copying) {
                        actions.push(self.drag_move(&ctx, src, grid::place_json(place)));
                    } else if let (Some(src), Some(m)) = (src, grid::drag_move(&slots, &group, place)) {
                        let mut params = grid::place_json(m.place);
                        if m.leave_blank {
                            params["leaveBlank"] = json!(true);
                        }
                        actions.push(self.drag_move(&ctx, src, params));
                    }
                }
                file_marker = any_files && over.is_some() || any_files && files == 0;
                row_grid = Some((base, slots.clone()));
            }
        });
        if moving {
            ctx.request_repaint();
        }
        if from.is_some() || files > 0 {
            self.anim_until = Some(Instant::now() + Duration::from_millis(400));
        }
        if !ctx.input(|i| i.pointer.any_down()) {
            grab = None;
        }
        self.slot_anim = anim;
        self.drag_grab = grab;
        self.tile_rects = rects;
        self.focus_selects(ui, focused, actions);
        self.focused_tile = focused;
        self.row_grid = row_grid;
        self.file_marker = file_marker;
    }

    /// The pads a drag of `tile` moves (D-219): the selection in its set, in the set's
    /// order, when the tile is part of it; otherwise the tile alone.
    fn drag_group(&self, tile: Uuid) -> Vec<Uuid> {
        let lib = &self.ctl.store.library;
        match lib.locate(tile) {
            Some((si, _)) if self.ctl.is_selected(tile) => lib.sets[si].tiles.iter().map(|t| t.id).filter(|id| self.ctl.is_selected(*id)).collect(),
            _ => vec![tile],
        }
    }

    /// `tile.move` for a drag of `tile`: the tile, or the selection it's part of; with
    /// Alt held at the drop, `tile.copy` (D-229).
    fn drag_move(&self, ctx: &egui::Context, tile: Uuid, mut params: Value) -> Action {
        match self.drag_group(tile).as_slice() {
            [one] => params["tile"] = json!(one),
            group => params["tiles"] = json!(group),
        }
        Invocation::new(if drag_copies(ctx) { "tile.copy" } else { "tile.move" }, params).into()
    }

    /// A pad (or the selection it's part of, D-219) dropped on a set in the sidebar
    /// moves to that set's end, or with Alt its copies go there (D-229); not moved into
    /// its own set, or into or out of a locked one.
    fn drop_on_set(&self, resp: &egui::Response, set: Uuid, actions: &mut Vec<Action>) -> bool {
        let lib = &self.ctl.store.library;
        let Some(src) = egui::DragAndDrop::payload::<Uuid>(&resp.ctx) else { return false };
        let ok = match (lib.locate(*src), lib.set_index(set)) {
            (Some((from, _)), Some(to)) => (from != to || drag_copies(&resp.ctx)) && !lib.sets[from].locked && !lib.sets[to].locked,
            _ => false,
        };
        if !ok || !resp.contains_pointer() {
            return false;
        }
        if resp.dnd_release_payload::<Uuid>().is_some() {
            actions.push(self.drag_move(&resp.ctx, *src, json!({"set": set})));
        }
        true
    }

    /// The keyboard's focus selects the tile it lands on (M22); a click selects by itself.
    fn focus_selects(&self, ui: &Ui, focused: Option<Uuid>, actions: &mut Vec<Action>) {
        let Some(id) = focused.filter(|f| Some(*f) != self.focused_tile && !self.ctl.is_selected(*f)) else { return };
        if !ui.input(|i| i.pointer.any_down() || i.pointer.any_released()) {
            actions.push(inv!("view.select", {"tile": id}).into());
        }
    }

    /// A tile's flags as the board draws it.
    fn tile_flags<'a>(&'a self, ui: &Ui, tile: &Tile, peaks: Option<&'a [f32]>, resp: &egui::Response, drop_hover: bool) -> TileFlags<'a> {
        // From memory, as the focus ring reads it: egui's `has_focus` is false while the
        // window is in the background, which left a Panel pad with the ring's rectangle.
        let focus = ui.memory(|m| m.has_focus(resp.id)).then_some(resp.id);
        TileFlags { drop_hover, hovered: resp.hovered(), recording: self.rec_mark(ui.ctx(), tile.id), focus, ..self.tile_flags_plain(tile, peaks) }
    }

    /// A tile's flags without the pointer; `peaks` from [`Self::shown_peaks`].
    fn tile_flags_plain<'a>(&'a self, tile: &Tile, peaks: Option<&'a [f32]>) -> TileFlags<'a> {
        let duration = self.ctl.store.audio(tile).map(|a| a.duration());
        TileFlags {
            recording: None,
            selected: self.ctl.is_selected(tile.id),
            drop_hover: false,
            hovered: false,
            failed: matches!(self.ctl.store.media(tile), Some(Media::Failed(_))),
            waveform: peaks.zip(duration),
            picture: self.tile_texture(tile),
            caption: self.rec_caption(tile.id).or_else(|| self.action_caption(tile)),
            level: self.tile_meter(tile.id),
            colored: self.ctl.store.library.colored_pads,
            played: false,
            focus: None,
        }
    }

    /// A tile's waveform, when waveforms are shown.
    fn shown_peaks(&self, tile: &Tile) -> Option<Arc<Vec<f32>>> {
        if self.ctl.store.library.show_waveforms { self.ctl.store.peaks(tile) } else { None }
    }

    /// A blank slot of a Rows grid (D-143): a faint dashed outline, focusable, with a menu
    /// that removes it; in a locked set it's empty space, unless something is dragged
    /// over the board (D-150).
    #[allow(clippy::too_many_arguments)]
    fn blank_slot(&self, ui: &Ui, rect: Rect, row: usize, at: usize, dragging: bool, locked: bool, actions: &mut Vec<Action>) {
        if locked && !dragging {
            return;
        }
        let t = Tokens::get(ui.ctx());
        let faint = if dragging { t.border_light } else { theme::with_alpha(t.border_light, 0.8) };
        dashed_outline(ui.painter(), rect.shrink(1.0), Stroke::new(1.0_f32, faint));
        if locked {
            return;
        }
        let resp = ui.interact(rect, egui::Id::unique(("blank", row, at)), Sense::click());
        let resp = theme::role(resp, AxRole::Button, format!("Blank slot, row {}, slot {}", row + 1, at + 1), None);
        let resp = theme::hint(resp, "A blank slot: drop a sound here to fill it; right-click to remove it");
        let remove = inv!("set.removeBlank", {"row": row + 1, "slot": at + 1});
        if resp.has_focus() && ui.input(|i| i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace)) {
            actions.push(remove.clone().into());
        }
        resp.context_menu(|ui| {
            if theme::hint(ui.button("Remove Blank"), "Take this blank slot out; the slots after it move up").clicked() {
                actions.push(remove.into());
            }
        });
    }

    fn list(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let t = Tokens::get(ui.ctx());
        let si = self.ctl.set_idx();
        let set = &self.ctl.store.library.sets[si];
        // Queued, while a cue plays: the one that waits for it (D-197), marked NEXT.
        let queued = self.ctl.list_queued(si).filter(|_| !set.auto_advance);
        let mut auto_advance = set.auto_advance;
        let mut crossfade = set.crossfade;
        let mut reset = false;

        let is_panel = theme::is_panel(ui.ctx());
        // While searching, the rows are the hits from every set: no list bar or add slot.
        let hits = self.ctl.search.as_deref().map(|q| self.ctl.search_hits(q));
        if hits.is_none() {
            self.set_notes(ui, si);
            ui.add_space(6.0);
        }
        // The bar under the list (D-199): Continuous | Queued, the cross-fade, Reset List;
        // on a narrow board in two lines.
        let narrow = ui.available_width() < 560.0;
        let bar_h = match (&hits, narrow) {
            (Some(_), _) => 0.0,
            (None, false) => 46.0,
            (None, true) => 86.0,
        };
        let lib = &self.ctl.store.library;
        let rows: Vec<(usize, &Tile)> = match &hits {
            Some(h) => h.iter().filter_map(|id| lib.locate(*id).map(|(s, i)| (i, &lib.sets[s].tiles[i]))).collect(),
            None => set.tiles.iter().enumerate().collect(),
        };

        let mut rects = Vec::new();
        let mut focused = None;
        egui::ScrollArea::vertical().auto_shrink(false).max_height((ui.available_height() - bar_h).max(60.0)).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            for (i, tile) in rows {
                let has_notes = !tile.notes.trim().is_empty();
                let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), if has_notes { 56.0 } else { 44.0 }), Sense::hover());
                let row_id = egui::Id::unique(("cue-row", tile.id));
                // The focused row is selected, so it's the next cue: Enter is GO (D-192),
                // which also lets a vamping cue play on. Taken before `interact`, which
                // would make it a click on the row.
                if hits.is_none() && ui.memory(|m| m.has_focus(row_id)) && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter)) {
                    actions.push(inv!("cue.go").into());
                }
                let resp = ui.interact(rect, row_id, Sense::click_and_drag());
                if resp.has_focus() {
                    focused = Some(tile.id);
                }
                rects.push((tile.id, i, rect));
                if self.reveal == Some(tile.id) {
                    ui.scroll_to_rect(rect, Some(Align::Center));
                }
                let played = self.ctl.played.contains(&tile.id);
                cue_mark_button(ui, rect, tile.id, i, played, actions);
                let set_name = hits.as_ref().and_then(|_| lib.locate(tile.id)).map(|(si, _)| lib.sets[si].name.as_str());
                let on = Some(self.ctl.is_selected(tile.id));
                let is_next_cue = hits.is_none() && Some(i) == queued;
                let resp = theme::role(resp, AxRole::Button, self.tile_label(tile, i, set_name, Some(is_next_cue), None), on);
                let resp = theme::hint(resp, self.tile_tip(tile, set_name));
                self.tile_interaction(ui, &resp, tile, i, true, None, Some(1), actions);
                let status = self.ctl.statuses.get(&tile.id);
                let is_next = hits.is_none() && Some(i) == queued;
                if is_panel {
                    let peaks = if self.ctl.store.library.show_waveforms { self.ctl.store.peaks(tile) } else { None };
                    let duration = self.ctl.store.audio(tile).map(|a| a.duration());
                    panel::paint_list_row(ui, rect, i, tile, status, &self.time_label(tile, status), is_next, TileFlags {
                        recording: self.rec_mark(ui.ctx(), tile.id),
                        selected: self.ctl.is_selected(tile.id),
                        drop_hover: !set.locked && resp.dnd_hover_payload::<Uuid>().is_some(),
                        hovered: resp.hovered(),
                        failed: matches!(self.ctl.store.media(tile), Some(Media::Failed(_))),
                        waveform: peaks.as_deref().map(|p| p.as_slice()).zip(duration),
                        picture: self.tile_texture(tile),
                        caption: self.rec_caption(tile.id).or_else(|| self.action_caption(tile)),
                        level: self.tile_meter(tile.id),
                        colored: false,
                        played,
                        focus: ui.memory(|m| m.has_focus(resp.id)).then_some(resp.id),
                    });
                    if hits.is_none() {
                        self.link_strip(ui, tile, next_cue(&set.tiles, i + 1).is_some(), true);
                    }
                    continue;
                }
                let p = ui.painter();
                let base = theme::console_tile(ui.ctx(), tile.color);
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
                let paused = status.is_some_and(|s| s.state == PlayState::Paused);
                let rec = self.rec_mark(ui.ctx(), tile.id);
                // The state as a symbol and a word (below), not only the row's colour.
                if let Some(rec) = rec {
                    paint_rec_dot(p, pos2(rect.left() + 18.0, cy), 5.0, &t, rec.pulse);
                } else if let Some((icon, c)) = match (status.is_some(), paused, is_next) {
                    (true, true, _) => Some(("⏸", t.solo)),
                    (true, false, _) => Some(("▶", t.counter_text)),
                    (false, _, true) => Some(("»", t.accent)),
                    _ => None,
                } {
                    p.text(pos2(rect.left() + 14.0, cy), Align2::LEFT_CENTER, icon, theme::bold(12.0), c);
                }
                theme::paint_tabular(p, pos2(rect.left() + 48.0, cy), Align2::RIGHT_CENTER, &(i + 1).to_string(), theme::semibold(14.0), t.text_dim);
                // The armed mark first; a cue list has no automatic keys, only hand-set ones (D-192).
                let mark = cue_mark(ui, status, played);
                paint_cue_mark(p, pos2(rect.left() + CUE_MARK_X, cy), 5.0, mark, if played && status.is_none() { t.text_dim } else { t.accent });
                let mut x = rect.left() + 88.0;
                if let Some(k) = tile.hotkey {
                    let cap = paint_key_cap(p, pos2(x, cy - 10.0), vec2(20.0, 20.0), k, theme::bold(11.0), t.key, Some(t.key_border), t.key_text());
                    x = cap.right() + 10.0;
                }
                if let Some(tex) = self.tile_texture(tile) {
                    let thumb = Rect::from_min_size(pos2(x, cy - 15.0), vec2(30.0, 30.0));
                    let [w, h] = tex.size();
                    let uv = cover_uv(w as f32 / h.max(1) as f32, 1.0);
                    p.add(egui::epaint::RectShape::filled(thumb, 4.0, Color32::WHITE).with_texture(tex.id(), uv));
                    x += 38.0;
                }
                // Right side: time, then badges, then a mini waveform between title and badges.
                let time = self.time_label(tile, status);
                let time_color = if rec.is_some() { t.error } else { t.text };
                let time_rect = theme::paint_tabular(p, pos2(rect.right() - 14.0, cy), Align2::RIGHT_CENTER, &time, theme::semibold(13.0), time_color);
                if let Some(b) = self.tile_meter(tile.id).filter(|_| status.is_some_and(|s| s.state != PlayState::Paused)) {
                    paint_tile_meter(p, meter_under(time_rect, 2.0), &b, &t);
                }
                if let Some(db) = rec.and_then(|r| r.level_db) {
                    paint_input_level(p, meter_under(time_rect, 2.0), db, &t);
                }
                let mut right = time_rect.left() - 16.0;
                if self.ctl.is_selected(tile.id) {
                    theme::paint_check_badge(p, pos2(right - 8.0, cy), t.accent, theme::ink_on(t.accent));
                    right -= 28.0;
                }
                let state_word = match (status.is_some(), paused, is_next) {
                    _ if rec.is_some() => Some("RECORDING"),
                    (true, false, _) if vamp_state_word(tile, status).is_some() => vamp_state_word(tile, status),
                    (true, true, _) => Some("PAUSED"),
                    (true, false, _) => Some("PLAYING"),
                    (false, _, true) => Some("NEXT"),
                    _ => None,
                };
                let badges = match (state_word, mode_badge_text(tile)) {
                    (Some(w), b) if b.is_empty() => w.to_owned(),
                    (Some(w), b) => format!("{w} · {b}"),
                    (None, b) => b,
                };
                if !badges.is_empty() {
                    let g = p.layout_no_wrap(badges, theme::bold(9.0), t.text_dim);
                    right -= g.size().x;
                    p.galley(pos2(right, cy - g.size().y / 2.0), g, t.text_dim);
                    right -= 16.0;
                }
                let title_w = (rect.width() * 0.32).min(right - x);
                let line = |text: &str, font: FontId, color: Color32, width: f32| {
                    ui.fonts_mut(|f| {
                        let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, color);
                        job.wrap.max_width = width.max(10.0);
                        job.wrap.max_rows = 1;
                        f.layout_job(job)
                    })
                };
                // A cue with notes puts them on a second line, under the title and waveform.
                let title_y = if has_notes { cy - 9.0 } else { cy };
                let title = line(&tile.title, theme::semibold(14.0), t.text, title_w);
                p.galley(pos2(x, title_y - title.size().y / 2.0), title, t.text);
                if has_notes {
                    let notes = line(tile.notes.trim(), theme::regular(12.0), t.text_dim, right - x);
                    p.galley(pos2(x, cy + 11.0 - notes.size().y / 2.0), notes, t.text_dim);
                }
                let wave_bottom = if has_notes { cy + 1.0 } else { rect.bottom() - 8.0 };
                let wave = Rect::from_min_max(pos2(x + title_w + 16.0, rect.top() + 8.0), pos2(right, wave_bottom));
                if let Some(c) = self.rec_caption(tile.id) {
                    let g = line(&c, theme::regular(12.0), t.error, wave.width());
                    p.galley(pos2(wave.left(), wave.center().y - g.size().y / 2.0), g, t.error);
                } else if let (true, Some(peaks), Some(audio)) = (self.ctl.store.library.show_waveforms, self.ctl.store.peaks(tile), self.ctl.store.audio(tile)) {
                    let played_x = status.map(|s| wave.left() + wave.width() * s.progress());
                    let rest = theme::with_alpha(theme::lighten(base, 0.35), if status.is_some() { 0.55 } else { 0.7 });
                    paint_waveform(p, wave, tile, &peaks, audio.duration(), played_x, theme::lighten(base, 0.75), rest);
                    if let Some(px) = played_x {
                        p.line_segment([pos2(px, wave.top()), pos2(px, wave.bottom())], Stroke::new(1.0_f32, t.playhead));
                    }
                } else if let Some(c) = self.action_caption(tile) {
                    let g = line(&c, theme::regular(12.0), t.text_dim, wave.width());
                    p.galley(pos2(wave.left(), wave.center().y - g.size().y / 2.0), g, t.text_dim);
                }
                let outline = if self.ctl.is_selected(tile.id) || (!set.locked && resp.dnd_hover_payload::<Uuid>().is_some()) {
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
                if tile.disabled {
                    let veil = theme::look(ui.ctx()).veil;
                    p.rect_filled(rect, 5.0, veil.fill);
                    theme::paint_hatch(p, rect.shrink(3.0), veil.stripes);
                }
                if let Some(s) = outline {
                    p.rect_stroke(rect, 5.0, s, StrokeKind::Inside);
                }
                if let Some(rec) = rec {
                    p.rect_stroke(rect, 5.0, Stroke::new(2.0_f32, theme::with_alpha(t.error, rec.pulse)), StrokeKind::Outside);
                }
                if hits.is_none() {
                    self.link_strip(ui, tile, next_cue(&set.tiles, i + 1).is_some(), false);
                }
            }
            if hits.is_some() || set.locked {
                return;
            }
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::click());
            if is_panel {
                panel::paint_add_slot(ui, rect, resp.hovered());
            } else {
                paint_add_slot(ui, rect, resp.hovered());
            }
            let resp = theme::role(resp, AxRole::Button, "Add sounds", None);
            let resp = theme::hint_keys(resp, tips::ADD_SLOT.0, tips::ADD_SLOT.1);
            if resp.clicked() {
                actions.push(inv!("tile.add").into());
            }
            if let Some(src) = resp.dnd_release_payload::<Uuid>() {
                actions.push(self.drag_move(ui.ctx(), *src, json!({"to": u32::MAX})));
            }
        });
        if hits.is_none() {
            ui.add_space(6.0);
            let locked = self.ctl.store.library.sets[si].locked;
            if is_panel {
                panel::list_bar(ui, narrow, (&mut auto_advance, &mut crossfade), &mut reset, locked);
            } else {
                list_bar(ui, narrow, (&mut auto_advance, &mut crossfade), &mut reset, locked);
            }
        }
        self.tile_rects = rects;
        self.focus_selects(ui, focused, actions);
        // The selection moved on by itself (GO, Next, Continuous): a focused row hands the
        // focus to the newly selected one, so Enter and the arrows go on from there.
        if hits.is_none() && focused.is_some() && focused == self.focused_tile {
            let sel = self.ctl.selected_tile.filter(|s| Some(*s) != focused && self.ctl.selected().len() == 1);
            if let Some(sel) = sel.filter(|s| self.ctl.store.library.sets[si].tiles.iter().any(|t| t.id == *s)) {
                ui.memory_mut(|m| m.request_focus(egui::Id::unique(("cue-row", sel))));
            }
        }
        self.focused_tile = focused;
        if auto_advance != self.ctl.store.library.sets[si].auto_advance {
            actions.push(inv!("set.listMode", {"mode": if auto_advance { "continuous" } else { "queued" }}).into());
        }
        if crossfade != self.ctl.store.library.sets[si].crossfade {
            actions.push(inv!("set.crossfade", {"seconds": crossfade}).into());
        }
        if reset {
            actions.push(inv!("cue.reset").into());
        }
    }

    /// Under a linked cue: a line saying what starts the next cue and, while it's
    /// waiting, how soon.
    fn link_strip(&self, ui: &mut Ui, tile: &Tile, has_next: bool, panel: bool) {
        if !has_next {
            return;
        }
        let counting = self.ctl.linking(tile.id);
        let Some(text) = link_text(tile, counting) else { return };
        let t = Tokens::get(ui.ctx());
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 18.0), Sense::hover());
        let (text, font, color) = if panel {
            (text.to_uppercase(), theme::condensed_semi(14.0), { let pal = theme::palette(ui.ctx()); if counting.is_some() { pal.amber } else { pal.lilac } })
        } else {
            (text, theme::semibold(11.0), if counting.is_some() { t.accent_text() } else { t.text_dim })
        };
        // A hook from this row down toward the next, then the words.
        let x = rect.left() + if panel { 90.0 } else { 58.0 };
        let p = ui.painter();
        let stroke = Stroke::new(1.5_f32, color);
        p.line_segment([pos2(x - 14.0, rect.top() - 2.0), pos2(x - 14.0, rect.center().y)], stroke);
        p.line_segment([pos2(x - 14.0, rect.center().y), pos2(x - 5.0, rect.center().y)], stroke);
        p.text(pos2(x, rect.center().y), Align2::LEFT_CENTER, text, font, color);
    }

    /// The set's notes above the cue list (they're changed in the set's settings).
    fn set_notes(&self, ui: &mut Ui, si: usize) {
        let set = &self.ctl.store.library.sets[si];
        if !set.notes.trim().is_empty() {
            let t = Tokens::get(ui.ctx());
            ui.add_space(8.0);
            ui.add(egui::Label::new(RichText::new(set.notes.trim()).color(t.text_dim)).wrap());
        }
    }

    // ── inspector ───────────────────────────────────────────────────────────

    /// The vamp switch, the markers and Play On: the inspector's (`inspector`, with the
    /// vamp's times and the marker list) and the editor's (which has those in its own rows).
    /// `locked`: the tile's set is locked, so only Jump and Play On (which play) respond.
    fn cue_points(&self, ui: &mut Ui, t: &mut Tile, dur: f64, inspector: bool, locked: bool, actions: &mut Vec<Action>) {
        let id = t.id;
        let (s, e) = play_range(t, dur);
        // The vamp: a section that loops until it's let go (Play On). Its rows wrap rather
        // than widen a narrow inspector (D-211).
        let mut on = t.vamp.is_some();
        ui.horizontal_wrapped(|ui| {
            let vamp = ui.add_enabled_ui(!locked, |ui| theme::checkbox(ui, &mut on, "Vamp")).inner;
            if theme::hint_or(vamp, "Loop a section until the next press (or GO) lets it go, then play on to the end", tips::LOCKED_WHY).changed() {
                // Start with the middle third of what plays.
                t.vamp = on.then(|| (s + (e - s) / 3.0, s + 2.0 * (e - s) / 3.0));
            }
            if let (true, Some((a, b))) = (inspector, t.vamp) {
                let (mut a2, mut b2) = (a, b);
                ui.add_enabled_ui(!locked, |ui| {
                    theme::hint(theme::name(ui.add(egui::DragValue::new(&mut a2).range(s..=e).speed(0.01).max_decimals(2).suffix(" s")), "Vamp start"), "Where the vamp section starts, in seconds");
                    ui.label("to");
                    theme::hint(theme::name(ui.add(egui::DragValue::new(&mut b2).range(s..=e).speed(0.01).max_decimals(2).suffix(" s")), "Vamp end"), "Where the vamp section ends and loops back, in seconds");
                });
                if (a2, b2) != (a, b) {
                    let a2 = a2.min(b2 - 0.05).max(s);
                    t.vamp = Some((a2, b2.max(a2 + 0.05).min(e)));
                }
            }
        });
        // Markers: jump buttons, and one more at the playhead.
        let playing = self.ctl.statuses.contains_key(&id);
        for (i, m) in t.markers.iter().enumerate().filter(|_| inspector) {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(format!("{} {}", m.name, format_clock(m.at, true))).color(Tokens::get(ui.ctx()).marker));
                if theme::hint(theme::small_button(ui, "Jump"), format!("Play from {} (or jump there while it plays)", m.name)).clicked() {
                    actions.push(inv!("tile.jump", {"tile": id, "marker": format!("#{}", i + 1)}).into());
                }
                let remove = ui.add_enabled(!locked, theme::small(egui::Button::new("🗙")));
                if theme::hint_or(theme::name(remove, &format!("Remove {}", m.name)), format!("Remove the marker {}", m.name), tips::LOCKED_WHY).clicked() {
                    actions.push(inv!("tile.markerRemove", {"tile": id, "marker": format!("#{}", i + 1)}).into());
                }
            });
        }
        let marker = theme::name(ui.add_enabled(playing && !locked, theme::small(egui::Button::new("+ Marker at playhead"))), "Add marker at playhead");
        if theme::hint_or(marker, "Add a marker where the tile is playing now", if locked { tips::LOCKED_WHY } else { "Play the tile first" }).clicked() {
            actions.push(inv!("tile.markerAdd", {"tile": id}).into());
        }
        if t.vamp.is_some() {
            let status = self.ctl.statuses.get(&id);
            let vamping = status.is_some_and(|st| st.vamping);
            ui.horizontal_wrapped(|ui| {
                let play_on = ui.add_enabled(vamping, theme::small(egui::Button::new("Play On")));
                if theme::hint_or(play_on, "Let the vamp go: play on to the end (the tile's next press does the same)", "Only while the tile is looping its vamp").clicked() {
                    actions.push(inv!("tile.playOn", {"tile": id}).into());
                }
                // What the vamp is doing now (D-157).
                let state = match vamp_state_word(t, status) {
                    Some("VAMPING") => "Vamping: the next press plays on",
                    Some(_) => "Playing on to the end",
                    None => "The next press while it vamps plays on",
                };
                ui.weak(state);
            });
        }
    }

    /// An action tile's section in the inspector: what it does, how long a fade takes,
    /// and its targets (tiles, or every tile in a set).
    fn action_editor(&self, ui: &mut Ui, t: &Tile, actions: &mut Vec<Action>) {
        let Some(a) = &t.action else { return };
        let id = t.id;
        ui.label(RichText::new("ACTION").small().strong().weak());
        let with = |list: &[String], r: String| -> Vec<String> { list.iter().cloned().chain([r]).collect() };
        let refs = |a: &crate::model::TileAction| -> Vec<String> {
            a.targets
                .iter()
                .filter_map(|tg| match (tg.tile, tg.set) {
                    (Some(tile), _) => Some(format!("{tile}@{}", tg.offset)),
                    (None, Some(set)) => Some(format!("set:{set}@{}", tg.offset)),
                    _ => None,
                })
                .collect()
        };
        egui::Grid::new("action-grid").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
            ui.label("Does");
            let mut kind = a.kind;
            let combo = egui::ComboBox::from_id_salt("action-kind").width(ui.available_width().min(200.0)).selected_text(kind.label()).show_ui(ui, |ui| {
                for k in ActionKind::ALL {
                    ui.selectable_value(&mut kind, k, k.label());
                }
            });
            theme::hint(theme::name(combo.response, &format!("Does, {}", kind.label())), "What this tile does to its targets when it's triggered");
            if kind != a.kind {
                actions.push(inv!("tile.action", {"tile": id, "kind": kind}).into());
            }
            ui.end_row();
            if a.kind == ActionKind::Fade {
                ui.label("Over");
                inspector::fit_sliders(ui, &["30.0 s"], 0.0);
                let mut fade = a.fade;
                if theme::hint(theme::name(ui.add(egui::Slider::new(&mut fade, 0.0..=30.0).step_by(0.1).suffix(" s")), "Fade over"), "Seconds the targets take to fade out").changed() {
                    actions.push(inv!("tile.action", {"tile": id, "fade": fade}).into());
                }
                ui.end_row();
            }
            ui.label("Targets");
            ui.vertical(|ui| {
                let current = refs(a);
                let names = self.ctl.action_json(a)["targets"].clone();
                for (i, n) in names.as_array().into_iter().flatten().enumerate() {
                    ui.horizontal(|ui| {
                        let label = match (n["title"].as_str(), n["name"].as_str()) {
                            (Some(title), _) => title.to_owned(),
                            (None, Some(set)) => format!("Every tile in {set}"),
                            _ => "A removed tile".to_owned(),
                        };
                        // A long title is cut short, leaving room for its offset (at most two
                        // decimals) and ✕ (D-211).
                        let gap = ui.spacing().item_spacing.x;
                        let offset = a.targets.get(i).filter(|_| a.kind == ActionKind::Play).map_or(0.0, |t| inspector::box_width(ui, &format!("+{:.2} s", t.offset)) + gap);
                        let remove = inspector::box_width(ui, "🗙").max(theme::TARGET);
                        let room = (ui.available_width() - offset - remove - gap).max(40.0);
                        ui.scope(|ui| {
                            ui.set_max_width(room);
                            ui.add(egui::Label::new(&label).truncate());
                        });
                        if a.kind == ActionKind::Play {
                            let mut off = a.targets.get(i).map_or(0.0, |t| t.offset);
                            let resp = ui.add(egui::DragValue::new(&mut off).range(0.0..=3600.0).speed(0.05).max_decimals(2).prefix("+").suffix(" s"));
                            if theme::hint(resp, format!("When {label} starts, in seconds after the group is triggered")).changed() {
                                let mut moved = current.clone();
                                if let Some(r) = moved.get_mut(i) {
                                    let base = r.rsplit_once('@').map_or(r.as_str(), |(b, _)| b).to_owned();
                                    *r = format!("{base}@{off}");
                                }
                                actions.push(inv!("tile.action", {"tile": id, "targets": moved}).into());
                            }
                        }
                        if theme::hint(theme::name(theme::small_button(ui, "🗙"), &format!("Remove {label}")), format!("Take {label} off the targets")).clicked() {
                            let mut rest = current.clone();
                            rest.remove(i);
                            actions.push(inv!("tile.action", {"tile": id, "targets": rest}).into());
                        }
                    });
                }
                let lib = &self.ctl.store.library;
                // The targets without their offsets, to leave out of the list.
                let bases: Vec<String> = current.iter().map(|r| r.rsplit_once('@').map_or(r.as_str(), |(b, _)| b).to_owned()).collect();
                let combo = egui::ComboBox::from_id_salt("action-add").width(ui.available_width().min(200.0)).selected_text("Add a target…").show_ui(ui, |ui| {
                    for set in &lib.sets {
                        let r = format!("set:{}", set.id);
                        if !bases.contains(&r) && ui.selectable_label(false, format!("Every tile in {}", set.name)).clicked() {
                            actions.push(inv!("tile.action", {"tile": id, "targets": with(&current, r)}).into());
                        }
                        for tile in set.tiles.iter().filter(|x| x.id != id && x.file.is_some()) {
                            let r = tile.id.to_string();
                            if !bases.contains(&r) && ui.selectable_label(false, format!("    {}", tile.title)).clicked() {
                                actions.push(inv!("tile.action", {"tile": id, "targets": with(&current, r)}).into());
                            }
                        }
                    }
                });
                theme::hint(theme::name(combo.response, "Add a target"), "Add a tile, or every tile in a set, for this tile to act on");
            });
            ui.end_row();
        });
        let hint = if a.kind == ActionKind::Play {
            "Triggering this tile (a click, its key, a pad, or GO in the cue list) plays each target after its offset."
        } else {
            "Triggering this tile (a click, its key, a pad, or GO in the cue list) acts on the targets that are playing."
        };
        ui.label(RichText::new(hint).small().weak());
    }

    fn picture_editor(&self, ui: &mut Ui, t: &mut Tile, actions: &mut Vec<Action>) {
        let id = t.id;
        if t.image.is_none() {
            ui.weak("No picture. Choose one, or drop an image file onto the tile.");
            if theme::hint(ui.button("Choose Picture…"), "Choose a picture to show on the tile, or drop an image file onto it").clicked() {
                actions.push(inv!("tile.picture", {"tile": id}).into());
            }
            return;
        }
        ui.horizontal(|ui| {
            // A small preview at the tile's own aspect, so Fill vs Fit is visible.
            let (rect, _) = ui.allocate_exact_size(vec2(96.0, 64.0), Sense::hover());
            let p = ui.painter_at(rect);
            p.rect_filled(rect, 6.0, theme::console_tile(ui.ctx(), t.color));
            match (self.tile_texture(t), self.ctl.store.picture_state(t)) {
                (Some(tex), _) => paint_picture(&p, rect, 6.0, t, tex),
                (None, Some(PictureState::Failed(e))) => {
                    p.text(rect.center(), Align2::CENTER_CENTER, "⚠", FontId::proportional(18.0), Tokens::get(ui.ctx()).error);
                    ui.colored_label(Tokens::get(ui.ctx()).error, format!("Can't show this picture: {e}"));
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
                    if theme::hint(ui.button("Choose…"), "Choose another picture for the tile").clicked() {
                        actions.push(inv!("tile.picture", {"tile": id}).into());
                    }
                    if theme::hint(ui.button("Remove"), "Take the picture off this tile").clicked() {
                        actions.push(inv!("tile.pictureClear", {"tile": id}).into());
                    }
                });
            });
        });
        egui::Grid::new("picture-grid").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            ui.label("Layer");
            ui.horizontal(|ui| {
                theme::hint(ui.selectable_value(&mut t.image_layer, ImageLayer::Underlay, "Under"), "Show the picture behind the waveform and progress");
                theme::hint(ui.selectable_value(&mut t.image_layer, ImageLayer::Overlay, "Over"), "Show the picture over the waveform and progress (the title stays on top)");
            });
            ui.end_row();
            ui.label("Fit");
            ui.horizontal(|ui| {
                theme::hint(ui.selectable_value(&mut t.image_fit, ImageFit::Fill, "Fill"), "Cover the whole tile, cropping the picture");
                theme::hint(ui.selectable_value(&mut t.image_fit, ImageFit::Fit, "Fit"), "Show the whole picture; good for icons");
            });
            ui.end_row();
            ui.label("Opacity");
            theme::hint(ui.add(egui::Slider::new(&mut t.image_opacity, 0.0..=1.0).custom_formatter(|v, _| format!("{:.0}%", v * 100.0))), "How solid the picture is");
            ui.end_row();
        });
    }

    fn theme_panels(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        if theme::is_panel(ui.ctx()) {
            self.panel_panels(ui, actions);
        } else {
            self.console_panels(ui, actions);
        }
    }

    fn console_panels(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let t = Tokens::get(ui.ctx());
        let panel = |fill: Color32, margin: egui::Margin| egui::Frame::new().fill(fill).inner_margin(margin);
        // Each panel's frame draws its border. egui's separator line would add a second,
        // since egui 0.36 gives it room of its own instead of letting the next panel cover it.
        egui::Panel::top("toolbar")
            .frame(panel(t.panel, egui::Margin { left: 14, right: 14, top: 8, bottom: 8 }).stroke(Stroke::new(1.0_f32, t.border)))
            .show_separator_line(false)
            .show(ui, |ui| {
                theme::name_panel(ui, egui::accesskit::Role::Toolbar, "Toolbar");
                if !self.opts.in_window_menu {
                    crate::titlebar::drag_area(ui, ui.max_rect().expand2(vec2(14.0, 8.0)));
                }
                self.toolbar(ui, actions)
            });
        egui::Panel::bottom("status")
            .frame(panel(t.bar, egui::Margin { left: 12, right: 12, top: 2, bottom: 2 }).stroke(Stroke::new(1.0_f32, t.border)))
            .show_separator_line(false)
            .show(ui, |ui| {
                theme::name_panel(ui, egui::accesskit::Role::Region, "Status bar");
                self.status_bar(ui)
            });
        egui::Panel::bottom("bottom-bar")
            .frame(panel(t.panel, egui::Margin { left: 10, right: 10, top: 3, bottom: 3 }).stroke(Stroke::new(1.0_f32, t.border)))
            .show_separator_line(false)
            .show(ui, |ui| {
                theme::name_panel(ui, egui::accesskit::Role::Toolbar, "Board bar");
                self.bottom_bar(ui, actions)
            });
        if self.ctl.store.library.show_sidebar {
            egui::Panel::left("sets")
                .frame(panel(t.panel2, egui::Margin { left: 10, right: 10, top: 0, bottom: 0 }).stroke(Stroke::new(1.0_f32, t.border)))
                .show_separator_line(false)
                .resizable(true)
                .default_size(208.0)
                .size_range(150.0..=320.0)
                .show(ui, |ui| {
                    theme::name_panel(ui, egui::accesskit::Role::Region, "Setlist");
                    self.sets_panel(ui, actions)
                });
        }
        self.inspector_width = 0.0;
        if self.ctl.store.library.show_inspector {
            let shown = egui::Panel::right("inspector")
                .frame(panel(t.panel2, egui::Margin { left: 12, right: 12, top: 0, bottom: 0 }).stroke(Stroke::new(1.0_f32, t.border)))
                .show_separator_line(false)
                .resizable(true)
                .default_size(INSPECTOR_WIDTH)
                .size_range(INSPECTOR_MIN_WIDTH..=INSPECTOR_MAX_WIDTH)
                .show(ui, |ui| {
                    theme::name_panel(ui, egui::accesskit::Role::Region, "Inspector");
                    self.inspector(ui, actions)
                });
            self.inspector_width = shown.response.rect.width();
        }
        self.board_rect = ui.available_rect_before_wrap();
        egui::CentralPanel::default().frame(panel(t.window, egui::Margin::same(16))).show(ui, |ui| {
            theme::name_panel(ui, egui::accesskit::Role::Region, "Board");
            self.board(ui, actions)
        });
    }

    /// The Panel layout: a fixed frame, with the panels sized to fit inside it.
    fn panel_panels(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let area = ui.available_rect_before_wrap();
        panel::paint_frame(ui.ctx(), area, ui.clip_rect(), crate::brand::PANEL_SUBTITLE, self.ctl.store.library.show_sidebar);
        let none = egui::Frame::NONE;
        egui::Panel::top("toolbar")
            .frame(none)
            .exact_size(panel::TOP_H)
            .show_separator_line(false)
            .show(ui, |ui| {
                theme::name_panel(ui, egui::accesskit::Role::Toolbar, "Toolbar");
                // The traffic lights show in a black pill in the elbow's corner while the
                // pointer is there (D-232).
                if !self.opts.in_window_menu {
                    let pill = panel::lights_pill_at(area, crate::titlebar::pill_size(ui.ctx()));
                    crate::titlebar::drag_area_pill_at(ui, ui.max_rect(), pill);
                }
                self.panel_toolbar(ui, area, actions)
            });
        egui::Panel::bottom("status")
            .frame(none)
            .exact_size(panel::BOTTOM_H)
            .show_separator_line(false)
            .show(ui, |ui| {
                theme::name_panel(ui, egui::accesskit::Role::Region, "Status bar");
                self.panel_status(ui, area, actions)
            });
        let sidebar = self.ctl.store.library.show_sidebar;
        egui::Panel::left("sets")
            .frame(none)
            .exact_size(panel::left_w(sidebar))
            .resizable(false)
            .show_separator_line(false)
            .show(ui, |ui| {
                theme::name_panel(ui, egui::accesskit::Role::Region, "Setlist");
                if sidebar {
                    self.panel_sets(ui, area, actions)
                }
            });
        self.inspector_width = 0.0;
        if self.ctl.store.library.show_inspector {
            let shown = egui::Panel::right("inspector")
                .frame(none.inner_margin(egui::Margin { left: 14, right: 12, top: 4, bottom: 0 }).stroke(Stroke::new(2.0_f32, theme::palette(ui.ctx()).lilac)))
                .show_separator_line(false)
                .resizable(true)
                .default_size(INSPECTOR_WIDTH)
                .size_range(INSPECTOR_MIN_WIDTH..=INSPECTOR_MAX_WIDTH)
                .show(ui, |ui| {
                    theme::name_panel(ui, egui::accesskit::Role::Region, "Inspector");
                    self.inspector(ui, actions)
                });
            self.inspector_width = shown.response.rect.width();
        }
        self.board_rect = ui.available_rect_before_wrap();
        egui::CentralPanel::default()
            .frame(none.inner_margin(egui::Margin { left: 8, right: 16, top: 6, bottom: 8 }))
            .show(ui, |ui| {
                theme::name_panel(ui, egui::accesskit::Role::Region, "Board");
                self.board(ui, actions)
            });
    }

    fn drop_overlay(&self, ctx: &egui::Context) {
        let hovering = ctx.input(|i| !i.raw.hovered_files.is_empty());
        if !hovering {
            return;
        }
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("drop-overlay")));
        let screen = ctx.content_rect();
        if self.file_marker {
            // Over a Rows grid the board shows where they land: keep it in view, framed
            // in the theme's accent.
            painter.rect_stroke(screen.shrink(12.0), 12.0, Stroke::new(3.0_f32, Tokens::get(ctx).accent), StrokeKind::Inside);
            let caption = "Drop to add sounds where the board shows (a picture: onto a tile)";
            let g = painter.layout_no_wrap(caption.into(), FontId::proportional(15.0), Color32::WHITE);
            let r = Rect::from_center_size(pos2(screen.center().x, screen.bottom() - 40.0), g.size() + vec2(24.0, 12.0));
            painter.rect_filled(r, 8.0, Color32::from_black_alpha(200));
            painter.galley(r.center() - g.size() / 2.0, g, Color32::WHITE);
            return;
        }
        painter.rect_filled(screen, 0.0, Color32::from_black_alpha(140));
        painter.rect_stroke(screen.shrink(12.0), 12.0, Stroke::new(3.0_f32, Tokens::get(ctx).drop), StrokeKind::Inside);
        painter.text(
            screen.center(),
            Align2::CENTER_CENTER,
            "Drop audio to add tiles\n(after the tile you drop them on)",
            FontId::proportional(22.0),
            Color32::WHITE,
        );
    }

    /// Ask egui for this frame's pixels, for the screenshots asked for so far.
    fn capture(&mut self, ctx: &egui::Context) {
        if self.shots.is_empty() {
            return;
        }
        let (tx, wake, batch) = (self.shot_tx.clone(), ctx.clone(), std::mem::take(&mut self.shots));
        self.shots_pending += 1;
        ctx.request_screenshot(move |img| {
            tx.send((batch, img)).ok();
            wake.request_repaint();
        });
    }

    /// Hand the set windows their settled `view.screenshot {window}` shots: each window
    /// captures itself on its pass (D-226).
    fn capture_windows(&mut self) {
        for (set, path) in std::mem::take(&mut self.window_shots) {
            match self.set_wins.get_mut(&set) {
                Some(w) => w.shots.push((path, false)),
                None => self.ctl.notify_error("Couldn't save the screenshot: that set's window has closed"),
            }
        }
    }

    /// `LEADSTINGER_SNAPSHOT`: after `snapshot_frames` frames, save a screenshot (and one
    /// of each set window, beside it) and quit. `view.screenshot` saves one without
    /// quitting, once the ui has settled.
    fn snapshot(&mut self, ctx: &egui::Context) {
        if let Some(path) = self.opts.snapshot.clone() {
            ctx.request_repaint();
            if self.frame >= self.opts.snapshot_frames && !self.snapshot_requested {
                self.snapshot_requested = true;
                self.shots.push((path.clone(), true));
                self.quit_after = 1;
                for (n, w) in self.ctl.store.library.set_windows.iter().enumerate() {
                    if let Some(win) = self.set_wins.get_mut(&w.set) {
                        win.shots.push((window_shot_path(&path, n + 1), true));
                        self.quit_after += 1;
                    }
                }
                self.capture(ctx);
            }
        }
        if let Some((frames, since)) = &mut self.shot_settle {
            *frames += 1;
            if *frames >= SHOT_SETTLE.0 && since.elapsed() >= SHOT_SETTLE.1 {
                self.shot_settle = None;
                self.capture(ctx);
                self.capture_windows();
            }
        }
    }

    /// Save the captured frames that have arrived on `shot_rx`. Runs in `logic`, so a
    /// hidden window saves its capture without waiting for another ui pass.
    fn save_shots(&mut self, ctx: &egui::Context) {
        while let Ok((batch, img)) = self.shot_rx.try_recv() {
            self.shots_pending = self.shots_pending.saturating_sub(1);
            let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
            let mut quit = false;
            for (path, then_quit) in batch {
                match image::save_buffer(&path, &bytes, img.size[0] as u32, img.size[1] as u32, image::ColorType::Rgba8) {
                    Ok(()) => eprintln!("{}: saved screenshot to {}", crate::brand::SLUG, path.display()),
                    Err(e) => self.ctl.notify_error(format!("Couldn't save the screenshot: {e}")),
                }
                quit |= then_quit;
            }
            if quit {
                self.quit_after = self.quit_after.saturating_sub(1);
                if self.quit_after == 0 {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
        if self.shots_pending > 0 {
            ctx.request_repaint();
        }
    }
}

/// Where a `LEADSTINGER_SNAPSHOT` run saves the `n`th set window (from 1): beside the
/// main shot, `shot.png` → `shot-window-1.png`.
fn window_shot_path(main: &std::path::Path, n: usize) -> PathBuf {
    let stem = main.file_stem().and_then(|s| s.to_str()).unwrap_or("snapshot");
    let ext = main.extension().and_then(|s| s.to_str()).unwrap_or("png");
    main.with_file_name(format!("{stem}-window-{n}.{ext}"))
}

impl eframe::App for StingerApp {
    // eframe skips `ui` while the window is hidden (minimised or covered) but still runs
    // `logic`, so everything a show needs without the window lives there: cue ticks,
    // key registration, effects, saving and the repaints that keep them going.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.ctl.0 = Some(self.lender.borrow());
        self.frame_logic(ctx);
        self.lender.give_back(self.ctl.0.take().expect("borrowed for this frame"));
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.ctl.0 = Some(self.lender.borrow());
        // iPadOS: stay clear of the status bar and the home indicator (the safe area).
        #[cfg(target_os = "ios")]
        {
            let safe = ui.ctx().content_rect();
            ui.scope_builder(egui::UiBuilder::new().max_rect(safe), |ui| self.frame_ui(ui));
        }
        #[cfg(not(target_os = "ios"))]
        self.frame_ui(ui);
        self.lender.give_back(self.ctl.0.take().expect("borrowed for this frame"));
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        // The drag:/files: script steps: press on the tile, glide to the point, stay down.
        if let Some((files, k, to, step)) = &mut self.script_drag {
            if self.script_drag_alt {
                raw_input.events.push(egui::Event::ModifiersChanged(Modifiers::ALT));
            }
            if *files {
                raw_input.hovered_files = (0..*k).map(|i| egui::HoveredFile { path: Some(PathBuf::from(format!("Sound {}.wav", i + 1))), ..Default::default() }).collect();
                raw_input.events.push(egui::Event::PointerMoved(*to));
            } else if let Some(from) = self.script_drag_from.or_else(|| self.tile_rects.iter().find(|(_, i, _)| i == k).map(|r| r.2.center())) {
                self.script_drag_from = Some(from);
                if *step == 0 {
                    raw_input.events.push(egui::Event::PointerMoved(from));
                    raw_input.events.push(egui::Event::PointerButton { pos: from, button: egui::PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
                } else {
                    raw_input.events.push(egui::Event::PointerMoved(from + (*to - from) * (*step as f32 / 8.0).min(1.0)));
                }
            }
            *step += 1;
            ctx.request_repaint();
        }
        if let Some((points, at)) = self.script_scroll.take() {
            raw_input.events.push(egui::Event::PointerMoved(at));
            raw_input.events.push(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: vec2(0.0, -points),
                phase: egui::TouchPhase::Move,
                source: egui::MouseWheelSource::Unknown,
                modifiers: Modifiers::NONE,
            });
        }
        if let Some(drops) = &mut self.wayland_drops {
            drops.apply(ctx.zoom_factor(), raw_input);
        }
        if let Some(drops) = &self.mac_drops {
            drops.apply(ctx, raw_input);
        }
    }
}

impl StingerApp {
    fn frame_logic(&mut self, ctx: &egui::Context) {
        self.ctl.tick_now();
        // leadstinger:// links and set files the system opened us with (src/quip.rs).
        for opened in crate::quip::take_links() {
            let (id, params) = crate::quip::opened_command(&opened);
            if let Err(e) = self.ctl.execute(id, params) {
                self.ctl.report(&e);
            }
        }
        if self.hotkeys_synced != self.ctl.store.revision {
            self.hotkeys_synced = self.ctl.store.revision;
            if let Some(h) = &mut self.hotkeys {
                self.ctl.hotkey_status = h.sync(&self.ctl.store.library.bindings);
            }
            #[cfg(target_os = "linux")]
            if let Some(p) = &mut self.portal {
                p.sync(&self.ctl.store.library.bindings);
            }
        }
        self.handle_effects(ctx);
        self.save_and_schedule(ctx);
        self.save_shots(ctx);
        // A hidden window runs no ui, so a screenshot asked for from the CLI has nothing
        // fresh to capture: have the ui run (and paint) until `snapshot` takes it.
        if self.shot_settle.is_some() {
            ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaintWhileHidden);
            ctx.request_repaint();
        } else if self.opts.background {
            // A background window is meant to be covered: every frame it runs, it paints.
            ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaintWhileHidden);
        }
    }

    fn frame_ui(&mut self, ui: &mut Ui) {
        let ctx = &ui.ctx().clone();
        #[cfg(test)]
        theme::hint_log_clear(ctx);
        self.frame += 1;
        // The library's zoom, however it was changed (menu, keys, Settings, the CLI).
        let zoom = self.ctl.store.library.zoom as f32 / 100.0;
        if (ctx.zoom_factor() - zoom).abs() > 1e-3 {
            ctx.set_zoom_factor(zoom);
        }
        let now = Instant::now();
        let dt = (now - self.last_frame).as_secs_f64();
        self.last_frame = now;
        self.meter.feed(self.ctl.engine.take_peaks(), dt as f32);
        self.prepare_textures(ctx);
        self.run_script(ctx);
        if let Some(id) = self.focus_script.take() {
            let row = if self.ctl.current_set().layout == Layout::List { "cue-row" } else { "tile" };
            ctx.memory_mut(|m| m.request_focus(egui::Id::unique((row, id))));
        }
        self.handle_effects(ctx);
        self.live_region(ui);

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
        self.handle_move_keys(ctx, &mut actions);
        self.handle_shortcuts(ctx, &mut actions);
        self.handle_hotkeys(ctx, &mut actions);
        self.handle_file_drop(ctx);
        self.handle_paste(ctx);
        self.handle_undo_keys(ctx);

        let t = Tokens::get(ctx);
        let panel = |fill: Color32, margin: egui::Margin| egui::Frame::new().fill(fill).inner_margin(margin);
        if !cfg!(target_os = "macos") || self.opts.in_window_menu {
            // Linux and Windows: the menu bar lives at the top of the window.
            let menus = menu::menus(&menu_state, false);
            egui::Panel::top("menubar")
                .frame(panel(t.bar, egui::Margin { left: 8, right: 8, top: 2, bottom: 2 }))
                .show_separator_line(false)
                .show(ui, |ui| {
                    theme::name_panel(ui, egui::accesskit::Role::Region, "Menu bar");
                    // On macOS (with the in-window menus) it's the window's top strip.
                    crate::titlebar::drag_area(ui, ui.max_rect().expand2(vec2(8.0, 2.0)));
                    ui.horizontal(|ui| {
                        ui.add_space((crate::titlebar::inset(ui.ctx()).x - 8.0).max(0.0));
                        if let Some(c) = menu::menu_bar(ui, &menus) {
                            menu_commands.push(c);
                        }
                    });
                });
        }
        // Zoomed in, a small window holds fewer points than the layouts are made for (the
        // smallest window at 100 %): the layout keeps that size and scrolls instead.
        let avail = ui.available_rect_before_wrap();
        let floor = vec2(LAYOUT_MIN[0], LAYOUT_MIN[1] - (ctx.content_rect().height() - avail.height()));
        let size = avail.size().max(floor);
        if size == avail.size() {
            self.theme_panels(ui, &mut actions);
        } else {
            egui::ScrollArea::both().id_salt("zoomed-layout").auto_shrink(false).show(ui, |ui| {
                let rect = Rect::from_min_size(ui.cursor().min, size);
                ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                    ui.set_min_size(size);
                    self.theme_panels(ui, &mut actions);
                });
            });
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
        self.export_window(ctx);
        self.quip_window(ctx, &mut actions);
        self.quip_drop(ctx, &mut actions);
        self.confirm_set_window(ctx);
        self.confirm_theme_window(ctx);
        self.update_window(ctx);
        self.quip_prompt_window(ctx);
        actions.extend(menu_commands.into_iter().map(|c: Cmd| Action::Run(c.into())));
        self.drop_overlay(ctx);

        // Drag ghost while reordering (a Rows grid draws the dragged pad itself).
        if let (Some(id), Some(pos)) = (egui::DragAndDrop::payload::<Uuid>(ctx), ctx.pointer_hover_pos().filter(|_| self.drag_grab.is_none() || self.row_grid.is_none())) {
            if let Some(tile) = self.ctl.store.library.tile(*id) {
                let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("drag-ghost")));
                let r = Rect::from_min_size(pos + vec2(8.0, 8.0), vec2(150.0, 32.0));
                painter.rect_filled(r, 6.0, theme::console_tile(ctx, tile.color).gamma_multiply(0.9));
                painter.text(r.left_center() + vec2(8.0, 0.0), Align2::LEFT_CENTER, &tile.title, FontId::proportional(14.0), Color32::WHITE);
                if drag_copies(ctx) {
                    paint_copy_badge(&painter, r.right_center() - vec2(14.0, 0.0), Tokens::get(ctx).meter_green);
                }
            }
        }

        // A tile pressed with the mouse is released when the button comes up anywhere.
        if let Some(id) = self.mouse_held {
            if !ctx.input(|i| i.pointer.primary_down()) {
                self.mouse_held = None;
                actions.push(inv!("tile.trigger", {"tile": id, "event": "release"}).into());
            }
        }

        #[cfg(target_os = "ios")]
        self.more_fingers(ctx, &mut actions);
        for a in actions {
            self.apply(a);
        }
        theme::focus_ring(ctx);
        self.handle_effects(ctx);
        self.snapshot(ctx);
        // The sets open in windows of their own (D-222), each its own pass.
        self.set_windows(ctx);
        self.handle_effects(ctx);
        self.save_and_schedule(ctx);
    }

    /// Save when due (or now, when the window is closing), and ask for the next frame
    /// while anything is moving. Runs after `logic` and after `ui`.
    fn save_and_schedule(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested()) {
            // eframe may end the process without dropping the store, which would leave
            // ffmpeg running.
            self.ctl.store.stop_imports();
            self.ctl.stop_renders();
            if let Err(e) = self.ctl.store.save_now() {
                eprintln!("{}: failed to save library: {e:#}", crate::brand::SLUG);
            }
        } else if let Err(e) = self.ctl.store.save_if_due() {
            self.ctl.notify_error(format!("Couldn't save the library: {e:#}"));
        }

        // Waits (pre-waits, links, group offsets) start from tick, and count down on screen.
        if !self.ctl.statuses.is_empty() || !self.ctl.pending.is_empty() || self.mouse_held.is_some() || !self.meter.idle() {
            ctx.request_repaint_after(Duration::from_millis(16));
        } else if self.ctl.store.is_loading() || self.ctl.store.has_unsaved() || self.ctl.notice.is_some() || self.ctl.rendering() {
            ctx.request_repaint_after(Duration::from_millis(200));
        }
    }
}

/// The Settings window's height as egui 0.34 and earlier made it: 420 px of content plus
/// the title bar and frame. egui 0.35 counts a window's default size from the outside, so
/// the content would otherwise lose the title bar's height.
/// "Converting “Movie.mkv” 34% (+2 more)" while ffmpeg converts (or "Downloading …"
/// from QuipClipper), "Copying 3 files…" while only copies run, `None` when nothing is
/// being imported.
fn import_summary(store: &crate::store::Store) -> Option<String> {
    let all: Vec<&crate::store::Importing> = store.importing().map(|(_, i)| i).collect();
    let mut busy: Vec<&&crate::store::Importing> = all.iter().filter(|i| i.busy()).collect();
    busy.sort_by_key(|i| i.file_name());
    let more = |n: usize| if n > 0 { format!(" (+{n} more)") } else { String::new() };
    match busy.first() {
        Some(i) => {
            let pct = i.fraction().map(|f| format!(" {:.0}%", f * 100.0)).unwrap_or_default();
            Some(format!("{} “{}”{pct}{}", i.verb(), i.file_name(), more(all.len() - 1)))
        }
        None if !all.is_empty() => Some(format!("Copying {} file{}…", all.len(), if all.len() == 1 { "" } else { "s" })),
        None => None,
    }
}

fn settings_height(ctx: &egui::Context) -> f32 {
    let style = ctx.global_style();
    let heading = egui::TextStyle::Heading.resolve(&style);
    let title = ctx.fonts_mut(|f| f.row_height(&heading)).max(style.spacing.interact_size.y);
    let (margin, stroke) = (style.spacing.window_margin.sum().y, style.visuals.window_stroke.width);
    420.0 + title + 2.0 * margin + 3.0 * stroke
}

/// Floor of the meter scale.
const METER_FLOOR_DB: f32 = crate::logic::DB_FLOOR;

/// A tile's level meter (t-m19-tile-meters, D-110): a thin bar on an unlit track, lit in
/// the master meter's colours and zones on its scale, with the peak hold as a tick.
fn paint_tile_meter(p: &egui::Painter, rect: Rect, b: &Ballistics, t: &Tokens) {
    p.rect_filled(rect, rect.height() / 2.0, t.meter_off);
    let (green_end, yellow_end) = (meter_frac(-12.0), meter_frac(-3.0));
    let x = |f: f32| rect.left() + f * rect.width();
    let lit = meter_frac(b.level);
    for (from, to, c) in [(0.0, green_end, t.meter_green), (green_end, yellow_end, t.meter_yellow), (yellow_end, 1.0, t.meter_red)] {
        if lit > from {
            p.rect_filled(Rect::from_x_y_ranges(x(from)..=x(lit.min(to)), rect.y_range()), 0.0, c);
        }
    }
    let hold = meter_frac(b.hold);
    if hold > 0.0 {
        let c = if hold >= yellow_end { t.meter_red } else if hold >= green_end { t.meter_yellow } else { t.meter_green };
        p.rect_filled(Rect::from_min_size(pos2(x(hold) - 1.0, rect.top()), vec2(2.0, rect.height())), 0.0, c);
    }
}

/// The widest a tile's meter gets.
const TILE_METER_MAX_W: f32 = 120.0;

/// A meter 3 px high, `gap` below a time readout, as wide as it (at least 36 px) and
/// lined up with its right edge.
fn meter_under(time: Rect, gap: f32) -> Rect {
    let w = time.width().max(36.0);
    Rect::from_min_size(pos2(time.right() - w, time.bottom() + gap), vec2(w, 3.0))
}

/// The same, `gap` above the time readout.
fn meter_over(time: Rect, gap: f32) -> Rect {
    let w = time.width().max(36.0);
    Rect::from_min_size(pos2(time.right() - w, time.top() - gap - 3.0), vec2(w, 3.0))
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



/// Master meter display state: each channel's ballistics (as the tiles' meters) and a
/// clip light that stays lit until clicked.
struct Meter {
    ch: [Ballistics; 2],
    clip: bool,
}

impl Meter {
    fn new() -> Self {
        Meter { ch: [Ballistics::default(); 2], clip: false }
    }

    fn feed(&mut self, peaks: [f32; 2], dt: f32) {
        for (b, peak) in self.ch.iter_mut().zip(peaks) {
            b.feed(peak, dt);
            if peak > 1.0 {
                self.clip = true;
            }
        }
    }

    fn idle(&self) -> bool {
        self.ch.iter().all(Ballistics::idle)
    }
}

/// The app's mark beside its name: the cart (theme::paint_cart); `compact` draws the
/// mark alone.
fn paint_wordmark(ui: &mut Ui, compact: bool) {
    let t = Tokens::get(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(if compact { 22.0 } else { 150.0 }, 28.0), Sense::hover());
    let p = ui.painter();
    theme::paint_cart(p, Rect::from_min_size(rect.left_center() + vec2(0.0, -12.0), vec2(20.0, 24.0)));
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

/// A tile-colour swatch: 24 px to click, named in its hover text and for screen
/// readers (a radio button), and the current one ringed and checked, not only told by
/// its colour.
fn colour_swatch(ui: &mut Ui, c: TileColor, current: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::click());
    let fill = if theme::is_panel(ui.ctx()) { theme::panel_tile(ui.ctx(), c) } else { theme::console_tile(ui.ctx(), c) };
    let p = ui.painter();
    p.circle_filled(rect.center(), 9.5, fill);
    if current {
        p.circle_stroke(rect.center(), 11.0, Stroke::new(2.0_f32, Tokens::get(ui.ctx()).text));
        let ink = theme::ink_on(fill);
        let s = Stroke::new(2.0_f32, ink);
        let o = rect.center();
        p.line_segment([o + vec2(-4.0, 0.5), o + vec2(-1.0, 3.5)], s);
        p.line_segment([o + vec2(-1.0, 3.5), o + vec2(4.0, -3.0)], s);
    }
    let name = format!("{c:?}");
    let resp = theme::role(resp, AxRole::RadioButton, &name, Some(current));
    theme::hint(resp, format!("Colour the tile {}", name.to_lowercase()))
}

fn key_char(key: Key) -> Option<char> {
    let c = match key {
        Key::Minus => '-',
        Key::Equals => '=',
        Key::OpenBracket => '[',
        Key::CloseBracket => ']',
        Key::Quote => '\'',
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

/// A key combination as `global-hotkey` reads it: `Ctrl+Alt+1`, `Cmd+Shift+K`, `F13`.
fn key_combo(key: Key, m: Modifiers) -> String {
    let mut s = String::new();
    for (on, name) in [(m.ctrl, "Ctrl"), (m.alt, "Alt"), (m.shift, "Shift"), (m.mac_cmd, "Cmd")] {
        if on {
            s.push_str(name);
            s.push('+');
        }
    }
    s.push_str(key.name());
    s
}

impl StingerApp {
    /// A set row's menu: on a right-click, or held open under the row by a `setmenu`
    /// script step (egui opens a context menu only at the pointer).
    fn set_row_menu(&self, resp: &egui::Response, set: Uuid, n_sets: usize, locked: bool, windowed: bool, actions: &mut Vec<Action>) {
        if self.set_menu_script == Some(set) {
            egui::Popup::menu(resp).open_memory(Some(egui::SetOpenCommand::Bool(true))).show(|ui| set_menu(ui, set, n_sets, locked, windowed, actions));
        } else {
            resp.context_menu(|ui| set_menu(ui, set, n_sets, locked, windowed, actions));
        }
    }
}

/// A set's context menu, in the sidebar of either theme.
fn set_menu(ui: &mut Ui, set: Uuid, n_sets: usize, locked: bool, windowed: bool, actions: &mut Vec<Action>) {
    if theme::hint(ui.button("Rename"), "Rename this set in the sidebar").clicked() {
        actions.push(Action::StartRename(set));
    }
    if theme::hint(ui.button("Duplicate"), "Copy this set, tiles and all").clicked() {
        actions.push(inv!("set.duplicate", {"set": set}).into());
    }
    let delete = ui.add_enabled(n_sets > 1, egui::Button::new("Delete"));
    if theme::hint_or(delete, "Delete this set and its tiles (Undo brings it back)", "The last set can't be deleted").clicked() {
        actions.push(inv!("set.delete", {"set": set}).into());
    }
    ui.separator();
    // Lock or unlock this set, whichever applies (t-m22-set-menu-lock).
    let (cmd, label, tip) = if locked { ("set.unlock", "Unlock Set", tips::UNLOCK) } else { ("set.lock", "Lock Set", tips::LOCK) };
    if theme::hint(ui.button(label), tip).clicked() {
        actions.push(Invocation::new(cmd, json!({"set": set})).into());
    }
    ui.separator();
    // A window of its own, to put on another monitor or workspace (D-224).
    let (cmd, label, tip) = if windowed { ("window.close", "Close Window", tips::CLOSE_WINDOW) } else { ("window.open", "Open in New Window", tips::OPEN_WINDOW) };
    if theme::hint(ui.button(label), tip).clicked() {
        actions.push(Invocation::new(cmd, json!({"set": set})).into());
    }
}

/// Enter or Space pressed (not repeated, no modifiers) while `resp` has keyboard focus.
/// Whether this press on a tile is the second of a double-click (the first is
/// remembered): the press that plays it (M22).
fn second_press(ui: &Ui, tile: Uuid) -> bool {
    let key = egui::Id::unique("last-tile-press");
    let now = ui.input(|i| i.time);
    let delay = ui.ctx().options(|o| o.input_options.max_double_click_delay);
    let double = ui.data(|d| d.get_temp::<(Uuid, f64)>(key)).is_some_and(|(id, at)| id == tile && now - at <= delay);
    ui.data_mut(|d| {
        if double {
            d.remove::<(Uuid, f64)>(key);
        } else {
            d.insert_temp(key, (tile, now));
        }
    });
    double
}

/// A pad key's cap at `min`: `size` at least, wider for an Option key (`⌥Q`, `Alt+Q`).
#[allow(clippy::too_many_arguments)]
fn paint_key_cap(p: &egui::Painter, min: Pos2, size: Vec2, key: PadKey, font: FontId, fill: Color32, edge: Option<Color32>, ink: Color32) -> Rect {
    let galley = p.layout_no_wrap(key.label(), font, ink);
    let cap = Rect::from_min_size(min, vec2(size.x.max(galley.size().x + 8.0), size.y));
    p.rect_filled(cap, 3.0, fill);
    if let Some(edge) = edge {
        p.rect_stroke(cap, 3.0, Stroke::new(1.0_f32, edge), StrokeKind::Inside);
    }
    p.galley(cap.center() - galley.size() / 2.0, galley, ink);
    cap
}

fn activation_key(ui: &Ui, resp: &egui::Response) -> Option<Key> {
    if !resp.has_focus() {
        return None;
    }
    ui.input(|i| {
        i.events.iter().find_map(|e| match e {
            // Space is Pause All (D-199), so only Enter plays a focused tile.
            egui::Event::Key { key: k @ Key::Enter, pressed: true, repeat: false, modifiers, .. } if !modifiers.any() => Some(*k),
            _ => None,
        })
    })
}

/// A notes field: a tile's in the inspector, a set's above the cue list.
fn notes_editor(ui: &mut Ui, notes: &mut String, hint: &str) -> egui::Response {
    theme::text_field(ui, egui::TextEdit::multiline(notes).hint_text(hint).desired_rows(2).desired_width(f32::INFINITY))
}

/// A small key with an up or down arrow, for stepping the cue pointer. The arrow is
/// drawn: the bundled fonts have no arrow glyphs.
fn step_key(ui: &mut Ui, up: bool) -> egui::Response {
    // Two of these sit in the toolbar beside Pause All.
    let resp = theme::key(ui, "", vec2(34.0, 34.0), false);
    let c = resp.rect.center();
    let (tip, base) = if up { (c.y - 5.0, c.y + 4.0) } else { (c.y + 5.0, c.y - 4.0) };
    let ink = Tokens::get(ui.ctx()).text;
    ui.painter().add(egui::Shape::convex_polygon(vec![pos2(c.x, tip), pos2(c.x + 6.0, base), pos2(c.x - 6.0, base)], ink, Stroke::NONE));
    resp
}

/// Where a cue-list row's armed mark sits, from the row's left edge (both themes).
const CUE_MARK_X: f32 = 70.0;

/// A cue row's armed mark (D-197): armed until the cue plays, cleared by Reset List.
#[derive(Clone, Copy, PartialEq, Debug)]
enum CueMark {
    Armed,
    /// Playing now, with the spinner's angle in radians.
    Playing(f32),
    Played,
}

/// The mark a row shows: a spinner while it sounds, else armed or played.
fn cue_mark(ui: &Ui, status: Option<&VoiceStatus>, played: bool) -> CueMark {
    match status {
        Some(s) if s.state != PlayState::Paused => CueMark::Playing((ui.input(|i| i.time) * 5.0) as f32),
        _ if played => CueMark::Played,
        _ => CueMark::Armed,
    }
}

/// A filled dot (armed), a ring (played) or a turning arc (playing).
fn paint_cue_mark(p: &egui::Painter, c: Pos2, r: f32, mark: CueMark, color: Color32) {
    match mark {
        CueMark::Armed => {
            p.circle_filled(c, r, color);
        }
        CueMark::Played => {
            p.circle_stroke(c, r - 0.75, Stroke::new(1.5_f32, color));
        }
        CueMark::Playing(a) => {
            let pts: Vec<Pos2> = (0..=18).map(|k| a + k as f32 / 18.0 * std::f32::consts::TAU * 0.75).map(|t| c + r * vec2(t.cos(), t.sin())).collect();
            p.add(egui::Shape::line(pts, Stroke::new(2.0_f32, color)));
        }
    }
}

/// A cue row's armed mark as a button: a click passes the cue over (or arms it again).
/// Not a focus stop (it would tie with its row for the arrow keys); the row's menu has
/// the same item.
fn cue_mark_button(ui: &Ui, rect: Rect, tile: Uuid, index: usize, played: bool, actions: &mut Vec<Action>) {
    let area = Rect::from_center_size(pos2(rect.left() + CUE_MARK_X, rect.center().y), vec2(28.0, rect.height()));
    let resp = ui.interact(area, egui::Id::unique(("cue-mark", tile)), Sense::CLICK);
    if resp.clicked() {
        actions.push(inv!("cue.arm", {"tile": tile, "armed": played}).into());
    }
    let resp = theme::role(resp, AxRole::CheckBox, format!("Cue {} armed", index + 1), Some(!played));
    theme::hint(resp.on_hover_cursor(egui::CursorIcon::PointingHand), tips::CUE_ARM);
}

/// The bar under a cue list (D-199): Continuous | Queued and the cross-fade on the left,
/// Reset List at the right end; `narrow` puts the cross-fade on a second line.
fn list_bar(ui: &mut Ui, narrow: bool, (continuous, crossfade): (&mut bool, &mut f32), reset: &mut bool, locked: bool) {
    let row = |ui: &mut Ui, continuous: &mut bool, crossfade: &mut f32, reset: &mut bool| {
        ui.horizontal(|ui| {
            ui.set_height(30.0);
            if let Some(c) = ui.add_enabled_ui(!locked, |ui| list_mode_switch(ui, *continuous, locked)).inner {
                *continuous = c;
            }
            if !narrow {
                ui.add_space(16.0);
                crossfade_slider(ui, crossfade, *continuous && !locked);
            }
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                *reset = theme::hint(theme::role(theme::key(ui, "Reset List", vec2(96.0, 26.0), false), AxRole::Button, "Reset List", None), tips::RESET_LIST).clicked();
            });
        });
    };
    row(ui, continuous, crossfade, reset);
    if narrow {
        ui.horizontal(|ui| crossfade_slider(ui, crossfade, *continuous && !locked));
    }
}

/// A cue list's cross-fade, 0–5 s: only Continuous uses it, so it's off in Queued.
fn crossfade_slider(ui: &mut Ui, seconds: &mut f32, enabled: bool) {
    ui.add_enabled_ui(enabled, |ui| {
        ui.spacing_mut().slider_width = 110.0;
        ui.label("Cross-fade");
        let slider = egui::Slider::new(seconds, 0.0..=crate::model::CROSSFADE_MAX).step_by(0.1).custom_formatter(|x, _| if x <= 0.0 { "None".into() } else { format!("{x:.1} s") });
        let why = if enabled { tips::LOCKED_WHY } else { tips::CROSSFADE_WHY };
        theme::hint_or(theme::name(ui.add(slider), "Cross-fade"), tips::CROSSFADE, why);
    });
}

/// Continuous | Queued, a List set's playback mode: the new mode when it's clicked.
fn list_mode_switch(ui: &mut Ui, continuous: bool, locked: bool) -> Option<bool> {
    let mut picked = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        // 92 and 70 wide, or less in a narrow inspector (D-211).
        let scale = ((ui.available_width() - 2.0) / 162.0).clamp(0.75, 1.0);
        for (value, label, tip) in [(true, "Continuous", tips::CONTINUOUS), (false, "Queued", tips::QUEUED)] {
            let on = continuous == value;
            let key = theme::role(theme::key(ui, label, vec2(if value { 92.0 } else { 70.0 } * scale, 26.0), on), AxRole::RadioButton, label, Some(on));
            if theme::hint_or(key, tip, tips::LOCKED_WHY).clicked() && !on && !locked {
                picked = Some(value);
            }
        }
    });
    picked
}

/// One of the handles on the inspector's waveform.
#[derive(Clone, Copy)]
enum Handle {
    Start,
    End,
    VampStart,
    VampEnd,
}

impl Handle {
    fn name(self) -> &'static str {
        match self {
            Handle::Start => "Trim start",
            Handle::End => "Trim end",
            Handle::VampStart => "Vamp start",
            Handle::VampEnd => "Vamp end",
        }
    }

    fn tip(self) -> &'static str {
        match self {
            Handle::Start => "Where the tile starts playing: drag it — or Tab to it and press Left/Right (Shift for 0.1 s steps)",
            Handle::End => "Where the tile stops playing: drag it — or Tab to it and press Left/Right (Shift for 0.1 s steps)",
            Handle::VampStart => "Where the vamp section starts: drag it — or Tab to it and press Left/Right (Shift for 0.1 s steps)",
            Handle::VampEnd => "Where the vamp section loops back: drag it — or Tab to it and press Left/Right (Shift for 0.1 s steps)",
        }
    }

    /// Move it to `sec`, keeping start before end and the vamp inside them.
    fn set(self, t: &mut Tile, sec: f64, dur: f64) {
        let (s, e) = play_range(t, dur);
        match self {
            Handle::Start => t.start = sec.min(e - 0.01).max(0.0),
            Handle::End => {
                let end = sec.max(s + 0.01);
                t.end = if dur - end < 1e-3 { None } else { Some(end) };
            }
            Handle::VampStart | Handle::VampEnd => {
                if let Some((a, b)) = crate::logic::vamp_range(t, dur) {
                    t.vamp = Some(match self {
                        Handle::VampStart => (sec.clamp(s, b - 0.05), b),
                        _ => (a, sec.clamp(a + 0.05, e)),
                    });
                }
            }
        }
    }
}

/// A stop fade's readout: none of its own still gets the 0.1 s de-click (D-205).
/// The stop fade slider's readout at 0 (D-205): no fade of its own, only the 0.1 s
/// de-click every stop gets. Short, so the rail keeps its room in a narrow inspector
/// (D-211); the hover text says the rest.
const STOP_FADE_ZERO: &str = "De-click";

fn stop_fade_text(x: f64) -> String {
    if x <= 0.0 {
        STOP_FADE_ZERO.into()
    } else {
        format!("{x:.1} s")
    }
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
    put("emoji", t.emoji != orig.emoji, json!(t.emoji));
    put("color", t.color != orig.color, json!(t.color));
    put("hold", t.press.hold != orig.press.hold, json!(t.press.hold));
    put("pausable", t.press.pausable != orig.press.pausable, json!(t.press.pausable));
    put("oneShot", t.press.one_shot != orig.press.one_shot, json!(t.press.one_shot));
    put("volume", t.volume != orig.volume, json!(t.volume));
    put("muted", t.muted != orig.muted, json!(t.muted));
    put("gainDb", t.gain_db != orig.gain_db, json!(t.gain_db));
    put("speed", t.speed != orig.speed, json!(t.speed));
    put("pitch", t.pitch != orig.pitch, json!(t.pitch));
    put("channels", t.channel_mix != orig.channel_mix, json!(t.channel_mix));
    put("sends", t.sends != orig.sends, json!(t.sends.iter().map(|s| json!({"output": s.output, "gainDb": s.gain_db})).collect::<Vec<_>>()));
    put("duck", t.duck != orig.duck, json!(t.duck));
    put("duckLevel", t.duck_db != orig.duck_db, json!(t.duck_db));
    put("fadeIn", t.fade_in != orig.fade_in, json!(t.fade_in));
    put("fadeOut", t.fade_out != orig.fade_out, json!(t.fade_out));
    put("stopFade", t.stop_fade != orig.stop_fade, json!(t.stop_fade));
    put("preWait", t.pre_wait != orig.pre_wait, json!(t.pre_wait));
    put("link", t.link != orig.link, json!(t.link));
    put("postWait", t.post_wait != orig.post_wait, json!(t.post_wait));
    put("fadeCurve", t.fade_curve != orig.fade_curve, json!(t.fade_curve));
    put("loop", t.looping != orig.looping, json!(t.looping));
    put("solo", t.solo != orig.solo, json!(t.solo));
    put("trimStart", t.start != orig.start, json!(t.start));
    put("trimEnd", t.end != orig.end, json!(t.end));
    if t.vamp != orig.vamp {
        match t.vamp {
            Some((a, b)) => {
                put("vampStart", true, json!(a));
                put("vampEnd", true, json!(b));
            }
            None => put("vamp", true, Value::Null),
        }
    }
    put("pictureLayer", t.image_layer != orig.image_layer, json!(t.image_layer));
    put("pictureFit", t.image_fit != orig.image_fit, json!(t.image_fit));
    put("pictureOpacity", t.image_opacity != orig.image_opacity, json!(t.image_opacity));
    put("notes", t.notes != orig.notes, json!(t.notes));
    put("disabled", t.disabled != orig.disabled, json!(t.disabled));
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
    /// Being recorded into right now.
    recording: Option<RecMark>,
    /// An action tile's summary ("Fades Music, Rain"), drawn where the waveform goes.
    caption: Option<String>,
    /// The tile's meter, while it sounds and meters are shown.
    level: Option<Ballistics>,
    /// Settings ▸ Coloured pads: the tile's colour all over (D-152).
    colored: bool,
    /// A cue row whose armed mark has cleared (it has played since Reset List).
    played: bool,
    /// The widget's id while it has the keyboard focus, for a painter that draws its own
    /// focus cue (Panel's pads and cue rows).
    focus: Option<egui::Id>,
}

/// A tile being recorded into, as it's drawn (D-127): the input's level and how far the
/// red mark is through its pulse.
#[derive(Clone, Copy)]
struct RecMark {
    /// The input's level in dBFS; `None` while the input opens.
    level_db: Option<f32>,
    /// 0.45–1, once a second.
    pulse: f32,
}

/// Where the recording mark is in its once-a-second pulse (0.45–1), with a repaint to
/// keep it and the elapsed time moving.
fn rec_pulse(ctx: &egui::Context) -> f32 {
    ctx.request_repaint_after(Duration::from_millis(50));
    let t = ctx.input(|i| i.time);
    0.45 + 0.55 * (0.5 + 0.5 * (t * std::f64::consts::TAU).cos()) as f32
}

/// "Recording 0:12 from MacBook Pro Microphone" (or that it waits for the input).
fn recording_text(rec: &crate::controller::RecordingView) -> String {
    match &rec.input {
        Some(input) => format!("Recording {} from {input}", format_clock(rec.seconds, false)),
        None => format!("Recording · {}", crate::controller::INPUT_WAITING),
    }
}

/// A recording's tile title as the counter names it: "Recording 1" as it is, "Old" as
/// "Recording Old".
fn rec_title(title: &str) -> String {
    if title.starts_with("Recording") { title.to_owned() } else { format!("Recording {title}") }
}

/// The recording mark: a `status/error` dot at `alpha`.
fn paint_rec_dot(p: &egui::Painter, center: Pos2, radius: f32, t: &Tokens, alpha: f32) {
    p.circle_filled(center, radius, theme::with_alpha(t.error, alpha));
}

/// The input's level as a bar on the meter's scale, in `input/level`.
fn paint_input_level(p: &egui::Painter, rect: Rect, db: f32, t: &Tokens) {
    p.rect_filled(rect, rect.height() / 2.0, t.meter_off);
    let lit = meter_frac(db);
    if lit > 0.0 {
        p.rect_filled(Rect::from_min_size(rect.min, vec2(rect.width() * lit, rect.height())), rect.height() / 2.0, t.input_level);
    }
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
/// A wait in seconds, short: "2s", "1.5s", "1:30".
fn wait_text(s: f64) -> String {
    if s >= 60.0 {
        format_clock(s, false)
    } else if s.fract().abs() < 1e-6 {
        format!("{s:.0}s")
    } else {
        format!("{s:.1}s")
    }
}

/// The pre-wait, and what starts the next cue (the inspector's Cue tab).
fn cue_link_editor(ui: &mut Ui, t: &mut Tile) {
    fn secs(v: &mut f64) -> egui::DragValue<'_> {
        egui::DragValue::new(v).range(0.0..=crate::model::WAIT_MAX).speed(0.05).max_decimals(2).suffix(" s")
    }
    egui::Grid::new("cue-links").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
        ui.label("Pre-wait");
        theme::hint(theme::name(ui.add(secs(&mut t.pre_wait)), "Pre-wait"), "Seconds between triggering it (GO, a click, a key) and its start");
        ui.end_row();
        ui.label("Next cue");
        let combo = egui::ComboBox::from_id_salt("cue-link").width(ui.available_width().min(240.0)).selected_text(t.link.label()).show_ui(ui, |ui| {
            for l in CueLink::ALL {
                ui.selectable_value(&mut t.link, l, l.label());
            }
        });
        theme::hint(theme::name(combo.response, &format!("Next cue, {}", t.link.label())), "What starts the next cue in the set without a GO");
        ui.end_row();
        ui.label("Post-wait");
        let post = theme::name(ui.add_enabled(t.link != CueLink::None, secs(&mut t.post_wait)), "Post-wait");
        theme::hint_or(post, "Continue: seconds after this one starts. Follow: seconds after it ends", "Only when Next cue starts on its own");
        ui.end_row();
    });
}

/// The line under a linked cue in the cue list: what starts the next cue, and when.
fn link_text(tile: &Tile, counting: Option<f64>) -> Option<String> {
    let after = |what: &str| if tile.post_wait > 0.0 { format!("{} after this one {what}", wait_text(tile.post_wait)) } else { format!("when this one {what}") };
    let how = match tile.link {
        CueLink::None => return None,
        CueLink::Continue if tile.post_wait > 0.0 => format!("Continue: the next cue starts {}", after("starts")),
        CueLink::Continue => "Continue: the next cue starts with this one".to_owned(),
        CueLink::Follow => format!("Follow: the next cue starts {}", after("ends")),
    };
    Some(match counting {
        Some(left) => format!("{how} (in {:.1}s)", left),
        None => how,
    })
}

/// The badge line in the cue list: the options in words, then [`detail_badge_text`].
fn mode_badge_text(tile: &Tile) -> String {
    let mut parts = Vec::new();
    if tile.action.is_none() {
        let fade = pad_options::fades_out(tile);
        for (on, word) in [(tile.press.hold, "HOLD"), (tile.press.pausable, "PAUSABLE"), (tile.press.one_shot, "ONE-SHOT"), (tile.looping, "LOOP"), (tile.solo, "SOLO"), (fade, "FADE OUT")] {
            if on {
                parts.push(word);
            }
        }
    }
    let detail = detail_badge_text(tile, None);
    if !detail.is_empty() {
        parts.push(&detail);
    }
    parts.join(" · ")
}

/// A vamp's live state (D-157): looping its section, or let go and playing on.
pub(crate) fn vamp_state_word(tile: &Tile, status: Option<&VoiceStatus>) -> Option<&'static str> {
    let s = status.filter(|s| tile.vamp.is_some() && matches!(s.state, PlayState::Playing | PlayState::Paused))?;
    if s.vamping {
        Some("VAMPING")
    } else if s.played_on {
        Some("PLAYING ON")
    } else {
        None
    }
}

/// What a pad's face says beyond its option badges and Solo border: disabled, an action,
/// the vamp (VAMPING and PLAYING ON while it sounds), the pre-wait, a cue link, pitch and
/// speed.
fn detail_badge_text(tile: &Tile, status: Option<&VoiceStatus>) -> String {
    let mut parts = Vec::new();
    if tile.vamp.is_some() {
        parts.push(vamp_state_word(tile, status).unwrap_or("VAMP"));
    }
    if let Some(a) = &tile.action {
        parts.insert(0, a.kind.badge());
    }
    if tile.disabled {
        parts.insert(0, "DISABLED");
    }
    let mut out = parts.join(" · ");
    // Cue links: the pre-wait, then what starts the next cue.
    let mut add = |s: String| out = if out.is_empty() { s } else { format!("{out} · {s}") };
    if tile.pre_wait > 0.0 {
        add(format!("PRE {}", wait_text(tile.pre_wait)));
    }
    match tile.link {
        CueLink::Continue => add("CONT".into()),
        CueLink::Follow => add("FOLLOW".into()),
        CueLink::None => {}
    }
    if tile.pitch != 0.0 {
        out = if out.is_empty() { format!("{:+} st", tile.pitch) } else { format!("{out} · {:+} st", tile.pitch) };
    }
    if tile.speed != 1.0 {
        out = if out.is_empty() { format!("{:.2}×", tile.speed) } else { format!("{out} · {:.2}×", tile.speed) };
    }
    out
}

/// The narrowest Console tile the grid will draw in Fixed columns; below it the grid
/// shows fewer columns.
const TILE_MIN_WIDTH: f32 = 140.0;

/// Where ⌘/Ctrl+arrow moves the tile at flat `index` in Rows mode (D-116, D-143), as a
/// place for `SoundSet::move_tile`: Left and Right swap it with the slot beside it (a pad
/// or a blank) and cross into the neighbouring row's end or start; Up and Down take it to
/// the same slot of the row above or below (filling a blank there), or past the first or
/// last row into a new row of its own (unless it's alone already). `None` when it can't
/// go that way.
fn row_move(slots: &Grid<usize>, index: usize, key: Key) -> Option<Place> {
    let (row, at) = rows::find(slots, &index)?;
    let (last, alone) = (slots.len() - 1, slots[row].len() == 1);
    match key {
        Key::ArrowLeft if at > 0 => Some(Place::InRow { row, at: at - 1 }),
        Key::ArrowLeft if row > 0 => Some(Place::InRow { row: row - 1, at: usize::MAX }),
        Key::ArrowRight if at + 1 < slots[row].len() => Some(Place::InRow { row, at: at + 1 }),
        Key::ArrowRight if row < last => Some(Place::InRow { row: row + 1, at: 0 }),
        // A shorter row: its end, not blanks up to the same slot (D-190 is for drags).
        Key::ArrowUp if row > 0 => Some(Place::Slot { row: row - 1, at: at.min(slots[row - 1].len()) }),
        Key::ArrowUp if !alone => Some(Place::NewRow(0)),
        Key::ArrowDown if row < last => Some(Place::Slot { row: row + 1, at: at.min(slots[row + 1].len()) }),
        Key::ArrowDown if !alone => Some(Place::NewRow(slots.len())),
        _ => None,
    }
}

/// The row size when a set's rows are even (every row but a shorter last one that long),
/// for the menu's check mark.
fn even_rows(rows: &[usize]) -> Option<u32> {
    let (&first, rest) = rows.split_first()?;
    let even = rest.split_last().is_none_or(|(&last, mid)| mid.iter().all(|&r| r == first) && last <= first);
    even.then_some(first as u32)
}

/// The least room the status bar's notice gets before the output's name gives way (D-191).
const NOTICE_MIN: f32 = 240.0;

/// The shortest Console rows get when they share the board's height (D-140).
const ROW_MIN_HEIGHT: f32 = 72.0;

/// Where a dragged pad or a dropped file goes (D-141): a slot tinted and outlined in the
/// accent.
/// Alt held while a pad is dragged: the drop copies it (D-229), as Finder's Option-drag.
fn drag_copies(ctx: &egui::Context) -> bool {
    ctx.input(|i| i.modifiers.alt)
}

/// The "+" on a pad dragged with Alt, centred at `c`: its drop makes a copy (D-229).
fn paint_copy_badge(p: &egui::Painter, c: Pos2, green: Color32) {
    p.circle_filled(c, 11.0, green);
    p.circle_stroke(c, 11.0, Stroke::new(1.5_f32, Color32::WHITE));
    let ink = theme::ink_on(green);
    p.line_segment([c - vec2(5.0, 0.0), c + vec2(5.0, 0.0)], Stroke::new(2.5_f32, ink));
    p.line_segment([c - vec2(0.0, 5.0), c + vec2(0.0, 5.0)], Stroke::new(2.5_f32, ink));
}

fn paint_ghost(p: &egui::Painter, rect: Rect, accent: Color32) {
    p.rect_filled(rect, 6.0, theme::with_alpha(accent, 0.14));
    dashed_outline(p, rect.shrink(1.0), Stroke::new(2.0_f32, accent));
}

/// A dashed rectangle, as the add slot, blank slots and phantom rows have.
fn dashed_outline(p: &egui::Painter, r: Rect, stroke: Stroke) {
    for (a, b) in [(r.left_top(), r.right_top()), (r.right_top(), r.right_bottom()), (r.right_bottom(), r.left_bottom()), (r.left_bottom(), r.left_top())] {
        p.extend(egui::Shape::dashed_line(&[a, b], stroke, 6.0, 4.0));
    }
}

/// Tile height for a given width in the Console grid.
fn tile_height(w: f32) -> f32 {
    (w / 1.5).clamp(58.0, 170.0)
}

/// How many grid columns to draw: the set's `columns`, or fewer when tiles that wide
/// would be narrower than `min_w`. The saved setting is left alone, so widening the
/// window brings the columns back.
fn fit_columns(want: usize, avail: f32, min_w: f32, gap: f32) -> usize {
    let fit = ((avail + gap) / (min_w + gap)).floor().max(1.0) as usize;
    want.clamp(1, fit)
}

/// The least room a Console pad's title gets beside its key cap and badges before it
/// moves to the line below them.
const TITLE_BESIDE_MIN: f32 = 64.0;

/// The most title lines a pad shows before the ellipsis (M22). A narrow pad's title has
/// no such limit: it runs on down the pad, over the waveform (D-217).
const TITLE_LINES: usize = 4;

/// The largest of `sizes` (largest first) at which every word of `title` fits `room`, so
/// a narrow pad's title wraps between words instead of inside one (D-217); the smallest
/// when none does, and then a long word breaks.
/// Below this much room for its title a pad leaves its emoji off (D-253).
const EMOJI_ROOM_MIN: f32 = 64.0;

/// What a pad writes as its title: the tile's emoji, a space, then its title (D-253); the
/// title alone on a pad with less than [`EMOJI_ROOM_MIN`] of room, or with no emoji. The
/// emoji are drawn in the title's colour and size by egui's monochrome emoji face; one
/// that face lacks (newer ones, skin tones) is left off rather than drawn as a box.
fn pad_title(ui: &Ui, tile: &Tile, room: f32) -> String {
    pad_title_with(tile, room, |c| ui.fonts_mut(|f| f.has_glyph(&FontId::proportional(13.0), c)))
}

/// [`pad_title`], with `drawable` saying which characters the fonts have.
fn pad_title_with(tile: &Tile, room: f32, drawable: impl Fn(char) -> bool) -> String {
    let emoji: String = tile.emoji.chars().filter(|&c| crate::model::emoji_modifier(c) || drawable(c)).collect();
    if room < EMOJI_ROOM_MIN || emoji.chars().all(crate::model::emoji_modifier) {
        tile.title.clone()
    } else {
        format!("{emoji} {}", tile.title)
    }
}

fn title_size(ui: &Ui, title: &str, room: f32, sizes: &[f32], font: impl Fn(f32) -> egui::FontId) -> f32 {
    let widest = |size: f32| ui.fonts_mut(|f| title.split_whitespace().map(|w| f.layout_no_wrap(w.to_owned(), font(size), Color32::WHITE).size().x).fold(0.0, f32::max));
    // A little to spare: the wrap counts a glyph's full advance.
    sizes.iter().copied().find(|&s| widest(s) <= room - 3.0).unwrap_or(sizes[sizes.len() - 1])
}

/// A pad (M22): by default a dark body with the tile's colour on the title, the waveform
/// and the border (D-152), or with Coloured pads on, the tile's colour all over (D-146).
/// On it the waveform, and on top the hotkey cap and up to four lines of title at the top
/// left, the option badges at the top right, and the state, meter and time along the
/// bottom. The lettering reads over the waveform: a dark pad's is brighter than it and
/// haloed, a coloured pad's sits on plates of its colour (theme::pad_face). While it
/// plays, the played part is washed over and a red playhead marks the position. Solo adds
/// a ring inside the edge.
fn paint_tile(ui: &Ui, rect: Rect, tile: &Tile, status: Option<&VoiceStatus>, time: &str, f: TileFlags) {
    let t = Tokens::get(ui.ctx());
    let p = ui.painter();
    let radius = 6.0;
    let r = radius as u8;
    let corners = CornerRadius::same(r);
    let sounding = status.is_some_and(|s| matches!(s.state, PlayState::Playing | PlayState::Stopping));
    let paused = status.is_some_and(|s| s.state == PlayState::Paused);
    let high_contrast = theme::is_high_contrast(ui.ctx());
    let mut base = theme::console_tile(ui.ctx(), tile.color);
    if tile.file.is_none() && tile.action.is_none() {
        base = theme::shade(base, 0.6);
    }
    let face = theme::pad_face(base, f.colored, high_contrast, theme::inks(ui.ctx()).tile, f.hovered && !sounding, status.is_some());
    let ink = face.title;

    p.rect_filled(rect, radius, face.body);
    if let (Some(tex), ImageLayer::Underlay) = (f.picture, tile.image_layer) {
        paint_picture(p, rect, corners, tile, tex);
    }
    let wave = Rect::from_min_max(pos2(rect.left() + 8.0, rect.top() + 8.0), pos2(rect.right() - 8.0, rect.bottom() - 26.0));
    let played_x = status.map(|s| wave.left() + wave.width() * s.progress());
    if let Some(px) = played_x {
        let wash = Rect::from_min_max(rect.min, pos2(px, rect.bottom()));
        p.rect_filled(wash, CornerRadius { nw: r, ne: 0, sw: r, se: 0 }, face.wash);
    }
    // While it's recorded into, the input takes the waveform's place.
    match f.waveform.filter(|_| f.recording.is_none()) {
        Some((peaks, duration)) => paint_waveform(p, wave, tile, peaks, duration, played_x, face.wave_played, face.wave_rest),
        None => {
            let text = f.caption.as_deref().or(tile.file.is_none().then_some("Drop audio here"));
            if let Some(text) = text {
                let g = ui.fonts_mut(|fonts| {
                    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), theme::regular(12.0), ink);
                    job.wrap.max_width = wave.width();
                    job.wrap.max_rows = 2;
                    job.halign = Align::Center;
                    fonts.layout_job(job)
                });
                p.galley(pos2(wave.center().x, wave.bottom() - g.size().y), g, ink);
            }
        }
    }
    if let Some(px) = played_x {
        p.line_segment([pos2(px, rect.top() + 2.0), pos2(px, rect.bottom() - 2.0)], Stroke::new(1.0_f32, t.playhead));
    }
    if let (Some(tex), ImageLayer::Overlay) = (f.picture, tile.image_layer) {
        paint_picture(p, rect, corners, tile, tex);
    }

    // Top row: hotkey cap, title, then the selection's check and the option badges at the right.
    let plate = |r: Rect| {
        if face.plate.a() > 0 {
            p.rect_filled(r, 3.0, face.plate);
        }
    };
    let top = rect.top() + 6.0;
    let mut x = rect.left() + 6.0;
    if let Some(k) = tile.hotkey {
        let cap = paint_key_cap(p, pos2(x, top), vec2(18.0, 18.0), k, theme::bold(11.0), face.cap, face.cap_edge, face.cap_ink);
        x = cap.right() + 6.0;
    }
    let mut right = rect.right() - 6.0;
    if f.selected {
        theme::paint_check_badge(p, pos2(right - 9.0, top + 9.0), t.accent, theme::ink_on(t.accent));
        right -= 22.0;
    }
    // A narrow pad keeps its badges and starts the title on the line below them.
    let line_h = 16.0;
    let mut title_top = top + 9.0 - line_h / 2.0;
    let shown = pad_options::fit_badges(tile, right - x - 8.0, 17.0);
    if !shown.is_empty() {
        let badges_w = shown.len() as f32 * 17.0;
        let back = Rect::from_min_max(pos2(right - badges_w - 3.0, top - 1.0), pos2(right + 2.0, top + 15.0));
        plate(back);
        haloed(face.halo, |o, c| {
            pad_options::paint_badges(p, pos2(right, top + 1.0) + o, 13.0, &shown, c);
        });
        let used = pad_options::paint_badges(p, pos2(right, top + 1.0), 13.0, &shown, ink) + 6.0;
        right -= used;
    }
    // Too little room beside the key cap and the badges: the title starts on the line
    // below them with the pad's whole width, and runs on down the pad as far as it needs
    // (D-217).
    let below = right - x < TITLE_BESIDE_MIN;
    if below {
        title_top += 18.0;
        (x, right) = (rect.left() + 6.0, rect.right() - 6.0);
    }
    let most = if below { usize::MAX } else { TITLE_LINES };
    let mut rows = (((rect.bottom() - 24.0 - title_top) / line_h).floor() as usize).clamp(1, most);
    // A sounding vamp's state always gets its line (D-157).
    if rows > 1 && vamp_state_word(tile, status).is_some() {
        rows -= 1;
    }
    let room = (right - x).max(10.0);
    let text = pad_title(ui, tile, room);
    let size = title_size(ui, &text, room, &[13.0, 11.5, 10.0], theme::semibold);
    let title = ui.fonts_mut(|fonts| {
        let mut job = egui::text::LayoutJob::simple(text, theme::semibold(size), ink, room);
        job.wrap.max_rows = rows;
        fonts.layout_job(job)
    });
    let title_pos = pos2(x, title_top);
    for row in &title.rows {
        let r = row.rect().translate(title_pos.to_vec2()).expand2(vec2(3.0, 0.0));
        if r.width() > 6.0 {
            plate(r);
        }
    }
    let title_bottom = title_pos.y + title.size().y;
    haloed(face.halo, |o, c| p.galley_with_override_text_color(title_pos + o, title.clone(), c));
    p.galley(title_pos, title, ink);
    // What the badges don't show (VAMP, PRE 2 s, CONT…), under the title when there's room.
    let detail = if f.recording.is_some() { "RECORDING".to_owned() } else { detail_badge_text(tile, status) };
    if !detail.is_empty() && title_bottom + 12.0 <= rect.bottom() - 24.0 {
        let g = ui.fonts_mut(|fonts| {
            let mut job = egui::text::LayoutJob::simple_singleline(detail, theme::bold(9.0), face.detail);
            job.wrap.max_width = (rect.right() - 8.0 - x).max(10.0);
            job.wrap.max_rows = 1;
            fonts.layout_job(job)
        });
        let at = pos2(x, title_bottom + 1.0);
        plate(Rect::from_min_size(at, g.size()).expand2(vec2(3.0, 0.0)));
        haloed(face.halo, |o, c| p.galley_with_override_text_color(at + o, g.clone(), c));
        p.galley(at, g, face.detail);
    }

    // Footer: state on the left, time on the right.
    let foot = rect.bottom() - 13.0;
    if let Some(rec) = f.recording {
        paint_rec_dot(p, pos2(rect.left() + 14.0, foot), 5.0, &t, rec.pulse);
    } else if sounding || paused {
        // On the dark body the state shows in its own colour, as the outline does.
        let (icon, c) = if paused { ("⏸", t.solo) } else { ("▶", t.counter_text) };
        p.text(pos2(rect.left() + 10.0, foot), Align2::LEFT_CENTER, icon, theme::bold(11.0), if f.colored { ink } else { c });
    }
    let time_color = if f.failed || f.recording.is_some() { t.error } else { face.time };
    // A narrow pad's time steps down rather than run into the state mark (D-217).
    let marked = f.recording.is_some() || sounding || paused;
    let time_w = ui.fonts_mut(|fonts| fonts.layout_no_wrap(time.to_owned(), theme::semibold(13.0), time_color).size().x);
    let time_size = if marked && rect.right() - 10.0 - time_w < rect.left() + 24.0 { 11.0 } else { 13.0 };
    let time_rect = theme::paint_tabular(p, pos2(rect.right() - 10.0 + (13.0 - time_size), foot), Align2::RIGHT_CENTER, time, theme::semibold(time_size), time_color);
    // The meter runs from the ▶ toward the time; a pad too narrow for that (a full
    // row's, D-117) has it over the time instead (D-118).
    if let Some(b) = f.level.filter(|_| sounding) {
        let (left, right) = (rect.left() + 26.0, time_rect.left() - 10.0);
        let meter = if right - left >= 24.0 {
            Rect::from_min_size(pos2(left, foot - 1.5), vec2((right - left).min(TILE_METER_MAX_W), 3.0))
        } else {
            meter_over(time_rect, 1.0)
        };
        paint_tile_meter(p, meter, &b, &t);
    }
    // A recording's input level, where a playing pad's meter goes.
    if let Some(db) = f.recording.and_then(|r| r.level_db) {
        let (left, right) = (rect.left() + 26.0, time_rect.left() - 10.0);
        let meter = if right - left >= 24.0 {
            Rect::from_min_size(pos2(left, foot - 2.0), vec2((right - left).min(TILE_METER_MAX_W), 4.0))
        } else {
            meter_over(time_rect, 1.0)
        };
        paint_input_level(p, meter, db, &t);
    }

    if tile.solo {
        pad_options::paint_solo_ring(p, rect, corners, t.solo_mark);
    }
    if tile.disabled {
        let veil = theme::look(ui.ctx()).veil;
        p.rect_filled(rect, radius, veil.fill);
        theme::paint_hatch(p, rect.shrink(3.0), veil.stripes);
    }
    let state = if f.drop_hover || f.selected {
        Some(t.accent)
    } else if sounding {
        Some(t.counter_text)
    } else if paused {
        Some(t.solo)
    } else {
        None
    };
    match face.border {
        // The dark face: the border in the tile's colour, and the selection or playing
        // outline outside it with a gap, so a blue pad's border never reads as selected.
        Some(border) => {
            p.rect_stroke(rect, radius, Stroke::new(1.5_f32, border), StrokeKind::Inside);
            if let Some(c) = state {
                p.rect_stroke(rect.expand(STATE_RING_GAP), radius + STATE_RING_GAP, Stroke::new(2.0_f32, c), StrokeKind::Outside);
            }
            // Recording: a pulsing red ring outside that.
            if let Some(rec) = f.recording {
                let out = STATE_RING_GAP + 2.0 + 1.0;
                p.rect_stroke(rect.expand(out), radius + out, Stroke::new(2.5_f32, theme::with_alpha(t.error, rec.pulse)), StrokeKind::Outside);
            }
        }
        // Coloured pads (D-146): the outline inside the edge, the recording ring outside it.
        None => {
            if let Some(rec) = f.recording {
                p.rect_stroke(rect, radius, Stroke::new(3.0_f32, theme::with_alpha(t.error, rec.pulse)), StrokeKind::Outside);
            }
            let outline = state.map_or(Stroke::new(1.0_f32, Color32::from_rgb(0x0A, 0x0A, 0x0A)), |c| Stroke::new(2.0_f32, c));
            p.rect_stroke(rect, radius, outline, StrokeKind::Inside);
        }
    }
}

/// A soft dark edge round lettering drawn by `draw` (at an offset, in a colour): `draw` at
/// eight 1 px offsets in `halo`, before the lettering itself goes on top. Nothing when
/// `halo` is clear (a coloured pad, whose lettering sits on plates).
fn haloed(halo: Color32, draw: impl Fn(Vec2, Color32)) {
    if halo.a() == 0 {
        return;
    }
    for (dx, dy) in [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0), (-0.7, -0.7), (0.7, -0.7), (-0.7, 0.7), (0.7, 0.7)] {
        draw(vec2(dx, dy), halo);
    }
}

/// How far outside a default pad's coloured border its selection or playing outline
/// sits (D-152).
const STATE_RING_GAP: f32 = 1.5;

fn paint_add_slot(ui: &Ui, rect: Rect, hovered: bool) {
    let t = Tokens::get(ui.ctx());
    let p = ui.painter();
    let color = if hovered { t.text } else { t.text_dim };
    if hovered {
        p.rect_filled(rect, 6.0, theme::with_alpha(t.text, 0.04));
    }
    // Dashed outline.
    dashed_outline(p, rect.shrink(1.0), Stroke::new(1.0_f32, if hovered { t.text_dim } else { t.border_light }));
    // As much of the label as fits a narrow slot.
    let fits = |l: &str| ui.fonts_mut(|f| f.layout_no_wrap(l.to_owned(), theme::regular(13.0), color).size().x) <= rect.width() - 12.0;
    let label = ["✚  Add Sounds", "✚  Add"].into_iter().find(|l| fits(l)).unwrap_or("✚");
    p.text(rect.center(), Align2::CENTER_CENTER, label, theme::regular(13.0), color);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn move_keys_swap_with_blanks_and_fill_them_across_rows() {
        // [0 _ 1] [2 3]; each key's place, applied as move_tile would.
        let g: Grid<usize> = vec![vec![Some(0), None, Some(1)], vec![Some(2), Some(3)]];
        assert_eq!(row_move(&g, 0, Key::ArrowRight), Some(Place::InRow { row: 0, at: 1 }), "past the blank: they swap");
        assert_eq!(row_move(&g, 1, Key::ArrowLeft), Some(Place::InRow { row: 0, at: 1 }));
        assert_eq!(row_move(&g, 3, Key::ArrowUp), Some(Place::Slot { row: 0, at: 1 }), "up into the blank above");
        assert_eq!(row_move(&g, 1, Key::ArrowDown), Some(Place::Slot { row: 1, at: 2 }), "down past the row's end: its end");
        assert_eq!(row_move(&g, 2, Key::ArrowLeft), Some(Place::InRow { row: 0, at: usize::MAX }));
        assert_eq!(row_move(&g, 1, Key::ArrowRight), Some(Place::InRow { row: 1, at: 0 }));
        assert_eq!(row_move(&g, 0, Key::ArrowUp), Some(Place::NewRow(0)));
        let alone: Grid<usize> = vec![vec![Some(0)], vec![Some(1)]];
        assert_eq!((row_move(&alone, 0, Key::ArrowUp), row_move(&alone, 1, Key::ArrowDown)), (None, None));
        // Down from slot 3 into a 1-slot row: its end, no blanks opened (D-190).
        let long: Grid<usize> = vec![vec![Some(0), Some(1), Some(2), Some(3)], vec![Some(4)]];
        assert_eq!(row_move(&long, 3, Key::ArrowDown), Some(Place::Slot { row: 1, at: 1 }));
        assert_eq!(row_move(&long, 3, Key::ArrowRight), Some(Place::InRow { row: 1, at: 0 }), "a row's last pad crosses down");
        let mut s = crate::model::SoundSet { rows: vec![2, 2], blanks: vec![[0, 1]], ..crate::model::SoundSet::named("Keys") };
        s.tiles = (0..4).map(|i| Tile { title: i.to_string(), ..Default::default() }).collect();
        s.move_tile(0, row_move(&s.slots(), 0, Key::ArrowRight).unwrap());
        assert_eq!(s.slots()[0], vec![None, Some(0), Some(1)], "the blank and the tile swapped places");
    }

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
        for c in crate::logic::KEY_ROWS.concat().chars() {
            let found = Key::ALL.iter().any(|k| key_char(*k) == Some(c));
            assert!(found, "no egui key produces {c:?}");
        }
    }
}

#[cfg(test)]
mod glyph_tests {
    use eframe::egui::{self, FontId};

    /// Every symbol the source writes (outside comments) must exist in every font family
    /// the UI installs (theme::install_fonts: Inter, Antonio for the Panel theme, and
    /// their fallbacks), or it renders as a box. Scans all of src/, so new files count.
    #[test]
    fn ui_glyphs_exist_in_installed_fonts() {
        fn rust_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    rust_files(&path, out);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    out.push(path);
                }
            }
        }
        let mut files = Vec::new();
        rust_files(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut files);
        let mut symbols: Vec<(char, String)> = Vec::new();
        for file in &files {
            let source = std::fs::read_to_string(file).unwrap();
            for c in source.lines().filter(|l| !l.trim_start().starts_with("//")).flat_map(|l| l.chars()) {
                if !c.is_ascii() && !c.is_alphabetic() && !symbols.iter().any(|(s, _)| *s == c) {
                    symbols.push((c, file.file_name().unwrap().to_string_lossy().into_owned()));
                }
            }
        }
        assert!(symbols.iter().any(|(c, _)| *c == '▸'), "the scan found no ▸: is it reading src/?");

        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx, &Default::default());
        ctx.run_ui(Default::default(), |_| {}).textures_delta.clear();
        let families = ctx.fonts_mut(|f| f.families());
        assert!(families.len() >= 5, "expected Inter, Antonio and egui's families, got {families:?}");
        let mut missing = Vec::new();
        for family in &families {
            for (c, file) in &symbols {
                if !ctx.fonts_mut(|f| f.has_glyph(&FontId::new(14.0, family.clone()), *c)) {
                    missing.push(format!("{c:?} (U+{:04X}, {file}) in {family:?}", *c as u32));
                }
            }
        }
        assert!(missing.is_empty(), "glyphs missing from the installed fonts:\n{}", missing.join("\n"));
    }
}

/// A bottom-bar key with a label and an arrow on the side it points, which is the way a
/// click moves the panel it shows or hides (D-216).
fn arrow_key(ui: &mut Ui, label: &str, width: f32, on: bool, left: bool) -> egui::Response {
    let resp = theme::key(ui, "", vec2(width, 26.0), on);
    let t = Tokens::get(ui.ctx());
    let ink = if on { t.key_on_text() } else { t.key_text() };
    let p = ui.painter();
    let g = p.layout_no_wrap(label.into(), theme::semibold(12.5), ink);
    let (arrow, gap) = (10.0, 6.0);
    let left_x = resp.rect.center().x - (arrow + gap + g.size().x) / 2.0;
    let cy = resp.rect.center().y;
    let (arrow_x, text_x) = if left { (left_x + arrow / 2.0, left_x + arrow + gap) } else { (left_x + g.size().x + gap + arrow / 2.0, left_x) };
    pad_options::paint_arrow(p, pos2(arrow_x, cy), arrow, left, ink);
    p.galley(pos2(text_x, cy - g.size().y / 2.0), g, ink);
    resp
}

/// A bottom-bar key showing only an icon (Grid, List), drawn in the key's lettering colour.
fn icon_key(ui: &mut Ui, on: bool, paint: fn(&egui::Painter, egui::Pos2, f32, bool, Color32)) -> egui::Response {
    let resp = theme::key(ui, "", vec2(34.0, 26.0), on);
    let t = Tokens::get(ui.ctx());
    let ink = if on { t.key_on_text() } else { t.key_text() };
    // Up a little when on, clear of the lit bar along the bottom.
    let c = resp.rect.center() - vec2(0.0, if on { 1.5 } else { 0.0 });
    paint(ui.painter(), c, 12.0, false, ink);
    resp
}

