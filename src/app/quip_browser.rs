//! The QuipClipper window (M21, D-098): a connected server's clips library, to preview
//! and add from. Listings are fetched off the ui thread (src/quip/clips.rs); Preview and
//! Add run the `quip.preview` and `quip.add` commands, like every other action.

use super::*;
use crate::quip::clips::{self, Clip, ClipKind, Listing, PreviewState, Query, PREVIEW_ID};
use crate::quip::watch::host;
use std::sync::mpsc::{Receiver, TryRecvError};

/// How long typing pauses before the search goes to the server.
const DEBOUNCE: Duration = Duration::from_millis(300);
/// How long the connected servers are trusted before they're read again.
const SERVERS_TTL: Duration = Duration::from_secs(3);
/// The window's id, for its layer: keys typed while it has the focus aren't tile keys.
const WINDOW_ID: &str = "quip-browser";
const ROW_H: f32 = 46.0;

/// A listing on its way from the server.
type Answer = Receiver<Result<Arc<Listing>, String>>;

/// A clip dragged out of the window, onto the board.
#[derive(Clone, Debug)]
pub(super) struct QuipDrag {
    server: String,
    clip: String,
    title: String,
}

/// The window's state while the app runs (kept when it's closed).
#[derive(Default)]
pub(super) struct QuipBrowser {
    server: Option<String>,
    servers: Vec<String>,
    servers_read: Option<Instant>,
    folder: String,
    /// The search box, and the words sent to the server (after the pause).
    typing: String,
    query: String,
    typed: Option<Instant>,
    kind: Option<ClipKind>,
    oldest_first: bool,
    /// What's showing, and the query it answers.
    shown: Option<(String, Query, Arc<Listing>)>,
    error: Option<String>,
    loading: Option<(String, Query, Answer)>,
    /// The query that last failed: not asked again until it changes or Refresh.
    failed: Option<(String, Query)>,
    /// The new clips (`view.quip {new: true}`), while they're showing instead of a folder.
    /// The controller's watcher counts them (src/quip/watch.rs).
    new_view: Option<Arc<Listing>>,
    /// The selected row (a clip path), and a row to give the focus next frame.
    selected: Option<String>,
    focus: Option<String>,
    focus_search: bool,
}

/// Say a name a screen reader reads as words, not as a symbol: quoted when it starts
/// with anything but a letter or digit.
fn speakable(s: &str) -> String {
    if s.starts_with(|c: char| c.is_alphanumeric()) {
        s.to_owned()
    } else {
        format!("“{s}”")
    }
}

fn kind_word(k: Option<ClipKind>) -> &'static str {
    match k {
        Some(ClipKind::Audio) => "audio clip",
        Some(ClipKind::Video) => "video clip",
        Some(ClipKind::Gif) => "GIF",
        Some(ClipKind::Set) => "set file",
        None => "file",
    }
}

/// "1.2 seconds", "1:12": a length for a screen reader.
fn spoken(s: f64) -> String {
    if s < 60.0 {
        format!("{s:.1} seconds")
    } else {
        format_clock(s, false)
    }
}

/// A small drawing of what a row is: a note, a film frame, a set's tiles, a GIF.
fn paint_kind(p: &egui::Painter, c: Pos2, kind: Option<ClipKind>, color: Color32) {
    let s = Stroke::new(1.6_f32, color);
    match kind {
        Some(ClipKind::Audio) => {
            p.circle_filled(c + vec2(-3.0, 4.0), 3.2, color);
            p.line_segment([c + vec2(0.0, 4.0), c + vec2(0.0, -7.0)], s);
            p.line_segment([c + vec2(0.0, -7.0), c + vec2(5.0, -4.0)], s);
        }
        Some(ClipKind::Video) => {
            let r = Rect::from_center_size(c, vec2(16.0, 12.0));
            p.rect_stroke(r, 2.0, s, StrokeKind::Inside);
            p.add(egui::Shape::convex_polygon(vec![c + vec2(-2.5, -3.5), c + vec2(4.0, 0.0), c + vec2(-2.5, 3.5)], color, Stroke::NONE));
        }
        Some(ClipKind::Set) => {
            for (dx, dy) in [(-4.0, -4.0), (4.0, -4.0), (-4.0, 4.0), (4.0, 4.0)] {
                p.rect_filled(Rect::from_center_size(c + vec2(dx, dy), vec2(6.5, 6.5)), 1.5, color);
            }
        }
        _ => {
            p.rect_stroke(Rect::from_center_size(c, vec2(16.0, 12.0)), 2.0, s, StrokeKind::Inside);
        }
    }
}

/// A folder: a tab on a box.
fn paint_folder(p: &egui::Painter, c: Pos2, color: Color32) {
    p.rect_filled(Rect::from_min_size(c + vec2(-8.0, -6.5), vec2(7.0, 3.0)), 1.0, color);
    p.rect_filled(Rect::from_center_size(c + vec2(0.0, 1.0), vec2(16.0, 11.0)), 2.0, color);
}

impl QuipBrowser {
    /// The query the window wants answered now.
    fn query(&self) -> Query {
        Query { folder: self.folder.clone(), recursive: !self.query.is_empty(), q: self.query.clone(), kind: self.kind, since: None, newest: !self.oldest_first }
    }

    fn go(&mut self, folder: String) {
        self.folder = folder;
        self.typing.clear();
        self.query.clear();
        self.typed = None;
        self.new_view = None;
        self.selected = None;
    }

    fn servers(&mut self, data_dir: &std::path::Path) {
        if self.servers_read.is_some_and(|at| at.elapsed() < SERVERS_TTL) {
            return;
        }
        self.servers_read = Some(Instant::now());
        self.servers = crate::quip::list(data_dir).map(|l| l.into_iter().map(|(s, _)| s).collect()).unwrap_or_default();
        if !self.server.as_ref().is_some_and(|s| self.servers.contains(s)) {
            self.server = self.servers.first().cloned();
            self.shown = None;
            self.go(String::new());
        }
    }

    /// Fetch what should show, if it isn't showing or on its way, once typing pauses;
    /// and take what has arrived.
    fn fetch(&mut self, data_dir: &std::path::Path, ctx: &egui::Context) {
        if let Some((server, query, rx)) = &self.loading {
            match rx.try_recv() {
                Ok(Ok(l)) => {
                    self.shown = Some((server.clone(), query.clone(), l));
                    self.error = None;
                    self.failed = None;
                    self.loading = None;
                }
                Ok(Err(e)) => {
                    self.error = Some(crate::quip::sentence_case(&e));
                    self.failed = Some((server.clone(), query.clone()));
                    self.loading = None;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => self.loading = None,
            }
        }
        if let Some(at) = self.typed {
            if at.elapsed() < DEBOUNCE {
                ctx.request_repaint_after(DEBOUNCE - at.elapsed());
                return;
            }
            self.query = self.typing.split_whitespace().collect::<Vec<_>>().join(" ");
            self.typed = None;
        }
        let Some(server) = self.server.clone() else { return };
        let want = self.query();
        let showing = self.shown.as_ref().is_some_and(|(s, q, _)| *s == server && *q == want);
        let asked = self.loading.as_ref().is_some_and(|(s, q, _)| *s == server && *q == want);
        if showing || asked || self.failed_for(&server, &want) {
            return;
        }
        let Ok(Some(token)) = crate::quip::token_for(data_dir, &server) else { return };
        let (s, q, wake) = (server.clone(), want.clone(), ctx.clone());
        let (tx, rx) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new().name("quip-list".into()).spawn(move || {
            tx.send(clips::list(&s, &token, &q).map_err(String::from)).ok();
            wake.request_repaint();
        });
        match spawned {
            Ok(_) => self.loading = Some((server, want, rx)),
            Err(e) => self.error = Some(format!("Couldn't ask QuipClipper: {e}")),
        }
    }

    /// Whether the query on screen already failed (not asked again until it changes or
    /// Refresh is pressed).
    fn failed_for(&self, server: &str, q: &Query) -> bool {
        self.failed.as_ref().is_some_and(|(s, f)| s == server && f == q)
    }

    /// Whether the keyboard focus is in this window: its keys aren't tile keys then.
    pub(super) fn has_focus(ctx: &egui::Context) -> bool {
        ctx.memory(|m| m.focused()).and_then(|id| ctx.read_response(id)).is_some_and(|r| r.layer_id.id == egui::Id::unique(WINDOW_ID))
    }
}

impl StingerApp {
    /// The QuipClipper window, while `view.quip` has it open.
    pub(super) fn quip_window(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        if !self.ctl.show_quip {
            return;
        }
        let mut b = std::mem::take(&mut self.quip_browser);
        let data_dir = self.ctl.store.data_dir.clone();
        b.servers(&data_dir);
        if let Some(goto) = self.ctl.quip_goto.take() {
            if let Some(s) = goto.server.filter(|s| b.server.as_ref() != Some(s)) {
                b.server = Some(s);
                b.servers_read = None;
                b.go(String::new());
                b.servers(&data_dir);
            }
            if let Some(f) = goto.folder {
                b.go(f.trim_matches('/').to_owned());
            }
            if let Some(q) = goto.query {
                b.typing = q;
                b.typed = Some(Instant::now() - DEBOUNCE);
                b.new_view = None;
            }
            if let Some(new) = goto.new {
                b.new_view = Some(new);
                b.selected = None;
            }
        }
        b.fetch(&data_dir, ctx);
        let playing = self.ctl.engine.state(PREVIEW_ID) != PlayState::Stopped;
        let preview = self.ctl.quip_jobs.preview_state(playing);
        let mut open = true;
        let default_pos = pos2(self.board_rect.right() - 500.0, self.board_rect.top() + 16.0).max(self.window_pos());
        egui::Window::new("QuipClipper")
            .id(egui::Id::unique(WINDOW_ID))
            .open(&mut open)
            .default_pos(default_pos)
            .default_size(vec2(480.0, 540.0))
            .min_width(340.0)
            .min_height(260.0)
            .collapsible(false)
            .resizable(true)
            .show(ctx, |ui| self.quip_contents(ui, &mut b, preview.as_ref(), actions));
        if !open {
            self.ctl.show_quip = false;
            if preview.is_some() {
                actions.push(inv!("quip.preview", {"stop": true}).into());
            }
        }
        self.quip_browser = b;
    }

    fn quip_contents(&mut self, ui: &mut Ui, b: &mut QuipBrowser, preview: Option<&PreviewState>, actions: &mut Vec<Action>) {
        let t = Tokens::get(ui.ctx());
        let Some(server) = b.server.clone() else {
            ui.add_space(8.0);
            ui.label("No QuipClipper is connected yet.");
            ui.add(egui::Label::new(RichText::new("Connect one found on this network, or in Settings ▸ QuipClipper with its address; its clips library shows here.").small().color(t.text_dim)).wrap());
            ui.add_space(6.0);
            // With nothing connected, look on the network by itself (D-134).
            crate::quip::discover::start_if_stale();
            self.quip_found_ui(ui);
            ui.add_space(6.0);
            if theme::hint(ui.button("Open Settings"), "Open Settings, where QuipClipper servers are connected").clicked() {
                actions.push(inv!("view.settings", {"open": true}).into());
            }
            return;
        };

        // The server, its new clips, and Refresh.
        ui.horizontal(|ui| {
            if b.servers.len() > 1 {
                let watch = &self.ctl.quip_watch;
                let combo = egui::ComboBox::from_id_salt("quip-server").width(220.0).selected_text(&server).show_ui(ui, |ui| {
                    for s in b.servers.clone() {
                        // Each server's new clips, so they show while another is browsed.
                        let label = match watch.count_for(&s) {
                            0 => s.clone(),
                            n => format!("{s} ({n} new)"),
                        };
                        if ui.selectable_label(s == server, label).clicked() && s != server {
                            b.server = Some(s);
                            b.go(String::new());
                        }
                    }
                });
                theme::hint(theme::name(combo.response, &format!("Server, {server}")), "Which connected QuipClipper to browse");
            } else {
                ui.add(egui::Label::new(RichText::new(&server).monospace().small().color(t.text_dim)).truncate());
            }
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                let refresh = ui.add_enabled(b.loading.is_none(), egui::Button::new("Refresh"));
                if theme::hint_or(refresh, "Ask the server again", "Already asking the server").clicked() {
                    b.failed = None;
                    b.shown = None;
                    self.ctl.quip_watch.ask_now();
                }
                let n = self.ctl.quip_watch.count_for(&server);
                if n > 0 {
                    let badge = egui::Button::new(RichText::new(format!("{n} new")).color(theme::ink_on(t.accent)).strong()).fill(t.accent);
                    let badge = theme::name(ui.add(badge), &format!("{n} new in QuipClipper"));
                    if theme::hint(badge, "Clips cut since you last looked: show them (they count as seen then)").clicked() {
                        actions.push(inv!("view.quip", {"new": true, "server": server}).into());
                    }
                }
            });
        });

        if let Some(new) = b.new_view.clone() {
            ui.horizontal(|ui| {
                if theme::hint(ui.button("Back"), "Back to the folder").clicked() {
                    b.new_view = None;
                }
                ui.label(RichText::new(format!("New since you last looked: {}", new.clips.len())).font(theme::semibold(13.0)).color(t.text));
            });
            ui.separator();
            self.quip_rows(ui, b, &server, &[], &new.clips, preview, actions);
            return;
        }

        // Where we are: Clips › folder › folder.
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let parts: Vec<String> = if b.folder.is_empty() { Vec::new() } else { b.folder.split('/').map(str::to_owned).collect() };
            if parts.is_empty() {
                ui.label(RichText::new("Clips").font(theme::semibold(13.0)).color(t.text));
            } else if theme::hint(theme::small_button(ui, "Clips"), "Back to the top of the clips library").clicked() {
                b.go(String::new());
            }
            for (i, part) in parts.iter().enumerate() {
                ui.label(RichText::new("›").color(t.text_dim));
                if i + 1 == parts.len() {
                    ui.label(RichText::new(part).font(theme::semibold(13.0)).color(t.text));
                } else {
                    let up = theme::name(theme::small_button(ui, part), &format!("Up to {}", speakable(part)));
                    if theme::hint(up, "Back to this folder").clicked() {
                        b.go(parts[..=i].join("/"));
                    }
                }
            }
        });

        // Search, kinds, order.
        // Right to left, so the field takes what Clear leaves (sizing it from the row's
        // width would widen the window a little every frame).
        let row = vec2(ui.available_width(), ui.spacing().interact_size.y + 4.0);
        ui.allocate_ui_with_layout(row, egui::Layout::right_to_left(Align::Center), |ui| {
            let clear = ui.add_enabled(!b.typing.is_empty(), egui::Button::new("Clear"));
            if theme::hint_or(clear, "Clear the search", "Nothing to clear").clicked() {
                b.typing.clear();
                b.typed = Some(Instant::now() - DEBOUNCE);
            }
            let field = egui::TextEdit::singleline(&mut b.typing).hint_text("Search clips").desired_width(ui.available_width());
            let field = theme::name(theme::text_field(ui, field), "Search clips");
            if b.focus_search {
                field.request_focus();
                b.focus_search = false;
            }
            let field = theme::hint_keys(field, "Words every clip must have, in its line, file name, show or episode; searches this folder and every folder in it", "Down goes to the clips");
            if field.changed() {
                b.typed = Some(Instant::now());
                b.new_view = None;
            }
            if field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                b.typed = Some(Instant::now() - DEBOUNCE);
            }
            if field.has_focus() && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowDown)) {
                b.focus = b.shown.as_ref().and_then(|(_, _, l)| l.clips.first()).map(|c| c.clip.clone());
            }
        });
        ui.horizontal_wrapped(|ui| {
            for (k, label, tip) in [
                (None, "All", "Every clip and set file"),
                (Some(ClipKind::Audio), "Audio", "Only audio clips"),
                (Some(ClipKind::Video), "Video", "Only video clips (their sound is added)"),
                (Some(ClipKind::Set), "Sets", "Only set files, which add as sets"),
            ] {
                let chip = theme::role(ui.selectable_label(b.kind == k, label), AxRole::RadioButton, label, Some(b.kind == k));
                if theme::hint(chip, tip).clicked() && b.kind != k {
                    b.kind = k;
                    b.selected = None;
                }
            }
            ui.add_space(8.0);
            let mut newest = !b.oldest_first;
            if theme::hint(theme::checkbox(ui, &mut newest, "Newest first"), "Newest clips at the top; off: by name").changed() {
                b.oldest_first = !newest;
            }
        });
        ui.separator();

        let shown = b.shown.as_ref().filter(|(s, _, _)| *s == server).map(|(_, q, l)| (q.clone(), l.clone()));
        match &shown {
            Some((_, l)) => {
                let folders: Vec<String> = if b.query.is_empty() { l.folders.clone() } else { Vec::new() };
                let clips = l.clips.clone();
                self.quip_rows(ui, b, &server, &folders, &clips, preview, actions);
            }
            None if b.error.is_none() => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(RichText::new("Asking QuipClipper…").color(t.text_dim));
                });
            }
            None => {}
        }
        if let Some(e) = &b.error {
            ui.add(egui::Label::new(RichText::new(e).small().color(Tokens::get(ui.ctx()).error)).wrap());
        }
    }

    /// The folders, then the clips: a list a screen reader names "Clips", with the
    /// keyboard handled here (Up/Down move, Enter adds, Space previews).
    #[allow(clippy::too_many_arguments)]
    fn quip_rows(&mut self, ui: &mut Ui, b: &mut QuipBrowser, server: &str, folders: &[String], clips: &[Clip], preview: Option<&PreviewState>, actions: &mut Vec<Action>) {
        let t = Tokens::get(ui.ctx());
        let status = match (clips.len(), b.loading.is_some()) {
            (_, true) => "Asking QuipClipper…".to_owned(),
            (0, false) if !b.query.is_empty() => format!("Nothing matches “{}”", b.query),
            (0, false) if folders.is_empty() => "No clips here".to_owned(),
            (n, false) => format!("{n} clip{}", if n == 1 { "" } else { "s" }),
        };
        ui.horizontal(|ui| {
            if b.loading.is_some() {
                ui.spinner();
            }
            ui.label(RichText::new(&status).small().color(t.text_dim));
        });
        let row_id = |clip: &str| egui::Id::unique(("quip-row", clip));
        let current_set = self.ctl.current_set().name.clone();
        egui::ScrollArea::vertical().auto_shrink(false).id_salt("quip-rows").show(ui, |ui| {
            ui.ctx().accesskit_node_builder(ui.unique_id(), |n| {
                n.set_role(AxRole::ListBox);
                n.set_label("Clips");
            });
            ui.spacing_mut().item_spacing.y = 2.0;
            for f in folders {
                let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click());
                let resp = theme::role(resp, AxRole::Button, format!("{}, folder", speakable(f)), None);
                if ui.is_rect_visible(rect) {
                    if resp.hovered() || resp.has_focus() {
                        ui.painter().rect_filled(rect, 4.0, t.panel2);
                    }
                    paint_folder(ui.painter(), pos2(rect.left() + 14.0, rect.center().y), t.text_dim);
                    let mut job = egui::text::LayoutJob::simple_singleline(f.clone(), theme::regular(14.0), t.text);
                    job.wrap = egui::text::TextWrapping::truncate_at_width(rect.width() - 40.0);
                    let g = ui.painter().layout_job(job);
                    ui.painter().galley(pos2(rect.left() + 32.0, rect.center().y - g.size().y / 2.0), g, t.text);
                }
                if theme::hint(resp, "Open this folder").clicked() {
                    let to = if b.folder.is_empty() { f.clone() } else { format!("{}/{f}", b.folder) };
                    b.go(to);
                }
            }
            for (i, c) in clips.iter().enumerate() {
                let id = row_id(&c.clip);
                let kind = c.kind();
                let title = c.display_title();
                let has_sound = kind.is_some_and(ClipKind::has_sound);
                let can_add = has_sound || kind == Some(ClipKind::Set);
                // The keys, before egui turns Enter or Space into a click.
                if ui.memory(|m| m.has_focus(id)) {
                    let filter = egui::EventFilter { vertical_arrows: true, ..Default::default() };
                    ui.memory_mut(|m| m.set_focus_lock_filter(id, filter));
                    let (up, down, enter, space) = ui.input_mut(|inp| {
                        (
                            inp.consume_key(Modifiers::NONE, Key::ArrowUp),
                            inp.consume_key(Modifiers::NONE, Key::ArrowDown),
                            inp.consume_key(Modifiers::NONE, Key::Enter),
                            inp.consume_key(Modifiers::NONE, Key::Space),
                        )
                    });
                    if up {
                        match i.checked_sub(1) {
                            Some(j) => b.focus = Some(clips[j].clip.clone()),
                            None => b.focus_search = true,
                        }
                    }
                    if down {
                        if let Some(next) = clips.get(i + 1) {
                            b.focus = Some(next.clip.clone());
                        }
                    }
                    if enter && can_add {
                        actions.push(inv!("quip.add", {"server": server, "clip": c.clip}).into());
                    }
                    if space && has_sound {
                        actions.push(inv!("quip.preview", {"server": server, "clip": c.clip, "title": title}).into());
                    }
                }
                let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::hover());
                let resp = ui.interact(rect, id, Sense::click_and_drag());
                if b.focus.as_deref() == Some(c.clip.as_str()) {
                    resp.request_focus();
                    resp.scroll_to_me(None);
                    b.focus = None;
                    b.selected = Some(c.clip.clone());
                }
                if resp.has_focus() {
                    b.selected = Some(c.clip.clone());
                }
                let state = match preview {
                    Some(PreviewState::Loading(p)) if *p == c.clip => Some("loading the preview"),
                    Some(PreviewState::Playing(p)) if *p == c.clip => Some("previewing"),
                    _ => None,
                };
                let mut label = format!("{}, ", speakable(&title));
                if let Some(s) = c.source_text() {
                    label.push_str(&format!("{s}, "));
                }
                label.push_str(kind_word(kind));
                if let Some(d) = c.duration() {
                    label.push_str(&format!(", {}", spoken(d)));
                }
                if let Some(s) = c.surround() {
                    label.push_str(&format!(", {s}"));
                }
                if let Some(s) = state {
                    label.push_str(&format!(", {s}"));
                }
                let selected = b.selected.as_deref() == Some(c.clip.as_str());
                let resp = theme::role(resp, AxRole::ListBoxOption, label, Some(selected));
                let tip = match kind {
                    Some(ClipKind::Set) => format!("{title}\nA set file: Add imports it as a set, after yours"),
                    Some(ClipKind::Gif) => format!("{title}\nA GIF: it has no sound to add"),
                    _ => format!("{title}\n{}Add puts it at the end of “{current_set}”, trimmed to the line", c.source_text().map(|s| format!("{s}\n")).unwrap_or_default()),
                };
                let resp = theme::hint_keys(resp, &tip, "Enter adds · Space previews · drag it onto the board · Up/Down move");
                if resp.clicked() {
                    b.selected = Some(c.clip.clone());
                    resp.request_focus();
                }
                if resp.double_clicked() && can_add {
                    actions.push(inv!("quip.add", {"server": server, "clip": c.clip}).into());
                }
                if can_add {
                    resp.dnd_set_drag_payload(QuipDrag { server: server.to_owned(), clip: c.clip.clone(), title: title.clone() });
                }

                if ui.is_rect_visible(rect) {
                    let p = ui.painter();
                    if selected || resp.hovered() {
                        p.rect_filled(rect, 4.0, if selected { theme::with_alpha(t.accent, 0.22) } else { t.panel2 });
                    }
                    paint_kind(p, pos2(rect.left() + 14.0, rect.center().y), kind, if state.is_some() { t.playing } else { t.text_dim });
                }
                // Preview and Add, on the row's right.
                let buttons = Rect::from_min_max(pos2(rect.right() - 150.0, rect.top()), rect.right_bottom());
                let mut text_right = buttons.left() - 6.0;
                ui.scope_builder(egui::UiBuilder::new().max_rect(buttons).layout(egui::Layout::right_to_left(Align::Center)), |ui| {
                    let add = theme::name(ui.add_enabled(can_add, egui::Button::new("Add")), &format!("Add {}", speakable(&title)));
                    let add_tip = if kind == Some(ClipKind::Set) { "Import this set file as a set".to_owned() } else { format!("Add it to “{current_set}”, trimmed to the line — Enter") };
                    if theme::hint_or(add, add_tip, "A GIF has no sound to add").clicked() {
                        actions.push(inv!("quip.add", {"server": server, "clip": c.clip}).into());
                    }
                    let (text, name, tip) = match state {
                        Some("previewing") => ("⏹ Stop", "Stop the preview", "Stop the preview — Space"),
                        Some(_) => ("Loading…", "Stop loading the preview", "Stop loading the preview"),
                        None => ("▶ Preview", "Preview", "Play it on the Preview output (on Main when there's none), without adding it — Space"),
                    };
                    let play = theme::name(ui.add_enabled(has_sound, egui::Button::new(text)), &format!("{name}, {}", speakable(&title)));
                    if theme::hint_or(play, tip, "A set file or a GIF has no sound to preview").clicked() {
                        actions.push(inv!("quip.preview", {"server": server, "clip": c.clip, "title": title}).into());
                    }
                    text_right = ui.min_rect().left() - 8.0;
                });
                if ui.is_rect_visible(rect) {
                    let p = ui.painter();
                    let mut right = text_right;
                    // Length and channels, right-aligned before the buttons.
                    if let Some(s) = c.surround() {
                        let g = p.layout_no_wrap(s, theme::semibold(10.0), t.text);
                        let r = Rect::from_min_size(pos2(right - g.size().x - 8.0, rect.center().y - 8.0), vec2(g.size().x + 8.0, 16.0));
                        p.rect_stroke(r, 3.0, Stroke::new(1.0_f32, t.text_dim), StrokeKind::Inside);
                        p.galley(r.center() - g.size() / 2.0, g, t.text);
                        right = r.left() - 6.0;
                    }
                    if let Some(d) = c.duration() {
                        let r = theme::paint_tabular(p, pos2(right, rect.center().y), Align2::RIGHT_CENTER, &format_clock(d, d < 10.0), theme::regular(12.0), t.text_dim);
                        right = r.left() - 8.0;
                    }
                    let left = rect.left() + 32.0;
                    let width = (right - left).max(20.0);
                    let sub = c.source_text().unwrap_or_else(|| c.name.clone());
                    for (text, font, color, y) in [(title.clone(), theme::semibold(14.0), t.text, 13.0), (sub, theme::regular(12.0), t.text_dim, 32.0)] {
                        let mut job = egui::text::LayoutJob::simple_singleline(text, font, color);
                        job.wrap = egui::text::TextWrapping::truncate_at_width(width);
                        let g = p.layout_job(job);
                        p.galley(pos2(left, rect.top() + y - g.size().y / 2.0), g, color);
                    }
                }
            }
        });
    }

    /// A clip dropped on the board: added to the current set.
    pub(super) fn quip_drop(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let Some(drag) = egui::DragAndDrop::payload::<QuipDrag>(ctx) else { return };
        let Some(pos) = ctx.pointer_hover_pos().or_else(|| ctx.input(|i| i.pointer.latest_pos())) else { return };
        let on_board = self.board_rect.contains(pos) && ctx.layer_id_at(pos).is_none_or(|l| l.order == egui::Order::Background);
        let t = Tokens::get(ctx);
        if ctx.input(|i| i.pointer.any_released()) {
            egui::DragAndDrop::clear_payload(ctx);
            if on_board {
                actions.push(inv!("quip.add", {"server": drag.server, "clip": drag.clip}).into());
            }
            return;
        }
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("quip-drag")));
        if on_board {
            painter.rect_stroke(self.board_rect.shrink(4.0), 8.0, Stroke::new(2.0_f32, t.accent), StrokeKind::Inside);
        }
        let r = Rect::from_min_size(pos + vec2(10.0, 10.0), vec2(220.0, 30.0));
        painter.rect_filled(r, 6.0, theme::with_alpha(t.panel, 0.95));
        painter.rect_stroke(r, 6.0, Stroke::new(1.0_f32, t.accent), StrokeKind::Inside);
        let text = if on_board { format!("Add “{}”", drag.title) } else { drag.title.clone() };
        painter.with_clip_rect(r.shrink(4.0)).text(r.left_center() + vec2(8.0, 0.0), Align2::LEFT_CENTER, text, theme::regular(13.0), t.text);
    }
}

/// The status bar's "N new in QuipClipper" (D-103), in either theme.
pub(super) struct QuipBadge {
    pub n: usize,
    /// Its hover text: where the clips are, what a click does, servers out of reach.
    pub tip: String,
}

impl QuipBadge {
    /// What a screen reader calls it. It's never announced: it changes quietly, as a
    /// show runs, and a click is the only way to it besides Tab.
    pub fn name(&self) -> String {
        format!("{} new in QuipClipper", self.n)
    }
}

impl StingerApp {
    /// The badge, when clips are new and it's on (`settings.quipBadge`), and the window
    /// is closed (its own "N new" takes over while it's open).
    pub(super) fn quip_badge(&self) -> Option<QuipBadge> {
        let w = &self.ctl.quip_watch;
        let n = w.count();
        if n == 0 || self.ctl.show_quip || !self.ctl.store.library.quip_badge {
            return None;
        }
        let clips = |n: usize| if n == 1 { "1 clip".to_owned() } else { format!("{n} clips") };
        let with: Vec<_> = w.servers().iter().filter(|s| s.count() > 0).collect();
        let mut tip = match with.as_slice() {
            [one] => format!("{} cut on {} since you last looked. Click to list them in the QuipClipper window; they count as seen then", clips(n), host(&one.server)),
            many => format!(
                "New since you last looked: {}. Click to list {}'s in the QuipClipper window; they count as seen then",
                many.iter().map(|s| format!("{} on {}", clips(s.count()), host(&s.server))).collect::<Vec<_>>().join(", "),
                host(&many[0].server),
            ),
        };
        for s in w.servers().iter().filter(|s| s.error.is_some()) {
            tip.push_str(&format!("\nCan't reach {} at the moment: its count is from when it last answered", host(&s.server)));
        }
        Some(QuipBadge { n, tip })
    }
}
