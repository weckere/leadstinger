//! Sets in windows of their own (t-m22-multi-window, D-222 to D-226): a set's board in
//! a real OS window (an egui immediate viewport), to put on another monitor or
//! workspace while the main window shows another set.
//!
//! A set window borrows the board for its pass (D-223): while it draws, handles its keys
//! and takes its drops, the library's current set is that window's set, and the
//! per-board state (pad rects, the Rows grid, held keys, drags) is its own copy. So
//! every board function works in it unchanged, in both themes.

use super::*;
use crate::controller::{WindowOp, WindowView};
use crate::model::{SetWindow, SET_WINDOW_MIN};

/// What a board keeps between frames, swapped in for a set window's pass.
pub(super) struct BoardState {
    tile_rects: Vec<(Uuid, usize, Rect)>,
    row_grid: Option<(grid::Layout, Grid<usize>)>,
    file_marker: bool,
    slot_anim: HashMap<egui::Id, Rect>,
    anim_until: Option<Instant>,
    drag_grab: Option<Vec2>,
    focused_tile: Option<Uuid>,
    mouse_held: Option<Uuid>,
    keys_held: HashMap<Key, Uuid>,
    esc_down: Option<(Instant, bool)>,
    board_rect: Rect,
}

impl Default for BoardState {
    fn default() -> Self {
        BoardState {
            tile_rects: Vec::new(),
            row_grid: None,
            file_marker: false,
            slot_anim: HashMap::new(),
            anim_until: None,
            drag_grab: None,
            focused_tile: None,
            mouse_held: None,
            keys_held: HashMap::new(),
            esc_down: None,
            board_rect: Rect::ZERO,
        }
    }
}

/// A set window while it's open.
pub(super) struct SetWin {
    board: BoardState,
    /// Ui passes it has had: a background window opens hidden and is shown on its third.
    frames: u32,
    /// Where it opened and its inside size, in points at the zoom it opened at. Fixed, so
    /// egui never moves it back; `window.move` and the person move it after that.
    pos: Option<Pos2>,
    size: Vec2,
    /// Screenshots asked of it, taken on its next pass (`true` quits afterwards).
    pub(super) shots: Vec<(PathBuf, bool)>,
}

/// A set window's size when nothing says otherwise, in points.
const DEFAULT_SIZE: [f32; 2] = [720.0, 520.0];

impl SetWin {
    fn new(w: &SetWindow, zoom: f32) -> SetWin {
        let size = vec2(w.width.unwrap_or(DEFAULT_SIZE[0] * zoom), w.height.unwrap_or(DEFAULT_SIZE[1] * zoom)) / zoom;
        SetWin { board: BoardState::default(), frames: 0, pos: w.x.zip(w.y).map(|(x, y)| pos2(x, y) / zoom), size, shots: Vec::new() }
    }
}

/// Save this window's pixels to `batch`'s paths once it's painted. eframe 0.37 never
/// answers `request_screenshot` in an immediate viewport (its immediate render path
/// doesn't read the requests), so a paint callback above everything else reads the
/// framebuffer itself, the way eframe does for the main window.
#[cfg(target_os = "ios")]
fn capture(_ctx: &egui::Context, _set: Uuid, _batch: Vec<(PathBuf, bool)>, _tx: ShotSender) {
    // iPadOS has one window (no set windows), and its renderer is wgpu, not glow.
}

#[cfg(not(target_os = "ios"))]
fn capture(ctx: &egui::Context, set: Uuid, batch: Vec<(PathBuf, bool)>, tx: ShotSender) {
    let batch = std::sync::Mutex::new(Some(batch));
    let wake = ctx.clone();
    let read = eframe::egui_glow::CallbackFn::new(move |info, painter| {
        let Some(batch) = batch.lock().ok().and_then(|mut b| b.take()) else { return };
        tx.send((batch, Arc::new(painter.read_screen_rgba(info.screen_size_px)))).ok();
        wake.request_repaint_of(egui::ViewportId::ROOT);
    });
    let layer = egui::LayerId::new(egui::Order::Debug, egui::Id::unique(("set-window-shot", set)));
    ctx.layer_painter(layer).add(egui::PaintCallback { rect: ctx.content_rect(), callback: Arc::new(read) });
}

pub(super) fn viewport_id(set: Uuid) -> egui::ViewportId {
    egui::ViewportId::from_hash_of(("set-window", set))
}

/// The window's title: the set's name, then the app's.
pub(super) fn title(name: &str) -> String {
    format!("{name} · {APP_NAME}")
}

impl StingerApp {
    /// Trade the board's state for a set window's (and back again).
    fn swap_board(&mut self, b: &mut BoardState) {
        std::mem::swap(&mut self.tile_rects, &mut b.tile_rects);
        std::mem::swap(&mut self.row_grid, &mut b.row_grid);
        std::mem::swap(&mut self.file_marker, &mut b.file_marker);
        std::mem::swap(&mut self.slot_anim, &mut b.slot_anim);
        std::mem::swap(&mut self.anim_until, &mut b.anim_until);
        std::mem::swap(&mut self.drag_grab, &mut b.drag_grab);
        std::mem::swap(&mut self.focused_tile, &mut b.focused_tile);
        std::mem::swap(&mut self.mouse_held, &mut b.mouse_held);
        std::mem::swap(&mut self.keys_held, &mut b.keys_held);
        std::mem::swap(&mut self.esc_down, &mut b.esc_down);
        std::mem::swap(&mut self.board_rect, &mut b.board_rect);
    }

    /// Forget held keys and pads in every set window too (`Effect::ReleaseHeld`).
    pub(super) fn release_held_in_windows(&mut self) {
        for w in self.set_wins.values_mut() {
            w.board.mouse_held = None;
            w.board.keys_held.clear();
        }
    }

    /// Show every set window the library has open, after the main window's pass, and
    /// note where each window is for `window.list`.
    pub(super) fn set_windows(&mut self, ctx: &egui::Context) {
        let zoom = ctx.zoom_factor();
        let (outer, inner, focused) = ctx.input(|i| (i.viewport().outer_rect, i.viewport().inner_rect, i.viewport().focused));
        let mut views = vec![WindowView {
            set: None,
            title: APP_NAME.to_owned(),
            pos: outer.map(|r| [r.min.x * zoom, r.min.y * zoom]),
            size: Some(inner.map_or_else(|| ctx.content_rect().size(), |r| r.size()) * zoom).map(|s| [s.x, s.y]),
            focused: focused.unwrap_or(false),
        }];
        let open = self.ctl.store.library.set_windows.clone();
        self.set_wins.retain(|id, _| open.iter().any(|w| w.set == *id));
        for w in &open {
            let Some(si) = self.ctl.store.library.set_index(w.set) else { continue };
            let title = title(&self.ctl.store.library.sets[si].name);
            let mut win = self.set_wins.remove(&w.set).unwrap_or_else(|| SetWin::new(w, zoom));
            // In the background (D-214) it opens hidden, is made see-through and
            // click-through like the main window, and only then is shown.
            let hidden = self.opts.background && win.frames < 2;
            if self.opts.background && win.frames == 1 {
                crate::background::hide_windows();
            }
            let mut builder = crate::titlebar::integrate(egui::ViewportBuilder::default())
                .with_title(&title)
                .with_app_id(crate::brand::SLUG)
                .with_inner_size(win.size)
                .with_min_inner_size(SET_WINDOW_MIN)
                .with_drag_and_drop(true)
                .with_active(!self.opts.background)
                .with_visible(!hidden)
                .with_icon(self.window_icon.clone());
            if let Some(pos) = win.pos {
                builder = builder.with_position(pos);
            }
            let view = ctx.show_viewport_immediate(viewport_id(w.set), builder, |ui, class| self.set_window_ui(ui, w.set, class, &mut win));
            let mut view = view;
            view.title = title;
            views.push(view);
            self.set_wins.insert(w.set, win);
        }
        self.ctl.windows = views;
    }

    /// One set window's pass: its bar and board, its keys and drops, its screenshots,
    /// and where it is now (kept in the library).
    fn set_window_ui(&mut self, ui: &mut Ui, set: Uuid, class: egui::ViewportClass, win: &mut SetWin) -> WindowView {
        let ctx = ui.ctx().clone();
        // Embedded (the tests' harness has one window): drawn inside the main window,
        // whose keys, drops and place are the main window's own.
        let own = class != egui::ViewportClass::EmbeddedWindow;
        win.frames += 1;
        let zoom = ctx.zoom_factor();
        let (close, outer, inner, focused) = ctx.input(|i| {
            let v = i.viewport();
            (v.close_requested(), v.outer_rect, v.inner_rect, v.focused.unwrap_or(false))
        });
        if own && self.opts.background {
            ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaintWhileHidden);
        }

        // Lend it the board (D-223).
        let main_set = self.ctl.store.library.selected_set;
        self.ctl.store.library.selected_set = Some(set);
        let search = self.ctl.search.take();
        self.swap_board(&mut win.board);
        let mut actions = Vec::new();
        if own {
            self.handle_move_keys(&ctx, &mut actions);
            self.handle_shortcuts(&ctx, &mut actions);
            self.handle_hotkeys(&ctx, &mut actions);
            self.handle_file_drop(&ctx);
            self.handle_undo_keys(&ctx);
        }
        self.set_window_panels(ui, set, &mut actions);
        if let Some(id) = self.mouse_held {
            if !ctx.input(|i| i.pointer.primary_down()) {
                self.mouse_held = None;
                actions.push(inv!("tile.trigger", {"tile": id, "event": "release"}).into());
            }
        }
        for a in actions {
            self.apply(a);
        }
        self.swap_board(&mut win.board);
        if self.ctl.search.is_none() {
            self.ctl.search = search;
        }
        // Back to the main window's set, unless something in this one chose another.
        if self.ctl.store.library.selected_set == Some(set) {
            self.ctl.store.library.selected_set = main_set;
        }

        if own && !win.shots.is_empty() {
            self.shots_pending += 1;
            capture(&ctx, set, std::mem::take(&mut win.shots), self.shot_tx.clone());
        }

        let pos = outer.map(|r| [r.min.x * zoom, r.min.y * zoom]);
        let size = inner.map_or_else(|| ctx.content_rect().size(), |r| r.size()) * zoom;
        if own {
            let lib = &mut self.ctl.store.library;
            if close {
                lib.set_windows.retain(|w| w.set != set);
                self.ctl.store.mark_dirty();
            } else if win.frames > 2 {
                // Keep where it is, once it has settled where it opened.
                if let Some(w) = lib.set_windows.iter_mut().find(|w| w.set == set) {
                    let near = |a: Option<f32>, b: f32| a.is_some_and(|a| (a - b).abs() < 0.5);
                    let mut moved = !near(w.width, size.x) || !near(w.height, size.y);
                    (w.width, w.height) = (Some(size.x.round()), Some(size.y.round()));
                    if let Some([x, y]) = pos {
                        moved |= !near(w.x, x) || !near(w.y, y);
                        (w.x, w.y) = (Some(x.round()), Some(y.round()));
                    }
                    if moved {
                        self.ctl.store.mark_unsaved();
                    }
                }
            }
        }
        WindowView { set: Some(set), title: String::new(), pos: pos.filter(|_| own), size: Some([size.x, size.y]), focused: own && focused }
    }

    /// A set window's layout: a slim bar, then the board.
    fn set_window_panels(&mut self, ui: &mut Ui, set: Uuid, actions: &mut Vec<Action>) {
        let t = Tokens::get(ui.ctx());
        let panel = egui::Frame::new().fill(t.panel).inner_margin(egui::Margin { left: 12, right: 12, top: 6, bottom: 6 }).stroke(Stroke::new(1.0_f32, t.border));
        egui::Panel::top(egui::Id::unique(("set-window-bar", set)))
            .frame(panel)
            .show_separator_line(false)
            .show(ui, |ui| {
                theme::name_panel(ui, egui::accesskit::Role::Toolbar, "Set window bar");
                crate::titlebar::drag_area(ui, ui.max_rect().expand2(vec2(12.0, 6.0)));
                self.set_window_bar(ui, actions)
            });
        self.board_rect = ui.available_rect_before_wrap();
        egui::CentralPanel::default().frame(egui::Frame::new().fill(t.window).inner_margin(egui::Margin::same(16))).show(ui, |ui| {
            theme::name_panel(ui, egui::accesskit::Role::Region, "Board");
            match self.ctl.current_set().layout {
                Layout::Grid => self.grid(ui, actions),
                Layout::List => self.list(ui, actions),
            }
        });
    }

    /// The set's name, its lock and what's playing in it; at the right Stop All, the
    /// transport keys and Grid | Cue List.
    fn set_window_bar(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let t = Tokens::get(ui.ctx());
        let set = self.ctl.current_set();
        let (name, locked, layout) = (set.name.clone(), set.locked, set.layout);
        let playing = set.tiles.iter().filter(|tile| self.ctl.statuses.contains_key(&tile.id)).count();
        ui.horizontal(|ui| {
            ui.set_height(34.0);
            ui.spacing_mut().item_spacing.x = 8.0;
            // The window's top strip on macOS: its name (the hidden title) right of the
            // traffic lights (D-228).
            ui.add_space((crate::titlebar::inset(ui.ctx()).x - 12.0).max(0.0));
            ui.label(RichText::new(&name).font(theme::semibold(15.0)).color(t.text));
            if locked {
                let (rect, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                pad_options::paint_lock(ui.painter(), rect.center(), 14.0, true, t.text_dim);
            }
            if playing > 0 {
                ui.label(RichText::new(format!("{playing} playing")).color(t.text_dim));
            }
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let stop = theme::key_with(ui, "⏹  Stop All", vec2(112.0, 30.0), t.stop, theme::inks(ui.ctx()).stop, theme::semibold(13.0));
                if theme::hint(theme::role(stop, AxRole::Button, "Stop All", None), tips::stop_all(self.ctl.store.library.stop_all_hold)).clicked() {
                    actions.push(inv!("transport.stopAll").into());
                }
                if ui.available_width() > 320.0 {
                    self.transport_keys(ui, actions);
                }
                ui.add_space(4.0);
                let key = theme::role(icon_key(ui, layout == Layout::List, pad_options::paint_list_icon), AxRole::Button, "Cue list", Some(layout == Layout::List));
                if theme::hint_keys(key, tips::LIST.0, tips::LIST.1).clicked() && layout != Layout::List {
                    actions.push(inv!("set.layout", {"layout": "list"}).into());
                }
                let key = theme::role(icon_key(ui, layout == Layout::Grid, pad_options::paint_grid_icon), AxRole::Button, "Grid", Some(layout == Layout::Grid));
                if theme::hint_keys(key, tips::GRID.0, tips::GRID.1).clicked() && layout != Layout::Grid {
                    actions.push(inv!("set.layout", {"layout": "grid"}).into());
                }
            });
        });
    }

    /// `window.focus`, `window.move`, `window.gather` (D-224).
    pub(super) fn window_effect(&mut self, ctx: &egui::Context, window: Option<Uuid>, op: WindowOp) {
        let zoom = ctx.zoom_factor();
        let id = window.map_or(egui::ViewportId::ROOT, viewport_id);
        let view = |w: Option<Uuid>| self.ctl.windows.iter().find(|v| v.set == w).cloned();
        match op {
            WindowOp::Focus => {
                ctx.send_viewport_cmd_to(id, egui::ViewportCommand::Minimized(false));
                if !self.opts.background {
                    ctx.send_viewport_cmd_to(id, egui::ViewportCommand::Focus);
                }
            }
            WindowOp::Place { pos, size } => {
                if let Some([x, y]) = pos {
                    ctx.send_viewport_cmd_to(id, egui::ViewportCommand::OuterPosition(pos2(x, y) / zoom));
                }
                if let Some([w, h]) = size {
                    // A side given as 0 keeps the size it has.
                    let now = view(window).and_then(|v| v.size).unwrap_or([w, h]);
                    let size = vec2(if w > 0.0 { w } else { now[0] }, if h > 0.0 { h } else { now[1] });
                    ctx.send_viewport_cmd_to(id, egui::ViewportCommand::InnerSize(size / zoom));
                }
            }
            WindowOp::Gather => {
                let [x, y] = view(None).and_then(|v| v.pos).unwrap_or([40.0, 40.0]);
                for (i, w) in self.ctl.store.library.set_windows.iter().enumerate() {
                    let step = 32.0 * i as f32;
                    let to = pos2(x + 48.0 + step, y + 64.0 + step);
                    ctx.send_viewport_cmd_to(viewport_id(w.set), egui::ViewportCommand::OuterPosition(to / zoom));
                    if !self.opts.background {
                        ctx.send_viewport_cmd_to(viewport_id(w.set), egui::ViewportCommand::Focus);
                    }
                }
            }
        }
    }
}
