//! iPadOS has no system-wide keys: the same surface as `src/hotkeys.rs`, always off.
use crate::bindings::Binding;
use crate::control::Lender;
use std::sync::Arc;

#[derive(Clone, Debug, Default)]
pub struct HotkeyStatus {
    pub available: bool,
    pub note: Option<String>,
    pub active: Vec<String>,
    pub failed: Vec<(String, String)>,
}

pub fn parse(keys: &str) -> Result<(), String> {
    Err(format!("{keys:?}: system-wide keys aren't available on iPadOS"))
}

pub fn id_of(_keys: &str) -> Option<u32> {
    None
}

pub struct Hotkeys;

impl Hotkeys {
    pub fn start(_lender: Arc<Lender>, _wake: Option<eframe::egui::Context>) -> Result<Hotkeys, String> {
        Err("System-wide keys aren't available on iPadOS".to_owned())
    }

    pub fn sync(&mut self, _bindings: &[Binding]) -> HotkeyStatus {
        HotkeyStatus::default()
    }
}
