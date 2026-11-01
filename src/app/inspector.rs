//! The inspector (D-144, M22): a short column for the selected tile, several tiles or an
//! action tile, or, with nothing selected, the current set's settings. Top to bottom, for
//! a tile: its name and key, its picture and colour; the hero (the sound's waveform with
//! the trim handles, playhead and meter, the transport, then position, Edit… and length);
//! the pad options; the volume; the rest in tabs (Play, Sound, Cue, Control); and a
//! footer pinned to the bottom with the file and Replace Audio…. In a locked set the
//! inspector only plays and switches tabs: everything that would change the set is off.

use super::*;
use crate::controller::InspectorTab;

/// The tile inspector's footer: a separator, the file's name and a row of buttons.
const FOOTER_H: f32 = 74.0;

/// The widest readouts the tabs' sliders show, so each tab's rails line up (D-211).
const PLAY_VALUES: [&str; 2] = ["10.0 s", STOP_FADE_ZERO];
const SOUND_VALUES: [&str; 3] = ["+24.0 dB", "+12.0 st", "2.00×"];

/// How wide a button or value box showing `text` is: the text and the button padding.
pub(super) fn box_width(ui: &Ui, text: &str) -> f32 {
    let font = egui::TextStyle::Button.resolve(ui.style());
    let w = ui.painter().layout_no_wrap(text.to_owned(), font, Color32::WHITE).size().x;
    (w + 2.0 * ui.spacing().button_padding.x).max(ui.spacing().interact_size.x)
}

/// Fits the next sliders' rails to the room left in their grid cell, keeping beside each
/// its value box (as wide as the widest of `values`) and `extra` points after it, so the
/// rails line up and never widen the inspector (D-211).
pub(super) fn fit_sliders(ui: &mut Ui, values: &[&str], extra: f32) {
    let value = values.iter().map(|v| box_width(ui, v)).fold(0.0, f32::max);
    let room = ui.available_width() - value - ui.spacing().item_spacing.x - extra;
    ui.spacing_mut().slider_width = room.max(40.0);
}

impl StingerApp {
    pub(super) fn inspector(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let Some(id) = self.ctl.selected_tile else {
            self.set_inspector(ui, actions);
            return;
        };
        let selected = self.ctl.selected();
        if selected.len() > 1 {
            self.multi_inspector(ui, &selected, actions);
            return;
        }
        let Some(orig) = self.ctl.store.library.tile(id).cloned() else {
            actions.push(inv!("view.select").into());
            return;
        };
        if orig.action.is_some() {
            self.action_inspector(ui, &orig, actions);
        } else {
            self.tile_inspector(ui, &orig, actions);
        }
    }

    /// The inspector's first line: what it shows, and ✕ to hide it; then, when the set
    /// is locked, a line saying so with Unlock. Returns whether it's locked.
    fn inspector_head(&self, ui: &mut Ui, title: &str, set: Option<usize>, actions: &mut Vec<Action>) -> bool {
        let t = Tokens::get(ui.ctx());
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new(title).small().strong().weak());
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                let hide = theme::name(theme::small_button(ui, "🗙"), "Hide inspector");
                if theme::hint_keys(hide, "Hide the inspector, giving the board its room", "⌘/Ctrl+E").clicked() {
                    actions.push(inv!("view.inspector", {"open": false}).into());
                }
            });
        });
        let Some(set) = set.map(|si| &self.ctl.store.library.sets[si]).filter(|s| s.locked) else { return false };
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(16.0, 18.0), Sense::hover());
            pad_options::paint_lock(ui.painter(), rect.center(), 14.0, true, t.text_dim);
            ui.label(RichText::new(format!("“{}” is locked", set.name)).color(t.text_dim));
            if theme::hint(theme::small_button(ui, "Unlock"), tips::UNLOCK).clicked() {
                actions.push(inv!("set.unlock", {"set": set.id}).into());
            }
        });
        true
    }

    fn tile_inspector(&mut self, ui: &mut Ui, orig: &Tile, actions: &mut Vec<Action>) {
        let id = orig.id;
        let mut t = orig.clone();
        let set = self.ctl.store.library.locate(id).map(|(si, _)| si);
        let locked = self.inspector_head(ui, "TILE", set, actions);
        let more_rows = self.footer_rows(ui, &t).len() as f32 - 1.0;
        let footer = FOOTER_H + more_rows * (ui.spacing().interact_size.y + ui.spacing().item_spacing.y) + if self.import_line(&t).is_some() { 26.0 } else { 0.0 };
        egui::ScrollArea::vertical().id_salt("inspector").auto_shrink([false, false]).max_height((ui.available_height() - footer).max(120.0)).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            ui.add_enabled_ui(!locked, |ui| self.identity_row(ui, &mut t, true, actions));
            self.hero(ui, &mut t, locked, actions);
            ui.add_enabled_ui(!locked, |ui| {
                // The pad options as toggle buttons (D-144, D-145), Disabled under them.
                if let Some(opt) = pad_options::option_row(ui, |o| Some(o.get(&t))) {
                    let on = !opt.get(&t);
                    opt.set(&mut t, on);
                }
                volume_slider(ui, &mut t.volume, false);
            });
            let tab = self.tab_bar(ui, &InspectorTab::ALL, actions);
            // The Cue tab greys only its edits: a locked set still runs, so Play On and Jump play.
            ui.add_enabled_ui(!locked || tab == InspectorTab::Cue, |ui| match tab {
                InspectorTab::Play => play_tab(ui, &mut t),
                InspectorTab::Sound => self.sound_tab(ui, &mut t, actions),
                InspectorTab::Cue => self.cue_tab(ui, &mut t, locked, actions),
                InspectorTab::Control => self.control_tab(ui, &t, orig, actions),
            });
        });
        self.tile_footer(ui, &t, locked, actions);
        if t != *orig && !locked {
            actions.push(Action::Run(tile_changes(orig, &t)));
        }
    }

    /// The name with the tile's key beside it; under them the picture and the colour.
    fn identity_row(&self, ui: &mut Ui, t: &mut Tile, picture: bool, actions: &mut Vec<Action>) {
        let tokens = Tokens::get(ui.ctx());
        ui.horizontal(|ui| {
            let cap_font = theme::bold(12.0);
            let cap_w = t.hotkey.map_or(0.0, |k| (ui.painter().layout_no_wrap(k.label(), cap_font.clone(), tokens.key_text()).size().x + 8.0).max(26.0));
            let cap = if t.hotkey.is_some() { cap_w + ui.spacing().item_spacing.x } else { 0.0 };
            let field = egui::TextEdit::singleline(&mut t.title).font(FontId::proportional(16.0)).desired_width((ui.available_width() - cap).max(80.0));
            theme::hint(theme::name(theme::text_field(ui, field), "Title"), "The tile's title, shown on it and read out by screen readers");
            if let Some(k) = t.hotkey {
                // The key, as the pad shows it (the Control tab changes it).
                let (rect, _) = ui.allocate_exact_size(vec2(cap_w, 24.0), Sense::hover());
                super::paint_key_cap(ui.painter(), rect.min, rect.size(), k, cap_font, tokens.key, Some(tokens.key_border), tokens.key_text());
            }
        });
        ui.horizontal(|ui| {
            if picture {
                self.picture_button(ui, t, actions);
            }
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                if let Some(c) = colour_menu(ui, Some(t.color)) {
                    t.color = c;
                }
            });
        });
    }

    /// Picture…: the tile's picture in a pop-up (choose, remove, under or over, fit, opacity).
    fn picture_button(&self, ui: &mut Ui, t: &mut Tile, actions: &mut Vec<Action>) {
        let label = if t.image.is_some() { "Picture…" } else { "Add Picture…" };
        let resp = theme::hint(theme::small_button(ui, label), "The picture shown on the tile: choose one, or how it sits (also drop an image file onto the tile)");
        egui::Popup::from_toggle_button_response(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
            ui.set_width(300.0);
            ui.label(RichText::new("PICTURE").small().strong().weak());
            self.picture_editor(ui, t, actions);
        });
    }

    /// The hero (D-144): the waveform with its trim handles, playhead and meter; to start,
    /// Preview (or Stop) and fade out; then the position, Edit… and the length.
    fn hero(&self, ui: &mut Ui, t: &mut Tile, locked: bool, actions: &mut Vec<Action>) {
        let id = t.id;
        let open = self.ctl.editor == Some(id);
        if open {
            ui.weak("The trim, vamp and markers are in the editor.");
        } else {
            self.hero_waveform(ui, t, !locked);
        }
        let status = self.ctl.statuses.get(&id).copied();
        let recording_here = self.ctl.recording.as_ref().is_some_and(|r| r.tile == id);
        let tokens = Tokens::get(ui.ctx());
        // To start at the left, Fade Out at the right, and the big button filling the
        // room between them, so a narrow column (D-211) never wraps its label. While this
        // tile records, Stop Recording takes the whole row: it wouldn't fit between them,
        // and it's the one thing to press then (the pad itself still stops a playback).
        ui.horizontal(|ui| {
            if recording_here {
                let stop = egui::Button::new(RichText::new("⏹  Stop Recording").color(theme::ink_on(tokens.error))).fill(tokens.error).min_size(vec2(ui.available_width(), 30.0));
                if theme::hint(theme::name(ui.add(stop), "Stop Recording"), "Stop recording; the recording becomes this tile's sound").clicked() {
                    actions.push(inv!("record.stop").into());
                }
                return;
            }
            let (label, name) = if status.is_some() { ("⏹  Stop", "Stop") } else { ("▶  Preview", "Preview") };
            // Near the narrowest width the buttons' padding gives way first.
            if 34.0 + 2.0 * ui.spacing().item_spacing.x + box_width(ui, "▶  Preview") + box_width(ui, "Fade Out") > ui.available_width() {
                ui.spacing_mut().button_padding.x = 4.0;
            }
            let back = theme::name(ui.add_enabled(status.is_some(), egui::Button::new("⏮").min_size(vec2(34.0, 30.0))), "To start");
            if theme::hint_or(back, "Play it again from the start", "Only while it plays").clicked() {
                actions.push(inv!("tile.play", {"tile": id, "restart": true}).into());
            }
            let fade_w = box_width(ui, "Fade Out");
            let wide = ui.available_width() - fade_w - ui.spacing().item_spacing.x;
            let button = egui::Button::new(RichText::new(label).strong().color(theme::ink_on(tokens.accent))).fill(tokens.accent).min_size(vec2(wide, 30.0));
            let tip = if status.is_some() { "Stop this tile now" } else { "Play this tile, on the Preview output if one is set in Settings" };
            if theme::hint_or(theme::name(ui.add_enabled(t.file.is_some(), button), name), tip, "This tile has no sound yet").clicked() {
                actions.push(inv!("tile.preview", {"tile": id}).into());
            }
            let fade = fade_out_secs(t);
            let resp = theme::name(ui.add_enabled(status.is_some(), egui::Button::new("Fade Out").min_size(vec2(fade_w, 30.0))), "Fade out");
            if theme::hint_or(resp, format!("Fade it out over {fade:.1} s (its stop fade, or 2 s without one)"), "Only while it plays").clicked() {
                actions.push(inv!("tile.stop", {"tile": id, "fade": fade}).into());
            }
        });
        let dur = self.ctl.store.audio(t).map(|a| a.duration());
        // The position, Edit… in the middle, and the length.
        ui.horizontal(|ui| {
            let edit = if open { "Close Editor" } else { "Edit…" };
            let side = ((ui.available_width() - box_width(ui, edit)) / 2.0 - ui.spacing().item_spacing.x).max(0.0);
            let h = ui.spacing().interact_size.y;
            let at = status.map_or(0.0, |s| s.position);
            ui.allocate_ui_with_layout(vec2(side, h), egui::Layout::left_to_right(Align::Center), |ui| {
                ui.set_min_width(side);
                ui.label(RichText::new(format_clock(at, true)).color(tokens.text_dim));
            });
            let button = ui.add_enabled(t.file.is_some(), egui::Button::new(edit));
            let tip = if open { editor::CLOSE_TIP } else { editor::OPEN_TIP };
            if theme::hint_or(theme::name(button, if open { "Close Editor" } else { "Edit waveform" }), tip, "This tile has no sound yet").clicked() {
                actions.push(inv!("view.editor", {"tile": id, "open": !open}).into());
            }
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                let length = dur.map(|d| {
                    let (s, e) = play_range(t, d);
                    (e - s) / t.speed.clamp(0.5, 2.0) as f64
                });
                ui.label(RichText::new(length.map_or_else(|| "—".to_owned(), |l| format_clock(l, true))).color(tokens.text_dim));
            });
        });
    }

    /// The hero's waveform, in the tile's colour, with the trim shaded off, the vamp and
    /// markers, the playhead and, at its right, the tile's meter. With `editable`, its
    /// handles trim it: drag near one, or focus one (Tab) and press Left/Right.
    fn hero_waveform(&self, ui: &mut Ui, t: &mut Tile, editable: bool) {
        // Not a focus stop: its handles (below) are, one by one.
        let (full, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 78.0), if editable { Sense::CLICK | Sense::DRAG } else { Sense::hover() });
        let rect = Rect::from_min_max(full.min, pos2(full.right() - 14.0, full.bottom()));
        let resp = theme::role(resp, egui::accesskit::Role::Image, "Waveform", None);
        let resp = theme::hint(resp, if editable {
            "The tile's sound: drag near the start or end handle to trim where it plays (the Sound tab takes exact values)"
        } else {
            "The tile's sound and where it's playing"
        });
        let tokens = Tokens::get(ui.ctx());
        let p = ui.painter_at(full);
        p.rect_filled(rect, 6.0, Color32::from_black_alpha(90));
        // The tile's meter: a bar beside the waveform, while it sounds.
        let meter = Rect::from_min_max(pos2(full.right() - 8.0, full.top() + 2.0), pos2(full.right(), full.bottom() - 2.0));
        p.rect_filled(meter, 2.0, tokens.meter_off);
        if let Some(b) = self.tile_meter(t.id) {
            let frac = meter_frac(b.level).clamp(0.0, 1.0);
            let c = if b.level >= -3.0 { tokens.meter_red } else if b.level >= -12.0 { tokens.meter_yellow } else { tokens.meter_green };
            p.rect_filled(Rect::from_min_max(pos2(meter.left(), meter.bottom() - meter.height() * frac), meter.max), 2.0, c);
        }
        let (Some(peaks), Some(audio)) = (self.ctl.store.peaks(t), self.ctl.store.audio(t)) else {
            let msg = if t.file.is_none() { "No audio" } else { "Loading waveform…" };
            p.text(rect.center(), Align2::CENTER_CENTER, msg, FontId::proportional(13.0), ui.visuals().weak_text_color());
            return;
        };
        let dur = audio.duration().max(1e-6);
        let base = if theme::is_panel(ui.ctx()) { theme::panel_tile(ui.ctx(), t.color) } else { theme::console_tile(ui.ctx(), t.color) };
        let color = theme::lighten(base, 0.15);
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
        for m in &t.markers {
            let x = x_of(m.at);
            p.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], Stroke::new(1.0_f32, tokens.marker));
            p.add(egui::Shape::convex_polygon(
                vec![pos2(x, rect.bottom() - 10.0), pos2(x + 7.0, rect.bottom() - 6.0), pos2(x, rect.bottom() - 2.0)],
                tokens.marker,
                Stroke::NONE,
            ));
        }
        if let Some((a, b)) = crate::logic::vamp_range(t, dur) {
            let band = Rect::from_x_y_ranges(x_of(a)..=x_of(b), rect.y_range());
            p.rect_filled(band, 0.0, theme::with_alpha(tokens.vamp, 0.18));
            for x in [x_of(a), x_of(b)] {
                p.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], Stroke::new(1.5_f32, tokens.vamp));
            }
        }
        for x in [x_of(s), x_of(e)] {
            p.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], Stroke::new(2.0_f32, tokens.trim));
            p.rect_filled(Rect::from_center_size(pos2(x, rect.top() + 6.0), vec2(8.0, 12.0)), 2.0, tokens.trim);
        }
        if let Some(st) = self.ctl.statuses.get(&t.id) {
            let x = x_of(st.from + st.position * st.speed);
            p.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], Stroke::new(2.0_f32, tokens.playhead));
            p.circle_filled(pos2(x, rect.top() + 3.0), 3.5, tokens.playhead);
        }
        if !editable {
            return;
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
        // Each handle is a control of its own as well: drag it, or focus it (Tab) and press
        // Left/Right (Shift: bigger steps); screen readers see a slider with its time.
        let (s, e) = play_range(t, dur);
        let mut handles = vec![(Handle::Start, s), (Handle::End, e)];
        if let Some((a, b)) = crate::logic::vamp_range(t, dur) {
            handles.extend([(Handle::VampStart, a), (Handle::VampEnd, b)]);
        }
        for (h, at) in handles {
            let strip = Rect::from_center_size(pos2(x_of(at), rect.center().y), vec2(10.0, rect.height()));
            let hr = ui.interact(strip, resp.id.with(h.name()), Sense::click_and_drag());
            let (hr, steps) = theme::slider_info(hr, h.name(), at, format!("{} seconds", format_clock(at, true)));
            let hr = theme::hint(hr, h.tip());
            let mut to = None;
            if let (true, Some(pos)) = (hr.dragged(), hr.interact_pointer_pos()) {
                to = Some((((pos.x - rect.left()) / rect.width()) as f64 * dur).clamp(0.0, dur));
            }
            if hr.has_focus() {
                theme::lock_arrows(&hr);
                let step = if ui.input(|i| i.modifiers.shift) { 0.1 } else { 0.01 };
                let (left, right) = ui.input_mut(|i| {
                    let m = i.modifiers;
                    (i.consume_key(m, Key::ArrowLeft), i.consume_key(m, Key::ArrowRight))
                });
                if left || right {
                    to = Some(at + if right { step } else { -step });
                }
            }
            if steps != 0 {
                to = Some(at + 0.1 * steps as f64);
            }
            if let Some(sec) = to {
                h.set(t, sec.clamp(0.0, dur), dur);
            }
            if hr.hovered() || hr.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
        }
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
    }

    /// The tabs (D-144), a segmented control: each segment a tab a screen reader names
    /// and Tab reaches; a click (or Enter or Space) shows it. Returns the tab showing,
    /// the first of `tabs` when the one chosen isn't among them.
    fn tab_bar(&self, ui: &mut Ui, tabs: &[InspectorTab], actions: &mut Vec<Action>) -> InspectorTab {
        let current = if tabs.contains(&self.ctl.inspector_tab) { self.ctl.inspector_tab } else { tabs[0] };
        let tokens = Tokens::get(ui.ctx());
        let panel = theme::is_panel(ui.ctx());
        ui.add_space(2.0);
        let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), if panel { 30.0 } else { 28.0 }), Sense::hover());
        let w = bar.width() / tabs.len() as f32;
        let corner = if panel { bar.height() / 2.0 } else { 5.0 };
        ui.painter().rect_filled(bar, corner, tokens.key);
        for (i, &tab) in tabs.iter().enumerate() {
            let rect = Rect::from_min_size(pos2(bar.left() + i as f32 * w, bar.top()), vec2(w, bar.height()));
            let resp = ui.interact(rect, egui::Id::unique(("inspector-tab", tab.name())), Sense::click());
            let on = tab == current;
            let p = ui.painter();
            if on {
                p.rect_filled(rect.shrink(2.0), corner - 2.0, tokens.accent);
            } else if resp.hovered() {
                p.rect_filled(rect.shrink(2.0), corner - 2.0, tokens.key_hover);
            }
            let ink = theme::ink_on(if on { tokens.accent } else { tokens.key });
            let (text, font) = if panel { (tab.label().to_uppercase(), theme::condensed(17.0)) } else { (tab.label().to_owned(), theme::semibold(12.5)) };
            // A label wider than its segment (a narrow column, D-211) shrinks to fit.
            let mut g = p.layout_no_wrap(text.clone(), font.clone(), ink);
            let room = w - 10.0;
            if g.size().x > room {
                g = p.layout_no_wrap(text, FontId::new((font.size * room / g.size().x).max(font.size * 0.75), font.family), ink);
            }
            p.galley(rect.center() - g.size() / 2.0, g, ink);
            if on {
                // A bar under the label, so the tab showing isn't told by colour alone.
                p.line_segment([pos2(rect.center().x - 12.0, rect.bottom() - 5.0), pos2(rect.center().x + 12.0, rect.bottom() - 5.0)], Stroke::new(2.0_f32, ink));
            }
            // A tab is selected, not toggled: VoiceOver says "selected, tab 2 of 4".
            let resp = theme::role(resp, AxRole::Tab, tab.label(), None);
            resp.ctx.accesskit_node_builder(resp.id, |n| n.set_selected(on));
            if theme::hint(resp, tab_tip(tab)).clicked() && !on {
                actions.push(inv!("view.inspector", {"tab": tab.name()}).into());
            }
        }
        ui.add_space(2.0);
        current
    }

    /// The Sound tab: gain and Normalise, speed, pitch, the outputs, the exact trim, and a
    /// file with more than two channels' mix.
    fn sound_tab(&self, ui: &mut Ui, t: &mut Tile, actions: &mut Vec<Action>) {
        let id = t.id;
        egui::Grid::new("sound-tab").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
            // Room after each rail for its reset button (1×, 0); anything more wraps
            // under it rather than widening the column (D-211).
            let reset = box_width(ui, "1×") + ui.spacing().item_spacing.x;
            ui.label("Gain");
            fit_sliders(ui, &SOUND_VALUES, reset);
            ui.horizontal(|ui| {
                let gain = ui.add(
                    egui::Slider::new(&mut t.gain_db, -GAIN_DB_MAX..=GAIN_DB_MAX)
                        .step_by(0.1)
                        .custom_formatter(|v, _| format!("{v:+.1} dB"))
                        .custom_parser(|s| s.trim().trim_end_matches("dB").trim().parse().ok()),
                );
                theme::hint(theme::name(gain, "Gain"), "Raise or lower the clip on top of its volume, ±dB");
            });
            ui.end_row();
            ui.label("");
            if theme::hint(theme::small_button(ui, "Normalise"), "Set the gain so this clip plays at −16 LUFS, like the rest of a normalised set").clicked() {
                actions.push(inv!("tile.normalize", {"tile": id}).into());
            }
            ui.end_row();

            ui.label("Speed");
            ui.horizontal_wrapped(|ui| {
                let speed = ui.add(
                    egui::Slider::new(&mut t.speed, SPEED_MIN..=SPEED_MAX)
                        .step_by(0.05)
                        .custom_formatter(|v, _| format!("{v:.2}×"))
                        .custom_parser(|s| s.trim().trim_end_matches(['×', 'x']).trim().parse().ok()),
                );
                theme::hint(theme::name(speed, "Speed"), "Faster or slower; the pitch follows, like tape");
                if t.speed != 1.0 && theme::hint(theme::name(theme::small_button(ui, "1×"), "Normal speed"), "Back to normal speed").clicked() {
                    t.speed = 1.0;
                }
            });
            ui.end_row();

            ui.label("Pitch");
            ui.horizontal_wrapped(|ui| {
                let pitch = ui.add(
                    egui::Slider::new(&mut t.pitch, -crate::pitch::PITCH_MAX..=crate::pitch::PITCH_MAX)
                        .step_by(0.5)
                        .custom_formatter(|v, _| format!("{v:+.1} st"))
                        .custom_parser(|s| s.trim().trim_end_matches("st").trim().parse().ok()),
                );
                theme::hint(theme::name(pitch, "Pitch"), "Higher or lower in semitones, without changing the speed (rendered once, then instant)");
                if t.pitch != 0.0 && theme::hint(theme::name(theme::small_button(ui, "0"), "Original pitch"), "Back to the original pitch").clicked() {
                    t.pitch = 0.0;
                }
                if t.pitch != 0.0 && matches!(self.ctl.store.playable(t), Err(true)) {
                    ui.weak("rendering…");
                }
            });
            ui.end_row();

            if !self.ctl.store.library.outputs.is_empty() {
                let names: Vec<String> = self.ctl.store.library.outputs.iter().map(|o| o.name.clone()).collect();
                routing::sends(ui, t, &names);
            }

            if let Some(dur) = self.ctl.store.audio(t).map(|a| a.duration()) {
                let (s, e) = play_range(t, dur);
                let (mut start, mut end) = (s, e);
                ui.label("Trim");
                ui.horizontal_wrapped(|ui| {
                    theme::hint(theme::name(ui.add(egui::DragValue::new(&mut start).range(0.0..=dur).speed(0.01).max_decimals(2).suffix(" s")), "Start"), "Where it starts playing, in seconds into the file");
                    ui.label("to");
                    theme::hint(theme::name(ui.add(egui::DragValue::new(&mut end).range(0.0..=dur).speed(0.01).max_decimals(2).suffix(" s")), "End"), "Where it stops playing, in seconds into the file");
                });
                ui.end_row();
                ui.label("");
                if theme::hint(theme::small_button(ui, "Reset"), "Play the whole file again").clicked() {
                    start = 0.0;
                    end = dur;
                }
                ui.end_row();
                if start != s || end != e {
                    t.start = start.min(end - 0.01).max(0.0);
                    t.end = if (dur - end).abs() < 1e-3 { None } else { Some(end.max(t.start + 0.01)) };
                }
                ui.label("");
                ui.weak(format!("Plays {} of {}", format_clock(e - s, true), format_clock(dur, true)));
                ui.end_row();
            }
        });
        if let Some((labels, downmix)) = self.ctl.store.audio(t).and_then(|a| a.native.as_ref().map(|n| (n.labels.clone(), n.downmix.clone()))) {
            ui.separator();
            ui.label(RichText::new("CHANNELS").small().strong().weak());
            routing::channels(ui, t, &labels, &downmix);
        }
    }

    /// The Cue tab: the notes, the pre-wait and what starts the next cue, the vamp and
    /// the markers. `locked` greys the edits but not Play On and Jump, which play.
    fn cue_tab(&self, ui: &mut Ui, t: &mut Tile, locked: bool, actions: &mut Vec<Action>) {
        ui.add_enabled_ui(!locked, |ui| {
            theme::hint(theme::name(notes_editor(ui, &mut t.notes, "Notes, shown in the cue list"), "Notes"), "Notes for this cue, shown under its title in the cue list and in search");
            cue_link_editor(ui, t);
        });
        if self.ctl.editor == Some(t.id) {
            return;
        }
        if let Some(dur) = self.ctl.store.audio(t).map(|a| a.duration()) {
            ui.separator();
            self.cue_points(ui, t, dur, true, locked, actions);
        }
    }

    /// The Control tab: the hotkey, MIDI pads and system-wide keys, and the messages the
    /// tile sends when it starts or stops.
    fn control_tab(&self, ui: &mut Ui, t: &Tile, orig: &Tile, actions: &mut Vec<Action>) {
        egui::Grid::new("control-tab").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
            self.hotkey_rows(ui, t, actions);
            ui.label("Pads, keys");
            self.bindings_field(ui, t.id, actions);
            ui.end_row();
        });
        ui.separator();
        ui.label(RichText::new("MESSAGES").small().strong().weak());
        self.messages_editor(ui, orig, actions);
    }

    /// The hotkey's grid rows: a button that waits for a key, and Clear; under them
    /// Automatic, for a key set by hand (D-188), on a row of its own so a narrow column
    /// (D-211) has room for it.
    fn hotkey_rows(&self, ui: &mut Ui, t: &Tile, actions: &mut Vec<Action>) {
        ui.label("Hotkey");
        ui.horizontal(|ui| {
            let label = if self.capturing_hotkey {
                "Press a key…".to_owned()
            } else {
                t.hotkey.map_or_else(|| "None".into(), PadKey::label)
            };
            // 90 wide, or what's left beside Clear in a narrow inspector (D-211).
            let wide = (ui.available_width() - box_width(ui, "Clear") - ui.spacing().item_spacing.x).clamp(40.0, 90.0);
            let btn = egui::Button::new(RichText::new(label).monospace()).min_size(vec2(wide, 0.0));
            let how = if t.hotkey_manual { ", set by hand" } else { ", from its slot" };
            let key_name = if self.capturing_hotkey { "Hotkey, press a key".to_owned() } else { format!("Hotkey, {}{how}", t.hotkey.map_or_else(|| "none".to_owned(), PadKey::spoken)) };
            if theme::hint(theme::name(ui.add(btn), &key_name), tips::HOTKEY_TIP).clicked() {
                actions.push(Action::CaptureHotkey);
            }
            if t.hotkey.is_some() && theme::hint(theme::small_button(ui, "Clear"), "Take the key off this tile").clicked() {
                actions.push(inv!("tile.set", {"tile": t.id, "hotkey": null}).into());
            }
        });
        ui.end_row();
        if t.hotkey_manual {
            ui.label("");
            if theme::hint(theme::small_button(ui, "Automatic"), tips::HOTKEY_AUTO_TIP).clicked() {
                actions.push(inv!("tile.set", {"tile": t.id, "hotkey": "auto"}).into());
            }
            ui.end_row();
        }
    }

    /// The tile's MIDI pads and system-wide keys, MIDI… and Key… to learn one, and Clear.
    fn bindings_field(&self, ui: &mut Ui, id: Uuid, actions: &mut Vec<Action>) {
        ui.horizontal_wrapped(|ui| {
            let binds: Vec<String> = self.ctl.tile_bindings(id).iter().map(|b| b.source.describe()).collect();
            for b in &binds {
                ui.label(RichText::new(b).small().monospace());
            }
            let tile_key = json!(id.to_string());
            let waiting = self.ctl.learning.as_ref().filter(|l| l.params.get("tile") == Some(&tile_key)).map(|l| l.key);
            let learning = waiting.is_some();
            if let Some(key) = waiting {
                ui.label(RichText::new(if key { "Press a key combination…" } else { "Press a pad…" }).small().italics());
                if theme::hint(theme::small_button(ui, "Cancel"), "Stop waiting for a pad or key").clicked() {
                    actions.push(inv!("bind.learn", {"cancel": true}).into());
                }
            } else {
                if theme::hint(theme::small_button(ui, "MIDI…"), "Click, then press a pad or key on a MIDI controller to play this tile with it").clicked() {
                    actions.push(inv!("bind.learn", {"tile": id}).into());
                }
                let keys_ok = self.ctl.hotkey_status.available;
                let tip = if keys_ok {
                    "Click, then press a key combination (with Ctrl, Alt, Shift or Cmd, or an F-key) to play this tile from any app".to_owned()
                } else {
                    self.ctl.hotkey_status.note.clone().unwrap_or_default()
                };
                let key = ui.add_enabled(keys_ok, theme::small(egui::Button::new("Key…")));
                let why = if keys_ok { String::new() } else { tip.clone() };
                let tip = if keys_ok { tip } else { "A key combination that plays this tile from any app".to_owned() };
                if theme::hint_or(key, tip, &why).clicked() {
                    actions.push(inv!("bind.learn", {"tile": id, "from": "key"}).into());
                }
            }
            if !binds.is_empty() && !learning && theme::hint(theme::small_button(ui, "Clear"), "Remove this tile's pads and keys").clicked() {
                actions.push(inv!("bind.remove", {"tile": id}).into());
            }
        });
    }

    /// What the tile's sound is, for the footer: "WAV · 44.1 kHz · stereo · 0:06.0". The
    /// library keeps its sounds under made-up names, so the file's name says nothing.
    fn sound_line(&self, t: &Tile) -> String {
        let Some(file) = t.file.as_deref() else { return "No sound yet".to_owned() };
        let mut parts = vec![std::path::Path::new(file).extension().and_then(|e| e.to_str()).unwrap_or("Audio").to_uppercase()];
        if let Some(a) = self.ctl.store.audio(t) {
            parts.push(format!("{} kHz", a.sample_rate as f32 / 1000.0));
            parts.push(a.native.as_ref().map_or_else(|| "stereo".to_owned(), |n| format!("{} channels", n.labels.len())));
            parts.push(format_clock(a.duration(), true));
        }
        parts.join(" · ")
    }

    /// An import under way or a file that can't be used: a line above the footer.
    fn import_line(&self, t: &Tile) -> Option<()> {
        (matches!(self.ctl.store.media(t), Some(Media::Failed(_))) || self.ctl.store.import_of(t).is_some_and(|i| i.busy())).then_some(())
    }

    /// The footer's buttons in rows that fit the column: one row when they fit, else they
    /// wrap (D-211), Remove last and at the right. Record isn't there while this tile
    /// records: its Stop Recording is in the hero.
    fn footer_rows(&self, ui: &Ui, t: &Tile) -> Vec<Vec<FooterButton>> {
        let recording_here = self.ctl.recording.as_ref().is_some_and(|r| r.tile == t.id);
        let buttons = [FooterButton::Replace, FooterButton::Record, FooterButton::Remove].into_iter().filter(|b| *b != FooterButton::Record || !recording_here);
        let (avail, gap) = (ui.available_width(), ui.spacing().item_spacing.x);
        let mut rows: Vec<Vec<FooterButton>> = vec![];
        let mut used = 0.0;
        for b in buttons {
            let w = box_width(ui, b.label());
            match rows.last_mut() {
                Some(row) if used + gap + w <= avail => {
                    row.push(b);
                    used += gap + w;
                }
                _ => {
                    rows.push(vec![b]);
                    used = w;
                }
            }
        }
        rows
    }

    /// The footer, pinned to the inspector's bottom: the sound file's name, Replace
    /// Audio…, Record and Remove (also in the tile's menu).
    fn tile_footer(&self, ui: &mut Ui, t: &Tile, locked: bool, actions: &mut Vec<Action>) {
        let id = t.id;
        let rows = self.footer_rows(ui, t);
        ui.with_layout(egui::Layout::bottom_up(Align::LEFT), |ui| {
            ui.add_space(8.0);
            ui.add_enabled_ui(!locked, |ui| {
                // Bottom up: the last row first.
                for row in rows.iter().rev() {
                    ui.horizontal(|ui| {
                        for b in row {
                            match b {
                                FooterButton::Replace => {
                                    if theme::hint_or(ui.button(b.label()), "Choose another sound file for this tile, keeping its settings", tips::LOCKED_WHY).clicked() {
                                        actions.push(inv!("tile.replaceAudio", {"tile": id}).into());
                                    }
                                }
                                FooterButton::Record => {
                                    let record = theme::name(ui.add_enabled(self.ctl.recording.is_none(), egui::Button::new(b.label())), "Record");
                                    let why = if locked { tips::LOCKED_WHY } else { "Another tile is recording: stop it first" };
                                    if theme::hint_or(record, format!("Record from {} into this tile, replacing its sound (Settings > Voice chooses the input)", self.ctl.input_name()), why).clicked() {
                                        actions.push(inv!("record.start", {"tile": id}).into());
                                    }
                                }
                                FooterButton::Remove => {
                                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                                        let remove = ui.button(RichText::new(b.label()).color(Tokens::get(ui.ctx()).error));
                                        if theme::hint_or(theme::name(remove, "Remove Tile"), "Remove this tile (Undo brings it back)", tips::LOCKED_WHY).clicked() {
                                            actions.push(inv!("tile.remove", {"tile": id}).into());
                                        }
                                    });
                                }
                            }
                        }
                    });
                }
            });
            ui.add(egui::Label::new(RichText::new(self.sound_line(t)).small().color(Tokens::get(ui.ctx()).text_dim)).truncate());
            if let Some(Media::Failed(e)) = self.ctl.store.media(t) {
                ui.colored_label(Tokens::get(ui.ctx()).error, format!("Can't use this file: {e}"));
            } else if let Some(i) = self.ctl.store.import_of(t).filter(|i| i.busy()) {
                ui.horizontal(|ui| {
                    let what = if i.verb() == "Converting" { format!("Converting {} with ffmpeg", i.file_name()) } else { format!("{} {}", i.verb(), i.file_name()) };
                    ui.add(egui::ProgressBar::new(i.fraction().unwrap_or(0.0)).desired_width(160.0).text(what));
                    if theme::hint(theme::small_button(ui, "Cancel"), "Stop this import; the tile stays, marked cancelled").clicked() {
                        actions.push(inv!("import.cancel").into());
                    }
                });
            }
            ui.separator();
        });
    }

    /// An action tile in the same frame: its name and colour, then what it does (in
    /// place of the hero), Disabled, and its cue and keys.
    fn action_inspector(&self, ui: &mut Ui, orig: &Tile, actions: &mut Vec<Action>) {
        let mut t = orig.clone();
        let id = t.id;
        let set = self.ctl.store.library.locate(id).map(|(si, _)| si);
        let locked = self.inspector_head(ui, "ACTION TILE", set, actions);
        egui::ScrollArea::vertical().id_salt("inspector").auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            ui.add_enabled_ui(!locked, |ui| {
                self.identity_row(ui, &mut t, false, actions);
                self.action_editor(ui, &t, actions);
                let w = (ui.available_width() * 0.6).max(120.0);
                if pad_options::option_button(ui, PadOption::Disabled, Some(t.disabled), w, true).clicked() {
                    t.disabled = !t.disabled;
                }
                ui.separator();
                ui.label(RichText::new("CUE").small().strong().weak());
                theme::hint(theme::name(notes_editor(ui, &mut t.notes, "Notes, shown in the cue list"), "Notes"), "Notes for this cue, shown under its title in the cue list and in search");
                cue_link_editor(ui, &mut t);
                ui.separator();
                ui.label(RichText::new("CONTROL").small().strong().weak());
                egui::Grid::new("action-control").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
                    self.hotkey_rows(ui, &t, actions);
                    ui.label("Pads, keys");
                    self.bindings_field(ui, id, actions);
                    ui.end_row();
                });
                ui.add_space(8.0);
                if theme::hint_or(ui.button(RichText::new("Remove Tile").color(Tokens::get(ui.ctx()).error)), "Remove this tile (Undo brings it back)", tips::LOCKED_WHY).clicked() {
                    actions.push(inv!("tile.remove", {"tile": id}).into());
                }
            });
        });
        if t != *orig && !locked {
            actions.push(Action::Run(tile_changes(orig, &t)));
        }
    }

    /// Several tiles in the same frame: the settings they can share, each shown as a
    /// dash where the tiles differ (a mixed option is outlined). A change goes to all of
    /// them as one `tile.set`, so it's one undo step.
    fn multi_inspector(&self, ui: &mut Ui, ids: &[Uuid], actions: &mut Vec<Action>) {
        let lib = &self.ctl.store.library;
        let tiles: Vec<Tile> = ids.iter().filter_map(|id| lib.tile(*id)).cloned().collect();
        let Some(first) = tiles.first().cloned() else { return };
        // A setting's value if every tile has the same one.
        fn same<T: PartialEq>(tiles: &[Tile], f: impl Fn(&Tile) -> T) -> Option<T> {
            let v = f(&tiles[0]);
            tiles[1..].iter().all(|t| f(t) == v).then_some(v)
        }
        // Changes collect here and go out as tile.set over the selection afterwards.
        let changes = std::cell::RefCell::new(Vec::<(&str, Value)>::new());
        let set = |key: &'static str, v: Value| changes.borrow_mut().push((key, v));
        let locked_set = tiles.iter().find_map(|t| lib.locate(t.id).filter(|(si, _)| lib.sets[*si].locked).map(|(si, _)| si));
        let locked = self.inspector_head(ui, &format!("{} TILES", tiles.len()), locked_set.or_else(|| lib.locate(first.id).map(|(si, _)| si)), actions);
        ui.label(RichText::new("Changes here apply to every selected tile. ⌘/Ctrl-click adds or removes a tile, Shift-click selects the rectangle of tiles up to it.").small().weak());
        egui::ScrollArea::vertical().id_salt("inspector").auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            ui.add_enabled_ui(!locked, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Colour");
                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        if let Some(c) = colour_menu(ui, same(&tiles, |t| t.color)) {
                            set("color", json!(c));
                        }
                    });
                });
                // The pad options: a mixed one turns on for all of them.
                if let Some(opt) = pad_options::option_row(ui, |o| same(&tiles, |t| o.get(t))) {
                    set(opt.key(), json!(same(&tiles, |t| opt.get(t)) != Some(true)));
                }
                let common = same(&tiles, |t| t.volume);
                let mut v = common.unwrap_or(first.volume);
                if volume_slider(ui, &mut v, common.is_none()).changed() {
                    set("volume", json!(v));
                }
            });
            let tab = self.tab_bar(ui, &[InspectorTab::Play, InspectorTab::Sound], actions);
            ui.add_enabled_ui(!locked, |ui| {
                egui::Grid::new("multi-inspector-grid").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
                    const MIXED: &str = "—";
                    let values: &[&str] = if tab == InspectorTab::Sound { &SOUND_VALUES } else { &PLAY_VALUES };
                    let slider = |ui: &mut Ui, key: &'static str, label: &str, tip: &str, get: fn(&Tile) -> f32, range: std::ops::RangeInclusive<f32>, step: f64, fmt: fn(f64) -> String| {
                        ui.label(label);
                        fit_sliders(ui, values, 0.0);
                        let common = same(&tiles, get);
                        let mut v = common.unwrap_or(get(&first));
                        let mixed = common.is_none();
                        let resp = ui.add(egui::Slider::new(&mut v, range).step_by(step).custom_formatter(move |x, _| if mixed { MIXED.to_owned() } else { fmt(x) }));
                        let resp = theme::hint(theme::name(resp, label), if mixed { format!("{tip} (— where they differ; a change sets them all)") } else { tip.to_owned() });
                        if resp.changed() {
                            set(key, json!(v));
                        }
                        ui.end_row();
                    };
                    if tab == InspectorTab::Sound {
                        slider(ui, "gainDb", "Gain", "Raise or lower each clip on top of its volume, ±dB", |t| t.gain_db, -GAIN_DB_MAX..=GAIN_DB_MAX, 0.1, |v| format!("{v:+.1} dB"));
                        slider(ui, "speed", "Speed", "Faster or slower; the pitch follows, like tape", |t| t.speed, SPEED_MIN..=SPEED_MAX, 0.05, |v| format!("{v:.2}×"));
                        return;
                    }
                    slider(ui, "fadeIn", "Fade in", tips::FADE_IN_TIP, |t| t.fade_in, 0.0..=10.0, 0.1, |v| format!("{v:.1} s"));
                    slider(ui, "fadeOut", "Fade out", tips::FADE_OUT_TIP, |t| t.fade_out, 0.0..=10.0, 0.1, |v| format!("{v:.1} s"));
                    slider(ui, "stopFade", "Stop fade", tips::STOP_FADE_TIP, |t| t.stop_fade, 0.0..=10.0, 0.1, stop_fade_text);

                    ui.label("Fade shape");
                    let curve = same(&tiles, |t| t.fade_curve);
                    let mut c = curve;
                    let combo = egui::ComboBox::from_id_salt("multi-fade-curve").width(ui.available_width().min(200.0)).selected_text(curve.map_or(MIXED, |c| c.label())).show_ui(ui, |ui| {
                        for x in FadeCurve::ALL {
                            ui.selectable_value(&mut c, Some(x), x.label());
                        }
                    });
                    theme::hint(theme::name(combo.response, &format!("Fade shape, {}", curve.map_or(MIXED, |c| c.label()))), tips::FADE_SHAPE_TIP);
                    if let Some(c) = c.filter(|_| c != curve) {
                        set("fadeCurve", json!(c));
                    }
                    ui.end_row();

                    ui.label("Ducking");
                    let duck = same(&tiles, |t| t.duck);
                    let mut d = duck;
                    let combo = egui::ComboBox::from_id_salt("multi-duck").width(ui.available_width().min(200.0)).selected_text(duck.map_or(MIXED, |d| d.label())).show_ui(ui, |ui| {
                        for x in Duck::ALL {
                            theme::hint(ui.selectable_value(&mut d, Some(x), x.label()), x.help());
                        }
                    });
                    theme::hint(theme::name(combo.response, &format!("Ducking, {}", duck.map_or(MIXED, |d| d.label()))), tips::DUCK_TIP);
                    if let Some(d) = d.filter(|_| d != duck) {
                        set("duck", json!(d));
                    }
                    ui.end_row();
                });
            });
        });
        let changes = changes.into_inner();
        if !changes.is_empty() && !locked {
            let mut params = serde_json::Map::new();
            params.insert("tiles".into(), json!(ids));
            params.extend(changes.into_iter().map(|(k, v)| (k.to_owned(), v)));
            actions.push(Invocation::new("tile.set", Value::Object(params)).into());
        }
    }

    /// The current set's settings (M22), when no tile is selected: its name, the lock, the
    /// layout and columns, auto-advance and its notes. Locked, only the lock and Grid or
    /// List change.
    fn set_inspector(&self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let si = self.ctl.set_idx();
        let set = &self.ctl.store.library.sets[si];
        let (sid, locked) = (set.id, set.locked);
        self.inspector_head(ui, "SET", None, actions);
        egui::ScrollArea::vertical().id_salt("inspector").auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            ui.add_enabled_ui(!locked, |ui| {
                let mut name = set.name.clone();
                let field = theme::text_field(ui, egui::TextEdit::singleline(&mut name).font(FontId::proportional(16.0)).desired_width(f32::INFINITY));
                if theme::hint(theme::name(field, "Set name"), "The set's name, in the sidebar").changed() && !name.trim().is_empty() {
                    actions.push(inv!("set.rename", {"set": sid, "name": name}).into());
                }
            });
            let resp = lock_button(ui, locked);
            if theme::hint(theme::role(resp, AxRole::Button, "Lock set", Some(locked)), if locked { tips::UNLOCK } else { tips::LOCK }).clicked() {
                actions.push(Invocation::new(if locked { "set.unlock" } else { "set.lock" }, json!({"set": sid})).into());
            }
            ui.separator();
            let lib = &self.ctl.store.library;
            egui::Grid::new("set-settings").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
                // Grid or List is how the set is run, so it isn't locked (D-149).
                ui.label("Layout");
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    for (layout, label, name, tip) in [(Layout::Grid, "Grid", "Grid", tips::GRID), (Layout::List, "List", "Cue list", tips::LIST)] {
                        let key = theme::role(theme::key(ui, label, vec2(54.0, 24.0), set.layout == layout), AxRole::Button, name, Some(set.layout == layout));
                        if theme::hint_keys(key, tip.0, tip.1).clicked() && set.layout != layout {
                            actions.push(inv!("set.layout", {"layout": layout, "set": sid}).into());
                        }
                    }
                });
                ui.end_row();
                ui.add_enabled_ui(!locked, |ui| ui.label("Columns"));
                ui.add_enabled_ui(!locked, |ui| {
                    let mut columns = set.columns;
                    let field = ui.add_enabled(lib.grid != GridMode::Auto, egui::DragValue::new(&mut columns).range(1..=12).speed(0.05));
                    let tip = match lib.grid {
                        GridMode::Rows => "Tiles per row, 1–12: evens this set's rows out (a fuller row's tiles narrow to fit)",
                        _ => "Tiles per row in this set's grid, 1–12",
                    };
                    theme::hint_or(theme::name(field, "Columns"), tip, tips::AUTO_COLUMNS);
                    if columns != set.columns {
                        match lib.grid {
                            GridMode::Rows => actions.push(inv!("set.rows", {"perRow": columns, "set": sid}).into()),
                            _ => actions.push(inv!("set.columns", {"columns": columns, "set": sid}).into()),
                        }
                    }
                });
                ui.end_row();
                if lib.grid == GridMode::Rows {
                    // Under the field, where a long list of rows has room (D-211).
                    let rows = set.row_lengths();
                    ui.label("");
                    ui.add_enabled_ui(!locked, |ui| ui.weak(format!("{} row{}: {}", rows.len(), if rows.len() == 1 { "" } else { "s" }, rows.iter().map(|r| r.to_string()).collect::<Vec<_>>().join(", "))));
                    ui.end_row();
                }
                ui.add_enabled_ui(!locked, |ui| ui.label("Cue list"));
                ui.add_enabled_ui(!locked, |ui| {
                    if let Some(continuous) = list_mode_switch(ui, set.auto_advance, locked) {
                        actions.push(inv!("set.listMode", {"mode": if continuous { "continuous" } else { "queued" }, "set": sid}).into());
                    }
                });
                ui.end_row();
            });
            ui.add_enabled_ui(!locked, |ui| {
                let mut notes = set.notes.clone();
                if theme::hint(theme::name(notes_editor(ui, &mut notes, "Notes for this set, shown above the cue list"), "Set notes"), "Notes for this set (an act, a scene), shown above the cue list").changed() {
                    actions.push(inv!("set.notes", {"set": sid, "notes": notes}).into());
                }
            });
            let n = set.tiles.len();
            ui.weak(format!("{n} tile{}. Click a tile to show its settings here.", if n == 1 { "" } else { "s" }));
        });
    }
}

/// A button in the tile inspector's footer.
#[derive(Clone, Copy, PartialEq)]
enum FooterButton {
    Replace,
    Record,
    Remove,
}

impl FooterButton {
    fn label(self) -> &'static str {
        match self {
            FooterButton::Replace => "Replace Audio…",
            FooterButton::Record => "⏺ Record",
            FooterButton::Remove => "Remove",
        }
    }
}

/// The volume, one slider across the column (`mixed` for a selection that differs).
fn volume_slider(ui: &mut Ui, volume: &mut f32, mixed: bool) -> egui::Response {
    ui.horizontal(|ui| {
        ui.label("Volume");
        fit_sliders(ui, &["−∞ dB", "−40.0 dB"], 0.0);
        let slider = egui::Slider::new(volume, 0.0..=1.0).custom_formatter(move |v, _| {
            if mixed {
                "—".into()
            } else if v <= 0.0 {
                "−∞ dB".into()
            } else {
                format!("{:.1} dB", 20.0 * v.log10())
            }
        });
        let tip = if mixed { "Each tile's level (— where they differ; a change sets them all)" } else { "The tile's level: drag, or type a value" };
        theme::hint(theme::name(ui.add(slider), "Volume"), tip)
    })
    .inner
}

/// The Play tab: the fades, the stop fade, the fade shape and ducking.
fn play_tab(ui: &mut Ui, t: &mut Tile) {
    egui::Grid::new("play-tab").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
        ui.label("Fade in");
        fit_sliders(ui, &PLAY_VALUES, 0.0);
        theme::hint(theme::name(ui.add(egui::Slider::new(&mut t.fade_in, 0.0..=10.0).step_by(0.1).suffix(" s")), "Fade in"), tips::FADE_IN_TIP);
        ui.end_row();
        ui.label("Fade out");
        theme::hint(theme::name(ui.add(egui::Slider::new(&mut t.fade_out, 0.0..=10.0).step_by(0.1).suffix(" s")), "Fade out"), tips::FADE_OUT_TIP);
        ui.end_row();
        ui.label("Stop fade");
        theme::hint(theme::name(ui.add(egui::Slider::new(&mut t.stop_fade, 0.0..=10.0).step_by(0.1).custom_formatter(|x, _| stop_fade_text(x))), "Stop fade"), tips::STOP_FADE_TIP);
        ui.end_row();
        ui.label("Fade shape");
        let combo = egui::ComboBox::from_id_salt("fade-curve").width(ui.available_width().min(200.0)).selected_text(t.fade_curve.label()).show_ui(ui, |ui| {
            for c in FadeCurve::ALL {
                ui.selectable_value(&mut t.fade_curve, c, c.label());
            }
        });
        theme::hint(theme::name(combo.response, &format!("Fade shape, {}", t.fade_curve.label())), tips::FADE_SHAPE_TIP);
        ui.end_row();
        ui.label("Ducking");
        let combo = egui::ComboBox::from_id_salt("duck").width(ui.available_width().min(200.0)).selected_text(t.duck.label()).show_ui(ui, |ui| {
            for d in Duck::ALL {
                theme::hint(ui.selectable_value(&mut t.duck, d, d.label()), d.help());
            }
        });
        theme::hint(theme::name(combo.response, &format!("Ducking, {}", t.duck.label())), tips::DUCK_TIP);
        ui.end_row();
    });
}

/// What each tab holds, for its hover text.
fn tab_tip(tab: InspectorTab) -> &'static str {
    match tab {
        InspectorTab::Play => "Play: the fades, the stop fade, the fade shape and ducking",
        InspectorTab::Sound => "Sound: gain and Normalise, speed, pitch, outputs, the exact trim and channels",
        InspectorTab::Cue => "Cue: notes, the pre-wait, what starts the next cue, the vamp and markers",
        InspectorTab::Control => "Control: the hotkey, MIDI pads and keys, and the messages it sends",
    }
}

/// Fade Out's time: the tile's stop fade, or 2 s for a tile that stops hard.
fn fade_out_secs(t: &Tile) -> f32 {
    if t.stop_fade > 0.0 { t.stop_fade } else { 2.0 }
}

/// The colour: one swatch that opens the palette in a pop-up (D-144). `current` is
/// `None` for a selection whose colours differ. Returns the colour picked.
fn colour_menu(ui: &mut Ui, current: Option<TileColor>) -> Option<TileColor> {
    let tokens = Tokens::get(ui.ctx());
    let fill = |ui: &Ui, c: TileColor| if theme::is_panel(ui.ctx()) { theme::panel_tile(ui.ctx(), c) } else { theme::console_tile(ui.ctx(), c) };
    let (rect, resp) = ui.allocate_exact_size(vec2(52.0, 26.0), Sense::click());
    let p = ui.painter();
    let swatch = Rect::from_min_size(rect.min, vec2(30.0, rect.height()));
    p.rect_filled(rect, 5.0, if resp.hovered() { tokens.key_hover } else { tokens.key });
    match current {
        Some(c) => {
            p.rect_filled(swatch.shrink(3.0), 4.0, fill(ui, c));
        }
        None => {
            // Mixed: two halves of the palette.
            let (l, r) = swatch.shrink(3.0).split_left_right_at_fraction(0.5);
            p.rect_filled(l, 0.0, fill(ui, TileColor::nth(0)));
            p.rect_filled(r, 0.0, fill(ui, TileColor::nth(8)));
        }
    }
    p.rect_stroke(rect, 5.0, Stroke::new(1.0_f32, tokens.key_border), StrokeKind::Inside);
    // A chevron: there's a palette under it.
    let c = pos2(rect.right() - 11.0, rect.center().y);
    let chevron = Stroke::new(1.6_f32, tokens.text);
    p.line_segment([c + vec2(-4.0, -2.0), c + vec2(0.0, 2.0)], chevron);
    p.line_segment([c + vec2(0.0, 2.0), c + vec2(4.0, -2.0)], chevron);
    let name = current.map_or_else(|| "mixed".to_owned(), |c| format!("{c:?}").to_lowercase());
    let resp = theme::role(resp, AxRole::Button, format!("Colour, {name}"), None);
    let resp = theme::hint(resp, "The tile's colour: click for the palette");
    let mut picked = None;
    egui::Popup::from_toggle_button_response(&resp).show(|ui| {
        ui.set_width(4.0 * 29.0);
        egui::Grid::new("colour-palette").spacing([5.0, 5.0]).show(ui, |ui| {
            for (i, c) in TileColor::ALL.into_iter().enumerate() {
                if colour_swatch(ui, c, current == Some(c)).clicked() {
                    picked = Some(c);
                }
                if i % 4 == 3 {
                    ui.end_row();
                }
            }
        });
    });
    picked
}

/// The set lock as a wide toggle: a padlock and "Locked" lit in the accent, or open.
fn lock_button(ui: &mut Ui, locked: bool) -> egui::Response {
    let tokens = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::click());
    let fill = if locked {
        tokens.accent
    } else if resp.hovered() {
        tokens.key_hover
    } else {
        tokens.key
    };
    let ink = theme::ink_on(fill);
    let p = ui.painter();
    p.rect_filled(rect, 6.0, fill);
    let text = if locked { "Locked: nothing in this set can change" } else { "Lock Set" };
    let g = p.layout_no_wrap(text.to_owned(), egui::TextStyle::Button.resolve(ui.style()), ink);
    let left = rect.center().x - (18.0 + 8.0 + g.size().x) / 2.0;
    pad_options::paint_lock(p, pos2(left + 9.0, rect.center().y), 18.0, locked, ink);
    p.galley(pos2(left + 26.0, rect.center().y - g.size().y / 2.0), g, ink);
    resp
}
