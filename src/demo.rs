//! `LEADSTINGER_DEMO=1`: fill an empty library with synthesized sounds, for trying the
//! app out and for screenshots.

use crate::model::{Layout, PlayMode, PressOptions, SoundSet, TileColor};
use crate::store::Store;
use crate::wav;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

mod synth;

use synth::Kind;

struct Sound {
    title: &'static str,
    seconds: f64,
    kind: Kind,
    color: TileColor,
    mode: PlayMode,
    looping: bool,
}

const fn s(title: &'static str, seconds: f64, kind: Kind, color: TileColor, mode: PlayMode, looping: bool) -> Sound {
    Sound { title, seconds, kind, color, mode, looping }
}

// Chords as semitones above A4, one a bar.
const BRIGHT: &[[i32; 3]] = &[[3, 7, 10], [10, 14, 17], [0, 3, 7], [8, 12, 15]]; // C G Am F
const LOUNGE: &[[i32; 3]] = &[[5, 8, 12], [0, 3, 7], [3, 7, 10], [-2, 2, 5]]; // Dm Am C G
const WARM: &[[i32; 3]] = &[[-2, 2, 5], [3, 7, 10], [-4, 0, 3], [5, 8, 12]]; // G C F Dm
const CALM: &[[i32; 3]] = &[[0, 3, 7], [-4, 0, 3], [3, 7, 10], [-2, 2, 5]]; // Am F C G

const SHOW: &[Sound] = &[
    s("Intro Theme", 24.0, Kind::Music(116.0, true, BRIGHT), TileColor::Blue, PlayMode::Toggle, false),
    s("Applause", 6.0, Kind::Applause, TileColor::Green, PlayMode::Toggle, false),
    s("Rimshot", 1.2, Kind::Rimshot, TileColor::Orange, PlayMode::Restart, false),
    s("Sad Trombone", 3.5, Kind::SadTrombone, TileColor::Purple, PlayMode::Toggle, false),
    s("Air Horn", 2.0, Kind::AirHorn, TileColor::Red, PlayMode::Hold, false),
    s("Crickets", 8.0, Kind::Crickets, TileColor::Mint, PlayMode::Toggle, true),
    s("Drum Roll", 5.0, Kind::DrumRoll, TileColor::Brown, PlayMode::Toggle, false),
    s("Ding!", 1.0, Kind::Ding, TileColor::Yellow, PlayMode::Restart, false),
    s("Wrong Answer", 1.5, Kind::Buzzer, TileColor::Pink, PlayMode::Restart, false),
    s("Victory Fanfare", 7.0, Kind::Fanfare, TileColor::Indigo, PlayMode::Toggle, false),
    s("Ambience Bed", 30.0, Kind::Crowd, TileColor::Teal, PlayMode::Pause, true),
    s("Outro", 20.0, Kind::Music(92.0, true, WARM), TileColor::Cyan, PlayMode::Toggle, false),
    s("Welcome Back!", 3.6, Kind::Speech("2 1 4 | 1 1 2 2", 120.0, 0.9), TileColor::Cyan, PlayMode::Restart, false),
    s("The Answer Is…", 3.2, Kind::Speech("1 1 2 1 | 1", 105.0, 0.5), TileColor::Indigo, PlayMode::Restart, false),
    s("Time's Up!", 1.4, Kind::Speech("1 1 | 1", 210.0, 1.0), TileColor::Red, PlayMode::Restart, false),
    s("Ooh, So Close!", 2.0, Kind::Speech("1 | 1 1", 190.0, 0.8), TileColor::Pink, PlayMode::Restart, false),
];

const CUES: &[Sound] = &[
    s("Preshow Music", 40.0, Kind::Music(108.0, true, LOUNGE), TileColor::Blue, PlayMode::Toggle, false),
    s("House Lights Down", 4.0, Kind::Sweep, TileColor::Gray, PlayMode::Toggle, false),
    s("Scene 1 Underscore", 25.0, Kind::Music(72.0, false, CALM), TileColor::Teal, PlayMode::Toggle, false),
    s("Door Slam", 0.8, Kind::DoorSlam, TileColor::Orange, PlayMode::Toggle, false),
    s("Phone Ring", 5.4, Kind::PhoneRing, TileColor::Green, PlayMode::Toggle, false),
    s("Intermission", 30.0, Kind::Music(96.0, false, LOUNGE), TileColor::Purple, PlayMode::Toggle, false),
    s("Thunder", 6.0, Kind::Thunder, TileColor::Gray, PlayMode::Toggle, false),
    s("Closing Announcement", 10.5, Kind::Speech("2 1 3 2 | 1 2 1 3 | 2 1 1 2 1 | 1 3 2 | 2 1", 115.0, 0.4), TileColor::Yellow, PlayMode::Toggle, false),
];

/// A demo sound's samples, the same on every run (the seed is its place in the set).
/// Rendered once per process: the tests build the demo library many times.
fn samples(set: u64, i: usize, snd: &Sound) -> Arc<Vec<f32>> {
    type Rendered = BTreeMap<(u64, usize), Arc<Vec<f32>>>;
    static DONE: Mutex<Rendered> = Mutex::new(BTreeMap::new());
    if let Some(s) = DONE.lock().expect("demo cache").get(&(set, i)) {
        return s.clone();
    }
    let s = Arc::new(synth::render(snd.kind, snd.seconds, set * 100 + i as u64 + 1));
    DONE.lock().expect("demo cache").insert((set, i), s.clone());
    s
}

fn build_set(store: &mut Store, name: &str, seed: u64, sounds: &[Sound], layout: Layout, columns: u32) -> anyhow::Result<()> {
    let tmp = std::env::temp_dir().join(format!("leadstinger-demo-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp)?;
    let paths: Vec<_> = sounds
        .iter()
        .enumerate()
        .map(|(i, snd)| {
            let path = tmp.join(format!("{}.wav", snd.title.replace(['/', ':'], "-")));
            wav::write_mono(&path, synth::RATE, &samples(seed, i, snd)).map(|_| path)
        })
        .collect::<std::io::Result<_>>()?;
    let set = SoundSet { layout, columns, ..SoundSet::named(name) };
    store.library.sets.push(set);
    let si = store.library.sets.len() - 1;
    let (ids, _) = store.import(&paths, si, crate::rows::Place::Append);
    for (id, snd) in ids.iter().zip(sounds) {
        if let Some(t) = store.library.tile_mut(*id) {
            t.color = snd.color;
            t.press = PressOptions::from_mode(snd.mode);
            t.looping = snd.looping;
            t.stop_fade = if snd.looping { 1.5 } else { 0.0 };
        }
    }
    // The files are copied in the background; let that finish before they go.
    store.finish_imports();
    std::fs::remove_dir_all(tmp).ok();
    Ok(())
}

/// Replace an empty library with two demo sets. Does nothing if any tile exists.
pub fn populate_if_empty(store: &mut Store) -> anyhow::Result<()> {
    populate(store, SHOW.len(), CUES.len())
}

/// The demo library as it was when the window tests were written: the first 12 tiles of
/// Game Night and 6 of Theatre Cues (the later ones were added after them, D-258).
#[cfg(test)]
pub(crate) fn populate_for_tests(store: &mut Store) -> anyhow::Result<()> {
    populate(store, 12, 6)
}

fn populate(store: &mut Store, show: usize, cues: usize) -> anyhow::Result<()> {
    if store.library.sets.iter().any(|s| !s.tiles.is_empty()) {
        return Ok(());
    }
    store.library.sets.clear();
    build_set(store, "Game Night", 1, &SHOW[..show], Layout::Grid, 4)?;
    build_set(store, "Theatre Cues", 2, &CUES[..cues], Layout::List, 4)?;
    store.library.selected_set = Some(store.library.sets[0].id);
    store.mark_dirty();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Media;
    use std::time::{Duration, Instant};

    #[test]
    fn every_demo_tile_decodes() {
        let dir = std::env::temp_dir().join(format!("sb-demo-{}", uuid::Uuid::new_v4()));
        let mut store = Store::open(dir.clone()).unwrap();
        populate_if_empty(&mut store).unwrap();
        // The sounds are written to a temporary folder, removed once they're copied in.
        let deadline = Instant::now() + Duration::from_secs(30);
        while store.is_loading() && Instant::now() < deadline {
            store.poll();
            std::thread::sleep(Duration::from_millis(10));
        }
        let tiles: Vec<_> = store.library.sets.iter().flat_map(|s| &s.tiles).collect();
        assert_eq!(tiles.len(), SHOW.len() + CUES.len());
        for t in tiles {
            assert!(matches!(store.media(t), Some(Media::Ready { .. })), "“{}” didn't decode", t.title);
        }
        drop(store);
        std::fs::remove_dir_all(dir).ok();
    }

    const DEMO_HASH: u64 = 0x8601_f067_4c8c_e025;

    /// Each tile's peak envelope varies (stddev / mean of 400 bucket peaks), so the board
    /// shows shapes rather than flat bars.
    fn variation(samples: &[f32]) -> f64 {
        let buckets = 400;
        let mut peaks = vec![0.0f64; buckets];
        for (i, s) in samples.iter().enumerate() {
            let b = i * buckets / samples.len();
            peaks[b] = peaks[b].max(f64::from(s.abs()));
        }
        let mean = peaks.iter().sum::<f64>() / buckets as f64;
        let var = peaks.iter().map(|p| (p - mean).powi(2)).sum::<f64>() / buckets as f64;
        var.sqrt() / mean
    }

    #[test]
    fn demo_sounds_are_shaped_deterministic_and_safe() {
        let all: Vec<(u64, usize, &Sound)> = [(1, SHOW), (2, CUES)].iter().flat_map(|&(set, sounds)| sounds.iter().enumerate().map(move |(i, s)| (set, i, s))).collect();
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        let mut shaped = 0;
        for &(set, i, snd) in &all {
            let a = samples(set, i, snd);
            let a = a.as_slice();
            assert_eq!(a.len(), (snd.seconds * f64::from(synth::RATE)) as usize);
            let peak = a.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            assert!((0.4..=0.9).contains(&peak), "“{}” peaks at {peak}", snd.title);
            let v = variation(a);
            if v > 0.25 {
                shaped += 1;
            }
            for s in a {
                hash = (hash ^ u64::from((*s * 32767.0) as i16 as u16)).wrapping_mul(0x100_0000_01b3);
            }
        }
        assert!(shaped * 10 >= all.len() * 9, "only {shaped} of {} demo sounds have a shaped waveform", all.len());
        // The same samples on every run and platform. If you change a demo sound on
        // purpose, put the new hash here.
        assert_eq!(hash, DEMO_HASH, "the demo sounds changed: {hash:#018x}");
    }
}
