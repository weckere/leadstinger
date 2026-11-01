//! LeadStinger as a library: the model, audio, the command registry and Controller, the
//! control socket and the egui front end. Two binaries use it: `leadstinger` (the app, src/main.rs) and
//! `leadstinger-cli` (src/bin/leadstinger-cli.rs).
// The tile's JSON (controller.rs `tile_json`) has more fields than `json!`'s default
// recursion limit can expand.
#![recursion_limit = "256"]

pub mod app;
pub mod audio;
pub mod background;
pub mod bindings;
pub mod brand;
pub mod commands;
pub mod control;
pub mod controller;
pub mod decode;
pub mod demo;
pub mod hotkeys;
pub mod input;
pub mod launcher;
pub mod level;
pub mod logic;
pub mod mac_drop;
pub mod menu;
pub mod messages;
pub mod midi;
pub mod model;
#[cfg(target_os = "macos")]
pub mod native_menu;
pub mod osc;
pub mod pads;
pub mod paste;
pub mod picture;
pub mod pitch;
#[cfg(target_os = "linux")]
pub mod portal;
pub mod render;
pub mod rows;
pub mod quip;
pub mod setfile;
pub mod speech;
pub mod starter;
pub mod store;
pub mod theme;
pub mod themes;
pub mod transcode;
pub mod update;
pub mod wav;
pub mod wayland_drop;
pub mod waveform;
