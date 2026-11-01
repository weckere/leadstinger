//! System-wide keys: `key` bindings (D-041) fire while another app has focus, through
//! the `global-hotkey` crate: Carbon hotkeys on macOS (no Accessibility permission) and
//! XGrabKey in an X11 session.
//!
//! On Wayland an app can't grab keys for itself, so this stays off there and the
//! compositor's own binds do the job (`bind = SUPER, F1, exec, leadstinger-cli trigger
//! key:1` on Hyprland), as the README shows. The window owns the manager: macOS wants
//! hotkeys registered on the main thread. Key events go to a dispatcher thread, which
//! runs the bindings on the Controller through the [`Lender`].

use crate::bindings::{Binding, Source};
use crate::control::Lender;
use global_hotkey::hotkey::{Code, HotKey};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::mpsc::channel;
use std::sync::Arc;

/// What the window last registered, for `hotkey.status` and Settings.
#[derive(Clone, Debug, Default)]
pub struct HotkeyStatus {
    pub available: bool,
    /// Why system-wide keys are off, or a hint.
    pub note: Option<String>,
    pub active: Vec<String>,
    /// Keys that couldn't be registered (another app has them, or they don't parse).
    pub failed: Vec<(String, String)>,
}

/// Parse a key combination such as `Ctrl+Alt+1`, `Cmd+Shift+K` or `F13`. A plain letter,
/// digit or symbol needs a modifier: grabbing it system-wide would take it away from
/// every other app.
pub fn parse(keys: &str) -> Result<HotKey, String> {
    let hk = HotKey::from_str(keys.trim()).map_err(|e| format!("{keys:?}: {e}"))?;
    let function_key = matches!(
        hk.key,
        Code::F1 | Code::F2 | Code::F3 | Code::F4 | Code::F5 | Code::F6 | Code::F7 | Code::F8 | Code::F9 | Code::F10
            | Code::F11 | Code::F12 | Code::F13 | Code::F14 | Code::F15 | Code::F16 | Code::F17 | Code::F18 | Code::F19
            | Code::F20 | Code::F21 | Code::F22 | Code::F23 | Code::F24 | Code::MediaPlayPause | Code::MediaStop
            | Code::MediaTrackNext | Code::MediaTrackPrevious | Code::Pause | Code::ScrollLock
    );
    if hk.mods.is_empty() && !function_key {
        return Err(format!("{keys:?} needs a modifier (Ctrl, Alt, Shift or Cmd/Super); only F-keys and media keys can stand alone"));
    }
    Ok(hk)
}

/// The system-wide id of a key binding's combination, if it parses.
pub fn id_of(keys: &str) -> Option<u32> {
    parse(keys).ok().map(|h| h.id())
}

pub struct Hotkeys {
    manager: GlobalHotKeyManager,
    registered: HashMap<u32, (HotKey, String)>,
}

impl Hotkeys {
    /// Start listening, or say why system-wide keys aren't available here.
    pub fn start(lender: Arc<Lender>, wake: Option<eframe::egui::Context>) -> Result<Hotkeys, String> {
        if cfg!(target_os = "linux") && std::env::var_os("WAYLAND_DISPLAY").is_some() {
            return Err("Asking the desktop's global-shortcuts portal for keys; on Hyprland, binding keys to \
                        leadstinger-cli in the config works too (see the README)."
                .replace("leadstinger-cli", crate::brand::CLI_NAME));
        }
        let manager = GlobalHotKeyManager::new().map_err(|e| format!("System-wide keys are unavailable: {e}"))?;
        let (tx, rx) = channel::<(u32, bool)>();
        // The handler may run on the main thread between frames; hand off at once.
        GlobalHotKeyEvent::set_event_handler(Some(move |e: GlobalHotKeyEvent| {
            tx.send((e.id, e.state == HotKeyState::Pressed)).ok();
        }));
        std::thread::Builder::new()
            .name("hotkeys".into())
            .spawn(move || {
                for (id, pressed) in rx {
                    lender.with(|c| c.hotkey(id, pressed));
                    if let Some(ctx) = &wake {
                        ctx.request_repaint();
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Hotkeys { manager, registered: HashMap::new() })
    }

    /// Register the enabled key bindings and drop the rest. Call it from the main thread
    /// when the bindings change.
    pub fn sync(&mut self, bindings: &[Binding]) -> HotkeyStatus {
        let mut failed = Vec::new();
        let mut wanted: HashMap<u32, (HotKey, String)> = HashMap::new();
        for b in bindings.iter().filter(|b| b.enabled) {
            if let Source::Key(k) = &b.source {
                match parse(k) {
                    Ok(hk) => {
                        wanted.insert(hk.id(), (hk, k.clone()));
                    }
                    Err(e) => failed.push((k.clone(), e)),
                }
            }
        }
        let stale: Vec<u32> = self.registered.keys().filter(|id| !wanted.contains_key(id)).copied().collect();
        for id in stale {
            if let Some((hk, _)) = self.registered.remove(&id) {
                self.manager.unregister(hk).ok();
            }
        }
        for (id, (hk, k)) in wanted {
            if self.registered.contains_key(&id) {
                continue;
            }
            match self.manager.register(hk) {
                Ok(()) => {
                    self.registered.insert(id, (hk, k));
                }
                Err(e) => failed.push((k, format!("{e}"))),
            }
        }
        let mut active: Vec<String> = self.registered.values().map(|(_, k)| k.clone()).collect();
        active.sort();
        HotkeyStatus { available: true, note: None, active, failed }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_key_combinations() {
        assert!(parse("Ctrl+Alt+1").is_ok());
        assert!(parse("cmd+shift+k").is_ok());
        assert!(parse("F13").is_ok(), "F-keys can stand alone");
        assert!(parse("Q").is_err(), "a bare letter would be taken from every app");
        assert!(parse("Ctrl+Nope").is_err());
        assert_eq!(id_of("Ctrl+Alt+1"), id_of("alt+ctrl+Digit1"), "same combination, same id");
        assert_ne!(id_of("Ctrl+Alt+1"), id_of("Ctrl+Alt+2"));
    }
}
