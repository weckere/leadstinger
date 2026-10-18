//! The Panel theme: a retro sci-fi control-panel look, drawn in docs/design/panel.py (an
//! original composition).
//!
//! Layout is absolute, as on such panels: a top elbow sweeping from the left column into the
//! top bar, the title in a gap in that bar, pill controls under it, coloured blocks
//! down the column, and a bottom elbow whose bar carries the status text in its gaps.
//! Everything is placed relative to the area left after the (Linux) menu bar.

use super::*;
use crate::theme::{condensed, condensed_semi, panel_tile_fill, PANEL};
use eframe::egui::{Id, LayerId, Pos2, Vec2};

pub const M: f32 = 8.0; // outer margin
pub const COL: f32 = 176.0; // left column width
pub const TOP: f32 = 34.0; // top bar thickness
pub const ELBOW_Y: f32 = 200.0; // where the top elbow's column part ends
const R_OUT: u8 = 64;
const R_IN: f32 = 30.0;
pub const BOT: f32 = 26.0; // bottom bar thickness
const BOT_COL: f32 = 48.0; // height of the bottom elbow's column part above its bar
const R_OUT_B: u8 = 44;
const R_IN_B: f32 = 22.0;
const GAP: f32 = 5.0;
/// Panel sizes that line up with the frame.
pub const TOP_H: f32 = 168.0;
pub const LEFT_W: f32 = M + COL + 32.0;
pub const BOTTOM_H: f32 = M + BOT + 6.0;

/// Narrower than this, the header can't hold the pills, the title column and the right
/// column side by side, and switches to its compact layout: the title shrinks into the top
/// bar beside Settings, and the readouts move under the pills. (The pills need
/// x0 + 552 px, then a 16 px gap, and the title column starts 488 px from the right edge.)
const WIDE_MIN: f32 = M + COL + 40.0 + 552.0 + 16.0 + 488.0;
/// The compact title's size: its capitals are as tall as the top bar (Antonio's sit
/// 0.29 em below the top of the line), so it clears the pills below.
const TITLE_COMPACT: f32 = 38.0;
/// Width the wide title may take before the Settings cap.
const TITLE_ROOM: f32 = 254.0;

/// The brand's logo (src/brand.rs) in the theme's colours: the middle part in peach.
fn logo_parts() -> [(&'static str, Color32); 3] {
    let (a, b, c) = crate::brand::LOGO;
    [(a, PANEL.amber), (b, PANEL.peach), (c, PANEL.amber)]
}

/// The frame pieces that depend on the window width.
struct Geo {
    o: Pos2,
    size: Vec2,
    /// The full layout (see [`WIDE_MIN`]).
    wide: bool,
    /// Where the big title and the readouts under it start (wide layout only).
    title_x: f32,
    /// The Settings end cap's left edge.
    cap_x: f32,
    /// The right-hand column (Stop All, meter): its left edge; it runs to the margin.
    rcol_x: f32,
    bottom_bar_end: f32,
}

fn geo(area: Rect) -> Geo {
    let w = area.width();
    let wide = w >= WIDE_MIN;
    let title_x = w - 488.0;
    let rcol_x = w - M - if wide { 210.0 } else { (w * 0.22).clamp(160.0, 210.0) };
    Geo {
        o: area.min,
        size: area.size(),
        wide,
        title_x,
        cap_x: if wide { title_x + 268.0 } else { rcol_x },
        rcol_x,
        bottom_bar_end: (w * 0.5).clamp(300.0, 640.0),
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
fn caps_text(p: &egui::Painter, pos: Pos2, anchor: Align2, text: &str, font: FontId, color: Color32) -> Rect {
    p.text(pos, anchor, text.to_uppercase(), font, color)
}

/// A pill control at an absolute rect. `on` shows it in the active (amber) colour.
fn pill(ui: &mut Ui, rect: Rect, id: &str, label: &str, fill: Color32, on: bool) -> egui::Response {
    let resp = ui.interact(rect, Id::new(("panel-pill", id)), Sense::click());
    let base = if on { PANEL.amber } else { fill };
    let fill = if resp.is_pointer_button_down_on() {
        theme::shade(base, 0.85)
    } else if resp.hovered() {
        theme::lighten(base, 0.18)
    } else {
        base
    };
    let p = ui.painter();
    p.rect_filled(rect, rect.height() / 2.0, fill);
    caps_text(
        p,
        pos2(rect.right() - rect.height() * 0.42, rect.bottom() - rect.height() * 0.12),
        Align2::RIGHT_BOTTOM,
        label,
        condensed(rect.height() * 0.5),
        PANEL.black,
    );
    resp
}

/// The fixed frame. It goes on egui's shared background layer, the one the panels
/// draw on, and is painted before them so it stays underneath.
pub fn paint_frame(ctx: &egui::Context, area: Rect, subtitle: &str) {
    let g = geo(area);
    let p = ctx.layer_painter(LayerId::background());
    let (w, h) = (g.size.x, g.size.y);
    p.rect_filled(area, 0.0, PANEL.black);

    // Top elbow: column + bar, rounded outside, concave inside.
    p.rect_filled(r(&g, M, M, M + COL, ELBOW_Y), CornerRadius { nw: R_OUT, ..Default::default() }, PANEL.amber);
    // Title in the bar's gap. Wide: big, with the subtitle under it. Compact: the bar's
    // height, right up against the Settings cap, and no subtitle.
    let bar_end = if g.wide {
        // The brand's name, shrunk if needed to fit before the Settings cap.
        let full: String = logo_parts().iter().map(|(t, _)| *t).collect();
        let natural = ctx.fonts(|f| f.layout_no_wrap(full.to_uppercase(), condensed(54.0), PANEL.amber).size().x);
        let size = 54.0 * (TITLE_ROOM / natural).min(1.0);
        let mut at = g.o + vec2(g.title_x, M - 6.0 + (54.0 - size) * 0.45);
        for (part, color) in logo_parts() {
            if !part.is_empty() {
                at = caps_text(&p, at, Align2::LEFT_TOP, part, condensed(size), color).right_top();
            }
        }
        caps_text(&p, g.o + vec2(g.title_x, M + 56.0), Align2::LEFT_TOP, subtitle, condensed_semi(14.0), PANEL.dim);
        g.title_x - 12.0
    } else {
        let font = condensed(TITLE_COMPACT);
        let mut at = g.o + vec2(g.cap_x - 14.0, M - 0.29 * TITLE_COMPACT);
        for (part, color) in logo_parts().into_iter().rev() {
            if !part.is_empty() {
                at = caps_text(&p, at, Align2::RIGHT_TOP, part, font.clone(), color).left_top();
            }
        }
        at.x - g.o.x - 14.0
    };
    p.rect_filled(r(&g, M + COL - 1.0, M, bar_end.max(M + COL + R_IN), M + TOP), 0.0, PANEL.amber);
    p.rect_filled(r(&g, M + COL - 1.0, M + TOP - 1.0, M + COL + R_IN, M + TOP + R_IN), 0.0, PANEL.amber);
    p.circle_filled(g.o + vec2(M + COL + R_IN, M + TOP + R_IN), R_IN, PANEL.black);
    caps_text(&p, g.o + vec2(M + COL - 12.0, ELBOW_Y - 6.0), Align2::RIGHT_BOTTOM, "Sets", condensed(26.0), PANEL.black);
    caps_text(&p, g.o + vec2(M + COL - 12.0, ELBOW_Y - 40.0), Align2::RIGHT_BOTTOM, crate::brand::PANEL_TAG, condensed_semi(13.0), PANEL.black);

    // Bottom elbow.
    let yb = h - M;
    p.rect_filled(r(&g, M, yb - BOT - BOT_COL, M + COL, yb), CornerRadius { sw: R_OUT_B, ..Default::default() }, PANEL.violet);
    p.rect_filled(r(&g, M + COL - 1.0, yb - BOT, g.bottom_bar_end, yb), 0.0, PANEL.violet);
    p.rect_filled(r(&g, M + COL - 1.0, yb - BOT - R_IN_B, M + COL + R_IN_B, yb - BOT + 1.0), 0.0, PANEL.violet);
    p.circle_filled(g.o + vec2(M + COL + R_IN_B, yb - BOT - R_IN_B), R_IN_B, PANEL.black);
    // Status bar pieces: a middle block and an end cap (the text sits in the gaps).
    if let Some(mid) = bottom_mid(&g) {
        p.rect_filled(r(&g, mid, yb - BOT, mid + 60.0, yb), 0.0, PANEL.lilac);
    }
    p.rect_filled(r(&g, w - M - 40.0, yb - BOT, w - M, yb), CornerRadius { ne: 13, se: 13, ..Default::default() }, PANEL.lilac);
}

/// One toolbar pill as placed: its id, label, size and the gap after it.
struct PlacedPill<'a> {
    id: &'static str,
    label: &'a str,
    w: f32,
    h: f32,
    gap: f32,
}

/// The toolbar pills that fit in `room` px. A wide window gets the designed widths.
/// Narrower ones fit each pill to its label, then drop the column count (View ▸ Columns
/// has it), then + Add Sounds (File ▸ Add Sounds, the grid's add slot and file drops
/// have it), then shrink the pills, so the row never runs under Stop All.
fn toolbar_pills<'a>(ui: &Ui, cols_label: &'a str, room: f32, wide: bool) -> Vec<PlacedPill<'a>> {
    // Groups of (id, label, designed width); 6 px between pills, 14 between groups.
    let groups: [&[(&'static str, &'a str, f32)]; 3] = [
        &[("perform", "Perform", 100.0), ("edit", "Edit", 70.0)],
        &[("grid", "Grid", 70.0), ("list", "List", 70.0), ("cols", cols_label, 64.0)],
        &[("add", "+ Add Sounds", 132.0)],
    ];
    let row = |h: f32, designed: bool, drop: &[&str]| {
        let mut out: Vec<PlacedPill> = Vec::new();
        for group in groups {
            for &(id, label, w) in group.iter().filter(|(id, _, _)| !drop.contains(id)) {
                let w = if designed {
                    w
                } else {
                    let text = ui.fonts(|f| f.layout_no_wrap(label.to_uppercase(), condensed(h * 0.5), PANEL.black).size().x);
                    (text + h * 0.8).max(h * 1.5).ceil()
                };
                out.push(PlacedPill { id, label, w, h, gap: 6.0 });
            }
            if let Some(last) = out.last_mut() {
                last.gap = 14.0;
            }
        }
        out
    };
    let width = |row: &[PlacedPill]| row.iter().map(|p| p.w + p.gap).sum::<f32>() - row.last().map_or(0.0, |p| p.gap);
    if wide {
        return row(40.0, true, &[]);
    }
    let tries = [(40.0, &[][..]), (40.0, &["cols"][..]), (40.0, &["cols", "add"][..]), (34.0, &["cols", "add"][..])];
    let mut last = Vec::new();
    for (h, drop) in tries {
        last = row(h, false, drop);
        if width(&last) <= room {
            break;
        }
    }
    last
}

impl StingerApp {
    pub(super) fn panel_toolbar(&mut self, ui: &mut Ui, area: Rect, actions: &mut Vec<Action>) {
        let g = geo(area);
        let si = self.ctl.set_idx();
        let x0 = M + COL + 40.0;
        let y0 = 54.0;
        let hgt = 40.0;
        let layout = self.ctl.store.library.sets[si].layout;
        let columns = self.ctl.store.library.sets[si].columns;
        let cols_label = format!("{columns} col");
        let pills = toolbar_pills(ui, &cols_label, if g.wide { g.title_x } else { g.rcol_x - 12.0 } - x0, g.wide);
        let mut x = x0;
        for p in &pills {
            let rect = r(&g, x, y0 + (hgt - p.h) / 2.0, x + p.w, y0 + (hgt + p.h) / 2.0);
            x += p.w + p.gap;
            match p.id {
                "perform" => {
                    if pill(ui, rect, p.id, p.label, PANEL.peach, !self.ctl.edit_mode).on_hover_text("Click tiles to play them").clicked() {
                        actions.push(inv!("view.perform").into());
                    }
                }
                "edit" => {
                    if pill(ui, rect, p.id, p.label, PANEL.lilac, self.ctl.edit_mode).on_hover_text("Click tiles to edit them (⌘/Ctrl+E)").clicked() {
                        actions.push(inv!("view.edit").into());
                    }
                }
                "grid" => {
                    if pill(ui, rect, p.id, p.label, PANEL.peach, layout == Layout::Grid).on_hover_text("⌘/Ctrl+1").clicked() {
                        actions.push(inv!("set.layout", {"layout": "grid"}).into());
                    }
                }
                "list" => {
                    if pill(ui, rect, p.id, p.label, PANEL.periwinkle, layout == Layout::List).on_hover_text("⌘/Ctrl+2").clicked() {
                        actions.push(inv!("set.layout", {"layout": "list"}).into());
                    }
                }
                "cols" => {
                    let c = pill(ui, rect, p.id, p.label, PANEL.tan, false).on_hover_text("Tiles per row: click for more, right-click for fewer");
                    if c.clicked() || c.secondary_clicked() {
                        let next = if c.clicked() { columns % 12 + 1 } else { (columns + 10) % 12 + 1 };
                        actions.push(inv!("set.columns", {"columns": next}).into());
                    }
                }
                _ => {
                    if pill(ui, rect, p.id, p.label, PANEL.sky, false).on_hover_text("⌘/Ctrl+O, or drop files").clicked() {
                        actions.push(inv!("tile.add").into());
                    }
                }
            }
        }

        // Settings as the top bar's end cap; Stop All under it.
        let w = g.size.x;
        let cap = r(&g, g.cap_x, M, w - M, M + TOP);
        let resp = ui.interact(cap, Id::new("panel-settings"), Sense::click()).on_hover_text("Settings (⌘/Ctrl+,)");
        let fill = if resp.hovered() { theme::lighten(PANEL.peach, 0.18) } else { PANEL.peach };
        ui.painter().rect_filled(cap, CornerRadius { ne: 17, se: 17, ..Default::default() }, fill);
        caps_text(ui.painter(), cap.right_bottom() + vec2(-16.0, -4.0), Align2::RIGHT_BOTTOM, "Settings", condensed(16.0), PANEL.black);
        if resp.clicked() {
            actions.push(inv!("view.settings", {"open": !self.ctl.show_settings}).into());
        }
        if pill(ui, r(&g, g.rcol_x, y0, w - M, y0 + hgt), "stop", "Stop All", PANEL.red, false)
            .on_hover_text("Esc fades out, Shift+Esc stops at once")
            .clicked()
        {
            actions.push(inv!("transport.stopAll").into());
        }

        // The readouts: under the title when wide, under the pills when compact.
        let counter_x = if g.wide { g.title_x } else { x0 };
        self.panel_counter(ui, g.o + vec2(counter_x, 100.0), g.rcol_x - 12.0 - counter_x);
        self.panel_meter(ui, r(&g, g.rcol_x, 98.0, w - M, 154.0));
    }

    /// The focus voice's countdown, kept within `max_w`: a long title is cut short, and
    /// the position / length readout goes when it doesn't fit beside the countdown.
    fn panel_counter(&self, ui: &mut Ui, at: Pos2, max_w: f32) {
        let p = ui.painter();
        let sounding = self.ctl.statuses.values().filter(|s| s.state != PlayState::Paused).count();
        let head = |text: &str| {
            let galley = ui.fonts(|f| {
                let mut job = egui::text::LayoutJob::simple_singleline(text.to_uppercase(), condensed_semi(15.0), PANEL.dim);
                job.wrap.max_width = max_w;
                job.wrap.max_rows = 1;
                f.layout_job(job)
            });
            p.galley(at, galley, PANEL.dim);
        };
        match self.focus_voice() {
            Some((id, s)) => {
                let title = self.ctl.store.library.tile(id).map(|t| t.title.clone()).unwrap_or_default();
                head(&if sounding > 1 { format!("{title}  ·  {sounding} playing") } else { title });
                let color = if s.state == PlayState::Paused { PANEL.gold } else { PANEL.peach };
                let big = theme::paint_tabular(p, at + vec2(0.0, 14.0), Align2::LEFT_TOP, &format!("−{}", format_clock(s.remaining(), true)), condensed(40.0), color);
                let small = format!("{} / {}", format_clock(s.position, true), format_clock(s.length, true));
                let small_w = ui.fonts(|f| f.layout_no_wrap(small.clone(), condensed_semi(17.0), PANEL.dim).size().x);
                if big.right() + 14.0 + small_w <= at.x + max_w {
                    theme::paint_tabular(p, pos2(big.right() + 14.0, big.bottom() - 6.0), Align2::LEFT_BOTTOM, &small, condensed_semi(17.0), PANEL.dim);
                }
            }
            None => {
                head("Ready");
                theme::paint_tabular(p, at + vec2(0.0, 14.0), Align2::LEFT_TOP, "0:00.0", condensed(40.0), theme::shade(PANEL.peach, 0.4));
            }
        }
    }

    /// Segmented bar-graph meter: two rows of 16 segments, gold to amber to red, with the
    /// master volume as a draggable peach marker.
    fn panel_meter(&mut self, ui: &mut Ui, rect: Rect) {
        let resp = ui.interact(rect, Id::new("panel-meter"), Sense::click_and_drag())
            .on_hover_text("Master level. Drag to set the master volume; click the last segment to clear a clip.");
        const SEGS: usize = 16;
        let pitch = (rect.width() / SEGS as f32).floor();
        let seg_w = pitch - 3.0;
        let rows = Rect::from_min_size(rect.min + vec2(0.0, 26.0), vec2(pitch * SEGS as f32, 28.0));
        if let Some(pos) = resp.interact_pointer_pos() {
            let last = Rect::from_min_size(pos2(rows.left() + pitch * (SEGS - 1) as f32, rows.top()), vec2(pitch, rows.height()));
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
        caps_text(p, rect.left_top(), Align2::LEFT_TOP, &format!("Master  {db}"), condensed_semi(15.0), PANEL.dim);
        let (green_end, yellow_end) = (meter_frac(-12.0), meter_frac(-3.0));
        for ch in 0..2 {
            let lit = meter_frac(self.meter.level[ch]);
            let hold = meter_frac(self.meter.hold[ch]);
            for i in 0..SEGS {
                let f0 = i as f32 / SEGS as f32;
                let color = if f0 >= yellow_end { PANEL.red } else if f0 >= green_end { PANEL.amber } else { PANEL.gold };
                let on = f0 < lit || (hold > 0.0 && (hold - f0).abs() < 0.5 / SEGS as f32) || (i == SEGS - 1 && self.meter.clip);
                let seg = Rect::from_min_size(pos2(rows.left() + i as f32 * pitch, rows.top() + ch as f32 * 16.0), vec2(seg_w, 12.0));
                p.rect_filled(seg, 1.0, if on { color } else { Color32::from_rgb(0x2A, 0x1A, 0x0E) });
            }
        }
        let vx = rows.left() + meter_frac(gain_to_db(v)) * rows.width();
        p.add(egui::Shape::convex_polygon(
            vec![pos2(vx - 5.0, rows.top() - 9.0), pos2(vx + 5.0, rows.top() - 9.0), pos2(vx, rows.top() - 2.0)],
            PANEL.peach,
            Stroke::NONE,
        ));
    }

    pub(super) fn panel_sets(&mut self, ui: &mut Ui, area: Rect, actions: &mut Vec<Action>) {
        let g = geo(area);
        let colors = [PANEL.peach, PANEL.lilac, PANEL.periwinkle, PANEL.tan, PANEL.sky, PANEL.gold, PANEL.salmon, PANEL.violet];
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
        let scroll_id = Id::new("panel-sets-scroll");
        let follow_id = Id::new("panel-sets-follow");
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
            let resp = ui.interact(visible, Id::new(("panel-set", set.id)), Sense::click());
            let on = Some(set.id) == selected;
            let base = colors[i % colors.len()];
            let fill = if resp.hovered() { theme::lighten(base, 0.15) } else { base };
            let p = &clip;
            p.rect_filled(rect, 0.0, fill);
            if on {
                p.rect_filled(Rect::from_min_size(rect.min, vec2(10.0, block_h)), 0.0, PANEL.black);
            }
            let playing = set.tiles.iter().any(|t| self.ctl.statuses.contains_key(&t.id));
            if !compact {
                let code = format!("{:02}-{:03}{}", i + 1, set.tiles.len(), if playing { "  ·  LIVE" } else { "" });
                caps_text(p, rect.left_top() + vec2(18.0, 6.0), Align2::LEFT_TOP, &code, condensed_semi(13.0), PANEL.black);
            } else if playing {
                caps_text(p, rect.left_center() + vec2(18.0, 0.0), Align2::LEFT_CENTER, "LIVE", condensed_semi(13.0), PANEL.black);
            }
            if matches!(&self.renaming, Some((id, _, _)) if *id == set.id) {
                rename_rect = Some(rect);
            } else {
                let size = match (compact, on) {
                    (true, _) => 17.0,
                    (false, true) => 21.0,
                    (false, false) => 19.0,
                };
                let galley = ui.fonts(|f| {
                    let mut job = egui::text::LayoutJob::simple_singleline(set.name.to_uppercase(), condensed(size), PANEL.black);
                    job.wrap.max_width = COL - if compact && playing { 60.0 } else { 30.0 };
                    job.wrap.max_rows = 1;
                    f.layout_job(job)
                });
                let y_text = if compact { rect.center().y - galley.size().y / 2.0 } else { rect.bottom() - 6.0 - galley.size().y };
                p.galley(pos2(rect.right() - 10.0 - galley.size().x, y_text), galley, PANEL.black);
            }
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
        // Amber edge bars say there are more sets above or below.
        if scroll > 0.5 {
            ui.painter().rect_filled(Rect::from_min_size(list_rect.min, vec2(COL, 4.0)), 0.0, PANEL.amber);
        }
        if scroll < max_scroll - 0.5 {
            ui.painter().rect_filled(Rect::from_min_size(pos2(list_rect.left(), list_rect.bottom() - 4.0), vec2(COL, 4.0)), 0.0, PANEL.amber);
        }
        let y = (top + content_h).min(list_bottom) + GAP;
        if let (Some(rect), Some((_, draft, focused))) = (rename_rect, &mut self.renaming) {
            let edit = ui.put(rect.shrink2(vec2(14.0, 16.0)).translate(vec2(4.0, 8.0)), egui::TextEdit::singleline(draft));
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
        if pill_block(ui, new, "new-set", "+ New Set", PANEL.sky).on_hover_text("⌘/Ctrl+Shift+N").clicked() {
            actions.push(inv!("set.create").into());
        }
        let y = y + NEW_H + GAP;
        // The rest of the column runs on into the bottom elbow.
        let elbow_top = g.size.y - M - BOT - BOT_COL;
        if elbow_top - y > 0.0 {
            ui.painter().rect_filled(r(&g, M, y, M + COL, elbow_top + 1.0), 0.0, PANEL.violet);
        }
    }

    pub(super) fn panel_status(&mut self, ui: &mut Ui, area: Rect) {
        let g = geo(area);
        let p = ui.painter();
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
        let left = caps_text(p, g.o + vec2(g.bottom_bar_end + 12.0, yc), Align2::LEFT_CENTER, &left, condensed(22.0), PANEL.peach);
        // The right-hand text runs from the middle block (or the left text) to the end cap.
        let right_from = bottom_mid(&g).map_or(left.right() - g.o.x, |mid| mid + 60.0) + 24.0;
        let (right, color) = match (&self.ctl.notice, &self.ctl.engine.error, &self.ctl.engine.device_name) {
            (Some(n), _, _) if n.at.elapsed() < Duration::from_secs(if n.error { 10 } else { 4 }) => {
                (n.text.clone(), if n.error { PANEL.red } else { PANEL.peach })
            }
            (_, Some(err), _) => (err.clone(), PANEL.red),
            (_, None, Some(name)) => (format!("{name}  ·  {} kHz", self.ctl.engine.sample_rate() as f32 / 1000.0), PANEL.lilac),
            _ => ("No output".to_owned(), PANEL.dim),
        };
        let galley = ui.fonts(|f| {
            let mut job = egui::text::LayoutJob::simple_singleline(right.to_uppercase(), condensed_semi(17.0), color);
            job.wrap.max_width = (g.size.x - M - 52.0) - right_from;
            job.wrap.max_rows = 1;
            f.layout_job(job)
        });
        p.galley(g.o + vec2(g.size.x - M - 52.0 - galley.size().x, yc - galley.size().y / 2.0), galley, color);
    }

}

/// The cue list's GO header in PANEL style.
pub(super) fn go_header(ui: &mut Ui, next_title: Option<String>, auto_advance: &mut bool, actions: &mut Vec<Action>) {
    // Narrow windows get shorter pills, and the next cue's title is cut short rather
    // than run under them.
    let narrow = ui.available_width() < 640.0;
    let (top_w, auto_w) = if narrow { (80.0, 90.0) } else { (150.0, 170.0) };
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(140.0, 52.0), Sense::hover());
        ui.add_enabled_ui(next_title.is_some(), |ui| {
            if pill(ui, rect, "go", "Go", PANEL.gold, false).on_hover_text("Space").clicked() {
                actions.push(inv!("cue.go").into());
            }
        });
        ui.add_space(10.0);
        let label_w = (ui.available_width() - top_w - auto_w - 3.0 * ui.spacing().item_spacing.x).max(40.0);
        ui.vertical(|ui| {
            ui.set_max_width(label_w);
            ui.add(egui::Label::new(RichText::new("NEXT CUE").font(condensed_semi(15.0)).color(PANEL.dim)).truncate());
            let title = next_title.unwrap_or_else(|| "End of list".into()).to_uppercase();
            ui.add(egui::Label::new(RichText::new(title).font(condensed(26.0)).color(PANEL.peach)).truncate());
        });
        ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(top_w, 34.0), Sense::hover());
            if pill(ui, rect, "top", if narrow { "Top" } else { "Back to Top" }, PANEL.lilac, false).on_hover_text("Back to Top").clicked() {
                actions.push(inv!("cue.top").into());
            }
            let (rect, _) = ui.allocate_exact_size(vec2(auto_w, 34.0), Sense::hover());
            if pill(ui, rect, "auto", if narrow { "Auto" } else { "Auto-Advance" }, PANEL.periwinkle, *auto_advance)
                .on_hover_text("Auto-advance: when a cue finishes on its own, start the next one")
                .clicked()
            {
                *auto_advance = !*auto_advance;
            }
        });
    });
}

/// A square-cornered block that behaves like a button.
fn pill_block(ui: &mut Ui, rect: Rect, id: &str, label: &str, fill: Color32) -> egui::Response {
    let resp = ui.interact(rect, Id::new(("panel-block", id)), Sense::click());
    let fill = if resp.hovered() { theme::lighten(fill, 0.18) } else { fill };
    ui.painter().rect_filled(rect, 0.0, fill);
    caps_text(ui.painter(), rect.right_bottom() + vec2(-10.0, -6.0), Align2::RIGHT_BOTTOM, label, condensed(18.0), PANEL.black);
    resp
}

/// The narrowest PANEL tile the grid will draw; below it the grid shows fewer columns.
pub const TILE_MIN_WIDTH: f32 = 150.0;

/// Below this width a PANEL tile stacks its readouts: the time moves up beside the key
/// code and the title gets the whole bottom row.
const TILE_STACK_BELOW: f32 = 232.0;

/// Tile height for a given width in the PANEL grid (wider, shorter blocks than Console).
pub fn tile_height(w: f32) -> f32 {
    (w * 0.5).clamp(96.0, 150.0)
}

/// A tile as a panel block: rounded left edge, code top-left, label bottom-right,
/// black waveform; brighter while playing.
pub fn paint_tile(ui: &Ui, rect: Rect, tile: &Tile, status: Option<&VoiceStatus>, time: &str, f: TileFlags) {
    let p = ui.painter();
    let sounding = status.is_some_and(|s| matches!(s.state, PlayState::Playing | PlayState::Stopping));
    let paused = status.is_some_and(|s| s.state == PlayState::Paused);
    let mut base = panel_tile_fill(tile.color);
    if tile.file.is_none() {
        base = theme::shade(base, 0.45);
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
    let rad = (rect.height() / 2.8).min(255.0) as u8;
    let corners = CornerRadius { nw: rad, sw: rad, ne: 0, se: 0 };
    p.rect_filled(rect, corners, fill);
    if let (Some(tex), ImageLayer::Underlay) = (f.picture, tile.image_layer) {
        paint_picture(p, rect, corners, tile, tex);
    }
    let ink = PANEL.black;
    let wave = Rect::from_min_max(rect.min + vec2(22.0, 38.0), pos2(rect.right() - 16.0, rect.bottom() - 40.0));
    let played_x = status.map(|s| wave.left() + wave.width() * s.progress());
    if let Some((peaks, duration)) = f.waveform {
        let rest = theme::with_alpha(ink, if status.is_some() { 0.3 } else { 0.62 });
        paint_waveform(p, wave, tile, peaks, duration, played_x, ink, rest);
    } else if tile.file.is_none() {
        caps_text(p, wave.center(), Align2::CENTER_CENTER, "Drop audio here", condensed_semi(16.0), ink);
    }
    if let Some(px) = played_x {
        p.rect_filled(Rect::from_min_size(pos2(px - 1.0, wave.top() - 4.0), vec2(2.0, wave.height() + 8.0)), 0.0, ink);
    }
    if let (Some(tex), ImageLayer::Overlay) = (f.picture, tile.image_layer) {
        paint_picture(p, rect, corners, tile, tex);
    }
    let code = tile.hotkey.map(|k| if k.is_ascii_digit() { format!("0{k}") } else { k.to_ascii_uppercase().to_string() }).unwrap_or_else(|| "--".into());
    let code_rect = caps_text(p, rect.left_top() + vec2(18.0, 6.0), Align2::LEFT_TOP, &code, condensed(24.0), ink);
    let badge = if sounding {
        "Playing".to_owned()
    } else if paused {
        "Paused".to_owned()
    } else {
        mode_badge_text(tile)
    };
    let time_color = if f.failed { PANEL.red } else { ink };
    let stacked = rect.width() < TILE_STACK_BELOW;
    // Wide: badge top-right, time bottom-left beside the title. Stacked: time top-right,
    // badge to its left, title alone along the bottom.
    let (badge_right, title_left) = if stacked {
        let t = theme::paint_tabular(p, rect.right_top() + vec2(-12.0, 10.0), Align2::RIGHT_TOP, time, condensed_semi(18.0), time_color);
        (t.left() - 8.0, rect.left() + 18.0)
    } else {
        let t = theme::paint_tabular(p, rect.left_bottom() + vec2(18.0, -8.0), Align2::LEFT_BOTTOM, time, condensed_semi(18.0), time_color);
        (rect.right() - 12.0, t.right() + 12.0)
    };
    // A narrow tile drops the badge rather than run it into the key code.
    let badge_w = ui.fonts(|f| f.layout_no_wrap(badge.to_uppercase(), condensed_semi(13.0), ink).size().x);
    if !badge.is_empty() && badge_right - badge_w >= code_rect.right() + 8.0 {
        caps_text(p, pos2(badge_right, rect.top() + 10.0), Align2::RIGHT_TOP, &badge, condensed_semi(13.0), ink);
    }
    let room = (rect.right() - 12.0 - title_left).max(20.0);
    // A stacked title that doesn't fit at full size steps down once before truncating.
    let size = if stacked && ui.fonts(|f| f.layout_no_wrap(tile.title.to_uppercase(), condensed(22.0), ink).size().x) > room { 18.0 } else { 22.0 };
    let galley = ui.fonts(|fonts| {
        let mut job = egui::text::LayoutJob::simple_singleline(tile.title.to_uppercase(), condensed(size), ink);
        job.wrap.max_width = room;
        job.wrap.max_rows = 1;
        fonts.layout_job(job)
    });
    p.galley(pos2(rect.right() - 12.0 - galley.size().x, rect.bottom() - 6.0 - galley.size().y), galley, ink);
    if f.selected || f.drop_hover {
        p.rect_stroke(rect.expand(3.0), corners, Stroke::new(2.0_f32, PANEL.amber), StrokeKind::Outside);
    }
}

pub fn paint_add_slot(ui: &Ui, rect: Rect, hovered: bool) {
    let p = ui.painter();
    let color = if hovered { PANEL.peach } else { PANEL.dim };
    let rad = rect.height() / 2.8;
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
    let full = ui.fonts(|f| f.layout_no_wrap("+ ADD SOUNDS".into(), condensed(20.0), color).size().x);
    let label = if full <= rect.width() - 14.0 - rad { "+ Add Sounds" } else { "+ Add" };
    caps_text(p, rect.right_bottom() + vec2(-14.0, -8.0), Align2::RIGHT_BOTTOM, label, condensed(20.0), color);
}

/// A cue-list row as a panel block.
#[allow(clippy::too_many_arguments)]
pub fn paint_list_row(ui: &Ui, rect: Rect, index: usize, tile: &Tile, status: Option<&VoiceStatus>, time: &str, is_next: bool, f: TileFlags) {
    let p = ui.painter();
    let ink = PANEL.black;
    let base = panel_tile_fill(tile.color);
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
        p.rect_filled(Rect::from_min_size(pos2(rect.right() - 8.0, rect.top()), vec2(8.0, rect.height())), 0.0, PANEL.amber);
    }
    let cy = rect.center().y;
    theme::paint_tabular(p, pos2(rect.left() + 20.0, cy), Align2::LEFT_CENTER, &format!("{:02}", index + 1), condensed(26.0), ink);
    let key = tile.hotkey.map(|k| k.to_ascii_uppercase().to_string()).unwrap_or_default();
    caps_text(p, pos2(rect.left() + 64.0, cy), Align2::LEFT_CENTER, &key, condensed_semi(16.0), ink);
    let title = ui.fonts(|fonts| {
        let mut job = egui::text::LayoutJob::simple_singleline(tile.title.to_uppercase(), condensed(22.0), ink);
        job.wrap.max_width = rect.width() * 0.3;
        job.wrap.max_rows = 1;
        fonts.layout_job(job)
    });
    let title_w = title.size().x;
    p.galley(pos2(rect.left() + 90.0, cy - title.size().y / 2.0), title, ink);
    let t = theme::paint_tabular(p, pos2(rect.right() - 24.0, cy), Align2::RIGHT_CENTER, time, condensed_semi(18.0), ink);
    let wave = Rect::from_min_max(pos2(rect.left() + 110.0 + title_w, rect.top() + 9.0), pos2(t.left() - 20.0, rect.bottom() - 9.0));
    if let Some((peaks, duration)) = f.waveform {
        let played_x = status.map(|s| wave.left() + wave.width() * s.progress());
        paint_waveform(p, wave, tile, peaks, duration, played_x, ink, theme::with_alpha(ink, if status.is_some() { 0.3 } else { 0.55 }));
    }
    if f.selected || f.drop_hover {
        p.rect_stroke(rect.expand(3.0), corners, Stroke::new(2.0_f32, PANEL.amber), StrokeKind::Outside);
    }
}
