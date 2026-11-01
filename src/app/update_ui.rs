//! Check for Updates' answer in the window (src/update.rs, D-206): a newer release's
//! notes with Download (which only opens its page) and Not now; after a menu check, a
//! small "latest version" note, or a one-line error in the status bar.

use super::*;
use crate::update::{Outcome, Show};

impl StingerApp {
    /// Take a finished check's answer, then draw the dialog it opened, if any.
    pub(super) fn update_window(&mut self, ctx: &egui::Context) {
        if let Some(done) = self.ctl.updates.finished_since(self.update_seen) {
            self.update_seen = done.seq;
            match (done.outcome, done.show) {
                (_, Show::Never) => {}
                (o @ Outcome::Newer(_), _) => self.update_dialog = Some(o),
                (o @ Outcome::UpToDate { .. }, Show::Always) => self.update_dialog = Some(o),
                (Outcome::Failed(e), Show::Always) => self.ctl.notify_error(format!("Couldn't check for updates: {e}")),
                // The launch check stays quiet unless there's something new.
                _ => {}
            }
        }
        let Some(outcome) = &self.update_dialog else { return };
        let t = Tokens::get(ctx);
        let current = crate::update::CURRENT;
        let mut close = false;
        let modal = egui::Modal::new(egui::Id::unique("update-check")).show(ctx, |ui| {
            ui.set_max_width(440.0);
            match outcome {
                Outcome::Newer(r) => {
                    let title = format!("{APP_NAME} {} is out", r.version);
                    theme::name_dialog(ui, &title);
                    ui.label(RichText::new(&title).font(theme::bold(16.0)).color(t.text));
                    ui.label(RichText::new(format!("You have {current}.")).color(t.text_dim));
                    ui.add_space(6.0);
                    if !r.notes.is_empty() {
                        egui::Frame::new().stroke(egui::Stroke::new(1.0, t.border)).corner_radius(4.0).inner_margin(8.0).show(ui, |ui| {
                            egui::ScrollArea::vertical().max_height(260.0).auto_shrink([false, true]).show(ui, |ui| {
                                let notes = ui.add(egui::Label::new(RichText::new(&r.notes).color(t.text)).wrap().selectable(true));
                                theme::name(notes, "Release notes");
                            });
                        });
                        ui.add_space(6.0);
                    }
                    ui.label(RichText::new("Download opens the release page in your browser; nothing is installed for you.").small().color(t.text_dim));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let download = theme::hint_keys(ui.button("Download"), &format!("Open {} in the browser", r.url), "Enter");
                        if ui.memory(|m| m.focused().is_none()) {
                            download.request_focus();
                        }
                        if download.clicked() {
                            ui.ctx().open_url(egui::OpenUrl::new_tab(&r.url));
                            close = true;
                        }
                        if theme::hint_keys(ui.button("Not now"), "Close this; Help > Check for Updates… shows it again", "Esc").clicked() {
                            close = true;
                        }
                    });
                }
                _ => {
                    let title = format!("You have the latest version ({current})");
                    theme::name_dialog(ui, &title);
                    ui.label(RichText::new(&title).font(theme::bold(15.0)).color(t.text));
                    ui.add_space(8.0);
                    let ok = theme::hint_keys(ui.button("OK"), "Close this", "Enter");
                    if ui.memory(|m| m.focused().is_none()) {
                        ok.request_focus();
                    }
                    close |= ok.clicked();
                }
            }
        });
        if close || modal.should_close() {
            self.update_dialog = None;
        }
    }
}
