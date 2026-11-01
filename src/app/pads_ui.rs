//! Settings ▸ Pads and keys: pad pages and pad colours (src/pads.rs).

use super::*;

impl StingerApp {
    pub(super) fn pad_settings(&self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let midi = &self.ctl.store.library.midi;
        ui.horizontal(|ui| {
            let mut bank = midi.bank;
            let pages = ui.add_enabled(midi.auto_map.is_some(), egui::DragValue::new(&mut bank).range(0..=128).custom_formatter(|n, _| if n == 0.0 { "all".into() } else { format!("{n}") }));
            let paged = theme::hint_or(
                theme::name(pages, "Pads per page"),
                "Pads per page: bind pads to midi.pageUp and midi.pageDown to page through the set (leadstinger-cli midi.autoMap --bank 16)",
                "Turn on Notes play tiles first",
            )
            .changed();
            ui.label("pads per page");
            if paged {
                actions.push(inv!("midi.autoMap", {"bank": bank}).into());
            }
        });
        ui.horizontal(|ui| {
            ui.label("Pad colours");
            let current = midi.palette.as_ref().map_or("Lit/unlit".to_owned(), |p| p.name.clone());
            let combo = egui::ComboBox::from_id_salt("pad-colours").width(140.0).selected_text(&current).show_ui(ui, |ui| {
                if ui.selectable_label(midi.palette.is_none(), "Lit/unlit").clicked() {
                    actions.push(inv!("midi.feedback", {"colors": "off"}).into());
                }
                for p in crate::pads::builtin() {
                    if theme::hint(ui.selectable_label(current == p.name, &p.name), p.about.clone()).clicked() {
                        actions.push(inv!("midi.feedback", {"colors": p.name}).into());
                    }
                }
            });
            theme::hint(theme::name(combo.response, "Pad colours"), "Light each pad in its tile's colour, for controllers that read the velocity as a colour");
        });
    }
}
