//! Set files (`.lsset`): one or more sets with their clips and pictures in a single file
//! that another library can import (D-039).
//!
//! A set file is a zip of a pack folder (src/starter.rs, starter/README.md):
//!
//! ```text
//! manifest.json            {"format": "leadstinger-set", "version": 3, "sets": [...]}
//! <Set name>/01 Title.flac  each set's clips, in tile order
//! <Set name>/pictures/…     tile pictures chosen by the user
//! <id>.lstheme/…            the user theme in use when it was exported, if any
//! ```
//!
//! so an unzipped set file is a valid starter pack, and a zipped pack folder is a valid
//! set file. Importing appends sets and never replaces anything; exporting writes every
//! tile setting. Clips are stored as they are in the library (already compressed, so
//! they aren't deflated again).
//!
//! A theme travels as the manifest's `theme_file` (version 2, D-173): importing installs
//! it as a user theme, and the window asks whether to use it now. Version 1 files have
//! none, and older builds skip the theme and import the sets.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::Serialize;
use uuid::Uuid;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::logic::PadKey;
use crate::model::{ActionKind, CueLink, Duck, FadeCurve, Target, TileAction};
use crate::starter::{import_set, load_manifest, pack_root, Manifest, PackAction, PackSet, PackTarget, PackTile};
use crate::store::Store;
use crate::themes;

/// The set file's extension.
pub const EXTENSION: &str = "lsset";
/// `format` in a set file's manifest.
pub const FORMAT: &str = "leadstinger-set";
/// 2 added `theme_file`; 3 writes a tile's `hotkey` only when it was set by hand, and
/// keys may have Option (`alt+1`, D-188).
pub const VERSION: u32 = 3;
/// The set file's MIME type, which the Linux launcher entries claim (D-091).
pub const MIME: &str = "application/x-leadstinger-set";
/// The shared-mime-info declaration of [`MIME`]: the packages install it as
/// `share/mime/packages/<slug>.xml`, and `--register-links` writes it for an AppImage.
pub const MIME_XML: &str = include_str!("../assets/linux/leadstinger-set.xml");

/// Limits on what an imported zip may unpack to, so a hostile or broken file can't fill
/// the disk.
const MAX_ENTRIES: usize = 20_000;
const MAX_TOTAL_BYTES: u64 = 8 << 30;

/// A `.lsset` file, a `.zip` (what a file manager makes of a pack folder), or a
/// Farrago export (`.farragoset`, a folder or file; src/farrago.rs).
pub fn is_set_file(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case(EXTENSION) || e.eq_ignore_ascii_case("zip"))
        || crate::farrago::is_export(path)
}

/// A set file unpacked into a temporary folder, removed again when this is dropped.
pub struct Unpacked {
    dir: PathBuf,
    root: PathBuf,
}

impl Unpacked {
    pub fn new(file: &Path) -> Result<Unpacked> {
        let dir = std::env::temp_dir().join(format!("{}-set-{}", crate::brand::SLUG, Uuid::new_v4()));
        std::fs::create_dir_all(&dir)?;
        if let Err(e) = unzip(file, &dir) {
            std::fs::remove_dir_all(&dir).ok();
            return Err(e.context(format!("reading {}", file.display())));
        }
        Ok(Unpacked { root: pack_root(&dir), dir })
    }

    /// The pack folder inside (see [`pack_root`]).
    pub fn root(&self) -> &Path {
        &self.root
    }
}

impl Drop for Unpacked {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.dir).ok();
    }
}

/// Extract `file` into `dir`, keeping every entry inside it.
fn unzip(file: &Path, dir: &Path) -> Result<()> {
    let mut zip = ZipArchive::new(File::open(file)?).context("not a zip file")?;
    if zip.len() > MAX_ENTRIES {
        bail!("it holds {} files; at most {MAX_ENTRIES} are allowed", zip.len());
    }
    let mut total: u64 = 0;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        // enclosed_name refuses absolute paths and `..`, so nothing lands outside `dir`.
        let Some(rel) = entry.enclosed_name() else { continue };
        let skip = rel.components().any(|c| {
            let s = c.as_os_str().to_string_lossy();
            s == "__MACOSX" || s.starts_with("._") || s == ".DS_Store"
        });
        if skip || entry.is_dir() || entry.is_symlink() {
            continue;
        }
        let out = dir.join(&rel);
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // Count what is really written, not what the header claims.
        let budget = MAX_TOTAL_BYTES - total;
        let written = std::io::copy(&mut (&mut entry).take(budget + 1), &mut File::create(&out)?)?;
        total += written;
        if total > MAX_TOTAL_BYTES {
            bail!("it unpacks to more than {} GB", MAX_TOTAL_BYTES >> 30);
        }
    }
    Ok(())
}

/// What `set.import` added: the sets, and the theme the file carries, installed (or why
/// it couldn't be; the sets are imported either way).
pub struct Import {
    pub sets: Vec<ImportedSet>,
    pub theme: Option<std::result::Result<themes::Imported, String>>,
    /// A Farrago export: what came across and what didn't.
    pub farrago: Option<crate::farrago::Report>,
}

/// One set `set.import` added.
#[derive(Debug, Serialize)]
pub struct ImportedSet {
    pub id: Uuid,
    pub name: String,
    pub tiles: usize,
}

/// Import every set in a set file or pack folder, appended after the library's sets. A
/// name that's already taken gets a number ("Bridge 2"). A new library's untouched
/// "Set 1" is replaced rather than left empty above the imported sets. A Farrago export
/// (folder or zip) is read into the same form first (src/farrago.rs).
pub fn import(store: &mut Store, path: &Path) -> Result<Import> {
    // An unpacked set file stays until its clips are copied or converted into Media.
    let mut unpacked = None;
    let dir = if path.is_dir() {
        pack_root(path)
    } else if path.is_file() {
        unpacked.insert(Unpacked::new(path)?).root().to_path_buf()
    } else {
        bail!("{} doesn't exist", path.display());
    };
    let (mut dir, mut farrago) = (dir, None);
    let manifest = match crate::farrago::root(&dir) {
        Some(root) => {
            let (manifest, report) = crate::farrago::convert(path, &root, &store.library.outputs)?;
            (dir, farrago) = (root, Some(report));
            manifest
        }
        None => load_manifest(&dir)?,
    };
    if manifest.version.is_some_and(|v| v > VERSION) {
        eprintln!("{}: {} is from a newer version; importing what this one understands", crate::brand::SLUG, path.display());
    }
    // Before version 3 a set file named every tile's key, automatic or not (D-193).
    let old_keys = manifest.format.as_deref() == Some(FORMAT) && manifest.version.unwrap_or(1) < 3;
    let blank = match store.library.sets.as_slice() {
        [only] if only.is_untouched_first() => Some((store.library.sets.remove(0), store.library.selected_set)),
        _ => None,
    };
    let mut imported = Vec::new();
    // Per set in the file: its new id and its tiles' new ids, for action targets.
    let mut placed: Vec<Option<Placed>> = Vec::new();
    for gs in &manifest.sets {
        let (id, tiles, ids) = import_set(store, &dir, gs, "set file", old_keys);
        placed.push(None);
        let Some(si) = store.library.set_index(id) else { continue };
        if tiles == 0 {
            store.library.sets.remove(si);
            continue;
        }
        *placed.last_mut().expect("pushed") = Some((id, ids));
        let name = unique_name(store, si);
        store.library.sets[si].name = name.clone();
        imported.push(ImportedSet { id, name, tiles });
    }
    resolve_actions(store, &manifest, &placed);
    if let Some(u) = unpacked {
        store.keep_until_imported(u);
    }
    if imported.is_empty() {
        if let Some((set, selected)) = blank {
            store.library.sets.insert(0, set);
            store.library.selected_set = selected;
        }
        if let Some(first) = farrago.as_ref().and_then(|r| r.skipped.first()) {
            bail!("{} has no playable sounds (“{}”: {})", path.display(), first.title, first.reason);
        }
        bail!("{} has no sets with playable clips", path.display());
    }
    if blank.is_some() {
        store.library.selected_set = Some(imported[0].id);
    }
    store.mark_dirty();
    let theme = manifest.theme_file.as_deref().map(|file| install_theme(store, &dir, file));
    // A tile the library refused after all (its sound couldn't be read) is skipped too.
    if let (Some(report), Some(Some((_, ids)))) = (farrago.as_mut(), placed.first()) {
        let refused: Vec<usize> = ids.iter().enumerate().filter(|(_, id)| id.is_none()).map(|(i, _)| i).collect();
        for i in refused.into_iter().rev() {
            let title = report.imported.remove(i);
            report.skipped.push(crate::farrago::Skipped { title, reason: "its sound couldn't be read".into() });
        }
    }
    Ok(Import { sets: imported, theme, farrago })
}

/// Install the theme a set file carries, replacing an installed one of the same name.
fn install_theme(store: &Store, dir: &Path, file: &str) -> std::result::Result<themes::Imported, String> {
    let path = crate::starter::inside(dir, file).ok_or_else(|| format!("its theme {file:?} is outside the file"))?;
    themes::import(&store.data_dir, &path).map_err(|e| format!("{e:#}"))
}

/// An imported set's new id and its tiles' new ids (none for a tile that was skipped).
type Placed = (Uuid, Vec<Option<Uuid>>);

/// Give imported action tiles their targets, now that every set in the file has its new
/// ids. Targets outside the file (or skipped) are dropped.
fn resolve_actions(store: &mut Store, manifest: &Manifest, placed: &[Option<Placed>]) {
    for (gs, place) in manifest.sets.iter().zip(placed) {
        let Some((_, ids)) = place else { continue };
        for (gt, id) in gs.tiles.iter().zip(ids) {
            let (Some(pa), Some(id)) = (&gt.action, id) else { continue };
            let targets = pa
                .targets
                .iter()
                .filter_map(|pt| {
                    let (sid, ids) = placed.get(pt.set)?.as_ref()?;
                    match pt.tile {
                        Some(i) => Some(Target { tile: Some((*ids.get(i)?)?), offset: pt.offset.max(0.0), ..Default::default() }),
                        None => Some(Target { set: Some(*sid), offset: pt.offset.max(0.0), ..Default::default() }),
                    }
                })
                .collect();
            let kind = serde_json::from_value::<ActionKind>(serde_json::json!(pa.kind)).unwrap_or_default();
            let fade = if pa.fade.is_finite() { pa.fade.clamp(0.0, 60.0) } else { 3.0 };
            if let Some(t) = store.library.tile_mut(*id) {
                t.action = Some(TileAction { kind, targets, fade });
            }
        }
    }
}

/// The set's name, numbered if another set already has it.
fn unique_name(store: &Store, si: usize) -> String {
    let base = store.library.sets[si].name.clone();
    let taken = |n: &str| store.library.sets.iter().enumerate().any(|(i, s)| i != si && s.name.eq_ignore_ascii_case(n));
    if !taken(&base) {
        return base;
    }
    (2..).map(|n| format!("{base} {n}")).find(|n| !taken(n)).expect("some number is free")
}

/// What `set.export` wrote.
#[derive(Debug, Serialize)]
pub struct Exported {
    pub path: PathBuf,
    pub sets: usize,
    pub tiles: usize,
    pub bytes: u64,
    /// Tiles left out because their audio file is missing.
    pub skipped: Vec<String>,
    /// The user theme that went in (its id): the one in use.
    pub theme: Option<String>,
}

/// Write the sets at `set_idxs` into a set file at `path` (`.lsset` is added when the
/// name has no extension). The file is written beside its final name and renamed into
/// place, so a failed export never leaves half a file under that name.
pub fn export(store: &mut Store, set_idxs: &[usize], path: &Path) -> Result<Exported> {
    // Clips still being copied in are waited for; a conversion can take minutes, so that
    // is refused instead.
    if let Some((_, i)) = store.importing().find(|(_, i)| i.busy()) {
        bail!("“{}” is still {}. Export once it's done, or cancel the import", i.file_name(), i.verb().to_lowercase());
    }
    store.finish_imports();
    let store = &*store;
    let path = if path.extension().is_none() { path.with_extension(EXTENSION) } else { path.to_path_buf() };
    let mut manifest = Manifest {
        format: Some(FORMAT.into()),
        version: Some(VERSION),
        app: Some(format!("{} {}", crate::brand::APP_NAME, env!("CARGO_PKG_VERSION"))),
        ..Default::default()
    };
    // (path in the zip, file in Media)
    let mut files: Vec<(String, String)> = Vec::new();
    let mut skipped = Vec::new();
    let mut folders: Vec<String> = Vec::new();
    let mut tiles_written = 0;
    // The user theme in use goes along, in a folder no set's folder takes (D-173).
    let mut theme_files: Vec<(String, Vec<u8>)> = Vec::new();
    let theme = store.library.theme_name.clone().filter(|id| themes::themes_dir(&store.data_dir).join(id).join(themes::THEME_FILE).is_file());
    if let Some(id) = &theme {
        let folder = format!("{id}.{}", themes::EXTENSION);
        for (rel, bytes) in themes::files(&themes::themes_dir(&store.data_dir).join(id))? {
            theme_files.push((format!("{folder}/{rel}"), bytes));
        }
        // Its base, for what opens it as a starter pack in a build without user themes.
        manifest.theme = Some(themes::base_name(store.library.theme).into());
        manifest.theme_file = Some(folder.clone());
        folders.push(folder);
    }
    // Where each written tile lands in the file, for action targets: tiles without their
    // audio are left out, action tiles (which have none) are kept.
    let written = |t: &crate::model::Tile| t.action.is_some() || t.file.as_deref().is_some_and(|f| store.media_dir.join(f).is_file());
    let mut tile_pos: HashMap<Uuid, (usize, usize)> = HashMap::new();
    let mut set_pos: HashMap<Uuid, usize> = HashMap::new();
    for (fi, &si) in set_idxs.iter().enumerate() {
        let set = &store.library.sets[si];
        set_pos.insert(set.id, fi);
        for (ti, t) in set.tiles.iter().filter(|t| written(t)).enumerate() {
            tile_pos.insert(t.id, (fi, ti));
        }
    }
    for &si in set_idxs {
        let set = &store.library.sets[si];
        let folder = unique_folder(&mut folders, &set.name);
        // The grid over the tiles written: those left out close their rows up.
        let mut shape = set.slots();
        for row in &mut shape {
            row.retain(|x| x.is_none_or(|i| written(&set.tiles[i])));
        }
        crate::rows::prune(&mut shape);
        // A media file shared by several tiles goes in once.
        let mut placed: HashMap<String, String> = HashMap::new();
        let mut pack = PackSet {
            name: set.name.clone(),
            layout: enum_name(set.layout),
            columns: Some(set.columns),
            // Counted over the tiles written.
            rows: Some(shape.iter().map(|r| r.iter().flatten().count() as u32).collect()),
            blanks: shape.iter().enumerate().flat_map(|(r, row)| row.iter().enumerate().filter(|(_, x)| x.is_none()).map(move |(a, _)| [r as u32, a as u32])).collect(),
            auto_advance: set.auto_advance,
            crossfade: set.crossfade,
            tiles: Vec::new(),
            notes: set.notes.clone(),
            locked: set.locked,
        };
        for (i, t) in set.tiles.iter().enumerate() {
            let media = t.file.as_deref().filter(|f| store.media_dir.join(f).is_file());
            if media.is_none() && t.action.is_none() {
                skipped.push(format!("{} / {}", set.name, t.title));
                continue;
            }
            let mut place = |media: &str, sub: &str| -> String {
                placed
                    .entry(media.to_owned())
                    .or_insert_with(|| {
                        let ext = Path::new(media).extension().and_then(|e| e.to_str()).unwrap_or("bin");
                        let name = format!("{folder}/{sub}{:02} {}.{ext}", i + 1, file_name(&t.title));
                        files.push((name.clone(), media.to_owned()));
                        name
                    })
                    .clone()
            };
            let file = media.map(|m| place(m, "")).unwrap_or_default();
            // Cover art taken from the audio comes back by itself on import.
            let picture = t
                .image
                .as_deref()
                .filter(|_| !t.image_from_art)
                .filter(|f| store.media_dir.join(f).is_file())
                .map(|img| place(img, "pictures/"));
            pack.tiles.push(PackTile {
                file,
                title: Some(t.title.clone()),
                emoji: t.emoji.clone(),
                // Only a key set by hand: the others come from the slot (version 3).
                hotkey: t.hotkey_manual.then(|| t.hotkey.map(PadKey::name).unwrap_or_default()),
                color: enum_name(t.color),
                mode: enum_name(t.press.mode()),
                hold: t.press.hold.then_some(true),
                pausable: t.press.pausable.then_some(true),
                one_shot: t.press.one_shot.then_some(true),
                looping: t.looping,
                volume: Some(t.volume),
                muted: t.muted,
                gain_db: t.gain_db,
                speed: (t.speed != 1.0).then_some(t.speed),
                pitch: (t.pitch != 0.0).then_some(t.pitch),
                output: (!t.sends.is_empty()).then(|| t.output().to_owned()),
                duck: (t.duck != Duck::None).then(|| enum_name(t.duck)).flatten(),
                duck_db: t.duck_db,
                fade_in: t.fade_in,
                fade_out: t.fade_out,
                stop_fade: (t.stop_fade != t.fade_out).then_some(t.stop_fade),
                fade_curve: (t.fade_curve != FadeCurve::Gentle).then(|| enum_name(t.fade_curve)).flatten(),
                solo: t.solo,
                trim_start: t.start,
                trim_end: t.end,
                vamp: t.vamp,
                markers: t.markers.clone(),
                picture_layer: picture.as_ref().and_then(|_| enum_name(t.image_layer)),
                picture_fit: picture.as_ref().and_then(|_| enum_name(t.image_fit)),
                picture_opacity: picture.as_ref().map(|_| t.image_opacity),
                picture,
                channels: t.channel_mix.clone(),
                sends: if t.sends.len() > 1 || t.sends.iter().any(|s| s.gain_db != 0.0) { t.sends.clone() } else { Vec::new() },
                on_start: t.on_start.clone(),
                on_stop: t.on_stop.clone(),
                notes: t.notes.clone(),
                disabled: t.disabled,
                pre_wait: t.pre_wait,
                link: (t.link != CueLink::None).then(|| enum_name(t.link)).flatten(),
                post_wait: t.post_wait,
                action: t.action.as_ref().map(|a| PackAction {
                    kind: enum_name(a.kind).unwrap_or_default(),
                    fade: a.fade,
                    targets: a
                        .targets
                        .iter()
                        .filter_map(|tg| match (tg.tile, tg.set) {
                            (Some(id), _) => tile_pos.get(&id).map(|&(set, tile)| PackTarget { set, tile: Some(tile), offset: tg.offset }),
                            (None, Some(sid)) => set_pos.get(&sid).map(|&set| PackTarget { set, tile: None, offset: tg.offset }),
                            _ => None,
                        })
                        .collect(),
                }),
            });
            tiles_written += 1;
        }
        manifest.sets.push(pack);
    }
    if tiles_written == 0 {
        bail!("there is nothing to export: no tile has its audio");
    }

    let part = path.with_extension(format!("{EXTENSION}.part"));
    let write = || -> Result<()> {
        let mut zip = ZipWriter::new(File::create(&part).with_context(|| format!("creating {}", part.display()))?);
        let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        zip.start_file("manifest.json", deflated)?;
        zip.write_all(&serde_json::to_vec_pretty(&manifest)?)?;
        for (name, bytes) in &theme_files {
            zip.start_file(name.as_str(), deflated)?;
            zip.write_all(bytes)?;
        }
        for (name, media) in &files {
            let src = store.media_dir.join(media);
            let size = std::fs::metadata(&src)?.len();
            // Compressed audio and pictures gain nothing from deflate; WAV and AIFF do.
            let raw = matches!(Path::new(media).extension().and_then(|e| e.to_str()), Some("wav" | "aif" | "aiff"));
            let options = SimpleFileOptions::default()
                .compression_method(if raw { CompressionMethod::Deflated } else { CompressionMethod::Stored })
                .large_file(size > u32::MAX as u64);
            zip.start_file(name.as_str(), options)?;
            std::io::copy(&mut File::open(&src).with_context(|| format!("reading {}", src.display()))?, &mut zip)?;
        }
        zip.finish()?;
        Ok(())
    };
    if let Err(e) = write() {
        std::fs::remove_file(&part).ok();
        return Err(e);
    }
    std::fs::rename(&part, &path).with_context(|| format!("saving {}", path.display()))?;
    let bytes = std::fs::metadata(&path)?.len();
    Ok(Exported { path, sets: set_idxs.len(), tiles: tiles_written, bytes, skipped, theme })
}

/// A snake_case enum's name, as the manifest writes it.
fn enum_name<T: Serialize>(v: T) -> Option<String> {
    serde_json::to_value(v).ok().and_then(|v| v.as_str().map(str::to_owned))
}

/// A title or set name made safe as one path component on every system.
fn file_name(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') { '_' } else { c })
        .collect();
    let trimmed: String = cleaned.trim().trim_matches('.').chars().take(80).collect();
    if trimmed.is_empty() { "Untitled".into() } else { trimmed }
}

/// The set's folder in the zip, numbered if two sets share a name.
fn unique_folder(taken: &mut Vec<String>, name: &str) -> String {
    let base = file_name(name);
    let folder = std::iter::once(base.clone())
        .chain((2..).map(|n| format!("{base} {n}")))
        .find(|f| !taken.iter().any(|t| t.eq_ignore_ascii_case(f)))
        .expect("some number is free");
    taken.push(folder.clone());
    folder
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ImageLayer, Layout, TileColor};
    use crate::wav;

    #[test]
    fn the_mime_package_is_well_formed() {
        // shared-mime-info rejects the whole file over a "--" inside a comment.
        for comment in MIME_XML.split("<!--").skip(1) {
            let body = comment.split_once("-->").expect("comment closed").0;
            assert!(!body.contains("--"), "double hyphen in an XML comment: {body}");
        }
        assert!(MIME_XML.contains(&format!("type=\"{MIME}\"")) && MIME_XML.contains(&format!("pattern=\"*.{EXTENSION}\"")));
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("sb-setfile-{tag}-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn tone(path: &Path, hz: f32) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        wav::write_tone(path, 8000, 0.3, &[hz], 0.4).unwrap();
    }

    /// A library with one set of two tiles, every setting changed from its default.
    fn library(root: &Path) -> Store {
        let mut store = Store::open_with(root.join("lib"), false).unwrap();
        let si = store.library.sets.len() - 1;
        store.library.sets[si].name = "Show: Night/1".into();
        store.library.sets[si].layout = Layout::List;
        store.library.sets[si].columns = 3;
        store.library.sets[si].auto_advance = true;
        store.library.sets[si].crossfade = 0.5;
        store.library.sets[si].locked = true;
        store.library.sets[si].notes = "Act 1: house to half".into();
        let a = root.join("src/Air Horn.wav");
        let b = root.join("src/Rimshot.wav");
        tone(&a, 440.0);
        tone(&b, 880.0);
        let (ids, errors) = store.import(&[a, b], si, crate::rows::Place::Append);
        assert!(errors.is_empty(), "{errors:?}");
        let png = root.join("src/logo.png");
        image::RgbaImage::from_pixel(8, 8, image::Rgba([200, 0, 0, 255])).save(&png).unwrap();
        store.set_image(ids[0], &png).unwrap();
        let t = store.library.tile_mut(ids[0]).unwrap();
        t.title = "Air Horn!".into();
        t.emoji = "📯".into();
        t.color = TileColor::Crimson;
        t.hotkey = Some(PadKey { key: 'h', alt: true });
        t.hotkey_manual = true;
        t.press = crate::model::PressOptions { hold: true, pausable: true, one_shot: false };
        t.looping = true;
        t.volume = 0.5;
        t.muted = true;
        t.gain_db = 4.5;
        t.duck = Duck::Ducked;
        t.duck_db = Some(-20.0);
        t.fade_curve = FadeCurve::EqualPower;
        t.speed = 1.25;
        t.pitch = -2.5;
        t.fade_in = 0.1;
        t.fade_out = 0.4;
        t.solo = true;
        t.start = 0.05;
        t.end = Some(0.25);
        t.vamp = Some((0.1, 0.2));
        t.markers = vec![crate::model::Marker { name: "Hit".into(), at: 0.15 }];
        t.on_start = vec!["osc://127.0.0.1:53000/cue/1/go 1 \"two words\"".into(), "midi:note:60:ch2 @Lights".into()];
        t.on_stop = vec!["midi:noteoff:60:ch2".into()];
        t.image_layer = ImageLayer::Overlay;
        t.image_opacity = 0.7;
        t.notes = "Standby: door slam".into();
        t.disabled = true;
        t.pre_wait = 0.5;
        t.link = CueLink::Follow;
        t.post_wait = 1.25;
        // An action tile that fades the second clip, and one that stops the whole set.
        let (rim, set_id) = (ids[1], store.library.sets[si].id);
        for target in [crate::model::Target { tile: Some(rim), offset: 0.5, ..Default::default() }, crate::model::Target { set: Some(set_id), ..Default::default() }] {
            let action = TileAction { kind: ActionKind::Fade, targets: vec![target], fade: 1.5 };
            store.library.sets[si].tiles.push(crate::model::Tile { title: "Fade".into(), action: Some(action), ..Default::default() });
        }
        store
    }

    #[test]
    fn export_then_import_keeps_every_setting() {
        let root = temp_dir("roundtrip");
        let mut store = library(&root);
        let si = store.library.sets.len() - 1;
        store.library.sets[si].set_rows(&[1, 3]);
        store.library.sets[si].insert_blanks(crate::rows::Place::InRow { row: 1, at: 1 }, 1);
        // As every command does: the keys follow the new slots.
        store.mark_dirty();
        let out = export(&mut store, &[si], &root.join("night")).unwrap();
        assert_eq!(out.path, root.join("night.lsset"));
        assert_eq!((out.sets, out.tiles), (1, 4), "two clips, two action tiles");
        assert!(!root.join("night.lsset.part").exists());

        let mut other = Store::open_with(root.join("other"), false).unwrap();
        other.library.sets[0].name = "Bridge".into();
        let before = other.library.sets.len();
        let got = import(&mut other, &out.path).unwrap().sets;
        assert_eq!(got.len(), 1);
        assert_eq!((got[0].name.as_str(), got[0].tiles), ("Show: Night/1", 4));
        assert_eq!(other.library.sets.len(), before + 1, "appended, nothing replaced");
        let (a, b) = (&store.library.sets[si], other.library.sets.last().unwrap());
        assert_eq!((b.layout, b.columns, b.auto_advance, b.crossfade, &b.notes, b.locked), (a.layout, a.columns, a.auto_advance, a.crossfade, &a.notes, a.locked));
        assert_eq!(b.row_lengths(), vec![1, 3], "rows travel with the set (D-113)");
        assert_eq!(b.blanks, vec![[1, 1]], "and their blank slots (D-140)");
        for (x, y) in a.tiles.iter().zip(&b.tiles) {
            assert_eq!(
                (&x.title, x.color, x.hotkey, x.press, x.looping, x.volume, x.fade_in, x.fade_out, x.stop_fade, x.solo, x.start, x.end),
                (&y.title, y.color, y.hotkey, y.press, y.looping, y.volume, y.fade_in, y.fade_out, y.stop_fade, y.solo, y.start, y.end)
            );
            assert_eq!((x.gain_db, x.duck, x.duck_db, x.vamp, x.fade_curve, x.speed), (y.gain_db, y.duck, y.duck_db, y.vamp, y.fade_curve, y.speed));
            assert_eq!(x.markers, y.markers);
            assert_eq!((x.pitch, &x.notes, x.disabled, x.muted, &x.emoji), (y.pitch, &y.notes, y.disabled, y.muted, &y.emoji));
            assert_eq!((x.pre_wait, x.link, x.post_wait), (y.pre_wait, y.link, y.post_wait));
            assert_eq!((&x.on_start, &x.on_stop), (&y.on_start, &y.on_stop));
            assert_eq!((x.image.is_some(), x.image_layer, x.image_opacity), (y.image.is_some(), y.image_layer, y.image_opacity));
            if x.action.is_none() {
                assert_ne!(x.file, y.file, "copied into the other library's Media");
            }
        }
        // The actions point at the imported copies.
        let fade = b.tiles[2].action.as_ref().unwrap();
        assert_eq!((fade.kind, fade.fade, fade.targets[0].tile, fade.targets[0].offset), (ActionKind::Fade, 1.5, Some(b.tiles[1].id), 0.5));
        assert_eq!(b.tiles[3].action.as_ref().unwrap().targets[0].set, Some(b.id));
        // Importing again numbers the name instead of merging.
        assert_eq!(import(&mut other, &out.path).unwrap().sets[0].name, "Show: Night/1 2");
        std::fs::remove_dir_all(root).ok();
    }

    /// Version 3 writes only the keys set by hand; an older set file named every tile's
    /// key, and those in the old straight-through order come in automatic (D-188, D-193).
    #[test]
    fn set_files_carry_only_hand_set_keys() {
        let root = temp_dir("keys");
        let mut store = library(&root);
        let si = store.library.sets.len() - 1;
        let out = export(&mut store, &[si], &root.join("keys")).unwrap();
        let unpacked = Unpacked::new(&out.path).unwrap();
        let manifest: serde_json::Value = serde_json::from_slice(&std::fs::read(unpacked.root().join("manifest.json")).unwrap()).unwrap();
        let keys: Vec<_> = manifest["sets"][0]["tiles"].as_array().unwrap().iter().map(|t| t["hotkey"].clone()).collect();
        assert_eq!(keys[0], serde_json::json!("alt+h"), "set by hand");
        assert!(keys[1..].iter().all(|k| k.is_null()), "the others come from the slot: {keys:?}");

        // A version 2 file: 1, 2 and 3 were the old order's; R was set by hand.
        let pack = root.join("v2");
        for n in ["a", "b", "c", "d"] {
            tone(&pack.join(format!("S/{n}.wav")), 440.0);
        }
        let tiles: Vec<_> = ["1", "2", "r", "3"].iter().zip(["a", "b", "c", "d"]).map(|(k, n)| serde_json::json!({"file": format!("S/{n}.wav"), "hotkey": k})).collect();
        let old = serde_json::json!({"format": FORMAT, "version": 2, "sets": [{"name": "Old", "columns": 2, "rows": [2, 2], "tiles": tiles}]});
        std::fs::write(pack.join("manifest.json"), old.to_string()).unwrap();
        let mut other = Store::open_with(root.join("other"), false).unwrap();
        import(&mut other, &pack).unwrap();
        let set = other.library.sets.last().unwrap();
        let got: Vec<_> = set.tiles.iter().map(|t| (t.hotkey.map(PadKey::name).unwrap_or_default(), t.hotkey_manual)).collect();
        let want = [("1", false), ("2", false), ("r", true), ("w", false)];
        assert_eq!(got, want.map(|(k, m)| (k.to_owned(), m)), "row 2's second pad takes W");
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn a_set_file_carries_the_theme_in_use() {
        let root = temp_dir("theme");
        let mut store = library(&root);
        let si = store.library.sets.len() - 1;
        // Without a user theme in use, none goes in.
        assert_eq!(export(&mut store, &[si], &root.join("plain")).unwrap().theme, None);
        let harbor = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/themes/harbor.lstheme");
        let installed = themes::import(&store.data_dir, &harbor).unwrap();
        store.library.theme = installed.look.base;
        store.library.theme_name = Some(installed.theme.id.clone());
        // A set named like the theme's folder doesn't collide with it.
        store.library.sets[si].name = "harbor.lstheme".into();
        let out = export(&mut store, &[si], &root.join("themed")).unwrap();
        assert_eq!(out.theme.as_deref(), Some("harbor"));
        let unpacked = Unpacked::new(&out.path).unwrap();
        let manifest: serde_json::Value = serde_json::from_slice(&std::fs::read(unpacked.root().join("manifest.json")).unwrap()).unwrap();
        assert_eq!((&manifest["version"], &manifest["theme"], &manifest["theme_file"]), (&serde_json::json!(VERSION), &serde_json::json!("console"), &serde_json::json!("harbor.lstheme")));
        assert_eq!(manifest["sets"][0]["tiles"][0]["file"].as_str().unwrap().split('/').next(), Some("harbor.lstheme 2"));

        // Importing installs it beside the sets, and leaves the look alone.
        let mut other = Store::open_with(root.join("other"), false).unwrap();
        let got = import(&mut other, &out.path).unwrap();
        assert_eq!((got.sets[0].name.as_str(), got.sets[0].tiles), ("harbor.lstheme", 4));
        let theme = got.theme.unwrap().unwrap();
        assert_eq!((theme.theme.id.as_str(), theme.theme.name.as_str(), theme.replaced), ("harbor", "Harbor", false));
        assert_eq!(themes::files(&theme.theme.path).unwrap(), themes::files(&installed.theme.path).unwrap(), "the same files");
        assert_eq!(other.library.theme_name, None, "the window asks before using it");
        // Again: the installed copy is replaced, the sets numbered.
        let again = import(&mut other, &out.path).unwrap();
        assert!(again.theme.unwrap().unwrap().replaced);
        // A theme that doesn't load is reported; the sets still come in.
        std::fs::write(unpacked.root().join("harbor.lstheme/tokens.json"), "{ not json").unwrap();
        let broken = import(&mut other, unpacked.root()).unwrap();
        assert_eq!(broken.sets.len(), 1);
        assert!(broken.theme.unwrap().is_err());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn sends_and_channel_mixes_travel_in_set_files() {
        use crate::model::{OutputDef, TileSend};
        let root = temp_dir("sends");
        let mut store = library(&root);
        store.library.outputs.push(OutputDef { name: "Stream".into(), ..Default::default() });
        let si = store.library.sets.len() - 1;
        let (a, b) = (store.library.sets[si].tiles[0].id, store.library.sets[si].tiles[1].id);
        let both = vec![TileSend { output: "Main".into(), gain_db: -3.0 }, TileSend { output: "Stream".into(), gain_db: -6.0 }];
        store.library.tile_mut(a).unwrap().sends = both.clone();
        store.library.tile_mut(b).unwrap().sends = vec![TileSend::to("Stream")];
        // A channel mix travels with it (here on a mono file, where it plays as Auto).
        let mix = vec![crate::model::ChannelRoute { to: crate::model::ChannelTo::Left, gain_db: -3.0 }];
        store.library.tile_mut(a).unwrap().channel_mix = mix.clone();
        let out = export(&mut store, &[si], &root.join("sends")).unwrap();

        // A single send is written the old way, so older builds still route it.
        let unpacked = Unpacked::new(&out.path).unwrap();
        let manifest: serde_json::Value = serde_json::from_slice(&std::fs::read(unpacked.root().join("manifest.json")).unwrap()).unwrap();
        let tiles = &manifest["sets"][0]["tiles"];
        assert_eq!((&tiles[0]["output"], tiles[0]["sends"].as_array().map(Vec::len)), (&serde_json::json!("Main"), Some(2)));
        assert_eq!((&tiles[1]["output"], tiles[1].get("sends")), (&serde_json::json!("Stream"), None));

        let mut with = Store::open_with(root.join("with"), false).unwrap();
        with.library.outputs.push(OutputDef { name: "Stream".into(), ..Default::default() });
        import(&mut with, &out.path).unwrap();
        let got = &with.library.sets.last().unwrap().tiles;
        assert_eq!((&got[0].sends, &got[1].sends), (&both, &vec![TileSend::to("Stream")]));
        assert_eq!((&got[0].channel_mix, got[1].channel_mix.is_empty()), (&mix, true));
        // Without that output, its sends drop and the tiles play on Main.
        let mut without = Store::open_with(root.join("without"), false).unwrap();
        import(&mut without, &out.path).unwrap();
        let got = &without.library.sets.last().unwrap().tiles;
        assert_eq!(got[0].sends, vec![TileSend { output: "Main".into(), gain_db: -3.0 }]);
        assert!(got[1].sends.is_empty());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn a_new_library_trades_its_empty_first_set_for_the_import() {
        let root = temp_dir("blank");
        let mut store = library(&root);
        let last = store.library.sets.len() - 1;
        let out = export(&mut store, &[last], &root.join("night")).unwrap();
        let mut fresh = Store::open_with(root.join("fresh"), false).unwrap();
        assert!(fresh.library.sets[0].is_untouched_first());
        // A failed import leaves the new library as it was.
        std::fs::write(root.join("bad.lsset"), b"nope").unwrap();
        std::fs::create_dir_all(root.join("empty")).unwrap();
        assert!(import(&mut fresh, &root.join("bad.lsset")).is_err());
        assert!(import(&mut fresh, &root.join("empty")).is_err());
        assert_eq!(fresh.library.sets.len(), 1);
        assert!(fresh.library.sets[0].is_untouched_first());
        assert_eq!(fresh.library.selected_set, Some(fresh.library.sets[0].id));
        let got = import(&mut fresh, &out.path).unwrap().sets;
        let names: Vec<&str> = fresh.library.sets.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["Show: Night/1"]);
        assert_eq!(fresh.library.selected_set, Some(got[0].id));
        // A first set that has been touched (here, renamed) stays.
        let mut kept = Store::open_with(root.join("kept"), false).unwrap();
        kept.library.sets[0].name = "Bridge".into();
        import(&mut kept, &out.path).unwrap();
        let names: Vec<&str> = kept.library.sets.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["Bridge", "Show: Night/1"]);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn imports_a_zipped_folder_and_refuses_escapes() {
        let root = temp_dir("folder");
        // What Finder's Compress makes of a pack folder: the folder itself inside.
        let zip_path = root.join("pack.zip");
        let mut zip = ZipWriter::new(File::create(&zip_path).unwrap());
        let opts = SimpleFileOptions::default();
        tone(&root.join("t.wav"), 330.0);
        let wav_bytes = std::fs::read(root.join("t.wav")).unwrap();
        for name in ["Pack/Reactions/Applause.wav", "Pack/Reactions/Cheer.wav", "../evil.wav", "__MACOSX/Pack/._x.wav"] {
            zip.start_file(name, opts).unwrap();
            zip.write_all(&wav_bytes).unwrap();
        }
        zip.finish().unwrap();
        let mut store = Store::open_with(root.join("lib"), false).unwrap();
        let got = import(&mut store, &zip_path).unwrap().sets;
        assert_eq!(got.len(), 1);
        assert_eq!((got[0].name.as_str(), got[0].tiles), ("Reactions", 2));
        assert!(!root.join("evil.wav").exists() && !std::env::temp_dir().join("evil.wav").exists());
        // Not a zip, and a zip with no clips: errors, library untouched.
        let n = store.library.sets.len();
        std::fs::write(root.join("bad.lsset"), b"nope").unwrap();
        assert!(import(&mut store, &root.join("bad.lsset")).is_err());
        assert_eq!(store.library.sets.len(), n);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn a_set_file_clip_ffmpeg_converts_is_read_before_the_unpacked_copy_goes() {
        let Ok(ffmpeg) = crate::transcode::find_ffmpeg() else {
            eprintln!("a_set_file_clip_ffmpeg_converts…: skipped, ffmpeg isn't installed (the Nix dev shell has it)");
            return;
        };
        let root = temp_dir("ac3");
        let ac3 = root.join("boom.ac3");
        let made = std::process::Command::new(&ffmpeg)
            .args(["-nostdin", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "sine=f=220:d=0.5", "-ac", "6", "-c:a", "ac3"])
            .arg(&ac3)
            .status()
            .unwrap();
        assert!(made.success());
        let zip_path = root.join("movie.lsset");
        let mut zip = ZipWriter::new(File::create(&zip_path).unwrap());
        zip.start_file("Pack/Movie/Boom.ac3", SimpleFileOptions::default()).unwrap();
        zip.write_all(&std::fs::read(&ac3).unwrap()).unwrap();
        zip.finish().unwrap();

        let mut store = Store::open_with(root.join("lib"), false).unwrap();
        let got = import(&mut store, &zip_path).unwrap().sets;
        assert_eq!((got[0].name.as_str(), got[0].tiles), ("Movie", 1));
        // The conversion runs after import() returns, from the unpacked copy.
        store.finish_imports();
        let tile = store.library.sets.last().unwrap().tiles[0].clone();
        assert_eq!(tile.title, "Boom");
        let file = tile.file.clone().unwrap();
        assert!(file.ends_with(".flac") && store.media_dir.join(&file).is_file(), "{:?}", store.media(&tile).map(|m| matches!(m, crate::store::Media::Failed(_))));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn names_are_safe_and_unique() {
        assert_eq!(file_name("Show: Night/1"), "Show_ Night_1");
        assert_eq!(file_name(" .. "), "Untitled");
        let mut taken = Vec::new();
        assert_eq!(unique_folder(&mut taken, "Bridge"), "Bridge");
        assert_eq!(unique_folder(&mut taken, "bridge"), "bridge 2");
        assert!(is_set_file(Path::new("a.LSSET")) && is_set_file(Path::new("a.zip")) && !is_set_file(Path::new("a.wav")));
    }
}
