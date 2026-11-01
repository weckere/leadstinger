//! Background mode (t-m22-background-dev-windows, D-214): `LEADSTINGER_BACKGROUND=1`, and
//! every `LEADSTINGER_SNAPSHOT` run, opens the app out of the way of whoever is using the
//! computer, for screenshots, scripted checks and the CLI.
//!
//! On macOS the app never activates (no focus taken, no Dock icon, the menu bar left to
//! the app in front) and its window is fully transparent and lets clicks through, so it
//! is on screen for the system, which keeps painting it, but nobody sees it. A separate
//! Space would be tidier, but no public API puts a window on one. The window still paints
//! every frame it is asked for while covered, so screenshots are fresh.
//!
//! Linux has no portable way to open a window without focus (winit's `with_active` is
//! unsupported on X11 and Wayland, and the compositor decides), so there it only keeps
//! painting while covered.

/// Whether this run is in the background: `LEADSTINGER_BACKGROUND` set to anything but
/// `0`/`false`/`no`, or, when it isn't set, a `LEADSTINGER_SNAPSHOT` run.
pub fn on() -> bool {
    let background = crate::brand::env("BACKGROUND").map(|v| v.to_string_lossy().into_owned());
    wanted(background.as_deref(), crate::brand::env("SNAPSHOT").is_some())
}

fn wanted(background: Option<&str>, snapshot: bool) -> bool {
    match background {
        Some("0" | "false" | "no") => false,
        Some(_) => true,
        None => snapshot,
    }
}

/// The event loop for a background run: macOS starts the app as an accessory (no Dock
/// icon or menu bar) and doesn't activate it.
#[cfg(target_os = "macos")]
pub fn event_loop(b: &mut winit::event_loop::EventLoopBuilder<eframe::UserEvent>) {
    use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
    b.with_activation_policy(ActivationPolicy::Accessory).with_activate_ignoring_other_apps(false);
}

/// Make the (still hidden) window invisible to people and to the mouse before eframe
/// shows it after the first frame. Elsewhere, nothing.
pub fn hide_window(cc: &eframe::CreationContext<'_>) {
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::{NSView, NSWindowCollectionBehavior};
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        let Ok(handle) = cc.window_handle() else { return };
        let RawWindowHandle::AppKit(h) = handle.as_raw() else { return };
        // SAFETY: winit's handle points at the window's live content view, and this runs
        // on the main thread.
        let Some(view) = (unsafe { objc2::rc::Retained::retain(h.ns_view.as_ptr().cast::<NSView>()) }) else { return };
        let Some(window) = view.window() else { return };
        window.setAlphaValue(0.0);
        window.setIgnoresMouseEvents(true);
        window.setHasShadow(false);
        // Not in Mission Control's window cycling or ⌘`.
        window.setCollectionBehavior(NSWindowCollectionBehavior::Transient | NSWindowCollectionBehavior::IgnoresCycle);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = cc;
}

#[cfg(test)]
mod tests {
    use super::wanted;

    #[test]
    fn a_snapshot_is_in_the_background_unless_told_otherwise() {
        assert!(!wanted(None, false));
        assert!(wanted(Some("1"), false));
        assert!(wanted(None, true));
        assert!(!wanted(Some("0"), true));
        assert!(!wanted(Some("no"), false));
    }
}
