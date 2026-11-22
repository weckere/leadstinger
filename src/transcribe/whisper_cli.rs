//! The whisper-cli engine: whisper.cpp's command-line program, installed separately and
//! found at run time the way ffmpeg is ([`crate::transcode::find_named`]), with a ggml
//! model file the user chose (D-278). Never bundled, never required, nothing downloaded.
//!
//! It runs `whisper-cli -m <model> -f <wav> -l <lang> -ojf -of <stem> -pp -t <n>` on the
//! prepared WAV in a scratch folder, and reads the full JSON (`-ojf`): each segment's
//! tokens carry their own times, and a word is the tokens from one that starts with a
//! space to the next.

use super::{Context, Engine, EngineKind, Heard, Request, CANCELLED};
use crate::subtitles::{SubtitleLine, SubtitleWord, MAX_TEXT, MAX_WORD, MAX_WORDS};
use crate::transcode::Progress;
use serde_json::Value;
use std::ffi::OsString;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The settings that point at the program and the model: `LEADSTINGER_WHISPER`,
/// `LEADSTINGER_WHISPER_MODEL`.
pub const ENV_PROGRAM: &str = "WHISPER";
pub const ENV_MODEL: &str = "WHISPER_MODEL";
/// The program's names: today's, then the one older packages installed.
pub const NAMES: &[&str] = &["whisper-cli", "whisper-cpp"];
/// The most threads a run takes: transcription shouldn't slow down playback.
const MAX_THREADS: usize = 4;

/// whisper-cli, found, with a model.
#[derive(Clone, Debug)]
pub struct WhisperCli {
    pub program: PathBuf,
    pub model: PathBuf,
}

/// whisper-cli: `$LEADSTINGER_WHISPER`, else on PATH, else a usual install folder.
pub fn find_program() -> Result<PathBuf, String> {
    find_program_in(crate::brand::env(ENV_PROGRAM), std::env::var_os("PATH"), &crate::transcode::extra_dirs())
}

pub fn find_program_in(explicit: Option<OsString>, path: Option<OsString>, extra: &[PathBuf]) -> Result<PathBuf, String> {
    crate::transcode::find_named(explicit, path, extra, ENV_PROGRAM, NAMES, "whisper-cli")
}

/// The model: `$LEADSTINGER_WHISPER_MODEL`, else the one chosen in Settings. The error
/// finishes "Couldn't transcribe: …".
pub fn find_model(explicit: Option<OsString>, setting: Option<&Path>) -> Result<PathBuf, String> {
    let (path, from) = match (explicit.filter(|p| !p.is_empty()), setting) {
        (Some(p), _) => (PathBuf::from(p), crate::brand::env_name(ENV_MODEL)),
        (None, Some(p)) => (p.to_path_buf(), "Settings".to_owned()),
        (None, None) => return Err(format!("whisper-cli needs a model file, and none is chosen. {}", model_hint())),
    };
    let mut head = [0u8; 4];
    match std::fs::File::open(&path).and_then(|mut f| f.read_exact(&mut head)) {
        Err(_) if !path.exists() => Err(format!("the whisper model in {from} ({}) isn't there. {}", path.display(), model_hint())),
        Err(e) => Err(format!("the whisper model in {from} ({}) can't be read: {e}", path.display())),
        // ggml model files start with "lmgg" (0x67676d6c, little-endian).
        Ok(()) if &head != b"lmgg" => Err(format!("{} isn't a whisper.cpp model (a ggml-….bin file). {}", path.display(), model_hint())),
        Ok(()) => Ok(path),
    }
}

/// How to install whisper-cli, as one sentence.
pub fn install_hint() -> String {
    let how = if cfg!(target_os = "macos") {
        "with Homebrew: brew install whisper-cpp (or Nix: nix profile install nixpkgs#whisper-cpp)"
    } else {
        "Nix: nix profile install nixpkgs#whisper-cpp, or build whisper.cpp from github.com/ggml-org/whisper.cpp"
    };
    format!("Install whisper.cpp ({how}), or set {} to the whisper-cli program.", crate::brand::env_name(ENV_PROGRAM))
}

/// Where to get a model, as one sentence. The app downloads nothing itself.
pub fn model_hint() -> String {
    format!(
        "Download a ggml model (ggml-base.en.bin is a good start; whisper.cpp's models/download-ggml-model.sh fetches one) and choose it in Settings ▸ Subtitles, or set {} to it.",
        crate::brand::env_name(ENV_MODEL)
    )
}

impl WhisperCli {
    /// whisper-cli and its model, or why it can't run.
    pub fn open(ctx: &Context) -> Result<WhisperCli, String> {
        let program = find_program().map_err(|why| format!("{why}. {}", install_hint()))?;
        let model = find_model(crate::brand::env(ENV_MODEL), ctx.whisper_model.as_deref())?;
        Ok(WhisperCli { program, model })
    }

    /// The model's name from its file: `ggml-base.en.bin` → `base.en`.
    pub fn model_name(&self) -> String {
        model_name(&self.model)
    }
}

pub fn model_name(path: &Path) -> String {
    let stem = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let stem = stem.strip_suffix(".bin").unwrap_or(&stem);
    stem.strip_prefix("ggml-").unwrap_or(stem).to_owned()
}

/// The language to ask for: the one given, else English for an English-only model
/// (`*.en`), else whisper's own guess.
pub fn language(asked: Option<&str>, model: &str) -> String {
    match asked.map(str::trim).filter(|l| !l.is_empty()) {
        Some(l) => l.to_ascii_lowercase(),
        None if model.ends_with(".en") || model.contains(".en-") => "en".to_owned(),
        None => "auto".to_owned(),
    }
}

fn threads() -> usize {
    std::thread::available_parallelism().map_or(2, |n| n.get() / 2).clamp(1, MAX_THREADS)
}

impl Engine for WhisperCli {
    fn kind(&self) -> EngineKind {
        EngineKind::WhisperCli
    }

    fn transcribe(&self, req: &Request, progress: &Arc<Progress>) -> Result<Heard, String> {
        let dir = std::env::temp_dir().join(format!("{}-transcribe-{}", crate::brand::SLUG, uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).map_err(|e| format!("couldn't make a scratch folder: {e}"))?;
        let result = self.run_in(&dir, req, progress);
        std::fs::remove_dir_all(&dir).ok();
        result
    }
}

impl WhisperCli {
    fn run_in(&self, dir: &Path, req: &Request, progress: &Arc<Progress>) -> Result<Heard, String> {
        let wav = dir.join("audio.wav");
        std::fs::write(&wav, req.wav.as_slice()).map_err(|e| format!("couldn't write the audio for whisper-cli: {e}"))?;
        let stem = dir.join("out");
        let model = self.model_name();
        let lang = language(req.lang.as_deref(), &model);
        let mut child = Command::new(&self.program)
            .arg("-m")
            .arg(&self.model)
            .arg("-f")
            .arg(&wav)
            .args(["-l", &lang, "-t", &threads().to_string(), "-ojf", "-pp", "-of"])
            .arg(&stem)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("couldn't start whisper-cli ({}): {e}", self.program.display()))?;
        let log = Arc::new(Mutex::new(Vec::<String>::new()));
        let reader = child.stderr.take().map(|err| {
            let log = log.clone();
            // The progress lines come from the same stream as everything else whisper-cli
            // says, which is kept (the last few lines) for an error message.
            let progress = progress.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(err).lines().map_while(Result::ok) {
                    if let Some(f) = parse_progress(&line) {
                        progress.set_fraction(Some(f));
                    }
                    let mut log = log.lock().unwrap_or_else(|e| e.into_inner());
                    log.push(line);
                    if log.len() > 40 {
                        log.remove(0);
                    }
                }
            })
        });
        let status = loop {
            if progress.is_cancelled() {
                child.kill().ok();
                child.wait().ok();
                return Err(CANCELLED.to_owned());
            }
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => std::thread::sleep(Duration::from_millis(50)),
                Err(e) => {
                    child.kill().ok();
                    return Err(format!("whisper-cli: {e}"));
                }
            }
        };
        if let Some(h) = reader {
            h.join().ok();
        }
        let log = log.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if !status.success() {
            return Err(failure_message(&log));
        }
        let text = std::fs::read_to_string(stem.with_extension("json")).map_err(|_| format!("whisper-cli wrote no transcript ({})", last_line(&log)))?;
        let json: Value = serde_json::from_str(&text).map_err(|e| format!("whisper-cli's transcript isn't JSON: {e}"))?;
        let mut heard = parse_json(&json);
        heard.model.get_or_insert(model);
        if heard.lang.as_deref().is_none_or(|l| l == "auto") && lang != "auto" {
            heard.lang = Some(lang);
        }
        Ok(heard)
    }
}

/// "progress = 45%" (whisper-cli's `-pp`) as 0.45.
pub fn parse_progress(line: &str) -> Option<f64> {
    let rest = line.split("progress =").nth(1)?;
    let n: f64 = rest.trim().trim_end_matches('%').trim().parse().ok()?;
    Some((n / 100.0).clamp(0.0, 1.0))
}

fn last_line(log: &[String]) -> String {
    log.iter().rev().map(|l| l.trim()).find(|l| !l.is_empty()).unwrap_or("it said nothing").to_owned()
}

/// A short reason from whisper-cli's log.
pub fn failure_message(log: &[String]) -> String {
    if log.iter().any(|l| l.contains("failed to load model") || l.contains("failed to initialize whisper context")) {
        return "whisper-cli couldn't load the model".to_owned();
    }
    if log.iter().any(|l| l.contains("unknown language")) {
        return "whisper-cli doesn't know that language".to_owned();
    }
    let line = log.iter().rev().map(|l| l.trim()).find(|l| l.starts_with("error") || l.contains(": error") || l.contains("failed")).map(str::to_owned).unwrap_or_else(|| last_line(log));
    format!("whisper-cli stopped with an error: {line}")
}

/// Text whisper writes for what isn't speech: `[BLANK_AUDIO]`, `(music)`, `♪`.
fn not_speech(text: &str) -> bool {
    let t = text.trim();
    t.is_empty()
        || (t.starts_with('[') && t.ends_with(']'))
        || (t.starts_with('(') && t.ends_with(')'))
        || t.chars().all(|c| matches!(c, '♪' | '♫' | '*' | '-' | ' ' | '.'))
}

/// Milliseconds in `v` (an `offsets` field) as seconds.
fn ms(v: &Value) -> Option<f64> {
    v.as_f64().filter(|x| x.is_finite() && *x >= 0.0).map(|x| x / 1000.0)
}

/// whisper-cli's full JSON (`-ojf`) as lines with words. Tokens in brackets (`[_BEG_]`,
/// `[_TT_150]`) are whisper's own markers, not words; a token starting with a space
/// starts a word, others (a word's later pieces, punctuation) join the word before. A
/// segment without tokens (plain `-oj`) has no words; the shared steps split it evenly.
pub fn parse_json(json: &Value) -> Heard {
    let mut lines = Vec::new();
    for seg in json.get("transcription").and_then(Value::as_array).into_iter().flatten() {
        let text: String = seg.get("text").and_then(Value::as_str).unwrap_or("").trim().chars().take(MAX_TEXT).collect();
        if not_speech(&text) {
            continue;
        }
        let off = seg.get("offsets");
        let (Some(start), Some(end)) = (off.and_then(|o| o.get("from")).and_then(ms), off.and_then(|o| o.get("to")).and_then(ms)) else { continue };
        let mut words: Vec<SubtitleWord> = Vec::new();
        for tok in seg.get("tokens").and_then(Value::as_array).into_iter().flatten() {
            let t = tok.get("text").and_then(Value::as_str).unwrap_or("");
            if t.is_empty() || (t.starts_with("[_") && t.ends_with(']')) {
                continue;
            }
            let o = tok.get("offsets");
            let (Some(a), Some(b)) = (o.and_then(|o| o.get("from")).and_then(ms), o.and_then(|o| o.get("to")).and_then(ms)) else { continue };
            let starts = t.starts_with(' ') || words.is_empty();
            if starts && !t.trim().is_empty() {
                if words.len() >= MAX_WORDS {
                    break;
                }
                words.push(SubtitleWord { start: a, end: b.max(a), text: t.trim().chars().take(MAX_WORD).collect() });
            } else if let Some(w) = words.last_mut() {
                if w.text.chars().count() + t.chars().count() <= MAX_WORD {
                    w.text.push_str(t.trim());
                }
                // Punctuation comes with the time of the pause after the word: it joins
                // the word's text, not its time.
                if t.chars().any(char::is_alphanumeric) {
                    w.end = w.end.max(b);
                }
            }
        }
        words.retain(|w| !not_speech(&w.text) || w.text.chars().any(char::is_alphanumeric));
        let end = end.max(words.last().map_or(end, |w| w.end));
        let start = start.min(words.first().map_or(start, |w| w.start));
        if end <= start {
            continue;
        }
        lines.push(SubtitleLine { start, end, text, words, aligned: false, source_timing: None, added: false, machine: None });
    }
    lines.sort_by(|a, b| a.start.total_cmp(&b.start));
    let lang = json.pointer("/result/language").or_else(|| json.pointer("/params/language")).and_then(Value::as_str).map(str::to_owned).filter(|l| l != "auto");
    let model = json.pointer("/params/model").and_then(Value::as_str).map(|m| model_name(Path::new(m)));
    Heard { lines, model, lang }
}
