//! The inspector's MESSAGES section: a tile's OSC and MIDI messages on start and on stop
//! (src/messages.rs). One message per line; a list is applied when the field loses focus, and
//! only if every line reads.

use super::*;

impl StingerApp {
    pub(super) fn messages_editor(&self, ui: &mut Ui, t: &Tile, actions: &mut Vec<Action>) {
        for (key, label, list) in [("onStart", "On start", &t.on_start), ("onStop", "On stop", &t.on_stop)] {
            let current = list.join("\n");
            let draft_id = egui::Id::unique(("messages-draft", t.id, key));
            let mut draft = ui.data(|d| d.get_temp::<String>(draft_id)).unwrap_or_else(|| current.clone());
            ui.label(label);
            let field = theme::text_field(
                ui,
                egui::TextEdit::multiline(&mut draft)
                    .desired_rows(1)
                    .desired_width(f32::INFINITY)
                    .font(egui::TextStyle::Monospace)
                    .hint_text("osc://127.0.0.1:53000/cue/1/go   or   midi:note:60:ch1"),
            );
            let resp = theme::hint(
                theme::name(field, &format!("Messages {}", label.to_lowercase())),
                "OSC and MIDI messages, one per line, sent when the tile starts or stops:\n\
                 osc://host:port/address 1 0.5 \"text\"\n\
                 midi:note:60:ch1:vel100 @Port\n\
                 midi:noteoff:60  midi:cc:20:val127  midi:program:5\n\
                 OSC goes to this machine only unless OSC may use the network (Settings).",
            );
            let lines: Vec<String> = draft.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_owned).collect();
            let problem = lines.iter().find_map(|l| crate::messages::parse(l).err());
            if let Some(e) = &problem {
                ui.colored_label(Tokens::get(ui.ctx()).error, RichText::new(e).small());
            }
            if resp.has_focus() {
                ui.data_mut(|d| d.insert_temp(draft_id, draft));
            } else {
                if resp.lost_focus() && problem.is_none() && lines != *list {
                    let mut params = serde_json::Map::new();
                    params.insert("tile".into(), json!(t.id));
                    params.insert(key.into(), json!(lines));
                    actions.push(Invocation::new("tile.set", Value::Object(params)).into());
                }
                // Not editing: follow the tile (the CLI may change it), unless a bad
                // list is still showing its error.
                if problem.is_some() {
                    ui.data_mut(|d| d.insert_temp(draft_id, draft));
                } else {
                    ui.data_mut(|d| d.remove::<String>(draft_id));
                }
            }
        }
    }
}
