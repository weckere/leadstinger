//! File drops (a LeadStinger addition to smithay-clipboard).
//!
//! Hyprland and Mutter send drag-and-drop events to only one of a client's
//! `wl_data_device`s (Hyprland the first one created, which in an egui app is this
//! crate's clipboard device). So drops are handled here, on the one device the app
//! has, and handed to the application through [`set_drop_handler`].
//!
//! Only `text/uri-list` offers are accepted, and only once a handler is set:
//! without one the crate behaves exactly as upstream (drops are refused).

use std::sync::OnceLock;

/// The MIME type of a list of dropped files (RFC 2483).
pub const URI_LIST: &str = "text/uri-list";

/// A step of a file drag over one of the application's surfaces. Positions are
/// surface-local, in logical pixels.
#[derive(Debug, Clone, PartialEq)]
pub enum DropEvent {
    /// A drag offering files entered `surface` (its `wl_surface` pointer).
    Enter { surface: usize, x: f64, y: f64 },
    /// The drag moved.
    Motion { x: f64, y: f64 },
    /// The drag left without dropping, or was cancelled.
    Leave,
    /// The files were dropped: `uri_list` is the `text/uri-list` the source sent.
    Drop { x: f64, y: f64, uri_list: String },
}

type Handler = Box<dyn Fn(DropEvent) + Send + Sync>;

static HANDLER: OnceLock<Handler> = OnceLock::new();

/// Accept file drops and pass them to `handler`, which runs on the clipboard
/// thread. Only the first handler set is kept; returns whether this one was.
pub fn set_drop_handler(handler: impl Fn(DropEvent) + Send + Sync + 'static) -> bool {
    HANDLER.set(Box::new(handler)).is_ok()
}

pub(crate) fn enabled() -> bool {
    HANDLER.get().is_some()
}

pub(crate) fn emit(event: DropEvent) {
    if let Some(handler) = HANDLER.get() {
        handler(event);
    }
}

/// Whether an offer's MIME types include a file list.
pub(crate) fn offers_files(mime_types: &[String]) -> bool {
    mime_types.iter().any(|m| m == URI_LIST)
}
