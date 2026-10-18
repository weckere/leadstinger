//! `LEADSTINGER_DEMO=1`: fill an empty library with synthesized sounds, for trying the
//! app out and for screenshots.

use crate::model::{Layout, PlayMode, SoundSet, TileColor};
use crate::store::Store;
use crate::wav;

struct Sound {
    title: &'static str,
    seconds: f32,
    freqs: &'static [f32],
    color: TileColor,
    mode: PlayMode,
    looping: bool,
}

const fn s(title: &'static str, seconds: f32, freqs: &'static [f32], color: TileColor, mode: PlayMode, looping: bool) -> Sound {
    Sound { title, seconds, freqs, color, mode, looping }
}

const SHOW: &[Sound] = &[
    s("Intro Theme", 24.0, &[261.6, 329.6, 392.0], TileColor::Blue, PlayMode::Toggle, false),
    s("Applause", 6.0, &[180.0, 911.0, 1307.0], TileColor::Green, PlayMode::Toggle, false),
    s("Rimshot", 1.2, &[200.0, 1800.0], TileColor::Orange, PlayMode::Restart, false),
    s("Sad Trombone", 3.5, &[233.1, 220.0], TileColor::Purple, PlayMode::Toggle, false),
    s("Air Horn", 2.0, &[466.2, 587.3, 698.5], TileColor::Red, PlayMode::Hold, false),
    s("Crickets", 8.0, &[4100.0, 4300.0], TileColor::Mint, PlayMode::Toggle, true),
    s("Drum Roll", 5.0, &[90.0, 140.0], TileColor::Brown, PlayMode::Toggle, false),
    s("Ding!", 1.0, &[1318.5, 1975.5], TileColor::Yellow, PlayMode::Restart, false),
    s("Ambience Bed", 30.0, &[110.0, 164.8], TileColor::Teal, PlayMode::Pause, true),
    s("Wrong Answer", 1.5, &[150.0, 155.0], TileColor::Pink, PlayMode::Restart, false),
    s("Victory Fanfare", 7.0, &[392.0, 523.3, 659.3], TileColor::Indigo, PlayMode::Toggle, false),
    s("Outro", 20.0, &[196.0, 246.9, 293.7], TileColor::Cyan, PlayMode::Toggle, false),
];

const CUES: &[Sound] = &[
    s("Preshow Music", 40.0, &[220.0, 277.2, 329.6], TileColor::Blue, PlayMode::Toggle, false),
    s("House Lights Down", 4.0, &[146.8], TileColor::Gray, PlayMode::Toggle, false),
    s("Scene 1 Underscore", 25.0, &[174.6, 220.0], TileColor::Teal, PlayMode::Toggle, false),
    s("Door Slam", 0.8, &[80.0, 120.0], TileColor::Orange, PlayMode::Toggle, false),
    s("Phone Ring", 5.0, &[440.0, 480.0], TileColor::Green, PlayMode::Toggle, false),
    s("Intermission", 30.0, &[261.6, 311.1], TileColor::Purple, PlayMode::Toggle, false),
];

fn build_set(store: &mut Store, name: &str, sounds: &[Sound], layout: Layout, columns: u32) -> anyhow::Result<()> {
    let tmp = std::env::temp_dir().join(format!("leadstinger-demo-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp)?;
    let paths: Vec<_> = sounds
        .iter()
        .map(|snd| {
            let path = tmp.join(format!("{}.wav", snd.title));
            wav::write_tone(&path, 44_100, snd.seconds, snd.freqs, 0.25).map(|_| path)
        })
        .collect::<std::io::Result<_>>()?;
    let set = SoundSet { layout, columns, ..SoundSet::named(name) };
    store.library.sets.push(set);
    let si = store.library.sets.len() - 1;
    let (ids, _) = store.import(&paths, si, None);
    for (id, snd) in ids.iter().zip(sounds) {
        if let Some(t) = store.library.tile_mut(*id) {
            t.color = snd.color;
            t.mode = snd.mode;
            t.looping = snd.looping;
            t.fade_out = if snd.looping { 1.5 } else { 0.0 };
        }
    }
    std::fs::remove_dir_all(tmp).ok();
    Ok(())
}

/// Replace an empty library with two demo sets. Does nothing if any tile exists.
pub fn populate_if_empty(store: &mut Store) -> anyhow::Result<()> {
    if store.library.sets.iter().any(|s| !s.tiles.is_empty()) {
        return Ok(());
    }
    store.library.sets.clear();
    build_set(store, "Game Night", SHOW, Layout::Grid, 4)?;
    build_set(store, "Theatre Cues", CUES, Layout::List, 4)?;
    store.library.selected_set = Some(store.library.sets[0].id);
    store.mark_dirty();
    Ok(())
}
