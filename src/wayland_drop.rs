//! File drops on native Wayland (D-108). winit 0.30 has none there, so LeadStinger's
//! copy of smithay-clipboard (vendor/smithay-clipboard), the clipboard egui already runs
//! on Wayland, accepts them on egui's own data device and hands them over. Here they
//! become the input an X11 or macOS drop gives egui (`hovered_files`, `dropped_files`
//! and the pointer position), so the board handles them exactly as it handles those.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use egui::{Event, HoveredFile, Pos2, RawInput};

/// A step of a file drag over the window, at a surface-local position in logical
/// pixels (before egui's zoom).
#[derive(Debug, Clone, PartialEq)]
pub enum DragEvent {
    Enter(Pos2),
    Motion(Pos2),
    Leave,
    Drop(Pos2, Vec<PathBuf>),
}

/// Drag events from the clipboard thread, turned into egui input frame by frame.
#[derive(Default)]
pub struct WaylandDrops {
    queue: Arc<Mutex<Vec<DragEvent>>>,
    hovering: bool,
}

impl WaylandDrops {
    /// Start taking file drops when the window is on Wayland (egui's clipboard is
    /// smithay-clipboard only there); `None` elsewhere, where winit delivers drops.
    pub fn install(cc: &eframe::CreationContext<'_>) -> Option<Self> {
        #[cfg(target_os = "linux")]
        {
            use winit::raw_window_handle::{HasDisplayHandle, RawDisplayHandle};
            let wayland = cc.display_handle().is_ok_and(|h| matches!(h.as_raw(), RawDisplayHandle::Wayland(_)));
            if !wayland {
                return None;
            }
            let drops = Self::default();
            let (queue, ctx) = (drops.queue.clone(), cc.egui_ctx.clone());
            // Only drags over the main window: these drops all go to its input, and a set
            // window's surface has positions of its own (D-227). Unknown: take them all.
            let main = {
                use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
                cc.window_handle().ok().and_then(|h| match h.as_raw() {
                    RawWindowHandle::Wayland(w) => Some(w.surface.as_ptr() as usize),
                    _ => None,
                })
            };
            let over_main = std::sync::atomic::AtomicBool::new(true);
            let set = smithay_clipboard::dnd::set_drop_handler(move |e| {
                use smithay_clipboard::dnd::DropEvent;
                let at = |x: f64, y: f64| Pos2::new(x as f32, y as f32);
                use std::sync::atomic::Ordering::Relaxed;
                if let DropEvent::Enter { surface, .. } = &e {
                    over_main.store(main.is_none_or(|m| m == *surface), Relaxed);
                }
                if !over_main.load(Relaxed) {
                    return;
                }
                let e = match e {
                    DropEvent::Enter { x, y, .. } => DragEvent::Enter(at(x, y)),
                    DropEvent::Motion { x, y } => DragEvent::Motion(at(x, y)),
                    DropEvent::Leave => DragEvent::Leave,
                    DropEvent::Drop { x, y, uri_list } => DragEvent::Drop(at(x, y), crate::paste::parse_uri_list(&uri_list)),
                };
                queue.lock().unwrap_or_else(|p| p.into_inner()).push(e);
                ctx.request_repaint();
            });
            set.then_some(drops)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = cc;
            None
        }
    }

    /// Add the drag events that arrived since the last frame to this frame's input.
    /// `zoom` is egui's zoom factor, which turns logical pixels into points.
    pub fn apply(&mut self, zoom: f32, raw: &mut RawInput) {
        let events = std::mem::take(&mut *self.queue.lock().unwrap_or_else(|p| p.into_inner()));
        for e in events {
            match e {
                // The compositor holds the pointer during a drag, so its position comes
                // from here: the board finds the tile under it as for any drop.
                DragEvent::Enter(at) | DragEvent::Motion(at) => {
                    self.hovering = true;
                    raw.events.push(Event::PointerMoved(at / zoom));
                }
                DragEvent::Leave => {
                    self.hovering = false;
                    raw.events.push(Event::PointerGone);
                }
                DragEvent::Drop(at, paths) => {
                    self.hovering = false;
                    raw.events.push(Event::PointerMoved(at / zoom));
                    raw.dropped_files.extend(paths.into_iter().map(|p| Arc::new(DroppedPath(p)) as egui::DroppedFileHandle));
                }
            }
        }
        // Which files are coming is only known once they're dropped.
        if self.hovering {
            raw.hovered_files = vec![HoveredFile { path: None, mime: "text/uri-list".into() }];
        }
    }

    #[cfg(test)]
    fn push(&self, e: DragEvent) {
        self.queue.lock().unwrap().push(e);
    }
}

/// A dropped file, as egui-winit's own (private) one.
#[derive(Debug)]
pub(crate) struct DroppedPath(pub(crate) PathBuf);

impl egui::DroppedFile for DroppedPath {
    fn path(&self) -> &Path {
        &self.0
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(d: &mut WaylandDrops, zoom: f32) -> RawInput {
        let mut raw = RawInput::default();
        d.apply(zoom, &mut raw);
        raw
    }

    #[test]
    fn a_drag_hovers_until_it_leaves() {
        let mut d = WaylandDrops::default();
        assert!(frame(&mut d, 1.0).hovered_files.is_empty());
        d.push(DragEvent::Enter(Pos2::new(10.0, 20.0)));
        let raw = frame(&mut d, 1.0);
        assert_eq!(raw.hovered_files.len(), 1);
        assert_eq!(raw.events, vec![Event::PointerMoved(Pos2::new(10.0, 20.0))]);
        // No new events: still over the window.
        let raw = frame(&mut d, 1.0);
        assert_eq!(raw.hovered_files.len(), 1);
        assert!(raw.events.is_empty());
        d.push(DragEvent::Motion(Pos2::new(30.0, 40.0)));
        d.push(DragEvent::Leave);
        let raw = frame(&mut d, 1.0);
        assert!(raw.hovered_files.is_empty());
        assert_eq!(raw.events, vec![Event::PointerMoved(Pos2::new(30.0, 40.0)), Event::PointerGone]);
        assert!(raw.dropped_files.is_empty());
    }

    #[test]
    fn a_drop_gives_its_files_at_the_drop_position_in_points() {
        let mut d = WaylandDrops::default();
        d.push(DragEvent::Enter(Pos2::new(100.0, 50.0)));
        frame(&mut d, 2.0);
        let files = crate::paste::parse_uri_list("file:///tmp/air%20horn.flac\r\nfile:///tmp/set.lsset\r\n");
        d.push(DragEvent::Drop(Pos2::new(300.0, 150.0), files));
        let raw = frame(&mut d, 2.0);
        assert!(raw.hovered_files.is_empty());
        assert_eq!(raw.events, vec![Event::PointerMoved(Pos2::new(150.0, 75.0))]);
        let paths: Vec<&Path> = raw.dropped_files.iter().map(|f| f.path()).collect();
        assert_eq!(paths, vec![Path::new("/tmp/air horn.flac"), Path::new("/tmp/set.lsset")]);
        assert!(frame(&mut d, 2.0).dropped_files.is_empty());
    }
}
