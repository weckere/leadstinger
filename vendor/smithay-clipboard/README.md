# smithay-clipboard 0.7.3, with file drops

LeadStinger's copy of [smithay-clipboard](https://github.com/smithay/smithay-clipboard)
0.7.3 (MIT, see LICENSE), used through `[patch.crates-io]` in the top-level `Cargo.toml`
in place of the crates.io release that egui-winit depends on.

The only change is file drops on Wayland: upstream's drag-and-drop handlers were empty,
and they now accept `text/uri-list` drags and pass them to a handler set with
`smithay_clipboard::dnd::set_drop_handler` (`src/dnd.rs`, and the `DataDeviceHandler`
impl and `State::receive_drop` in `src/state.rs`). The seat's data device is also made
when the seat has a pointer, not only a keyboard, since a drop needs only a pointer.
Without a handler, drops are refused as upstream refuses them.

Why here and not in a second `wl_data_device`: Hyprland and Mutter send drag events to
only one of a client's data devices (Hyprland the first one created, which in an egui app
is this crate's), so a second device of our own would miss drops there. When egui moves to winit 0.31, which handles Wayland drops itself, this
copy can go.
