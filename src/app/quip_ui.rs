//! Settings ▸ QuipClipper: connect the servers whose links open here, without the CLI
//! (src/quip.rs, D-089).

use super::*;
use std::sync::mpsc::{Receiver, TryRecvError};

/// How long the server list is trusted before it's read again (the CLI may change it).
const LIST_TTL: Duration = Duration::from_secs(2);

/// The QuipClipper part of Settings. The token typed here is never drawn, logged or
/// kept: it goes to `quip.connect` and the field is cleared once it connects.
#[derive(Default)]
pub(super) struct QuipSettings {
    url: String,
    token: String,
    /// Why the last Connect failed, shown under the form.
    error: Option<String>,
    /// A token check under way off the ui thread: the server, and its answer.
    checking: Option<(String, Receiver<Result<(), String>>)>,
    /// The connected servers (`quip.list`) and when they were read.
    servers: Option<(Instant, Vec<String>)>,
    /// What Register Links last said: the message, and whether it worked.
    registered: Option<(String, bool)>,
}

impl QuipSettings {
    /// Start Connect: check the URL and token here, then try the token on the server on
    /// its own thread, since that can take seconds. `wake` repaints when it's answered.
    fn connect(&mut self, wake: Option<egui::Context>) {
        self.error = None;
        let checked = crate::quip::normalize_server(self.url.trim()).and_then(|s| crate::quip::check_token(self.token.trim()).map(|()| s));
        let server = match checked {
            Ok(s) => s,
            Err(e) => return self.error = Some(sentence(&e)),
        };
        let (tx, rx) = std::sync::mpsc::channel();
        let (s, token) = (server.clone(), self.token.trim().to_owned());
        let spawned = std::thread::Builder::new().name("quip-check".into()).spawn(move || {
            tx.send(crate::quip::check_server(&s, &token)).ok();
            if let Some(ctx) = wake {
                ctx.request_repaint();
            }
        });
        match spawned {
            Ok(_) => self.checking = Some((server, rx)),
            Err(e) => self.error = Some(format!("Couldn't start the check: {e}")),
        }
    }

    /// Finish Connect once the server has answered: store the token with `quip.connect`
    /// (already checked, so not again) and clear the form, or show why not.
    fn poll(&mut self, ctl: &mut Controller) {
        let Some((server, rx)) = &self.checking else { return };
        let answer = match rx.try_recv() {
            Ok(a) => a,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err("the check stopped without an answer".into()),
        };
        let server = server.clone();
        self.checking = None;
        let stored = answer.and_then(|()| {
            ctl.execute("quip.connect", json!({"url": server, "token": self.token.trim(), "check": false})).map(|_| ()).map_err(|e| e.message)
        });
        match stored {
            Ok(()) => {
                self.url.clear();
                self.token.clear();
                self.servers = None;
            }
            Err(e) => self.error = Some(sentence(&e)),
        }
    }

    /// The connected servers, read again when stale.
    fn servers(&mut self, ctl: &mut Controller) -> Vec<String> {
        if self.servers.as_ref().is_none_or(|(at, _)| at.elapsed() > LIST_TTL) {
            let list = match ctl.execute("quip.list", Value::Null) {
                Ok(v) => v.as_array().into_iter().flatten().filter_map(|s| s["server"].as_str().map(str::to_owned)).collect(),
                Err(e) => {
                    self.error = Some(sentence(&e.message));
                    Vec::new()
                }
            };
            self.servers = Some((Instant::now(), list));
        }
        self.servers.as_ref().map(|(_, l)| l.clone()).unwrap_or_default()
    }
}

/// Capitalise an error from src/quip.rs, which reads as a clause in the CLI's output.
fn sentence(e: &str) -> String {
    let mut c = e.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

impl StingerApp {
    pub(super) fn quip_settings(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let q = &mut self.quip;
        q.poll(&mut self.ctl);
        let servers = q.servers(&mut self.ctl);
        let dim = Tokens::get(ui.ctx()).text_dim;
        if servers.is_empty() {
            ui.label(RichText::new("No server connected yet: a link from QuipClipper asks before connecting its server.").small().color(dim));
            // With nothing connected, look on the network by itself (D-134).
            crate::quip::discover::start_if_stale();
        }
        for s in &servers {
            ui.horizontal(|ui| {
                ui.add(egui::Label::new(RichText::new(s).monospace().small()).truncate());
                let resp = theme::name(ui.small_button("Disconnect"), &format!("Disconnect {s}"));
                if theme::hint(resp, "Forget this server's token, so its links are refused again").clicked() {
                    actions.push(inv!("quip.disconnect", {"url": s}).into());
                    q.servers = None;
                }
                let resp = theme::name(ui.small_button("Browse"), &format!("Browse {s}"));
                if theme::hint(resp, "Open this server's clips library, to preview and add clips").clicked() {
                    actions.push(inv!("view.quip", {"server": s}).into());
                }
            });
        }

        if !servers.is_empty() {
            let mut on = self.ctl.store.library.quip_badge;
            let tip = "Count the clips cut since you last looked, and show how many in the status bar while the QuipClipper window is closed (asks every minute; a click lists them)";
            if theme::hint(theme::checkbox(ui, &mut on, "Show new clips in the status bar"), tip).changed() {
                actions.push(inv!("settings.quipBadge", {"on": on}).into());
            }
            let mut ask = self.ctl.store.library.quip_ask_each;
            let tip = "A link from QuipClipper asks “Import …?” before it downloads, even from a server that's connected";
            if theme::hint(theme::checkbox(ui, &mut ask, "Ask before importing each QuipClipper link"), tip).changed() {
                actions.push(inv!("settings.quipAskEachImport", {"on": ask}).into());
            }
        }

        let busy = q.checking.is_some();
        let waiting = "Wait for the token check to finish";
        let mut go = false;
        ui.add_enabled_ui(!busy, |ui| {
            let url = theme::name(ui.add(egui::TextEdit::singleline(&mut q.url).hint_text("https://quip.example").desired_width(260.0)), "QuipClipper address");
            theme::hint_or(url, "The address QuipClipper's web page is at", waiting);
            ui.horizontal(|ui| {
                let field = theme::name(ui.add(egui::TextEdit::singleline(&mut q.token).password(true).hint_text("API token (optional)").desired_width(170.0)), "QuipClipper API token");
                let field = theme::hint_or(field, "QuipClipper's API token, QC_API_TOKEN on its server, kept in the data folder, readable only by you. Leave it empty to pair instead: QuipClipper asks you to allow this app", waiting);
                go |= field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                let ready = !q.url.trim().is_empty();
                let connect = ui.add_enabled(ready, egui::Button::new("Connect"));
                let tip = "With a token: try it on the server, then keep it. Without one: pair (QuipClipper asks you to allow it), or connect a server that has no password — Enter in the token field";
                go |= theme::hint_or(connect, tip, if busy { waiting } else { "Type the address first" }).clicked();
                if busy {
                    ui.spinner();
                }
            });
        });
        // No token: pair, in the prompt that shows the code (src/app/quip_connect.rs).
        let mut pair = None;
        if go && !busy {
            if q.token.trim().is_empty() {
                match crate::quip::normalize_server(q.url.trim()) {
                    Ok(s) => {
                        q.error = None;
                        q.url.clear();
                        q.servers = None;
                        pair = Some(s);
                    }
                    Err(e) => q.error = Some(sentence(&e)),
                }
            } else {
                q.connect(Some(ui.ctx().clone()));
            }
        }
        if let Some((server, _)) = &q.checking {
            ui.label(RichText::new(format!("Checking {server}…")).small().color(dim));
        }
        if let Some(e) = &q.error {
            ui.add(egui::Label::new(RichText::new(e).small().color(Tokens::get(ui.ctx()).error)).wrap());
        }

        if let Some(s) = pair {
            self.connect_quip(s);
        }
        ui.add_space(4.0);
        self.quip_found_ui(ui);
        let q = &mut self.quip;

        if cfg!(target_os = "linux") {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(format!("AppImage or tarball install? Register Links (or {} --register-links) makes this copy open the links.", crate::brand::SLUG)).small().color(dim));
                let resp = ui.small_button("Register Links");
                if theme::hint(resp, format!("Write a hidden launcher entry that opens {}:// links with this program", crate::brand::SLUG)).clicked() {
                    q.registered = Some(match crate::quip::register_links() {
                        Ok(m) => (sentence(&m), true),
                        Err(e) => (sentence(&e), false),
                    });
                }
            });
            if let Some((m, ok)) = &q.registered {
                ui.add(egui::Label::new(RichText::new(m).small().color(if *ok { dim } else { Tokens::get(ui.ctx()).error })).wrap());
            }
        } else {
            ui.label(RichText::new(format!("Links open {} once it's in Applications.", crate::brand::APP_NAME)).small().color(dim));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quip::tests::{controller, response, serve};

    /// Run Connect to the end, as the window would frame by frame.
    fn connect(q: &mut QuipSettings, c: &mut Controller) {
        q.connect(None);
        let deadline = Instant::now() + Duration::from_secs(10);
        while q.checking.is_some() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
            q.poll(c);
        }
        assert!(q.checking.is_none(), "the check finished");
    }

    #[test]
    fn connects_from_settings_and_clears_the_token() {
        let base = serve(|req, headers| {
            let ok = headers.iter().any(|h| h.eq_ignore_ascii_case("x-api-key: s3cret"));
            match (req, ok) {
                ("GET /api/config HTTP/1.1", true) => response("200 OK", "Content-Type: application/json\r\n", b"{}"),
                (_, false) => response("401 Unauthorized", "", b"{}"),
                _ => response("404 Not Found", "", b"{}"),
            }
        });
        let (mut c, _root) = controller();
        let mut q = QuipSettings::default();
        assert!(q.servers(&mut c).is_empty());

        // Bad input is caught before anything goes over the network.
        q.url = "ftp://quip.example".into();
        q.token = "s3cret".into();
        q.connect(None);
        assert!(q.checking.is_none() && q.error.as_deref().is_some_and(|e| e.starts_with(char::is_uppercase)), "{:?}", q.error);

        // A wrong token: refused inline, nothing stored, and the error doesn't echo it.
        q.url = format!("{base}/");
        q.token = "wrong-token".into();
        connect(&mut q, &mut c);
        let e = q.error.clone().unwrap_or_default();
        assert!(e.contains("refused the token") && !e.contains("wrong-token") && !e.contains("quip.connect"), "{e}");
        assert!(q.servers(&mut c).is_empty());
        assert_eq!(q.token, "wrong-token", "kept to correct and retry");

        // The right one: stored, listed, and the form is cleared.
        q.token = " s3cret ".into();
        connect(&mut q, &mut c);
        assert_eq!(q.error, None);
        assert_eq!((q.url.as_str(), q.token.as_str()), ("", ""));
        assert_eq!(q.servers(&mut c), vec![base.clone()]);
        assert_eq!(crate::quip::token_for(&c.store.data_dir, &base).unwrap().as_deref(), Some("s3cret"));
        assert!(!c.notice.as_ref().map(|n| n.text.clone()).unwrap_or_default().contains("s3cret"));

        c.execute("quip.disconnect", json!({"url": base})).unwrap();
        q.servers = None;
        assert!(q.servers(&mut c).is_empty());
    }
}
