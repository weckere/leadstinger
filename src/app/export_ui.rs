//! File ▸ Export Audio…: what to render (the selected tiles or the current set), as WAV
//! or FLAC at which rate, then a save dialog (one tile) or a folder picker (D-080). The
//! commands do the rest on a thread of their own (src/controller/export.rs).

use super::*;
use crate::render::Format;

/// The Export Audio window's choices while it's open.
pub(super) struct ExportDialog {
    /// The selected tiles when it opened; empty means only the set can be exported.
    tiles: Vec<Uuid>,
    set: Uuid,
    whole_set: bool,
    format: Format,
    /// `None`: each clip's own rate.
    rate: Option<u32>,
}

impl ExportDialog {
    pub(super) fn new(tiles: Vec<Uuid>, set: Uuid) -> Self {
        let whole_set = tiles.is_empty();
        ExportDialog { tiles, set, whole_set, format: Format::Wav, rate: None }
    }
}

/// The rates offered, besides the clip's own.
const RATES: [u32; 3] = [44_100, 48_000, 96_000];

fn rate_label(rate: Option<u32>) -> String {
    match rate {
        None => "Same as each clip".to_owned(),
        Some(r) => format!("{} kHz", r as f32 / 1000.0),
    }
}

/// What a tile export does, for the window.
const APPLIED: &str = "Trim, fades, volume, gain, speed, pitch and the channel mix are applied; a loop plays once. 24-bit stereo. The library isn't changed.";

impl StingerApp {
    pub(super) fn export_window(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.export.take() else { return };
        let t = Tokens::get(ctx);
        let lib = &self.ctl.store.library;
        let has_audio = |id: &Uuid| lib.tile(*id).is_some_and(|t| t.file.is_some() && t.action.is_none());
        let set = lib.set_index(d.set).map(|i| &lib.sets[i]);
        let set_name = set.map_or_else(String::new, |s| s.name.clone());
        let set_count = set.map_or(0, |s| s.tiles.iter().filter(|t| has_audio(&t.id)).count());
        let tile_count = d.tiles.iter().filter(|id| has_audio(id)).count();
        let one_title = d.tiles.first().and_then(|id| lib.tile(*id)).map(|t| t.title.clone()).unwrap_or_default();
        let mut open = true;
        let mut go = false;
        egui::Window::new("Export Audio").open(&mut open).default_pos(self.window_pos()).collapsible(false).resizable(false).default_width(380.0).show(ctx, |ui| {
            egui::Grid::new("export-grid").num_columns(2).spacing([14.0, 10.0]).show(ui, |ui| {
                ui.label("Export");
                ui.vertical(|ui| {
                    if !d.tiles.is_empty() {
                        let what = match d.tiles.len() {
                            1 => format!("“{one_title}”"),
                            n => format!("The {n} selected tiles"),
                        };
                        theme::hint(ui.radio_value(&mut d.whole_set, false, what), "Export only the tiles selected when this window opened");
                    }
                    theme::hint(ui.radio_value(&mut d.whole_set, true, format!("Every tile in “{set_name}” ({set_count})")), "Export every tile in the set that has audio, one file each");
                });
                ui.end_row();

                ui.label("Format");
                ui.horizontal(|ui| {
                    theme::hint(ui.radio_value(&mut d.format, Format::Wav, "WAV"), "Uncompressed 24-bit WAV, which every program opens");
                    theme::hint(ui.radio_value(&mut d.format, Format::Flac, "FLAC"), "Lossless FLAC: the same sound in about half the space");
                });
                ui.end_row();

                ui.label("Sample rate");
                let combo = egui::ComboBox::from_id_salt("export-rate").width(200.0).selected_text(rate_label(d.rate)).show_ui(ui, |ui| {
                    for r in std::iter::once(None).chain(RATES.map(Some)) {
                        ui.selectable_value(&mut d.rate, r, rate_label(r));
                    }
                });
                theme::hint(theme::name(combo.response, &format!("Sample rate, {}", rate_label(d.rate))), "The sample rate of the files: each clip's own, or one rate for all");
                ui.end_row();
            });
            ui.add_space(6.0);
            ui.add(egui::Label::new(RichText::new(APPLIED).small().color(t.text_dim)).wrap());
            ui.add_space(8.0);
            let count = if d.whole_set { set_count } else { tile_count };
            ui.horizontal(|ui| {
                let label = if !d.whole_set && d.tiles.len() == 1 { "Export…" } else { "Choose Folder…" };
                let tip = if label == "Export…" { "Choose where to save the file, then export it" } else { "Choose a folder, then export a file per tile into it" };
                go = theme::hint_or(ui.add_enabled(count > 0, egui::Button::new(label)), tip, "Nothing here has audio to export").clicked();
                if count > 0 && count < if d.whole_set { set.map_or(0, |s| s.tiles.len()) } else { d.tiles.len() } {
                    ui.label(RichText::new("Tiles without audio are skipped").small().color(t.text_dim));
                }
            });
        });
        if go {
            self.export_to(&d, &one_title);
        } else if open {
            self.export = Some(d);
        }
    }

    /// Ask where, then start the render.
    fn export_to(&mut self, d: &ExportDialog, title: &str) {
        let (format, rate) = (d.format.extension(), d.rate);
        if !d.whole_set && d.tiles.len() == 1 {
            let name = match crate::render::safe_name(title) {
                n if n.is_empty() => "Tile".to_owned(),
                n => n,
            };
            let file = rfd::FileDialog::new()
                .set_title("Export Audio")
                .set_file_name(format!("{name}.{format}"))
                .add_filter(d.format.label(), &[format])
                .save_file();
            if let Some(path) = file {
                self.run(inv!("tile.render", {"tile": d.tiles[0], "path": path, "format": format, "rate": rate}));
            }
            return;
        }
        let Some(folder) = rfd::FileDialog::new().set_title("Export Audio to a Folder").pick_folder() else { return };
        if d.whole_set {
            self.run(inv!("set.render", {"set": d.set, "folder": folder, "format": format, "rate": rate}));
        } else {
            self.run(inv!("tile.render", {"tiles": d.tiles, "path": folder, "format": format, "rate": rate}));
        }
    }
}
