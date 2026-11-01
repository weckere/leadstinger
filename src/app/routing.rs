//! The inspector's routing: where a tile plays (its sends, each at its own level) and,
//! for a file with more than two channels, its channel mix. Kept apart from the rest of
//! the inspector so the two change independently.

use super::*;
use crate::model::{tidy_sends, ChannelRoute, ChannelTo, TileSend};

/// A level: a compact dB field.
fn level(ui: &mut Ui, db: &mut f32, hint: &str) -> egui::Response {
    let resp = ui.add(
        egui::DragValue::new(db)
            .range(-GAIN_DB_MAX..=GAIN_DB_MAX)
            .speed(0.1)
            .fixed_decimals(1)
            .custom_formatter(|v, _| format!("{v:+.1} dB")),
    );
    theme::hint(resp, hint)
}

const SEND_LEVEL: &str = "This send's level, on top of the tile's own";

/// An output menu `width` wide; a longer name is cut short (the menu shows it whole).
fn output_combo(ui: &mut Ui, salt: &str, width: f32, current: &mut String, names: &[String], tip: &str) {
    ui.allocate_ui_with_layout(vec2(width, ui.spacing().interact_size.y), egui::Layout::left_to_right(Align::Center), |ui| {
        ui.set_max_width(width);
        let combo = egui::ComboBox::from_id_salt(salt).width(width).truncate().selected_text(current.as_str()).show_ui(ui, |ui| {
            for n in names {
                ui.selectable_value(current, n.clone(), n);
            }
        });
        theme::hint(theme::name(combo.response, tip), tip);
    });
}

/// Grid rows for the tile's Output (its first send) and its other sends, plus "Also
/// send to…". `outputs` are the library's outputs, Main not included.
pub(super) fn sends(ui: &mut Ui, t: &mut Tile, outputs: &[String]) {
    let mut names = vec![crate::model::MAIN_OUTPUT.to_owned()];
    names.extend(outputs.iter().cloned());
    let mut sends = t.routes();

    // Each send is two rows: its output's menu across the column, then its level (and
    // ✕), so a narrow inspector (D-211) still shows the output's name.
    ui.label("Output");
    let width = ui.available_width().min(200.0);
    output_combo(ui, "output", width, &mut sends[0].output, &names, "Where the tile plays");
    ui.end_row();
    ui.label("");
    level(ui, &mut sends[0].gain_db, SEND_LEVEL);
    ui.end_row();

    let mut remove = None;
    for (i, s) in sends.iter_mut().enumerate().skip(1) {
        ui.label(if i == 1 { "Also to" } else { "" });
        output_combo(ui, &format!("send{i}"), width, &mut s.output, &names, "Another output the tile also plays on");
        ui.end_row();
        ui.label("");
        ui.horizontal(|ui| {
            level(ui, &mut s.gain_db, SEND_LEVEL);
            if theme::hint(theme::name(theme::small_button(ui, "🗙"), &format!("Remove {}", s.output)), format!("Stop sending to {}", s.output)).clicked() {
                remove = Some(i);
            }
        });
        ui.end_row();
    }
    if let Some(i) = remove {
        sends.remove(i);
    }

    let free: Vec<&String> = names.iter().filter(|n| !sends.iter().any(|s| s.output.eq_ignore_ascii_case(n))).collect();
    if !free.is_empty() {
        ui.label("");
        let combo = egui::ComboBox::from_id_salt("send_add").width(width).truncate().selected_text("Also send to…").show_ui(ui, |ui| {
            for n in free {
                if ui.selectable_label(false, n.as_str()).clicked() {
                    sends.push(TileSend::to(n.clone()));
                }
            }
        });
        theme::hint(combo.response, "Play on another output too, at its own level (a headphone cue mix, a stream)");
        ui.end_row();
    }

    tidy_sends(&mut sends);
    t.sends = sends;
}

/// The Channels section, for a file with more than two channels (`labels`, with each
/// channel's `downmix` gains): Auto, the stereo downmix, or a route and level per channel.
pub(super) fn channels(ui: &mut Ui, t: &mut Tile, labels: &[String], downmix: &[(f32, f32)]) {
    // A mix made for a file with another channel count plays as Auto.
    let mut custom = t.channel_mix.len() == labels.len();
    ui.horizontal(|ui| {
        let auto = theme::hint(ui.selectable_label(!custom, "Auto"), "Downmix to stereo the ITU-R BS.775 way: centre and surrounds at −3 dB, LFE left out");
        let own = theme::hint(ui.selectable_label(custom, "Per channel"), "Choose where each channel goes, and its level");
        if auto.clicked() {
            t.channel_mix.clear();
            custom = false;
        }
        if own.clicked() && !custom {
            // Start from what Auto does, so switching changes nothing until you do.
            t.channel_mix = downmix.iter().map(|g| ChannelRoute::from_gains(*g)).collect();
            custom = true;
        }
        ui.weak(format!("{} channels", labels.len()));
    });
    if !custom {
        return;
    }
    egui::Grid::new("channel-mix").num_columns(3).spacing([12.0, 6.0]).show(ui, |ui| {
        for (i, (label, c)) in labels.iter().zip(t.channel_mix.iter_mut()).enumerate() {
            ui.label(label);
            let combo = egui::ComboBox::from_id_salt(("channel-to", i)).width(80.0).selected_text(c.to.label()).show_ui(ui, |ui| {
                for to in ChannelTo::ALL {
                    ui.selectable_value(&mut c.to, to, to.label());
                }
            });
            theme::hint(combo.response, format!("Where the {label} channel goes"));
            ui.add_enabled_ui(c.to != ChannelTo::Mute, |ui| {
                let resp = level(ui, &mut c.gain_db, "This channel's level");
                theme::hint_or(resp, "This channel's level", "The channel is muted");
            });
            ui.end_row();
        }
    });
}
