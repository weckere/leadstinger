//! Where a file drag is over the window on macOS (t-m20-drop-target, D-126). winit 0.30
//! reports a Finder drag only as hovered and dropped files, with no position, and AppKit
//! sends no mouse moves while the drag lasts, so egui's pointer stays wherever it was
//! before and every drop missed its tile. AppKit still knows where the cursor is: each
//! frame while files are over the window, that position goes into egui's input as a
//! pointer move, so the board finds the tile under a drop and the Rows markers follow the
//! drag, as on Wayland (D-108).

use egui::{Event, Pos2, RawInput};
use std::time::Duration;

/// How often the window redraws while files are over it, so the markers keep up (winit
/// sends nothing to redraw for while the drag moves).
const FOLLOW: Duration = Duration::from_millis(33);

/// The window's view, to read the cursor position from.
pub struct MacDrops {
    #[cfg(target_os = "macos")]
    view: objc2::rc::Retained<objc2_app_kit::NSView>,
}

impl MacDrops {
    /// The window's view on macOS; `None` elsewhere, where drops carry their position.
    pub fn install(cc: &eframe::CreationContext<'_>) -> Option<Self> {
        #[cfg(target_os = "macos")]
        {
            use raw_window_handle::{HasWindowHandle, RawWindowHandle};
            let RawWindowHandle::AppKit(h) = cc.window_handle().ok()?.as_raw() else { return None };
            // SAFETY: winit's handle points at the window's live content view, and this
            // runs on the main thread, where the view is only ever used.
            let view = unsafe { objc2::rc::Retained::retain(h.ns_view.as_ptr().cast::<objc2_app_kit::NSView>()) }?;
            Some(MacDrops { view })
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = cc;
            None
        }
    }

    /// Put the cursor's position into this frame's input while files are over the window.
    pub fn apply(&self, ctx: &egui::Context, raw: &mut RawInput) {
        if !files(raw) {
            return;
        }
        follow(self.cursor(), ctx.zoom_factor(), raw);
        if !raw.hovered_files.is_empty() {
            ctx.request_repaint_after(FOLLOW);
        }
    }

    /// The cursor in the view, in logical pixels from its top left (winit's view is
    /// flipped, so this is how winit itself reports the mouse).
    #[cfg(target_os = "macos")]
    fn cursor(&self) -> Option<Pos2> {
        let window = self.view.window()?;
        let on_screen = objc2_app_kit::NSEvent::mouseLocation();
        let p = self.view.convertPoint_fromView(window.convertPointFromScreen(on_screen), None);
        Some(Pos2::new(p.x as f32, p.y as f32))
    }

    #[cfg(not(target_os = "macos"))]
    fn cursor(&self) -> Option<Pos2> {
        None
    }
}

/// Files are over the window, or were dropped on it this frame.
fn files(raw: &RawInput) -> bool {
    !raw.hovered_files.is_empty() || !raw.dropped_files.is_empty()
}

/// Move egui's pointer to `cursor` (logical pixels; `zoom` is egui's zoom factor) while
/// files are over the window. It goes first, so a real mouse move in the same frame wins.
pub fn follow(cursor: Option<Pos2>, zoom: f32, raw: &mut RawInput) {
    if let Some(at) = cursor.filter(|_| files(raw)) {
        raw.events.insert(0, Event::PointerMoved(at / zoom));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wayland_drop::DroppedPath;
    use std::sync::Arc;

    #[test]
    fn the_pointer_follows_a_file_drag_and_its_drop() {
        let at = Some(Pos2::new(300.0, 200.0));
        // No files: the pointer is left alone.
        let mut raw = RawInput::default();
        follow(at, 1.0, &mut raw);
        assert!(raw.events.is_empty());
        // Hovering, at 150 % zoom: points, ahead of the frame's other events.
        let mut raw = RawInput { hovered_files: vec![egui::HoveredFile::default()], events: vec![Event::PointerGone], ..Default::default() };
        follow(at, 1.5, &mut raw);
        assert_eq!(raw.events, [Event::PointerMoved(Pos2::new(200.0, 400.0 / 3.0)), Event::PointerGone]);
        // Dropped (winit clears the hovered files in the drop's frame).
        let mut raw = RawInput::default();
        raw.dropped_files.push(Arc::new(DroppedPath("/tmp/a.wav".into())));
        follow(at, 1.0, &mut raw);
        assert_eq!(raw.events, [Event::PointerMoved(Pos2::new(300.0, 200.0))]);
        // AppKit gave no position: nothing to add.
        let mut raw = RawInput { hovered_files: vec![egui::HoveredFile::default()], ..Default::default() };
        follow(None, 1.0, &mut raw);
        assert!(raw.events.is_empty());
    }
}
