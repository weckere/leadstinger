//! Library persistence, media import and a background decode cache.

use crate::decode::{decode_file, DecodedAudio, EmbeddedArt};
use crate::logic::first_free_key;
use crate::model::{Library, Tile, TileColor};
use crate::picture::{load_picture, Picture};
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use uuid::Uuid;

const SAVE_DELAY: Duration = Duration::from_millis(600);
const DECODE_WORKERS: usize = 2;
pub const PEAK_BUCKETS: usize = 600;

pub enum Media {
    Loading,
    Ready { audio: Arc<DecodedAudio>, peaks: Arc<Vec<f32>> },
    Failed(String),
}

pub enum PictureState {
    Loading,
    Ready(Arc<Picture>),
    Failed(String),
}

enum Job {
    Audio(String, PathBuf),
    Picture(String, PathBuf),
}

enum JobResult {
    Audio(String, Result<(Arc<DecodedAudio>, Arc<Vec<f32>>), String>),
    Picture(String, Result<Arc<Picture>, String>),
}

fn run_job(job: Job) -> JobResult {
    match job {
        Job::Audio(name, path) => JobResult::Audio(
            name,
            decode_file(&path)
                .map(|a| {
                    let peaks = Arc::new(a.peaks(PEAK_BUCKETS));
                    (Arc::new(a), peaks)
                })
                .map_err(|e| format!("{e:#}")),
        ),
        Job::Picture(name, path) => JobResult::Picture(name, load_picture(&path).map(Arc::new).map_err(|e| format!("{e:#}"))),
    }
}

pub struct Store {
    pub library: Library,
    pub data_dir: PathBuf,
    pub media_dir: PathBuf,
    library_path: PathBuf,
    dirty_since: Option<Instant>,
    media: HashMap<String, Media>,
    pictures: HashMap<String, PictureState>,
    jobs: Sender<Job>,
    results: Receiver<JobResult>,
    /// Message for the UI if the library file had to be moved aside on load.
    pub load_warning: Option<String>,
    /// Bumped by every change, so watchers can tell the library changed.
    pub revision: u64,
}

fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

impl Store {
    pub fn open(data_dir: PathBuf) -> Result<Store> {
        Self::open_with(data_dir, true)
    }

    /// `decode: false` skips decoding the library up front: for the CLI (`leadstinger-cli`) working
    /// on the library while the app is closed, which needs no audio.
    pub fn open_with(data_dir: PathBuf, decode: bool) -> Result<Store> {
        let media_dir = data_dir.join("Media");
        std::fs::create_dir_all(&media_dir).with_context(|| format!("creating {}", media_dir.display()))?;
        let library_path = data_dir.join("Library.json");
        let mut load_warning = None;
        let mut library = if library_path.exists() {
            match std::fs::read(&library_path).map_err(anyhow::Error::from).and_then(|b| Ok(serde_json::from_slice::<Library>(&b)?)) {
                Ok(lib) => lib,
                Err(err) => {
                    let backup = data_dir.join(format!("Library.unreadable-{}.json", unix_now()));
                    std::fs::rename(&library_path, &backup).ok();
                    load_warning = Some(format!(
                        "Your library couldn't be read ({err}). It was moved aside to {} and a new one was started.",
                        backup.display()
                    ));
                    Library::starter()
                }
            }
        } else {
            Library::starter()
        };
        library.normalize();

        let (jobs, job_rx) = channel::<Job>();
        let (result_tx, results) = channel::<JobResult>();
        let job_rx = Arc::new(Mutex::new(job_rx));
        for _ in 0..DECODE_WORKERS {
            let job_rx = job_rx.clone();
            let result_tx = result_tx.clone();
            std::thread::spawn(move || loop {
                let job = job_rx.lock().unwrap_or_else(|e| e.into_inner()).recv();
                let Ok(job) = job else { break };
                if result_tx.send(run_job(job)).is_err() {
                    break;
                }
            });
        }

        let mut store = Store {
            library,
            data_dir,
            media_dir,
            library_path,
            dirty_since: None,
            media: HashMap::new(),
            pictures: HashMap::new(),
            jobs,
            results,
            load_warning,
            revision: 0,
        };
        if decode {
            store.request_all_media();
        }
        Ok(store)
    }

    /// Queue every referenced file for decoding, current set first.
    fn request_all_media(&mut self) {
        let sel = self.library.selected_index();
        let mut order: Vec<usize> = vec![sel];
        order.extend((0..self.library.sets.len()).filter(|i| *i != sel));
        let tiles: Vec<(Option<String>, Option<String>)> = order
            .into_iter()
            .flat_map(|i| self.library.sets[i].tiles.iter().map(|t| (t.file.clone(), t.image.clone())).collect::<Vec<_>>())
            .collect();
        for (file, image) in tiles {
            if let Some(f) = file {
                self.request_media(&f);
            }
            if let Some(img) = image {
                self.request_picture(&img);
            }
        }
    }

    fn request_media(&mut self, file: &str) {
        if self.media.contains_key(file) {
            return;
        }
        self.media.insert(file.to_owned(), Media::Loading);
        self.jobs.send(Job::Audio(file.to_owned(), self.media_dir.join(file))).ok();
    }

    fn request_picture(&mut self, file: &str) {
        if self.pictures.contains_key(file) {
            return;
        }
        self.pictures.insert(file.to_owned(), PictureState::Loading);
        self.jobs.send(Job::Picture(file.to_owned(), self.media_dir.join(file))).ok();
    }

    /// Collect finished decodes. Returns true if anything changed.
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        while let Ok(result) = self.results.try_recv() {
            changed = true;
            match result {
                JobResult::Audio(name, Ok((audio, peaks))) => {
                    let dur = audio.duration();
                    let mut dirty = false;
                    for t in self.library.sets.iter_mut().flat_map(|s| s.tiles.iter_mut()) {
                        if t.file.as_deref() == Some(&name) && t.duration.is_none_or(|d| (d - dur).abs() > 1e-3) {
                            t.duration = Some(dur);
                            dirty = true;
                        }
                    }
                    if self.library.needs_art(&name) {
                        self.adopt_embedded_art(&name, audio.art.as_ref());
                        dirty = true;
                    }
                    if dirty {
                        self.mark_dirty();
                    }
                    self.media.insert(name, Media::Ready { audio, peaks });
                }
                JobResult::Audio(name, Err(e)) => {
                    self.media.insert(name, Media::Failed(e));
                }
                JobResult::Picture(name, res) => {
                    let state = match res {
                        Ok(p) => PictureState::Ready(p),
                        Err(e) => PictureState::Failed(e),
                    };
                    self.pictures.insert(name, state);
                }
            }
        }
        changed
    }

    /// Save the audio file's cover art (if any) into Media and give it to the tiles
    /// that haven't been checked yet. Tiles sharing one audio file share one art file.
    fn adopt_embedded_art(&mut self, audio_file: &str, art: Option<&EmbeddedArt>) {
        let saved = art.and_then(|art| {
            let name = format!("{}.{}", Uuid::new_v4(), art.extension());
            match std::fs::write(self.media_dir.join(&name), &art.data) {
                Ok(()) => Some(name),
                Err(e) => {
                    eprintln!("{}: couldn't save cover art: {e}", crate::brand::SLUG);
                    None
                }
            }
        });
        self.library.apply_embedded_art(audio_file, saved.as_deref());
        if let Some(name) = saved {
            if self.library.file_in_use(&name) {
                self.request_picture(&name);
            } else {
                // Every tile already had a picture of its own.
                std::fs::remove_file(self.media_dir.join(&name)).ok();
            }
        }
    }

    pub fn is_loading(&self) -> bool {
        self.media.values().any(|m| matches!(m, Media::Loading))
    }

    pub fn media(&self, tile: &Tile) -> Option<&Media> {
        tile.file.as_ref().and_then(|f| self.media.get(f))
    }

    pub fn audio(&self, tile: &Tile) -> Option<Arc<DecodedAudio>> {
        match self.media(tile)? {
            Media::Ready { audio, .. } => Some(audio.clone()),
            _ => None,
        }
    }

    pub fn peaks(&self, tile: &Tile) -> Option<Arc<Vec<f32>>> {
        match self.media(tile)? {
            Media::Ready { peaks, .. } => Some(peaks.clone()),
            _ => None,
        }
    }

    pub fn picture_state(&self, tile: &Tile) -> Option<&PictureState> {
        tile.image.as_ref().and_then(|f| self.pictures.get(f))
    }

    /// Give a tile a picture chosen by the user (copied into Media).
    pub fn set_image(&mut self, tile_id: Uuid, path: &Path) -> Result<()> {
        // Check it decodes before copying, so a bad file doesn't replace a good picture.
        load_picture(path)?;
        let name = self.copy_in(path)?;
        let old = {
            let tile = self.library.tile_mut(tile_id).context("tile no longer exists")?;
            let old = tile.image.replace(name.clone());
            tile.image_from_art = false;
            tile.art_checked = true;
            old
        };
        self.request_picture(&name);
        if let Some(old) = old {
            self.release_media(&old);
        }
        self.mark_dirty();
        Ok(())
    }

    /// Remove a tile's picture. Embedded art isn't re-applied afterwards.
    pub fn clear_image(&mut self, tile_id: Uuid) {
        let old = self.library.tile_mut(tile_id).and_then(|t| {
            t.image_from_art = false;
            t.image.take()
        });
        if let Some(old) = old {
            self.release_media(&old);
            self.mark_dirty();
        }
    }

    pub fn mark_dirty(&mut self) {
        self.revision += 1;
        if self.dirty_since.is_none() {
            self.dirty_since = Some(Instant::now());
        }
    }

    /// Save if a change is older than the debounce delay.
    pub fn save_if_due(&mut self) -> Result<()> {
        match self.dirty_since {
            Some(t) if t.elapsed() >= SAVE_DELAY => self.save_now(),
            _ => Ok(()),
        }
    }

    pub fn has_unsaved(&self) -> bool {
        self.dirty_since.is_some()
    }

    pub fn save_now(&mut self) -> Result<()> {
        let json = serde_json::to_vec_pretty(&self.library)?;
        let tmp = self.library_path.with_extension("json.tmp");
        std::fs::write(&tmp, json).with_context(|| format!("writing {}", tmp.display()))?;
        std::fs::rename(&tmp, &self.library_path).with_context(|| format!("saving {}", self.library_path.display()))?;
        self.dirty_since = None;
        Ok(())
    }

    /// Copy a file into the media folder under a fresh unique name.
    fn copy_in(&self, src: &Path) -> Result<String> {
        let ext = src.extension().and_then(|e| e.to_str()).unwrap_or("audio").to_ascii_lowercase();
        let name = format!("{}.{}", Uuid::new_v4(), ext);
        std::fs::copy(src, self.media_dir.join(&name)).with_context(|| format!("copying {}", src.display()))?;
        Ok(name)
    }

    /// Import files as new tiles in set `set_idx`, inserted at `at` (or appended).
    /// Returns the new tile ids and any per-file error messages.
    pub fn import(&mut self, paths: &[PathBuf], set_idx: usize, at: Option<usize>) -> (Vec<Uuid>, Vec<String>) {
        let mut errors = Vec::new();
        let mut new_tiles: Vec<Tile> = Vec::new();
        for p in paths {
            if p.is_dir() {
                errors.push(format!("{} is a folder", p.display()));
                continue;
            }
            match self.copy_in(p) {
                Ok(name) => {
                    let set = &self.library.sets[set_idx];
                    let existing: Vec<Tile> = set.tiles.iter().chain(new_tiles.iter()).cloned().collect();
                    let tile = Tile {
                        title: p.file_stem().and_then(|s| s.to_str()).unwrap_or("Sound").to_owned(),
                        file: Some(name.clone()),
                        color: TileColor::nth(existing.len()),
                        hotkey: first_free_key(&existing),
                        ..Default::default()
                    };
                    self.request_media(&name);
                    new_tiles.push(tile);
                }
                Err(e) => errors.push(format!("{e:#}")),
            }
        }
        let ids = new_tiles.iter().map(|t| t.id).collect();
        let tiles = &mut self.library.sets[set_idx].tiles;
        let at = at.unwrap_or(tiles.len()).min(tiles.len());
        tiles.splice(at..at, new_tiles);
        self.mark_dirty();
        (ids, errors)
    }

    /// Point an existing tile at a new file (copied in). Keeps the tile's settings but
    /// resets its trim, since the old trim points are meaningless for new audio.
    pub fn replace_audio(&mut self, tile_id: Uuid, path: &Path) -> Result<()> {
        let name = self.copy_in(path)?;
        let (old, old_art) = {
            let tile = self.library.tile_mut(tile_id).context("tile no longer exists")?;
            let old = tile.file.replace(name.clone());
            if tile.title == "Untitled" || old.is_none() {
                tile.title = path.file_stem().and_then(|s| s.to_str()).unwrap_or("Sound").to_owned();
            }
            tile.start = 0.0;
            tile.end = None;
            tile.duration = None;
            // The old file's cover art belongs to the old audio; check the new file instead.
            // A picture the user chose themselves stays.
            tile.art_checked = false;
            let old_art = if tile.image_from_art {
                tile.image_from_art = false;
                tile.image.take()
            } else {
                None
            };
            (old, old_art)
        };
        // The new file may already be decoded (another tile uses it), in which case
        // no decode result will arrive to trigger the art check.
        let cached_art = match self.media.get(&name) {
            Some(Media::Ready { audio, .. }) => Some(audio.art.clone()),
            _ => None,
        };
        match cached_art {
            Some(art) => self.adopt_embedded_art(&name, art.as_ref()),
            None => self.request_media(&name),
        }
        for f in [old, old_art].into_iter().flatten() {
            self.release_media(&f);
        }
        self.mark_dirty();
        Ok(())
    }

    pub fn remove_tile(&mut self, tile_id: Uuid) {
        if let Some((s, t)) = self.library.locate(tile_id) {
            let tile = self.library.sets[s].tiles.remove(t);
            if let Some(f) = tile.file {
                self.release_media(&f);
            }
            self.mark_dirty();
        }
    }

    pub fn remove_set(&mut self, set_idx: usize) {
        if self.library.sets.len() <= 1 || set_idx >= self.library.sets.len() {
            return;
        }
        let set = self.library.sets.remove(set_idx);
        for f in set.tiles.into_iter().filter_map(|t| t.file) {
            self.release_media(&f);
        }
        self.library.normalize();
        self.mark_dirty();
    }

    /// Delete a media file (audio or picture) once no tile references it.
    fn release_media(&mut self, file: &str) {
        if !self.library.file_in_use(file) {
            std::fs::remove_file(self.media_dir.join(file)).ok();
            self.media.remove(file);
            self.pictures.remove(file);
        }
    }

    pub fn library_path(&self) -> &Path {
        &self.library_path
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        if self.has_unsaved() {
            if let Err(e) = self.save_now() {
                eprintln!("{}: failed to save library: {e:#}", crate::brand::SLUG);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wav;

    fn temp_dir() -> PathBuf {
        let d = std::env::temp_dir().join(format!("sb-store-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn wait_loaded(store: &mut Store) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while store.is_loading() && Instant::now() < deadline {
            store.poll();
            std::thread::sleep(Duration::from_millis(10));
        }
        store.poll();
    }

    #[test]
    fn import_decode_save_reload_and_cleanup() {
        let root = temp_dir();
        let src = root.join("Boom Hit.wav");
        wav::write_tone(&src, 8000, 0.25, &[220.0], 0.5).unwrap();
        let data = root.join("data");

        let tile_id;
        {
            let mut store = Store::open(data.clone()).unwrap();
            // (A WAV has no cover art: the tile must end up checked, with no picture.)
            let (ids, errors) = store.import(&[src.clone(), root.join("missing.wav")], 0, None);
            assert_eq!(ids.len(), 1);
            assert_eq!(errors.len(), 1);
            tile_id = ids[0];
            wait_loaded(&mut store);
            let tile = store.library.tile(tile_id).unwrap().clone();
            assert_eq!(tile.title, "Boom Hit");
            assert_eq!(tile.hotkey, Some('1'));
            assert!((tile.duration.unwrap() - 0.25).abs() < 1e-3);
            assert!(store.audio(&tile).is_some());
            assert!(tile.art_checked && tile.image.is_none());
            store.save_now().unwrap();
        }

        let mut store = Store::open(data.clone()).unwrap();
        wait_loaded(&mut store);
        let tile = store.library.tile(tile_id).unwrap().clone();
        let media_file = data.join("Media").join(tile.file.as_ref().unwrap());
        assert!(media_file.exists());
        assert!(store.audio(&tile).is_some());

        store.remove_tile(tile_id);
        assert!(!media_file.exists(), "unreferenced media is deleted");
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn duplicated_sets_share_media_safely() {
        let root = temp_dir();
        let src = root.join("a.wav");
        wav::write_tone(&src, 8000, 0.1, &[300.0], 0.5).unwrap();
        let mut store = Store::open(root.join("data")).unwrap();
        let (ids, _) = store.import(&[src], 0, None);
        let copy = store.library.sets[0].duplicate();
        store.library.sets.push(copy);
        let file = store.library.tile(ids[0]).unwrap().file.clone().unwrap();
        store.remove_tile(ids[0]);
        assert!(store.media_dir.join(&file).exists(), "still used by the copy");
        store.remove_set(1);
        assert!(!store.media_dir.join(&file).exists());
        std::fs::remove_dir_all(root).ok();
    }

    fn wait_pictures(store: &mut Store) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while store.pictures.values().any(|p| matches!(p, PictureState::Loading)) && Instant::now() < deadline {
            store.poll();
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn user_pictures_are_copied_shared_and_cleaned_up() {
        let root = temp_dir();
        let audio = root.join("a.wav");
        wav::write_tone(&audio, 8000, 0.1, &[300.0], 0.5).unwrap();
        let png = root.join("icon.png");
        std::fs::write(&png, crate::picture::tests::tiny_png()).unwrap();
        let mut store = Store::open(root.join("data")).unwrap();
        let (ids, _) = store.import(&[audio], 0, None);
        let id = ids[0];
        wait_loaded(&mut store);

        assert!(store.set_image(id, &root.join("missing.png")).is_err());
        std::fs::write(root.join("bad.png"), b"nope").unwrap();
        assert!(store.set_image(id, &root.join("bad.png")).is_err(), "undecodable pictures are refused");
        assert!(store.library.tile(id).unwrap().image.is_none());

        store.set_image(id, &png).unwrap();
        wait_pictures(&mut store);
        let tile = store.library.tile(id).unwrap().clone();
        let image = tile.image.clone().unwrap();
        assert!(!tile.image_from_art);
        assert!(matches!(store.picture_state(&tile), Some(PictureState::Ready(p)) if p.width == 4));

        // A duplicate shares the picture file; removing one keeps it for the other.
        let copy = tile.duplicate();
        let copy_id = copy.id;
        store.library.sets[0].tiles.push(copy);
        store.clear_image(id);
        assert!(store.media_dir.join(&image).exists());
        store.clear_image(copy_id);
        assert!(!store.media_dir.join(&image).exists());
        assert!(store.library.tile(id).unwrap().art_checked, "cleared art never comes back");
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn unreadable_library_is_moved_aside() {
        let root = temp_dir();
        std::fs::write(root.join("Library.json"), b"{ not json").unwrap();
        let store = Store::open(root.clone()).unwrap();
        assert!(store.load_warning.is_some());
        assert_eq!(store.library.sets.len(), 1);
        let moved = std::fs::read_dir(&root)
            .unwrap()
            .filter_map(|e| e.ok())
            .any(|e| e.file_name().to_string_lossy().starts_with("Library.unreadable-"));
        assert!(moved);
        std::fs::remove_dir_all(root).ok();
    }
}
