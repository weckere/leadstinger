//! The full-window tile editor (`view.editor`, D-096): the tile's waveform across the
//! board's space, zoomable from the whole file down to single samples, with its trim and
//! vamp handles, markers, a cursor and exact timecode fields.
//!
//! The waveform comes from a min/max [`PeakPyramid`] built when the editor opens, so any
//! zoom draws in about one lookup per column; below one frame per point it draws the
//! samples themselves. ⌘/Ctrl+scroll or a pinch zooms around the pointer, scrolling or
//! dragging the ruler pans, dragging the waveform selects a stretch (D-104), and every
//! handle is a slider that takes the arrow keys.

use super::*;
use crate::decode::DecodedAudio;
use crate::logic::{format_samples, format_timecode, parse_timecode, vamp_range};
use crate::controller::EditorZoom;
use crate::waveform::{nearest_zero_crossing, PeakPyramid};

pub const OPEN_TIP: &str =
    "Open this tile in the full-window waveform editor: zoom in to trim, vamp and place markers to the sample (⌘/Ctrl+Shift+E)";
pub const CLOSE_TIP: &str = "Close the waveform editor and show the board again (⌘/Ctrl+Shift+E)";

/// Deepest zoom: points per frame, so 1/32 frame per point.
const MIN_FPP: f64 = 1.0 / 32.0;
/// Below this many frames per point the samples themselves are drawn.
const RAW_FPP: f64 = 1.0;
/// Below this, each sample gets a dot as well.
const DOT_FPP: f64 = 1.0 / 6.0;
/// The time ruler along the top of the waveform.
const RULER_H: f32 = 18.0;
/// One zoom step (buttons and the + and − keys).
const ZOOM_STEP: f64 = 2.0;

const TIME_TIP: &str = "Type a time: 1:02.500, 62.5 (seconds), 250ms or 12345smp (samples); Enter applies it, Esc puts it back";
const NO_SELECTION: &str = "Drag across the waveform to select a stretch first, or type its start and end";
/// The time rows' first column.
const LABEL_W: f32 = 110.0;

/// What part of the file the editor shows: the frame at its left edge and how many
/// frames each point covers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub start: f64,
    pub fpp: f64,
}

impl View {
    /// The whole file across `width` points.
    pub fn fit(frames: usize, width: f32) -> View {
        View { start: 0.0, fpp: (frames.max(1) as f64 / f64::from(width.max(1.0))).max(MIN_FPP) }
    }

    /// Frames `from..to` across `width` points, clamped.
    pub fn span(from: f64, to: f64, frames: usize, width: f32) -> View {
        let fpp = (to - from).max(1.0) / f64::from(width.max(1.0));
        View { start: from, fpp }.clamp(frames, width)
    }

    /// Kept inside the file: no further out than the whole file, no deeper than
    /// [`MIN_FPP`], and never scrolled past either end.
    pub fn clamp(self, frames: usize, width: f32) -> View {
        let width = f64::from(width.max(1.0));
        let most = (frames.max(1) as f64 / width).max(MIN_FPP);
        let fpp = if self.fpp.is_finite() { self.fpp.clamp(MIN_FPP, most) } else { most };
        let last = (frames as f64 - width * fpp).max(0.0);
        let start = if self.start.is_finite() { self.start.clamp(0.0, last) } else { 0.0 };
        View { start, fpp }
    }

    /// Zoomed in by `factor` (out below 1), keeping the frame under `x` where it is.
    pub fn zoom(self, factor: f64, x: f32, frames: usize, width: f32) -> View {
        let anchor = self.frame_at(x);
        let fpp = self.fpp / factor.max(1e-6);
        View { start: anchor - f64::from(x) * fpp, fpp }.clamp(frames, width)
    }

    /// Scrolled by `points` (positive moves later in the file).
    pub fn pan(self, points: f64, frames: usize, width: f32) -> View {
        View { start: self.start + points * self.fpp, ..self }.clamp(frames, width)
    }

    /// The frame at `x` points from the left edge.
    pub fn frame_at(self, x: f32) -> f64 {
        self.start + f64::from(x) * self.fpp
    }

    /// Points from the left edge to `frame`.
    pub fn x_of(self, frame: f64) -> f32 {
        ((frame - self.start) / self.fpp) as f32
    }
}

/// The ruler's tick spacing in seconds: the first round step at least `min_gap`
/// points apart.
pub fn tick_step(seconds_per_point: f64, min_gap: f64) -> f64 {
    const STEPS: &[f64] = &[
        0.0001, 0.0002, 0.0005, 0.001, 0.002, 0.005, 0.01, 0.02, 0.05, 0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0,
        600.0, 1800.0, 3600.0,
    ];
    STEPS.iter().copied().find(|s| s / seconds_per_point >= min_gap).unwrap_or(3600.0)
}

/// `sec` moved to the nearest zero crossing within `radius` frames, if there is one.
pub fn snap(audio: &DecodedAudio, sec: f64, radius: usize) -> f64 {
    let rate = f64::from(audio.sample_rate.max(1));
    let frame = (sec * rate).round().max(0.0) as usize;
    nearest_zero_crossing(&audio.data, frame, radius).map_or(sec, |f| f as f64 / rate)
}

/// The editor's state while it's open: which audio, its pyramid, the view, snapping and
/// the cursor.
pub struct EditorView {
    tile: Uuid,
    audio: Arc<DecodedAudio>,
    pyramid: PeakPyramid,
    /// `None` until the first frame knows how wide the waveform is.
    view: Option<View>,
    pub snap: bool,
    /// Seconds into the file: where a click put it.
    pub cursor: Option<f64>,
    /// The waveform's width last frame, in points.
    width: f32,
    /// A drag on the waveform: where it started, in seconds, while it selects.
    selecting: Option<f64>,
}

impl EditorView {
    fn new(tile: Uuid, audio: Arc<DecodedAudio>) -> Self {
        let pyramid = PeakPyramid::new(&audio.data);
        EditorView { tile, audio, pyramid, view: None, snap: true, cursor: None, width: 1.0, selecting: None }
    }

    /// Zoom in `doublings` times around the middle (a script step, for screenshots).
    pub fn zoom_in(&mut self, doublings: u32) {
        let (frames, width) = (self.pyramid.frames(), self.width);
        if let Some(v) = self.view.as_mut() {
            *v = v.zoom(f64::from(2u32.pow(doublings.min(20))), width / 2.0, frames, width);
        }
    }

    #[cfg(test)]
    pub fn view(&self) -> Option<View> {
        self.view
    }
}

/// The stretch between the nearest marks either side of `sec` (the file's ends, the trim,
/// the vamp's edges and the markers): what a double-click selects.
pub fn between_marks(marks: &[f64], sec: f64, dur: f64) -> (f64, f64) {
    let from = marks.iter().copied().filter(|&m| m <= sec).fold(0.0, f64::max);
    let to = marks.iter().copied().filter(|&m| m > sec).fold(dur, f64::min);
    (from, to)
}

/// A draggable, focusable mark on the waveform: a trim or vamp handle, or a marker.
#[derive(Clone, Copy)]
enum Mark {
    Handle(Handle),
    Marker(usize),
}

impl StingerApp {
    pub(super) fn editor(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let Some(id) = self.ctl.editor else { return };
        let Some(orig) = self.ctl.store.library.tile(id).cloned() else {
            actions.push(inv!("view.editor", {"open": false}).into());
            return;
        };
        let mut t = orig.clone();
        let playing = self.ctl.statuses.contains_key(&id);
        // A locked set's tile is read-only here (D-152): it plays, previews and can be
        // looked through, but nothing that would change it responds.
        let locked = self.tile_locked(id);
        let mut ev = self.editor_view.take().filter(|v| v.tile == id);
        let audio = self.ctl.store.audio(&t);
        if let Some(a) = &audio {
            if !ev.as_ref().is_some_and(|v| Arc::ptr_eq(&v.audio, a)) {
                let (snap, cursor) = ev.as_ref().map_or((true, None), |v| (v.snap, v.cursor));
                ev = Some(EditorView { snap, cursor, ..EditorView::new(id, a.clone()) });
            }
        }

        // Header: what's open, and the view's controls.
        let selection = self.ctl.editor_selection;
        // The controls sit right of the title, or wrap on a row of their own when the board's
        // space is narrow (a tiled window at 720 px leaves under 300 points).
        let narrow = ui.available_width() < 640.0;
        let mut controls = |ui: &mut Ui| {
            if theme::hint(theme::small_button(ui, "Close Editor"), CLOSE_TIP).clicked() {
                actions.push(inv!("view.editor", {"open": false}).into());
            }
            let preview = theme::name(ui.add_enabled(t.file.is_some(), theme::small(egui::Button::new(if playing { "⏹ Stop" } else { "▶ Preview" }))), if playing { "Stop" } else { "Preview" });
            let tip = if playing { "Stop this tile now" } else { "Play this tile, on the Preview output if one is set in Settings" };
            if theme::hint_or(preview, tip, "This tile has no sound yet").clicked() {
                actions.push(inv!("tile.preview", {"tile": id}).into());
            }
            let Some(ev) = ev.as_mut() else { return };
            ui.separator();
            if theme::hint(theme::small_button(ui, "Fit"), "Zoom out to the whole file").clicked() {
                actions.push(inv!("view.editorZoom", {"fit": true}).into());
            }
            if theme::hint(theme::small_button(ui, "Trim"), "Zoom to the part that plays, from the start handle to the end handle").clicked() {
                actions.push(inv!("view.editorZoom", {"trim": true}).into());
            }
            let zsel = ui.add_enabled(selection.is_some(), theme::small(egui::Button::new("Selection")));
            if theme::hint_or(theme::name(zsel, "Zoom to selection"), "Zoom to the selected stretch", NO_SELECTION).clicked() {
                actions.push(inv!("view.editorZoom", {"selection": true}).into());
            }
            let zin = theme::name(theme::small_button(ui, "+"), "Zoom in");
            if theme::hint(zin, "Zoom in on the cursor, or the middle (or ⌘/Ctrl+scroll, pinch, or + with the waveform focused)").clicked() {
                actions.push(inv!("view.editorZoom", {"by": ZOOM_STEP}).into());
            }
            let zout = theme::name(theme::small_button(ui, "−"), "Zoom out");
            if theme::hint(zout, "Zoom out (or ⌘/Ctrl+scroll, pinch, or − with the waveform focused)").clicked() {
                actions.push(inv!("view.editorZoom", {"by": 1.0 / ZOOM_STEP}).into());
            }
            let snap = ui.add_enabled_ui(!locked, |ui| theme::checkbox(ui, &mut ev.snap, "Snap")).inner;
            theme::hint_or(snap, "Handles and markers land on the nearest zero crossing, so cuts don't click (Alt with the arrow keys moves one sample, unsnapped)", tips::LOCKED_WHY);
        };
        ui.horizontal(|ui| {
            // Not .strong(): the Panel theme draws strong text black, for its pills.
            ui.label(RichText::new("EDITOR").small().color(ui.visuals().weak_text_color()));
            ui.label(RichText::new(&t.title).color(ui.visuals().text_color()).size(16.0));
            if !narrow {
                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| controls(ui));
            }
        });
        if narrow {
            ui.horizontal_wrapped(|ui| controls(ui));
        }
        if locked {
            self.editor_locked_banner(ui, id, actions);
        }

        let (Some(mut ev), Some(audio)) = (ev, audio) else {
            let msg = match self.ctl.store.media(&t) {
                Some(Media::Failed(e)) => format!("Can't use this file: {e}"),
                _ if t.file.is_none() => "This tile has no sound".to_owned(),
                _ => "Loading waveform…".to_owned(),
            };
            ui.add_space(24.0);
            ui.vertical_centered(|ui| ui.weak(msg));
            return;
        };
        let rate = audio.sample_rate.max(1);
        let ratef = f64::from(rate);
        let frames = audio.frames();
        let dur = audio.duration().max(1e-6);

        // The waveform: as tall as the space leaves after the fields below.
        let height = (ui.available_height() - 250.0).max(140.0);
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click_and_drag());
        let width = rect.width().max(1.0);
        // A new width (a panel resized, the window zoomed) keeps the same stretch in view.
        let mut view = ev.view.map_or_else(
            || View::fit(frames, width),
            |v| View { fpp: v.fpp * f64::from(ev.width / width), ..v }.clamp(frames, width),
        );
        let (s, e) = play_range(&t, dur);
        let centre = |view: View, cursor: Option<f64>| {
            cursor.map(|c| view.x_of(c * ratef)).filter(|x| (0.0..=width).contains(x)).unwrap_or(width / 2.0)
        };
        // What view.editorZoom asked for (the buttons above, or the CLI).
        if let Some(z) = self.ctl.editor_zoom.take() {
            view = match z {
                EditorZoom::Fit => View::fit(frames, width),
                EditorZoom::Span(a, b) => View::span(a * ratef, b * ratef, frames, width),
                EditorZoom::By(by) => view.zoom(by, centre(view, ev.cursor), frames, width),
            };
        }

        // Pointer: ⌘/Ctrl+scroll and pinch zoom anywhere over the waveform (its handles
        // too), scrolling and dragging the ruler pan, dragging the waveform selects, a
        // click places the cursor, Shift+click extends the selection, a double-click
        // selects between the marks either side.
        if ui.rect_contains_pointer(rect) {
            let (z, scroll) = ui.input(|i| (i.zoom_delta(), i.smooth_scroll_delta));
            let x = ui.input(|i| i.pointer.hover_pos()).map_or(width / 2.0, |p| p.x - rect.left());
            if z != 1.0 {
                view = view.zoom(f64::from(z), x, frames, width);
            }
            if scroll != egui::Vec2::ZERO {
                view = view.pan(-f64::from(scroll.x + scroll.y), frames, width);
                ui.input_mut(|i| i.smooth_scroll_delta = egui::Vec2::ZERO);
            }
        }
        let sec_at = |view: View, x: f32| (view.frame_at(x - rect.left()) / ratef).clamp(0.0, dur);
        let snap_on = ev.snap;
        let snapped = |view: View, sec: f64| if snap_on { snap(&audio, sec, (6.0 * view.fpp).max(1.0) as usize) } else { sec };
        let on_ruler = |pos: Pos2| pos.y < rect.top() + RULER_H;
        if resp.drag_started() {
            let origin = ui.input(|i| i.pointer.press_origin());
            ev.selecting = origin.filter(|&p| !on_ruler(p)).map(|p| snapped(view, sec_at(view, p.x)));
        }
        let mut live = None;
        if resp.dragged() {
            match (ev.selecting, resp.interact_pointer_pos()) {
                (Some(a), Some(pos)) => live = Some((a, sec_at(view, pos.x))),
                _ => view = view.pan(-f64::from(resp.drag_delta().x), frames, width),
            }
        }
        if resp.drag_stopped() {
            if let (Some(a), Some(pos)) = (ev.selecting.take(), resp.interact_pointer_pos()) {
                let b = snapped(view, sec_at(view, pos.x));
                // A drag shorter than a couple of points is a click that wobbled.
                if (view.x_of(a * ratef) - view.x_of(b * ratef)).abs() >= 2.0 && a != b {
                    actions.push(inv!("view.editorSelect", {"from": a.min(b), "to": a.max(b)}).into());
                }
            }
        }
        if resp.double_clicked() {
            if let Some(pos) = resp.interact_pointer_pos() {
                let mut at = vec![s, e];
                if let Some((a, b)) = vamp_range(&t, dur) {
                    at.extend([a, b]);
                }
                at.extend(t.markers.iter().map(|m| m.at));
                let (a, b) = between_marks(&at, sec_at(view, pos.x), dur);
                if b > a {
                    actions.push(inv!("view.editorSelect", {"from": a, "to": b}).into());
                }
            }
        } else if resp.clicked() {
            if let Some(pos) = resp.interact_pointer_pos() {
                let sec = snapped(view, sec_at(view, pos.x));
                // Shift: from the far end of the selection, or the cursor, to here.
                let anchor = ui.input(|i| i.modifiers.shift).then(|| match selection {
                    Some((a, b)) => if (sec - a).abs() > (sec - b).abs() { a } else { b },
                    None => ev.cursor.unwrap_or(sec),
                });
                match anchor.filter(|&a| a != sec) {
                    Some(a) => actions.push(inv!("view.editorSelect", {"from": a.min(sec), "to": a.max(sec)}).into()),
                    None => {
                        ev.cursor = Some(sec);
                        if selection.is_some() {
                            actions.push(inv!("view.editorSelect", {"clear": true}).into());
                        }
                    }
                }
            }
        }
        // While a drag selects, it shows where it's going; the selection follows once it ends.
        let shown_sel = live.map(|(a, b): (f64, f64)| (a.min(b), a.max(b))).or(selection);
        // Keys, with the waveform focused: arrows scroll, + and − zoom, Home and End.
        let shown = |v: View| {
            let a = format_timecode(v.start / ratef);
            let b = format_timecode((v.frame_at(width) / ratef).min(dur));
            format!("Showing {a} to {b} of {}", format_timecode(dur))
        };
        let (resp, steps) = theme::slider_info(resp, "Waveform view", view.start / ratef, shown(view));
        let resp = theme::hint(
            resp,
            "The tile's waveform. Click to place the cursor; drag across it to select a stretch (Shift+click extends it, a double-click selects between marks); scroll or drag the ruler to move along it; ⌘/Ctrl+scroll or pinch to zoom. Focused: Left/Right scroll, + and − zoom, Home and End go to the ends",
        );
        if steps != 0 {
            view = view.pan(f64::from(width) * 0.1 * f64::from(steps), frames, width);
        }
        // Tile keys leave it − and = (`handle_hotkeys`).
        self.editor_wave = Some(resp.id);
        if resp.has_focus() {
            theme::lock_arrows(&resp);
            let (left, right, zin, zout, home, end) = ui.input_mut(|i| {
                let n = Modifiers::NONE;
                (
                    i.consume_key(n, Key::ArrowLeft),
                    i.consume_key(n, Key::ArrowRight),
                    i.consume_key(n, Key::Plus) || i.consume_key(n, Key::Equals),
                    i.consume_key(n, Key::Minus),
                    i.consume_key(n, Key::Home),
                    i.consume_key(n, Key::End),
                )
            });
            if left || right {
                view = view.pan(f64::from(width) * if right { 0.1 } else { -0.1 }, frames, width);
            }
            if zin || zout {
                view = view.zoom(if zin { ZOOM_STEP } else { 1.0 / ZOOM_STEP }, centre(view, ev.cursor), frames, width);
            }
            if home {
                view.start = 0.0;
            }
            if end {
                view = View { start: frames as f64, ..view }.clamp(frames, width);
            }
        }

        self.paint_editor_wave(ui, rect, &t, &audio, &ev.pyramid, view, ev.cursor, shown_sel, locked);

        // The marks: drawn above, and controls of their own.
        let x_of = |sec: f64| rect.left() + view.x_of(sec * ratef);
        let mut marks: Vec<(Mark, f64)> = vec![(Mark::Handle(Handle::Start), s), (Mark::Handle(Handle::End), e)];
        if let Some((a, b)) = vamp_range(&t, dur) {
            marks.extend([(Mark::Handle(Handle::VampStart), a), (Mark::Handle(Handle::VampEnd), b)]);
        }
        marks.extend(t.markers.iter().enumerate().map(|(i, m)| (Mark::Marker(i), m.at)));
        // The marks' controls, in a child that a lock disables: greyed for screen readers
        // too, and out of the Tab order.
        let mut marks_ui = ui.new_child(egui::UiBuilder::new().id_salt("editor-marks").max_rect(rect));
        if locked {
            marks_ui.disable();
        }
        for (mark, at) in marks {
            // Off screen it's still a Tab stop, at the nearer edge.
            let x = x_of(at).clamp(rect.left(), rect.right());
            let strip = match mark {
                Mark::Marker(_) => Rect::from_center_size(pos2(x, rect.bottom() - 9.0), vec2(14.0, 18.0)),
                Mark::Handle(_) => Rect::from_x_y_ranges(x - 5.0..=x + 5.0, rect.top() + RULER_H..=rect.bottom()),
            };
            let hr = marks_ui.interact(strip, resp.id.with(mark_key(mark)), Sense::click_and_drag());
            let (mut name, tip) = match mark {
                Mark::Handle(h) => (h.name().to_owned(), editor_tip(h.name())),
                Mark::Marker(i) => (format!("Marker {}", t.markers[i].name), editor_tip(&format!("The marker {}", t.markers[i].name))),
            };
            if locked {
                name.push_str(", read-only");
            }
            let (hr, steps) = theme::slider_info(hr, &name, at, format!("{}, sample {}", format_timecode(at), (at * ratef).round() as u64));
            let hr = theme::hint_or(hr, tip, tips::LOCKED_WHY);
            if locked {
                continue;
            }
            let mut to = None;
            let mut free = false;
            if let (true, Some(pos)) = (hr.dragged(), hr.interact_pointer_pos()) {
                let sec = (view.frame_at(pos.x - rect.left()) / ratef).clamp(0.0, dur);
                to = Some(if ev.snap { snap(&audio, sec, (6.0 * view.fpp).max(1.0) as usize) } else { sec });
                free = true;
            }
            if hr.has_focus() {
                theme::lock_arrows(&hr);
                let m = ui.input(|i| i.modifiers);
                let (left, right) = ui.input_mut(|i| (i.consume_key(m, Key::ArrowLeft), i.consume_key(m, Key::ArrowRight)));
                if left || right {
                    let dir = if right { 1.0 } else { -1.0 };
                    if m.alt {
                        to = Some(at + dir / ratef);
                        free = true;
                    } else {
                        let step = if m.shift { 0.1 } else { 0.01 };
                        to = Some(at + dir * step);
                        if ev.snap {
                            to = to.map(|sec| snap(&audio, sec, (step * ratef / 2.0) as usize));
                        }
                        free = true;
                    }
                }
            }
            if steps != 0 {
                to = Some(at + 0.01 * f64::from(steps));
                free = true;
            }
            if let (true, Some(sec)) = (free, to) {
                let sec = sec.clamp(0.0, dur);
                match mark {
                    Mark::Handle(h) => h.set(&mut t, sec, dur),
                    Mark::Marker(i) => {
                        // A marker isn't part of tile.set: move it once the drag ends, and
                        // show where it's going meanwhile.
                        if hr.dragged() {
                            ui.ctx().data_mut(|d| d.insert_temp(hr.id, sec));
                        } else {
                            actions.push(inv!("tile.markerMove", {"tile": id, "marker": t.markers[i].name, "at": sec}).into());
                        }
                    }
                }
            }
            if let Mark::Marker(i) = mark {
                if hr.drag_stopped() {
                    if let Some(sec) = ui.ctx().data_mut(|d| d.remove_temp::<f64>(hr.id)) {
                        actions.push(inv!("tile.markerMove", {"tile": id, "marker": t.markers[i].name, "at": sec}).into());
                    }
                }
                if let Some(sec) = ui.ctx().data(|d| d.get_temp::<f64>(hr.id)) {
                    let gx = x_of(sec);
                    ui.painter_at(rect).line_segment([pos2(gx, rect.top()), pos2(gx, rect.bottom())], Stroke::new(1.0_f32, theme::with_alpha(Tokens::get(ui.ctx()).marker, 0.6)));
                }
            }
            if hr.hovered() || hr.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
        }
        if resp.dragged() {
            ui.ctx().set_cursor_icon(if ev.selecting.is_some() { egui::CursorIcon::Text } else { egui::CursorIcon::Grabbing });
        } else if resp.hovered() {
            let ruler = ui.input(|i| i.pointer.hover_pos()).is_some_and(on_ruler);
            ui.ctx().set_cursor_icon(if ruler { egui::CursorIcon::Grab } else { egui::CursorIcon::Text });
        }
        ev.view = Some(view);
        ev.width = width;

        // Below: where the view is, then exact times, the selection, the vamp and the markers.
        let (s, e) = play_range(&t, dur);
        ui.horizontal_wrapped(|ui| {
            ui.weak(shown(view));
            let zoom = if view.fpp >= 1.0 { format!("{:.0} samples per point", view.fpp) } else { format!("{:.0} points per sample", 1.0 / view.fpp) };
            ui.weak(format!("· {zoom} · {} Hz", rate));
            ui.weak(format!("· plays {}", format_timecode(e - s)));
            if let Some((a, b)) = shown_sel {
                ui.weak(format!("· selected {}", format_timecode(b - a)));
            }
        });
        egui::ScrollArea::both().id_salt("editor-below").auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            ui.horizontal_wrapped(|ui| {
                let at = ev.cursor;
                let why = |other: &'static str| if locked { tips::LOCKED_WHY } else { other };
                let add = theme::name(ui.add_enabled(at.is_some() && !locked, theme::small(egui::Button::new("+ Marker at cursor"))), "Add marker at cursor");
                if theme::hint_or(add, "Add a marker where the cursor is", why("Click the waveform (or type a Cursor time) to place the cursor first")).clicked() {
                    actions.push(inv!("tile.markerAdd", {"tile": id, "at": at}).into());
                }
                let trim = ui.add_enabled(at.is_some_and(|c| c < e) && !locked, theme::small(egui::Button::new("Start here")));
                if theme::hint_or(trim, "Move the trim start to the cursor", why("Place the cursor before the end first")).clicked() {
                    Handle::Start.set(&mut t, at.unwrap_or(s), dur);
                }
                let trim = ui.add_enabled(at.is_some_and(|c| c > s) && !locked, theme::small(egui::Button::new("End here")));
                if theme::hint_or(trim, "Move the trim end to the cursor", why("Place the cursor after the start first")).clicked() {
                    Handle::End.set(&mut t, at.unwrap_or(e), dur);
                }
                let reset = ui.add_enabled(!locked, theme::small(egui::Button::new("Reset Trim")));
                if theme::hint_or(reset, "Play the whole file again", tips::LOCKED_WHY).clicked() {
                    t.start = 0.0;
                    t.end = None;
                }
            });
            // What to do with the selection.
            ui.horizontal_wrapped(|ui| {
                let sel = selection;
                let trim = ui.add_enabled(sel.is_some() && !locked, theme::small(egui::Button::new("Trim to Selection")));
                let why = if locked { tips::LOCKED_WHY } else { NO_SELECTION };
                if theme::hint_or(trim, "Play just the selected stretch: move the trim start and end to its edges", why).clicked() {
                    if let Some((a, b)) = sel {
                        t.start = a;
                        t.end = if dur - b < 1e-3 { None } else { Some(b) };
                    }
                }
                // The vamp lies inside the trim and lasts at least 50 ms.
                let inside = sel.map(|(a, b)| (a.max(s), b.min(e))).filter(|(a, b)| b - a >= 0.05);
                let vamp = ui.add_enabled(inside.is_some() && !locked, theme::small(egui::Button::new("Vamp Selection")));
                let why = if locked {
                    tips::LOCKED_WHY
                } else if sel.is_some() {
                    "Select at least 50 ms inside the trim first"
                } else {
                    NO_SELECTION
                };
                if theme::hint_or(vamp, "Loop the selected stretch until the next press lets it go (the part inside the trim)", why).clicked() {
                    t.vamp = inside;
                }
                let label = if playing { "Stop" } else { "Play Selection" };
                let play = ui.add_enabled(sel.is_some() || playing, theme::small(egui::Button::new(label)));
                let tip = if playing { "Stop this tile now" } else { "Play just the selected stretch, once, on the Preview output if one is set in Settings" };
                if theme::hint_or(play, tip, NO_SELECTION).clicked() {
                    match sel {
                        Some((a, b)) if !playing => actions.push(inv!("tile.preview", {"tile": id, "from": a, "to": b}).into()),
                        _ => actions.push(inv!("tile.preview", {"tile": id}).into()),
                    }
                }
                let clear = ui.add_enabled(sel.is_some(), theme::small(egui::Button::new("Clear Selection")));
                if theme::hint_or(clear, "Select nothing (a click on the waveform does too)", "Nothing is selected").clicked() {
                    actions.push(inv!("view.editorSelect", {"clear": true}).into());
                }
            });
            self.editor_times(ui, &mut t, &mut ev, &audio, locked, actions);
            ui.separator();
            self.cue_points(ui, &mut t, dur, false, locked, actions);
        });
        self.editor_view = Some(ev);
        if t != orig && !locked {
            actions.push(Action::Run(tile_changes(&orig, &t)));
        }
    }

    /// The exact times, in a grid with their names on the left: the trim, the vamp, the
    /// cursor, the selection, and each marker, whose name can be typed over to rename it.
    /// Across the editor of a locked set's tile: it's read-only, and Unlock.
    fn editor_locked_banner(&self, ui: &mut Ui, id: Uuid, actions: &mut Vec<Action>) {
        let Some((si, _)) = self.ctl.store.library.locate(id) else { return };
        let set = &self.ctl.store.library.sets[si];
        let t = Tokens::get(ui.ctx());
        egui::Frame::new()
            .fill(t.selected)
            .stroke(Stroke::new(1.0_f32, t.border_light))
            .corner_radius(4.0)
            .inner_margin(egui::Margin::symmetric(8, 4))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    let (rect, _) = ui.allocate_exact_size(vec2(16.0, 18.0), Sense::hover());
                    pad_options::paint_lock(ui.painter(), rect.center(), 14.0, true, t.text);
                    ui.label(RichText::new(format!("“{}” is locked: this editor is read-only.", set.name)).color(t.text).strong());
                    ui.label(RichText::new("It still plays and previews.").color(t.text));
                    if theme::hint(theme::small_button(ui, "Unlock"), tips::UNLOCK).clicked() {
                        actions.push(inv!("set.unlock", {"set": set.id}).into());
                    }
                });
            });
    }

    /// In a locked set the trim, vamp and marker rows are greyed (the cursor and the
    /// selection still move, and Jump still plays).
    fn editor_times(&self, ui: &mut Ui, t: &mut Tile, ev: &mut EditorView, audio: &DecodedAudio, locked: bool, actions: &mut Vec<Action>) {
        enum Row {
            Handle(Handle),
            Cursor,
            SelStart,
            SelEnd,
            Marker(usize),
        }
        let id = t.id;
        let rate = audio.sample_rate.max(1);
        let dur = audio.duration().max(1e-6);
        let (s, e) = play_range(t, dur);
        let selection = self.ctl.editor_selection;
        let mut rows = vec![(Row::Handle(Handle::Start), Some(s)), (Row::Handle(Handle::End), Some(e))];
        if let Some((a, b)) = vamp_range(t, dur) {
            rows.extend([(Row::Handle(Handle::VampStart), Some(a)), (Row::Handle(Handle::VampEnd), Some(b))]);
        }
        rows.push((Row::Cursor, ev.cursor));
        rows.extend([(Row::SelStart, selection.map(|(a, _)| a)), (Row::SelEnd, selection.map(|(_, b)| b))]);
        rows.extend(t.markers.iter().enumerate().map(|(i, m)| (Row::Marker(i), Some(m.at))));
        egui::Grid::new("editor-times").num_columns(4).min_col_width(LABEL_W).spacing(vec2(8.0, 6.0)).show(ui, |ui| {
            for (row, at) in rows {
                let name = match &row {
                    Row::Handle(h) => h.name().to_owned(),
                    Row::Cursor => "Cursor".to_owned(),
                    Row::SelStart => "Selection start".to_owned(),
                    Row::SelEnd => "Selection end".to_owned(),
                    Row::Marker(i) => format!("Marker {}", t.markers[*i].name),
                };
                // A marker's fields keep their id (its place) while it's renamed.
                let fid = match &row {
                    Row::Marker(i) => egui::Id::unique(("editor-marker", *i)),
                    _ => egui::Id::unique(("editor-time", name.as_str())),
                };
                let edits = !locked || matches!(row, Row::Cursor | Row::SelStart | Row::SelEnd);
                if let Row::Marker(i) = row {
                    let m = &t.markers[i];
                    let renamed = ui.add_enabled_ui(edits, |ui| name_field(ui, fid.with("name"), &format!("{name}, name"), &m.name, LABEL_W)).inner;
                    if let Some(new) = renamed {
                        actions.push(inv!("tile.markerMove", {"tile": id, "marker": format!("#{}", i + 1), "name": new}).into());
                    }
                } else {
                    ui.label(&name);
                }
                let a = ui.add_enabled_ui(edits, |ui| time_field(ui, fid, &format!("{name}, time"), at.map(format_timecode), rate, 120.0)).inner;
                let b = ui.add_enabled_ui(edits, |ui| time_field(ui, fid.with("smp"), &format!("{name}, samples"), at.map(|at| format_samples(at, rate)), rate, 150.0)).inner;
                if let Some(sec) = a.or(b) {
                    let sec = sec.clamp(0.0, dur);
                    match row {
                        Row::Handle(h) => h.set(t, sec, dur),
                        Row::Cursor => ev.cursor = Some(sec),
                        Row::SelStart => {
                            let to = selection.map_or(dur, |(_, b)| b);
                            actions.push(inv!("view.editorSelect", {"from": sec, "to": to}).into());
                        }
                        Row::SelEnd => {
                            let from = selection.map_or(0.0, |(a, _)| a);
                            actions.push(inv!("view.editorSelect", {"from": from, "to": sec}).into());
                        }
                        Row::Marker(i) => actions.push(inv!("tile.markerMove", {"tile": id, "marker": format!("#{}", i + 1), "at": sec}).into()),
                    }
                }
                if let Row::Marker(i) = row {
                    let m = &t.markers[i];
                    ui.horizontal(|ui| {
                        if theme::hint(theme::small_button(ui, "Jump"), format!("Play from {} (or jump there while it plays)", m.name)).clicked() {
                            actions.push(inv!("tile.jump", {"tile": id, "marker": format!("#{}", i + 1)}).into());
                        }
                        let remove = ui.add_enabled(!locked, theme::small(egui::Button::new("🗙")));
                        if theme::hint_or(theme::name(remove, &format!("Remove {}", m.name)), format!("Remove the marker {}", m.name), tips::LOCKED_WHY).clicked() {
                            actions.push(inv!("tile.markerRemove", {"tile": id, "marker": format!("#{}", i + 1)}).into());
                        }
                    });
                }
                ui.end_row();
            }
        });
    }

    /// The waveform, the ruler, the trim shading, the selection, the vamp, the markers, the
    /// cursor and the playhead.
    /// In a locked set the handles and markers are greyed, as their controls are.
    #[allow(clippy::too_many_arguments)]
    fn paint_editor_wave(&self, ui: &Ui, rect: Rect, t: &Tile, audio: &DecodedAudio, pyramid: &PeakPyramid, view: View, cursor: Option<f64>, selection: Option<(f64, f64)>, locked: bool) {
        let tokens = Tokens::get(ui.ctx());
        let p = ui.painter_at(rect);
        let ratef = f64::from(audio.sample_rate.max(1));
        let dur = audio.duration().max(1e-6);
        let x_of = |sec: f64| rect.left() + view.x_of(sec * ratef);
        p.rect_filled(rect, 6.0, Color32::from_black_alpha(90));
        let wave = Rect::from_x_y_ranges(rect.x_range(), rect.top() + RULER_H..=rect.bottom());
        let mid = wave.center().y;
        let half = wave.height() / 2.0 - 4.0;
        let color = theme::lighten(theme::console_tile(ui.ctx(), t.color), 0.25);
        p.line_segment([pos2(rect.left(), mid), pos2(rect.right(), mid)], Stroke::new(1.0_f32, theme::with_alpha(color, 0.35)));
        let y = |v: f32| mid - v.clamp(-1.0, 1.0) * half;
        if view.fpp >= RAW_FPP {
            let count = rect.width().ceil() as usize;
            for (i, mm) in pyramid.columns(&audio.data, view.start, view.fpp, count).into_iter().enumerate() {
                if let Some(mm) = mm {
                    let x = rect.left() + i as f32 + 0.5;
                    let (top, bottom) = (y(mm.max), y(mm.min));
                    p.line_segment([pos2(x, top), pos2(x, bottom.max(top + 1.0))], Stroke::new(1.0_f32, color));
                }
            }
        } else {
            // Deep zoom: each sample, both channels, joined up.
            let frames = audio.frames();
            let first = view.start.floor().max(0.0) as usize;
            let last = ((view.frame_at(rect.width()).ceil() as usize) + 1).min(frames);
            let pairs = audio.data.as_chunks::<2>().0;
            for (ch, c) in [(0usize, color), (1, theme::with_alpha(theme::lighten(color, 0.3), 0.7))] {
                let points: Vec<Pos2> = (first..last).map(|f| pos2(rect.left() + view.x_of(f as f64), y(pairs[f][ch]))).collect();
                if view.fpp < DOT_FPP {
                    for &pt in &points {
                        p.circle_filled(pt, 2.0, c);
                    }
                }
                p.add(egui::Shape::line(points, Stroke::new(1.5_f32, c)));
            }
        }

        // The ruler.
        let ruler = Rect::from_x_y_ranges(rect.x_range(), rect.top()..=rect.top() + RULER_H);
        p.rect_filled(ruler, CornerRadius { nw: 6, ne: 6, sw: 0, se: 0 }, Color32::from_black_alpha(60));
        let spp = view.fpp / ratef;
        let step = tick_step(spp, 90.0);
        let from = (view.start / ratef / step).floor() as i64;
        let to = (view.frame_at(rect.width()) / ratef / step).ceil() as i64;
        let text = ui.visuals().weak_text_color();
        for k in from..=to {
            let sec = k as f64 * step;
            if !(0.0..=dur).contains(&sec) {
                continue;
            }
            let x = x_of(sec);
            p.line_segment([pos2(x, ruler.bottom() - 5.0), pos2(x, ruler.bottom())], Stroke::new(1.0_f32, text));
            let label = if step < 0.001 { format_samples(sec, audio.sample_rate) } else { format_timecode(sec) };
            p.text(pos2(x + 3.0, ruler.top() + 2.0), Align2::LEFT_TOP, label, FontId::monospace(10.0), text);
        }

        // What doesn't play, the vamp, the markers and the handles.
        let (s, e) = play_range(t, dur);
        let (trim, marker) = if locked { (tokens.text_dim, theme::with_alpha(tokens.marker, 0.5)) } else { (tokens.trim, tokens.marker) };
        let shade = Color32::from_black_alpha(150);
        let clamp_x = |x: f32| x.clamp(rect.left(), rect.right());
        p.rect_filled(Rect::from_x_y_ranges(rect.left()..=clamp_x(x_of(s)), wave.y_range()), 0.0, shade);
        p.rect_filled(Rect::from_x_y_ranges(clamp_x(x_of(e))..=rect.right(), wave.y_range()), 0.0, shade);
        if let Some((a, b)) = selection {
            // The theme's own selection colour, as text selections use.
            let sel = ui.visuals().selection.bg_fill;
            let (xa, xb) = (clamp_x(x_of(a)), clamp_x(x_of(b)).max(clamp_x(x_of(a)) + 1.0));
            p.rect_filled(Rect::from_x_y_ranges(xa..=xb, rect.y_range()), 0.0, theme::with_alpha(sel, 0.3));
            for x in [x_of(a), x_of(b)] {
                p.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], Stroke::new(1.0_f32, sel));
            }
        }
        if let Some((a, b)) = vamp_range(t, dur) {
            p.rect_filled(Rect::from_x_y_ranges(clamp_x(x_of(a))..=clamp_x(x_of(b)), wave.y_range()), 0.0, theme::with_alpha(tokens.vamp, 0.18));
            for x in [x_of(a), x_of(b)] {
                p.line_segment([pos2(x, wave.top()), pos2(x, wave.bottom())], Stroke::new(1.5_f32, tokens.vamp));
            }
            p.text(pos2(x_of(a) + 4.0, wave.top() + 2.0), Align2::LEFT_TOP, "VAMP", FontId::proportional(10.0), tokens.vamp);
        }
        for m in &t.markers {
            let x = x_of(m.at);
            p.line_segment([pos2(x, wave.top()), pos2(x, wave.bottom())], Stroke::new(1.0_f32, marker));
            p.add(egui::Shape::convex_polygon(
                vec![pos2(x, rect.bottom() - 16.0), pos2(x + 9.0, rect.bottom() - 10.0), pos2(x, rect.bottom() - 4.0)],
                marker,
                Stroke::NONE,
            ));
            p.text(pos2(x + 4.0, wave.top() + 14.0), Align2::LEFT_TOP, &m.name, FontId::proportional(11.0), tokens.marker);
        }
        for x in [x_of(s), x_of(e)] {
            p.line_segment([pos2(x, wave.top()), pos2(x, wave.bottom())], Stroke::new(2.0_f32, trim));
            // A locked handle has no grip to take hold of.
            if !locked {
                p.rect_filled(Rect::from_center_size(pos2(x, wave.top() + 6.0), vec2(8.0, 12.0)), 2.0, trim);
            }
        }
        if let Some(c) = cursor {
            let x = x_of(c);
            let dash = egui::Shape::dashed_line(&[pos2(x, rect.top()), pos2(x, rect.bottom())], Stroke::new(1.0_f32, tokens.text), 4.0, 3.0);
            p.extend(dash);
        }
        if let Some(st) = self.ctl.statuses.get(&t.id) {
            let x = x_of(st.from + st.position * st.speed);
            p.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], Stroke::new(1.5_f32, tokens.playhead));
            ui.ctx().request_repaint();
        }
    }
}

/// The id part for one mark's control.
fn mark_key(mark: Mark) -> String {
    match mark {
        Mark::Handle(h) => h.name().to_owned(),
        Mark::Marker(i) => format!("marker-{i}"),
    }
}

fn editor_tip(what: &str) -> String {
    format!("{what}: drag it, or Tab to it and press Left/Right for 10 ms steps (Shift: 100 ms; Alt: one sample). With Snap on it lands on a zero crossing")
}

/// A time typed in as text: shows `shown` (empty when there's none yet) until focused,
/// and gives the parsed time (to seconds) when Enter or a click elsewhere ends the edit.
fn time_field(ui: &mut Ui, id: egui::Id, name: &str, shown: Option<String>, rate: u32, width: f32) -> Option<f64> {
    let draft = id.with("draft");
    let mut text = ui.ctx().data(|d| d.get_temp::<String>(draft)).or(shown).unwrap_or_default();
    let edit = egui::TextEdit::singleline(&mut text).id(id).desired_width(width).font(egui::TextStyle::Monospace).hint_text("not set");
    let resp = theme::hint(theme::name(theme::text_field(ui, edit), name), TIME_TIP);
    if resp.has_focus() {
        ui.ctx().data_mut(|d| d.insert_temp(draft, text));
        return None;
    }
    ui.ctx().data_mut(|d| d.remove::<String>(draft));
    (resp.lost_focus() && !ui.input(|i| i.key_pressed(Key::Escape))).then(|| parse_timecode(&text, rate)).flatten()
}

/// A marker's name, typed over: gives the new name when Enter or a click elsewhere ends
/// the edit (Esc, or an empty name, puts the old one back).
fn name_field(ui: &mut Ui, id: egui::Id, name: &str, current: &str, width: f32) -> Option<String> {
    let draft = id.with("draft");
    let mut text = ui.ctx().data(|d| d.get_temp::<String>(draft)).unwrap_or_else(|| current.to_owned());
    let edit = egui::TextEdit::singleline(&mut text).id(id).desired_width(width);
    let tip = "The marker's name: type a new one and press Enter to rename it (Esc puts it back). tile.jump and pads find the marker by its name";
    let resp = theme::hint(theme::name(theme::text_field(ui, edit), name), tip);
    if resp.has_focus() {
        ui.ctx().data_mut(|d| d.insert_temp(draft, text));
        return None;
    }
    ui.ctx().data_mut(|d| d.remove::<String>(draft));
    let new = text.trim();
    (resp.lost_focus() && !ui.input(|i| i.key_pressed(Key::Escape)) && !new.is_empty() && new != current).then(|| new.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_fits_zooms_around_the_pointer_and_stays_in_the_file() {
        let frames = 48_000;
        let v = View::fit(frames, 480.0);
        assert_eq!(v, View { start: 0.0, fpp: 100.0 });
        // Zooming in keeps the frame under the pointer still.
        let z = v.zoom(4.0, 120.0, frames, 480.0);
        assert_eq!(z.fpp, 25.0);
        assert!((z.frame_at(120.0) - v.frame_at(120.0)).abs() < 1e-9);
        // No further out than the whole file, no deeper than MIN_FPP.
        assert_eq!(z.zoom(0.001, 0.0, frames, 480.0), v);
        assert_eq!(v.zoom(1e9, 0.0, frames, 480.0).fpp, MIN_FPP);
        // Panning stops at both ends.
        assert_eq!(z.pan(-1e9, frames, 480.0).start, 0.0);
        assert_eq!(z.pan(1e9, frames, 480.0).start, frames as f64 - 480.0 * 25.0);
        // Garbage in, the whole file out.
        assert_eq!(View { start: f64::NAN, fpp: f64::NAN }.clamp(frames, 480.0), v);
        // A span shows just those frames.
        let s = View::span(1000.0, 5800.0, frames, 480.0);
        assert_eq!(s, View { start: 1000.0, fpp: 10.0 });
        assert!((s.x_of(5800.0) - 480.0).abs() < 1e-3);
        // A file shorter than the width still fits.
        assert_eq!(View::fit(10, 480.0).clamp(10, 480.0).start, 0.0);
    }

    #[test]
    fn ruler_ticks_are_round_and_far_enough_apart() {
        assert_eq!(tick_step(0.01, 90.0), 1.0);
        assert_eq!(tick_step(1.0 / 48_000.0, 90.0), 0.002);
        assert_eq!(tick_step(1.0 / 48_000.0 / 32.0, 90.0), 0.0001);
        assert_eq!(tick_step(100.0, 90.0), 3600.0);
    }

    #[test]
    fn snapping_finds_the_crossing_or_leaves_it() {
        // A 100 Hz sine at 48 kHz: crossings every 240 frames.
        let data: Vec<f32> = (0..4800).flat_map(|i| {
            let v = (i as f32 * std::f32::consts::TAU / 480.0).sin() * 0.5;
            [v, v]
        })
        .collect();
        let audio = DecodedAudio { sample_rate: 48_000, data, art: None, native: None };
        let at = snap(&audio, 250.0 / 48_000.0, 20);
        assert!(((at * 48_000.0).round() - 240.0).abs() <= 1.0, "{}", at * 48_000.0);
        let far = 360.0 / 48_000.0;
        assert_eq!(snap(&audio, far, 20), far, "no crossing within reach: unchanged");
    }
}
