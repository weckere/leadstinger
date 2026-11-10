//! Rendered export (M18, D-077, D-078–D-080): a tile written to a WAV or FLAC file with
//! every adjustment applied. A render is the live [`Mixer`] run offline: the tile plays
//! once into a mixer of its own and the blocks are kept, so the file is what the tile
//! sounds like. Jobs run on a thread of their own and report progress through
//! [`Job`]; the library is never touched.

use crate::audio::Mixer;
use crate::decode::{decode_file, DecodedAudio};
use crate::logic::play_range;
use crate::model::{Duck, Tile};
use anyhow::{anyhow, bail, Context, Result};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// Rendered files are stereo.
pub const CHANNELS: usize = 2;
/// The sample rates a render may be asked for.
pub const RATE_MIN: u32 = 8_000;
pub const RATE_MAX: u32 = 192_000;
/// Frames per mixer block, as a device callback would ask for them.
const BLOCK: usize = 1024;
/// A file name's longest stem in a folder export.
const NAME_MAX: usize = 80;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Wav,
    Flac,
}

impl Format {
    pub fn parse(s: &str) -> Option<Format> {
        match s.trim().trim_start_matches('.').to_ascii_lowercase().as_str() {
            "wav" | "wave" => Some(Format::Wav),
            "flac" => Some(Format::Flac),
            _ => None,
        }
    }

    /// The format a path's extension names, if it names one.
    pub fn of_path(path: &Path) -> Option<Format> {
        path.extension().and_then(|e| e.to_str()).and_then(Format::parse)
    }

    pub fn extension(self) -> &'static str {
        match self {
            Format::Wav => "wav",
            Format::Flac => "flac",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Format::Wav => "WAV",
            Format::Flac => "FLAC",
        }
    }
}

/// The tile as a render plays it (D-078): one pass, on its own. A loop plays once and
/// fades out at its end, a vamp section plays straight through, nothing ducks it, and a
/// muted tile is heard (Mute is for the live show, D-251).
pub fn one_pass(tile: &Tile) -> Tile {
    Tile { looping: false, vamp: None, duck: Duck::None, solo: false, muted: false, ..tile.clone() }
}

/// About how many frames a render of `tile` playing `audio` at `rate` has (progress).
pub fn expected_frames(tile: &Tile, audio: &DecodedAudio, rate: u32) -> usize {
    let (start, end) = play_range(tile, audio.duration());
    ((end - start) * rate as f64 / tile.speed.clamp(0.5, 2.0) as f64).ceil().max(0.0) as usize
}

/// Render `tile` playing `audio` (its transposed copy when it has a pitch) at `rate`:
/// interleaved stereo, exactly as the live mixer plays it. `progress` gets the fraction
/// done after each block and returns false to stop (then the result is `None`).
pub fn render_tile(tile: &Tile, audio: Arc<DecodedAudio>, rate: u32, mut progress: impl FnMut(f32) -> bool) -> Option<Vec<f32>> {
    let tile = one_pass(tile);
    let expected = expected_frames(&tile, &audio, rate).max(1);
    let mut mixer = Mixer::new(rate);
    mixer.play(&tile, audio);
    let mut out = Vec::with_capacity((expected + BLOCK) * CHANNELS);
    let mut block = vec![0.0; BLOCK * CHANNELS];
    while !mixer.is_idle() {
        mixer.render(&mut block, CHANNELS);
        out.extend_from_slice(&block[..mixer.sounded() * CHANNELS]);
        if !progress((out.len() / CHANNELS) as f32 / expected as f32) {
            return None;
        }
    }
    Some(out)
}

/// A sample as a 24-bit integer (D-079: rounded, not dithered).
fn to_i24(s: f32) -> i32 {
    (s.clamp(-1.0, 1.0) * 8_388_607.0).round() as i32
}

/// Write interleaved stereo `samples` at `rate` as 24-bit `format` to `path`.
pub fn write(path: &Path, format: Format, rate: u32, samples: &[f32]) -> Result<()> {
    let pcm: Vec<i32> = samples.iter().map(|&s| to_i24(s)).collect();
    match format {
        Format::Wav => crate::wav::write_pcm24(path, rate, CHANNELS as u16, &pcm)?,
        Format::Flac => {
            use flacenc::component::BitRepr;
            use flacenc::error::Verify;
            let config = flacenc::config::Encoder::default().into_verified().map_err(|(_, e)| anyhow!("FLAC encoder settings: {e:?}"))?;
            let source = flacenc::source::MemSource::from_samples(&pcm, CHANNELS, 24, rate as usize);
            let mut stream = flacenc::encode_with_fixed_block_size(&config, source, config.block_size).map_err(|e| anyhow!("FLAC encoding failed: {e:?}"))?;
            // flacenc counts the short last frame as the minimum block size, which makes
            // the stream look variable-blocked to a strict reader (symphonia, so our own
            // import, refuses every frame). The spec excludes the last block: fixed it is.
            stream.stream_info_mut().set_block_sizes(config.block_size, config.block_size).map_err(|e| anyhow!("FLAC encoding failed: {e:?}"))?;
            let mut sink = flacenc::bitsink::ByteSink::new();
            stream.write(&mut sink).map_err(|e| anyhow!("FLAC encoding failed: {e:?}"))?;
            std::fs::write(path, sink.as_slice())?;
        }
    }
    Ok(())
}

/// Where a tile's audio comes from for a render.
pub enum Source {
    /// What the app plays, ready: the clip, or its transposed copy.
    Playable(Arc<DecodedAudio>),
    /// The decoded clip, still to be transposed by the tile's pitch.
    Clip(Arc<DecodedAudio>),
    /// The clip's file, not decoded here yet (the offline CLI, or still loading).
    File(PathBuf),
}

impl Source {
    /// The audio the tile plays, decoding and transposing as the app does.
    fn prepare(self, tile: &Tile) -> Result<Arc<DecodedAudio>> {
        let clip = match self {
            Source::Playable(a) => return Ok(a),
            Source::Clip(a) => a,
            Source::File(path) => Arc::new(decode_file(&path)?),
        };
        // The same rounding as the app's cache (Store::request_pitch), so the copy matches.
        let cents = crate::pitch::cents(tile.pitch);
        Ok(if cents == 0 { clip } else { Arc::new(crate::pitch::shift(&clip, cents as f32 / 100.0)) })
    }
}

/// One file of a job.
pub struct Item {
    pub tile: Tile,
    pub source: Source,
    pub path: PathBuf,
}

/// A job's state, shared with its thread.
#[derive(Default)]
struct Shared {
    /// Files finished (written or failed).
    done: AtomicUsize,
    /// The current file's fraction, as `f32` bits.
    fraction: AtomicU32,
    cancel: AtomicBool,
    finished: AtomicBool,
    /// The tile being rendered.
    current: Mutex<String>,
    written: Mutex<Vec<PathBuf>>,
    errors: Mutex<Vec<String>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// A render under way or done.
pub struct Job {
    pub id: u32,
    /// Every file it will write, in order.
    pub files: Vec<PathBuf>,
    pub format: Format,
    /// Set by the controller once it has told the user the job finished.
    pub reported: bool,
    shared: Arc<Shared>,
}

/// What a finished job did.
pub struct Outcome {
    pub written: Vec<PathBuf>,
    pub errors: Vec<String>,
    pub cancelled: bool,
}

impl Job {
    /// Start rendering `items` as `format` on a thread of its own, at `rate` (or each
    /// source's own rate).
    pub fn start(id: u32, items: Vec<Item>, format: Format, rate: Option<u32>) -> Job {
        let shared = Arc::new(Shared::default());
        let files = items.iter().map(|i| i.path.clone()).collect();
        let s = shared.clone();
        std::thread::Builder::new()
            .name(format!("render-{id}"))
            .spawn(move || {
                for item in items {
                    if s.cancel.load(Ordering::Relaxed) {
                        break;
                    }
                    *lock(&s.current) = item.tile.title.clone();
                    s.fraction.store(0f32.to_bits(), Ordering::Relaxed);
                    match render_item(item, format, rate, &s) {
                        Ok(Some(path)) => lock(&s.written).push(path),
                        Ok(None) => {}
                        Err(e) => lock(&s.errors).push(format!("{e:#}")),
                    }
                    s.done.fetch_add(1, Ordering::Relaxed);
                }
                lock(&s.current).clear();
                s.finished.store(true, Ordering::Release);
            })
            .expect("spawn the render thread");
        Job { id, files, format, reported: false, shared }
    }

    pub fn is_finished(&self) -> bool {
        self.shared.finished.load(Ordering::Acquire)
    }

    pub fn cancel(&self) {
        self.shared.cancel.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.shared.cancel.load(Ordering::Relaxed)
    }

    /// Files finished so far.
    pub fn done(&self) -> usize {
        self.shared.done.load(Ordering::Relaxed)
    }

    /// The whole job's fraction done.
    pub fn fraction(&self) -> f32 {
        if self.is_finished() {
            return 1.0;
        }
        let part = f32::from_bits(self.shared.fraction.load(Ordering::Relaxed)).clamp(0.0, 1.0);
        ((self.done() as f32 + part) / self.files.len().max(1) as f32).min(1.0)
    }

    /// The title of the tile being rendered now.
    pub fn current(&self) -> String {
        lock(&self.shared.current).clone()
    }

    pub fn outcome(&self) -> Outcome {
        Outcome { written: lock(&self.shared.written).clone(), errors: lock(&self.shared.errors).clone(), cancelled: self.is_cancelled() }
    }

    /// Wait for the job to end (the offline CLI, tests).
    pub fn wait(&self) {
        while !self.is_finished() {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
}

/// Render one file: into a `.part` beside it, renamed into place when complete, so a
/// cancelled or failed render leaves nothing behind. `None` when cancelled.
fn render_item(item: Item, format: Format, rate: Option<u32>, s: &Shared) -> Result<Option<PathBuf>> {
    let title = item.tile.title.clone();
    let audio = item.source.prepare(&item.tile).with_context(|| format!("“{title}”"))?;
    let rate = rate.unwrap_or(audio.sample_rate).clamp(RATE_MIN, RATE_MAX);
    let Some(samples) = render_tile(&item.tile, audio, rate, |f| {
        s.fraction.store(f.to_bits(), Ordering::Relaxed);
        !s.cancel.load(Ordering::Relaxed)
    }) else {
        return Ok(None);
    };
    if samples.is_empty() {
        bail!("“{title}” has nothing to render (its trim is empty)");
    }
    let name = item.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let part = item.path.with_file_name(format!(".{name}.part"));
    let result = write(&part, format, rate, &samples).and_then(|()| Ok(std::fs::rename(&part, &item.path)?));
    if let Err(e) = result {
        std::fs::remove_file(&part).ok();
        return Err(e.context(format!("writing {}", item.path.display())));
    }
    Ok(Some(item.path))
}

/// A tile title as a file name: what file systems and shells dislike becomes `-`,
/// control characters and leading dots or spaces go, at most [`NAME_MAX`] characters.
pub fn safe_name(title: &str) -> String {
    let mapped: String = title
        .chars()
        .filter(|c| !c.is_control())
        .map(|c| if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') { '-' } else { c })
        .collect();
    let trimmed = mapped.trim().trim_start_matches(['.', ' ']).trim_end_matches(['.', ' ']);
    trimmed.chars().take(NAME_MAX).collect::<String>().trim_end().to_owned()
}

/// File names in `folder` for tiles at these (1-based) positions with these titles:
/// `NN Title.ext`, none of them an existing file or each other (D-080). A clash gets
/// ` (2)`, ` (3)`…
pub fn folder_names(folder: &Path, tiles: &[(usize, &str)], format: Format) -> Vec<PathBuf> {
    let width = tiles.iter().map(|(pos, _)| pos.to_string().len()).max().unwrap_or(1).max(2);
    let mut taken: Vec<String> = Vec::new();
    tiles
        .iter()
        .map(|&(pos, title)| {
            let name = match safe_name(title) {
                n if n.is_empty() => format!("Tile {pos:0width$}"),
                n => n,
            };
            let stem = format!("{pos:0width$} {name}");
            let free = |f: &str| !taken.iter().any(|t| t.eq_ignore_ascii_case(f)) && !folder.join(f).exists();
            let mut file = format!("{stem}.{}", format.extension());
            let mut n = 2;
            while !free(&file) {
                file = format!("{stem} ({n}).{}", format.extension());
                n += 1;
            }
            taken.push(file.clone());
            folder.join(file)
        })
        .collect()
}

/// `path` with the format's extension when it has none (or another audio one).
pub fn with_extension(path: &Path, format: Format) -> PathBuf {
    match Format::of_path(path) {
        Some(f) if f == format => path.to_path_buf(),
        _ => {
            let mut s = path.as_os_str().to_owned();
            s.push(".");
            s.push(format.extension());
            PathBuf::from(s)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::NativeChannels;
    use crate::model::{ChannelRoute, ChannelTo, FadeCurve};

    const RATE: u32 = 8_000;

    /// A stereo test signal: different sines left and right.
    fn signal(seconds: f64) -> Arc<DecodedAudio> {
        let n = (seconds * RATE as f64) as usize;
        let data = (0..n)
            .flat_map(|i| {
                let t = i as f32 / RATE as f32;
                [0.6 * (t * 330.0 * std::f32::consts::TAU).sin(), 0.4 * (t * 550.0 * std::f32::consts::TAU).sin()]
            })
            .collect();
        Arc::new(DecodedAudio { sample_rate: RATE, data, art: None, native: None })
    }

    fn tile() -> Tile {
        Tile { file: Some("x.wav".into()), ..Default::default() }
    }

    /// The live mixer playing `tile`, pulled in device-sized blocks until it's done.
    fn live(tile: &Tile, audio: Arc<DecodedAudio>, rate: u32, block: usize) -> Vec<f32> {
        let mut m = Mixer::new(rate);
        m.play(tile, audio);
        let mut out = Vec::new();
        let mut buf = vec![0.0; block * 2];
        while !m.is_idle() {
            m.render(&mut buf, 2);
            out.extend_from_slice(&buf[..m.sounded() * 2]);
        }
        out
    }

    fn close(a: &[f32], b: &[f32]) {
        assert_eq!(a.len(), b.len());
        let worst = a.iter().zip(b).map(|(x, y)| (x - y).abs()).fold(0.0, f32::max);
        assert!(worst < 1e-6, "differs by {worst}");
    }

    #[test]
    fn a_render_is_what_the_live_mixer_plays() {
        let t = Tile {
            start: 0.25,
            end: Some(1.5),
            fade_in: 0.2,
            fade_out: 0.3,
            fade_curve: FadeCurve::SCurve,
            volume: 0.7,
            gain_db: 4.0,
            speed: 1.25,
            ..tile()
        };
        let audio = signal(2.0);
        let rendered = render_tile(&t, audio.clone(), RATE, |_| true).unwrap();
        close(&rendered, &live(&t, audio.clone(), RATE, 441));
        // At another rate too (the mixer resamples).
        let at = render_tile(&t, audio.clone(), 11_025, |_| true).unwrap();
        close(&at, &live(&t, audio, 11_025, 256));
    }

    #[test]
    fn trim_and_fades_land_on_the_right_samples() {
        // A constant 0.5, trimmed to 0.5–1.5 s, a 0.25 s linear fade in and out.
        let dc = Arc::new(DecodedAudio { sample_rate: RATE, data: vec![0.5; 2 * RATE as usize * 2], art: None, native: None });
        let t = Tile { start: 0.5, end: Some(1.5), fade_in: 0.25, fade_out: 0.25, fade_curve: FadeCurve::Linear, ..tile() };
        let out = render_tile(&t, dc, RATE, |_| true).unwrap();
        let left: Vec<f32> = out.chunks(2).map(|f| f[0]).collect();
        assert_eq!(left.len(), RATE as usize, "exactly the trimmed second");
        let at = |s: f64| left[(s * RATE as f64) as usize];
        // The fade in rises (after the mixer's ~5 ms gain smoothing) …
        assert!(at(0.0) < 0.01, "{}", at(0.0));
        assert!((at(0.125) - 0.25).abs() < 0.02, "{}", at(0.125));
        // … the middle is full level …
        assert!((at(0.5) - 0.5).abs() < 1e-3, "{}", at(0.5));
        // … and the fade out falls to nothing at the very end.
        assert!((at(0.875) - 0.25).abs() < 0.02, "{}", at(0.875));
        // (The 5 ms smoothing lags the ramp by about 0.01 at the very end.)
        assert!(left[left.len() - 1] < 0.02, "{}", left[left.len() - 1]);
    }

    #[test]
    fn loops_and_vamps_render_one_pass() {
        let audio = signal(1.0);
        let looped = Tile { looping: true, fade_out: 0.1, vamp: Some((0.2, 0.4)), ..tile() };
        let out = render_tile(&looped, audio.clone(), RATE, |_| true).unwrap();
        assert_eq!(out.len(), RATE as usize * 2, "one pass of the second");
        // The fade-out applies, as if Loop were off.
        let plain = Tile { fade_out: 0.1, ..tile() };
        close(&out, &render_tile(&plain, audio, RATE, |_| true).unwrap());
    }

    #[test]
    fn plays_the_channel_mix() {
        // Three native channels; the mix sends the third one to the left only.
        let native = NativeChannels { data: (0..RATE * 3).map(|i| [0.1, 0.2, 0.3][(i % 3) as usize]).collect(), labels: vec!["L".into(), "R".into(), "C".into()], downmix: vec![(1.0, 0.0), (0.0, 1.0), (0.5, 0.5)] };
        let audio = Arc::new(DecodedAudio { sample_rate: RATE, data: vec![0.0; RATE as usize * 2], art: None, native: Some(native) });
        let route = |to| ChannelRoute { to, gain_db: 0.0 };
        let t = Tile { channel_mix: vec![route(ChannelTo::Mute), route(ChannelTo::Mute), route(ChannelTo::Left)], ..tile() };
        let out = render_tile(&t, audio.clone(), RATE, |_| true).unwrap();
        let mid = &out[RATE as usize..RATE as usize + 2];
        assert!((mid[0] - 0.3).abs() < 1e-4 && mid[1].abs() < 1e-6, "{mid:?}");
        close(&out, &live(&t, audio, RATE, 512));
    }

    #[test]
    fn stops_when_cancelled() {
        let mut calls = 0;
        assert!(render_tile(&tile(), signal(1.0), RATE, |_| {
            calls += 1;
            calls < 2
        })
        .is_none());
    }

    /// Read a file back with symphonia, as the app would import it.
    fn decode(path: &Path) -> Vec<f32> {
        let a = decode_file(path).unwrap();
        assert_eq!(a.sample_rate, RATE);
        a.data
    }

    #[test]
    fn wav_and_flac_decode_back_to_the_same_samples() {
        let dir = std::env::temp_dir().join(format!("render-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let t = Tile { fade_in: 0.1, fade_out: 0.2, gain_db: -3.0, speed: 0.8, ..tile() };
        let samples = render_tile(&t, signal(0.75), RATE, |_| true).unwrap();
        let (wav, flac) = (dir.join("a.wav"), dir.join("a.flac"));
        write(&wav, Format::Wav, RATE, &samples).unwrap();
        write(&flac, Format::Flac, RATE, &samples).unwrap();
        assert_eq!(&std::fs::read(&flac).unwrap()[..4], b"fLaC");
        let (w, f) = (decode(&wav), decode(&flac));
        // Lossless: the FLAC holds exactly the WAV's 24-bit samples …
        assert_eq!(w, f);
        // … which are the render's to within 24-bit rounding (and symphonia's 2^23 scale).
        assert_eq!(w.len(), samples.len());
        let worst = w.iter().zip(&samples).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
        assert!(worst <= 2.0 / 8_388_607.0, "{worst}");
        // A stinger shorter than one FLAC block reads back too.
        let short = render_tile(&Tile { end: Some(0.05), ..tile() }, signal(0.75), RATE, |_| true).unwrap();
        write(&flac, Format::Flac, RATE, &short).unwrap();
        assert_eq!(decode(&flac).len(), short.len());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn jobs_write_files_and_leave_nothing_when_they_fail() {
        let dir = std::env::temp_dir().join(format!("render-job-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let items = vec![
            Item { tile: tile(), source: Source::Playable(signal(0.5)), path: dir.join("one.flac") },
            Item { tile: Tile { title: "Gone".into(), ..tile() }, source: Source::File(dir.join("missing.wav")), path: dir.join("two.flac") },
        ];
        let job = Job::start(7, items, Format::Flac, Some(16_000));
        job.wait();
        let out = job.outcome();
        assert_eq!(out.written, [dir.join("one.flac")]);
        assert_eq!(out.errors.len(), 1, "{:?}", out.errors);
        assert!(out.errors[0].contains("Gone"), "{:?}", out.errors);
        assert_eq!(job.fraction(), 1.0);
        let a = decode_file(&dir.join("one.flac")).unwrap();
        assert_eq!((a.sample_rate, a.data.len()), (16_000, 8_000 * 2));
        let names: Vec<String> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(names, ["one.flac"], "no .part files left");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn folder_names_are_safe_and_unique() {
        let dir = std::env::temp_dir().join(format!("render-names-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("01 Intro.wav"), b"").unwrap();
        let long = "x".repeat(200);
        let tiles = [(1, "Intro"), (2, "a/b: c?"), (3, "..hidden"), (4, ""), (5, "Dup"), (5, "dup"), (6, "Tab\there"), (7, long.as_str())];
        let names: Vec<String> = folder_names(&dir, &tiles, Format::Wav).iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(&names[..7], ["01 Intro (2).wav", "02 a-b- c-.wav", "03 hidden.wav", "04 Tile 04.wav", "05 Dup.wav", "05 dup (2).wav", "06 Tabhere.wav"]);
        assert_eq!(names[7], format!("07 {}.wav", "x".repeat(NAME_MAX)));
        assert!(folder_names(&dir, &[(100, "A")], Format::Flac)[0].ends_with("100 A.flac"));
        assert_eq!(with_extension(Path::new("/t/a"), Format::Flac), Path::new("/t/a.flac"));
        assert_eq!(with_extension(Path::new("/t/a.FLAC"), Format::Flac), Path::new("/t/a.FLAC"));
        assert_eq!(with_extension(Path::new("/t/a.wav"), Format::Flac), Path::new("/t/a.wav.flac"));
        std::fs::remove_dir_all(&dir).ok();
    }
}
