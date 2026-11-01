//! System-wide keys on Wayland, through the desktop's GlobalShortcuts portal
//! (org.freedesktop.portal.GlobalShortcuts): KDE Plasma, GNOME 48+ and
//! xdg-desktop-portal-hyprland implement it. The app asks the desktop to bind each `key`
//! binding, suggesting its combination; the desktop decides (and may ask the user), then
//! says when a shortcut goes down and up.
//!
//! On Hyprland the portal's shortcuts are bound in the config, e.g.
//! `bind = CTRL ALT, 1, global, :<binding id>` (the ids are the bindings' ids, as
//! `leadstinger-cli bind.list` shows); binding keys straight to `leadstinger-cli` works
//! there too and needs no portal. Runs on its own thread with a small executor
//! (`pollster`), the same portal library the file dialogs already use.

use crate::bindings::{Binding, Source};
use crate::control::Lender;
use crate::hotkeys::HotkeyStatus;
use ashpd::desktop::global_shortcuts::{GlobalShortcuts, NewShortcut};
use futures_channel::mpsc::{unbounded, UnboundedSender};
use futures_util::{stream, StreamExt};
use std::sync::Arc;

/// The key bindings to ask for: (shortcut id = the binding's id, combination).
type Wanted = Vec<(String, String)>;

enum Msg {
    Rebind(Wanted),
    Key(String, bool),
}

pub struct PortalKeys {
    tx: UnboundedSender<Wanted>,
    last: Option<Wanted>,
}

/// A combination in the shortcuts spec's form: `CTRL+ALT+1`, `LOGO+F13`.
pub fn portal_trigger(keys: &str) -> String {
    keys.split('+')
        .map(|part| match part.trim().to_ascii_lowercase().as_str() {
            "ctrl" | "control" => "CTRL".to_owned(),
            "alt" | "option" => "ALT".to_owned(),
            "shift" => "SHIFT".to_owned(),
            "cmd" | "command" | "super" | "meta" | "logo" => "LOGO".to_owned(),
            _ => part.trim().to_owned(),
        })
        .collect::<Vec<_>>()
        .join("+")
}

impl PortalKeys {
    pub fn start(lender: Arc<Lender>, wake: Option<eframe::egui::Context>) -> PortalKeys {
        let (tx, rx) = unbounded::<Wanted>();
        std::thread::Builder::new()
            .name("portal-keys".into())
            .spawn(move || pollster::block_on(run(lender, wake, rx)))
            .ok();
        PortalKeys { tx, last: None }
    }

    /// Ask for the current key bindings, if they changed. Call it when the library does.
    pub fn sync(&mut self, bindings: &[Binding]) {
        let want: Wanted = bindings
            .iter()
            .filter(|b| b.enabled)
            .filter_map(|b| match &b.source {
                Source::Key(k) => Some((b.id.to_string(), k.clone())),
                _ => None,
            })
            .collect();
        if self.last.as_ref() != Some(&want) {
            self.tx.unbounded_send(want.clone()).ok();
            self.last = Some(want);
        }
    }
}

async fn run(lender: Arc<Lender>, wake: Option<eframe::egui::Context>, rx: futures_channel::mpsc::UnboundedReceiver<Wanted>) {
    let status = |available: bool, note: String, active: Vec<String>| {
        lender.with(|c| c.hotkey_status = HotkeyStatus { available, note: Some(note), active, failed: Vec::new() });
    };
    let proxy = match GlobalShortcuts::new().await {
        Ok(p) => p,
        Err(e) => {
            status(
                false,
                format!("This desktop has no global-shortcuts portal ({e}); bind keys in the compositor to {} instead", crate::brand::CLI_NAME),
                Vec::new(),
            );
            return;
        }
    };
    let (Ok(acts), Ok(deacts)) = (proxy.receive_activated().await, proxy.receive_deactivated().await) else {
        status(false, "The global-shortcuts portal didn't answer".into(), Vec::new());
        return;
    };
    status(true, "System-wide keys come from the desktop's global shortcuts".into(), Vec::new());
    let keys = stream::select(
        acts.map(|a| Msg::Key(a.shortcut_id().to_owned(), true)),
        deacts.map(|d| Msg::Key(d.shortcut_id().to_owned(), false)),
    );
    let mut all = Box::pin(stream::select(keys, rx.map(Msg::Rebind)));
    let mut session = None;
    while let Some(msg) = all.next().await {
        match msg {
            Msg::Key(id, pressed) => {
                lender.with(|c| c.portal_key(&id, pressed));
                if let Some(ctx) = &wake {
                    ctx.request_repaint();
                }
            }
            Msg::Rebind(wanted) => {
                // A session binds once: start a fresh one for the new set of keys.
                if let Some(old) = session.take() {
                    let old: ashpd::desktop::Session<'_, GlobalShortcuts<'_>> = old;
                    old.close().await.ok();
                }
                if wanted.is_empty() {
                    status(true, "System-wide keys come from the desktop's global shortcuts (none bound yet)".into(), Vec::new());
                    continue;
                }
                let new = match proxy.create_session().await {
                    Ok(s) => s,
                    Err(e) => {
                        status(true, format!("Couldn't start a global-shortcuts session: {e}"), Vec::new());
                        continue;
                    }
                };
                let triggers: Vec<String> = wanted.iter().map(|(_, k)| portal_trigger(k)).collect();
                let shortcuts: Vec<NewShortcut> = wanted
                    .iter()
                    .zip(&triggers)
                    .map(|((id, k), t)| NewShortcut::new(id.clone(), format!("{} {k}", crate::brand::APP_NAME)).preferred_trigger(t.as_str()))
                    .collect();
                match proxy.bind_shortcuts(&new, &shortcuts, None).await.and_then(|r| r.response()) {
                    Ok(bound) => {
                        let active = bound.shortcuts().iter().map(|s| format!("{} ({})", s.trigger_description(), s.description())).collect();
                        status(true, "System-wide keys come from the desktop's global shortcuts".into(), active);
                    }
                    Err(e) => status(true, format!("The desktop didn't bind the keys: {e}"), Vec::new()),
                }
                session = Some(new);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triggers_in_the_spec_form() {
        assert_eq!(portal_trigger("Ctrl+Alt+1"), "CTRL+ALT+1");
        assert_eq!(portal_trigger("Cmd+Shift+K"), "LOGO+SHIFT+K");
        assert_eq!(portal_trigger("F13"), "F13");
    }
}
