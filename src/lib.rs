//! LeadStinger as a library (built as ST:TNGer with `--features sttnger`, src/brand.rs):
//! the model, audio, the command registry and Controller, the control socket and the egui
//! front end. Two binaries use it: `leadstinger` (the app, src/main.rs) and
//! `leadstinger-cli` (src/bin/leadstinger-cli.rs).

pub mod app;
pub mod audio;
pub mod brand;
pub mod commands;
pub mod control;
pub mod controller;
pub mod decode;
pub mod demo;
pub mod logic;
pub mod menu;
pub mod model;
#[cfg(target_os = "macos")]
pub mod native_menu;
pub mod paste;
pub mod picture;
pub mod starter;
pub mod store;
pub mod theme;
pub mod wav;
