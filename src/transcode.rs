//! Audio the decoder can't read, converted to FLAC with ffmpeg at import (D-068, D-072).
//!
//! symphonia has no AC-3, E-AC-3, DTS or TrueHD decoder, and video containers (mkv, mov,
//! mp4, vob) carry their audio beside pictures nobody needs in the library. Those files
//! are run through ffmpeg once, when they're imported, into a FLAC file in the library's
//! Media folder, keeping every channel; playback then never needs ffmpeg. ffmpeg is an
//! optional, separately installed program, found at run time ([`find_ffmpeg`]) and never
//! bundled (the Nix package puts one on its PATH).

use std::ffi::{OsStr, OsString};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Extensions that go through ffmpeg whenever it's installed: formats symphonia can't
/// decode, and video files, whose audio is all the library keeps. Without ffmpeg they're
/// still imported directly if symphonia can read them (an audio-only .mp4, say).
pub const FFMPEG_EXTENSIONS: &[&str] =
    &["ac3", "eac3", "ec3", "dts", "dtshd", "thd", "mka", "mkv", "webm", "mov", "mp4", "m4v", "vob", "avi", "m2ts", "mts"];

/// How an imported file gets into the library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    /// Copied as it is.
    Copy,
    /// Converted to FLAC by this ffmpeg.
    Convert(PathBuf),
}

/// Whether `path` is one of [`FFMPEG_EXTENSIONS`].
pub fn prefers_ffmpeg(path: &Path) -> bool {
    path.extension().and_then(OsStr::to_str).is_some_and(|e| FFMPEG_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

/// How to import `path`: through ffmpeg if it's a [`FFMPEG_EXTENSIONS`] file or the
/// decoder can't read it, otherwise a plain copy. An error names what to install when
/// the file needs ffmpeg and there is none.
pub fn plan(path: &Path) -> Result<Plan, String> {
    decide(prefers_ffmpeg(path), find_ffmpeg(), || crate::decode::can_read(path), &display_name(path))
}

/// [`plan`] with its inputs given: the file's kind, the ffmpeg search, and a check that
/// the decoder can read the file (only run when the answer matters).
pub fn decide(prefers: bool, ffmpeg: Result<PathBuf, String>, readable: impl FnOnce() -> bool, name: &str) -> Result<Plan, String> {
    match ffmpeg {
        Ok(ff) if prefers => Ok(Plan::Convert(ff)),
        Ok(ff) => Ok(if readable() { Plan::Copy } else { Plan::Convert(ff) }),
        Err(why) => {
            if readable() {
                Ok(Plan::Copy)
            } else if prefers {
                Err(format!("“{name}” needs ffmpeg to import, and {why}. {}", install_hint()))
            } else {
                Err(format!("“{name}” isn't a format {} reads, and {why}; with ffmpeg it can try. {}", crate::brand::APP_NAME, install_hint()))
            }
        }
    }
}

fn display_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string())
}

/// How to install ffmpeg on this system, as one sentence.
pub fn install_hint() -> String {
    let how = if cfg!(target_os = "macos") {
        "with Homebrew: brew install ffmpeg (or Nix: nix profile install nixpkgs#ffmpeg-headless)"
    } else {
        "Debian/Ubuntu: sudo apt install ffmpeg; Fedora: sudo dnf install ffmpeg-free; Nix: nix profile install nixpkgs#ffmpeg-headless"
    };
    format!("Install it ({how}), or set {} to the ffmpeg program, then import the file again.", crate::brand::env_name("FFMPEG"))
}

/// Where else to look when ffmpeg isn't on PATH: an app started from the macOS Finder or
/// Dock gets only the system PATH, without Homebrew, MacPorts or Nix (D-073).
fn extra_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = ["/opt/homebrew/bin", "/usr/local/bin", "/opt/local/bin", "/run/current-system/sw/bin", "/nix/var/nix/profiles/default/bin"]
        .iter()
        .map(PathBuf::from)
        .collect();
    if let Some(home) = std::env::var_os("HOME") {
        dirs.push(Path::new(&home).join(".nix-profile/bin"));
    }
    dirs
}

/// ffmpeg: `$LEADSTINGER_FFMPEG` if set, else the first on PATH, else a usual install
/// folder. The error says what was looked for, to finish a sentence ("and …").
pub fn find_ffmpeg() -> Result<PathBuf, String> {
    find_in(crate::brand::env("FFMPEG"), std::env::var_os("PATH"), &extra_dirs())
}

pub fn find_in(explicit: Option<OsString>, path: Option<OsString>, extra: &[PathBuf]) -> Result<PathBuf, String> {
    if let Some(p) = explicit.filter(|p| !p.is_empty()) {
        let p = PathBuf::from(p);
        return if is_program(&p) {
            Ok(p)
        } else {
            Err(format!("{} is set to {}, which isn't a program", crate::brand::env_name("FFMPEG"), p.display()))
        };
    }
    let exe = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
    path.iter()
        .flat_map(std::env::split_paths)
        .chain(extra.iter().cloned())
        .map(|d| d.join(exe))
        .find(|p| is_program(p))
        .ok_or_else(|| "ffmpeg isn't installed".to_owned())
}

fn is_program(p: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        p.is_file()
    }
}

/// A conversion's progress, shared with the UI, and its cancel switch.
#[derive(Debug)]
pub struct Progress {
    /// Thousandths done, or `u32::MAX` while the length is unknown.
    done: AtomicU32,
    cancel: AtomicBool,
    /// A download (src/quip.rs) is under way; a conversion may follow it.
    downloading: AtomicBool,
}

impl Default for Progress {
    fn default() -> Progress {
        Progress { done: AtomicU32::new(u32::MAX), cancel: AtomicBool::new(false), downloading: AtomicBool::new(false) }
    }
}

impl Progress {
    /// Record how far along (0..=1) something is, or `None` while its size is unknown.
    pub fn set_fraction(&self, f: Option<f64>) {
        let v = f.map_or(u32::MAX, |f| (f.clamp(0.0, 1.0) * 1000.0) as u32);
        self.done.store(v, Ordering::Relaxed);
    }

    pub fn set_downloading(&self, on: bool) {
        self.downloading.store(on, Ordering::Relaxed);
    }

    pub fn is_downloading(&self) -> bool {
        self.downloading.load(Ordering::Relaxed)
    }

    /// How far along (0..=1), once ffmpeg has said how long the file is.
    pub fn fraction(&self) -> Option<f32> {
        match self.done.load(Ordering::Relaxed) {
            u32::MAX => None,
            d => Some(d as f32 / 1000.0),
        }
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

/// What a cancelled import reports.
pub const CANCELLED: &str = "Import cancelled";

/// Convert the audio in `src` to FLAC at `dest`, keeping every channel. ffmpeg picks the
/// file's main audio stream (the one with the most channels) and a sample format that
/// keeps its precision (24-bit for AC-3 and DTS). The FLAC is written beside `dest` and
/// moved into place only when ffmpeg succeeds, so a cancelled or failed conversion
/// leaves nothing behind.
pub fn convert(ffmpeg: &Path, src: &Path, dest: &Path, progress: &Arc<Progress>) -> Result<(), String> {
    let part = dest.with_extension("flac.part");
    let mut child = Command::new(ffmpeg)
        .args(["-nostdin", "-hide_banner", "-y", "-progress", "pipe:1", "-nostats", "-i"])
        .arg(src)
        .args(["-vn", "-sn", "-dn", "-map_metadata", "-1", "-c:a", "flac", "-f", "flac"])
        .arg(&part)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("couldn't start ffmpeg ({}): {e}", ffmpeg.display()))?;

    // The length comes from ffmpeg's description of the input (stderr); how far it got
    // from its progress reports (stdout).
    let length_us = Arc::new(AtomicU64::new(0));
    let log = Arc::new(Mutex::new(Vec::<String>::new()));
    let stderr = child.stderr.take().map(|err| {
        let (length_us, log) = (length_us.clone(), log.clone());
        std::thread::spawn(move || {
            for line in BufReader::new(err).lines().map_while(Result::ok) {
                if let Some(us) = line.trim_start().strip_prefix("Duration: ").and_then(|r| parse_clock(r.split(',').next()?)) {
                    length_us.fetch_max(us, Ordering::Relaxed);
                }
                let mut log = log.lock().unwrap_or_else(|e| e.into_inner());
                log.push(line);
                if log.len() > 40 {
                    log.remove(0);
                }
            }
        })
    });
    let stdout = child.stdout.take().map(|out| {
        let (length_us, progress) = (length_us.clone(), progress.clone());
        std::thread::spawn(move || {
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                let Some(at) = line.strip_prefix("out_time=").and_then(parse_clock) else { continue };
                let total = length_us.load(Ordering::Relaxed);
                if total > 0 {
                    progress.done.store(((at as f64 / total as f64).clamp(0.0, 1.0) * 1000.0) as u32, Ordering::Relaxed);
                }
            }
        })
    });

    let status = loop {
        if progress.is_cancelled() {
            child.kill().ok();
            child.wait().ok();
            std::fs::remove_file(&part).ok();
            return Err(CANCELLED.to_owned());
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => {
                child.kill().ok();
                std::fs::remove_file(&part).ok();
                return Err(format!("ffmpeg: {e}"));
            }
        }
    };
    for h in [stdout, stderr].into_iter().flatten() {
        h.join().ok();
    }
    if !status.success() {
        std::fs::remove_file(&part).ok();
        let log = log.lock().unwrap_or_else(|e| e.into_inner());
        return Err(failure_message(&log));
    }
    progress.done.store(1000, Ordering::Relaxed);
    std::fs::rename(&part, dest).map_err(|e| format!("saving the converted audio: {e}"))
}

/// A short reason from ffmpeg's log.
fn failure_message(log: &[String]) -> String {
    if log.iter().any(|l| l.contains("does not contain any stream") || l.contains("matches no streams")) {
        return "it has no audio track".to_owned();
    }
    if log.iter().any(|l| l.contains("Invalid data found when processing input")) {
        return "ffmpeg doesn't recognise it as audio or video".to_owned();
    }
    let last = log
        .iter()
        .rev()
        .map(|l| l.trim())
        .find(|l| !l.is_empty() && !l.starts_with("Conversion failed") && !l.starts_with("Error opening output") && !l.starts_with("size="))
        .unwrap_or("it stopped with an error");
    // Drop ffmpeg's "[component @ 0x…] " prefix.
    let last = match last.split_once("] ") {
        Some((head, rest)) if head.starts_with('[') => rest,
        _ => last,
    };
    format!("ffmpeg couldn't convert it: {last}")
}

/// `HH:MM:SS.fraction` in microseconds (`N/A` is `None`).
fn parse_clock(s: &str) -> Option<u64> {
    let mut parts = s.trim().splitn(3, ':');
    let h: u64 = parts.next()?.parse().ok()?;
    let m: u64 = parts.next()?.parse().ok()?;
    let sec: f64 = parts.next()?.parse().ok()?;
    sec.is_finite().then(|| (h * 3600 + m * 60) * 1_000_000 + (sec * 1_000_000.0) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ff() -> Result<PathBuf, String> {
        Ok(PathBuf::from("/bin/ffmpeg"))
    }

    fn none() -> Result<PathBuf, String> {
        Err("ffmpeg isn't installed".to_owned())
    }

    #[test]
    fn decides_when_to_convert() {
        let never = || -> bool { panic!("the decoder check isn't needed here") };
        // AC-3 and video files go through ffmpeg whenever there is one.
        assert_eq!(decide(true, ff(), never, "a.ac3"), Ok(Plan::Convert("/bin/ffmpeg".into())));
        // Anything else is copied if the decoder reads it, else converted.
        assert_eq!(decide(false, ff(), || true, "a.wav"), Ok(Plan::Copy));
        assert_eq!(decide(false, ff(), || false, "a.wma"), Ok(Plan::Convert("/bin/ffmpeg".into())));
        // Without ffmpeg, what the decoder reads is still copied (an audio-only .mp4)…
        assert_eq!(decide(true, none(), || true, "a.mp4"), Ok(Plan::Copy));
        assert_eq!(decide(false, none(), || true, "a.wav"), Ok(Plan::Copy));
        // …and the rest is refused, saying what to install.
        let e = decide(true, none(), || false, "Movie.mkv").unwrap_err();
        assert!(e.contains("“Movie.mkv” needs ffmpeg") && e.contains("ffmpeg isn't installed"), "{e}");
        assert!(e.contains(&crate::brand::env_name("FFMPEG")), "{e}");
        let e = decide(false, none(), || false, "notes.txt").unwrap_err();
        assert!(e.contains("isn't a format") && e.contains("Install it"), "{e}");
    }

    #[test]
    fn extensions_are_matched_without_case() {
        assert!(prefers_ffmpeg(Path::new("/x/Main Feature.MKV")));
        assert!(prefers_ffmpeg(Path::new("dts.dts")));
        assert!(!prefers_ffmpeg(Path::new("song.flac")));
        assert!(!prefers_ffmpeg(Path::new("noext")));
    }

    #[test]
    fn the_missing_ffmpeg_message_says_how_to_install_it() {
        let hint = install_hint();
        if cfg!(target_os = "macos") {
            assert!(hint.contains("brew install ffmpeg"), "{hint}");
        } else {
            assert!(hint.contains("apt install ffmpeg") && hint.contains("dnf install"), "{hint}");
        }
        assert!(hint.contains("nixpkgs#ffmpeg"), "{hint}");
    }

    #[cfg(unix)]
    #[test]
    fn finds_ffmpeg_by_setting_then_path_then_usual_places() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("sb-ffmpeg-{}", uuid::Uuid::new_v4()));
        let (bin, other, usual) = (root.join("bin"), root.join("other"), root.join("usual"));
        for d in [&bin, &other, &usual] {
            std::fs::create_dir_all(d).unwrap();
        }
        let make = |dir: &Path, mode: u32| {
            let p = dir.join("ffmpeg");
            std::fs::write(&p, "#!/bin/sh\n").unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode)).unwrap();
            p
        };
        let on_path = make(&bin, 0o755);
        let in_usual = make(&usual, 0o755);
        let not_a_program = make(&other, 0o644);
        let path = std::env::join_paths([&other, &bin]).unwrap();

        // PATH, skipping a file that isn't executable.
        assert_eq!(find_in(None, Some(path.clone()), std::slice::from_ref(&usual)), Ok(on_path.clone()));
        // The setting wins, and says so when it's wrong.
        assert_eq!(find_in(Some(in_usual.clone().into()), Some(path.clone()), &[]), Ok(in_usual.clone()));
        let e = find_in(Some(not_a_program.into()), Some(path), &[]).unwrap_err();
        assert!(e.contains("isn't a program"), "{e}");
        // The usual places when PATH has none.
        assert_eq!(find_in(None, Some(other.clone().into()), std::slice::from_ref(&usual)), Ok(in_usual));
        assert_eq!(find_in(None, None, &[]), Err("ffmpeg isn't installed".to_owned()));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn reads_ffmpeg_clocks_and_errors() {
        assert_eq!(parse_clock("00:01:02.500000"), Some(62_500_000));
        assert_eq!(parse_clock(" 01:00:00.00"), Some(3_600_000_000));
        assert_eq!(parse_clock("N/A"), None);
        let log = |lines: &[&str]| lines.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(failure_message(&log(&["Output file does not contain any stream"])), "it has no audio track");
        assert_eq!(failure_message(&log(&["[in#0 @ 0x1] Error opening input: Invalid data found when processing input"])), "ffmpeg doesn't recognise it as audio or video");
        assert_eq!(failure_message(&log(&["[flac @ 0x2] Unsupported channel layout", "Conversion failed!"])), "ffmpeg couldn't convert it: Unsupported channel layout");
    }
}
