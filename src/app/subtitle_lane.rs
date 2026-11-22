//! The waveform editor's subtitle lane (D-267 to D-270, D-274, D-275): a tile's subtitle
//! lines across the top of its waveform, under the ruler, on the same time axis at every
//! zoom, each line a bar and, zoomed in far enough, each word a block that can be
//! dragged onto its sound.
//!
//! A word's edges and body drag (snapping to where sounds start, [`align::onsets`]); a
//! line's bar drags the whole line. Shift leaves a shared boundary's neighbour where it
//! is, Alt drags without snapping. Each drag sends one command when it ends (one undo
//! step); until then the lane shows where it's going. A click selects a word and gives it
//! the keyboard: Left/Right nudge its grip (start, end or the whole word), Up/Down change
//! the grip, Enter starts it at the playhead and goes to the next word. Green guide lines
//! run from the selected word's edges down through the waveform. A click on a line's bar
//! gives the line the keyboard: Left/Right move it, Delete deletes it; the tools row adds
//! lines (D-301, D-302). The rules themselves
//! are in src/align.rs.

use super::*;
use super::editor::View;
use super::icons::{self, Glyph};
use crate::align::{self, Edge, Grip, Onset};
use crate::decode::DecodedAudio;
use crate::logic::format_timecode;
use crate::subtitles::SubtitleLine;

const TOUCH: bool = cfg!(target_os = "ios");
/// The lane's height; on a touch screen the blocks are a finger tall.
pub const LANE_H: f32 = if TOUCH { 66.0 } else { 50.0 };
/// The line bars along the lane's top.
const BAR_H: f32 = if TOUCH { 18.0 } else { 14.0 };
/// How wide a word's edge handle is.
const HANDLE_W: f32 = if TOUCH { 22.0 } else { 8.0 };
/// A dragged edge snaps to a sound's start this many points away.
const SNAP_PX: f32 = if TOUCH { 14.0 } else { 7.0 };
/// Words show once a line has this many points per word.
const WORD_PX: f32 = 16.0;
/// Onsets are drawn while there are fewer than one per this many points.
const ONSET_PX: f32 = 3.0;
/// The keyboard's nudges, in seconds (Shift: the coarse one).
const NUDGE: f64 = 0.01;
const NUDGE_BIG: f64 = 0.1;
/// The guide lines' colour: a green no other mark in the editor uses (D-275).
const GUIDE: Color32 = Color32::from_rgb(0x5C, 0xF0, 0x8A);

pub const KEYS: &str = "Left/Right nudge it 10 ms (Shift: 100 ms), Up/Down choose its start, the whole word or its end, Enter starts it at the playhead and goes to the next word";
pub const WORD_TIP: &str = "A subtitle word: click it to select it, then Left/Right nudge it 10 ms (Shift: 100 ms) and Up/Down choose what they move: its start, the whole word or its end (a click on an edge picks that edge). Enter starts it at the playhead (or the cursor) and goes to the next word, to tap a line in while it plays. Or drag it, or its edges, onto its sound: they snap to where sounds start (the short ticks); Shift leaves the neighbour's edge where it is, Alt drags without snapping";
pub const LINE_KEYS: &str = "Left/Right move the whole line 10 ms (Shift: 100 ms), Delete or Backspace deletes it";
const LINE_TIP: &str = "A subtitle line: click it to select it, then Left/Right move the whole line 10 ms (Shift: 100 ms) and Delete or Backspace deletes it; or drag it to move all its words. Dashed words are a guess spread over the line; filled words came with the subtitles; yellow ones were placed by hand; the short ticks along the bottom are where sounds start";

/// Which part of the lane a drag holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Part {
    Start,
    End,
    Word,
    Line,
}

/// A drag in progress: what it holds, how far from its anchor it was taken, and the line
/// as it would be if it ended now (with the time the command will get).
#[derive(Clone)]
struct Drag {
    line: usize,
    word: usize,
    part: Part,
    grab: f64,
    preview: Option<(SubtitleLine, f64, bool)>,
}

/// The lane's state while the editor is open (the selected word is the controller's,
/// `view.editorWord`).
#[derive(Default)]
pub struct LaneState {
    /// Where sounds start in the tile's audio, found once when the lane first shows.
    onsets: Option<Vec<Onset>>,
    drag: Option<Drag>,
    /// A word to give the keyboard focus to (after a click on its edge, or Enter).
    focus: Option<(usize, usize)>,
    /// A line's bar to give the keyboard focus to (after its neighbour was deleted).
    focus_line: Option<usize>,
    /// The line whose bar had the keyboard this frame, drawn with a ring.
    line_has_focus: Option<usize>,
}

impl LaneState {
    /// Give word `word` of line `line` the keyboard focus (a script step).
    pub fn focus(&mut self, line: usize, word: usize) {
        self.focus = Some((line, word));
    }

    /// Give line `line`'s bar the keyboard focus (a script step).
    pub fn focus_line(&mut self, line: usize) {
        self.focus_line = Some(line);
    }
}

fn word_id(base: egui::Id, li: usize, wi: usize) -> egui::Id {
    base.with(("word", li, wi))
}

fn line_id(base: egui::Id, li: usize) -> egui::Id {
    base.with(("line", li))
}

/// The word after `(li, wi)`, in the next line when it's the last of its line.
fn next_word(lines: &[SubtitleLine], li: usize, wi: usize) -> Option<(usize, usize)> {
    if wi + 1 < align::words_of(&lines[li]).len() {
        Some((li, wi + 1))
    } else {
        (li + 1 < lines.len()).then_some((li + 1, 0))
    }
}

/// A guide line at `x` from `top` to `bottom`: green over a dark halo, solid for the edge
/// the arrows move, dashed for the other, with a tag naming it at the bottom.
fn guide(p: &egui::Painter, x: f32, top: f32, bottom: f32, solid: bool, tag: &str) {
    let (a, b) = (pos2(x, top), pos2(x, bottom));
    let halo = Stroke::new(if solid { 4.0 } else { 3.0 }, Color32::from_black_alpha(170));
    let line = Stroke::new(if solid { 2.0 } else { 1.2 }, if solid { GUIDE } else { theme::with_alpha(GUIDE, 0.85) });
    if solid {
        p.line_segment([a, b], halo);
        p.line_segment([a, b], line);
        let font = FontId::proportional(10.0);
        let galley = p.layout_no_wrap(tag.to_owned(), font, Color32::BLACK);
        let r = Rect::from_min_size(pos2(x - galley.size().x / 2.0 - 3.0, bottom - galley.size().y - 4.0), galley.size() + vec2(6.0, 2.0));
        p.rect_filled(r, 3.0, GUIDE);
        p.galley(r.min + vec2(3.0, 1.0), galley, Color32::BLACK);
    } else {
        p.extend(egui::Shape::dashed_line(&[a, b], halo, 5.0, 4.0));
        p.extend(egui::Shape::dashed_line(&[a, b], line, 5.0, 4.0));
    }
}

impl StingerApp {
    /// The selected word in the open editor, when it still names a word of this tile.
    fn editor_word(&self, t: &Tile) -> Option<crate::controller::subtitle_edit::EditorWord> {
        self.ctl.editor_word.filter(|w| w.tile == t.id && t.subtitles.get(w.line).is_some_and(|l| w.word < align::words_of(l).len()))
    }

    /// The lane in `rect`, across the top of the waveform in `wave` (drawn with `view`).
    /// `trim` is the part that plays (lines outside it are dimmed).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn subtitle_lane(&mut self, ui: &mut Ui, rect: Rect, wave: Rect, t: &Tile, audio: &DecodedAudio, lane: &mut LaneState, view: View, snap_on: bool, cursor: Option<f64>, trim: (f64, f64), locked: bool, actions: &mut Vec<Action>) {
        let id = t.id;
        let ratef = f64::from(audio.sample_rate.max(1));
        let dur = audio.duration().max(1e-6);
        let spp = view.fpp / ratef;
        let x_of = |sec: f64| rect.left() + view.x_of(sec * ratef);
        let sec_at = |x: f32| (view.frame_at(x - rect.left()) / ratef).clamp(0.0, dur);
        let (view_a, view_b) = (sec_at(rect.left()), sec_at(rect.right()));
        // Taken out while the lane uses it, and put back at the end.
        let found = lane.onsets.take().unwrap_or_else(|| align::onsets(audio));
        let onsets = &found[..];
        let playhead = self.ctl.statuses.get(&id).map(|st| st.from + st.position * st.speed);
        if playhead.is_some() {
            ui.ctx().request_repaint();
        }
        let selected = self.editor_word(t);
        let base = egui::Id::unique(("subtitle-lane", id));
        let lines = &t.subtitles;
        let bar = |l: &SubtitleLine| Rect::from_x_y_ranges(x_of(l.start)..=x_of(l.end).max(x_of(l.start) + 2.0), rect.top() + 2.0..=rect.top() + BAR_H);
        let row = rect.top() + BAR_H + 3.0..=rect.bottom() - 9.0;
        let shows_words = |l: &SubtitleLine| (x_of(l.end) - x_of(l.start)) / align::words_of(l).len().max(1) as f32 >= WORD_PX;
        let visible = |l: &SubtitleLine| l.end >= view_a && l.start <= view_b;
        let select = |line: usize, word: usize, grip: Option<Grip>| -> Action {
            match grip {
                Some(g) => inv!("view.editorWord", {"line": line + 1, "word": word + 1, "grip": g.name()}).into(),
                None => inv!("view.editorWord", {"line": line + 1, "word": word + 1}).into(),
            }
        };

        // The pointer and the keyboard: every visible line's bar and, zoomed in, its
        // words' bodies and edges. A locked set's lane only shows.
        let shown_line = |li: usize, lane: &LaneState| match &lane.drag {
            Some(Drag { line, preview: Some((l, ..)), .. }) if *line == li => l.clone(),
            _ => lines[li].clone(),
        };
        lane.line_has_focus = None;
        let mut lane_ui = ui.new_child(egui::UiBuilder::new().id_salt(("subtitle-lane-ui", id)).max_rect(rect));
        if locked {
            lane_ui.disable();
        }
        for li in 0..lines.len() {
            let l = shown_line(li, lane);
            if !visible(&l) {
                continue;
            }
            let mut parts: Vec<(Part, usize, Rect)> = vec![(Part::Line, 0, bar(&l))];
            if shows_words(&l) {
                for (wi, w) in align::words_of(&l).iter().enumerate() {
                    let (a, b) = (x_of(w.start), x_of(w.end));
                    let hw = HANDLE_W.min((b - a) / 3.0).max(2.0);
                    parts.push((Part::Word, wi, Rect::from_x_y_ranges(a + hw / 2.0..=(b - hw / 2.0).max(a + hw / 2.0 + 1.0), row.clone())));
                    parts.push((Part::Start, wi, Rect::from_x_y_ranges(a - hw / 2.0..=a + hw / 2.0, row.clone())));
                    parts.push((Part::End, wi, Rect::from_x_y_ranges(b - hw / 2.0..=b + hw / 2.0, row.clone())));
                }
            }
            for (part, wi, r) in parts {
                // A word scrolled half out of view is held by the part that shows.
                let r = r.intersect(rect);
                if r.width() < 1.0 {
                    continue;
                }
                // A word's body and a line's bar are Tab stops; a word's edges are for the pointer.
                let (rid, sense) = match part {
                    Part::Word => (word_id(base, li, wi), Sense::click_and_drag()),
                    Part::Line => (line_id(base, li), Sense::click_and_drag()),
                    _ => (base.with(("edge", li, wi, part == Part::Start)), Sense::CLICK | Sense::DRAG),
                };
                let resp = lane_ui.interact(r, rid, sense);
                let resp = match part {
                    Part::Word => {
                        let w = &align::words_of(&l)[wi];
                        let name = format!("Subtitle word “{}”, line {}{}", w.text, li + 1, if locked { ", read-only" } else { "" });
                        let (resp, steps) = theme::slider_info(resp, &name, w.start, format!("{} to {}", format_timecode(w.start), format_timecode(w.end)));
                        if steps != 0 && !locked {
                            let grip = selected.filter(|s| (s.line, s.word) == (li, wi)).map_or(Grip::Start, |s| s.grip);
                            actions.push(inv!("tile.subtitleNudge", {"tile": id, "line": li + 1, "word": wi + 1, "grip": grip.name(), "by": NUDGE * f64::from(steps)}).into());
                        }
                        theme::hint_or(resp, WORD_TIP, tips::LOCKED_WHY)
                    }
                    Part::Line => {
                        let name = format!("Subtitle line {} “{}”{}", li + 1, l.text.replace('\n', " / "), if locked { ", read-only" } else { "" });
                        theme::hint_or(theme::name(resp, &name), LINE_TIP, tips::LOCKED_WHY)
                    }
                    _ => resp,
                };
                if locked {
                    continue;
                }
                if lane.focus == Some((li, wi)) && part == Part::Word {
                    resp.request_focus();
                    lane.focus = None;
                }
                if lane.focus_line == Some(li) && part == Part::Line {
                    resp.request_focus();
                    lane.focus_line = None;
                }
                // A click selects the word and gives it the keyboard at once (egui doesn't
                // focus a clicked widget by itself); a click on an edge picks that edge.
                if resp.clicked() || resp.drag_started() {
                    let grip = match part {
                        Part::Start => Some(Grip::Start),
                        Part::End => Some(Grip::End),
                        _ => None,
                    };
                    let wi = if part == Part::Line { 0 } else { wi };
                    actions.push(select(li, wi, grip));
                    // A click on a line's bar gives the line the keyboard (to move or
                    // delete it); a click on a word, the word.
                    if part == Part::Word || part == Part::Line {
                        resp.request_focus();
                    } else {
                        ui.memory_mut(|m| m.request_focus(word_id(base, li, wi)));
                    }
                }
                if part == Part::Word && resp.gained_focus() && selected.is_none_or(|s| (s.line, s.word) != (li, wi)) {
                    actions.push(select(li, wi, None));
                }
                if part == Part::Line && resp.gained_focus() && selected.is_none_or(|s| s.line != li) {
                    actions.push(select(li, 0, None));
                }
                // The keyboard, on a focused line: Left/Right move it, Delete deletes it.
                if part == Part::Line && resp.has_focus() {
                    lane.line_has_focus = Some(li);
                    let filter = egui::EventFilter { horizontal_arrows: true, ..Default::default() };
                    ui.memory_mut(|m| m.set_focus_lock_filter(resp.id, filter));
                    let (left, right, delete, shift) = ui.input_mut(|i| {
                        let shift = i.modifiers.shift;
                        let m = if shift { Modifiers::SHIFT } else { Modifiers::NONE };
                        let delete = i.consume_key(Modifiers::NONE, Key::Delete) | i.consume_key(Modifiers::NONE, Key::Backspace);
                        (i.consume_key(m, Key::ArrowLeft), i.consume_key(m, Key::ArrowRight), delete, shift)
                    });
                    if left || right {
                        let by = if right { 1.0 } else { -1.0 } * if shift { NUDGE_BIG } else { NUDGE };
                        actions.push(inv!("tile.subtitleLineMove", {"tile": id, "line": li + 1, "by": by}).into());
                        // The line may take another number; its bar keeps the keyboard
                        // when it doesn't (the common case: lines rarely cross).
                    }
                    if delete {
                        actions.push(inv!("tile.subtitleDelete", {"tile": id, "line": li + 1}).into());
                        // The next line (now this number), or the one before, takes the keyboard.
                        if lines.len() > 1 {
                            lane.focus_line = Some(if li + 1 < lines.len() { li } else { li - 1 });
                        }
                    }
                }
                match part {
                    Part::Start | Part::End => {
                        if resp.hovered() || resp.dragged() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                        }
                    }
                    _ if resp.dragged() => ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing),
                    _ if resp.hovered() => ui.ctx().set_cursor_icon(egui::CursorIcon::Grab),
                    _ => {}
                }
                // Dragging: the anchor is the edge, the word's start or the line's start.
                let orig = &lines[li];
                let anchor = |l: &SubtitleLine| {
                    let ws = align::words_of(l);
                    match part {
                        Part::Start | Part::Word => ws[wi].start,
                        Part::End => ws[wi].end,
                        Part::Line => l.start,
                    }
                };
                if resp.drag_started() {
                    let at = ui.input(|i| i.pointer.press_origin()).map_or(anchor(orig), |p| sec_at(p.x));
                    lane.drag = Some(Drag { line: li, word: wi, part, grab: at - anchor(orig), preview: None });
                }
                let mine = lane.drag.as_ref().is_some_and(|d| d.line == li && d.word == wi && d.part == part);
                if mine && resp.dragged() {
                    if let (Some(pos), Some(d)) = (resp.interact_pointer_pos(), lane.drag.as_mut()) {
                        let m = ui.input(|i| i.modifiers);
                        let mut to = (sec_at(pos.x) - d.grab).clamp(0.0, dur);
                        if snap_on && !m.alt {
                            to = align::nearest_onset(onsets, to, f64::from(SNAP_PX) * spp).unwrap_or(to);
                        }
                        let mut p = orig.clone();
                        let _ = match part {
                            Part::Start => align::move_edge(&mut p, wi, Edge::Start, to, m.shift, dur),
                            Part::End => align::move_edge(&mut p, wi, Edge::End, to, m.shift, dur),
                            Part::Word => align::move_word(&mut p, wi, to, m.shift, dur),
                            Part::Line => {
                                align::move_line(&mut p, to - orig.start, dur);
                                Ok(())
                            }
                        };
                        d.preview = Some((p, to, m.shift));
                    }
                }
                if mine && resp.drag_stopped() {
                    if let Some(Drag { preview: Some((p, to, split)), .. }) = lane.drag.take() {
                        if p != *orig {
                            let (line, word) = (li + 1, wi + 1);
                            actions.push(match part {
                                Part::Start | Part::End => {
                                    let edge = if part == Part::Start { "start" } else { "end" };
                                    inv!("tile.subtitleEdge", {"tile": id, "line": line, "word": word, "edge": edge, "at": to, "split": split})
                                }
                                Part::Word => inv!("tile.subtitleWordMove", {"tile": id, "line": line, "word": word, "at": to, "split": split}),
                                Part::Line => inv!("tile.subtitleLineMove", {"tile": id, "line": line, "at": to}),
                            }.into());
                        }
                    }
                }
                // The keyboard, on a focused word: arrows nudge its grip, Up/Down change
                // the grip, Enter taps it in. A focused control keeps the arrows from the
                // board, and none of these keys are pad keys.
                if part == Part::Word && resp.has_focus() {
                    let filter = egui::EventFilter { horizontal_arrows: true, vertical_arrows: true, ..Default::default() };
                    ui.memory_mut(|m| m.set_focus_lock_filter(resp.id, filter));
                    let (left, right, up, down, enter, shift) = ui.input_mut(|i| {
                        let shift = i.modifiers.shift;
                        let m = if shift { Modifiers::SHIFT } else { Modifiers::NONE };
                        (i.consume_key(m, Key::ArrowLeft), i.consume_key(m, Key::ArrowRight), i.consume_key(Modifiers::NONE, Key::ArrowUp), i.consume_key(Modifiers::NONE, Key::ArrowDown), i.consume_key(Modifiers::NONE, Key::Enter), shift)
                    });
                    let grip = selected.filter(|s| (s.line, s.word) == (li, wi)).map_or(Grip::Start, |s| s.grip);
                    if left || right {
                        let by = if right { 1.0 } else { -1.0 } * if shift { NUDGE_BIG } else { NUDGE };
                        actions.push(inv!("tile.subtitleNudge", {"tile": id, "line": li + 1, "word": wi + 1, "grip": grip.name(), "by": by}).into());
                    }
                    if up || down {
                        actions.push(select(li, wi, Some(grip.step(down))));
                    }
                    if enter {
                        if let Some(at) = playhead.or(cursor) {
                            actions.push(inv!("tile.subtitleTap", {"tile": id, "line": li + 1, "word": wi + 1, "at": at}).into());
                            if let Some((nl, nw)) = next_word(lines, li, wi) {
                                lane.focus = Some((nl, nw));
                                actions.push(select(nl, nw, None));
                            }
                        }
                    }
                }
            }
        }

        // Drawn after the controls so a drag shows where it's going this frame.
        let tokens = Tokens::get(ui.ctx());
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 0.0, Color32::from_black_alpha(70));
        let tile_c = theme::lighten(theme::console_tile(ui.ctx(), t.color), 0.25);
        let text_c = ui.visuals().text_color();
        // Where sounds start: short ticks along the bottom, pointing into the waveform,
        // while they're far enough apart to read.
        let in_view: Vec<&Onset> = onsets.iter().filter(|o| o.at >= view_a && o.at <= view_b).collect();
        if (in_view.len() as f32) < rect.width() / ONSET_PX {
            for o in in_view {
                let x = x_of(o.at);
                let a = (o.strength / 40.0).clamp(0.25, 0.6);
                p.line_segment([pos2(x, rect.bottom() - 7.0), pos2(x, rect.bottom())], Stroke::new(1.5_f32, theme::with_alpha(text_c, a)));
            }
        }
        let now = playhead.and_then(|at| align::at_time(lines, at));
        let font = FontId::proportional(if TOUCH { 13.0 } else { 11.0 });
        for li in 0..lines.len() {
            let l = shown_line(li, lane);
            if !visible(&l) {
                continue;
            }
            // Lines that aren't heard (outside the trim) are dimmed.
            let fade = if l.end > trim.0 && l.start < trim.1 { 1.0 } else { 0.35 };
            let dim = |c: Color32| theme::with_alpha(c, f32::from(c.a()) / 255.0 * fade);
            let provisional = l.words.is_empty();
            let (fill, stroke) = match (provisional, l.aligned) {
                (true, _) => (Color32::TRANSPARENT, theme::with_alpha(tokens.text_dim, 0.8)),
                (false, false) => (theme::with_alpha(tile_c, 0.3), theme::with_alpha(tile_c, 0.8)),
                (false, true) => (theme::with_alpha(tokens.trim, 0.4), tokens.trim),
            };
            let b = bar(&l);
            let current_line = now.is_some_and(|(n, _)| n == li);
            p.rect_filled(b, 3.0, dim(theme::with_alpha(if l.aligned { tokens.trim } else { tile_c }, 0.25)));
            p.rect_stroke(b, 3.0, Stroke::new(if current_line { 2.0 } else { 1.0 }, dim(if current_line { tokens.playhead } else { stroke })), egui::StrokeKind::Inside);
            if lane.line_has_focus == Some(li) {
                p.rect_stroke(b.expand(1.0), 3.0, Stroke::new(2.0_f32, text_c), egui::StrokeKind::Outside);
            }
            let words = align::words_of(&l);
            // A label starts where its block comes into view, so one that begins left of
            // the view still shows its text.
            let label_x = |r: Rect| r.left().max(rect.left()) + 4.0;
            if !shows_words(&l) {
                let block = Rect::from_x_y_ranges(b.x_range(), row.clone());
                p.with_clip_rect(block.intersect(rect)).text(pos2(label_x(block), block.center().y), Align2::LEFT_CENTER, l.text.replace('\n', " / "), font.clone(), dim(text_c));
                continue;
            }
            for (wi, w) in words.iter().enumerate() {
                let r = Rect::from_x_y_ranges(x_of(w.start)..=x_of(w.end).max(x_of(w.start) + 1.0), row.clone()).shrink2(vec2(0.5, 0.0));
                let current = current_line && now.is_some_and(|(_, n)| n == Some(wi));
                if current {
                    p.rect_filled(r, 3.0, theme::with_alpha(tokens.playhead, 0.45));
                } else {
                    p.rect_filled(r, 3.0, dim(fill));
                }
                if provisional {
                    let c = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
                    p.extend(egui::Shape::dashed_line(&c, Stroke::new(1.0_f32, dim(stroke)), 3.0, 3.0));
                } else {
                    p.rect_stroke(r, 3.0, Stroke::new(1.0_f32, dim(stroke)), egui::StrokeKind::Inside);
                }
                if selected.is_some_and(|s| (s.line, s.word) == (li, wi)) && !locked {
                    p.rect_stroke(r.expand(1.0), 3.0, Stroke::new(2.0_f32, text_c), egui::StrokeKind::Outside);
                }
                let tc = if provisional { tokens.text_dim } else { text_c };
                p.with_clip_rect(r.shrink(2.0).intersect(rect)).text(pos2(label_x(r), r.center().y), Align2::LEFT_CENTER, &w.text, font.clone(), dim(tc));
            }
        }
        if let Some(at) = playhead {
            let x = x_of(at);
            p.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], Stroke::new(1.5_f32, tokens.playhead));
        }

        // Guide lines down through the waveform: the edge being dragged, or the selected
        // word's start and end, the one the arrows move solid.
        let gp = ui.painter_at(wave);
        let (top, bottom) = (rect.top(), wave.bottom());
        let edges: Vec<(f64, bool, &str)> = match &lane.drag {
            Some(Drag { word, part, preview: Some((l, ..)), .. }) => {
                let ws = align::words_of(l);
                match part {
                    Part::Start => vec![(ws[*word].start, true, "start")],
                    Part::End => vec![(ws[*word].end, true, "end")],
                    Part::Word => vec![(ws[*word].start, true, "start"), (ws[*word].end, true, "end")],
                    Part::Line => vec![(l.start, true, "start"), (l.end, true, "end")],
                }
            }
            _ => match selected.filter(|_| !locked) {
                Some(s) => {
                    let w = &align::words_of(&lines[s.line])[s.word];
                    vec![(w.start, s.grip != Grip::End, "start"), (w.end, s.grip != Grip::Start, "end")]
                }
                None => vec![],
            },
        };
        for (at, solid, tag) in edges {
            let x = x_of(at);
            if (wave.left()..=wave.right()).contains(&x) {
                guide(&gp, x, top, bottom, solid, tag);
            }
        }
        lane.onsets = Some(found);
    }

    /// Under the editor's other controls, for a tile with subtitles: the selected word's
    /// text, what the arrows move, and the steps for it and its line.
    pub(super) fn subtitle_tools(&self, ui: &mut Ui, t: &Tile, lane: &mut LaneState, cursor: Option<f64>, locked: bool, actions: &mut Vec<Action>) {
        let id = t.id;
        let playhead = self.ctl.statuses.get(&id).map(|st| st.from + st.position * st.speed);
        let selected = self.editor_word(t);
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("SUBTITLES").small().color(ui.visuals().weak_text_color()));
            if let Some(a) = super::transcribe_ui::transcribe_button(ui, &self.ctl, t, locked) {
                actions.push(a);
            }
            if t.subtitles.is_empty() {
                ui.weak("None yet: Transcribe listens to the sound and writes them, words timed, or type one below");
                return;
            }
            let Some(sel) = selected else {
                ui.weak("Click a word in the subtitle lane at the top of the waveform to align or retype it, or a line's bar to move or delete it");
                return;
            };
            let (li, wi) = (sel.line, sel.word);
            let line = &t.subtitles[li];
            let words = align::words_of(line);
            let state = match (line.words.is_empty(), line.aligned) {
                (true, _) => "a guess",
                (false, false) => "as timed",
                (false, true) => "placed by hand",
            };
            let made = if line.added { ", added here" } else { "" };
            ui.weak(format!("Line {}, word {} of {} ({state}{made})", li + 1, wi + 1, words.len()));
            let why = |other: &'static str| if locked { tips::LOCKED_WHY } else { other };
            ui.add_enabled_ui(!locked, |ui| {
                if let Some(text) = text_field(ui, egui::Id::unique(("subtitle-word", id, li, wi)), "Subtitle word", &words[wi].text, 110.0, "The word: fix its spelling and press Enter. A space splits it in two, its time shared out by length") {
                    actions.push(inv!("tile.subtitleWordText", {"tile": id, "line": li + 1, "word": wi + 1, "text": text}).into());
                }
            });
            ui.separator();
            // What the arrow keys move.
            ui.weak("Arrows move");
            for (grip, glyph, name, tip) in [
                (Grip::Start, Glyph::GripStart, "Arrows move the word's start", "The arrow keys move the word's start (Up/Down switch)"),
                (Grip::Word, Glyph::GripWord, "Arrows move the whole word", "The arrow keys move the whole word, keeping its length (Up/Down switch)"),
                (Grip::End, Glyph::GripEnd, "Arrows move the word's end", "The arrow keys move the word's end (Up/Down switch)"),
            ] {
                let b = icons::button(ui, glyph, name, !locked, sel.grip == grip);
                let on = sel.grip == grip;
                b.widget_info(|| egui::WidgetInfo::selected(egui::Role::RadioButton, !locked, on, name));
                if theme::hint_or(b, format!("{tip}. {KEYS}"), tips::LOCKED_WHY).clicked() {
                    actions.push(inv!("view.editorWord", {"grip": grip.name()}).into());
                    lane.focus = Some((li, wi));
                }
            }
            ui.separator();
            let at = playhead.or(cursor);
            let tap = icons::button(ui, Glyph::StartAtPlayhead, "Start at Playhead", at.is_some() && !locked, false);
            if theme::hint_or(tap, "Start this word where it's playing (or at the cursor), then select the next word: Enter on the selected word does the same", why("Play the tile, or click the waveform to place the cursor, first")).clicked() {
                if let Some(at) = at {
                    actions.push(inv!("tile.subtitleTap", {"tile": id, "line": li + 1, "word": wi + 1, "at": at}).into());
                    if let Some((nl, nw)) = next_word(&t.subtitles, li, wi) {
                        actions.push(inv!("view.editorWord", {"line": nl + 1, "word": nw + 1}).into());
                        lane.focus = Some((nl, nw));
                    }
                }
            }
            let join = icons::button(ui, Glyph::JoinNext, "Join Next", wi + 1 < words.len() && !locked, false);
            if theme::hint_or(join, "Join this word and the next into one", why("It's the line's last word")).clicked() {
                actions.push(inv!("tile.subtitleWordJoin", {"tile": id, "line": li + 1, "word": wi + 1}).into());
            }
            let auto = ui.add_enabled(!locked, theme::small(egui::Button::new("Auto-Align")));
            if theme::hint_or(theme::name(auto, "Auto-Align Line"), "Suggest where the line's words go: each starts where a sound rises inside the line (or they're spread evenly when there are fewer rises than words). Then fix any that are wrong", tips::LOCKED_WHY).clicked() {
                actions.push(inv!("tile.subtitleAutoAlign", {"tile": id, "line": li + 1}).into());
            }
            let busy = self.ctl.transcriber.busy(id).is_some();
            let align = icons::button(ui, Glyph::AlignWords, "Align Words", !locked && !line.aligned && !busy, false);
            let align_why = if locked { tips::LOCKED_WHY } else if busy { "It's being transcribed" } else { "The line was placed by hand: Reset Line first" };
            if theme::hint_or(align, format!("Align Words: {}", super::transcribe_ui::ALIGN_TIP), align_why).clicked() {
                actions.push(inv!("tile.alignWords", {"tile": id, "line": li + 1}).into());
            }
            let reset = icons::button(ui, Glyph::Reset, "Reset Line", line.source_timing.is_some() && !locked, false);
            if theme::hint_or(reset, "Reset Line: put the line's timing back as the subtitles gave it", why("The line hasn't been changed here")).clicked() {
                actions.push(inv!("tile.subtitleReset", {"tile": id, "line": li + 1}).into());
            }
            let resplit = icons::button(ui, Glyph::SplitAgain, "Split Again", !locked, false);
            if theme::hint_or(resplit, "Split Again: drop the line's word timings and spread its words evenly over it", tips::LOCKED_WHY).clicked() {
                actions.push(inv!("tile.subtitleText", {"tile": id, "line": li + 1}).into());
            }
            let delete = icons::button(ui, Glyph::DeleteLine, "Delete Line", !locked, false);
            if theme::hint_or(delete, "Delete Line: remove this whole line (Delete or Backspace on its bar in the lane does the same; Undo puts it back)", tips::LOCKED_WHY).clicked() {
                actions.push(inv!("tile.subtitleDelete", {"tile": id, "line": li + 1}).into());
            }
        });
        // The line's text (retyping it splits it into words again), and the keys.
        if let Some(sel) = selected {
            ui.horizontal_wrapped(|ui| {
                ui.add_enabled_ui(!locked, |ui| {
                    let line = &t.subtitles[sel.line];
                    let tip = "The line's text: change it and press Enter to split it into words again (their timing starts over, spread evenly over the line)";
                    if let Some(text) = text_field(ui, egui::Id::unique(("subtitle-line", t.id, sel.line)), "Subtitle line", &line.text.replace('\n', " / "), 300.0, tip) {
                        actions.push(inv!("tile.subtitleText", {"tile": t.id, "line": sel.line + 1, "text": text.replace(" / ", "\n")}).into());
                    }
                });
                if lane.line_has_focus == Some(sel.line) {
                    ui.weak(format!("The line: {LINE_KEYS}."));
                } else {
                    ui.weak(format!("{KEYS}."));
                }
            });
        }
        self.new_line_row(ui, t, cursor, locked, actions);
    }
}

impl StingerApp {
    /// The row for a new line: its text, and Add Line, which puts it over the selected
    /// stretch, or from the playhead or the cursor (D-301).
    fn new_line_row(&self, ui: &mut Ui, t: &Tile, cursor: Option<f64>, locked: bool, actions: &mut Vec<Action>) {
        let id = t.id;
        let playhead = self.ctl.statuses.get(&id).map(|st| st.from + st.position * st.speed);
        let span = self.ctl.editor_selection.filter(|(a, b)| b > a);
        let at = playhead.or(cursor);
        let draft_id = egui::Id::unique(("subtitle-new-line", id));
        ui.horizontal_wrapped(|ui| {
            ui.add_enabled_ui(!locked, |ui| {
                let mut text = ui.ctx().data(|d| d.get_temp::<String>(draft_id)).unwrap_or_default();
                let place = match (span, at) {
                    (Some((a, b)), _) => format!("over the selection ({} to {})", format_timecode(a), format_timecode(b)),
                    (None, Some(at)) => format!("from {} ({})", if playhead.is_some() { "the playhead" } else { "the cursor" }, format_timecode(at)),
                    (None, None) => "where you select a stretch, play, or place the cursor".to_owned(),
                };
                let edit = egui::TextEdit::singleline(&mut text).id(draft_id.with("field")).hint_text("New line…").desired_width(220.0);
                let tip = format!("A new subtitle line: type what's said and press Enter (or Add Line) to put it {place}. Its words start where sounds rise inside it; then align them as any other line");
                let resp = theme::hint_or(theme::name(theme::text_field(ui, edit), "New subtitle line"), tip, tips::LOCKED_WHY);
                let entered = resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                let ready = !text.trim().is_empty() && (span.is_some() || at.is_some());
                let add = icons::button(ui, Glyph::AddLine, "Add Line", ready && !locked, false);
                let why = if locked {
                    tips::LOCKED_WHY
                } else if text.trim().is_empty() {
                    "Type the new line's text first"
                } else {
                    "Select a stretch of the waveform, play the tile, or click the waveform to place the cursor first"
                };
                let clicked = theme::hint_or(add, format!("Add Line: a new subtitle line {place}, with the text typed beside it"), why).clicked();
                if (clicked || entered) && ready {
                    let text = text.trim().to_owned();
                    actions.push(match span {
                        Some((a, b)) => inv!("tile.subtitleAdd", {"tile": id, "text": text, "start": a, "end": b}),
                        None => inv!("tile.subtitleAdd", {"tile": id, "text": text, "at": at}),
                    }.into());
                    ui.ctx().data_mut(|d| d.remove::<String>(draft_id));
                } else {
                    ui.ctx().data_mut(|d| d.insert_temp(draft_id, text));
                }
            });
        });
    }
}

/// Text typed over: gives the new text when Enter or a click elsewhere ends the edit
/// (Esc, or empty text, puts the old one back).
fn text_field(ui: &mut Ui, id: egui::Id, name: &str, current: &str, width: f32, tip: &str) -> Option<String> {
    let draft = id.with("draft");
    let mut text = ui.ctx().data(|d| d.get_temp::<String>(draft)).unwrap_or_else(|| current.to_owned());
    let edit = egui::TextEdit::singleline(&mut text).id(id).desired_width(width);
    let resp = theme::hint(theme::name(theme::text_field(ui, edit), name), tip);
    if resp.has_focus() {
        ui.ctx().data_mut(|d| d.insert_temp(draft, text));
        return None;
    }
    ui.ctx().data_mut(|d| d.remove::<String>(draft));
    let new = text.trim();
    (resp.lost_focus() && !ui.input(|i| i.key_pressed(Key::Escape)) && !new.is_empty() && new != current).then(|| new.to_owned())
}
