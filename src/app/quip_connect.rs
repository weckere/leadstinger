//! Connecting QuipClipper without pasting its token (D-132..D-135): the prompt an opened
//! link from a server that isn't connected brings up, pairing's progress, and the servers
//! found on the network.
//!
//! The prompt is drawn from the link alone: nothing goes to the server until Connect is
//! clicked (src/quip.rs). Cancel has the keyboard focus, so a key pressed for the board
//! (Space for GO, Enter) cancels instead of connecting; Connect takes a click, or a
//! deliberate Tab to it.

use super::*;
use crate::quip::pair::Status;
use crate::quip::{Ask, Whereabouts};

/// The prompt, while it's up.
pub(super) struct QuipPrompt {
    ask: Ask,
    phase: Phase,
}

#[derive(PartialEq)]
enum Phase {
    /// Waiting for Connect (or Import) or Cancel.
    Asking,
    /// Connect was clicked: pairing (src/quip/pair.rs) is under way.
    Pairing,
    /// It didn't work: why.
    Failed(String),
}

impl StingerApp {
    /// An opened link asks (Effect::ConfirmQuip). One prompt at a time: a link for another
    /// server while one is up is refused, and so is one from a server Cancel was clicked
    /// for since the app started, so a page can't stack prompts or ask again and again.
    pub(super) fn offer_quip(&mut self, ask: Ask) {
        let host = crate::quip::host_of(&ask.server);
        if !ask.connected && self.quip_declined.contains(&ask.server) {
            self.ctl.notify_error(format!("Refused a link from QuipClipper at {host}: you chose not to connect it. Connect it in Settings ▸ QuipClipper if you meant to."));
            return;
        }
        match &mut self.quip_prompt {
            Some(p) if p.ask.server == ask.server => {
                // The same server again: the newest clip is the one to import.
                if p.phase == Phase::Asking && ask.link.is_some() {
                    p.ask.link = ask.link;
                    p.ask.notes = ask.notes;
                }
            }
            Some(p) => {
                let other = crate::quip::host_of(&p.ask.server);
                self.ctl.notify_error(format!("Refused a link from QuipClipper at {host} while asking about {other}"));
            }
            None => self.quip_prompt = Some(QuipPrompt { ask, phase: Phase::Asking }),
        }
    }

    /// Connect `server` from Settings or a server found on the network: the user chose it
    /// there, so the prompt opens straight on pairing's progress.
    pub(super) fn connect_quip(&mut self, server: String) {
        if self.quip_prompt.is_some() {
            return;
        }
        self.quip_declined.retain(|s| *s != server);
        self.quip_prompt = Some(QuipPrompt { ask: Ask { server: server.clone(), link: None, notes: None, connected: false }, phase: Phase::Pairing });
        if let Err(e) = self.ctl.execute("quip.pair", json!({"url": server})) {
            self.quip_prompt.as_mut().expect("just set").phase = Phase::Failed(sentence(&e.message));
        }
    }

    /// Whether a dialog that keeps the board's keys is up.
    pub(super) fn asking(&self) -> bool {
        !self.confirm_sets.is_empty() || self.quip_prompt.is_some()
    }

    pub(super) fn quip_prompt_window(&mut self, ctx: &egui::Context) {
        let Some(p) = &mut self.quip_prompt else { return };
        // Pairing ended: import what the link sent, or say why not.
        if p.phase == Phase::Pairing {
            match crate::quip::pair::status(&p.ask.server) {
                Some(Status::Connected { .. }) => {
                    let p = self.quip_prompt.take().expect("checked");
                    crate::quip::pair::forget(&p.ask.server);
                    if let Some(link) = p.ask.link {
                        self.run(inv!("quip.import", {"link": link.to_link(), "notes": p.ask.notes}));
                    }
                    return;
                }
                Some(Status::Denied) => p.phase = Phase::Failed(format!("QuipClipper at {} said no.", crate::quip::host_of(&p.ask.server))),
                Some(Status::Expired) => p.phase = Phase::Failed("Nobody allowed it in QuipClipper in time. Try again, then click Allow there.".into()),
                Some(Status::Failed(m)) => p.phase = Phase::Failed(sentence(&m)),
                Some(Status::Cancelled) | None => {
                    self.quip_prompt = None;
                    return;
                }
                Some(Status::Starting | Status::Waiting { .. }) => ctx.request_repaint_after(Duration::from_millis(250)),
            }
        }
        let p = self.quip_prompt.as_ref().expect("still up");
        let t = Tokens::get(ctx);
        let host = crate::quip::host_of(&p.ask.server);
        let title = if p.ask.connected { "Import from QuipClipper?" } else { "Connect to QuipClipper?" };
        let mut answer: Option<bool> = None;
        let mut cancel_pairing = false;
        let modal = egui::Modal::new(egui::Id::unique("quip-connect")).show(ctx, |ui| {
            ui.set_max_width(400.0);
            theme::name_dialog(ui, title);
            ui.label(RichText::new(title).font(theme::bold(16.0)).color(t.text));
            ui.add_space(6.0);
            ui.label(RichText::new(&host).font(theme::semibold(18.0)).color(t.text));
            let place = Whereabouts::of(&p.ask.server);
            ui.label(RichText::new(format!("{} · {}", place.describe(), p.ask.server)).small().color(t.text_dim));
            if crate::quip::unencrypted_on_internet(&p.ask.server) {
                ui.label(RichText::new("Not encrypted (http) and on the internet: anyone on the way can see what's sent.").small().color(t.error));
            }
            ui.add_space(6.0);
            match &p.phase {
                Phase::Asking => {
                    if let Some(link) = &p.ask.link {
                        ui.label(format!("It sent {}.", link.described()));
                    }
                    if !p.ask.connected {
                        ui.add(egui::Label::new(RichText::new("Connect only to a QuipClipper you know. Nothing is sent to it, or downloaded from it, until you do.").small().color(t.text_dim)).wrap());
                    }
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        let cancel = theme::hint_keys(ui.button("Cancel"), if p.ask.connected { "Don't import it" } else { "Don't connect; links from it are refused until the app restarts" }, "Esc");
                        // Cancel takes the focus: a key meant for the board mustn't connect.
                        if ui.memory(|m| m.focused().is_none()) {
                            cancel.request_focus();
                        }
                        if cancel.clicked() {
                            answer = Some(false);
                        }
                        let yes = match (p.ask.connected, p.ask.link.is_some()) {
                            (true, _) => "Import",
                            (false, true) => "Connect and Import",
                            (false, false) => "Connect",
                        };
                        let tip = if p.ask.connected { "Download the clip into a new tile" } else { "Pair with this QuipClipper (it asks you to allow it there), or connect it if it has no password" };
                        if theme::hint(ui.button(yes), tip).clicked() {
                            answer = Some(true);
                        }
                    });
                }
                Phase::Pairing => match crate::quip::pair::status(&p.ask.server) {
                    Some(Status::Waiting { code, approve_url, .. }) => {
                        ui.label(format!("Allow {APP_NAME} in QuipClipper: it shows a request from {APP_NAME} on {} with this code.", crate::quip::pair::host_name()));
                        ui.add_space(4.0);
                        let code_label = ui.label(RichText::new(&code).font(theme::bold(28.0)).color(t.text));
                        theme::name(code_label, &format!("Pairing code {}", code.chars().map(String::from).collect::<Vec<_>>().join(" ")));
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            let cancel = theme::hint_keys(ui.button("Cancel"), "Stop waiting; nothing is kept", "Esc");
                            if ui.memory(|m| m.focused().is_none()) {
                                cancel.request_focus();
                            }
                            cancel_pairing |= cancel.clicked();
                            if theme::hint(ui.button("Open QuipClipper"), "Open QuipClipper's page in the browser, where Allow is").clicked() {
                                ui.ctx().open_url(egui::OpenUrl::new_tab(approve_url));
                            }
                            ui.spinner();
                        });
                    }
                    _ => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(format!("Asking {host}…"));
                        });
                        ui.add_space(6.0);
                        cancel_pairing |= theme::hint_keys(ui.button("Cancel"), "Stop", "Esc").clicked();
                    }
                },
                Phase::Failed(why) => {
                    ui.add(egui::Label::new(RichText::new(why).color(t.error)).wrap());
                    ui.add_space(10.0);
                    let close = theme::hint_keys(ui.button("Close"), "Close this; nothing was connected", "Esc");
                    if ui.memory(|m| m.focused().is_none()) {
                        close.request_focus();
                    }
                    if close.clicked() {
                        answer = Some(false);
                    }
                }
            }
        });
        let phase_pairing = p.phase == Phase::Pairing;
        if modal.should_close() {
            if phase_pairing {
                cancel_pairing = true;
            } else if answer.is_none() {
                answer = Some(false);
            }
        }
        if cancel_pairing {
            let p = self.quip_prompt.take().expect("still up");
            crate::quip::pair::cancel(&p.ask.server);
            crate::quip::pair::forget(&p.ask.server);
            return;
        }
        match answer {
            Some(true) => {
                let p = self.quip_prompt.as_mut().expect("still up");
                if p.ask.connected {
                    let p = self.quip_prompt.take().expect("still up");
                    if let Some(link) = p.ask.link {
                        self.run(inv!("quip.import", {"link": link.to_link(), "notes": p.ask.notes}));
                    }
                } else {
                    p.phase = Phase::Pairing;
                    let server = p.ask.server.clone();
                    if let Err(e) = self.ctl.execute("quip.pair", json!({"url": server})) {
                        self.quip_prompt.as_mut().expect("still up").phase = Phase::Failed(sentence(&e.message));
                    }
                }
            }
            Some(false) => {
                let p = self.quip_prompt.take().expect("still up");
                crate::quip::pair::forget(&p.ask.server);
                if !p.ask.connected && p.phase == Phase::Asking && !self.quip_declined.contains(&p.ask.server) {
                    self.quip_declined.push(p.ask.server);
                }
            }
            None => {}
        }
    }

    /// The QuipClipper servers found on the network, each with Connect, and Find on
    /// Network: in Settings ▸ QuipClipper and the QuipClipper window with no server.
    pub(super) fn quip_found_ui(&mut self, ui: &mut Ui) {
        let t = Tokens::get(ui.ctx());
        let looking = crate::quip::discover::looking();
        let (found, error) = crate::quip::discover::found(&self.ctl.store.data_dir);
        let mut connect = None;
        for f in &found {
            ui.horizontal(|ui| {
                let lock = if f.protected { " · has a password" } else { "" };
                ui.add(egui::Label::new(RichText::new(format!("QuipClipper on {} ({}){lock}", f.name, f.address)).small()).truncate());
                let resp = theme::name(ui.small_button("Connect"), &format!("Connect QuipClipper on {}", f.name));
                if theme::hint(resp, format!("Connect {} (it asks you to allow it in QuipClipper, unless it has no password)", f.server)).clicked() {
                    connect = Some(f.server.clone());
                }
            });
        }
        ui.horizontal(|ui| {
            let find = ui.add_enabled(!looking, egui::Button::new("Find on Network").small());
            if theme::hint_or(find, "Look for QuipClipper on this network for a few seconds (it has to advertise itself: QC_ADVERTISE)", "Looking…").clicked() {
                crate::quip::discover::start();
            }
            if looking {
                ui.spinner();
                ui.label(RichText::new("Looking for QuipClipper on this network…").small().color(t.text_dim));
                ui.ctx().request_repaint_after(Duration::from_millis(250));
            } else if let Some(e) = &error {
                ui.label(RichText::new(sentence(e)).small().color(t.error));
            } else if found.is_empty() && crate::quip::discover::has_looked() {
                ui.label(RichText::new("None found on this network.").small().color(t.text_dim));
            }
        });
        if let Some(s) = connect {
            self.connect_quip(s);
        }
    }
}

fn sentence(e: &str) -> String {
    crate::quip::sentence_case(e)
}
