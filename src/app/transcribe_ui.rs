//! Transcription in the window (D-277 to D-285): the Transcribe and Align Words buttons
//! (inspector, editor, tile menu), the busy state, the set menu's confirmation and the
//! Settings section. Every one sends a command (src/controller/transcribe.rs).

use super::icons::{self, Glyph};
use super::*;
use crate::controller::transcribe::Busy;
use crate::controller::Controller;

pub const TILE_TIP: &str = "Listen to the sound and write its subtitles, so a search finds it by what's said in it. A tile with subtitles keeps its text: its lines get word timings instead. Runs in the background; nothing leaves this computer with whisper-cli";
pub const SET_TIP: &str = "Transcribe every tile in this set that has a sound and no subtitles, one at a time in the background (it asks first, naming the engine)";
pub const ALIGN_TIP: &str = "Listen to this line and time its words from what's heard: the text stays as it is, and Reset Line puts the old timing back";

/// A set waiting for a yes to `set.transcribe`.
#[derive(Clone, Debug, PartialEq)]
pub struct Confirm {
    pub set: Uuid,
    pub tiles: usize,
    /// Tiles with subtitles from elsewhere, whose gaps it could fill (D-298).
    pub subtitled: usize,
    /// The box to fill them is ticked.
    pub gaps: bool,
    pub engine: String,
    /// Where the audio goes, for an engine that sends it off this computer.
    pub sends_to: Option<String>,
}

/// The Transcribe tooltip: [`TILE_TIP`], and where the audio goes when the engine in use
/// sends it to a server.
fn tile_tip(ctl: &Controller) -> String {
    match ctl.transcribe_destination() {
        Some(to) => format!("{TILE_TIP}. With the current engine the sound is sent to the {to}, which keeps none of it"),
        None => TILE_TIP.to_owned(),
    }
}

/// "Transcribing 45%" / "Waiting to transcribe", for a busy tile.
pub fn busy_text(b: Busy) -> String {
    match b {
        Busy::Queued => "Queued".to_owned(),
        Busy::Running(Some(f)) => format!("Transcribing {:.0}%", f * 100.0),
        Busy::Running(None) => "Transcribing…".to_owned(),
    }
}

/// The Transcribe button (icon and word), or a spinner and how far it is while the tile
/// is being transcribed. Gives the command to send.
pub fn transcribe_button(ui: &mut Ui, ctl: &Controller, t: &Tile, locked: bool) -> Option<Action> {
    if let Some(b) = ctl.transcriber.busy(t.id) {
        ui.spinner();
        let text = ui.label(RichText::new(busy_text(b)).small());
        theme::hint(text, "Transcription runs in the background; Cancel in the status bar stops it");
        return None;
    }
    let resp = ui.add_enabled(!locked && t.file.is_some(), theme::small(egui::Button::new("     Transcribe")));
    let color = if resp.enabled() { ui.style().interact(&resp).fg_stroke.color } else { ui.visuals().widgets.noninteractive.fg_stroke.color.gamma_multiply(0.6) };
    icons::paint(ui.painter(), resp.rect.left_center() + vec2(14.0, 0.0), 14.0, Glyph::Transcribe, color);
    let why = if locked { tips::LOCKED_WHY } else { "This tile has no sound" };
    let resp = theme::name(resp, "Transcribe");
    let tip = if resp.hovered() { tile_tip(ctl) } else { TILE_TIP.to_owned() };
    theme::hint_or(resp, &tip, why).clicked().then(|| inv!("tile.transcribe", {"tile": t.id}).into())
}

/// The tile menu's Transcribe item.
pub fn tile_menu_item(ui: &mut Ui, ctl: &Controller, t: &Tile, locked: bool, actions: &mut Vec<Action>) {
    if t.action.is_some() || t.file.is_none() {
        return;
    }
    let busy = ctl.transcriber.busy(t.id);
    let label = match busy {
        Some(b) => busy_text(b),
        None if t.subtitles.is_empty() => "Transcribe".to_owned(),
        None => "Transcribe (Time the Words)".to_owned(),
    };
    let item = ui.add_enabled(!locked && busy.is_none(), egui::Button::new(label));
    let why = if locked { tips::LOCKED_WHY } else { "It's being transcribed" };
    let item = theme::name(item, "Transcribe");
    let tip = if item.hovered() { tile_tip(ctl) } else { TILE_TIP.to_owned() };
    if theme::hint_or(item, &tip, why).clicked() {
        // The selection, when this tile is part of it (as the tile menu's other items do).
        let sel = ctl.selected();
        if sel.len() > 1 && sel.contains(&t.id) {
            actions.push(inv!("tile.transcribe", {"tiles": sel}).into());
        } else {
            actions.push(inv!("tile.transcribe", {"tile": t.id}).into());
        }
    }
}

impl StingerApp {
    /// `set.transcribe {ask}`: transcribe the set's tiles without subtitles? Names how many
    /// and the engine. Transcribe has the focus, so Enter starts and Esc doesn't.
    pub(super) fn confirm_transcribe_window(&mut self, ctx: &egui::Context) {
        let Some(c) = self.confirm_transcribe.first().cloned() else { return };
        let t = Tokens::get(ctx);
        let name = self.ctl.store.library.sets.iter().find(|s| s.id == c.set).map(|s| s.name.clone());
        let Some(name) = name else {
            self.confirm_transcribe.remove(0);
            return;
        };
        let mut answer = None;
        let mut gaps = c.gaps;
        let title = "Transcribe tiles without subtitles?";
        let modal = egui::Modal::new(egui::Id::unique("confirm-transcribe")).show(ctx, |ui| {
            ui.set_max_width(400.0);
            theme::name_dialog(ui, title);
            ui.label(RichText::new(title).font(theme::bold(16.0)).color(t.text));
            ui.add_space(6.0);
            let count = |n: usize| if n == 1 { "1 tile".to_owned() } else { format!("{n} tiles") };
            if c.tiles == 0 {
                ui.label(format!("Every tile in “{name}” with a sound has subtitles already."));
            } else {
                ui.label(format!("{} will listen to {} in “{name}” and write their subtitles, one at a time in the background.", c.engine, count(c.tiles)));
            }
            if c.subtitled > 0 {
                ui.add_space(4.0);
                let what = if c.subtitled == 1 { "the tile that has subtitles, for lines its subtitles leave out".to_owned() } else { format!("the {} tiles that have subtitles, for lines theirs leave out", c.subtitled) };
                let tip = "Adds the lines it hears where none of their subtitle lines is spoken, marked machine-made. Their own lines stay as they are";
                theme::hint(ui.checkbox(&mut gaps, format!("Also listen to {what}")), tip);
            }
            let total = c.tiles + if gaps { c.subtitled } else { 0 };
            if total > 0 {
                if let Some(to) = &c.sends_to {
                    ui.add_space(4.0);
                    ui.label(RichText::new(format!("Their audio is sent to the {to}, which keeps none of it.")).color(t.text));
                }
                ui.label(RichText::new("Each tile is one undo step. Cancel in the status bar stops it.").small().color(t.text_dim));
            }
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if total > 0 {
                    let how = match &c.sends_to {
                        Some(to) => format!("Send their audio to the {to} to transcribe them"),
                        None => format!("Transcribe them with {}", c.engine),
                    };
                    let go = theme::hint_keys(ui.button("Transcribe"), &how, "Enter");
                    if ui.memory(|m| m.focused().is_none()) {
                        go.request_focus();
                    }
                    if go.clicked() {
                        answer = Some(true);
                    }
                }
                if theme::hint_keys(ui.button(if total > 0 { "Cancel" } else { "OK" }), "Don't transcribe them", "Esc").clicked() {
                    answer = Some(false);
                }
            });
        });
        if let Some(first) = self.confirm_transcribe.first_mut() {
            first.gaps = gaps;
        }
        if answer.is_none() && modal.should_close() {
            answer = Some(false);
        }
        if let Some(go) = answer {
            self.confirm_transcribe.remove(0);
            if go {
                self.run(inv!("set.transcribe", {"set": c.set, "gaps": gaps}));
            }
        }
    }

    /// Settings ▸ Subtitles: the engine, the language and whisper-cli's model, with
    /// whether each engine can run and why not.
    pub(super) fn transcribe_settings(&self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let s = self.ctl.store.library.transcribe.clone();
        let ctx = self.ctl.transcribe_context();
        let found = crate::transcribe::availability(&ctx);
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.label("Engine");
                let choice = crate::transcribe::Choice::parse(&s.engine).unwrap_or_default();
                let shown = match choice {
                    crate::transcribe::Choice::Auto => "Automatic".to_owned(),
                    crate::transcribe::Choice::One(k) => k.label().to_owned(),
                };
                let combo = egui::ComboBox::from_id_salt("transcribe-engine").selected_text(shown).show_ui(ui, |ui| {
                    if ui.selectable_label(choice == crate::transcribe::Choice::Auto, "Automatic").clicked() {
                        actions.push(inv!("settings.transcribe", {"engine": "auto"}).into());
                    }
                    for a in &found {
                        let on = choice == crate::transcribe::Choice::One(a.kind);
                        if ui.selectable_label(on, a.kind.label()).clicked() {
                            actions.push(inv!("settings.transcribe", {"engine": a.kind.name()}).into());
                        }
                    }
                });
                theme::hint(theme::name(combo.response, "Transcription engine"), "Which engine writes subtitles. Automatic uses the first that can run. None is built in: whisper-cli is a separate program you install");
                ui.label("Language");
                let mut lang = s.lang.clone().unwrap_or_default();
                let edit = egui::TextEdit::singleline(&mut lang).hint_text("auto").desired_width(48.0);
                let resp = theme::hint(theme::name(theme::text_field(ui, edit), "Transcription language"), "A language code (en, de, fr…), or empty to let the engine tell; an English-only model always hears English");
                if resp.lost_focus() && lang.trim() != s.lang.clone().unwrap_or_default() {
                    actions.push(inv!("settings.transcribe", {"lang": lang.trim()}).into());
                }
            });
            ui.horizontal(|ui| {
                ui.label("whisper model");
                let shown = s.whisper_model.as_ref().and_then(|p| p.file_name()).map_or_else(|| "None chosen".to_owned(), |n| n.to_string_lossy().into_owned());
                let label = ui.label(RichText::new(shown).monospace());
                if let Some(p) = &s.whisper_model {
                    theme::hint(label, p.display().to_string());
                }
                if theme::hint(theme::small_button(ui, "Choose…"), "Choose a whisper.cpp model file (ggml-base.en.bin, say). The app downloads none itself").clicked() {
                    if let Some(path) = rfd::FileDialog::new().add_filter("whisper.cpp model", &["bin"]).set_title("Choose a whisper.cpp model").pick_file() {
                        actions.push(inv!("settings.transcribe", {"whisperModel": path}).into());
                    }
                }
                if s.whisper_model.is_some() && theme::hint(theme::small_button(ui, "Clear"), "Forget the model file (the file stays where it is)").clicked() {
                    actions.push(inv!("settings.transcribe", {"whisperModel": ""}).into());
                }
            });
            for a in &found {
                let (text, color) = match &a.reason {
                    None if a.kind == crate::transcribe::EngineKind::Quip => (quip_ready(&ctx), Tokens::get(ui.ctx()).text_dim),
                    None => (format!("{}: ready", a.kind.label()), Tokens::get(ui.ctx()).text_dim),
                    Some(why) => (format!("{}: {why}", a.kind.label()), Tokens::get(ui.ctx()).text_dim),
                };
                ui.add(egui::Label::new(RichText::new(text).small().color(color)).wrap());
            }
            self.quip_server_choice(ui, &ctx, actions);
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            Self::apple_speech_row(ui, actions, s.lang.as_deref());
        });
    }

    /// Apple Speech's language, whether it's on the device, and the one button that asks
    /// for permission (D-288: nothing else ever does).
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    fn apple_speech_row(ui: &mut Ui, actions: &mut Vec<Action>, lang: Option<&str>) {
        let st = crate::transcribe::apple::status(lang);
        let dim = Tokens::get(ui.ctx()).text_dim;
        ui.horizontal_wrapped(|ui| {
            if let Some(name) = &st.language {
                let place = if cfg!(target_os = "ios") { "this iPad" } else { "this Mac" };
                let where_ = match st.on_device {
                    Some(true) => format!("recognised on {place}"),
                    _ => format!("no on-device model on {place}"),
                };
                ui.add(egui::Label::new(RichText::new(format!("Apple Speech language: {name}, {where_}")).small().color(dim)).wrap());
            }
            if st.blocker.as_ref().is_some_and(crate::transcribe::apple_core::Blocker::can_ask)
                && theme::hint(theme::small_button(ui, "Allow Speech Recognition…"), "Ask the system for permission to recognise speech. Recognition runs on this device and nothing is sent anywhere; LeadStinger never asks on its own").clicked()
            {
                actions.push(inv!("transcribe.allowSpeech", {}).into());
            }
        });
    }
}

/// Settings' line for a QuipClipper that can transcribe: which server, why that one, and
/// why each other connected server can't.
fn quip_ready(ctx: &crate::transcribe::Context) -> String {
    use crate::quip::transcribe::{choose, why_chosen};
    let Ok(chosen) = choose(&ctx.quip, ctx.quip_server.as_deref()) else { return "QuipClipper: ready".to_owned() };
    let mut text = format!("QuipClipper: ready, sends the audio to {}, {}, which keeps none of it", chosen.label(), why_chosen(&ctx.quip, chosen, ctx.quip_server.as_deref()));
    for o in ctx.quip.iter().filter(|o| o.server != chosen.server) {
        match o.reason() {
            Some(why) => text.push_str(&format!(". Not {}: {}", o.label(), why.trim_start_matches(&format!("the QuipClipper at {} ", o.label())))),
            None => text.push_str(&format!(". {} transcribes too", o.label())),
        }
    }
    text
}

impl StingerApp {
    /// With more than one connected QuipClipper, which one transcribes.
    fn quip_server_choice(&self, ui: &mut Ui, ctx: &crate::transcribe::Context, actions: &mut Vec<Action>) {
        if ctx.quip.len() < 2 {
            return;
        }
        ui.horizontal(|ui| {
            ui.label("QuipClipper server");
            let wanted = ctx.quip_server.as_deref();
            let shown = wanted.and_then(|w| ctx.quip.iter().find(|o| o.server == w)).map_or_else(|| "First that transcribes".to_owned(), |o| o.label());
            let combo = egui::ComboBox::from_id_salt("transcribe-quip-server").selected_text(shown).show_ui(ui, |ui| {
                if ui.selectable_label(wanted.is_none(), "First that transcribes").clicked() {
                    actions.push(inv!("settings.transcribe", {"quipServer": ""}).into());
                }
                for o in &ctx.quip {
                    let label = match o.reason() {
                        None => o.label(),
                        Some(_) => format!("{} (doesn't transcribe now)", o.label()),
                    };
                    if ui.selectable_label(wanted == Some(o.server.as_str()), label).clicked() {
                        actions.push(inv!("settings.transcribe", {"quipServer": o.server}).into());
                    }
                }
            });
            theme::hint(theme::name(combo.response, "QuipClipper server for transcription"), "Which connected QuipClipper is sent the audio to transcribe. First that transcribes: the first, by address, whose answer says it takes uploads");
        });
    }
}
