//! Farrago set exports (`.farragoset`, D-240..D-243): read one into a pack manifest, so
//! `set.import` brings it in through the same path as a set file (src/setfile.rs), with
//! a report of what didn't come across. docs/FARRAGO-IMPORT.md describes the format in
//! our own words; SPEC.md lists which settings map to what.
//!
//! An export is a folder (a package on macOS), or that folder zipped:
//!
//! ```text
//! Sound Set Data.plist     the set and its tiles (XML or binary property list)
//! Sound Files/<uuid>.<ext> each tile's audio, named by the tile's fileUUID
//! Metadata/<uuid>          a property list per sound: its tags (Title, Artist, …)
//! ```

use std::collections::{BTreeMap, HashSet};
use std::path::{Component, Path, PathBuf};

use anyhow::{bail, Context, Result};
use plist::{Dictionary, Value};
use serde::Serialize;

use crate::model::{OutputDef, TileColor, MAIN_OUTPUT};
use crate::starter::{Manifest, PackSet, PackTile};

/// The export's extension.
pub const EXTENSION: &str = "farragoset";
/// The set's property list, at the top of the export.
const DATA: &str = "Sound Set Data.plist";
const SOUNDS: &str = "Sound Files";
const METADATA: &str = "Metadata";
/// The newest export format this reader was written against (`formatVersion`).
const FORMAT_VERSION: f64 = 2.0;
/// LeadStinger's widest grid (`SoundSet::columns` is clamped to it).
const MAX_COLUMNS: i64 = 12;

/// Farrago's tile colours by `colorIndex`, matched to ours by nearest hue (D-243).
/// All eight were checked in Farrago 2.2 (D-250); a set with an index past them crashes it.
pub const COLORS: [TileColor; 8] = [
    TileColor::Purple,
    TileColor::Pink,
    TileColor::Red,
    TileColor::Orange,
    TileColor::Yellow,
    TileColor::Green,
    TileColor::Teal,
    TileColor::Blue,
];

/// Keys this reader knows, mapped or deliberately dropped; anything else is reported.
const SET_KEYS: &[&str] = &[
    "automations", "crossfadeDuration", "formatMinSupportedVersion", "formatVersion", "maxRowsGrid", "maxRowsList", "mode", "notes",
    "playbackMode", "position", "setWindowState", "splitViewState", "tiles", "title", "type", "uuid",
];
const TILE_KEYS: &[&str] = &[
    "automations", "colorIndex", "editorModelState", "editorWindowState", "favorite", "fileName", "fileUUID", "gridPositionX", "gridPositionY",
    "holdToPlay", "lastPlayStart", "lifetimePlayCount", "listPositionY", "notes", "notesShown", "playbackEnabled", "playerSettings", "selected",
    "solo", "tileIcon", "tileUUID", "title",
];
const PLAYER_KEYS: &[&str] = &[
    "channels", "defaultToPause", "ducked", "duckedVolume", "duration", "endPackets", "fadeInTime", "fadeOutTime", "fullVolume", "loop", "muted",
    "packetCount", "startPackets",
];

/// What a Farrago import brought in and what it left behind: `set.import`'s `farrago`
/// answer and the window's notice.
#[derive(Debug, Default, Serialize)]
pub struct Report {
    /// The tiles imported, by title, in set order.
    pub imported: Vec<String>,
    /// Tiles left out, and why.
    pub skipped: Vec<Skipped>,
    /// Settings with no LeadStinger equivalent that some tiles had, with how many.
    pub dropped: Vec<String>,
    /// Unknown keys and other things worth knowing.
    pub warnings: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct Skipped {
    pub title: String,
    pub reason: String,
}

/// A `.farragoset` by its name (folder, package or file).
pub fn is_export(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case(EXTENSION))
}

/// Where the export's data is: `dir` itself, or the one folder inside it (a zipped export).
pub fn root(dir: &Path) -> Option<PathBuf> {
    if dir.join(DATA).is_file() {
        return Some(dir.to_path_buf());
    }
    let entries: Vec<PathBuf> = std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    match entries.as_slice() {
        [only] if only.join(DATA).is_file() => Some(only.clone()),
        _ => None,
    }
}

/// The set's name: the export's file name (what the user called it when exporting),
/// less `.zip` and `.farragoset`; the plist's own title only when that is empty (D-242).
pub fn set_name(path: &Path, root: &Path, plist_title: Option<&str>) -> String {
    let stem = |p: &Path| -> Option<String> {
        let mut name = p.file_name()?.to_string_lossy().into_owned();
        for ext in [".zip", ".farragoset"] {
            if name.len() > ext.len() && name.to_ascii_lowercase().ends_with(ext) {
                name.truncate(name.len() - ext.len());
            }
        }
        Some(name.trim().to_owned()).filter(|n| !n.is_empty())
    };
    let inner = is_export(root).then(|| stem(root)).flatten();
    inner.or_else(|| stem(path)).or_else(|| plist_title.map(str::trim).filter(|t| !t.is_empty()).map(str::to_owned)).unwrap_or_else(|| "Farrago Set".into())
}

/// One tile as read from the plist.
struct Read {
    tile: PackTile,
    grid: Option<(i64, i64)>,
    list: f64,
    order: usize,
}

/// Read the export at `root` into a one-set manifest (files relative to `root`) and a
/// report. Tiles whose sound is missing or unreadable are left out (a blank keeps their
/// grid slot); everything else that doesn't map is listed. `outputs` are the library's,
/// which a tile's output channel choice is matched against ([`output_for`]).
pub(crate) fn convert(path: &Path, root: &Path, outputs: &[OutputDef]) -> Result<(Manifest, Report)> {
    let data = root.join(DATA);
    let value = Value::from_file(&data).with_context(|| format!("reading {DATA}"))?;
    let Some(set) = value.as_dictionary() else { bail!("{DATA} isn't a dictionary") };
    let mut report = Report::default();
    let mut dropped: BTreeMap<String, usize> = BTreeMap::new();
    let mut unknown: BTreeMap<String, usize> = BTreeMap::new();
    if real(set, "formatMinSupportedVersion").is_some_and(|v| v > FORMAT_VERSION) {
        report.warnings.push("made by a newer Farrago: imported what this version understands".into());
    }
    note_unknown(set, SET_KEYS, "", &mut unknown);
    if set.get("automations").is_some_and(has_automations) {
        *dropped.entry("set automations".into()).or_default() += 1;
    }
    let list_mode = int(set, "mode") == Some(1);
    let name = set_name(path, root, string(set, "title"));

    let tiles = set.get("tiles").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
    let mut read = Vec::new();
    let mut any_ducked = false;
    for (order, v) in tiles.iter().enumerate() {
        let Some(t) = v.as_dictionary() else {
            report.warnings.push(format!("tile {} isn't a dictionary", order + 1));
            continue;
        };
        note_unknown(t, TILE_KEYS, "", &mut unknown);
        let uuid = string(t, "fileUUID").filter(|u| plain(u));
        let file_name = string(t, "fileName").filter(|f| plain(f));
        let title = title(root, t, uuid, file_name, order);
        let Some(sound) = sound_file(root, uuid, file_name) else {
            report.skipped.push(Skipped { title, reason: "its sound file isn't in the export".into() });
            continue;
        };
        if let Err(e) = crate::transcode::plan(&root.join(&sound)) {
            report.skipped.push(Skipped { title, reason: e });
            continue;
        }
        let mut tile = PackTile { file: sound, title: Some(title), hotkey: None, ..Default::default() };
        if let Some(i) = int(t, "colorIndex") {
            match usize::try_from(i).ok().and_then(|i| COLORS.get(i)) {
                Some(c) => tile.color = serde_json::to_value(c).ok().and_then(|v| v.as_str().map(str::to_owned)),
                None => report.warnings.push(format!("“{}”: colour {i} is unknown; it gets a colour of its own", tile.title.as_deref().unwrap_or_default())),
            }
        }
        tile.hold = boolean(t, "holdToPlay").filter(|h| *h);
        tile.disabled = boolean(t, "playbackEnabled") == Some(false);
        tile.solo = boolean(t, "solo").unwrap_or(false);
        tile.notes = string(t, "notes").unwrap_or_default().to_owned();
        if boolean(t, "favorite") == Some(true) {
            *dropped.entry("favourite".into()).or_default() += 1;
        }
        // Its emoji, one string each, joined into the tile's emoji (D-253); anything that
        // isn't an emoji is left out and counted.
        if let Some(icons) = t.get("tileIcon").and_then(Value::as_array) {
            let (emoji, other): (Vec<&str>, Vec<&Value>) = icons.iter().fold((Vec::new(), Vec::new()), |(mut e, mut o), v| {
                match v.as_string().map(str::trim).filter(|s| crate::model::is_emoji(s)) {
                    Some(s) => e.push(s),
                    None => o.push(v),
                }
                (e, o)
            });
            tile.emoji = crate::model::emoji_only(&emoji.concat());
            if !other.is_empty() {
                *dropped.entry("a tile icon that isn't an emoji".into()).or_default() += 1;
            }
        }
        if t.get("automations").is_some_and(has_automations) {
            *dropped.entry("automations".into()).or_default() += 1;
        }
        if int(t, "lifetimePlayCount").is_some_and(|n| n > 0) {
            *dropped.entry("play count and last played".into()).or_default() += 1;
        }
        if let Some(p) = t.get("playerSettings").and_then(Value::as_dictionary) {
            note_unknown(p, PLAYER_KEYS, "playerSettings.", &mut unknown);
            tile.pausable = boolean(p, "defaultToPause").filter(|p| *p);
            tile.looping = boolean(p, "loop").unwrap_or(false);
            if let Some(v) = real(p, "fullVolume").filter(|v| v.is_finite() && *v >= 0.0) {
                tile.volume = Some(v.min(1.0) as f32);
                if v > 1.0 {
                    tile.gain_db = (20.0 * v.log10()) as f32;
                }
            }
            tile.fade_in = real(p, "fadeInTime").filter(|f| f.is_finite()).unwrap_or(0.0).max(0.0) as f32;
            tile.fade_out = real(p, "fadeOutTime").filter(|f| f.is_finite()).unwrap_or(0.0).max(0.0) as f32;
            if boolean(p, "ducked") == Some(true) {
                tile.duck = Some("ducked".into());
                any_ducked = true;
                tile.duck_db = Some(ducked_db(real(p, "duckedVolume")));
            }
            tile.muted = boolean(p, "muted").unwrap_or(false);
            // Its output channels, onto the library's output on that pair (D-254).
            if let Some(pair) = channels(p) {
                match output_for(pair, outputs) {
                    Some(name) if name != MAIN_OUTPUT => tile.output = Some(name.to_owned()),
                    Some(_) => {}
                    None => *dropped.entry(format!("channels {}/{}: no matching output", pair.0, pair.1)).or_default() += 1,
                }
            }
            // Trim, in packets of the source file: from the start, and cut off the end.
            let (dur, packets) = (real(p, "duration").unwrap_or(0.0), int(p, "packetCount").unwrap_or(0));
            if dur.is_finite() && dur > 0.0 && packets > 0 {
                let per = dur / packets as f64;
                let start = int(p, "startPackets").unwrap_or(0).clamp(0, packets);
                let cut = int(p, "endPackets").unwrap_or(0).clamp(0, packets);
                tile.trim_start = start as f64 * per;
                if cut > 0 && cut + start < packets {
                    tile.trim_end = Some(dur - cut as f64 * per);
                }
            }
        }
        let pos = |k: &str| real(t, k).filter(|v| v.is_finite()).map(|v| v.round() as i64);
        let grid = pos("gridPositionX").zip(pos("gridPositionY"));
        let list = real(t, "listPositionY").filter(|v| v.is_finite()).unwrap_or(order as f64);
        read.push(Read { tile, grid, list, order });
    }
    // In Farrago every other tile dips a ducked one; in LeadStinger only "Ducks beds"
    // tiles do, so the others take that part (D-240).
    if any_ducked {
        for r in &mut read {
            if r.tile.duck.is_none() {
                r.tile.duck = Some("ducks".into());
            }
        }
    }
    for (what, n) in dropped {
        report.dropped.push(format!("{what} ({n} tile{})", if n == 1 { "" } else { "s" }));
    }
    for (key, n) in unknown {
        report.warnings.push(format!("unknown setting {key} ({n}×), left out"));
    }

    let mut pack = PackSet {
        name,
        layout: Some(if list_mode { "list" } else { "grid" }.into()),
        auto_advance: int(set, "playbackMode") == Some(2),
        crossfade: real(set, "crossfadeDuration").filter(|c| c.is_finite() && *c > 0.0).unwrap_or(0.0) as f32,
        notes: string(set, "notes").unwrap_or_default().to_owned(),
        ..Default::default()
    };
    let shape = (!list_mode).then(|| grid_shape(&read)).flatten();
    match shape {
        Some((columns, rows, blanks, order)) => {
            pack.columns = Some(columns);
            pack.rows = Some(rows);
            pack.blanks = blanks;
            let mut by_order: Vec<Option<Read>> = read.into_iter().map(Some).collect();
            pack.tiles = order.into_iter().filter_map(|i| by_order[i].take()).map(|r| r.tile).collect();
        }
        None => {
            if !list_mode && !read.is_empty() {
                report.warnings.push("the grid positions overlap or are too wide; tiles are in list order".into());
            }
            read.sort_by(|a, b| a.list.total_cmp(&b.list).then(a.order.cmp(&b.order)));
            pack.tiles = read.into_iter().map(|r| r.tile).collect();
        }
    }
    report.imported = pack.tiles.iter().filter_map(|t| t.title.clone()).collect();
    Ok((Manifest { sets: vec![pack], ..Default::default() }, report))
}

/// The grid the tiles' positions make: columns, tiles per row, blank slots and the
/// tiles (indices into `read`) in reading order. None when positions are missing,
/// negative or shared, or the grid is wider than LeadStinger's.
#[allow(clippy::type_complexity)]
fn grid_shape(read: &[Read]) -> Option<(u32, Vec<u32>, Vec<[u32; 2]>, Vec<usize>)> {
    let mut placed: Vec<(i64, i64, usize)> = Vec::new();
    for (i, r) in read.iter().enumerate() {
        let (x, y) = r.grid?;
        if x < 0 || y < 0 || x >= MAX_COLUMNS {
            return None;
        }
        placed.push((y, x, i));
    }
    let mut seen = HashSet::new();
    if !placed.iter().all(|(y, x, _)| seen.insert((*y, *x))) {
        return None;
    }
    placed.sort_unstable();
    let columns = placed.iter().map(|p| p.1 + 1).max().unwrap_or(1);
    let last_row = placed.last().map_or(0, |p| p.0);
    let mut rows = Vec::new();
    let mut blanks = Vec::new();
    for y in 0..=last_row {
        let xs: Vec<i64> = placed.iter().filter(|p| p.0 == y).map(|p| p.1).collect();
        rows.push(xs.len() as u32);
        let end = xs.last().copied().unwrap_or(-1);
        blanks.extend((0..end).filter(|x| !xs.contains(x)).map(|x| [y as u32, x as u32]));
    }
    Some((columns as u32, rows, blanks, placed.into_iter().map(|p| p.2).collect()))
}

/// The tile's title: the plist's, else the sound's Title tag, else its file name
/// (unless that is only an id), else "Sound N".
fn title(root: &Path, t: &Dictionary, uuid: Option<&str>, file_name: Option<&str>, order: usize) -> String {
    let given = string(t, "title").map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned);
    given
        .or_else(|| uuid.and_then(|u| tag_title(&root.join(METADATA).join(u))))
        .or_else(|| {
            let stem = Path::new(file_name?).file_stem()?.to_string_lossy().trim().to_owned();
            (!stem.is_empty() && !looks_like_uuid(&stem)).then_some(stem)
        })
        .unwrap_or_else(|| format!("Sound {}", order + 1))
}

/// The Title tag in a sound's metadata plist.
fn tag_title(path: &Path) -> Option<String> {
    let v = Value::from_file(path).ok()?;
    let tags = v.as_dictionary()?.get("tags")?.as_dictionary()?;
    tags.get("Title")?.as_string().map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned)
}

/// The tile's sound, relative to `root`: `Sound Files/<fileUUID>.<any extension>`, or
/// the file under its `fileName`.
fn sound_file(root: &Path, uuid: Option<&str>, file_name: Option<&str>) -> Option<String> {
    let dir = root.join(SOUNDS);
    if let Some(u) = uuid {
        let mut found: Vec<PathBuf> = std::fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_file() && p.file_stem().is_some_and(|s| s.to_string_lossy().eq_ignore_ascii_case(u)))
            .collect();
        found.sort();
        if let Some(p) = found.first() {
            return Some(format!("{SOUNDS}/{}", p.file_name()?.to_string_lossy()));
        }
    }
    let f = file_name?;
    dir.join(f).is_file().then(|| format!("{SOUNDS}/{f}"))
}

fn looks_like_uuid(s: &str) -> bool {
    s.len() == 36 && s.chars().enumerate().all(|(i, c)| if [8, 13, 18, 23].contains(&i) { c == '-' } else { c.is_ascii_hexdigit() })
}

/// A single file name, with nothing that could leave the folder.
fn plain(name: &str) -> bool {
    let p = Path::new(name);
    !name.is_empty() && p.components().count() == 1 && matches!(p.components().next(), Some(Component::Normal(_)))
}

/// A ducked tile's own level in dB, from Farrago's `duckedVolume`: the linear factor it
/// dips to (0.4 when missing, Farrago's default), so 0.4 is −8 dB and 0 the deepest duck.
fn ducked_db(volume: Option<f64>) -> f32 {
    let v = volume.filter(|v| v.is_finite()).unwrap_or(0.4).clamp(0.0, 1.0);
    let db = if v > 0.0 { (20.0 * v.log10() * 10.0).round() / 10.0 } else { f64::from(crate::model::DUCK_DB_MIN) };
    crate::model::clamp_duck_db(db as f32)
}

/// A tile's `playerSettings.channels`, left and right, when it chose any: 0 is Auto (the
/// device's first two), otherwise an output channel from 1. None for Auto on both.
fn channels(p: &Dictionary) -> Option<(i64, i64)> {
    let c = p.get("channels")?.as_array()?;
    let at = |i: usize| c.get(i).and_then(Value::as_signed_integer).unwrap_or(0);
    let pair = (at(0), at(1));
    (pair != (0, 0)).then_some(pair)
}

/// The output a channel choice plays on (D-254): an adjacent pair, left `n` and right
/// `n + 1`, is the output whose first channel is `n`; channels 1 and 2 are Main, and
/// otherwise the library's first output on `n`. A mono choice (both the same), a gap,
/// right before left or Auto on one side has none. Every output is a channel pair on its
/// device, and the export doesn't say which device the tile used, so only the channel
/// numbers are matched.
fn output_for((left, right): (i64, i64), outputs: &[OutputDef]) -> Option<&str> {
    if left < 1 || right != left + 1 {
        return None;
    }
    if left == 1 {
        return Some(MAIN_OUTPUT);
    }
    outputs.iter().find(|o| i64::from(o.channel) == left).map(|o| o.name.as_str())
}

fn has_automations(v: &Value) -> bool {
    v.as_dictionary().and_then(|d| d.get("modelItems")).and_then(Value::as_array).is_some_and(|a| !a.is_empty())
}

fn note_unknown(d: &Dictionary, known: &[&str], prefix: &str, out: &mut BTreeMap<String, usize>) {
    for k in d.keys() {
        if !known.contains(&k.as_str()) {
            *out.entry(format!("{prefix}{k}")).or_default() += 1;
        }
    }
}

fn string<'a>(d: &'a Dictionary, k: &str) -> Option<&'a str> {
    d.get(k)?.as_string()
}

fn boolean(d: &Dictionary, k: &str) -> Option<bool> {
    d.get(k)?.as_boolean()
}

fn int(d: &Dictionary, k: &str) -> Option<i64> {
    match d.get(k)? {
        Value::Integer(i) => i.as_signed(),
        Value::Real(r) if r.is_finite() => Some(r.round() as i64),
        _ => None,
    }
}

fn real(d: &Dictionary, k: &str) -> Option<f64> {
    match d.get(k)? {
        Value::Real(r) => Some(*r),
        Value::Integer(i) => i.as_signed().map(|i| i as f64),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Duck, Layout, PressOptions};
    use crate::store::Store;
    use plist::Integer;
    use std::io::Write;

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("sb-farrago-{tag}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn dict(pairs: Vec<(&str, Value)>) -> Value {
        Value::Dictionary(pairs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
    }

    fn int(i: i64) -> Value {
        Value::Integer(Integer::from(i))
    }

    #[allow(clippy::too_many_arguments)]
    fn tile(uuid: &str, file: &str, title: &str, x: f64, y: f64, list: f64, extra: Vec<(&str, Value)>, player: Vec<(&str, Value)>) -> Value {
        let mut settings = vec![
            ("channels", Value::Array(vec![int(0), int(0)])),
            ("defaultToPause", false.into()),
            ("ducked", false.into()),
            ("duckedVolume", 0.4.into()),
            ("duration", 0.3.into()),
            ("startPackets", int(0)),
            ("endPackets", int(0)),
            ("packetCount", int(30)),
            ("fadeInTime", 0.0.into()),
            ("fadeOutTime", 0.0.into()),
            ("fullVolume", 1.0.into()),
            ("loop", false.into()),
            ("muted", false.into()),
        ];
        for (k, v) in player {
            settings.retain(|(n, _)| *n != k);
            settings.push((k, v));
        }
        let mut pairs = vec![
            ("automations", dict(vec![("modelItems", Value::Array(vec![]))])),
            ("colorIndex", int(0)),
            ("favorite", false.into()),
            ("fileName", file.into()),
            ("fileUUID", uuid.into()),
            ("gridPositionX", x.into()),
            ("gridPositionY", y.into()),
            ("holdToPlay", false.into()),
            ("lastPlayStart", 0.0.into()),
            ("lifetimePlayCount", int(0)),
            ("listPositionY", list.into()),
            ("notes", "".into()),
            ("playbackEnabled", true.into()),
            ("playerSettings", dict(settings)),
            ("solo", false.into()),
            ("tileIcon", Value::Array(vec![])),
            ("title", title.into()),
        ];
        for (k, v) in extra {
            pairs.retain(|(n, _)| *n != k);
            pairs.push((k, v));
        }
        dict(pairs)
    }

    const A: &str = "11111111-2222-3333-4444-555555555555";
    const B: &str = "AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE";
    const GONE: &str = "99999999-8888-7777-6666-555555555555";

    /// A two-sound export (plus a tile whose sound is missing), binary or XML.
    fn export(dir: &Path, list_mode: bool, binary: bool) {
        std::fs::create_dir_all(dir.join(SOUNDS)).unwrap();
        std::fs::create_dir_all(dir.join(METADATA)).unwrap();
        crate::wav::write_tone(&dir.join(SOUNDS).join(format!("{A}.wav")), 8000, 0.3, &[440.0], 0.4).unwrap();
        crate::wav::write_tone(&dir.join(SOUNDS).join(format!("{B}.wav")), 8000, 0.3, &[660.0], 0.4).unwrap();
        dict(vec![("tags", dict(vec![("Title", "Door Slam".into())])), ("fileUUID", B.into())]).to_file_binary(dir.join(METADATA).join(B)).unwrap();
        let tiles = vec![
            tile(
                A,
                "first.wav",
                "Horn",
                0.0,
                0.0,
                2.0,
                vec![
                    ("colorIndex", int(5)),
                    ("holdToPlay", true.into()),
                    ("favorite", true.into()),
                    ("notes", "cue 1".into()),
                    ("tileIcon", Value::Array(vec!["🌲".into(), "🐕\u{200D}\u{1F9BA}".into(), "not an emoji".into(), int(7)])),
                ],
                vec![
                    ("fullVolume", 0.5.into()),
                    ("fadeInTime", 0.25.into()),
                    ("loop", true.into()),
                    ("ducked", true.into()),
                    ("duckedVolume", 0.2.into()),
                    ("startPackets", int(3)),
                    ("endPackets", int(6)),
                    ("channels", Value::Array(vec![int(3), int(4)])),
                ],
            ),
            tile(GONE, "gone.wav", "Missing", 1.0, 0.0, 0.0, vec![], vec![]),
            // No title of its own: the Metadata tag names it.
            tile(B, "second.wav", "", 2.0, 0.0, 1.0, vec![("colorIndex", int(2)), ("playbackEnabled", false.into()), ("solo", true.into()), ("sparkle", true.into())], vec![("defaultToPause", true.into()), ("muted", true.into()), ("channels", Value::Array(vec![int(5), int(6)]))]),
        ];
        let set = dict(vec![
            ("formatVersion", 2.0.into()),
            ("formatMinSupportedVersion", 2.0.into()),
            ("mode", int(i64::from(list_mode))),
            ("playbackMode", int(if list_mode { 2 } else { 1 })),
            ("crossfadeDuration", if list_mode { 0.5 } else { 0.0 }.into()),
            ("notes", "Opening night".into()),
            ("title", "Sound Set".into()),
            ("tiles", Value::Array(tiles)),
        ]);
        if binary {
            set.to_file_binary(dir.join(DATA)).unwrap();
        } else {
            set.to_file_xml(dir.join(DATA)).unwrap();
        }
    }

    #[test]
    fn settings_map_onto_tiles_and_the_rest_is_reported() {
        let root = temp_dir("convert");
        let dir = root.join("Show.farragoset");
        export(&dir, false, true);
        let booth = OutputDef { name: "Booth".into(), device: Some("Interface".into()), channel: 3 };
        let (manifest, report) = convert(&dir, &dir, std::slice::from_ref(&booth)).unwrap();
        let set = &manifest.sets[0];
        assert_eq!((set.name.as_str(), set.layout.as_deref(), set.notes.as_str()), ("Show", Some("grid"), "Opening night"), "named after the file");
        // The missing tile's slot stays a blank between the two.
        assert_eq!((set.columns, set.rows.as_deref(), set.blanks.as_slice()), (Some(3), Some(&[2][..]), &[[0, 1]][..]));
        let [horn, door] = set.tiles.as_slice() else { panic!("two tiles: {:?}", set.tiles) };
        assert_eq!((horn.title.as_deref(), horn.color.as_deref(), horn.hold, horn.pausable), (Some("Horn"), Some("green"), Some(true), None));
        assert_eq!((horn.volume, horn.fade_in, horn.looping, horn.duck.as_deref(), horn.notes.as_str()), (Some(0.5), 0.25, true, Some("ducked"), "cue 1"));
        assert_eq!(horn.duck_db, Some(-14.0), "its own ducked level, 0.2 as dB");
        assert!((horn.trim_start - 0.03).abs() < 1e-9 && horn.trim_end.is_some_and(|e| (e - 0.24).abs() < 1e-9), "{} {:?}", horn.trim_start, horn.trim_end);
        assert_eq!(horn.hotkey, None, "keys follow the rows, as Farrago's do");
        assert_eq!((door.title.as_deref(), door.color.as_deref(), door.disabled, door.solo, door.pausable), (Some("Door Slam"), Some("red"), true, true, Some(true)));
        assert_eq!(door.duck.as_deref(), Some("ducks"), "in a set with a ducked tile the others duck it");
        assert_eq!((horn.muted, door.muted, door.volume), (false, true, Some(1.0)), "muted, at its own volume (D-251)");
        assert_eq!(report.imported, ["Horn", "Door Slam"]);
        assert_eq!(report.skipped.len(), 1);
        assert_eq!((report.skipped[0].title.as_str(), report.skipped[0].reason.as_str()), ("Missing", "its sound file isn't in the export"));
        assert_eq!((horn.emoji.as_str(), door.emoji.as_str()), ("🌲🐕\u{200D}\u{1F9BA}", ""), "the emoji, joined; the rest left out");
        assert_eq!((horn.output.as_deref(), door.output.as_deref()), (Some("Booth"), None), "channels 3/4 are Booth; 5/6 has no output");
        assert_eq!(report.dropped, ["a tile icon that isn't an emoji (1 tile)", "channels 5/6: no matching output (1 tile)", "favourite (1 tile)"]);
        assert_eq!(report.warnings, ["unknown setting sparkle (1×), left out"]);

        // List mode: list order, Continuous with its cross-fade; an XML plist reads too.
        let list = root.join("Cues.farragoset");
        export(&list, true, false);
        let (manifest, _) = convert(&list, &list, &[]).unwrap();
        let set = &manifest.sets[0];
        assert_eq!((set.layout.as_deref(), set.auto_advance, set.crossfade, set.rows.as_ref()), (Some("list"), true, 0.5, None));
        let titles: Vec<_> = set.tiles.iter().map(|t| t.title.as_deref().unwrap()).collect();
        assert_eq!(titles, ["Door Slam", "Horn"], "by list position");
        std::fs::remove_dir_all(root).ok();
    }

    /// Farrago 2.2's eight colours, each set by colorIndex and seen in its window (D-250).
    #[test]
    fn every_farrago_colour_maps_by_hue() {
        let dir = temp_dir("colours").join("Colours.farragoset");
        std::fs::create_dir_all(dir.join(SOUNDS)).unwrap();
        crate::wav::write_tone(&dir.join(SOUNDS).join(format!("{A}.wav")), 8000, 0.3, &[440.0], 0.4).unwrap();
        let tiles = (0..9).map(|i| tile(A, "tone.wav", &format!("c{i}"), i as f64, 0.0, i as f64, vec![("colorIndex", int(i))], vec![])).collect();
        dict(vec![("formatVersion", 2.0.into()), ("mode", int(0)), ("title", "Colours".into()), ("tiles", Value::Array(tiles))])
            .to_file_binary(dir.join(DATA))
            .unwrap();
        let (manifest, report) = convert(&dir, &dir, &[]).unwrap();
        let colours: Vec<_> = manifest.sets[0].tiles.iter().map(|t| t.color.as_deref()).collect();
        let expected = ["purple", "pink", "red", "orange", "yellow", "green", "teal", "blue"];
        assert_eq!(colours[..8], expected.map(Some), "Farrago's purple, pink, red, orange, yellow, green, teal, blue");
        assert_eq!(colours[8], None, "past the palette: a colour of its own");
        assert_eq!(report.warnings, ["“c8”: colour 8 is unknown; it gets a colour of its own"]);
        std::fs::remove_dir_all(dir.parent().unwrap()).ok();
    }

    #[test]
    fn overlapping_or_wide_grids_fall_back_to_list_order() {
        let r = |x: i64, y: i64, list: f64, order: usize| Read { tile: PackTile::default(), grid: Some((x, y)), list, order };
        assert!(grid_shape(&[r(0, 0, 0.0, 0), r(0, 0, 1.0, 1)]).is_none(), "two tiles in one slot");
        assert!(grid_shape(&[r(12, 0, 0.0, 0)]).is_none(), "wider than 12");
        assert!(grid_shape(&[r(-1, 0, 0.0, 0)]).is_none());
        let (cols, rows, blanks, order) = grid_shape(&[r(1, 2, 0.0, 0), r(0, 0, 1.0, 1), r(3, 0, 2.0, 2)]).unwrap();
        assert_eq!((cols, rows, blanks, order), (4, vec![2, 0, 1], vec![[0, 1], [0, 2], [2, 0]], vec![1, 2, 0]));
    }

    #[test]
    fn ducked_volume_is_the_beds_own_level_in_db() {
        assert_eq!(ducked_db(Some(0.4)), -8.0, "Farrago's default");
        assert_eq!(ducked_db(None), -8.0, "missing reads as the default");
        assert_eq!(ducked_db(Some(0.5)), -6.0);
        assert_eq!(ducked_db(Some(1.0)), 0.0, "no dip");
        assert_eq!(ducked_db(Some(0.0)), -40.0, "silent is the deepest duck");
        assert_eq!(ducked_db(Some(0.001)), -40.0, "clamped to −40 dB");
        assert_eq!(ducked_db(Some(3.0)), 0.0, "above 1 doesn't boost");
        assert_eq!(ducked_db(Some(f64::NAN)), -8.0);
    }

    /// Farrago's channel choice onto an output (D-254): an adjacent pair by its first
    /// channel, 1/2 being Main; anything else has none.
    #[test]
    fn channel_pairs_match_an_output_by_its_first_channel() {
        let out = |name: &str, channel| OutputDef { name: name.into(), device: None, channel };
        let outputs = [out("Phones", 3), out("Booth", 5), out("Also 3", 3)];
        assert_eq!(output_for((1, 2), &outputs), Some(MAIN_OUTPUT));
        assert_eq!(output_for((3, 4), &outputs), Some("Phones"), "the first on that pair");
        assert_eq!(output_for((5, 6), &outputs), Some("Booth"));
        for pair in [(7, 8), (3, 3), (3, 5), (4, 3), (0, 2), (3, 0), (-1, 0)] {
            assert_eq!(output_for(pair, &outputs), None, "{pair:?}");
        }
        let player = |c: Vec<Value>| dict(vec![("channels", Value::Array(c))]).into_dictionary().unwrap();
        assert_eq!(channels(&player(vec![int(0), int(0)])), None, "Auto");
        assert_eq!(channels(&player(vec![int(3), int(4)])), Some((3, 4)));
        assert_eq!(channels(&player(vec![int(3)])), Some((3, 0)), "a missing right is Auto");
        assert_eq!(channels(&dict(vec![]).into_dictionary().unwrap()), None);
    }

    #[test]
    fn names() {
        let p = Path::new;
        assert_eq!(set_name(p("/x/Talent Show.farragoset"), p("/x/Talent Show.farragoset"), Some("Sound Set")), "Talent Show");
        assert_eq!(set_name(p("/x/Talent.farragoset.zip"), p("/tmp/u"), None), "Talent");
        assert_eq!(set_name(p("/x/a.zip"), p("/tmp/u/Inner.farragoset"), None), "Inner", "the zipped folder's name wins");
        assert_eq!(set_name(p("/"), p("/"), Some(" Bits ")), "Bits");
        assert!(looks_like_uuid(A) && !looks_like_uuid("ringtone"));
        assert!(plain("a.m4a") && !plain("../a.m4a") && !plain("a/b") && !plain(""));
    }

    #[test]
    fn imports_into_a_library_as_a_folder_or_a_zip() {
        let root = temp_dir("import");
        let dir = root.join("Show.farragoset");
        export(&dir, false, true);
        let mut store = Store::open_with(root.join("lib"), false).unwrap();
        store.library.outputs.push(OutputDef { name: "Booth".into(), device: None, channel: 3 });
        assert!(crate::setfile::is_set_file(&dir));
        let out = crate::setfile::import(&mut store, &dir).unwrap();
        store.finish_imports();
        assert_eq!(out.sets.len(), 1);
        assert_eq!((out.sets[0].name.as_str(), out.sets[0].tiles), ("Show", 2));
        assert_eq!(out.farrago.as_ref().unwrap().skipped.len(), 1);
        let set = &store.library.sets[store.library.set_index(out.sets[0].id).unwrap()];
        assert_eq!(set.layout, Layout::Grid);
        assert_eq!(set.blanks, vec![[0, 1]], "the missing tile's slot");
        let t: Vec<_> = set.tiles.iter().map(|t| (t.title.as_str(), t.color, t.press, t.disabled, t.duck, t.looping)).collect();
        assert_eq!(
            t,
            [
                ("Horn", TileColor::Green, PressOptions { hold: true, pausable: false, one_shot: false }, false, Duck::Ducked, true),
                ("Door Slam", TileColor::Red, PressOptions { hold: false, pausable: true, one_shot: false }, true, Duck::Ducks, false),
            ]
        );
        assert_eq!((set.tiles[0].duck_db, set.tiles[1].duck_db), (Some(-14.0), None), "the bed keeps its own ducked level");
        assert_eq!((set.tiles[0].muted, set.tiles[1].muted), (false, true), "Door Slam is muted");
        assert_eq!(set.tiles[0].emoji, "🌲🐕\u{200D}\u{1F9BA}", "Horn's emoji");
        assert_eq!((set.tiles[0].output(), set.tiles[1].output()), ("Booth", MAIN_OUTPUT), "Horn plays on the library's Booth (channels 3/4)");
        assert!(out.farrago.as_ref().unwrap().dropped.iter().any(|d| d.starts_with("channels 5/6: no matching output")));
        assert!(set.tiles.iter().all(|t| store.media_dir.join(t.file.as_deref().unwrap()).is_file()), "sounds copied into the library");

        // Zipped (what Finder's Compress makes of the package): the folder inside the zip.
        let zip = root.join("Show.zip");
        let mut w = zip::ZipWriter::new(std::fs::File::create(&zip).unwrap());
        for entry in walk(&dir) {
            let rel = entry.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
            w.start_file(rel, zip::write::SimpleFileOptions::default()).unwrap();
            w.write_all(&std::fs::read(&entry).unwrap()).unwrap();
        }
        w.finish().unwrap();
        let out = crate::setfile::import(&mut store, &zip).unwrap();
        store.finish_imports();
        assert_eq!((out.sets[0].name.as_str(), out.sets[0].tiles), ("Show 2", 2), "a second copy gets a number");

        // Nothing playable: an error that says why.
        std::fs::remove_dir_all(dir.join(SOUNDS)).unwrap();
        let err = crate::setfile::import(&mut store, &dir).err().unwrap().to_string();
        assert!(err.contains("isn't in the export"), "{err}");
        std::fs::remove_dir_all(root).ok();
    }

    fn walk(dir: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() { out.extend(walk(&p)) } else { out.push(p) }
        }
        out
    }
}
