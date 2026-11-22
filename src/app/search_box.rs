//! The search box at the top of the window (SPEC "Find", D-263..D-266): a field in
//! Console's toolbar or in a gap in Panel's top bar, and the list of what it finds in
//! every set, under it while it has the keyboard. The board isn't rearranged: the tiles
//! the search doesn't find are dimmed where they are.
//!
//! Keys in the box: Up and Down choose a hit, Enter goes to it (its set, selected, its
//! pad focused, so a second Enter plays it), ⌘/Ctrl+Enter plays it and stays, Esc
//! empties the box and gives the keyboard back to the board.

use super::*;
use crate::controller::SearchHit;
use crate::search::Query;

/// The search box's widget id (one main window, one box).
pub fn box_id() -> egui::Id {
    egui::Id::unique("search-box")
}

/// The most hits the list shows; the rest are a count at its foot.
const LIST_MAX: usize = 50;

impl StingerApp {
    /// The search box's query while it holds words, for dimming the board.
    pub(super) fn search_query(&self) -> Option<Query> {
        self.ctl.search.as_deref().map(Query::new).filter(|q| !q.is_empty())
    }

    /// Whether the box needs its room: it has text, the keyboard, or is about to get it.
    pub(super) fn search_active(&self, ctx: &egui::Context) -> bool {
        self.ctl.search.is_some() || self.search_focus || ctx.memory(|m| m.has_focus(box_id()))
    }

    /// The magnifier key that stands for the box where the toolbar is too narrow for it.
    pub(super) fn search_key(&self, ui: &mut Ui, rect: Rect, panel: bool, actions: &mut Vec<Action>) {
        let resp = ui.interact(rect, egui::Id::unique("search-key"), Sense::click());
        let resp = theme::role(resp, AxRole::Button, "Find", None);
        let keys = ui.ctx().format_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::F));
        let resp = theme::hint_keys(resp, "Find tiles in every set by title, file name, notes or subtitles", &keys);
        let ink = if panel {
            let pal = theme::palette(ui.ctx());
            if resp.hovered() { theme::lighten(pal.amber, 0.25) } else { pal.amber }
        } else {
            let t = Tokens::get(ui.ctx());
            let fill = if resp.hovered() { t.key_hover } else { t.key };
            ui.painter().rect(rect, 6.0, fill, Stroke::new(1.0, t.key_border), StrokeKind::Inside);
            t.key_text()
        };
        paint_magnifier(ui.painter(), rect.center(), 14.0, ink);
        if resp.clicked() {
            actions.push(inv!("view.search").into());
        }
    }

    /// The search box in `rect`, and the list of hits under it while it has the keyboard.
    pub(super) fn search_box(&mut self, ui: &mut Ui, rect: Rect, panel: bool, actions: &mut Vec<Action>) {
        let ctx = ui.ctx().clone();
        let id = box_id();
        let mut query = self.ctl.search.clone().unwrap_or_default();
        let hits = self.ctl.search_results(&query, None);
        self.search_pick = self.search_pick.min(hits.len().saturating_sub(1));
        // Keys for the list, taken before the field sees them. egui lets go of the
        // focus on Esc before the field is drawn, so Esc counts if it had it last frame.
        let esc = std::mem::take(&mut self.search_had_focus) && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape));
        if esc {
            actions.push(inv!("view.search", {"clear": true}).into());
        } else if ctx.memory(|m| m.has_focus(id)) {
            ctx.input_mut(|i| {
                if i.consume_key(Modifiers::NONE, Key::ArrowDown) && !hits.is_empty() {
                    self.search_pick = (self.search_pick + 1).min(hits.len() - 1);
                    self.search_scroll = true;
                }
                if i.consume_key(Modifiers::NONE, Key::ArrowUp) {
                    self.search_pick = self.search_pick.saturating_sub(1);
                    self.search_scroll = true;
                }
                let index = self.search_pick + 1;
                if i.consume_key(Modifiers::COMMAND, Key::Enter) && !hits.is_empty() {
                    actions.push(inv!("view.search", {"play": true, "index": index}).into());
                } else if i.consume_key(Modifiers::NONE, Key::Enter) && !hits.is_empty() {
                    actions.push(inv!("view.search", {"go": true, "index": index}).into());
                }
            });
        }

        // The field: Console's sunken box, or text in the black gap of Panel's bar.
        let (font, ink, hint_ink) = if panel {
            let pal = theme::palette(&ctx);
            (theme::condensed(22.0), pal.peach, theme::panel_dim(&ctx))
        } else {
            let t = Tokens::get(&ctx);
            (theme::regular(14.0), t.text, t.text_dim)
        };
        // The focus ring (theme::focus_ring) goes round the whole box in Console; Panel
        // underlines the gap instead.
        let focused = ctx.memory(|m| m.has_focus(id));
        if !panel {
            let t = Tokens::get(&ctx);
            ui.painter().rect(rect, 6.0, t.window, Stroke::new(1.0, t.key_border), StrokeKind::Inside);
        } else if focused {
            let pal = theme::palette(&ctx);
            let under = Rect::from_min_max(pos2(rect.left() + 6.0, rect.bottom() - 4.0), pos2(rect.right() - 6.0, rect.bottom() - 2.0));
            ui.painter().rect_filled(under, 0.0, pal.lilac);
        }
        paint_magnifier(ui.painter(), pos2(rect.left() + 15.0, rect.center().y), 13.0, hint_ink);
        let clear_w = if query.is_empty() { 0.0 } else { 24.0 };
        let keys = ctx.format_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::F));
        let hint = if panel { format!("FIND  {keys}") } else { format!("Find  {keys}") };
        let edit = egui::TextEdit::singleline(&mut query)
            .id(id)
            .frame(egui::Frame::NONE.inner_margin(egui::Margin { left: 28, right: 4 + clear_w as i8, top: 0, bottom: 0 }))
            .font(font)
            .text_color(ink)
            .hint_text(RichText::new(hint).color(hint_ink))
            .align(Align2::LEFT_CENTER)
            .desired_width(rect.width());
        let resp = ui.put(rect, edit);
        let resp = theme::hint_keys(
            theme::name(resp, "Find"),
            "Find tiles in every set by title, file name, notes or subtitles; the board dims the rest",
            "Up and Down choose, Enter goes to it, ⌘Enter plays it, Esc clears",
        );
        if std::mem::take(&mut self.search_focus) {
            resp.request_focus();
        }
        self.search_had_focus = resp.has_focus();
        if resp.changed() {
            self.search_pick = 0;
            actions.push(inv!("view.search", {"query": query}).into());
        }
        if clear_w > 0.0 {
            let x = Rect::from_center_size(pos2(rect.right() - 4.0 - clear_w / 2.0, rect.center().y), vec2(20.0, 20.0));
            let c = ui.interact(x, egui::Id::unique("search-clear"), Sense::click());
            let c = theme::hint_keys(theme::role(c, AxRole::Button, "Clear the search", None), "Clear the search and show the whole set again", "Esc in the box");
            let ink = if c.hovered() { ink } else { hint_ink };
            let (a, b) = (x.center() - vec2(4.5, 4.5), x.center() + vec2(4.5, 4.5));
            ui.painter().line_segment([a, b], Stroke::new(1.6, ink));
            ui.painter().line_segment([pos2(a.x, b.y), pos2(b.x, a.y)], Stroke::new(1.6, ink));
            if c.clicked() {
                actions.push(inv!("view.search", {"clear": true}).into());
            }
        }

        // The list shows while the box has the keyboard, and while the pointer is on it
        // (a click on a hit takes the keyboard from the box before it lands).
        let over_list = self.search_list_rect.is_some_and(|r| ctx.pointer_hover_pos().is_some_and(|p| r.contains(p)));
        let show = !query.trim().is_empty() && (resp.has_focus() || over_list || self.search_list_pinned);
        self.search_list_rect = None;
        if show {
            self.search_list(&ctx, rect, panel, &hits, actions);
        }
    }

    /// The hits under the box: title and set, and the line that matched when it wasn't
    /// the title, each with a play key.
    fn search_list(&mut self, ctx: &egui::Context, anchor: Rect, panel: bool, hits: &[SearchHit], actions: &mut Vec<Action>) {
        let screen = ctx.content_rect();
        let width = anchor.width().max(380.0).min(screen.width() - 16.0);
        let left = anchor.left().min(screen.right() - 8.0 - width).max(screen.left() + 8.0);
        let (fill, edge, ink, dim, pick_fill) = if panel {
            let pal = theme::palette(ctx);
            (pal.black, Stroke::new(2.0, pal.amber), pal.peach, theme::panel_dim(ctx), theme::with_alpha(pal.lilac, 0.35))
        } else {
            let t = Tokens::get(ctx);
            (t.panel2, Stroke::new(1.0, t.border_light), t.text, t.text_dim, t.selected)
        };
        let lib = &self.ctl.store.library;
        let shown = egui::Area::new(egui::Id::unique("search-list"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos2(left, anchor.bottom() + 4.0))
            .show(ctx, |ui| {
                egui::Frame::new().fill(fill).stroke(edge).corner_radius(if panel { 0 } else { 8 }).inner_margin(6).show(ui, |ui| {
                    ui.set_width(width - 12.0);
                    ui.ctx().accesskit_node_builder(ui.unique_id(), |n| {
                        n.set_role(egui::accesskit::Role::List);
                        n.set_label("Search results");
                    });
                    if hits.is_empty() {
                        ui.label(RichText::new("No tiles match").color(dim));
                        return;
                    }
                    egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        for (n, h) in hits.iter().take(LIST_MAX).enumerate() {
                            let set = &lib.sets[h.set];
                            let tile = &set.tiles[h.position];
                            let other = h.matched.iter().find(|m| m.field != crate::search::Field::Title);
                            // Laid out first: the row is as tall as its lines.
                            let full_w = ui.available_width();
                            let text_w = full_w - 30.0 - 22.0;
                            let title_font = if panel { theme::condensed(19.0) } else { theme::semibold(13.5) };
                            let small = if panel { theme::condensed_semi(14.0) } else { theme::regular(12.0) };
                            let set_w = ui.painter().layout_no_wrap(set.name.clone(), small.clone(), dim).size().x.min(text_w * 0.4);
                            let title_g = clipped(ui, &tile.title, title_font, ink, text_w - set_w - 10.0);
                            let set_g = clipped(ui, &set.name, small.clone(), dim, set_w);
                            let line_g = other.map(|m| clipped(ui, &format!("{}: {}", m.field.label(), m.text), small, dim, text_w));
                            let height = 8.0 + title_g.size().y + line_g.as_ref().map_or(0.0, |g| g.size().y);
                            let (rect, _) = ui.allocate_exact_size(vec2(full_w, height.max(26.0)), Sense::hover());
                            let row = ui.interact(rect, egui::Id::unique(("search-hit", h.tile)), Sense::click());
                            let picked = n == self.search_pick;
                            if picked || row.hovered() {
                                ui.painter().rect_filled(rect, if panel { 0.0 } else { 5.0 }, if picked { pick_fill } else { theme::with_alpha(pick_fill, 0.5) });
                            }
                            if picked && std::mem::take(&mut self.search_scroll) {
                                ui.scroll_to_rect(rect, None);
                            }
                            // The colour of its pad, as a bar at the row's start.
                            let swatch = Rect::from_min_size(rect.min + vec2(4.0, 5.0), vec2(4.0, rect.height() - 10.0));
                            ui.painter().rect_filled(swatch, 1.0, if panel { theme::panel_tile(ctx, tile.color) } else { theme::console_tile(ctx, tile.color) });
                            let play = Rect::from_min_size(pos2(rect.right() - 30.0, rect.center().y - 11.0), vec2(26.0, 22.0));
                            let top = rect.top() + (rect.height() - height) / 2.0 + 4.0;
                            let title_h = title_g.size().y;
                            ui.painter().galley(pos2(rect.left() + 14.0, top), title_g, ink);
                            ui.painter().galley(pos2(play.left() - 8.0 - set_g.size().x, top + title_h - set_g.size().y), set_g, dim);
                            if let Some(g) = line_g {
                                ui.painter().galley(pos2(rect.left() + 14.0, top + title_h), g, dim);
                            }
                            let label = match other {
                                Some(m) => format!("{}, in {}; {}: {}", tile.title, set.name, m.field.label(), m.text),
                                None => format!("{}, in {}", tile.title, set.name),
                            };
                            let row = theme::hint(theme::role(row, AxRole::Button, label, Some(picked)), "Go to this tile: its set, selected, Enter then plays it");
                            let p = ui.interact(play, egui::Id::unique(("search-play", h.tile)), Sense::click());
                            let p = theme::hint(theme::role(p, AxRole::Button, format!("Play {}", tile.title), None), "Play it, and keep the search");
                            let tri = if p.hovered() { ink } else { dim };
                            let c = play.center();
                            ui.painter().add(egui::Shape::convex_polygon(vec![c + vec2(-4.0, -6.0), c + vec2(6.0, 0.0), c + vec2(-4.0, 6.0)], tri, Stroke::NONE));
                            if p.clicked() {
                                actions.push(inv!("view.search", {"play": true, "index": n + 1}).into());
                            } else if row.clicked() {
                                actions.push(inv!("view.search", {"go": true, "index": n + 1}).into());
                            }
                        }
                        if hits.len() > LIST_MAX {
                            ui.label(RichText::new(format!("and {} more: type more words", hits.len() - LIST_MAX)).color(dim));
                        }
                    });
                });
            });
        self.search_list_rect = Some(shown.response.rect);
    }

    /// After Esc in the box: the keyboard back to the board, on the selected tile when
    /// it's in this set.
    pub(super) fn search_focus_board(&mut self, ctx: &egui::Context) {
        if !std::mem::take(&mut self.search_to_board) {
            return;
        }
        ctx.memory_mut(|m| m.surrender_focus(box_id()));
        let sel = self.ctl.selected_tile.filter(|id| self.ctl.current_set().tiles.iter().any(|t| t.id == *id));
        self.focus_script = sel;
    }
}

/// One line of text, cut short with an ellipsis to fit `width`.
fn clipped(ui: &Ui, text: &str, font: egui::FontId, color: Color32, width: f32) -> Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap.max_width = width.max(10.0);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    ui.fonts_mut(|f| f.layout_job(job))
}

/// A magnifying glass, `size` across, centred on `c`.
pub fn paint_magnifier(p: &egui::Painter, c: Pos2, size: f32, color: Color32) {
    let r = size * 0.32;
    let lens = c - vec2(size * 0.1, size * 0.1);
    p.circle_stroke(lens, r, Stroke::new(size * 0.13, color));
    let d = std::f32::consts::FRAC_1_SQRT_2;
    let from = lens + vec2(d, d) * r;
    p.line_segment([from, from + vec2(d, d) * size * 0.32], Stroke::new(size * 0.16, color));
}

/// Dims a tile the search doesn't find, where it is (the board keeps its order).
pub(super) fn dim_unmatched(ui: &Ui, rect: Rect) {
    let bg = if theme::is_panel(ui.ctx()) { theme::palette(ui.ctx()).black } else { Tokens::get(ui.ctx()).window };
    ui.painter().rect_filled(rect.expand(2.0), 6.0, theme::with_alpha(bg, 0.72));
}
