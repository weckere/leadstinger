//! MIDI pad pages and pad colours (D-058).
//!
//! Pages: with `midi.autoMap --bank N`, the N notes from the auto-map base play a page of
//! N tiles in the current set, and `midi.page` (or `midi.pageUp` / `midi.pageDown` on a
//! pad) moves through the set N tiles at a time. The page is the running app's, starts
//! at the first one, and goes back to it when another set is chosen.
//!
//! Colours: many pad controllers read a note's velocity as a colour, and pulse or flash
//! it when it arrives on another channel. A palette maps each tile colour to the velocity
//! (and channel) for a pad whose tile is idle and one whose tile plays. The built-in
//! palettes are data (src/pad_colors.json); a library can hold its own table instead.
//! They follow the makers' published MIDI documents and haven't been tried on every
//! controller, so a table of your own is the way to fix one.

use crate::model::TileColor;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Tile colour → pad velocity, for one kind of controller.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Palette {
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub about: String,
    /// The velocity for each colour while its tile is idle.
    pub idle: HashMap<TileColor, u8>,
    /// While it plays; a colour missing here keeps its idle velocity.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub playing: HashMap<TileColor, u8>,
    /// The channel (1-16) idle colours go on; otherwise the pad's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idle_channel: Option<u8>,
    /// The channel (1-16) playing colours go on, where pads pulse or flash.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub playing_channel: Option<u8>,
    /// The velocity for a pad with no tile (past the end of the set).
    #[serde(default)]
    pub empty: u8,
}

impl Palette {
    /// What a pad on `channel` shows for a tile of `color`: (channel, velocity).
    pub fn light(&self, color: TileColor, playing: bool, channel: u8) -> (u8, u8) {
        let idle = self.idle.get(&color).copied().unwrap_or(self.empty);
        let idle_ch = self.idle_channel.unwrap_or(channel);
        if playing {
            (self.playing_channel.unwrap_or(idle_ch), self.playing.get(&color).copied().unwrap_or(idle))
        } else {
            (idle_ch, idle)
        }
    }

    /// A table of one's own must name every velocity and channel in range.
    pub fn validate(&self) -> Result<(), String> {
        if self.idle.is_empty() {
            return Err("a palette needs an idle velocity for at least one colour".into());
        }
        let vel_ok = self.idle.values().chain(self.playing.values()).chain([&self.empty]).all(|v| *v <= 127);
        let ch_ok = [self.idle_channel, self.playing_channel].iter().flatten().all(|c| (1..=16).contains(c));
        match (vel_ok, ch_ok) {
            (true, true) => Ok(()),
            (false, _) => Err("palette velocities are 0-127".into()),
            (_, false) => Err("palette channels are 1-16".into()),
        }
    }
}

/// The palettes that come with the app.
pub fn builtin() -> Vec<Palette> {
    serde_json::from_str(include_str!("pad_colors.json")).expect("src/pad_colors.json is valid")
}

pub fn find(name: &str) -> Option<Palette> {
    builtin().into_iter().find(|p| p.name.eq_ignore_ascii_case(name))
}

/// How many pages of `bank` pads a set of `tiles` takes (at least one).
pub fn pages(tiles: usize, bank: u8) -> usize {
    if bank == 0 {
        1
    } else {
        tiles.div_ceil(bank as usize).max(1)
    }
}

/// The page the pads show, kept per set: choosing another set starts at its first page.
#[derive(Clone, Debug, Default)]
pub struct Paging {
    set: Option<Uuid>,
    page: usize,
}

impl Paging {
    /// The current page (0-based) for `set`, within its `pages`.
    pub fn page(&self, set: Uuid, pages: usize) -> usize {
        if self.set == Some(set) {
            self.page.min(pages.saturating_sub(1))
        } else {
            0
        }
    }

    pub fn set_page(&mut self, set: Uuid, page: usize, pages: usize) -> usize {
        self.set = Some(set);
        self.page = page.min(pages.saturating_sub(1));
        self.page
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_palettes_cover_every_colour() {
        let all = builtin();
        assert!(all.len() >= 3);
        for p in &all {
            p.validate().unwrap();
            for c in TileColor::ALL {
                assert!(p.idle.contains_key(&c), "{} has no idle velocity for {c:?}", p.name);
            }
        }
        let lp = find("Launchpad").unwrap();
        assert_eq!(lp.light(TileColor::Red, false, 1), (1, 5));
        assert_eq!(lp.light(TileColor::Red, true, 1), (3, 5), "pulses on channel 3");
        let apc = find("apc-mini").unwrap();
        assert_eq!(apc.light(TileColor::Green, false, 1), (1, 1));
        assert_eq!(apc.light(TileColor::Green, true, 1), (1, 2), "blinks");
        assert!(find("nope").is_none());
    }

    #[test]
    fn pages_clamp_and_reset_with_the_set() {
        assert_eq!((pages(0, 8), pages(8, 8), pages(9, 8), pages(20, 0)), (1, 1, 2, 1));
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        let mut p = Paging::default();
        assert_eq!(p.set_page(a, 5, 3), 2);
        assert_eq!(p.page(a, 3), 2);
        assert_eq!(p.page(a, 2), 1, "the set shrank");
        assert_eq!(p.page(b, 3), 0, "another set starts at its first page");
    }
}
