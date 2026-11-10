//! Several fingers on the board at once (iPadOS, docs/IPAD-PLAN.md). egui-winit turns the
//! first finger into the pointer (so one tile's press and release go through
//! `tile_interaction` as a mouse button does) and reports every finger as
//! `Event::Touch`. Each further finger that lands on a tile plays it here and releases it
//! when it lifts, so a Hold tile plays for as long as its finger is down.

use super::*;
use egui::{Event, TouchId, TouchPhase};
use std::collections::HashMap;

#[derive(Default)]
pub(super) struct Touches {
    /// The finger egui emulates the pointer with, as egui-winit tracks it: the first one
    /// down, until it lifts.
    pointer: Option<TouchId>,
    /// The other fingers that pressed a tile.
    held: HashMap<u64, Uuid>,
}

impl StingerApp {
    pub(super) fn more_fingers(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let events: Vec<(TouchId, TouchPhase, egui::Pos2)> = ctx.input(|i| {
            i.events.iter().filter_map(|e| if let Event::Touch { id, phase, pos, .. } = e { Some((*id, *phase, *pos)) } else { None }).collect()
        });
        for (id, phase, pos) in events {
            match phase {
                TouchPhase::Start => {
                    if self.touches.pointer.is_none() {
                        self.touches.pointer = Some(id);
                    } else if let Some((tile, ..)) = self.tile_rects.iter().find(|(_, _, r)| r.contains(pos)) {
                        self.touches.held.insert(id.0, *tile);
                        actions.push(inv!("tile.trigger", {"tile": tile, "event": "press"}).into());
                    }
                }
                TouchPhase::Move => {}
                TouchPhase::End | TouchPhase::Cancel => {
                    if self.touches.pointer == Some(id) {
                        self.touches.pointer = None;
                    }
                    if let Some(tile) = self.touches.held.remove(&id.0) {
                        actions.push(inv!("tile.trigger", {"tile": tile, "event": "release"}).into());
                    }
                }
            }
        }
        if !self.touches.held.is_empty() {
            ctx.request_repaint();
        }
    }
}
