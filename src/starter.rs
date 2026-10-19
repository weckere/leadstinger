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
//! The starter pack is `$LEADSTINGER_STARTER_DIR` (or the older `…_GIFT_DIR`), else
//! `<data dir>/Starter` (or `Gift`, or a `Starter.lsset` file), else one next to the
//! executable (`../Resources/Starter` in the macOS bundle, `../share/<slug>/starter` in
//! the Nix package; `Gift`/`gift` there too). It applies once: only to an empty library,
//! and never again after `Starter.applied` (or the older `Gift.applied`) is in the data dir.

use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::decode::AUDIO_EXTENSIONS;
use crate::logic::{first_free_key, normalize_key, set_hotkey};
use crate::model::{Appearance, ImageFit, ImageLayer, Layout, PlayMode, SoundSet, ThemeKind, TileColor};
use crate::store::Store;

/// Written to the data dir once a pack has been imported.
pub const MARKER: &str = "Starter.applied";
/// The marker older builds wrote; it counts too.
const OLD_MARKER: &str = "Gift.applied";

fn applied(data_dir: &Path) -> bool {
    data_dir.join(MARKER).exists() || data_dir.join(OLD_MARKER).exists()
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
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub auto_advance: bool,
    pub tiles: Vec<PackTile>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(rename = "loop", skip_serializing_if = "std::ops::Not::not")]
    pub looping: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume: Option<f32>,
    #[serde(skip_serializing_if = "is_zero")]
    pub fade_in: f32,
    #[serde(skip_serializing_if = "is_zero")]
    pub fade_out: f32,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub solo: bool,
    /// Seconds into the file.
    #[serde(skip_serializing_if = "is_zero64")]
    pub trim_start: f64,
    /// Seconds into the file; none means the end.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trim_end: Option<f64>,
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
    let mut dirs = vec![data_dir.join("Starter"), data_dir.join("Gift"), data_dir.join("Starter.lsset")];
    if let Some(bin) = exe.and_then(Path::parent) {
        for (bundle, share) in [("Starter", "starter"), ("Gift", "gift")] {
            dirs.push(bin.join("../Resources").join(bundle));
            dirs.push(bin.join("../share").join(crate::brand::SLUG).join(share));
        }
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
    let env_dir = crate::brand::env("STARTER_DIR").or_else(|| crate::brand::env("GIFT_DIR")).map(PathBuf::from);
    let found = candidates(env_dir, &store.data_dir, exe.as_deref())
        .into_iter()
        .find(|d| d.is_dir() || (d.is_file() && crate::setfile::is_set_file(d)));
    match found {
        Some(dir) if dir.is_file() => {
            let unpacked = crate::setfile::Unpacked::new(&dir)?;
            preload_from(store, unpacked.root(), &dir)
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
        added += import_set(store, dir, gs, "starter pack").1;
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
    store.mark_dirty();
    store.save_now()?;
    std::fs::write(&marker, format!("{}\n", source.display()))?;
    Ok(Some(added))
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
    p.is_file() && p.extension().and_then(|e| e.to_str()).is_some_and(|e| AUDIO_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
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
fn inside(dir: &Path, file: &str) -> Option<PathBuf> {
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

/// Append one set from a pack and its tiles. Returns the new set's id and how many tiles
/// were added; problems are printed, naming `what` ("starter pack", "set file").
pub(crate) fn import_set(store: &mut Store, dir: &Path, gs: &PackSet, what: &str) -> (Uuid, usize) {
    let slug = crate::brand::SLUG;
    let mut set = SoundSet::named(if gs.name.trim().is_empty() { "Starter" } else { gs.name.trim() });
    if let Some(l) = named::<Layout>(&gs.layout, "layout", &format!("set “{}”", set.name)) {
        set.layout = l;
    }
    if let Some(c) = gs.columns {
        set.columns = c.clamp(1, 12);
    }
    set.auto_advance = gs.auto_advance;
    let set_id = set.id;
    store.library.sets.push(set);
    let si = store.library.sets.len() - 1;
    let mut keyed = Vec::new();
    let mut added = 0;
    for gt in &gs.tiles {
        let Some(path) = inside(dir, &gt.file).filter(|p| p.is_file()) else {
            eprintln!("{slug}: {what} clip {:?} is missing", gt.file);
            continue;
        };
        // One file at a time, so each clip's settings land on its own tile.
        let (ids, errors) = store.import(std::slice::from_ref(&path), si, None);
        for e in errors {
            eprintln!("{slug}: {what} clip: {e}");
        }
        let Some(&id) = ids.first() else { continue };
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
        if let Some(m) = named::<PlayMode>(&gt.mode, "mode", &whose) {
            t.mode = m;
        }
        t.looping = gt.looping;
        t.volume = gt.volume.unwrap_or(1.0).clamp(0.0, 1.0);
        t.fade_in = gt.fade_in.max(0.0);
        t.fade_out = gt.fade_out.max(0.0);
        t.solo = gt.solo;
        t.start = gt.trim_start.max(0.0);
        t.end = gt.trim_end.filter(|e| *e > t.start);
        if let Some(l) = named::<ImageLayer>(&gt.picture_layer, "picture layer", &whose) {
            t.image_layer = l;
        }
        if let Some(f) = named::<ImageFit>(&gt.picture_fit, "picture fit", &whose) {
            t.image_fit = f;
        }
        if let Some(o) = gt.picture_opacity {
            t.image_opacity = o.clamp(0.0, 1.0);
        }
        match gt.hotkey.as_deref().map(str::trim) {
            None => {}
            Some("") => keyed.push((id, None)),
            Some(k) => match k.chars().next().filter(|_| k.chars().count() == 1).and_then(normalize_key) {
                Some(c) => keyed.push((id, Some(c))),
                None => eprintln!("{slug}: {what} {whose}: {k:?} can't be a hotkey"),
            },
        }
    }
    // Hotkeys from the manifest win; tiles that lose their automatic key to one get
    // the next free key.
    let tiles = &mut store.library.sets[si].tiles;
    for (id, key) in &keyed {
        set_hotkey(tiles, *id, *key);
    }
    for i in 0..tiles.len() {
        if tiles[i].hotkey.is_none() && !keyed.iter().any(|(id, _)| *id == tiles[i].id) {
            tiles[i].hotkey = first_free_key(tiles);
        }
    }
    store.mark_dirty();
    (set_id, added)
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
        let gift = root.join("Gift");
        tone(&gift.join("Show/Air Horn.wav"), 440.0);
        tone(&gift.join("Show/Ding.wav"), 660.0);
        tone(&gift.join("Show/Room Tone.wav"), 110.0);
        tone(&gift.join("Music/Rimshot.wav"), 220.0);
        let manifest = json!({
            "format": "sttnger-gift", "version": 1, "theme": "PADD",
            "sets": [
                {"name": "Show", "tiles": [
                    {"file": "Show/Air Horn.wav", "title": "Air Horn", "hotkey": "R", "color": "red", "mode": "restart"},
                    {"file": "Show/Ding.wav", "title": "Ding", "hotkey": "1", "color": "teal", "mode": "hold"},
                    {"file": "Show/Missing.wav", "title": "Not clipped yet"},
                    {"file": "../outside.wav"},
                    {"file": "Show/Room Tone.wav", "color": "Gold", "mode": "toggle", "loop": true, "fade_out": 2.0, "volume": 0.6}
                ]},
                {"name": "Music", "tiles": [{"file": "Music/Rimshot.wav", "hotkey": ""}]},
                {"name": "Empty", "tiles": [{"file": "nope.wav"}]}
            ]
        });
        std::fs::write(gift.join("manifest.json"), manifest.to_string()).unwrap();

        let data = root.join("data");
        let mut store = Store::open(data.clone()).unwrap();
        assert_eq!(preload_from(&mut store, &gift, &gift).unwrap(), Some(4));
        let names: Vec<&str> = store.library.sets.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["Show", "Music"], "sets with no clips are dropped");
        assert_eq!(store.library.selected_set, Some(store.library.sets[0].id));

        let bridge = &store.library.sets[0].tiles;
        let t: Vec<_> = bridge.iter().map(|t| (t.title.as_str(), t.hotkey, t.color, t.mode)).collect();
        assert_eq!(
            t,
            [
                ("Air Horn", Some('r'), TileColor::Red, PlayMode::Restart),
                ("Ding", Some('1'), TileColor::Teal, PlayMode::Hold),
                // Keeps its automatic key; an unknown colour keeps the automatic one.
                ("Room Tone", Some('3'), TileColor::nth(2), PlayMode::Toggle),
            ]
        );
        assert!(bridge[2].looping && bridge[2].fade_out == 2.0 && (bridge[2].volume - 0.6).abs() < 1e-6);
        assert_eq!(store.library.sets[1].tiles[0].hotkey, None, "an empty hotkey means none");
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
        assert_eq!(preload_from(&mut store, &gift, &gift).unwrap(), None, "the marker stops a second import");
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn folders_without_a_manifest_become_sets_and_a_full_library_is_left_alone() {
        let root = temp_dir();
        let gift = root.join("Gift");
        tone(&gift.join("Ship Systems/Transporter.wav"), 500.0);
        tone(&gift.join("Ship Systems/Communicator.wav"), 900.0);
        tone(&gift.join("Loose.wav"), 300.0);
        std::fs::write(gift.join("Ship Systems/notes.txt"), "not audio").unwrap();

        let mut store = Store::open(root.join("data")).unwrap();
        assert_eq!(preload_from(&mut store, &gift, &gift).unwrap(), Some(3));
        let sets: Vec<(&str, Vec<&str>)> =
            store.library.sets.iter().map(|s| (s.name.as_str(), s.tiles.iter().map(|t| t.title.as_str()).collect())).collect();
        assert_eq!(sets, [("Ship Systems", vec!["Communicator", "Transporter"]), ("Starter", vec!["Loose"])]);
        assert_eq!(store.library.theme, ThemeKind::default(), "no manifest: the look is left alone");

        let mut other = Store::open(root.join("other")).unwrap();
        let src = root.join("Mine.wav");
        tone(&src, 250.0);
        other.import(&[src], 0, None);
        assert_eq!(preload_from(&mut other, &gift, &gift).unwrap(), None);
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
        let exe = Path::new("/app/Contents/MacOS/STTNGer");
        assert_eq!(candidates(Some("/g".into()), data, Some(exe)), [PathBuf::from("/g")]);
        assert_eq!(
            candidates(None, data, Some(exe)),
            [
                PathBuf::from("/d/Starter"),
                PathBuf::from("/d/Gift"),
                PathBuf::from("/d/Starter.lsset"),
                PathBuf::from("/app/Contents/MacOS/../Resources/Starter"),
                PathBuf::from(format!("/app/Contents/MacOS/../share/{}/starter", crate::brand::SLUG)),
                PathBuf::from("/app/Contents/MacOS/../Resources/Gift"),
                PathBuf::from(format!("/app/Contents/MacOS/../share/{}/gift", crate::brand::SLUG)),
            ]
        );
    }
}
