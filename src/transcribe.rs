//! Transcription: subtitles for a tile that has none, and word timings for subtitles that
//! have none (D-277 to D-285), free of UI. Runs only when asked, never on import.
//!
//! Every engine gets the same thing, the tile's audio as mono 16 kHz WAV ([`prepare`]),
//! and gives back the same thing, lines with their words in seconds into that WAV
//! ([`Heard`]). The shared steps then make it the tile's: the times are moved into the
//! file's frame, each word's start is moved onto the sound it starts on ([`refine`]), and
//! the result becomes new machine-made lines or, for a tile that already has subtitles,
//! timings for the known words ([`align_text`]: the known text is never replaced by
//! what an engine heard). [`apply`] does that last choice; the controller makes it one
//! undo step (src/controller/transcribe.rs).
//!
//! Engines (D-277): a local `whisper-cli` ([`whisper_cli`], an optional,
//! separately installed program found like ffmpeg); Apple's on-device speech on macOS
//! and iPadOS ([`apple`], D-286 to D-289: the system's own recogniser, only after the
//! user allowed it in Settings); and a connected QuipClipper server
//! ([`crate::quip::transcribe`], D-290 to D-293, which is sent the audio). None is
//! compiled in and none downloads a model.

pub mod whisper_cli;
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub mod apple;
pub mod apple_core;

use crate::align::{self, Onset, MIN_WORD};
use crate::decode::DecodedAudio;
use crate::subtitles::{SourceTiming, SubtitleLine, SubtitleSource, SubtitleWord, MAX_WORD, MAX_WORDS};
use crate::transcode::Progress;
use serde_json::{json, Map, Value};
use std::path::PathBuf;
use std::sync::Arc;

/// The sample rate every engine gets.
pub const RATE: u32 = 16_000;
/// How far [`refine`] moves a word's start to reach a sound's start, in seconds: back as
/// far as `earlier`, on as far as `later` (D-281). The onset finder marks where a sound
/// gets loud, which for a word starting on a stop ("concert") or running on from the
/// word before ("…and enjoy") is a syllable later than the word starts; a real run
/// moved such words 0.14 s too late when the reach was the same both ways.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reach {
    pub earlier: f64,
    pub later: f64,
}

pub const REFINE_REACH: Reach = Reach { earlier: 0.15, later: 0.05 };
/// The `kind` of a machine-made source, as QuipClipper writes it.
pub const MACHINE: &str = "machine";
/// What a cancelled transcription reports.
pub const CANCELLED: &str = "Transcription cancelled";

// ── engines ─────────────────────────────────────────────────────────────────

/// A transcription engine. To add one: a variant here, its names in [`EngineKind::name`],
/// [`EngineKind::label`] and [`EngineKind::parse`], a place in [`AUTO_ORDER`], and an
/// arm in [`open`] that gives a [`Engine`] or says why it can't.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EngineKind {
    /// whisper.cpp's `whisper-cli`, installed separately ([`whisper_cli`]).
    WhisperCli,
    /// A connected QuipClipper server that transcribes uploads ([`crate::quip::transcribe`]).
    Quip,
    /// Apple's on-device speech recognition, macOS and iPadOS ([`apple`]).
    AppleSpeech,
}

/// Which engine `auto` picks: the first available in this order (D-279). Apple's
/// on-device speech needs nothing installed and sends nothing anywhere; whisper-cli
/// needs a program and a model the user installed; QuipClipper sends the audio to a
/// server.
pub const AUTO_ORDER: &[EngineKind] = &[EngineKind::AppleSpeech, EngineKind::WhisperCli, EngineKind::Quip];

impl EngineKind {
    /// The name settings and commands use.
    pub fn name(self) -> &'static str {
        match self {
            EngineKind::WhisperCli => "whisper",
            EngineKind::Quip => "quip",
            EngineKind::AppleSpeech => "apple",
        }
    }

    /// What the UI calls it.
    pub fn label(self) -> &'static str {
        match self {
            EngineKind::WhisperCli => "whisper-cli",
            EngineKind::Quip => "QuipClipper",
            EngineKind::AppleSpeech => "Apple Speech",
        }
    }

    /// The `engine` a machine-made source records.
    pub fn source_name(self) -> &'static str {
        match self {
            EngineKind::WhisperCli => "whisper.cpp",
            EngineKind::Quip => "quipclipper",
            EngineKind::AppleSpeech => "apple-speech",
        }
    }

    pub fn parse(s: &str) -> Option<EngineKind> {
        match s.trim().to_ascii_lowercase().as_str() {
            "whisper" | "whisper-cli" | "whisper.cpp" => Some(EngineKind::WhisperCli),
            "quip" | "quipclipper" => Some(EngineKind::Quip),
            "apple" | "apple-speech" | "speech" => Some(EngineKind::AppleSpeech),
            _ => None,
        }
    }

    /// Whether this build has it.
    pub fn built(self) -> bool {
        match self {
            EngineKind::WhisperCli => true,
            EngineKind::AppleSpeech => cfg!(any(target_os = "macos", target_os = "ios")),
            EngineKind::Quip => true,
        }
    }
}

/// The engine setting: `auto`, or one engine.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Choice {
    #[default]
    Auto,
    One(EngineKind),
}

impl Choice {
    pub fn parse(s: &str) -> Option<Choice> {
        if s.trim().eq_ignore_ascii_case("auto") || s.trim().is_empty() {
            return Some(Choice::Auto);
        }
        EngineKind::parse(s).map(Choice::One)
    }

    pub fn name(self) -> &'static str {
        match self {
            Choice::Auto => "auto",
            Choice::One(k) => k.name(),
        }
    }
}

/// What engines need to know to say whether they can run: the settings, and (for the
/// lanes to come) what's connected. Built by the controller on the UI thread.
#[derive(Clone, Debug, Default)]
pub struct Context {
    /// Settings ▸ Subtitles' whisper model file (`LEADSTINGER_WHISPER_MODEL` wins).
    pub whisper_model: Option<PathBuf>,
    /// Settings ▸ Subtitles' language (`None`: the engine's own default), for the
    /// engines that check it up front (Apple Speech: is there an on-device model?).
    pub lang: Option<String>,
    /// The connected QuipClipper servers and whether each transcribes, as last asked
    /// ([`crate::quip::transcribe::offers`]).
    pub quip: Vec<crate::quip::transcribe::Offer>,
    /// The QuipClipper server chosen in Settings, if any.
    pub quip_server: Option<String>,
    /// The library folder (where a server's token is kept).
    pub data_dir: Option<PathBuf>,
}

/// What an engine is given: the audio as [`prepare`] made it, and the language asked
/// for (`None`: the engine's own default or its guess).
#[derive(Clone, Debug)]
pub struct Request {
    /// Mono 16-bit 16 kHz WAV.
    pub wav: Arc<Vec<u8>>,
    /// Its length in seconds.
    pub seconds: f64,
    /// An ISO 639-1 code (`en`), or none.
    pub lang: Option<String>,
}

/// What an engine heard: lines in time order, each with its words where the engine gives
/// them, in seconds into the request's WAV; and what it used, for the source.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Heard {
    pub lines: Vec<SubtitleLine>,
    /// The model's name, e.g. `base.en`.
    pub model: Option<String>,
    /// The language it heard or was told.
    pub lang: Option<String>,
}

/// An engine, opened and ready to run. `transcribe` runs on a worker thread: it sets
/// `progress`'s fraction when it knows how far along it is, checks
/// `progress.is_cancelled()` often (it may hand `progress` to a thread of its own) (killing any program it started) and then returns
/// `Err(CANCELLED)`, and otherwise fails with a sentence that finishes "Couldn't
/// transcribe “Tile”: …".
pub trait Engine: Send {
    fn kind(&self) -> EngineKind;
    fn transcribe(&self, req: &Request, progress: &Arc<Progress>) -> Result<Heard, String>;
    /// Where the audio is sent, for an engine that sends it off this computer
    /// ("QuipClipper at Name (host)"): the UI says so before a run.
    fn destination(&self) -> Option<String> {
        None
    }
}

/// Open `kind`, or say in a sentence why it can't run here and what would fix it.
pub fn open(kind: EngineKind, ctx: &Context) -> Result<Box<dyn Engine>, String> {
    match kind {
        EngineKind::WhisperCli => whisper_cli::WhisperCli::open(ctx).map(|e| Box::new(e) as Box<dyn Engine>),
        EngineKind::Quip => crate::quip::transcribe::Quip::open(ctx).map(|e| Box::new(e) as Box<dyn Engine>),
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        EngineKind::AppleSpeech => apple::open(ctx).map(|e| Box::new(e) as Box<dyn Engine>),
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        EngineKind::AppleSpeech => Err("Apple Speech is only on macOS and iPadOS".to_owned()),
    }
}

/// One engine's availability, for `transcribe.engines` and Settings.
#[derive(Clone, Debug, PartialEq)]
pub struct Availability {
    pub kind: EngineKind,
    pub available: bool,
    /// Why not, and what to do about it.
    pub reason: Option<String>,
}

/// Every engine this build has, in `auto` order, with whether it can run.
pub fn availability(ctx: &Context) -> Vec<Availability> {
    AUTO_ORDER
        .iter()
        .filter(|k| k.built())
        .map(|&kind| match open(kind, ctx) {
            Ok(_) => Availability { kind, available: true, reason: None },
            Err(why) => Availability { kind, available: false, reason: Some(why) },
        })
        .collect()
}

/// The engine to use: the one asked for, or `auto`'s first available. The error says
/// why none can run (the first engine's reason, which is the one to fix).
pub fn pick(choice: Choice, ctx: &Context) -> Result<Box<dyn Engine>, String> {
    match choice {
        Choice::One(kind) => open(kind, ctx),
        Choice::Auto => {
            let mut first_reason = None;
            for &kind in AUTO_ORDER.iter().filter(|k| k.built()) {
                match open(kind, ctx) {
                    Ok(e) => return Ok(e),
                    Err(why) => {
                        first_reason.get_or_insert(why);
                    }
                }
            }
            Err(first_reason.unwrap_or_else(|| "no transcription engine is available".to_owned()))
        }
    }
}

// ── prepare ─────────────────────────────────────────────────────────────────

/// The tile's audio for an engine: `range` (seconds into the file; `None` the whole
/// file) as mono 16 kHz WAV. A time `t` in it is `offset + t` in the file.
#[derive(Clone, Debug)]
pub struct Prepared {
    pub wav: Vec<u8>,
    pub offset: f64,
    pub seconds: f64,
}

/// Mix `audio` down to mono, resample it to [`RATE`] and write it as 16-bit WAV.
pub fn prepare(audio: &DecodedAudio, range: Option<(f64, f64)>) -> Prepared {
    let rate = f64::from(audio.sample_rate.max(1));
    let frames = audio.frames();
    let (from, to) = match range {
        Some((a, b)) => (((a.max(0.0) * rate) as usize).min(frames), ((b.max(0.0) * rate).ceil() as usize).min(frames)),
        None => (0, frames),
    };
    let to = to.max(from);
    let mono: Vec<f32> = audio.data[from * 2..to * 2].as_chunks::<2>().0.iter().map(|[l, r]| (l + r) * 0.5).collect();
    let out = resample(&mono, audio.sample_rate.max(1), RATE);
    Prepared { seconds: out.len() as f64 / f64::from(RATE), wav: crate::wav::mono_bytes(RATE, &out), offset: from as f64 / rate }
}

/// Zero crossings either side of the resampler's kernel.
const SINC_ZEROS: f64 = 16.0;

/// Windowed-sinc resampling of mono audio from `from` Hz to `to` Hz. Going down, the
/// kernel's cutoff is 95 % of the new Nyquist frequency, so what can't be kept is
/// filtered out rather than folded back as noise. Each output sample is `x = n · from /
/// to` in the input: the length (and every time) scales exactly.
pub fn resample(input: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || input.is_empty() {
        return input.to_vec();
    }
    let ratio = f64::from(to) / f64::from(from);
    let n_out = ((input.len() as f64) * ratio).round() as usize;
    // The cutoff, as a fraction of the input's Nyquist frequency.
    let cut = ratio.min(1.0) * 0.95;
    let half = (SINC_ZEROS / cut).ceil() as isize;
    let step = f64::from(from) / f64::from(to);
    let len = input.len() as isize;
    (0..n_out)
        .map(|n| {
            let x = n as f64 * step;
            let centre = x.floor() as isize;
            let (lo, hi) = ((centre - half + 1).max(0), (centre + half).min(len - 1));
            let mut acc = 0.0f64;
            let mut weight = 0.0f64;
            for k in lo..=hi {
                let d = x - k as f64;
                let h = cut * sinc(cut * d) * blackman(d / half as f64);
                acc += f64::from(input[k as usize]) * h;
                weight += h;
            }
            // The kernel sums to about 1; dividing by its sum keeps the level at the
            // file's ends too, where it's cut short.
            (if weight.abs() > 1e-9 { acc / weight } else { 0.0 }) as f32
        })
        .collect()
}

fn sinc(x: f64) -> f64 {
    if x.abs() < 1e-9 {
        1.0
    } else {
        let px = std::f64::consts::PI * x;
        px.sin() / px
    }
}

/// The Blackman window over -1..1.
fn blackman(u: f64) -> f64 {
    if u.abs() >= 1.0 {
        return 0.0;
    }
    let a = std::f64::consts::PI * (u + 1.0);
    0.42 - 0.5 * a.cos() + 0.08 * (2.0 * a).cos()
}

// ── refine ──────────────────────────────────────────────────────────────────

/// Move each word's start to the nearest sound start within `reach`, keeping the
/// words in order and each at least [`MIN_WORD`] long; then each word ends where the next
/// starts if it ran past it, and the line spans its words. Lines without words are left
/// as they are. Never marks a line `aligned` (placed by hand, D-261). Returns how far
/// each word's start moved, in seconds (+ later).
pub fn refine(lines: &mut [SubtitleLine], onsets: &[Onset], reach: Reach) -> Vec<f64> {
    let mut moved = Vec::new();
    for line in lines.iter_mut() {
        let n = line.words.len();
        let mut prev = f64::NEG_INFINITY;
        for i in 0..n {
            let next = line.words.get(i + 1).map_or(f64::INFINITY, |w| w.start);
            let w = &mut line.words[i];
            let want = onsets
                .iter()
                .map(|o| o.at)
                .filter(|&at| at >= w.start - reach.earlier && at <= w.start + reach.later)
                .min_by(|a, b| (a - w.start).abs().total_cmp(&(b - w.start).abs()))
                .unwrap_or(w.start);
            let (lo, hi) = (prev + MIN_WORD, next - MIN_WORD);
            let to = if lo <= hi { want.clamp(lo, hi) } else { w.start };
            moved.push(to - w.start);
            w.start = to;
            if w.end < to + MIN_WORD {
                w.end = (to + MIN_WORD).min(next.max(to + MIN_WORD));
            }
            prev = to;
        }
        for i in 0..n.saturating_sub(1) {
            let next = line.words[i + 1].start;
            let w = &mut line.words[i];
            if w.end > next {
                w.end = next;
            }
        }
        if let (Some(a), Some(b)) = (line.words.first(), line.words.last()) {
            line.start = a.start;
            line.end = b.end.max(a.start + MIN_WORD);
        }
    }
    moved
}

// ── align known text ────────────────────────────────────────────────────────

/// What [`align_text`] did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AlignReport {
    /// Lines given word timings.
    pub lines: usize,
    /// Words that took a heard word's time.
    pub matched: usize,
    /// Words placed between them.
    pub placed: usize,
    /// Lines left as they were: nothing heard matched them.
    pub missed: usize,
    /// The heard words that known words took the time of: they were the known lines'
    /// words, so never a gap line's ([`gap_lines`]).
    pub used: Vec<SubtitleWord>,
}

/// A word as matching compares it: folded (case and accents, [`crate::search::fold`]),
/// letters and digits only.
pub fn match_key(word: &str) -> String {
    crate::search::fold(word).chars().filter(|c| c.is_alphanumeric()).collect()
}

/// Whether two keys are the same word heard slightly differently: equal, or one or two
/// letters apart in a longer word.
fn near(a: &str, b: &str) -> bool {
    let longest = a.chars().count().max(b.chars().count());
    let limit = match longest {
        0..=3 => 0,
        4..=6 => 1,
        _ => 2,
    };
    limit > 0 && edit_distance(a, b) <= limit
}

fn edit_distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cur = row[j + 1];
            row[j + 1] = (prev + usize::from(ca != cb)).min(row[j] + 1).min(cur + 1);
            prev = cur;
        }
    }
    row[b.len()]
}

/// The most cells one alignment table may have; past it each line is aligned with the
/// words heard near it.
const MAX_CELLS: usize = 8_000_000;

/// A plain sequence alignment of known words with heard words, as an edit distance with
/// costs: the same word 0, nearly the same 1, another word in its place 3, a word missing
/// from either side 2. Each known word gets the heard word it lines up with, if any, and
/// whether they're the same word (or nearly).
fn align_words(known: &[String], heard: &[String]) -> Vec<Option<(usize, bool)>> {
    let (n, m) = (known.len(), heard.len());
    let sub = |i: usize, j: usize| -> u32 {
        if !known[i].is_empty() && known[i] == heard[j] {
            0
        } else if near(&known[i], &heard[j]) {
            1
        } else {
            3
        }
    };
    let w = m + 1;
    let mut cost = vec![0u32; (n + 1) * w];
    // 0 diagonal, 1 skip a known word, 2 skip a heard word.
    let mut from = vec![0u8; (n + 1) * w];
    for i in 1..=n {
        cost[i * w] = 2 * i as u32;
        from[i * w] = 1;
    }
    for j in 1..=m {
        cost[j] = 2 * j as u32;
        from[j] = 2;
    }
    for i in 1..=n {
        for j in 1..=m {
            let d = cost[(i - 1) * w + j - 1] + sub(i - 1, j - 1);
            let k = cost[(i - 1) * w + j] + 2;
            let h = cost[i * w + j - 1] + 2;
            let (c, f) = if d <= k && d <= h { (d, 0) } else if k <= h { (k, 1) } else { (h, 2) };
            cost[i * w + j] = c;
            from[i * w + j] = f;
        }
    }
    let mut out = vec![None; n];
    let (mut i, mut j) = (n, m);
    while i > 0 || j > 0 {
        match from[i * w + j] {
            0 if i > 0 && j > 0 => {
                out[i - 1] = Some((j - 1, sub(i - 1, j - 1) < 3));
                i -= 1;
                j -= 1;
            }
            1 if i > 0 => i -= 1,
            _ => j -= 1,
        }
    }
    out
}

/// A rough length for unplaced words, in seconds: about 15 characters a second.
fn spoken_length(words: &[&str]) -> f64 {
    words.iter().map(|w| w.chars().count().max(1) as f64 * 0.065).sum::<f64>().max(MIN_WORD * words.len() as f64)
}

/// Give word timings to the lines `targets` of `lines` (lines placed by hand are
/// skipped; a line's own words, if it has any, are timed again), from the words an engine heard (in seconds into
/// the file). The known words keep their text: each takes the time of the heard word it
/// lines up with, and the ones between are spread over the time between those by their
/// length. A line that nothing heard lines up with is left as it was. A changed line
/// keeps its old timing in `source_timing` (so Reset puts it back) and isn't marked
/// `aligned`.
pub fn align_text(lines: &mut [SubtitleLine], targets: &[usize], heard: &[SubtitleWord]) -> AlignReport {
    let targets: Vec<usize> = targets.iter().copied().filter(|&i| lines.get(i).is_some_and(|l| !l.aligned)).collect();
    let mut report = AlignReport::default();
    if targets.is_empty() {
        return report;
    }
    // Heard words inside a line that already has its words belong to that line.
    let owned = |w: &SubtitleWord| {
        let mid = (w.start + w.end) * 0.5;
        lines.iter().enumerate().any(|(i, l)| !targets.contains(&i) && !l.words.is_empty() && mid >= l.start && mid <= l.end)
    };
    let heard: Vec<&SubtitleWord> = heard.iter().filter(|w| !owned(w)).collect();
    // The known words, line by line.
    let known: Vec<(usize, Vec<String>)> =
        targets.iter().map(|&i| (i, lines[i].text.split_whitespace().take(MAX_WORDS).map(|w| w.chars().take(MAX_WORD).collect()).collect())).collect();
    let total: usize = known.iter().map(|(_, w)| w.len()).sum();
    let heard_keys: Vec<String> = heard.iter().map(|w| match_key(&w.text)).collect();
    // Which heard word each known word lines up with, per line.
    let mut matches: Vec<Vec<Option<(usize, bool)>>> = Vec::new();
    if total * heard.len() <= MAX_CELLS {
        let keys: Vec<String> = known.iter().flat_map(|(_, ws)| ws.iter().map(|w| match_key(w))).collect();
        let all = align_words(&keys, &heard_keys);
        let mut at = 0;
        for (_, ws) in &known {
            matches.push(all[at..at + ws.len()].to_vec());
            at += ws.len();
        }
    } else {
        for (li, ws) in &known {
            let l = &lines[*li];
            let near_line: Vec<usize> = (0..heard.len()).filter(|&j| heard[j].end > l.start - 1.0 && heard[j].start < l.end + 1.0).collect();
            let keys: Vec<String> = ws.iter().map(|w| match_key(w)).collect();
            let sub: Vec<String> = near_line.iter().map(|&j| heard_keys[j].clone()).collect();
            matches.push(align_words(&keys, &sub).into_iter().map(|m| m.map(|(k, good)| (near_line[k], good))).collect());
        }
    }
    for ((li, ws), found) in known.iter().zip(matches) {
        // Only a line with at least one word heard as itself is trusted: otherwise the
        // words that line up with it are other words.
        if ws.is_empty() || !found.iter().any(|m| m.is_some_and(|(_, good)| good)) {
            report.missed += 1;
            continue;
        }
        let found: Vec<Option<usize>> = found.into_iter().map(|m| m.map(|(j, _)| j)).collect();
        let words = place(ws, &found, &heard, &lines[*li]);
        report.matched += found.iter().filter(|m| m.is_some()).count();
        report.used.extend(found.iter().flatten().map(|&j| heard[j].clone()));
        report.placed += found.iter().filter(|m| m.is_none()).count();
        report.lines += 1;
        let line = &mut lines[*li];
        if line.source_timing.is_none() {
            line.source_timing = Some(SourceTiming { start: line.start, end: line.end, words: line.words.clone() });
        }
        line.start = words[0].start;
        line.end = words[words.len() - 1].end;
        line.words = words;
        line.aligned = false;
    }
    report
}

/// The known words `ws` with times: the matched ones from what was heard, the rest
/// spread between them.
fn place(ws: &[String], found: &[Option<usize>], heard: &[&SubtitleWord], line: &SubtitleLine) -> Vec<SubtitleWord> {
    let n = ws.len();
    let mut times: Vec<Option<(f64, f64)>> = found.iter().map(|m| m.map(|j| (heard[j].start, heard[j].end))).collect();
    let mut i = 0;
    while i < n {
        if times[i].is_some() {
            i += 1;
            continue;
        }
        let run_end = (i..n).find(|&k| times[k].is_some()).unwrap_or(n);
        let run: Vec<&str> = ws[i..run_end].iter().map(String::as_str).collect();
        let before = i.checked_sub(1).and_then(|k| times[k]).map(|t| t.1);
        let after = times.get(run_end).copied().flatten().map(|t| t.0);
        let est = spoken_length(&run);
        let (lo, hi) = match (before, after) {
            (Some(a), Some(b)) => (a, b.max(a)),
            (None, Some(b)) => ((b - est).max(line.start.min(b)).max(b - 2.0 * est).max(0.0), b),
            (Some(a), None) => (a, (a + est).min(line.end.max(a)).max(a + est * 0.5)),
            (None, None) => (line.start, line.end),
        };
        let total: f64 = run.iter().map(|w| w.chars().count().max(1) as f64).sum();
        let mut at = lo;
        for (k, w) in run.iter().enumerate() {
            let len = (hi - lo) * w.chars().count().max(1) as f64 / total;
            times[i + k] = Some((at, at + len));
            at += len;
        }
        i = run_end;
    }
    // In order, none shorter than MIN_WORD.
    let mut out: Vec<SubtitleWord> = Vec::with_capacity(n);
    for (k, w) in ws.iter().enumerate() {
        let (mut s, mut e) = times[k].unwrap_or((line.start, line.end));
        if let Some(prev) = out.last_mut() {
            // A word heard running into the next ends where the next starts.
            s = s.max(prev.start + MIN_WORD);
            prev.end = prev.end.min(s);
        }
        e = e.max(s + MIN_WORD);
        out.push(SubtitleWord { start: s, end: e, text: w.clone() });
    }
    out
}

/// Whether two sets of lines are the same but for times a microsecond apart (the
/// library file keeps times a little shorter than they're worked out), so a repeated
/// transcription that hears the same changes nothing.
pub fn same_lines(a: &[SubtitleLine], b: &[SubtitleLine]) -> bool {
    let t = |x: f64, y: f64| (x - y).abs() < 1e-6;
    let words = |x: &[SubtitleWord], y: &[SubtitleWord]| x.len() == y.len() && x.iter().zip(y).all(|(p, q)| p.text == q.text && t(p.start, q.start) && t(p.end, q.end));
    let timing = |x: &Option<SourceTiming>, y: &Option<SourceTiming>| match (x, y) {
        (None, None) => true,
        (Some(p), Some(q)) => t(p.start, q.start) && t(p.end, q.end) && words(&p.words, &q.words),
        _ => false,
    };
    a.len() == b.len()
        && a.iter().zip(b).all(|(x, y)| x.text == y.text && x.aligned == y.aligned && x.machine == y.machine && t(x.start, y.start) && t(x.end, y.end) && words(&x.words, &y.words) && timing(&x.source_timing, &y.source_timing))
}

// ── apply ───────────────────────────────────────────────────────────────────

// ── gap lines ───────────────────────────────────────────────────────────────

/// The shortest stretch between two known lines that heard lines are added to, in
/// seconds (D-296). Shorter is a breath, or the engine's slop around a known line.
pub const MIN_GAP: f64 = 0.5;
/// A heard word whose middle is this close outside a known line, and that is one of
/// that line's first or last word, is that line's word heard early or late (an
/// engine's word times are this loose), so it's never a gap line's (D-297).
pub const GAP_EDGE: f64 = 0.25;

/// Whether a line was changed by a person since it was made: placed by hand, or changed
/// in the editor (which keeps the old timing in `source_timing`).
pub fn touched(line: &SubtitleLine) -> bool {
    line.aligned || line.source_timing.is_some()
}

/// The lines heard where no line of `covered` is spoken (D-296): each heard line's
/// words that fall in one gap between the covered lines (or before the first, or after
/// the last) become a line of their own, marked `machine` with `source`. A word whose
/// middle is inside a covered line is that line's; so is one just outside it (within
/// [`GAP_EDGE`]) that is its first or last word. Words in a gap shorter
/// than [`MIN_GAP`] are left out, and a word reaching over a gap's edge is cut at it.
/// None of the lines overlaps a covered line or another, and none is `aligned`.
pub fn gap_lines(covered: &[SubtitleLine], heard: &[SubtitleLine], source: &SubtitleSource) -> Vec<SubtitleLine> {
    // The covered stretches, joined where they overlap.
    let mut spans: Vec<(f64, f64)> = covered.iter().map(|l| (l.start, l.end)).collect();
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut merged: Vec<(f64, f64)> = Vec::new();
    for (s, e) in spans {
        match merged.last_mut() {
            Some(last) if s <= last.1 => last.1 = last.1.max(e),
            _ => merged.push((s, e)),
        }
    }
    // Each covered line's first and last word, as matching compares them.
    let edges: Vec<(f64, f64, String, String)> = covered
        .iter()
        .map(|l| {
            let ws: Vec<String> = if l.words.is_empty() { l.text.split_whitespace().map(match_key).collect() } else { l.words.iter().map(|w| match_key(&w.text)).collect() };
            let first = ws.first().cloned().unwrap_or_default();
            let last = ws.last().cloned().unwrap_or_default();
            (l.start, l.end, first, last)
        })
        .collect();
    let owned = |t: f64, key: &str| {
        edges.iter().any(|(s, e, first, last)| {
            (t >= *s && t <= *e) || (t < *s && t >= s - GAP_EDGE && first == key) || (t > *e && t <= e + GAP_EDGE && last == key)
        })
    };
    // The gap a moment falls in (its index and bounds), if it's a gap at all.
    let gap_of = |t: f64, key: &str| -> Option<(usize, f64, f64)> {
        if key.is_empty() || owned(t, key) || merged.iter().any(|&(s, e)| t >= s && t <= e) {
            return None;
        }
        let k = merged.iter().take_while(|&&(_, e)| e < t).count();
        let lo = k.checked_sub(1).map_or(f64::NEG_INFINITY, |i| merged[i].1);
        let hi = merged.get(k).map_or(f64::INFINITY, |m| m.0);
        (hi - lo >= MIN_GAP).then_some((k, lo, hi))
    };
    let mut out: Vec<SubtitleLine> = Vec::new();
    for line in heard {
        let mut run: Vec<SubtitleWord> = Vec::new();
        let mut run_gap: Option<usize> = None;
        let mut flush = |run: &mut Vec<SubtitleWord>| {
            if run.is_empty() {
                return;
            }
            let text = run.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ");
            let (start, end) = (run[0].start, run[run.len() - 1].end);
            out.push(SubtitleLine { start, end, text, words: std::mem::take(run), aligned: false, source_timing: None, added: false, machine: Some(source.clone()) });
        };
        for w in &line.words {
            let found = gap_of((w.start + w.end) * 0.5, &match_key(&w.text));
            let Some((k, lo, hi)) = found else {
                flush(&mut run);
                run_gap = None;
                continue;
            };
            if run_gap != Some(k) {
                flush(&mut run);
                run_gap = Some(k);
            }
            let (start, end) = (w.start.max(lo), w.end.min(hi));
            if end - start < MIN_WORD * 0.5 {
                continue;
            }
            let start = run.last().map_or(start, |p| start.max(p.end));
            run.push(SubtitleWord { start, end: end.max(start + MIN_WORD).min(hi), text: w.text.clone() });
        }
        flush(&mut run);
    }
    // Nor over each other: a line ends where the next begins.
    out.sort_by(|a, b| a.start.total_cmp(&b.start));
    for i in 1..out.len() {
        let next = out[i].start;
        let prev = &mut out[i - 1];
        if prev.end > next {
            prev.end = next;
            for w in &mut prev.words {
                w.end = w.end.min(next);
            }
            prev.words.retain(|w| w.end - w.start >= MIN_WORD * 0.5);
            if let Some(last) = prev.words.last() {
                prev.end = last.end;
            }
        }
    }
    out.retain(|l| !l.words.is_empty() && l.end > l.start && l.words.iter().all(|w| w.end > w.start));
    out
}

// ── apply ───────────────────────────────────────────────────────────────────

/// What applying a transcription did to a tile.
#[derive(Clone, Debug, PartialEq)]
pub struct Applied {
    pub lines: Vec<SubtitleLine>,
    pub source: Option<SubtitleSource>,
    /// The engine's lines became the tile's (it had none, or only machine-made ones).
    pub replaced: bool,
    /// Lines aligned by hand that stayed as they were.
    pub kept_aligned: usize,
    /// For known text: what [`align_text`] did.
    pub aligned: AlignReport,
    /// For known text: the lines heard in its gaps (D-296).
    pub gaps: GapReport,
}

/// What filling a tile's gaps did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GapReport {
    /// Lines heard in the gaps and added.
    pub added: usize,
    /// Gap lines from an earlier transcription, untouched since, that this one replaced.
    pub replaced: usize,
    /// Gap lines from earlier that a person has changed since: kept as they are.
    pub kept: usize,
}

/// The source of machine-made lines: `{kind: machine, engine, model, lang}`, the shape
/// QuipClipper writes.
pub fn machine_source(kind: EngineKind, heard: &Heard) -> SubtitleSource {
    let mut extra = Map::new();
    extra.insert("engine".into(), json!(kind.source_name()));
    if let Some(m) = &heard.model {
        extra.insert("model".into(), json!(m));
    }
    SubtitleSource { kind: MACHINE.to_owned(), lang: heard.lang.clone(), extra }
}

/// Whether a source says its lines were machine-made.
pub fn is_machine(source: Option<&SubtitleSource>) -> bool {
    source.is_some_and(|s| s.kind.eq_ignore_ascii_case(MACHINE) || s.kind.eq_ignore_ascii_case("whisper"))
}

/// How the UI names a source: "Machine-made · whisper.cpp · base.en · en", or the
/// source's kind and language.
pub fn source_label(source: &SubtitleSource) -> String {
    let get = |k: &str| source.extra.get(k).and_then(Value::as_str).map(str::to_owned);
    if is_machine(Some(source)) {
        let parts = [Some("Machine-made".to_owned()), get("engine"), get("model"), source.lang.clone()];
        return parts.into_iter().flatten().collect::<Vec<_>>().join(" · ");
    }
    let mut parts = vec![source.kind.to_uppercase()];
    parts.extend(source.lang.clone());
    if let Some(by) = source.extra.get("word_timing").and_then(|v| v.get("engine")).and_then(Value::as_str) {
        parts.push(format!("words by {by}"));
    }
    parts.join(" · ")
}

/// A transcription made the tile's. A tile with no subtitles, or only machine-made ones,
/// takes the engine's lines through [`crate::subtitles::merge`] (lines aligned by hand
/// stay); a tile with subtitles from elsewhere keeps its text, and its lines `targets`
/// (`None`: every line) that have no word timings take them from what was heard
/// ([`align_text`]; with `targets`, those lines are timed again even if they have
/// words), the source noting which engine timed the words. With `gaps`, what was heard
/// where none of its lines is spoken is added as machine-made lines ([`gap_lines`],
/// D-299): the gap lines an earlier transcription added are replaced, unless a person
/// has changed them since ([`touched`]), and those stay and count as known lines.
pub fn apply(current: &[SubtitleLine], source: Option<&SubtitleSource>, kind: EngineKind, heard: Heard, targets: Option<&[usize]>, gaps: bool) -> Applied {
    if current.is_empty() || is_machine(source) {
        let new_source = machine_source(kind, &heard);
        let (lines, kept) = crate::subtitles::merge(current, heard.lines);
        return Applied { lines, source: Some(new_source), replaced: true, kept_aligned: kept, aligned: AlignReport::default(), gaps: GapReport::default() };
    }
    let mut report_gaps = GapReport::default();
    let mut lines = current.to_vec();
    let mut targets = targets.map(<[usize]>::to_vec);
    if gaps {
        // This transcription's gap lines stand in for the last one's.
        let stale = |l: &SubtitleLine| l.machine.is_some() && !touched(l);
        report_gaps.replaced = lines.iter().filter(|l| stale(l)).count();
        report_gaps.kept = lines.iter().filter(|l| l.machine.is_some() && touched(l)).count();
        if let Some(ts) = &mut targets {
            let map: Vec<Option<usize>> = {
                let mut at = 0;
                lines.iter().map(|l| if stale(l) { None } else { at += 1; Some(at - 1) }).collect()
            };
            *ts = ts.iter().filter_map(|&i| map.get(i).copied().flatten()).collect();
        }
        lines.retain(|l| !stale(l));
    }
    let untimed: Vec<usize> = (0..lines.len()).filter(|&i| lines[i].words.is_empty() && !lines[i].aligned && lines[i].machine.is_none()).collect();
    let words: Vec<SubtitleWord> = heard.lines.iter().flat_map(|l| l.words.iter().cloned()).collect();
    let report = align_text(&mut lines, targets.as_deref().unwrap_or(&untimed), &words);
    let mut source = source.cloned();
    if report.lines > 0 {
        if let Some(s) = &mut source {
            let mut by = Map::new();
            by.insert("engine".into(), json!(kind.source_name()));
            if let Some(m) = &heard.model {
                by.insert("model".into(), json!(m));
            }
            s.extra.insert("word_timing".into(), Value::Object(by));
        }
    }
    if gaps {
        let mut rest = heard.lines.clone();
        for l in &mut rest {
            l.words.retain(|w| !report.used.contains(w));
        }
        let added = gap_lines(&lines, &rest, &machine_source(kind, &heard));
        report_gaps.added = added.len();
        lines.extend(added);
        lines.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.end.total_cmp(&b.end)));
    }
    let kept = current.iter().filter(|l| l.aligned).count();
    Applied { lines, source, replaced: false, kept_aligned: kept, aligned: report, gaps: report_gaps }
}

/// The lines an engine heard, moved from the WAV's frame into the file's.
pub fn shift(heard: &mut Heard, offset: f64) {
    if offset == 0.0 {
        return;
    }
    for l in &mut heard.lines {
        l.start += offset;
        l.end += offset;
        for w in &mut l.words {
            w.start += offset;
            w.end += offset;
        }
    }
}

/// Lines with no words get an even split, so every line has words to refine and match.
fn ensure_words(heard: &mut Heard) {
    for l in &mut heard.lines {
        if l.words.is_empty() {
            l.words = l.even_words();
        }
    }
}

/// Where a job's audio comes from: decoded already, or a file to decode on the job's
/// thread.
#[derive(Clone)]
pub enum Audio {
    Decoded(Arc<DecodedAudio>),
    File(PathBuf),
}

/// One tile's transcription, done on a worker thread: decode if needed, prepare, run the
/// engine, move its times into the file and onto the sounds. Returns what was heard (in
/// file seconds) and how far [`refine`] moved each word.
pub fn run(engine: &dyn Engine, audio: Audio, range: Option<(f64, f64)>, lang: Option<String>, progress: &Arc<Progress>) -> Result<(Heard, Vec<f64>), String> {
    let audio = match audio {
        Audio::Decoded(a) => a,
        Audio::File(path) => Arc::new(crate::decode::decode_file(&path).map_err(|e| format!("can't read its audio: {e}"))?),
    };
    if progress.is_cancelled() {
        return Err(CANCELLED.to_owned());
    }
    let prepared = prepare(&audio, range);
    if prepared.seconds < 0.05 {
        return Err("there's no audio to transcribe".to_owned());
    }
    let req = Request { wav: Arc::new(prepared.wav), seconds: prepared.seconds, lang };
    let mut heard = engine.transcribe(&req, progress)?;
    if progress.is_cancelled() {
        return Err(CANCELLED.to_owned());
    }
    heard.lines.retain(|l| l.end > l.start && !l.text.trim().is_empty());
    shift(&mut heard, prepared.offset);
    ensure_words(&mut heard);
    let (from, to) = (prepared.offset, prepared.offset + prepared.seconds);
    let onsets = align::onsets_in(&audio, (from - 0.2).max(0.0), to);
    let moved = refine(&mut heard.lines, &onsets, REFINE_REACH);
    progress.set_fraction(Some(1.0));
    Ok((heard, moved))
}

#[cfg(test)]
pub(crate) mod tests;
