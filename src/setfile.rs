//! Set files (`.lsset`): one or more sets with their clips and pictures in a single file
//! that another library can import (D-039).
//!
//! A set file is a zip of a pack folder (src/starter.rs, starter/README.md):
//!
//! ```text
//! manifest.json            {"format": "leadstinger-set", "version": 1, "sets": [...]}
//! <Set name>/01 Title.flac  each set's clips, in tile order
//! <Set name>/pictures/…     tile pictures chosen by the user
//! ```
//!
//! so an unzipped set file is a valid starter pack, and a zipped pack folder is a valid
//! set file. Importing appends sets and never replaces anything; exporting writes every
//! tile setting. Clips are stored as they are in the library (already compressed, so
//! they aren't deflated again).

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::Serialize;
use uuid::Uuid;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::starter::{import_set, load_manifest, pack_root, Manifest, PackSet, PackTile};
use crate::store::Store;

/// The set file's extension.
pub const EXTENSION: &str = "lsset";
/// `format` in a set file's manifest.
pub const FORMAT: &str = "leadstinger-set";
pub const VERSION: u32 = 1;

/// Limits on what an imported zip may unpack to, so a hostile or broken file can't fill
/// the disk.
const MAX_ENTRIES: usize = 20_000;
const MAX_TOTAL_BYTES: u64 = 8 << 30;

/// A `.lsset` file, or a `.zip` (what a file manager makes of a pack folder).
pub fn is_set_file(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case(EXTENSION) || e.eq_ignore_ascii_case("zip"))
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

/// What `set.import` added.
#[derive(Debug, Serialize)]
pub struct ImportedSet {
    pub id: Uuid,
    pub name: String,
    pub tiles: usize,
}

/// Import every set in a set file or pack folder, appended after the library's sets. A
/// name that's already taken gets a number ("Bridge 2").
pub fn import(store: &mut Store, path: &Path) -> Result<Vec<ImportedSet>> {
    let unpacked;
    let dir = if path.is_dir() {
        pack_root(path)
    } else if path.is_file() {
        unpacked = Unpacked::new(path)?;
        unpacked.root().to_path_buf()
    } else {
        bail!("{} doesn't exist", path.display());
    };
    let manifest = load_manifest(&dir)?;
    if manifest.version.is_some_and(|v| v > VERSION) {
        eprintln!("{}: {} is from a newer version; importing what this one understands", crate::brand::SLUG, path.display());
    }
    let mut imported = Vec::new();
    for gs in &manifest.sets {
        let (id, tiles) = import_set(store, &dir, gs, "set file");
        let Some(si) = store.library.set_index(id) else { continue };
        if tiles == 0 {
            store.library.sets.remove(si);
            continue;
        }
        let name = unique_name(store, si);
        store.library.sets[si].name = name.clone();
        imported.push(ImportedSet { id, name, tiles });
    }
    if imported.is_empty() {
        bail!("{} has no sets with playable clips", path.display());
    }
    store.mark_dirty();
    Ok(imported)
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
}

/// Write the sets at `set_idxs` into a set file at `path` (`.lsset` is added when the
/// name has no extension). The file is written beside its final name and renamed into
/// place, so a failed export never leaves half a file under that name.
pub fn export(store: &Store, set_idxs: &[usize], path: &Path) -> Result<Exported> {
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
    for &si in set_idxs {
        let set = &store.library.sets[si];
        let folder = unique_folder(&mut folders, &set.name);
        // A media file shared by several tiles goes in once.
        let mut placed: HashMap<String, String> = HashMap::new();
        let mut pack = PackSet {
            name: set.name.clone(),
            layout: enum_name(set.layout),
            columns: Some(set.columns),
            auto_advance: set.auto_advance,
            tiles: Vec::new(),
        };
        for (i, t) in set.tiles.iter().enumerate() {
            let Some(media) = t.file.as_deref().filter(|f| store.media_dir.join(f).is_file()) else {
                skipped.push(format!("{} / {}", set.name, t.title));
                continue;
            };
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
            let file = place(media, "");
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
                hotkey: Some(t.hotkey.map(String::from).unwrap_or_default()),
                color: enum_name(t.color),
                mode: enum_name(t.mode),
                looping: t.looping,
                volume: Some(t.volume),
                fade_in: t.fade_in,
                fade_out: t.fade_out,
                solo: t.solo,
                trim_start: t.start,
                trim_end: t.end,
                picture_layer: picture.as_ref().and_then(|_| enum_name(t.image_layer)),
                picture_fit: picture.as_ref().and_then(|_| enum_name(t.image_fit)),
                picture_opacity: picture.as_ref().map(|_| t.image_opacity),
                picture,
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
    Ok(Exported { path, sets: set_idxs.len(), tiles: tiles_written, bytes, skipped })
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
    use crate::model::{ImageLayer, Layout, PlayMode, TileColor};
    use crate::wav;

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
        let a = root.join("src/Air Horn.wav");
        let b = root.join("src/Rimshot.wav");
        tone(&a, 440.0);
        tone(&b, 880.0);
        let (ids, errors) = store.import(&[a, b], si, None);
        assert!(errors.is_empty(), "{errors:?}");
        let png = root.join("src/logo.png");
        image::RgbaImage::from_pixel(8, 8, image::Rgba([200, 0, 0, 255])).save(&png).unwrap();
        store.set_image(ids[0], &png).unwrap();
        let t = store.library.tile_mut(ids[0]).unwrap();
        t.title = "Air Horn!".into();
        t.color = TileColor::Crimson;
        t.hotkey = Some('h');
        t.mode = PlayMode::Hold;
        t.looping = true;
        t.volume = 0.5;
        t.fade_in = 0.1;
        t.fade_out = 0.4;
        t.solo = true;
        t.start = 0.05;
        t.end = Some(0.25);
        t.image_layer = ImageLayer::Overlay;
        t.image_opacity = 0.7;
        store
    }

    #[test]
    fn export_then_import_keeps_every_setting() {
        let root = temp_dir("roundtrip");
        let store = library(&root);
        let si = store.library.sets.len() - 1;
        let out = export(&store, &[si], &root.join("night")).unwrap();
        assert_eq!(out.path, root.join("night.lsset"));
        assert_eq!((out.sets, out.tiles), (1, 2));
        assert!(!root.join("night.lsset.part").exists());

        let mut other = Store::open_with(root.join("other"), false).unwrap();
        let before = other.library.sets.len();
        let got = import(&mut other, &out.path).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!((got[0].name.as_str(), got[0].tiles), ("Show: Night/1", 2));
        assert_eq!(other.library.sets.len(), before + 1, "appended, nothing replaced");
        let (a, b) = (&store.library.sets[si], other.library.sets.last().unwrap());
        assert_eq!((b.layout, b.columns, b.auto_advance), (a.layout, a.columns, a.auto_advance));
        for (x, y) in a.tiles.iter().zip(&b.tiles) {
            assert_eq!(
                (&x.title, x.color, x.hotkey, x.mode, x.looping, x.volume, x.fade_in, x.fade_out, x.solo, x.start, x.end),
                (&y.title, y.color, y.hotkey, y.mode, y.looping, y.volume, y.fade_in, y.fade_out, y.solo, y.start, y.end)
            );
            assert_eq!((x.image.is_some(), x.image_layer, x.image_opacity), (y.image.is_some(), y.image_layer, y.image_opacity));
            assert_ne!(x.file, y.file, "copied into the other library's Media");
        }
        // Importing again numbers the name instead of merging.
        assert_eq!(import(&mut other, &out.path).unwrap()[0].name, "Show: Night/1 2");
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
        let got = import(&mut store, &zip_path).unwrap();
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
    fn names_are_safe_and_unique() {
        assert_eq!(file_name("Show: Night/1"), "Show_ Night_1");
        assert_eq!(file_name(" .. "), "Untitled");
        let mut taken = Vec::new();
        assert_eq!(unique_folder(&mut taken, "Bridge"), "Bridge");
        assert_eq!(unique_folder(&mut taken, "bridge"), "bridge 2");
        assert!(is_set_file(Path::new("a.LSSET")) && is_set_file(Path::new("a.zip")) && !is_set_file(Path::new("a.wav")));
    }
}
