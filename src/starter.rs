//! Packs: a folder of the user's own clips, arranged in sets, with an optional
//! `manifest.json`. The same layout is a starter pack (imported on a first launch, below)
//! and, zipped, a set file (`.lsset`, src/setfile.rs). The app ships no clips; see
//! starter/README.md for how a pack is made.
//!
//! The manifest names the sets and gives each tile's settings (title, hotkey, colour, mode,
//! loop, fades, volume, solo, trim, picture), and each set's layout; `scripts/clip-pack.py`
//! and `set.export` write it. A starter pack's manifest can also choose the look the app
//! opens in (`"theme": "panel"`). Without a manifest, each subfolder becomes a set and its
//! audio files become tiles.
//!
//! The starter pack is `$LEADSTINGER_STARTER_DIR`, else `<data dir>/Starter` (or a
//! `Starter.lsset` file), else one next to the executable (`../Resources/Starter` in the
//! macOS bundle, `../share/<slug>/starter` in the Nix package). It applies once: only to
//! an empty library, and never again after `Starter.applied` is in the data dir.

use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::decode::AUDIO_EXTENSIONS;
use crate::logic::{guess_hand_set, set_hotkey, PadKey};
use crate::model::{tidy_channel_mix, tidy_sends, ChannelRoute, TileSend};
use crate::model::{Appearance, Duck, FadeCurve, Marker, ImageFit, ImageLayer, Layout, PlayMode, PressOptions, SoundSet, ThemeKind, TileColor, GAIN_DB_MAX};
use crate::store::Store;

/// Written to the data dir once a pack has been imported.
pub const MARKER: &str = "Starter.applied";

fn applied(data_dir: &Path) -> bool {
    data_dir.join(MARKER).exists()
}

/// `manifest.json`. Every field is optional, so packs from older and newer builds load.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Manifest {
    /// `leadstinger-set` (written by `set.export`) or `leadstinger-starter` (clip-pack.py).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<u32>,
    /// What wrote it, e.g. "LeadStinger 0.2.0".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    pub sets: Vec<PackSet>,
    /// `console` or `panel`: the look the app opens in (starter packs only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
    /// `dark`, `light` or `system` (starter packs only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub appearance: Option<String>,
    /// A user theme inside the pack (a `.lstheme` folder or zip). A first launch installs
    /// it and opens in it; importing a set file installs it and asks whether to use it
    /// (D-173). Builds without user themes ignore it and open in `theme`, so a pack names
    /// the theme's base there too.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme_file: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct PackSet {
    pub name: String,
    /// `grid` or `list`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layout: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub columns: Option<u32>,
    /// Tiles per grid row, top to bottom (D-113); without it, rows of `columns`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rows: Option<Vec<u32>>,
    /// The blank slots, [row, slot] 0-based (D-140); without it, none.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub blanks: Vec<[u32; 2]>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub auto_advance: bool,
    /// A cue list's cross-fade in seconds (Continuous); none when 0.
    #[serde(skip_serializing_if = "is_zero")]
    pub crossfade: f32,
    pub tiles: Vec<PackTile>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub notes: String,
    /// The set lock (M22): a locked set imports locked.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub locked: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct PackTile {
    /// Relative to the pack folder.
    pub file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// `None`: the app picks a free key; `Some("")`: no hotkey.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hotkey: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// The old single mode (`toggle`, `restart`, `hold`, `pause`); set files still write
    /// it for older builds. Any of `hold`, `pausable` and `one_shot` overrides it (D-145).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    /// The pad options, written only when on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hold: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pausable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub one_shot: Option<bool>,
    #[serde(rename = "loop", skip_serializing_if = "std::ops::Not::not")]
    pub looping: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume: Option<f32>,
    /// The output it plays on, by name (default Main). Imported only if the library has
    /// an output of that name. Set files write it beside `sends` for older builds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    /// Semitones, −12 to +12 (default 0).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pitch: Option<f32>,
    /// Playback speed, 0.5–2 (default 1).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed: Option<f32>,
    /// dB on top of the volume, −24 to +24.
    #[serde(skip_serializing_if = "is_zero")]
    pub gain_db: f32,
    /// `ducks` or `ducked`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duck: Option<String>,
    #[serde(skip_serializing_if = "is_zero")]
    pub fade_in: f32,
    #[serde(skip_serializing_if = "is_zero")]
    pub fade_out: f32,
    /// Seconds to fade when stopped (0 = at once, with the 0.1 s de-click). Missing means
    /// the same as `fade_out`, which is what packs and set files meant before the two were
    /// split.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_fade: Option<f32>,
    /// `linear`, `equal_power`, `exponential` or `s_curve` (default `gentle`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fade_curve: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub solo: bool,
    /// Seconds into the file.
    #[serde(skip_serializing_if = "is_zero64")]
    pub trim_start: f64,
    /// Seconds into the file; none means the end.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trim_end: Option<f64>,
    /// Named points to jump to.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub markers: Vec<Marker>,
    /// `[start, end]` seconds into the file: loops until it's let go (Play On).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vamp: Option<(f64, f64)>,
    /// An image, relative to the pack folder.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub picture: Option<String>,
    /// `underlay` or `overlay`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub picture_layer: Option<String>,
    /// `fill` or `fit`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub picture_fit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub picture_opacity: Option<f32>,
    /// Every output it plays on, each at its own level (`{output, gain_db}`); wins over
    /// `output` when both are there. Sends to outputs the library lacks are dropped.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub sends: Vec<TileSend>,
    /// A multichannel file's channel mix: one `{to, gain_db}` per source channel (`to`
    /// is left, right, both or mute). Absent is Auto, the stereo downmix.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub channels: Vec<ChannelRoute>,
    /// OSC or MIDI messages when the tile starts and stops (src/messages.rs); bad ones are
    /// skipped on import.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub on_start: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub on_stop: Vec<String>,
    /// The cue's notes.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub notes: String,
    /// Triggers and GO skip it.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub disabled: bool,
    /// Seconds between triggering it and its start.
    #[serde(skip_serializing_if = "is_zero64")]
    pub pre_wait: f64,
    /// `continue` or `follow`: what starts the next cue without GO (default none).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
    /// Seconds after it starts (continue) or ends (follow) before the next cue starts.
    #[serde(skip_serializing_if = "is_zero64")]
    pub post_wait: f64,
    /// An action tile (with `file` empty): what it does to other tiles in the file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<PackAction>,
}

/// An action tile's action. Its targets name tiles by position, since ids don't travel.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct PackAction {
    /// `play`, `fade`, `stop`, `pause` or `play_on` (`release`, its old name, still reads).
    pub kind: String,
    pub fade: f32,
    pub targets: Vec<PackTarget>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct PackTarget {
    /// The set's position in the file, from 0.
    pub set: usize,
    /// The tile's position in that set's `tiles`, from 0; none means every tile in it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tile: Option<usize>,
    /// Seconds after the trigger, for a group.
    #[serde(skip_serializing_if = "is_zero64")]
    pub offset: f64,
}

fn is_zero(v: &f32) -> bool {
    *v == 0.0
}

fn is_zero64(v: &f64) -> bool {
    *v == 0.0
}

/// The places to look, in order. `env_dir` (from `LEADSTINGER_STARTER_DIR`) is the only
/// candidate when it is set; it may be a folder or a set file.
fn candidates(env_dir: Option<PathBuf>, data_dir: &Path, exe: Option<&Path>) -> Vec<PathBuf> {
    if let Some(dir) = env_dir {
        return vec![dir];
    }
    let mut dirs = vec![data_dir.join("Starter"), data_dir.join("Starter.lsset")];
    if let Some(bin) = exe.and_then(Path::parent) {
        dirs.push(bin.join("../Resources/Starter"));
        dirs.push(bin.join("../share").join(crate::brand::SLUG).join("starter"));
    }
    dirs
}

/// Import the starter pack if this is a first launch and a pack exists. Returns how many
/// tiles were added, or `None` if nothing was done.
pub fn preload_if_first_launch(store: &mut Store) -> anyhow::Result<Option<usize>> {
    if applied(&store.data_dir) || !is_empty(store) {
        return Ok(None);
    }
    let exe = std::env::current_exe().ok();
    let env_dir = crate::brand::env("STARTER_DIR").map(PathBuf::from);
    let found = candidates(env_dir, &store.data_dir, exe.as_deref())
        .into_iter()
        .find(|d| d.is_dir() || (d.is_file() && crate::setfile::is_set_file(d)));
    match found {
        Some(dir) if dir.is_file() => {
            let unpacked = crate::setfile::Unpacked::new(&dir)?;
            let added = preload_from(store, unpacked.root(), &dir);
            store.keep_until_imported(unpacked);
            added
        }
        Some(dir) => preload_from(store, &dir, &dir),
        None => Ok(None),
    }
}

fn is_empty(store: &Store) -> bool {
    store.library.sets.iter().all(|s| s.tiles.is_empty())
}

/// [`preload_if_first_launch`] with the pack already chosen (and unpacked into `dir`;
/// `source` is what the marker records).
fn preload_from(store: &mut Store, dir: &Path, source: &Path) -> anyhow::Result<Option<usize>> {
    let marker = store.data_dir.join(MARKER);
    if applied(&store.data_dir) || !is_empty(store) {
        return Ok(None);
    }
    let manifest = load_manifest(dir)?;
    let saved = (std::mem::take(&mut store.library.sets), store.library.selected_set);
    let mut added = 0;
    for gs in &manifest.sets {
        added += import_set(store, dir, gs, "starter pack", false).1;
    }
    store.library.sets.retain(|s| !s.tiles.is_empty());
    if added == 0 {
        // Nothing usable: keep the library as it was, and try again next launch.
        (store.library.sets, store.library.selected_set) = saved;
        eprintln!("{}: the starter pack {} has no playable clips", crate::brand::SLUG, source.display());
        return Ok(None);
    }
    store.library.selected_set = Some(store.library.sets[0].id);
    if let Some(t) = named::<ThemeKind>(&manifest.theme, "theme", "manifest") {
        store.library.theme = t;
    }
    if let Some(a) = named::<Appearance>(&manifest.appearance, "appearance", "manifest") {
        store.library.appearance = a;
    }
    if let Some(file) = &manifest.theme_file {
        install_theme(store, dir, file);
    }
    store.mark_dirty();
    store.save_now()?;
    std::fs::write(&marker, format!("{}\n", source.display()))?;
    Ok(Some(added))
}

/// Install the pack's theme and choose it; the controller opens it at start. A theme that
/// doesn't load leaves the look the manifest chose, with a message.
fn install_theme(store: &mut Store, dir: &Path, file: &str) {
    let slug = crate::brand::SLUG;
    let Some(path) = inside(dir, file) else {
        eprintln!("{slug}: pack manifest: the theme {file:?} is outside the pack");
        return;
    };
    match crate::themes::import(&store.data_dir, &path) {
        Ok(imported) => {
            for w in &imported.warnings {
                eprintln!("{slug}: theme {}: {w}", imported.theme.name);
            }
            store.library.theme = imported.look.base;
            store.library.theme_name = Some(imported.theme.id);
        }
        Err(e) => eprintln!("{slug}: couldn't install the pack's theme {file}: {e:#}"),
    }
}

/// The pack's manifest, or one made by scanning its folders.
pub(crate) fn load_manifest(dir: &Path) -> anyhow::Result<Manifest> {
    let path = dir.join("manifest.json");
    if path.exists() {
        let bytes = std::fs::read(&path)?;
        return serde_json::from_slice(&bytes).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()));
    }
    Ok(scan(dir))
}

fn is_audio(p: &Path) -> bool {
    p.is_file() && (crate::transcode::prefers_ffmpeg(p) || p.extension().and_then(|e| e.to_str()).is_some_and(|e| AUDIO_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str())))
}

/// No manifest: each subfolder is a set (in name order), and loose files go in "Starter".
fn scan(dir: &Path) -> Manifest {
    let entries = |d: &Path| -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = std::fs::read_dir(d).into_iter().flatten().flatten().map(|e| e.path()).collect();
        v.sort();
        v
    };
    let tiles = |paths: Vec<PathBuf>| -> Vec<PackTile> {
        paths
            .into_iter()
            .filter(|p| is_audio(p))
            .filter_map(|p| p.strip_prefix(dir).ok().map(|r| PackTile { file: r.to_string_lossy().into_owned(), ..Default::default() }))
            .collect()
    };
    let top = entries(dir);
    let mut sets: Vec<PackSet> = top
        .iter()
        .filter(|p| p.is_dir())
        .map(|d| PackSet { name: d.file_name().unwrap_or_default().to_string_lossy().into_owned(), tiles: tiles(entries(d)), ..Default::default() })
        .collect();
    let loose = tiles(top);
    if !loose.is_empty() {
        sets.push(PackSet { name: "Starter".into(), tiles: loose, ..Default::default() });
    }
    Manifest { sets, ..Default::default() }
}

/// Where a pack really starts: a zip made by zipping a folder (Finder's Compress, say)
/// holds that one folder, so look inside it when the top has no manifest or clips.
pub(crate) fn pack_root(dir: &Path) -> PathBuf {
    if dir.join("manifest.json").exists() {
        return dir.to_path_buf();
    }
    let entries: Vec<PathBuf> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).collect();
    match entries.as_slice() {
        [only] if only.is_dir() && only.join("manifest.json").exists() => only.clone(),
        [only] if only.is_dir() && !entries.iter().any(|p| is_audio(p)) => {
            // One folder holding set folders, not clips: that folder is the pack.
            let inner: Vec<PathBuf> = std::fs::read_dir(only).into_iter().flatten().flatten().map(|e| e.path()).collect();
            if inner.iter().any(|p| p.is_dir()) && !inner.iter().any(|p| is_audio(p)) { only.clone() } else { dir.to_path_buf() }
        }
        _ => dir.to_path_buf(),
    }
}

/// A manifest path that stays inside the pack folder.
pub(crate) fn inside(dir: &Path, file: &str) -> Option<PathBuf> {
    let rel = Path::new(file);
    rel.components().all(|c| matches!(c, Component::Normal(_))).then(|| dir.join(rel))
}

/// Parse a snake_case enum value, warning (about `whose` value) if it's unknown.
fn named<T: serde::de::DeserializeOwned>(text: &Option<String>, what: &str, whose: &str) -> Option<T> {
    let t = text.as_deref()?.trim().to_ascii_lowercase();
    let v = serde_json::from_value(Value::String(t.clone())).ok();
    if v.is_none() && !t.is_empty() {
        eprintln!("{}: pack {whose}: unknown {what} {t:?}", crate::brand::SLUG);
    }
    v
}

/// Append one set from a pack and its tiles. Returns the new set's id, how many tiles
/// were added, and each pack tile's new id (none where it was skipped); problems are
/// printed, naming `what` ("starter pack", "set file"). Action tiles come in without
/// targets: those name positions across the whole file (setfile.rs resolves them).
pub(crate) fn import_set(store: &mut Store, dir: &Path, gs: &PackSet, what: &str, old_keys: bool) -> (Uuid, usize, Vec<Option<Uuid>>) {
    let slug = crate::brand::SLUG;
    let mut set = SoundSet::named(if gs.name.trim().is_empty() { "Starter" } else { gs.name.trim() });
    if let Some(l) = named::<Layout>(&gs.layout, "layout", &format!("set “{}”", set.name)) {
        set.layout = l;
    }
    if let Some(c) = gs.columns {
        set.columns = c.clamp(1, 12);
    }
    set.auto_advance = gs.auto_advance;
    set.crossfade = gs.crossfade.clamp(0.0, crate::model::CROSSFADE_MAX);
    set.locked = gs.locked;
    set.notes = gs.notes.clone();
    let set_id = set.id;
    store.library.sets.push(set);
    let si = store.library.sets.len() - 1;
    let mut keyed = Vec::new();
    let mut added = 0;
    let mut new_ids = Vec::new();
    for gt in &gs.tiles {
        new_ids.push(None);
        let id = if gt.action.is_some() && gt.file.is_empty() {
            let t = crate::model::Tile { action: Some(Default::default()), ..Default::default() };
            let id = t.id;
            store.library.sets[si].tiles.push(t);
            id
        } else {
            let Some(path) = inside(dir, &gt.file).filter(|p| p.is_file()) else {
                eprintln!("{slug}: {what} clip {:?} is missing", gt.file);
                continue;
            };
            // One file at a time, so each clip's settings land on its own tile.
            let (ids, errors) = store.import(std::slice::from_ref(&path), si, crate::rows::Place::Append);
            for e in errors {
                eprintln!("{slug}: {what} clip: {e}");
            }
            let Some(&id) = ids.first() else { continue };
            id
        };
        *new_ids.last_mut().expect("pushed") = Some(id);
        added += 1;
        let picture = gt.picture.as_deref().and_then(|p| inside(dir, p)).filter(|p| p.is_file());
        if let Some(pic) = &picture {
            if let Err(e) = store.set_image(id, pic) {
                eprintln!("{slug}: {what} picture {:?}: {e:#}", gt.picture.as_deref().unwrap_or_default());
            }
        }
        let tiles = &mut store.library.sets[si].tiles;
        let Some(t) = tiles.iter_mut().find(|t| t.id == id) else { continue };
        if let Some(title) = gt.title.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            t.title = title.to_owned();
        }
        let whose = format!("tile “{}”", t.title);
        if let Some(c) = named::<TileColor>(&gt.color, "colour", &whose) {
            t.color = c;
        }
        t.press = PressOptions::read(named::<PlayMode>(&gt.mode, "mode", &whose), gt.hold, gt.pausable, gt.one_shot);
        if let Some(c) = named::<FadeCurve>(&gt.fade_curve, "fade curve", &whose) {
            t.fade_curve = c;
        }
        if let Some(d) = named::<Duck>(&gt.duck, "ducking", &whose) {
            t.duck = d;
        }
        t.looping = gt.looping;
        t.volume = gt.volume.unwrap_or(1.0).clamp(0.0, 1.0);
        let sends = if gt.sends.is_empty() { gt.output.iter().map(TileSend::to).collect() } else { gt.sends.clone() };
        let known = |s: &TileSend| s.is_main() || store.library.outputs.iter().any(|d| d.name == s.output);
        t.sends = sends.into_iter().filter(known).collect();
        tidy_sends(&mut t.sends);
        t.channel_mix = gt.channels.clone();
        tidy_channel_mix(&mut t.channel_mix);
        t.pitch = gt.pitch.filter(|p| p.is_finite()).unwrap_or(0.0).clamp(-crate::pitch::PITCH_MAX, crate::pitch::PITCH_MAX);
        t.speed = gt.speed.filter(|s| s.is_finite()).unwrap_or(1.0).clamp(0.5, 2.0);
        t.gain_db = if gt.gain_db.is_finite() { gt.gain_db.clamp(-GAIN_DB_MAX, GAIN_DB_MAX) } else { 0.0 };
        t.fade_in = gt.fade_in.max(0.0);
        t.fade_out = gt.fade_out.max(0.0);
        t.stop_fade = gt.stop_fade.filter(|f| f.is_finite()).unwrap_or(t.fade_out).clamp(0.0, 10.0);
        t.solo = gt.solo;
        t.start = gt.trim_start.max(0.0);
        t.end = gt.trim_end.filter(|e| *e > t.start);
        t.vamp = gt.vamp.filter(|(a, b)| b - a >= 0.05 && *a >= t.start);
        t.markers = gt.markers.iter().filter(|m| m.at >= 0.0 && !m.name.trim().is_empty()).cloned().collect();
        t.markers.sort_by(|a, b| a.at.total_cmp(&b.at));
        if let Some(l) = named::<ImageLayer>(&gt.picture_layer, "picture layer", &whose) {
            t.image_layer = l;
        }
        if let Some(f) = named::<ImageFit>(&gt.picture_fit, "picture fit", &whose) {
            t.image_fit = f;
        }
        if let Some(o) = gt.picture_opacity {
            t.image_opacity = o.clamp(0.0, 1.0);
        }
        let messages = |list: &[String], when: &str| -> Vec<String> {
            list.iter()
                .filter(|s| match crate::messages::parse(s) {
                    Ok(_) => true,
                    Err(e) => {
                        eprintln!("{slug}: {what} {whose}: {when} message skipped: {e}");
                        false
                    }
                })
                .map(|s| s.trim().to_owned())
                .collect()
        };
        t.on_start = messages(&gt.on_start, "start");
        t.on_stop = messages(&gt.on_stop, "stop");
        t.notes = gt.notes.clone();
        t.disabled = gt.disabled;
        let wait = |v: f64| if v.is_finite() { v.clamp(0.0, crate::model::WAIT_MAX) } else { 0.0 };
        t.pre_wait = wait(gt.pre_wait);
        t.post_wait = wait(gt.post_wait);
        if let Some(l) = named::<crate::model::CueLink>(&gt.link, "link", &whose) {
            t.link = l;
        }
        match gt.hotkey.as_deref().map(str::trim) {
            None => {}
            Some("") => keyed.push((id, None)),
            Some(k) => match PadKey::parse(k) {
                Some(c) => keyed.push((id, Some(c))),
                None => eprintln!("{slug}: {what} {whose}: {k:?} can't be a hotkey"),
            },
        }
    }
    // Keys the manifest names are set by hand; the others follow the rows (D-188).
    let tiles = &mut store.library.sets[si].tiles;
    for (id, key) in &keyed {
        set_hotkey(tiles, *id, *key);
    }
    // A set file from before D-188 names every tile's key: those in the old order were
    // automatic (D-193).
    if old_keys {
        guess_hand_set(tiles);
    }
    // The file's rows and blanks, less any tiles that didn't come in.
    if let Some(rows) = &gs.rows {
        let set = &mut store.library.sets[si];
        let mut g = crate::rows::derive(rows, &gs.blanks, new_ids.len(), set.columns);
        for row in &mut g {
            row.retain(|x| x.is_none_or(|i| new_ids[i].is_some()));
        }
        set.set_shape(g);
    }
    store.mark_dirty();
    (set_id, added, new_ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wav;
    use serde_json::json;

    fn temp_dir() -> PathBuf {
        let d = std::env::temp_dir().join(format!("sb-starter-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn tone(path: &Path, freq: f32) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        wav::write_tone(path, 8000, 0.2, &[freq], 0.3).unwrap();
    }

    #[test]
    fn manifest_pack_fills_an_empty_library_once() {
        let root = temp_dir();
        let pack = root.join("Starter");
        tone(&pack.join("Show/Air Horn.wav"), 440.0);
        tone(&pack.join("Show/Ding.wav"), 660.0);
        tone(&pack.join("Show/Room Tone.wav"), 110.0);
        tone(&pack.join("Music/Rimshot.wav"), 220.0);
        let manifest = json!({
            "format": "leadstinger-starter", "version": 1, "theme": "Panel",
            "sets": [
                {"name": "Show", "tiles": [
                    {"file": "Show/Air Horn.wav", "title": "Air Horn", "hotkey": "R", "color": "red", "mode": "restart"},
                    {"file": "Show/Ding.wav", "title": "Ding", "hotkey": "1", "color": "teal", "mode": "hold"},
                    {"file": "Show/Missing.wav", "title": "Not clipped yet"},
                    {"file": "../outside.wav"},
                    {"file": "Show/Room Tone.wav", "color": "Gold", "mode": "toggle", "loop": true, "fade_out": 2.0, "volume": 0.6}
                ]},
                {"name": "Music", "tiles": [{"file": "Music/Rimshot.wav", "hotkey": "", "mode": "restart", "hold": true, "pausable": true}]},
                {"name": "Empty", "tiles": [{"file": "nope.wav"}]}
            ]
        });
        std::fs::write(pack.join("manifest.json"), manifest.to_string()).unwrap();

        let data = root.join("data");
        let mut store = Store::open(data.clone()).unwrap();
        assert_eq!(preload_from(&mut store, &pack, &pack).unwrap(), Some(4));
        let names: Vec<&str> = store.library.sets.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["Show", "Music"], "sets with no clips are dropped");
        assert_eq!(store.library.selected_set, Some(store.library.sets[0].id));

        let bridge = &store.library.sets[0].tiles;
        let t: Vec<_> = bridge.iter().map(|t| (t.title.as_str(), t.hotkey, t.color, t.press.mode())).collect();
        assert_eq!(
            t,
            [
                ("Air Horn", Some(PadKey::plain('r')), TileColor::Red, PlayMode::Restart),
                ("Ding", Some(PadKey::plain('1')), TileColor::Teal, PlayMode::Hold),
                // Its slot's key; an unknown colour keeps the automatic one.
                ("Room Tone", Some(PadKey::plain('3')), TileColor::nth(2), PlayMode::Toggle),
            ]
        );
        assert!(bridge[2].looping && bridge[2].fade_out == 2.0 && (bridge[2].volume - 0.6).abs() < 1e-6);
        assert_eq!(bridge[2].stop_fade, 2.0, "a pack's fade_out still fades it when stopped");
        assert_eq!(store.library.sets[1].tiles[0].hotkey, None, "an empty hotkey means none");
        assert_eq!(store.library.sets[1].tiles[0].press, PressOptions { hold: true, pausable: true, one_shot: false }, "the options win over the old mode");
        assert!(data.join(MARKER).exists());
        assert_eq!(store.library.theme, ThemeKind::Panel, "the manifest picks the look");
        assert_eq!(store.library.appearance, Appearance::default(), "no appearance given: unchanged");

        // Saved: a relaunch sees the same library, and the pack isn't applied again.
        drop(store);
        let mut store = Store::open(data.clone()).unwrap();
        assert_eq!(store.library.sets.len(), 2);
        for s in &mut store.library.sets {
            s.tiles.clear();
        }
        assert_eq!(preload_from(&mut store, &pack, &pack).unwrap(), None, "the marker stops a second import");
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn a_pack_can_bring_its_own_theme() {
        let root = temp_dir();
        let pack = root.join("Starter");
        tone(&pack.join("Show/Ding.wav"), 660.0);
        let harbor = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/themes/harbor.lstheme");
        for f in ["theme.json", "tokens.json"] {
            std::fs::create_dir_all(pack.join("Harbor.lstheme")).unwrap();
            std::fs::copy(harbor.join(f), pack.join("Harbor.lstheme").join(f)).unwrap();
        }
        let manifest = json!({"theme": "console", "theme_file": "Harbor.lstheme", "sets": [{"name": "Show", "tiles": [{"file": "Show/Ding.wav"}]}]});
        std::fs::write(pack.join("manifest.json"), manifest.to_string()).unwrap();
        let data = root.join("data");
        let mut store = Store::open(data.clone()).unwrap();
        assert_eq!(preload_from(&mut store, &pack, &pack).unwrap(), Some(1));
        assert_eq!((store.library.theme, store.library.theme_name.as_deref()), (ThemeKind::Console, Some("harbor")));
        assert!(crate::themes::themes_dir(&data).join("harbor/tokens.json").exists(), "installed in the data dir");

        // A theme outside the pack, or one that doesn't load, leaves the manifest's look.
        for file in ["../Harbor.lstheme", "Show"] {
            let pack2 = root.join(format!("Pack-{}", file.len()));
            tone(&pack2.join("Show/Ding.wav"), 660.0);
            let manifest = json!({"theme": "panel", "theme_file": file, "sets": [{"name": "Show", "tiles": [{"file": "Show/Ding.wav"}]}]});
            std::fs::write(pack2.join("manifest.json"), manifest.to_string()).unwrap();
            let mut store = Store::open(root.join(format!("data-{}", file.len()))).unwrap();
            assert_eq!(preload_from(&mut store, &pack2, &pack2).unwrap(), Some(1));
            assert_eq!((store.library.theme, store.library.theme_name.as_deref()), (ThemeKind::Panel, None), "{file}");
        }
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn folders_without_a_manifest_become_sets_and_a_full_library_is_left_alone() {
        let root = temp_dir();
        let pack = root.join("Starter");
        tone(&pack.join("Sound Effects/Door.wav"), 500.0);
        tone(&pack.join("Sound Effects/Chime.wav"), 900.0);
        tone(&pack.join("Loose.wav"), 300.0);
        std::fs::write(pack.join("Sound Effects/notes.txt"), "not audio").unwrap();

        let mut store = Store::open(root.join("data")).unwrap();
        assert_eq!(preload_from(&mut store, &pack, &pack).unwrap(), Some(3));
        let sets: Vec<(&str, Vec<&str>)> =
            store.library.sets.iter().map(|s| (s.name.as_str(), s.tiles.iter().map(|t| t.title.as_str()).collect())).collect();
        assert_eq!(sets, [("Sound Effects", vec!["Chime", "Door"]), ("Starter", vec!["Loose"])]);
        assert_eq!(store.library.theme, ThemeKind::default(), "no manifest: the look is left alone");

        let mut other = Store::open(root.join("other")).unwrap();
        let src = root.join("Mine.wav");
        tone(&src, 250.0);
        other.import(&[src], 0, crate::rows::Place::Append);
        assert_eq!(preload_from(&mut other, &pack, &pack).unwrap(), None);
        assert_eq!(other.library.sets.len(), 1);

        let empty = temp_dir();
        let mut fresh = Store::open(root.join("fresh")).unwrap();
        assert_eq!(preload_from(&mut fresh, &empty, &empty).unwrap(), None, "an empty folder changes nothing");
        assert_eq!(fresh.library.sets.len(), 1);
        assert!(!root.join("fresh").join(MARKER).exists());
        std::fs::remove_dir_all(root).ok();
        std::fs::remove_dir_all(empty).ok();
    }

    #[test]
    fn search_order() {
        let data = Path::new("/d");
        let exe = Path::new("/app/Contents/MacOS/LeadStinger");
        assert_eq!(candidates(Some("/g".into()), data, Some(exe)), [PathBuf::from("/g")]);
        assert_eq!(
            candidates(None, data, Some(exe)),
            [
                PathBuf::from("/d/Starter"),
                PathBuf::from("/d/Starter.lsset"),
                PathBuf::from("/app/Contents/MacOS/../Resources/Starter"),
                PathBuf::from(format!("/app/Contents/MacOS/../share/{}/starter", crate::brand::SLUG)),
            ]
        );
    }
}
