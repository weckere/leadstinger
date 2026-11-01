//! The Panel theme: a retro sci-fi control-panel look, drawn in docs/design/panel.py (an
//! original composition).
//!
//! Layout is absolute, as on such panels: a top elbow sweeping from the left column into the
//! top bar, the title in a gap in that bar, the readouts under it, coloured blocks down
//! the column, and a bottom elbow whose bar is made of the board's controls (Sidebar, Add
//! Sound, Grid, List, Inspector) with the status text in the gaps after it. With the
//! sidebar hidden the column narrows to a slim edge. Everything is placed relative to the
//! area left after the (Linux) menu bar.

use super::*;
use crate::theme::{condensed, condensed_semi};
use eframe::egui::{Id, LayerId, Pos2, Vec2};

pub const M: f32 = 8.0; // outer margin
pub const COL: f32 = 176.0; // left column width
const COL_SLIM: f32 = 52.0; // the column with the sidebar hidden
pub const TOP: f32 = 34.0; // top bar thickness
pub const ELBOW_Y: f32 = 200.0; // where the top elbow's column part ends
const R_OUT: u8 = 64;
const R_IN: f32 = 30.0;
pub const BOT: f32 = 26.0; // bottom bar thickness
const BOT_COL: f32 = 48.0; // height of the bottom elbow's column part above its bar
const R_OUT_B: u8 = 44;
const R_IN_B: f32 = 22.0;
const GAP: f32 = 5.0;
/// How far a set block's code and name sit from its top and bottom edges: clear of its
/// focus outline ([`INSIDE_REACH`]).
const SET_EDGE: f32 = INSIDE_REACH + 2.0;
/// Panel sizes that line up with the frame.
pub const TOP_H: f32 = 168.0;
pub const BOTTOM_H: f32 = M + BOT + 6.0;

/// The left panel's width, with the sidebar shown or hidden.
pub fn left_w(sidebar: bool) -> f32 {
    M + if sidebar { COL } else { COL_SLIM } + 32.0
}

/// Narrower than this (past the column), the header can't hold the readouts, the title
/// column and the right column side by side, and switches to its compact layout: the
/// title shrinks into the top bar beside Settings. (The readouts need 320 px from x0, then
/// a 16 px gap, and the title column starts 488 px from the right edge.)
const WIDE_MIN: f32 = 40.0 + 320.0 + 16.0 + 488.0;
/// The compact title's size: its capitals are as tall as the top bar (Antonio's sit
/// `theme::condensed_cap_top` below the top of the line), so it clears the pills below.
const TITLE_COMPACT: f32 = 38.0;
/// Width the wide title may take before the Settings cap.
const TITLE_ROOM: f32 = 254.0;

/// The theme's title (src/brand.rs `PANEL_LOGO`) in its colours: the middle part in peach.
fn logo_parts(ctx: &egui::Context) -> [(&'static str, Color32); 3] {
    let pal = theme::palette(ctx);
    let (a, b, c) = crate::brand::PANEL_LOGO;
    [(a, pal.amber), (b, pal.peach), (c, pal.amber)]
}

/// The frame pieces that depend on the window width.
struct Geo {
    o: Pos2,
    size: Vec2,
    /// The left column's width: [`COL`], or slim with the sidebar hidden.
    col: f32,
    /// The full layout (see [`WIDE_MIN`]).
    wide: bool,
    /// Where the big title and the readouts under it start (wide layout only).
    title_x: f32,
    /// The Settings end cap's left edge.
    cap_x: f32,
    /// The right-hand column (Stop All, meter): its left edge; it runs to the margin.
    rcol_x: f32,
    /// The bottom bar's blocks (the board's controls): id, label and x range.
    blocks: Vec<(&'static str, &'static str, f32, f32)>,
    /// Where the bottom bar ends: after its last block.
    bottom_bar_end: f32,
}

/// The bottom bar's blocks: id, label, and the shorter label for a narrow window.
/// Grid and List are icons, their words in the hover text (D-216).
const BLOCKS: [(&str, &str, &str); 5] = [("sidebar", "Setlist", "Setlist"), ("add", "Add Sound", "Add"), ("grid", "Grid", "Grid"), ("list", "List", "List"), ("inspector", "Inspector", "Inspector")];

/// An icon block's width (Grid, List), and the room an arrow takes beside a label
/// (Setlist, Inspector): the arrow and the gap before the lettering (D-216).
const ICON_BLOCK_W: f32 = 44.0;
const ARROW: f32 = 10.0;
const ARROW_GAP: f32 = 7.0;

/// The blocks drawn as an icon alone.
fn icon_block(id: &str) -> bool {
    matches!(id, "grid" | "list")
}

/// The blocks with an arrow beside their label.
fn arrow_block(id: &str) -> bool {
    matches!(id, "sidebar" | "inspector")
}

/// The bottom bar blocks' lettering: small enough that its capitals clear the ring of a
/// block that's on, in the bar's [`BOT`] (`bar_lettering_clears_the_ring`).
fn block_font() -> FontId {
    condensed(16.0)
}

/// The black ring inside a block or pill that's on: how far in from the edge, and how wide.
const RING_INSET: f32 = 2.0;
const RING_W: f32 = 2.0;

fn geo(ctx: &egui::Context, area: Rect, sidebar: bool) -> Geo {
    let w = area.width();
    let col = if sidebar { COL } else { COL_SLIM };
    let wide = w >= M + col + WIDE_MIN;
    let title_x = w - 488.0;
    let rcol_x = w - M - if wide { 210.0 } else { (w * 0.22).clamp(160.0, 210.0) };
    // The blocks start past the bottom elbow's inner curve; a narrow window gets the
    // short labels, so the status text keeps room after them.
    let x0 = M + col + R_IN_B + GAP;
    let lay = |short: bool| {
        let mut x = x0;
        let mut out = Vec::new();
        for (id, long, brief) in BLOCKS {
            let label = if short { brief } else { long };
            let text = ctx.fonts_mut(|f| f.layout_no_wrap(label.to_uppercase(), block_font(), Color32::BLACK).size().x);
            let bw = if icon_block(id) {
                ICON_BLOCK_W
            } else {
                (text + 24.0 + if arrow_block(id) { ARROW + ARROW_GAP } else { 0.0 }).ceil()
            };
            out.push((id, label, x, x + bw));
            x += bw + GAP;
        }
        out
    };
    let mut blocks = lay(false);
    if blocks.last().is_some_and(|b| b.3 > w * 0.58) {
        blocks = lay(true);
    }
    let bottom_bar_end = blocks.last().map_or(x0, |b| b.3);
    Geo {
        o: area.min,
        size: area.size(),
        col,
        wide,
        title_x,
        cap_x: if wide { title_x + 268.0 } else { rcol_x },
        rcol_x,
        blocks,
        bottom_bar_end,
    }
}

/// The bottom bar's middle block, if there's room for it: its left edge.
fn bottom_mid(g: &Geo) -> Option<f32> {
    let mid = g.bottom_bar_end + 250.0;
    (mid + 60.0 < g.size.x - M - 300.0).then_some(mid)
}

fn r(g: &Geo, x0: f32, y0: f32, x1: f32, y1: f32) -> Rect {
    Rect::from_min_max(g.o + vec2(x0, y0), g.o + vec2(x1, y1))
}

/// Panel lettering: uppercase, black by default, anchored where asked.
/// A pad's key as Panel's code: a digit as two (`01`), a letter or mark as it is, with
/// Option before it (`⌥01`, `ALT 01`).
fn panel_code(k: PadKey) -> String {
    let base = if k.key.is_ascii_digit() { format!("0{}", k.key) } else { k.key.to_ascii_uppercase().to_string() };
    match (k.alt, cfg!(target_os = "macos")) {
        (false, _) => base,
        (true, true) => format!("⌥{base}"),
        (true, false) => format!("ALT {base}"),
    }
}

fn caps_text(p: &egui::Painter, pos: Pos2, anchor: Align2, text: &str, font: FontId, color: Color32) -> Rect {
    p.text(pos, anchor, text.to_uppercase(), font, color)
}

/// A control's label, centred in its block, pill or cap both ways. The fonts are set up
/// so a line's capitals sit in its middle (`theme::caps_offset`), so centring the line
/// centres the lettering by eye (t-m22-panel-polish).
fn block_label(p: &egui::Painter, rect: Rect, text: &str, font: FontId, color: Color32) -> Rect {
    caps_text(p, rect.center(), Align2::CENTER_CENTER, text, font, color)
}

/// The black ring that marks a block or pill as on, so "on" isn't told by colour alone.
fn on_ring(p: &egui::Painter, rect: Rect, corner: f32) {
    let rect = rect.shrink(RING_INSET);
    p.rect_stroke(rect, (corner - RING_INSET).max(0.0), Stroke::new(RING_W, theme::palette(p.ctx()).black), StrokeKind::Inside);
}

/// A pill control at an absolute rect. `on` shows it in the active (amber) colour.
fn pill(ui: &mut Ui, rect: Rect, id: &str, label: &str, fill: Color32, on: bool) -> egui::Response {
    let pal = theme::palette(ui.ctx());
    let resp = ui.interact(rect, Id::unique(("panel-pill", id)), Sense::click());
    theme::focus_corners(&resp, CornerRadius::same((rect.height() / 2.0) as u8));
    let base = if on { pal.amber } else { fill };
    let fill = if resp.is_pointer_button_down_on() {
        theme::shade(base, 0.85)
    } else if resp.hovered() {
        theme::lighten(base, 0.18)
    } else {
        base
    };
    let p = ui.painter();
    p.rect_filled(rect, rect.height() / 2.0, fill);
    if on {
        on_ring(p, rect, rect.height() / 2.0);
    }
    block_label(p, rect, label, condensed(rect.height() * 0.5), pal.black);
    resp
}

/// The fixed frame. It goes on egui's shared background layer, the one the panels
/// draw on, and is painted before them so it stays underneath.
pub fn paint_frame(ctx: &egui::Context, area: Rect, clip: Rect, subtitle: &str, sidebar: bool) {
    let pal = theme::palette(ctx);
    let g = geo(ctx, area, sidebar);
    let col = g.col;
    let p = ctx.layer_painter(LayerId::background()).with_clip_rect(clip);
    let (w, h) = (g.size.x, g.size.y);
    p.rect_filled(area, 0.0, pal.black);

    // Top elbow: column + bar, rounded outside, concave inside.
    p.rect_filled(r(&g, M, M, M + col, ELBOW_Y), CornerRadius { nw: R_OUT, ..Default::default() }, pal.amber);
    // Title in the bar's gap. Wide: big, with the subtitle under it. Compact: the bar's
    // height, right up against the Settings cap, and no subtitle.
    let bar_end = if g.wide {
        // The brand's name, shrunk if needed to fit before the Settings cap.
        let full: String = logo_parts(ctx).iter().map(|(t, _)| *t).collect();
        let natural = ctx.fonts_mut(|f| f.layout_no_wrap(full.to_uppercase(), condensed(54.0), pal.amber).size().x);
        let size = 54.0 * (TITLE_ROOM / natural).min(1.0);
        let mut at = g.o + vec2(g.title_x, M - 6.0 + (54.0 - size) * 0.45);
        for (part, color) in logo_parts(ctx) {
            if !part.is_empty() {
                at = caps_text(&p, at, Align2::LEFT_TOP, part, condensed(size), color).right_top();
            }
        }
        caps_text(&p, g.o + vec2(g.title_x, M + 56.0), Align2::LEFT_TOP, subtitle, condensed_semi(14.0), theme::panel_dim(ctx));
        g.title_x - 12.0
    } else {
        let font = condensed(TITLE_COMPACT);
        let mut at = g.o + vec2(g.cap_x - 14.0, M - theme::condensed_cap_top() * TITLE_COMPACT);
        for (part, color) in logo_parts(ctx).into_iter().rev() {
            if !part.is_empty() {
                at = caps_text(&p, at, Align2::RIGHT_TOP, part, font.clone(), color).left_top();
            }
        }
        at.x - g.o.x - 14.0
    };
    p.rect_filled(r(&g, M + col - 1.0, M, bar_end.max(M + col + R_IN), M + TOP), 0.0, pal.amber);
    p.rect_filled(r(&g, M + col - 1.0, M + TOP - 1.0, M + col + R_IN, M + TOP + R_IN), 0.0, pal.amber);
    p.circle_filled(g.o + vec2(M + col + R_IN, M + TOP + R_IN), R_IN, pal.black);
    if sidebar {
        caps_text(&p, g.o + vec2(M + col - 12.0, ELBOW_Y - 6.0), Align2::RIGHT_BOTTOM, "Setlist", condensed(26.0), pal.black);
        caps_text(&p, g.o + vec2(M + col - 12.0, ELBOW_Y - 40.0), Align2::RIGHT_BOTTOM, crate::brand::PANEL_TAG, condensed_semi(13.0), pal.black);
    } else {
        // The slim column runs straight down into the bottom elbow.
        p.rect_filled(r(&g, M, ELBOW_Y - 1.0, M + col, h - M - BOT - BOT_COL + 1.0), 0.0, pal.violet);
    }

    // Bottom elbow.
    let yb = h - M;
    p.rect_filled(r(&g, M, yb - BOT - BOT_COL, M + col, yb), CornerRadius { sw: R_OUT_B, ..Default::default() }, pal.violet);
    // The bar runs to its first block (the blocks are the board's controls, drawn with
    // the status bar).
    p.rect_filled(r(&g, M + col - 1.0, yb - BOT, M + col + R_IN_B, yb), 0.0, pal.violet);
    p.rect_filled(r(&g, M + col - 1.0, yb - BOT - R_IN_B, M + col + R_IN_B, yb - BOT + 1.0), 0.0, pal.violet);
    p.circle_filled(g.o + vec2(M + col + R_IN_B, yb - BOT - R_IN_B), R_IN_B, pal.black);
    // Status bar pieces: a middle block and an end cap (the text sits in the gaps).
    if let Some(mid) = bottom_mid(&g) {
        p.rect_filled(r(&g, mid, yb - BOT, mid + 60.0, yb), 0.0, pal.lilac);
    }
    p.rect_filled(r(&g, w - M - 40.0, yb - BOT, w - M, yb), CornerRadius { ne: 13, se: 13, ..Default::default() }, pal.lilac);
}

impl StingerApp {
    pub(super) fn panel_toolbar(&mut self, ui: &mut Ui, area: Rect, actions: &mut Vec<Action>) {
        let pal = theme::palette(ui.ctx());
        let g = geo(ui.ctx(), area, self.ctl.store.library.show_sidebar);
        let x0 = M + g.col + 40.0;
        let y0 = 54.0;
        let hgt = 40.0;

        // Settings as the top bar's end cap; Stop All under it.
        let w = g.size.x;
        let cap = r(&g, g.cap_x, M, w - M, M + TOP);
        let resp = theme::role(ui.interact(cap, Id::unique("panel-settings"), Sense::click()), AxRole::Button, "Settings", Some(self.ctl.show_settings));
        let resp = theme::hint_keys(resp, tips::SETTINGS.0, tips::SETTINGS.1);
        theme::focus_corners(&resp, CornerRadius { ne: 17, se: 17, ..Default::default() });
        let fill = if resp.hovered() { theme::lighten(pal.peach, 0.18) } else { pal.peach };
        ui.painter().rect_filled(cap, CornerRadius { ne: 17, se: 17, ..Default::default() }, fill);
        block_label(ui.painter(), cap, "Settings", condensed(16.0), pal.black);
        if resp.clicked() {
            actions.push(inv!("view.settings", {"open": !self.ctl.show_settings}).into());
        }
        let stop = theme::role(pill(ui, r(&g, g.rcol_x, y0, w - M, y0 + hgt), "stop", "Stop All", pal.red, false), AxRole::Button, "Stop All", None);
        if theme::hint(stop, tips::stop_all(self.ctl.store.library.stop_all_hold))
            .clicked()
        {
            actions.push(inv!("transport.stopAll").into());
        }

        // The readouts, under the top bar (the board's controls are in the bottom bar),
        // and Random (D-171) at their right end, level with Stop All.
        let mut room = if g.wide { g.title_x - 16.0 } else { g.rcol_x - 12.0 };
        // Fade All, Pause All and, in a List set, Previous and Next (D-197), right to left,
        // each only while the readout keeps room.
        let list = self.ctl.current_set().layout == Layout::List;
        let (pause_label, pause_tip, any) = self.pause_all_state();
        let pause_label = pause_label.trim_start_matches(['⏸', '▶', ' ']);
        let blocks: [(&str, f32); 3] = [("fade", 100.0), ("pause", 110.0), ("steps", if list { 40.0 } else { 0.0 })];
        for (which, bw) in blocks {
            if bw == 0.0 || room - x0 < bw + 200.0 {
                continue;
            }
            let rect = r(&g, room - bw, y0, room, y0 + hgt);
            match which {
                "fade" => {
                    let resp = theme::role(pill(ui, rect, "fade-all", "Fade All", pal.amber, false), AxRole::Button, "Fade All", None);
                    if theme::hint(resp, tips::FADE_ALL).clicked() {
                        actions.push(inv!("transport.fadeAll").into());
                    }
                }
                "pause" => {
                    let resp = ui.add_enabled_ui(any, |ui| pill(ui, rect, "pause-all", pause_label, pal.gold, false)).inner;
                    if theme::hint_or(theme::role(resp, AxRole::Button, pause_label, None), pause_tip.as_str(), tips::PAUSE_ALL_WHY).clicked() {
                        actions.push(inv!("transport.pauseAll").into());
                    }
                }
                _ => {
                    let (up, down) = (Rect::from_min_size(rect.min, vec2(bw, 18.0)), Rect::from_min_size(rect.min + vec2(0.0, 22.0), vec2(bw, 18.0)));
                    if theme::hint_keys(theme::role(step_block(ui, up, "cue-prev", true), AxRole::Button, "Previous cue", None), tips::CUE_PREV.0, tips::CUE_PREV.1).clicked() {
                        actions.push(inv!("cue.prev").into());
                    }
                    if theme::hint_keys(theme::role(step_block(ui, down, "cue-next", false), AxRole::Button, "Next cue", None), tips::CUE_NEXT.0, tips::CUE_NEXT.1).clicked() {
                        actions.push(inv!("cue.next").into());
                    }
                }
            }
            room -= bw + 10.0;
        }
        let random_w = 120.0;
        if room - x0 >= random_w + 160.0 {
            let rect = r(&g, room - random_w, y0, room, y0 + hgt);
            let any = self.ctl.can_play_random();
            let resp = ui.add_enabled_ui(any, |ui| pill(ui, rect, "random", "Random", pal.sky, false)).inner;
            if theme::hint_or(theme::role(resp, AxRole::Button, "Random", None), tips::RANDOM, tips::RANDOM_WHY).clicked() {
                actions.push(inv!("tile.random").into());
            }
            room -= random_w + 16.0;
        }
        self.panel_counter(ui, g.o + vec2(x0, y0), room - x0);
        self.panel_meter(ui, r(&g, g.rcol_x, 98.0, w - M, 154.0));
    }

    /// The focus voice's countdown, kept within `max_w`: a long title is cut short, and
    /// the position / length readout goes when it doesn't fit beside the countdown.
    fn panel_counter(&self, ui: &mut Ui, at: Pos2, max_w: f32) {
        let pal = theme::palette(ui.ctx());
        let read_out = ui.interact(Rect::from_min_size(at, vec2(max_w, 60.0)), Id::unique("panel-counter"), Sense::hover());
        theme::role(read_out, egui::accesskit::Role::Label, self.counter_label(), None);
        let p = ui.painter();
        let dim = theme::panel_dim(ui.ctx());
        let sounding = self.ctl.statuses.values().filter(|s| s.state != PlayState::Paused).count();
        let head = |text: &str| {
            let galley = ui.fonts_mut(|f| {
                let mut job = egui::text::LayoutJob::simple_singleline(text.to_uppercase(), condensed_semi(15.0), dim);
                job.wrap.max_width = max_w;
                job.wrap.max_rows = 1;
                f.layout_job(job)
            });
            p.galley(at, galley, dim);
        };
        let rec = self.ctl.recording_view();
        let error = Tokens::get(ui.ctx()).error;
        match self.focus_voice() {
            Some((id, s)) => {
                let title = self.ctl.store.library.tile(id).map(|t| t.title.clone()).unwrap_or_default();
                let mut text = if sounding > 1 { format!("{title}  ·  {sounding} playing") } else { title };
                if let Some(r) = &rec {
                    text.push_str(&format!("  ·  rec {}", format_clock(r.seconds, false)));
                }
                head(&text);
                let color = if s.state == PlayState::Paused { pal.gold } else { pal.text };
                let big = theme::paint_tabular(p, at + vec2(0.0, 14.0), Align2::LEFT_TOP, &format!("−{}", format_clock(s.remaining(), true)), condensed(40.0), color);
                let small = format!("{} / {}", format_clock(s.position, true), format_clock(s.length, true));
                let small_w = ui.fonts_mut(|f| f.layout_no_wrap(small.clone(), condensed_semi(17.0), dim).size().x);
                if big.right() + 14.0 + small_w <= at.x + max_w {
                    theme::paint_tabular(p, pos2(big.right() + 14.0, big.bottom() - 6.0), Align2::LEFT_BOTTOM, &small, condensed_semi(17.0), dim);
                }
            }
            None if rec.is_some() => {
                let r = rec.as_ref().expect("recording");
                let title = super::rec_title(&r.title);
                head(&match &r.input {
                    Some(i) => format!("{title}  ·  {i}"),
                    None => title,
                });
                theme::paint_tabular(p, at + vec2(0.0, 14.0), Align2::LEFT_TOP, &format_clock(r.seconds, true), condensed(40.0), error);
            }
            None => {
                head("Ready");
                let idle = if theme::is_high_contrast(ui.ctx()) { dim } else { theme::shade(pal.peach, 0.4) };
                theme::paint_tabular(p, at + vec2(0.0, 14.0), Align2::LEFT_TOP, "0:00.0", condensed(40.0), idle);
            }
        }
    }

    /// Segmented bar-graph meter: two rows of 16 segments, gold to amber to red, with the
    /// master volume as a draggable peach marker.
    fn panel_meter(&mut self, ui: &mut Ui, rect: Rect) {
        let pal = theme::palette(ui.ctx());
        let tip = tips::master(self.ctl.store.library.master_volume).replace("the clip light", "the last segment");
        let (value, text) = self.master_info();
        let (resp, steps) = theme::slider_info(ui.interact(rect, Id::unique("panel-meter"), Sense::click_and_drag()), "Master volume", value, text);
        let steps = steps + tips::arrow_steps(&resp);
        self.master_steps(steps);
        let resp = theme::hint(resp, tip);
        const SEGS: usize = 16;
        let pitch = (rect.width() / SEGS as f32).floor();
        let seg_w = pitch - 3.0;
        let rows = Rect::from_min_size(rect.min + vec2(0.0, 26.0), vec2(pitch * SEGS as f32, 28.0));
        if let Some(pos) = resp.interact_pointer_pos() {
            // The last segment clears a clip: at least a 24 px target.
            let w = pitch.max(theme::TARGET);
            let last = Rect::from_min_size(pos2(rows.right() - w, rows.top()), vec2(w, rows.height()));
            if resp.clicked() && last.contains(pos) {
                self.meter.clip = false;
            } else if resp.dragged() || resp.clicked() {
                let frac = ((pos.x - rows.left()) / rows.width()).clamp(0.0, 1.0);
                let v = if frac <= 0.0 { 0.0 } else { db_to_gain(meter_db(frac)) };
                self.run(inv!("master.volume", {"volume": v}));
            }
        }
        let p = ui.painter();
        let v = self.ctl.store.library.master_volume;
        let db = if v <= 0.0 { "−∞".to_owned() } else { format!("{:.1}", gain_to_db(v)).replace('-', "−") };
        caps_text(p, rect.left_top(), Align2::LEFT_TOP, &format!("Master  {db}"), condensed_semi(15.0), theme::panel_dim(ui.ctx()));
        if self.meter.clip {
            // In words too: a lit segment differs from an unlit one only in colour.
            caps_text(p, pos2(rows.right() - 3.0, rect.top()), Align2::RIGHT_TOP, "Clip", condensed_semi(15.0), pal.red);
        }
        let (green_end, yellow_end) = (meter_frac(-12.0), meter_frac(-3.0));
        // The designed look's unlit segment, in high contrast too.
        let unlit = theme::look(ui.ctx()).dark.meter_off;
        for ch in 0..2 {
            let lit = meter_frac(self.meter.ch[ch].level);
            let hold = meter_frac(self.meter.ch[ch].hold);
            for i in 0..SEGS {
                let f0 = i as f32 / SEGS as f32;
                let color = if f0 >= yellow_end { pal.red } else if f0 >= green_end { pal.amber } else { pal.gold };
                let on = f0 < lit || (hold > 0.0 && (hold - f0).abs() < 0.5 / SEGS as f32) || (i == SEGS - 1 && self.meter.clip);
                let seg = Rect::from_min_size(pos2(rows.left() + i as f32 * pitch, rows.top() + ch as f32 * 16.0), vec2(seg_w, 12.0));
                p.rect_filled(seg, 1.0, if on { color } else { unlit });
            }
        }
        let vx = rows.left() + meter_frac(gain_to_db(v)) * rows.width();
        p.add(egui::Shape::convex_polygon(
            vec![pos2(vx - 5.0, rows.top() - 9.0), pos2(vx + 5.0, rows.top() - 9.0), pos2(vx, rows.top() - 2.0)],
            pal.peach,
            Stroke::NONE,
        ));
    }

    pub(super) fn panel_sets(&mut self, ui: &mut Ui, area: Rect, actions: &mut Vec<Action>) {
        let pal = theme::palette(ui.ctx());
        let g = geo(ui.ctx(), area, true);
        let colors = theme::look(ui.ctx()).sets;
        let top = ELBOW_Y + GAP;
        let bottom = g.size.y - M - BOT - BOT_COL - GAP;
        const NEW_H: f32 = 40.0; // the "+ New Set" block, always below the list
        const MAX_H: f32 = 60.0;
        const MIN_H: f32 = 34.0; // the name alone still reads at this height
        let selected = self.ctl.store.library.selected_set;
        let n_sets = self.ctl.store.library.sets.len();
        // Every set stays reachable: blocks shrink to fit the column, and when even the
        // smallest don't fit, the column scrolls (wheel) and follows the current set.
        let list_bottom = (bottom - NEW_H - GAP).max(top + MIN_H);
        let avail = list_bottom - top;
        let n = n_sets.max(1) as f32;
        let block_h = ((avail - (n - 1.0) * GAP) / n).clamp(MIN_H, MAX_H);
        let pitch = block_h + GAP;
        let content_h = n * pitch - GAP;
        let max_scroll = (content_h - avail).max(0.0);
        let list_rect = r(&g, M, top, M + COL, list_bottom);
        let scroll_id = Id::unique("panel-sets-scroll");
        let follow_id = Id::unique("panel-sets-follow");
        let (mut scroll, followed) = ui.ctx().data(|d| (d.get_temp::<f32>(scroll_id).unwrap_or(0.0), d.get_temp::<Option<Uuid>>(follow_id)));
        if max_scroll > 0.0 && ui.rect_contains_pointer(list_rect) {
            scroll -= ui.input(|i| i.smooth_scroll_delta.y);
        }
        // When the current set changes (or on the first frame), bring it fully into view.
        if followed != Some(selected) {
            if let Some(i) = selected.and_then(|id| self.ctl.store.library.set_index(id)) {
                let (y0, y1) = (i as f32 * pitch, i as f32 * pitch + block_h);
                if y0 < scroll {
                    scroll = y0;
                } else if y1 > scroll + avail {
                    scroll = y1 - avail;
                }
            }
        }
        let scroll = scroll.clamp(0.0, max_scroll);
        ui.ctx().data_mut(|d| {
            d.insert_temp(scroll_id, scroll);
            d.insert_temp(follow_id, selected);
        });
        let compact = block_h < 46.0; // too short for the code line: the name alone
        let clip = ui.painter().with_clip_rect(list_rect.intersect(ui.clip_rect()));
        let mut rename_rect = None;
        for (i, set) in self.ctl.store.library.sets.iter().enumerate() {
            let y = top + i as f32 * pitch - scroll;
            let rect = r(&g, M, y, M + COL, y + block_h);
            let visible = rect.intersect(list_rect);
            if visible.height() <= 0.0 {
                continue;
            }
            let resp = ui.interact(visible, Id::unique(("panel-set", set.id)), Sense::click());
            // Pads dragged over it move there when let go (D-219).
            let drop = self.drop_on_set(&resp, set.id, actions);
            let on = Some(set.id) == selected;
            let base = colors[i % colors.len()];
            let fill = if resp.hovered() || drop { theme::lighten(base, 0.15) } else { base };
            let p = &clip;
            p.rect_filled(rect, 0.0, fill);
            if on {
                p.rect_filled(Rect::from_min_size(rect.min, vec2(10.0, block_h)), 0.0, pal.black);
            }
            // The keyboard focus (a click leaves it there) gets a pad's white outline,
            // inside the block, which sits too close to its neighbours for one outside
            // (t-m22-panel-outline-rows); the black notch still marks the current set.
            // Read from memory, as the ring is: egui's `has_focus` is false while the
            // window is in the background.
            if ui.memory(|m| m.has_focus(resp.id)) {
                theme::own_focus_cue(ui.ctx(), resp.id);
                paint_selection_inside(p, rect, CornerRadius::ZERO);
            }
            if drop {
                p.rect_stroke(rect.shrink(2.0), 0.0, Stroke::new(4.0_f32, pal.black), egui::StrokeKind::Inside);
            }
            let playing = set.tiles.iter().any(|t| self.ctl.statuses.contains_key(&t.id));
            // The lettering keeps as far from the edge as from that outline.
            if !compact {
                let code = format!("{:02}-{:03}{}", i + 1, set.tiles.len(), if playing { "  ·  LIVE" } else { "" });
                caps_text(p, rect.left_top() + vec2(18.0, SET_EDGE), Align2::LEFT_TOP, &code, condensed_semi(13.0), pal.black);
            } else if playing {
                caps_text(p, rect.left_center() + vec2(18.0, 0.0), Align2::LEFT_CENTER, "LIVE", condensed_semi(13.0), pal.black);
            }
            if matches!(&self.renaming, Some((id, _, _)) if *id == set.id) {
                rename_rect = Some(rect);
            } else {
                let size = match (compact, on) {
                    (true, _) => 17.0,
                    (false, true) => 21.0,
                    (false, false) => 19.0,
                };
                let galley = ui.fonts_mut(|f| {
                    let mut job = egui::text::LayoutJob::simple_singleline(set.name.to_uppercase(), condensed(size), pal.black);
                    job.wrap.max_width = COL - if compact && playing { 60.0 } else { 30.0 };
                    job.wrap.max_rows = 1;
                    f.layout_job(job)
                });
                let y_text = if compact { rect.center().y - galley.size().y / 2.0 } else { rect.bottom() - SET_EDGE - galley.size().y };
                p.galley(pos2(rect.right() - 12.0 - galley.size().x, y_text), galley, pal.black);
            }
            if set.locked {
                // The lock at the block's top right, in black like its lettering.
                super::pad_options::paint_lock(p, rect.right_top() + vec2(-16.0, 14.0), 14.0, true, pal.black);
            }
            let resp = theme::role(resp, AxRole::Button, tips::set_label(&set.name, i, n_sets, set.tiles.len(), playing, set.locked), Some(on));
            let resp = theme::hint(resp, tips::set_row(&set.name, set.tiles.len(), playing, set.locked));
            if resp.clicked() {
                actions.extend(self.show_set(set.id));
            }
            if resp.double_clicked() {
                actions.push(Action::StartRename(set.id));
            }
            self.set_row_menu(&resp, set.id, n_sets, set.locked, actions);
        }
        // Amber edge bars say there are more sets above or below.
        if scroll > 0.5 {
            ui.painter().rect_filled(Rect::from_min_size(list_rect.min, vec2(COL, 4.0)), 0.0, pal.amber);
        }
        if scroll < max_scroll - 0.5 {
            ui.painter().rect_filled(Rect::from_min_size(pos2(list_rect.left(), list_rect.bottom() - 4.0), vec2(COL, 4.0)), 0.0, pal.amber);
        }
        let y = (top + content_h).min(list_bottom) + GAP;
        if let (Some(rect), Some((_, draft, focused))) = (rename_rect, &mut self.renaming) {
            let edit = ui.put(rect.shrink2(vec2(14.0, 16.0)).translate(vec2(4.0, 8.0)), |ui: &mut Ui| theme::text_field(ui, egui::TextEdit::singleline(draft)));
            let edit = theme::hint(theme::name(edit, "Set name"), tips::RENAME_FIELD);
            if !*focused {
                edit.request_focus();
                *focused = true;
            }
            if edit.lost_focus() {
                let cancel = ui.input(|i| i.key_pressed(Key::Escape));
                actions.push(if cancel { Action::CancelRename } else { Action::CommitRename });
            }
        }
        let new = r(&g, M, y, M + COL, y + NEW_H);
        let new = theme::role(pill_block(ui, new, "new-set", "+ New Set", pal.sky), AxRole::Button, "New Set", None);
        if theme::hint_keys(new, tips::NEW_SET.0, tips::NEW_SET.1).clicked() {
            actions.push(inv!("set.create").into());
        }
        let y = y + NEW_H + GAP;
        // The rest of the column runs on into the bottom elbow.
        let elbow_top = g.size.y - M - BOT - BOT_COL;
        if elbow_top - y > 0.0 {
            ui.painter().rect_filled(r(&g, M, y, M + COL, elbow_top + 1.0), 0.0, pal.violet);
        }
    }

    pub(super) fn panel_status(&mut self, ui: &mut Ui, area: Rect, actions: &mut Vec<Action>) {
        let pal = theme::palette(ui.ctx());
        let g = geo(ui.ctx(), area, self.ctl.store.library.show_sidebar);
        self.panel_bar_blocks(ui, &g, actions);
        let yc = g.size.y - M - BOT / 2.0;
        let playing = self.ctl.statuses.values().filter(|s| s.state != PlayState::Paused).count();
        let paused = self.ctl.statuses.len() - playing;
        let mut left = if playing > 0 { format!("{playing} playing") } else { "Standing by".to_owned() };
        if paused > 0 {
            left.push_str(&format!("  ·  {paused} paused"));
        }
        if self.ctl.store.is_loading() {
            left.push_str("  ·  loading audio");
        }
        // Stop Recording (D-127) or the QuipClipper badge: a block of its own after the bar,
        // and the left text after that.
        let rec = self.ctl.recording_view();
        let text_x = match &rec {
            Some(r) => self.panel_rec_stop(ui, &g, &r.title),
            None => match self.quip_badge() {
                Some(badge) => self.panel_quip_badge(ui, &g, badge),
                None => g.bottom_bar_end,
            },
        };
        let p = ui.painter();
        // Recording takes the left text (D-127).
        let left = match &rec {
            Some(r) => {
                let t = Tokens::get(ui.ctx());
                let at = g.o + vec2(text_x + 20.0, yc);
                super::paint_rec_dot(p, at, 6.0, &t, super::rec_pulse(ui.ctx()));
                // Short, to fit before the bar's middle block; the input goes on the right.
                let text = caps_text(p, at + vec2(14.0, 0.0), Align2::LEFT_CENTER, &format!("Rec {}", format_clock(r.seconds, false)), condensed(22.0), t.error);
                match r.level_db {
                    Some(db) => {
                        let bar = Rect::from_min_size(pos2(text.right() + 12.0, yc - 4.0), vec2(56.0, 8.0));
                        super::paint_input_level(p, bar, db, &t);
                        text.union(bar)
                    }
                    None => text,
                }
            }
            None => caps_text(p, g.o + vec2(text_x + 12.0, yc), Align2::LEFT_CENTER, &left, condensed(22.0), pal.peach),
        };
        if let Some(r) = &rec {
            // Painted, so a screen reader is told it here.
            let read_out = ui.interact(left, Id::unique("panel-rec-status"), Sense::hover());
            theme::role(read_out, egui::accesskit::Role::Label, super::recording_text(r), None);
        }
        let p = ui.painter();
        // The right-hand text runs from the middle block (or the left text) to the end cap.
        let right_from = bottom_mid(&g).filter(|mid| *mid > left.right() - g.o.x).map_or(left.right() - g.o.x, |mid| mid + 60.0) + 24.0;
        let importing = super::import_summary(&self.ctl.store).or_else(|| self.ctl.render_summary());
        let (right, color) = match (&self.ctl.notice, &self.ctl.engine.error, &self.ctl.engine.device_name) {
            (Some(n), _, _) if self.notice_hovered || n.at.elapsed() < Duration::from_secs(if n.error { 10 } else { 4 }) => {
                (n.text.clone(), if n.error { pal.red } else { pal.peach })
            }
            // (Silent runs have an empty one, which says nothing.)
            (_, Some(err), _) if !err.is_empty() => (err.clone(), pal.red),
            _ if rec.is_some() => {
                let r = rec.as_ref().expect("recording");
                let from = r.input.as_ref().map_or_else(|| crate::controller::INPUT_WAITING.to_owned(), |i| format!("From {i}"));
                (from, Tokens::get(ui.ctx()).error)
            }
            // The import under way, with room for the file name (the left text has none).
            _ if importing.is_some() => (importing.unwrap_or_default(), pal.peach),
            (_, None, Some(name)) => (format!("{name}  ·  {} kHz", self.ctl.engine.sample_rate() as f32 / 1000.0), pal.lilac),
            _ => ("No output".to_owned(), theme::panel_dim(ui.ctx())),
        };
        let galley = ui.fonts_mut(|f| {
            let mut job = egui::text::LayoutJob::simple_singleline(right.to_uppercase(), condensed_semi(17.0), color);
            job.wrap.max_width = ((g.size.x - M - 52.0) - right_from).max(0.0);
            job.wrap.max_rows = 1;
            f.layout_job(job)
        });
        let at = g.o + vec2(g.size.x - M - 52.0 - galley.size().x, yc - galley.size().y / 2.0);
        let (rect, elided) = (Rect::from_min_size(at, galley.size()), galley.elided);
        p.galley(at, galley, color);
        // Cut short: the full text on hover, and a notice stays while it's read (D-191).
        let notice = self.ctl.notice.as_ref().is_some_and(|n| n.text == right);
        self.notice_hovered = false;
        if elided {
            let resp = ui.interact(rect, Id::unique("panel-status-right"), Sense::hover()).on_hover_text(right);
            self.notice_hovered = notice && resp.hovered();
        }
    }

    /// The bottom bar's blocks (M22): Setlist, Add Sound, Grid, List and Inspector, the
    /// same controls as Console's bottom bar, in Panel colours. A block that's on is amber
    /// with a black ring inside, like the pills.
    fn panel_bar_blocks(&mut self, ui: &mut Ui, g: &Geo, actions: &mut Vec<Action>) {
        let pal = theme::palette(ui.ctx());
        let lib = &self.ctl.store.library;
        let set = self.ctl.current_set();
        let (layout, locked, sidebar, inspector) = (set.layout, set.locked, lib.show_sidebar, lib.show_inspector);
        let yb = g.size.y - M;
        for &(id, label, x0, x1) in &g.blocks {
            let rect = r(g, x0, yb - BOT, x1, yb);
            let (fill, on) = match id {
                "sidebar" => (pal.lilac, Some(sidebar)),
                "add" => (pal.sky, None),
                "grid" => (pal.peach, Some(layout == Layout::Grid)),
                "list" => (pal.periwinkle, Some(layout == Layout::List)),
                _ => (pal.lilac, Some(inspector)),
            };
            // Setlist's arrow points left while it's shown, Inspector's right (D-216).
            let arrow = match id {
                "sidebar" => Some(sidebar),
                "inspector" => Some(!inspector),
                _ => None,
            };
            let resp = ui.add_enabled_ui(id != "add" || !locked, |ui| bar_block(ui, rect, id, label, fill, on == Some(true), arrow)).inner;
            match id {
                "sidebar" => {
                    let tip = tips::setlist(sidebar);
                    if theme::hint_keys(theme::role(resp, AxRole::Button, "Setlist", on), tip.0, tip.1).clicked() {
                        actions.push(inv!("view.sidebar").into());
                    }
                }
                "add" => {
                    let resp = theme::role(resp, AxRole::Button, "Add Sound", None);
                    if theme::hint_or(resp, format!("{} — {}", tips::ADD.0, tips::ADD.1), tips::LOCKED_WHY).clicked() {
                        actions.push(inv!("tile.add").into());
                    }
                }
                "grid" => {
                    if theme::hint_keys(theme::role(resp, AxRole::Button, "Grid", on), tips::GRID.0, tips::GRID.1).clicked() && layout != Layout::Grid {
                        actions.push(inv!("set.layout", {"layout": "grid"}).into());
                    }
                }
                "list" => {
                    if theme::hint_keys(theme::role(resp, AxRole::Button, "Cue list", on), tips::LIST.0, tips::LIST.1).clicked() && layout != Layout::List {
                        actions.push(inv!("set.layout", {"layout": "list"}).into());
                    }
                }
                _ => {
                    let tip = tips::inspector(inspector);
                    if theme::hint_keys(theme::role(resp, AxRole::Button, "Inspector", on), tip.0, tip.1).clicked() {
                        actions.push(inv!("view.inspector").into());
                    }
                }
            }
        }
    }

    /// Stop Recording (D-127): a red block after the bottom bar, where the QuipClipper
    /// badge goes otherwise. Returns where it ends.
    fn panel_rec_stop(&mut self, ui: &mut Ui, g: &Geo, title: &str) -> f32 {
        let pal = theme::palette(ui.ctx());
        let t = Tokens::get(ui.ctx());
        let (yb, left) = (g.size.y - M, g.bottom_bar_end + GAP);
        let font = block_font();
        let label = if g.size.x >= 1100.0 { "Stop Recording" } else { "Stop" };
        let w = ui.fonts_mut(|f| f.layout_no_wrap(label.to_uppercase(), font.clone(), pal.black).size().x) + 24.0;
        let rect = r(g, left, yb - BOT, left + w, yb);
        let resp = ui.interact(rect, Id::unique("panel-rec-stop"), Sense::click());
        let fill = if resp.hovered() { theme::lighten(t.error, 0.18) } else { t.error };
        let p = ui.painter();
        p.rect_filled(rect, 0.0, fill);
        block_label(p, rect, label, font, theme::ink_on(t.error));
        let resp = theme::role(resp, AxRole::Button, "Stop Recording", None);
        if theme::hint(resp, format!("Stop recording; it becomes the sound of {title}")).clicked() {
            self.run(inv!("record.stop"));
        }
        left + w
    }

    /// New QuipClipper clips (D-103): an amber block after the bottom bar, saying as much
    /// as fits. Returns where it ends.
    fn panel_quip_badge(&mut self, ui: &mut Ui, g: &Geo, badge: super::quip_browser::QuipBadge) -> f32 {
        let pal = theme::palette(ui.ctx());
        let (yb, left) = (g.size.y - M, g.bottom_bar_end + GAP);
        let room = (g.size.x * 0.75 - left).max(80.0);
        let font = block_font();
        let n = badge.n;
        let width = |l: &str| ui.fonts_mut(|f| f.layout_no_wrap(l.to_uppercase(), font.clone(), pal.black).size().x) + 24.0;
        let labels = [badge.name(), format!("{n} new clips"), format!("{n} new")];
        let label = labels.iter().find(|l| width(l) <= room * 0.5).unwrap_or(&labels[2]);
        let w = width(label);
        let rect = r(g, left, yb - BOT, left + w, yb);
        let resp = ui.interact(rect, Id::unique("panel-quip-badge"), Sense::click());
        let fill = if resp.is_pointer_button_down_on() {
            theme::shade(pal.amber, 0.85)
        } else if resp.hovered() {
            theme::lighten(pal.amber, 0.18)
        } else {
            pal.amber
        };
        let p = ui.painter();
        p.rect_filled(rect, 0.0, fill);
        block_label(p, rect, label, font, pal.black);
        let resp = theme::role(resp, AxRole::Button, badge.name(), None);
        if theme::hint(resp, badge.tip).clicked() {
            self.run(inv!("view.quip", {"new": true}));
        }
        left + w
    }
}

/// One of the bottom bar's blocks: square-cornered, its label centred in black; `on`
/// turns it amber with a black ring inside, clear of the label.
/// `arrow`, for Setlist and Inspector, points left (`true`) or right, on that side of the
/// label; Grid and List show their icon instead of the label (D-216).
fn bar_block(ui: &mut Ui, rect: Rect, id: &str, label: &str, fill: Color32, on: bool, arrow: Option<bool>) -> egui::Response {
    let pal = theme::palette(ui.ctx());
    let resp = ui.interact(rect, Id::unique(("panel-bar", id)), Sense::click());
    theme::focus_corners(&resp, CornerRadius::ZERO);
    let base = if on { pal.amber } else { fill };
    let fill = if resp.is_pointer_button_down_on() {
        theme::shade(base, 0.85)
    } else if resp.hovered() {
        theme::lighten(base, 0.18)
    } else {
        base
    };
    let p = ui.painter();
    p.rect_filled(rect, 0.0, fill);
    if on {
        on_ring(p, rect, 0.0);
    }
    let c = rect.center();
    match (id, arrow) {
        ("grid", _) => super::pad_options::paint_grid_icon(p, c, 13.0, true, pal.black),
        ("list", _) => super::pad_options::paint_list_icon(p, c, 13.0, true, pal.black),
        (_, Some(left)) => {
            let w = p.layout_no_wrap(label.to_uppercase(), block_font(), pal.black).size().x;
            let shift = (ARROW + ARROW_GAP) / 2.0;
            let text_c = c + vec2(if left { shift } else { -shift }, 0.0);
            let arrow_x = if left { text_c.x - w / 2.0 - ARROW_GAP - ARROW / 2.0 } else { text_c.x + w / 2.0 + ARROW_GAP + ARROW / 2.0 };
            super::pad_options::paint_arrow(p, pos2(arrow_x, c.y), ARROW * 1.3, left, pal.black);
            block_label(p, Rect::from_center_size(text_c, rect.size()), label, block_font(), pal.black);
        }
        _ => {
            block_label(p, rect, label, block_font(), pal.black);
        }
    }
    resp
}

/// The bar under a cue list in PANEL style (D-199): Continuous | Queued and the
/// cross-fade on the left, Reset List at the right end; `narrow` gives shorter pills and
/// puts the cross-fade on a second line.
pub(super) fn list_bar(ui: &mut Ui, narrow: bool, (continuous, crossfade): (&mut bool, &mut f32), reset: &mut bool, locked: bool) {
    let pal = theme::palette(ui.ctx());
    let (reset_w, mode_w) = if narrow { (80.0, 70.0) } else { (130.0, 120.0) };
    ui.horizontal(|ui| {
        for (value, label, short, tip) in [(true, "Continuous", "Cont", tips::CONTINUOUS), (false, "Queued", "Queue", tips::QUEUED)] {
            let (rect, _) = ui.allocate_exact_size(vec2(mode_w, 34.0), Sense::hover());
            let on = *continuous == value;
            let key = ui.add_enabled_ui(!locked, |ui| pill(ui, rect, label, if narrow { short } else { label }, pal.periwinkle, on)).inner;
            if theme::hint_or(theme::role(key, AxRole::RadioButton, label, Some(on)), tip, tips::LOCKED_WHY).clicked() && !locked {
                *continuous = value;
            }
        }
        if !narrow {
            ui.add_space(16.0);
            super::crossfade_slider(ui, crossfade, *continuous && !locked);
        }
        ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(reset_w, 34.0), Sense::hover());
            let top = theme::role(pill(ui, rect, "reset", if narrow { "Reset" } else { "Reset List" }, pal.lilac, false), AxRole::Button, "Reset List", None);
            *reset = theme::hint(top, tips::RESET_LIST).clicked();
        });
    });
    if narrow {
        ui.horizontal(|ui| super::crossfade_slider(ui, crossfade, *continuous && !locked));
    }
}

/// A square-cornered block with a black arrow (up or down), for stepping the cue pointer.
/// The arrow is drawn, since the condensed fonts have no arrow glyphs.
fn step_block(ui: &mut Ui, rect: Rect, id: &str, up: bool) -> egui::Response {
    let pal = theme::palette(ui.ctx());
    let resp = ui.interact(rect, Id::unique(("panel-step", id)), Sense::click());
    theme::focus_corners(&resp, CornerRadius::ZERO);
    let fill = if resp.hovered() { theme::lighten(pal.lilac, 0.18) } else { pal.lilac };
    ui.painter().rect_filled(rect, 0.0, fill);
    let c = rect.center();
    let (tip, base) = if up { (c.y - 5.0, c.y + 4.0) } else { (c.y + 5.0, c.y - 4.0) };
    ui.painter().add(egui::Shape::convex_polygon(vec![pos2(c.x, tip), pos2(c.x + 7.0, base), pos2(c.x - 7.0, base)], pal.black, Stroke::NONE));
    resp
}

/// A square-cornered block that behaves like a button.
fn pill_block(ui: &mut Ui, rect: Rect, id: &str, label: &str, fill: Color32) -> egui::Response {
    let pal = theme::palette(ui.ctx());
    let resp = ui.interact(rect, Id::unique(("panel-block", id)), Sense::click());
    theme::focus_corners(&resp, CornerRadius::ZERO);
    let fill = if resp.hovered() { theme::lighten(fill, 0.18) } else { fill };
    ui.painter().rect_filled(rect, 0.0, fill);
    block_label(ui.painter(), rect, label, condensed(18.0), pal.black);
    resp
}

/// The narrowest PANEL tile the grid will draw; below it the grid shows fewer columns.
pub const TILE_MIN_WIDTH: f32 = 150.0;

/// Below this width a PANEL tile stacks its readouts: the time moves up beside the key
/// code and the title gets the whole bottom row.
const TILE_STACK_BELOW: f32 = 232.0;

/// Below this width a PANEL tile's time gets a line of its own under the key code, and
/// its title runs up the block as far as it needs (D-217).
const TILE_NARROW_BELOW: f32 = 120.0;

/// How far in from a block's left edge something must start, `edge` from its top or
/// bottom, to clear a rounded corner of radius `r` by `gap`; never less than `min`.
fn corner_inset(r: f32, edge: f32, gap: f32, min: f32) -> f32 {
    let d = (r - edge).max(0.0);
    (r - (r * r - d * d).max(0.0).sqrt() + gap).max(min)
}

/// The shortest Panel rows get when they share the board's height (D-140).
pub const ROW_MIN_HEIGHT: f32 = 96.0;

/// Tile height for a given width in the PANEL grid (wider, shorter blocks than Console).
pub fn tile_height(w: f32) -> f32 {
    (w * 0.5).clamp(96.0, 150.0)
}

/// How strong a block's waveform is where it hasn't played: `idle` at rest, 0.3 while it
/// plays; solid at rest in high contrast and 0.45 while it plays.
fn rest_alpha(ui: &Ui, playing: bool, idle: f32) -> f32 {
    match (theme::is_high_contrast(ui.ctx()), playing) {
        (false, true) => 0.3,
        (false, false) => idle,
        (true, true) => 0.45,
        (true, false) => 1.0,
    }
}

/// A tile as a panel block: rounded left edge, code top-left, label bottom-right,
/// black waveform; brighter while playing.
pub fn paint_tile(ui: &Ui, rect: Rect, tile: &Tile, status: Option<&VoiceStatus>, time: &str, f: TileFlags) {
    let pal = theme::palette(ui.ctx());
    let p = ui.painter();
    let sounding = status.is_some_and(|s| matches!(s.state, PlayState::Playing | PlayState::Stopping));
    let paused = status.is_some_and(|s| s.state == PlayState::Paused);
    let mut base = theme::panel_tile(ui.ctx(), tile.color);
    let empty = tile.file.is_none() && tile.action.is_none();
    if empty {
        base = theme::shade(base, theme::PANEL_EMPTY_SHADE);
    }
    let fill = if sounding {
        theme::lighten(base, 0.45)
    } else if paused {
        theme::lighten(base, 0.15)
    } else if f.hovered {
        theme::lighten(base, 0.1)
    } else {
        base
    };
    // A narrow block's rounded edge stays a corner rather than a half circle.
    let rad = (rect.height() / 2.8).min(rect.width() * 0.35).min(255.0) as u8;
    let corners = CornerRadius { nw: rad, sw: rad, ne: 0, se: 0 };
    p.rect_filled(rect, corners, fill);
    if let (Some(tex), ImageLayer::Underlay) = (f.picture, tile.image_layer) {
        paint_picture(p, rect, corners, tile, tex);
    }
    let ink = if empty { theme::inks(ui.ctx()).empty } else { pal.black };
    let wave = Rect::from_min_max(rect.min + vec2(22.0, 38.0), pos2(rect.right() - 16.0, rect.bottom() - 40.0));
    let played_x = status.map(|s| wave.left() + wave.width() * s.progress());
    // While it's recorded into, the input and its level take the waveform's place.
    if let Some(rec) = f.recording {
        let c = f.caption.as_deref().unwrap_or_default();
        let g = ui.fonts_mut(|fonts| {
            let mut job = egui::text::LayoutJob::simple_singleline(c.to_uppercase(), condensed_semi(16.0), ink);
            job.wrap.max_width = wave.width();
            job.wrap.max_rows = 1;
            fonts.layout_job(job)
        });
        let top = wave.center().y - (g.size().y + 10.0) / 2.0;
        p.galley(pos2(wave.left(), top), g.clone(), ink);
        if let Some(db) = rec.level_db {
            let bar = Rect::from_min_size(pos2(wave.left(), top + g.size().y + 4.0), vec2(wave.width().min(200.0), 6.0));
            p.rect_filled(bar, 0.0, theme::with_alpha(ink, 0.3));
            p.rect_filled(Rect::from_min_size(bar.min, vec2(bar.width() * crate::logic::meter_frac(db), bar.height())), 0.0, ink);
        }
    } else if let Some((peaks, duration)) = f.waveform {
        let rest = theme::with_alpha(ink, rest_alpha(ui, status.is_some(), 0.62));
        paint_waveform(p, wave, tile, peaks, duration, played_x, ink, rest);
    } else if let Some(c) = &f.caption {
        let g = ui.fonts_mut(|fonts| {
            let mut job = egui::text::LayoutJob::simple_singleline(c.to_uppercase(), condensed_semi(16.0), ink);
            job.wrap.max_width = wave.width();
            job.wrap.max_rows = 1;
            fonts.layout_job(job)
        });
        p.galley(pos2(wave.center().x - g.size().x / 2.0, wave.center().y - g.size().y / 2.0), g, ink);
    } else if tile.file.is_none() {
        caps_text(p, wave.center(), Align2::CENTER_CENTER, "Drop audio here", condensed_semi(16.0), ink);
    }
    if let Some(px) = played_x {
        p.rect_filled(Rect::from_min_size(pos2(px - 1.0, wave.top() - 4.0), vec2(2.0, wave.height() + 8.0)), 0.0, ink);
    }
    if let (Some(tex), ImageLayer::Overlay) = (f.picture, tile.image_layer) {
        paint_picture(p, rect, corners, tile, tex);
    }
    let code = tile.hotkey.map_or_else(|| "--".into(), panel_code);
    // As far down as the badges, and in from the left far enough to clear the rounded
    // corner (D-204).
    let code_at = pos2(rect.left() + corner_inset(corners.nw as f32, 10.0, 8.0, 18.0), rect.top() + 10.0);
    let code_rect = caps_text(p, code_at, Align2::LEFT_TOP, &code, condensed(24.0), ink);
    let badge = if f.recording.is_some() {
        "Recording".to_owned()
    } else if let Some(w) = vamp_state_word(tile, status).filter(|_| !paused) {
        // Vamping, then Playing on (D-157).
        if w == "VAMPING" { "Vamping" } else { "Playing on" }.to_owned()
    } else if sounding {
        "Playing".to_owned()
    } else if paused {
        "Paused".to_owned()
    } else {
        detail_badge_text(tile, status)
    };
    let time_color = if f.failed { pal.red } else { ink };
    let stacked = rect.width() < TILE_STACK_BELOW;
    let narrow = rect.width() < TILE_NARROW_BELOW;
    // Wide: badge top-right, time bottom-left beside the title. Stacked: time top-right,
    // badge to its left, title alone along the bottom. Narrow: the time on a line of its
    // own under the key code, and the title from the bottom up as far as it needs (D-217).
    // The meter sits by the time: under the stacked one (above the waveform), over the
    // wide one (clear of the rounded corner).
    let meter = f.level.filter(|_| sounding);
    // The lowest of the lettering at the top, which the title stops short of.
    let mut ceiling = code_rect.bottom();
    let (badge_right, title_left) = if stacked {
        let at = if narrow { pos2(rect.right() - 12.0, code_rect.bottom() + 2.0) } else { rect.right_top() + vec2(-12.0, 10.0) };
        let wide = ui.fonts_mut(|f| f.layout_no_wrap(time.to_owned(), condensed_semi(18.0), time_color).size().x) > rect.width() - 24.0;
        let t = theme::paint_tabular(p, at, Align2::RIGHT_TOP, time, condensed_semi(if wide { 15.0 } else { 18.0 }), time_color);
        if let Some(b) = meter {
            paint_tile_meter(p, meter_under(t, 2.0), &b, &Tokens::get(ui.ctx()));
        }
        if narrow {
            ceiling = t.bottom() + if meter.is_some() { 5.0 } else { 0.0 };
            // In from the rounded corner far enough for the bottom line.
            (rect.right() - 12.0, rect.left() + corner_inset(corners.sw as f32, 10.0, 4.0, 10.0))
        } else {
            (t.left() - 8.0, rect.left() + 18.0)
        }
    } else {
        let t = theme::paint_tabular(p, rect.left_bottom() + vec2(corner_inset(corners.sw as f32, 8.0, 8.0, 18.0), -8.0), Align2::LEFT_BOTTOM, time, condensed_semi(18.0), time_color);
        if let Some(b) = meter {
            let w = t.width().max(36.0);
            paint_tile_meter(p, Rect::from_min_size(pos2(t.left(), t.top() - 2.0), vec2(w, 3.0)), &b, &Tokens::get(ui.ctx()));
        }
        (rect.right() - 12.0, t.right() + 12.0)
    };
    // A narrow tile drops the badge rather than run it into the key code.
    // Markers after the key code: a check when selected and, when the badge has no
    // room, a playing or paused symbol, so neither state is told by colour alone.
    let mut code_end = code_rect.right();
    if f.selected {
        theme::paint_check_badge(p, pos2(code_end + 14.0, code_rect.center().y), pal.amber, pal.black);
        code_end += 24.0;
    }
    // The option badges (D-145) in the corner, then the badge text to their left.
    let mut badge_right = badge_right;
    let shown = super::pad_options::fit_badges(tile, badge_right - code_end - 8.0, 21.0);
    if !shown.is_empty() {
        badge_right -= super::pad_options::paint_badges(p, pos2(badge_right, rect.top() + 10.0), 17.0, &shown, ink) + 10.0;
    }
    let badge_w = ui.fonts_mut(|f| f.layout_no_wrap(badge.to_uppercase(), condensed_semi(13.0), ink).size().x);
    let mut title_left = title_left;
    if !badge.is_empty() && badge_right - badge_w >= code_end + 8.0 {
        caps_text(p, pos2(badge_right, rect.top() + 10.0), Align2::RIGHT_TOP, &badge, condensed_semi(13.0), ink);
    } else if narrow && f.recording.is_none() && (sounding || paused) {
        // A narrow block's state goes under its time, right-aligned like it.
        let word = if !badge.is_empty() && vamp_state_word(tile, status).is_some() && !paused { badge.as_str() } else if paused { "⏸" } else { "▶" };
        let font = if word.len() > 3 { condensed_semi(13.0) } else { theme::bold(14.0) };
        let r = caps_text(p, pos2(rect.right() - 12.0, ceiling + 2.0), Align2::RIGHT_TOP, word, font, ink);
        ceiling = r.bottom();
    } else if vamp_state_word(tile, status).is_some() && !paused {
        // No room up top for a vamp's state: before the title, so a vamping block still
        // reads apart from a plain one (D-157).
        // Clear of the rounded bottom-left corner, at the word's baseline.
        let left = title_left.max(rect.left() + corner_inset(corners.sw as f32, 9.0, 4.0, 0.0));
        let r = caps_text(p, pos2(left, rect.bottom() - 9.0), Align2::LEFT_BOTTOM, &badge, condensed_semi(13.0), ink);
        title_left = r.right() + 8.0;
    } else if sounding || paused {
        let icon = if paused { "⏸" } else { "▶" };
        if badge_right - 16.0 >= code_end + 8.0 {
            p.text(pos2(code_end + 8.0, code_rect.center().y), Align2::LEFT_CENTER, icon, theme::bold(14.0), ink);
        } else {
            // No room up top: before the title instead.
            p.text(pos2(title_left, rect.bottom() - 18.0), Align2::LEFT_CENTER, icon, theme::bold(14.0), ink);
            title_left += 22.0;
        }
    }
    let right = rect.right() - if narrow { 8.0 } else { 12.0 };
    let room = (right - title_left).max(20.0);
    // A stacked title that doesn't fit at full size steps down once before wrapping; a
    // narrow one steps down until its longest word fits (D-217).
    let size = if narrow {
        super::title_size(ui, &tile.title.to_uppercase(), room, &[18.0, 15.0, 13.0, 11.0], condensed)
    } else if stacked && ui.fonts_mut(|f| f.layout_no_wrap(tile.title.to_uppercase(), condensed(22.0), ink).size().x) > room {
        18.0
    } else {
        22.0
    };
    // Up to four lines (M22), right-aligned and growing up from the bottom edge, as far
    // as the lettering at the top allows; then an ellipsis. A narrow block's runs on as
    // far as it needs.
    let line_h = size * 1.15;
    let most = if narrow { usize::MAX } else { super::TITLE_LINES };
    let rows = (((rect.bottom() - 6.0 - ceiling - 4.0) / line_h).floor() as usize).clamp(1, most);
    let galley = ui.fonts_mut(|fonts| {
        let mut job = egui::text::LayoutJob::simple(tile.title.to_uppercase(), condensed(size), ink, room);
        job.wrap.max_rows = rows;
        job.halign = Align::RIGHT;
        fonts.layout_job(job)
    });
    // Each line on a plate of the block's colour, so it reads over the waveform; never
    // past the title's left edge, so a plate doesn't cover the rounded corner.
    let title_pos = pos2(right, rect.bottom() - 6.0 - galley.size().y);
    let plate = theme::with_alpha(fill, if theme::is_high_contrast(ui.ctx()) { 1.0 } else { 0.85 });
    for row in &galley.rows {
        let mut r = row.rect().translate(title_pos.to_vec2()).expand2(vec2(4.0, 0.0));
        r.min.x = r.min.x.max(title_left - 2.0);
        p.rect_filled(r, 0.0, plate);
    }
    p.galley(title_pos, galley, ink);
    if tile.solo {
        super::pad_options::paint_solo_ring(p, rect, corners, Tokens::get(ui.ctx()).solo_mark);
    }
    if tile.disabled {
        let veil = theme::look(ui.ctx()).veil;
        p.rect_filled(rect, corners, veil.fill);
        theme::paint_hatch(p, Rect::from_min_max(pos2(rect.left() + rad as f32 * 0.7, rect.top()), rect.max), veil.stripes);
    }
    paint_outline(ui, rect, corners, &f, false);
    // Recording: a pulsing ring outside the selection's, and a dot by the key code (on
    // black, so it reads on a red block too).
    if let Some(rec) = f.recording {
        let t = Tokens::get(ui.ctx());
        let grow = if f.selected { 7.0 } else { 3.0 };
        p.rect_stroke(rect.expand(grow), corners, Stroke::new(3.0_f32, theme::with_alpha(t.error, rec.pulse)), StrokeKind::Outside);
        let at = pos2(code_end + 14.0, code_rect.center().y);
        p.circle_filled(at, 8.0, pal.black);
        super::paint_rec_dot(p, at, 5.5, &t, rec.pulse.max(0.75));
    }
}

/// The outline of a selected pad, or of the one a drag would drop on: white, outside the
/// block with a black gap. Never one of the blocks' colours, so on an amber, gold or
/// peach block it doesn't read as the block's own edge with a dark line through it by
/// the lettering (t-m22-panel-select-ring); wider in high contrast.
fn paint_selection(ui: &Ui, rect: Rect, corners: CornerRadius) {
    ui.painter().rect_stroke(rect.expand(SELECT_GAP), corners, Stroke::new(select_width(ui.ctx()), theme::PANEL_SELECT_RING), StrokeKind::Outside);
}

/// The black gap between the selection outline and the block.
const SELECT_GAP: f32 = 3.0;

/// How far past a pad its outlines reach: the recording ring, outside a selected pad's
/// selection outline, is the widest. The grid keeps this much room round its pads so
/// the outlines of the first and last columns and rows aren't cut off.
pub const OUTLINE_REACH: f32 = 10.0;

/// A pad's or cue row's one outline (D-215): white, outside the block with a black
/// gap and following its shape, when it's selected, a drag would drop on it, or it has
/// the keyboard focus (clicked or moved to). It is the selection mark and the focus
/// mark at once, so Panel never draws the focus ring's rectangle round a pad. `inside`:
/// within the block's edge, for a cue row ([`paint_selection_inside`]).
fn paint_outline(ui: &Ui, rect: Rect, corners: CornerRadius, f: &TileFlags, inside: bool) {
    if let Some(id) = f.focus {
        theme::own_focus_cue(ui.ctx(), id);
    }
    if f.selected || f.drop_hover || f.focus.is_some() {
        if inside {
            paint_selection_inside(ui.painter(), rect, corners);
        } else {
            paint_selection(ui, rect, corners);
        }
    }
}

/// The same outline inside a block's edge, for blocks stacked too close for one outside
/// it (a cue row, a set in the sets column): the neighbours would cover it and the list
/// would cut it off. White along the edge, then the black gap, both following the
/// block's corners, so the block's colour shrinks inside them (t-m22-panel-outline-rows).
fn paint_selection_inside(p: &egui::Painter, rect: Rect, corners: CornerRadius) {
    let w = select_width(p.ctx());
    let inset = |by: f32| {
        let f = |v: u8| (v as f32 - by).max(0.0).round() as u8;
        CornerRadius { nw: f(corners.nw), ne: f(corners.ne), sw: f(corners.sw), se: f(corners.se) }
    };
    p.rect_stroke(rect, corners, Stroke::new(w, theme::PANEL_SELECT_RING), StrokeKind::Inside);
    p.rect_stroke(rect.shrink(w), inset(w), Stroke::new(SELECT_GAP, theme::palette(p.ctx()).black), StrokeKind::Inside);
}

/// How far into a block [`paint_selection_inside`] reaches, the most it can be (high
/// contrast): lettering keeps at least this far in, plus a gap.
const INSIDE_REACH: f32 = SELECT_GAP + 3.0;

fn select_width(ctx: &egui::Context) -> f32 {
    if theme::is_high_contrast(ctx) { 3.0 } else { 2.0 }
}

pub fn paint_add_slot(ui: &Ui, rect: Rect, hovered: bool) {
    let pal = theme::palette(ui.ctx());
    let p = ui.painter();
    let color = if hovered { pal.peach } else { theme::panel_dim(ui.ctx()) };
    let rad = (rect.height() / 2.8).min(rect.width() * 0.35);
    // Dashed outline with a rounded left edge, drawn as segments.
    let r = rect.shrink(1.0);
    let stroke = Stroke::new(2.0_f32, color);
    let pts = [pos2(r.left() + rad, r.top()), r.right_top(), r.right_bottom(), pos2(r.left() + rad, r.bottom())];
    for w in pts.windows(2) {
        p.extend(egui::Shape::dashed_line(&[w[0], w[1]], stroke, 8.0, 6.0));
    }
    let arc: Vec<Pos2> = (0..=16)
        .map(|i| {
            let a = std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * i as f32 / 16.0;
            pos2(r.left() + rad + rad * a.cos(), r.center().y - (r.height() / 2.0) * a.sin())
        })
        .collect();
    p.extend(egui::Shape::dashed_line(&arc, stroke, 8.0, 6.0));
    // As much of the label as fits beside the rounded edge.
    let fits = |l: &str| ui.fonts_mut(|f| f.layout_no_wrap(l.to_uppercase(), condensed(20.0), color).size().x) <= rect.width() - 14.0 - rad;
    let label = ["+ Add Sounds", "+ Add"].into_iter().find(|l| fits(l)).unwrap_or("+");
    caps_text(p, rect.right_bottom() + vec2(-14.0, -8.0), Align2::RIGHT_BOTTOM, label, condensed(20.0), color);
}

/// A cue-list row as a panel block.
#[allow(clippy::too_many_arguments)]
pub fn paint_list_row(ui: &Ui, rect: Rect, index: usize, tile: &Tile, status: Option<&VoiceStatus>, time: &str, is_next: bool, f: TileFlags) {
    let pal = theme::palette(ui.ctx());
    let p = ui.painter();
    let ink = pal.black;
    let base = theme::panel_tile(ui.ctx(), tile.color);
    let fill = if status.is_some() {
        theme::lighten(base, 0.45)
    } else if f.hovered {
        theme::lighten(base, 0.1)
    } else {
        base
    };
    let corners = CornerRadius { nw: 22, sw: 22, ne: 0, se: 0 };
    p.rect_filled(rect, corners, fill);
    if is_next {
        // A black notch marks the next cue.
        p.rect_filled(Rect::from_min_size(pos2(rect.right() - 12.0, rect.top()), vec2(12.0, rect.height())), 0.0, ink);
        p.rect_filled(Rect::from_min_size(pos2(rect.right() - 8.0, rect.top()), vec2(8.0, rect.height())), 0.0, pal.amber);
    }
    let cy = rect.center().y;
    theme::paint_tabular(p, pos2(rect.left() + 20.0, cy), Align2::LEFT_CENTER, &format!("{:02}", index + 1), condensed(26.0), ink);
    // The armed mark first; a cue list has no automatic keys, only hand-set ones (D-192).
    super::paint_cue_mark(p, pos2(rect.left() + super::CUE_MARK_X, cy), 6.0, super::cue_mark(ui, status, f.played), ink);
    let mut text_x = rect.left() + 90.0;
    if let Some(k) = tile.hotkey {
        text_x = caps_text(p, pos2(text_x, cy), Align2::LEFT_CENTER, &PadKey::label(k), condensed_semi(16.0), ink).right() + 10.0;
    }
    let title = ui.fonts_mut(|fonts| {
        let mut job = egui::text::LayoutJob::simple_singleline(tile.title.to_uppercase(), condensed(22.0), ink);
        job.wrap.max_width = rect.width() * 0.3;
        job.wrap.max_rows = 1;
        fonts.layout_job(job)
    });
    let title_w = title.size().x;
    // A cue with notes puts them on a second line, under the title and waveform.
    let notes = tile.notes.trim();
    let title_y = if notes.is_empty() { cy } else { cy - 9.0 };
    p.galley(pos2(text_x, title_y - title.size().y / 2.0), title, ink);
    let t = theme::paint_tabular(p, pos2(rect.right() - 24.0, cy), Align2::RIGHT_CENTER, time, condensed_semi(18.0), ink);
    if let Some(b) = f.level.filter(|_| status.is_some_and(|s| s.state != PlayState::Paused)) {
        paint_tile_meter(p, meter_under(t, 2.0), &b, &Tokens::get(ui.ctx()));
    }
    // Left of the time: a check when selected, then the state in a word.
    let mut right = t.left() - 16.0;
    if f.selected {
        theme::paint_check_badge(p, pos2(right - 8.0, cy), pal.black, pal.amber);
        right -= 26.0;
    }
    let paused = status.is_some_and(|s| s.state == PlayState::Paused);
    let state_word = if tile.disabled {
        Some("Disabled")
    } else if f.recording.is_some() {
        Some("Recording")
    } else if status.is_some() {
        Some(if paused { "Paused" } else { "Playing" })
    } else if is_next {
        Some("Next")
    } else {
        None
    };
    if let Some(w) = state_word {
        let color = if tile.disabled { pal.peach } else { ink };
        right = caps_text(p, pos2(right, cy), Align2::RIGHT_CENTER, w, condensed_semi(15.0), color).left() - 12.0;
    }
    if !notes.is_empty() {
        let galley = ui.fonts_mut(|fonts| {
            let mut job = egui::text::LayoutJob::simple_singleline(notes.to_owned(), condensed_semi(15.0), ink);
            job.wrap.max_width = (right - 4.0 - text_x).max(20.0);
            job.wrap.max_rows = 1;
            fonts.layout_job(job)
        });
        p.galley(pos2(text_x, cy + 12.0 - galley.size().y / 2.0), galley, theme::with_alpha(ink, if theme::is_high_contrast(ui.ctx()) { 1.0 } else { 0.75 }));
    }
    let wave_bottom = if notes.is_empty() { rect.bottom() - 9.0 } else { cy + 1.0 };
    let wave = Rect::from_min_max(pos2(rect.left() + 110.0 + title_w, rect.top() + 9.0), pos2(right - 4.0, wave_bottom));
    if let Some((peaks, duration)) = f.waveform.filter(|_| f.recording.is_none()) {
        let played_x = status.map(|s| wave.left() + wave.width() * s.progress());
        paint_waveform(p, wave, tile, peaks, duration, played_x, ink, theme::with_alpha(ink, rest_alpha(ui, status.is_some(), 0.55)));
    } else if let Some(c) = &f.caption {
        let g = ui.fonts_mut(|fonts| {
            let mut job = egui::text::LayoutJob::simple_singleline(c.to_uppercase(), condensed_semi(15.0), ink);
            job.wrap.max_width = wave.width().max(20.0);
            job.wrap.max_rows = 1;
            fonts.layout_job(job)
        });
        p.galley(pos2(wave.left(), wave.center().y - g.size().y / 2.0), g, ink);
    }
    if tile.disabled {
        let veil = theme::look(ui.ctx()).veil;
        p.rect_filled(rect, corners, veil.fill);
        theme::paint_hatch(p, Rect::from_min_max(pos2(rect.left() + 16.0, rect.top()), rect.max), veil.stripes);
        // Redrawn over the veil, where it stays readable.
        caps_text(p, pos2(t.left() - 16.0 - if f.selected { 26.0 } else { 0.0 }, cy), Align2::RIGHT_CENTER, "Disabled", condensed_semi(15.0), pal.peach);
    }
    paint_outline(ui, rect, corners, &f, true);
    if let Some(rec) = f.recording {
        let t = Tokens::get(ui.ctx());
        p.rect_stroke(rect.expand(3.0), corners + 3, Stroke::new(3.0_f32, theme::with_alpha(t.error, rec.pulse)), StrokeKind::Outside);
        if let Some(db) = rec.level_db {
            let bar = Rect::from_min_size(pos2(wave.left(), wave.bottom() - 6.0), vec2(wave.width().min(160.0), 4.0));
            p.rect_filled(bar, 0.0, theme::with_alpha(ink, 0.3));
            p.rect_filled(Rect::from_min_size(bar.min, vec2(bar.width() * crate::logic::meter_frac(db), bar.height())), 0.0, ink);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Where `block_label` puts a label's ink in a block of `size`.
    fn label_ink(ctx: &egui::Context, size: Vec2, text: &str, font: FontId) -> (Rect, Rect) {
        let rect = Rect::from_min_size(pos2(100.0, 100.0), size);
        let galley = ctx.fonts_mut(|f| f.layout_no_wrap(text.to_uppercase(), font, Color32::BLACK));
        let at = Align2::CENTER_CENTER.anchor_size(rect.center(), galley.size()).min;
        (rect, theme::galley_ink(&galley).translate(at.to_vec2()))
    }

    /// A block's or pill's label is centred in it by eye, within a pixel both ways
    /// (t-m22-panel-polish: it sat low and, in pills, bottom right).
    #[test]
    fn labels_are_centred_in_blocks_and_pills() {
        theme::with_fonts(|ctx| {
            let mut cases: Vec<(Vec2, &str, FontId)> = Vec::new();
            for (_, long, short) in BLOCKS {
                for label in [long, short] {
                    let w = ctx.fonts_mut(|f| f.layout_no_wrap(label.to_uppercase(), block_font(), Color32::BLACK).size().x) + 24.0;
                    cases.push((vec2(w.ceil(), BOT), label, block_font()));
                }
            }
            for (h, label) in [(40.0, "Stop All"), (40.0, "Random"), (34.0, "Reset List"), (34.0, "Queued")] {
                cases.push((vec2(120.0, h), label, condensed(h * 0.5)));
            }
            cases.push((vec2(160.0, 40.0), "+ New Set", condensed(18.0)));
            cases.push((vec2(260.0, TOP), "Settings", condensed(16.0)));
            for (size, label, font) in cases {
                let (rect, ink) = label_ink(ctx, size, label, font);
                let off = ink.center() - rect.center();
                assert!(off.x.abs() <= 1.0 && off.y.abs() <= 1.0, "{label} in {size:?}: off by {off:?}");
            }
        });
    }

    /// A bottom-bar block that's on has a black ring inside it; the lettering's capitals
    /// keep clear of it, top and bottom (t-m22-panel-polish: the ring ran through
    /// SIDEBAR in High Contrast). 2 px by the font's metrics; the rasterised glyphs
    /// round to whole pixels.
    #[test]
    fn bar_lettering_clears_the_ring() {
        theme::with_fonts(|ctx| {
            for (_, long, short) in BLOCKS {
                for label in [long, short] {
                    let (rect, ink) = label_ink(ctx, vec2(200.0, BOT), label, block_font());
                    let ring = rect.shrink(RING_INSET + RING_W);
                    assert!(ink.top() - ring.top() >= 1.5 && ring.bottom() - ink.bottom() >= 1.5, "{label}: ink {ink:?}, ring inside {ring:?}");
                }
            }
        });
    }

    #[test]
    fn text_by_a_rounded_corner_clears_it() {
        // A wide block 130 px tall has a 46 px corner: the key code, 10 px down, starts
        // past the curve, not at the old 18 px (D-204).
        let left = corner_inset(46.0, 10.0, 8.0, 18.0);
        assert!(left > 25.0 && left < 26.0, "{left}");
        // A corner too small to reach it, or text below it: the minimum.
        assert_eq!(corner_inset(6.0, 10.0, 8.0, 18.0), 18.0);
        assert_eq!(corner_inset(0.0, 10.0, 8.0, 18.0), 18.0);
    }
}
