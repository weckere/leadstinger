//! Library persistence, media import and a background decode cache.

use crate::decode::{decode_file, DecodedAudio, EmbeddedArt};
use crate::model::{Library, SoundSet, Tile, TileColor};
use crate::rows::Place;
use crate::picture::{load_picture, Picture};
use crate::transcode::{self, Plan, Progress};
use anyhow::{Context, Result};
use std::collections::{HashMap, HashSet};
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
    /// Render `file` transposed by `cents` hundredths of a semitone.
    Pitch(String, i32, Arc<DecodedAudio>),
}

/// Bring a file into Media as `name`: a copy, or a conversion through ffmpeg. Runs on
/// the import thread, one file at a time, so a long conversion never holds up the UI,
/// the audio or the decoders.
struct ImportJob {
    name: String,
    src: Source,
    plan: Plan,
    progress: Arc<Progress>,
}

/// Where an imported file comes from.
enum Source {
    File(PathBuf),
    /// Downloaded first: a QuipClipper clip (src/quip.rs).
    Download(crate::quip::Fetch),
}

/// A file being copied or converted into Media (see [`Store::importing`]).
pub struct Importing {
    /// The file the user imported.
    pub source: PathBuf,
    /// Through ffmpeg rather than a copy.
    pub converting: bool,
    /// Downloaded first (from QuipClipper).
    pub remote: bool,
    progress: Arc<Progress>,
}

impl Importing {
    /// How far a conversion has got, once ffmpeg knows the file's length.
    pub fn fraction(&self) -> Option<f32> {
        self.progress.fraction()
    }

    /// Downloading or converting: slow enough to show progress and offer Cancel.
    pub fn busy(&self) -> bool {
        self.converting || self.remote
    }

    /// What it's doing now: "Downloading", "Converting" or "Copying".
    pub fn verb(&self) -> &'static str {
        if self.progress.is_downloading() {
            "Downloading"
        } else if self.converting {
            "Converting"
        } else {
            "Copying"
        }
    }

    /// The imported file's name, for messages.
    pub fn file_name(&self) -> String {
        self.source.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
    }
}

fn run_import(job: &ImportJob, dest: &Path) -> Result<(), String> {
    // Into place only when complete, so a half-copied file is never decoded.
    let part = dest.with_file_name(format!("{}.part", job.name));
    let src = match &job.src {
        Source::File(src) => src,
        Source::Download(fetch) => {
            fetch.download(&part, &job.progress)?;
            return match &job.plan {
                Plan::Convert(ffmpeg) => {
                    let res = transcode::convert(ffmpeg, &part, dest, &job.progress);
                    std::fs::remove_file(&part).ok();
                    res
                }
                Plan::Copy => std::fs::rename(&part, dest).map_err(|e| {
                    std::fs::remove_file(&part).ok();
                    format!("saving the download: {e}")
                }),
            };
        }
    };
    match &job.plan {
        Plan::Convert(ffmpeg) => transcode::convert(ffmpeg, src, dest, &job.progress),
        Plan::Copy => std::fs::copy(src, &part).and_then(|_| std::fs::rename(&part, dest)).map(drop).map_err(|e| {
            std::fs::remove_file(&part).ok();
            format!("copying {}: {e}", src.display())
        }),
    }
}

enum JobResult {
    /// A file is in Media (or couldn't be brought in).
    Imported(String, Result<(), String>),
    Audio(String, Result<(Arc<DecodedAudio>, Arc<Vec<f32>>), String>),
    Picture(String, Result<Arc<Picture>, String>),
    Pitch(String, i32, Arc<DecodedAudio>),
}

/// A transposed copy of a clip (`Tile::pitch`).
enum Pitched {
    Rendering,
    Ready(Arc<DecodedAudio>),
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
        Job::Pitch(name, cents, audio) => JobResult::Pitch(name, cents, Arc::new(crate::pitch::shift(&audio, cents as f32 / 100.0))),
    }
}

pub struct Store {
    pub library: Library,
    pub data_dir: PathBuf,
    pub media_dir: PathBuf,
    library_path: PathBuf,
    dirty_since: Option<Instant>,
    media: HashMap<String, Media>,
    /// Transposed copies, by file and pitch in cents.
    pitched: HashMap<(String, i32), Pitched>,
    pictures: HashMap<String, PictureState>,
    jobs: Sender<Job>,
    imports: Sender<ImportJob>,
    results: Receiver<JobResult>,
    /// Files on their way into Media, by media name.
    importing: HashMap<String, Importing>,
    /// Imports that ended (finished or failed) since [`Store::take_imported`], by media name.
    imported: Vec<String>,
    /// Kept until the imports finish: the unpacked set files they read from.
    keep: Vec<Box<dyn Send>>,
    /// Message for the UI if the library file had to be moved aside on load.
    pub load_warning: Option<String>,
    /// What migrating an older library changed (`Library::migrate`), for a notice.
    pub migrated: Option<String>,
    /// Bumped by every change, so watchers can tell the library changed.
    pub revision: u64,
    /// Media files an undo step still needs: not deleted (or evicted) when the library
    /// stops using them.
    held: HashSet<String>,
    /// Held files the library no longer uses, deleted once nothing holds them.
    orphans: HashSet<String>,
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
        // Leftovers of imports interrupted by a crash.
        for entry in std::fs::read_dir(&media_dir).into_iter().flatten().flatten() {
            if entry.file_name().to_string_lossy().ends_with(".part") {
                std::fs::remove_file(entry.path()).ok();
            }
        }
        let library_path = data_dir.join("Library.json");
        let mut load_warning = None;
        let mut migrated = None;
        let mut library = if library_path.exists() {
            match std::fs::read(&library_path).map_err(anyhow::Error::from).and_then(|b| Ok(serde_json::from_slice::<Library>(&b)?)) {
                Ok(mut lib) => {
                    let from = lib.version;
                    migrated = lib.migrate();
                    if migrated.is_some() {
                        // Kept as it was, for going back to an older version.
                        std::fs::copy(&library_path, data_dir.join(format!("Library.v{from}.json"))).ok();
                    }
                    lib
                }
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
        let (imports, import_rx) = channel::<ImportJob>();
        let dir = media_dir.clone();
        std::thread::spawn(move || {
            for job in import_rx {
                let res = if job.progress.is_cancelled() { Err(transcode::CANCELLED.to_owned()) } else { run_import(&job, &dir.join(&job.name)) };
                if result_tx.send(JobResult::Imported(job.name, res)).is_err() {
                    break;
                }
            }
        });

        let mut store = Store {
            library,
            data_dir,
            media_dir,
            library_path,
            dirty_since: None,
            media: HashMap::new(),
            pitched: HashMap::new(),
            pictures: HashMap::new(),
            jobs,
            imports,
            results,
            importing: HashMap::new(),
            imported: Vec::new(),
            keep: Vec::new(),
            load_warning,
            migrated,
            revision: 0,
            held: HashSet::new(),
            orphans: HashSet::new(),
        };
        if store.migrated.is_some() {
            store.mark_dirty();
        }
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
                JobResult::Imported(name, res) => {
                    self.importing.remove(&name);
                    match res {
                        // Removed while it was on its way in.
                        _ if !self.media.contains_key(&name) => {
                            std::fs::remove_file(self.media_dir.join(&name)).ok();
                        }
                        Ok(()) => {
                            self.jobs.send(Job::Audio(name.clone(), self.media_dir.join(&name))).ok();
                            self.imported.push(name);
                        }
                        Err(e) => {
                            self.media.insert(name.clone(), Media::Failed(e));
                            self.imported.push(name);
                        }
                    }
                    if self.importing.is_empty() {
                        self.keep.clear();
                    }
                }
                // Removed while it was decoding.
                JobResult::Audio(name, _) if !self.media.contains_key(&name) => {}
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
                    self.media.insert(name.clone(), Media::Ready { audio, peaks });
                    // Tiles that play this file transposed get their copy rendered now.
                    let pitches: Vec<f32> =
                        self.library.sets.iter().flat_map(|s| &s.tiles).filter(|t| t.file.as_deref() == Some(&name) && t.pitch != 0.0).map(|t| t.pitch).collect();
                    for p in pitches {
                        self.request_pitch(&name, p);
                    }
                }
                JobResult::Audio(name, Err(e)) => {
                    self.media.insert(name, Media::Failed(e));
                }
                JobResult::Pitch(name, cents, audio) => {
                    self.pitched.insert((name, cents), Pitched::Ready(audio));
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

    /// Files on their way into Media, by media name, in no particular order.
    pub fn importing(&self) -> impl Iterator<Item = (&String, &Importing)> {
        self.importing.iter()
    }

    /// The imports that ended since the last call, finished or failed, by media name:
    /// however soon one ended, it is in here once (the window announces them).
    pub fn take_imported(&mut self) -> Vec<String> {
        std::mem::take(&mut self.imported)
    }

    /// The tile's file, while it's still being copied or converted.
    pub fn import_of(&self, tile: &Tile) -> Option<&Importing> {
        tile.file.as_ref().and_then(|f| self.importing.get(f))
    }

    /// Stop every import under way (ffmpeg is stopped at once). Their tiles stay, showing
    /// the import as cancelled. Returns how many were stopped.
    pub fn cancel_imports(&mut self) -> usize {
        self.importing.values().filter(|i| !i.progress.is_cancelled()).inspect(|i| i.progress.cancel()).count()
    }

    /// Wait until every import has finished (the CLI, before it saves and exits; tests).
    pub fn finish_imports(&mut self) {
        while !self.importing.is_empty() {
            self.poll();
            if !self.importing.is_empty() {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }

    /// For quitting: cancel conversions and downloads (their tiles read as unreadable next
    /// time, to be imported again) and let plain copies finish. ffmpeg has stopped when
    /// this returns.
    pub fn stop_imports(&mut self) {
        for i in self.importing.values().filter(|i| i.busy()) {
            i.progress.cancel();
        }
        self.finish_imports();
    }

    /// Keep `guard` (an unpacked set file, say) until the imports under way finish.
    pub fn keep_until_imported(&mut self, guard: impl Send + 'static) {
        if !self.importing.is_empty() {
            self.keep.push(Box::new(guard));
        }
    }

    /// Start bringing `src` into Media under a new name, copied or converted
    /// ([`transcode::plan`]). Returns the name; the tile using it shows as loading until
    /// the file is in and decoded.
    fn start_import(&mut self, src: &Path) -> Result<String, String> {
        if src.is_dir() {
            return Err(format!("{} is a folder", src.display()));
        }
        if !src.is_file() {
            return Err(format!("{} doesn't exist", src.display()));
        }
        let plan = transcode::plan(src)?;
        let ext = match plan {
            Plan::Convert(_) => "flac".to_owned(),
            Plan::Copy => src.extension().and_then(|e| e.to_str()).unwrap_or("audio").to_ascii_lowercase(),
        };
        let name = format!("{}.{ext}", Uuid::new_v4());
        let progress = Arc::new(Progress::default());
        let converting = matches!(plan, Plan::Convert(_));
        self.importing.insert(name.clone(), Importing { source: src.to_path_buf(), converting, remote: false, progress: progress.clone() });
        self.media.insert(name.clone(), Media::Loading);
        self.imports.send(ImportJob { name: name.clone(), src: Source::File(src.to_path_buf()), plan, progress }).ok();
        Ok(name)
    }

    /// Download a QuipClipper clip as a new tile titled `title` in set `set_idx`
    /// (appended), on the import thread like any import: the tile appears at once and
    /// shows the download's progress. A clip in a video container or AC-3/DTS goes on
    /// through ffmpeg when there is one ([`transcode::FFMPEG_EXTENSIONS`]).
    pub fn import_download(&mut self, fetch: crate::quip::Fetch, title: &str, set_idx: usize) -> Uuid {
        let file = PathBuf::from(fetch.file_name());
        let plan = match transcode::find_ffmpeg() {
            Ok(ff) if transcode::prefers_ffmpeg(&file) => Plan::Convert(ff),
            _ => Plan::Copy,
        };
        let ext = match plan {
            Plan::Convert(_) => "flac".to_owned(),
            Plan::Copy => file.extension().and_then(|e| e.to_str()).filter(|e| e.chars().all(|c| c.is_ascii_alphanumeric())).unwrap_or("audio").to_ascii_lowercase(),
        };
        let name = format!("{}.{ext}", Uuid::new_v4());
        let progress = Arc::new(Progress::default());
        let converting = matches!(plan, Plan::Convert(_));
        self.importing.insert(name.clone(), Importing { source: file, converting, remote: true, progress: progress.clone() });
        self.media.insert(name.clone(), Media::Loading);
        self.imports.send(ImportJob { name: name.clone(), src: Source::Download(fetch), plan, progress }).ok();
        let set = &self.library.sets[set_idx];
        let tile = Tile {
            title: title.to_owned(),
            file: Some(name),
            color: TileColor::nth(set.tiles.len()),
            ..Default::default()
        };
        let id = tile.id;
        self.library.sets[set_idx].tiles.push(tile);
        self.mark_dirty();
        id
    }

    pub fn media(&self, tile: &Tile) -> Option<&Media> {
        tile.file.as_ref().and_then(|f| self.media.get(f))
    }

    /// Render the tile's file at `semitones` in the background, unless it's done or
    /// under way (or the file isn't decoded yet; it's rendered when that finishes).
    pub fn request_pitch(&mut self, file: &str, semitones: f32) {
        let cents = crate::pitch::cents(semitones);
        if cents == 0 || self.pitched.contains_key(&(file.to_owned(), cents)) {
            return;
        }
        if let Some(Media::Ready { audio, .. }) = self.media.get(file) {
            self.pitched.insert((file.to_owned(), cents), Pitched::Rendering);
            self.jobs.send(Job::Pitch(file.to_owned(), cents, audio.clone())).ok();
        }
    }

    /// What the tile plays: its clip, transposed when it has a pitch. `Err(true)` while
    /// that copy is still rendering, `Err(false)` when the clip isn't decoded.
    pub fn playable(&self, tile: &Tile) -> Result<Arc<DecodedAudio>, bool> {
        let audio = self.audio(tile).ok_or(false)?;
        let cents = crate::pitch::cents(tile.pitch);
        if cents == 0 {
            return Ok(audio);
        }
        match tile.file.as_ref().and_then(|f| self.pitched.get(&(f.clone(), cents))) {
            Some(Pitched::Ready(a)) => Ok(a.clone()),
            _ => Err(true),
        }
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
        self.library.rekey();
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

    /// Import files as new tiles in set `set_idx`, put at `place` in its grid. The tiles
    /// appear at once; their files are copied or converted on the import thread. Returns
    /// the new tile ids and the error messages for files that were refused.
    pub fn import(&mut self, paths: &[PathBuf], set_idx: usize, place: Place) -> (Vec<Uuid>, Vec<String>) {
        let mut errors = Vec::new();
        let mut new_tiles: Vec<Tile> = Vec::new();
        for p in paths {
            match self.start_import(p) {
                Ok(name) => {
                    let set = &self.library.sets[set_idx];
                    let tile = Tile {
                        title: p.file_stem().and_then(|s| s.to_str()).unwrap_or("Sound").to_owned(),
                        file: Some(name.clone()),
                        color: TileColor::nth(set.tiles.len() + new_tiles.len()),
                        ..Default::default()
                    };
                    new_tiles.push(tile);
                }
                Err(e) => errors.push(e),
            }
        }
        let ids = new_tiles.iter().map(|t| t.id).collect();
        self.library.sets[set_idx].insert_tiles(place, new_tiles);
        self.mark_dirty();
        (ids, errors)
    }

    /// Give a tile a recording: written into Media as a WAV, with the samples kept
    /// decoded so the tile plays at once. Replaces the tile's audio and resets its trim.
    pub fn attach_recording(&mut self, tile_id: Uuid, audio: DecodedAudio) -> Result<f64> {
        let name = format!("{}.wav", Uuid::new_v4());
        crate::wav::write_stereo(&self.media_dir.join(&name), audio.sample_rate, &audio.data).context("writing the recording")?;
        let dur = audio.duration();
        let old = {
            let tile = self.library.tile_mut(tile_id).context("the tile went away while recording")?;
            tile.start = 0.0;
            tile.end = None;
            tile.duration = Some(dur);
            // A recording carries no cover art.
            tile.art_checked = true;
            tile.file.replace(name.clone())
        };
        let peaks = Arc::new(audio.peaks(PEAK_BUCKETS));
        self.media.insert(name, Media::Ready { audio: Arc::new(audio), peaks });
        if let Some(old) = old {
            self.release_media(&old);
        }
        self.mark_dirty();
        Ok(dur)
    }

    /// Point an existing tile at a new file (imported as by [`Store::import`]). Keeps the
    /// tile's settings but resets its trim, since the old trim points are meaningless for
    /// new audio.
    pub fn replace_audio(&mut self, tile_id: Uuid, path: &Path) -> Result<()> {
        if self.library.tile(tile_id).is_none() {
            anyhow::bail!("tile no longer exists");
        }
        let name = self.start_import(path).map_err(anyhow::Error::msg)?;
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
        // The art check runs when the new file is decoded.
        for f in [old, old_art].into_iter().flatten() {
            self.release_media(&f);
        }
        self.mark_dirty();
        Ok(())
    }

    pub fn remove_tile(&mut self, tile_id: Uuid) {
        if let Some((s, t)) = self.library.locate(tile_id) {
            let tile = self.library.sets[s].remove_tile_at(t);
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

    /// Delete a media file (audio or picture) once no tile references it, unless an undo
    /// step holds it; then it stays, decoded, until nothing does.
    fn release_media(&mut self, file: &str) {
        if self.library.file_in_use(file) {
            return;
        }
        if self.held.contains(file) {
            self.orphans.insert(file.to_owned());
        } else {
            self.delete_media(file);
        }
    }

    fn delete_media(&mut self, file: &str) {
        // A file still on its way in is removed when it arrives.
        if let Some(i) = self.importing.get(file) {
            i.progress.cancel();
        }
        std::fs::remove_file(self.media_dir.join(file)).ok();
        self.media.remove(file);
        self.pitched.retain(|(f, _), _| f != file);
        self.pictures.remove(file);
    }

    /// The media files undo history refers to. Orphans no longer held are deleted.
    pub fn hold_media(&mut self, files: HashSet<String>) {
        self.held = files;
        let free: Vec<String> = self.orphans.iter().filter(|f| !self.held.contains(*f)).cloned().collect();
        for f in free {
            self.orphans.remove(&f);
            if !self.library.file_in_use(&f) {
                self.delete_media(&f);
            }
        }
    }

    /// Put back the sets from an undo step (the library's other settings stay as they
    /// are), releasing media the restored sets don't use and loading what they do.
    pub fn restore_sets(&mut self, sets: Vec<SoundSet>, selected: Option<Uuid>) {
        let before: HashSet<String> = self.library.media_files().collect();
        self.library.sets = sets;
        self.library.selected_set = selected;
        self.library.normalize();
        for f in &before {
            self.release_media(f);
        }
        let tiles = self.library.sets.iter().flat_map(|s| &s.tiles);
        let (audio, images): (Vec<String>, Vec<String>) =
            (tiles.clone().filter_map(|t| t.file.clone()).collect(), tiles.filter_map(|t| t.image.clone()).collect());
        for img in images {
            self.orphans.remove(&img);
            self.request_picture(&img);
        }
        for f in audio {
            self.orphans.remove(&f);
            match self.media.get(&f) {
                // Back from an undo step: fill in what decoding found, as poll() does.
                Some(Media::Ready { audio, .. }) => {
                    let (dur, art) = (audio.duration(), audio.art.clone());
                    for t in self.library.sets.iter_mut().flat_map(|s| s.tiles.iter_mut()) {
                        if t.file.as_deref() == Some(&f) {
                            t.duration = Some(dur);
                        }
                    }
                    if self.library.needs_art(&f) {
                        self.adopt_embedded_art(&f, art.as_ref());
                    }
                }
                Some(_) => {}
                None => self.request_media(&f),
            }
        }
        self.mark_dirty();
    }

    pub fn library_path(&self) -> &Path {
        &self.library_path
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        self.stop_imports();
        // Undo history ends with the process.
        for f in std::mem::take(&mut self.orphans) {
            if !self.library.file_in_use(&f) {
                std::fs::remove_file(self.media_dir.join(&f)).ok();
            }
        }
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
            let (ids, errors) = store.import(&[src.clone(), root.join("missing.wav")], 0, Place::Append);
            assert_eq!(ids.len(), 1);
            assert_eq!(errors.len(), 1);
            tile_id = ids[0];
            wait_loaded(&mut store);
            let tile = store.library.tile(tile_id).unwrap().clone();
            assert_eq!(tile.title, "Boom Hit");
            assert_eq!(tile.hotkey, Some(crate::logic::PadKey::plain('1')));
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
        let (ids, _) = store.import(&[src], 0, Place::Append);
        store.finish_imports();
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
        let (ids, _) = store.import(&[audio], 0, Place::Append);
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

    /// ffmpeg, or `None` (and a note) to skip a test that needs it.
    fn ffmpeg_or_skip(test: &str) -> Option<PathBuf> {
        match transcode::find_ffmpeg() {
            Ok(ff) => Some(ff),
            Err(why) => {
                eprintln!("{test}: skipped, {why} (the Nix dev shell has it)");
                None
            }
        }
    }

    /// Make a test file with ffmpeg itself: `inputs` are lavfi sources.
    fn ffmpeg_make(ffmpeg: &Path, inputs: &[&str], out_args: &[&str], out: &Path) {
        let mut cmd = std::process::Command::new(ffmpeg);
        cmd.args(["-nostdin", "-hide_banner", "-loglevel", "error", "-y"]);
        for i in inputs {
            cmd.args(["-f", "lavfi", "-i", i]);
        }
        let ok = cmd.args(out_args).arg(out).status().unwrap();
        assert!(ok.success(), "ffmpeg couldn't make {}", out.display());
    }

    const SURROUND: &str = "aevalsrc=0.5*sin(440*2*PI*t)|0.5*sin(550*2*PI*t)|0.5*sin(660*2*PI*t)|0.5*sin(60*2*PI*t)|0.5*sin(880*2*PI*t)|0.5*sin(990*2*PI*t):c=5.1:s=48000";

    #[test]
    fn converts_multichannel_ac3_and_video_audio_with_ffmpeg() {
        let Some(ffmpeg) = ffmpeg_or_skip("converts_multichannel_ac3_and_video_audio_with_ffmpeg") else { return };
        let root = temp_dir();
        let ac3 = root.join("Main Feature.ac3");
        ffmpeg_make(&ffmpeg, &[&format!("{SURROUND}:d=1")], &["-c:a", "ac3"], &ac3);
        let mkv = root.join("Clip.mkv");
        ffmpeg_make(&ffmpeg, &["testsrc=d=1:s=64x48:r=10", "sine=f=330:d=1:r=44100"], &["-c:v", "mpeg4", "-c:a", "ac3"], &mkv);

        let mut store = Store::open(root.join("data")).unwrap();
        let (ids, errors) = store.import(&[ac3, mkv], 0, Place::Append);
        assert!(errors.is_empty(), "{errors:?}");
        assert!(store.import_of(store.library.tile(ids[0]).unwrap()).is_some_and(|i| i.converting));
        wait_loaded(&mut store);
        assert_eq!(store.importing().count(), 0);

        let tile = store.library.tile(ids[0]).unwrap().clone();
        assert_eq!(tile.title, "Main Feature", "the original's name is the title");
        let file = tile.file.clone().unwrap();
        assert!(file.ends_with(".flac") && store.media_dir.join(&file).is_file());
        let audio = store.audio(&tile).expect("decoded");
        assert!((audio.duration() - 1.0).abs() < 0.05, "{}", audio.duration());
        let native = audio.native.as_ref().expect("every channel kept");
        assert_eq!(native.count(), 6);
        assert!(native.labels.iter().any(|l| l == "LFE"), "{:?}", native.labels);

        // Only the audio of a video file is kept.
        let clip = store.library.tile(ids[1]).unwrap().clone();
        assert_eq!(clip.title, "Clip");
        let audio = store.audio(&clip).expect("decoded");
        assert!(audio.native.is_none() && (audio.duration() - 1.0).abs() < 0.1);
        assert!(std::fs::read_dir(&store.media_dir).unwrap().flatten().all(|e| !e.file_name().to_string_lossy().ends_with(".part")));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn a_conversion_can_be_cancelled_and_a_bad_file_fails_cleanly() {
        let Some(ffmpeg) = ffmpeg_or_skip("a_conversion_can_be_cancelled_and_a_bad_file_fails_cleanly") else { return };
        let root = temp_dir();
        let long = root.join("Long.ac3");
        ffmpeg_make(&ffmpeg, &[&format!("{SURROUND}:d=300")], &["-c:a", "ac3"], &long);
        let bad = root.join("Broken.dts");
        std::fs::write(&bad, b"not a dts stream at all").unwrap();

        let mut store = Store::open(root.join("data")).unwrap();
        let (ids, errors) = store.import(&[long, bad], 0, Place::Append);
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(store.cancel_imports(), 2);
        store.finish_imports();
        let media = |store: &Store, i: usize| match store.media(store.library.tile(ids[i]).unwrap()) {
            Some(Media::Failed(e)) => e.clone(),
            _ => "not failed".to_owned(),
        };
        assert_eq!(media(&store, 0), transcode::CANCELLED, "the tile stays, marked cancelled");

        // Without cancelling, a file ffmpeg can't read fails with its reason.
        let (ids, _) = store.import(&[root.join("Broken.dts")], 0, Place::Append);
        store.finish_imports();
        let tile = store.library.tile(ids[0]).unwrap();
        assert!(matches!(store.media(tile), Some(Media::Failed(e)) if e.contains("ffmpeg")), "{:?}", store.media(tile).map(|m| matches!(m, Media::Failed(_))));
        assert_eq!(std::fs::read_dir(&store.media_dir).unwrap().count(), 0, "nothing left in Media");
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

    #[test]
    fn older_libraries_migrate_on_load_and_keep_a_copy() {
        let root = temp_dir();
        let old = r#"{"version": 1, "stop_all_fade": 1.0, "sets": [{"name": "Show", "tiles": [{"title": "Bed", "fade_out": 1.5}]}]}"#;
        std::fs::write(root.join("Library.json"), old).unwrap();
        let mut store = Store::open_with(root.clone(), false).unwrap();
        assert!(store.migrated.as_deref().is_some_and(|m| m.contains("Stop All")));
        assert_eq!(store.library.sets[0].tiles[0].stop_fade, 1.5);
        assert_eq!(std::fs::read_to_string(root.join("Library.v1.json")).unwrap(), old);
        store.save_now().unwrap();
        drop(store);
        let again = Store::open_with(root.clone(), false).unwrap();
        assert_eq!(again.migrated, None, "saved as the current version");
        assert_eq!((again.library.version, again.library.stop_all_fade), (crate::model::LIBRARY_VERSION, 0.0));
        std::fs::remove_dir_all(root).ok();
    }

    /// Version 2 → 3 (D-113): each set's N-column wrap becomes rows of N, so the grid
    /// looks the same, and the old file stays beside it.
    #[test]
    fn version_2_libraries_get_rows_and_keep_a_copy() {
        let root = temp_dir();
        let tiles: Vec<String> = (0..7).map(|i| format!(r#"{{"title": "T{i}"}}"#)).collect();
        let old = format!(r#"{{"version": 2, "sets": [{{"name": "Show", "columns": 3, "tiles": [{}]}}]}}"#, tiles.join(","));
        std::fs::write(root.join("Library.json"), &old).unwrap();
        let mut store = Store::open_with(root.clone(), false).unwrap();
        assert!(store.migrated.as_deref().is_some_and(|m| m.contains("grid rows")), "{:?}", store.migrated);
        assert_eq!(std::fs::read_to_string(root.join("Library.v2.json")).unwrap(), old);
        assert_eq!((store.library.sets[0].rows.clone(), store.library.grid), (vec![3, 3, 1], crate::model::GridMode::Rows));
        store.save_now().unwrap();
        let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(root.join("Library.json")).unwrap()).unwrap();
        assert_eq!((saved["version"].as_u64(), saved["sets"][0]["columns"].as_u64()), (Some(crate::model::LIBRARY_VERSION as u64), Some(3)), "columns stay for older builds");
        drop(store);
        assert_eq!(Store::open_with(root.clone(), false).unwrap().migrated, None);
        std::fs::remove_dir_all(root).ok();
    }

    /// Version 3 → 4 (D-140): rows become rows of slots with no blanks, so nothing in the
    /// set changes; the old file stays beside it, and blanks saved later are written for
    /// newer builds while `rows` still reads right in older ones.
    #[test]
    fn version_3_libraries_become_slot_grids_and_keep_a_copy() {
        let root = temp_dir();
        let tiles: Vec<String> = (0..5).map(|i| format!(r#"{{"title": "T{i}"}}"#)).collect();
        let old = format!(r#"{{"version": 3, "sets": [{{"name": "Show", "columns": 3, "rows": [2, 3], "tiles": [{}]}}]}}"#, tiles.join(","));
        std::fs::write(root.join("Library.json"), &old).unwrap();
        let mut store = Store::open_with(root.clone(), false).unwrap();
        assert!(store.migrated.as_deref().is_some_and(|m| m.contains("slots")), "{:?}", store.migrated);
        assert_eq!(std::fs::read_to_string(root.join("Library.v3.json")).unwrap(), old);
        let set = &mut store.library.sets[0];
        assert_eq!((set.rows.clone(), set.blanks.clone()), (vec![2, 3], vec![]));
        assert!(set.slots().iter().flatten().all(Option::is_some), "no blanks");
        set.insert_blanks(crate::rows::Place::InRow { row: 0, at: 1 }, 1);
        store.mark_dirty();
        store.save_now().unwrap();
        let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(root.join("Library.json")).unwrap()).unwrap();
        assert_eq!((&saved["sets"][0]["rows"], &saved["sets"][0]["blanks"]), (&serde_json::json!([2, 3]), &serde_json::json!([[0, 1]])));
        drop(store);
        let again = Store::open_with(root.clone(), false).unwrap();
        assert_eq!((again.migrated.clone(), again.library.sets[0].blanks.clone()), (None, vec![[0, 1]]));
        std::fs::remove_dir_all(root).ok();
    }

    /// Modes become the pad options on opening (D-145), with a note; the old mode is
    /// written beside them for older builds.
    #[test]
    fn modes_become_pad_options_on_opening() {
        let root = temp_dir();
        let old = r#"{"version": 3, "sets": [{"name": "Show", "tiles": [{"title": "Bed", "mode": "pause"}, {"title": "Hit", "mode": "restart"}]}]}"#;
        std::fs::write(root.join("Library.json"), old).unwrap();
        let mut store = Store::open_with(root.clone(), false).unwrap();
        assert!(store.migrated.as_deref().is_some_and(|m| m.contains("Always Restart is One-shot")), "{:?}", store.migrated);
        let press: Vec<_> = store.library.sets[0].tiles.iter().map(|t| (t.press.pausable, t.press.one_shot)).collect();
        assert_eq!(press, [(true, false), (false, true)]);
        store.mark_dirty();
        store.save_now().unwrap();
        let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(root.join("Library.json")).unwrap()).unwrap();
        let bed = &saved["sets"][0]["tiles"][0];
        assert_eq!((&bed["pausable"], &bed["mode"]), (&serde_json::json!(true), &serde_json::json!("pause")));
        std::fs::remove_dir_all(root).ok();
    }
}
