//! The parts of the Apple Speech engine that need no framework, so they build and are
//! tested on every platform (D-286 to D-289): cutting long audio into pieces the
//! recogniser takes, making lines out of the one stream of word segments it gives back,
//! choosing the locale for a language, and saying why the engine can't run.
//! The framework calls are in [`super::apple`] (macOS and iOS only).

use crate::subtitles::{SubtitleLine, SubtitleWord, MAX_TEXT, MAX_WORD, MAX_WORDS};

/// The longest piece of audio one request gets, in seconds (D-286): the framework's
/// headers warn that recognition tasks stop after a minute.
pub const MAX_PIECE: f64 = 50.0;
/// Where in a long piece to look for the quietest place to cut: its last this-many
/// seconds.
pub const CUT_SEARCH: f64 = 10.0;
/// A pause this long or longer always starts a new line.
pub const PAUSE: f64 = 0.7;
/// A line this long (seconds or words) breaks at its next comma or short pause, and at
/// [`HARD_SECONDS`] / [`HARD_WORDS`] whatever comes.
pub const SOFT_SECONDS: f64 = 5.0;
pub const SOFT_WORDS: usize = 12;
pub const HARD_SECONDS: f64 = 7.0;
pub const HARD_WORDS: usize = 16;
/// A gap between words that counts as a short pause for a soft break.
pub const SHORT_PAUSE: f64 = 0.25;

/// One word the recogniser heard: `range` is where it is in the transcription's
/// formatted string, in UTF-16 units; times are seconds into the piece it was given.
#[derive(Clone, Debug, PartialEq)]
pub struct Segment {
    pub text: String,
    pub range: (usize, usize),
    pub start: f64,
    pub duration: f64,
}

/// Where to cut `samples` (mono, `rate` Hz) into pieces of at most [`MAX_PIECE`]
/// seconds: each long piece ends at the quietest tenth of a second in its last
/// [`CUT_SEARCH`] seconds, so a cut falls between words rather than through one.
/// Returns sample ranges that cover everything, in order.
pub fn plan_pieces(samples: &[f32], rate: u32) -> Vec<(usize, usize)> {
    let rate = rate.max(1) as usize;
    let max = (MAX_PIECE * rate as f64) as usize;
    let search = (CUT_SEARCH * rate as f64) as usize;
    let window = (rate / 10).max(1);
    let mut out = Vec::new();
    let mut from = 0;
    while samples.len() - from > max {
        let lo = from + max - search;
        let hi = from + max - window;
        let mut best = (f64::INFINITY, from + max);
        let mut at = lo;
        while at <= hi {
            let energy: f64 = samples[at..at + window].iter().map(|s| f64::from(*s) * f64::from(*s)).sum();
            if energy < best.0 {
                best = (energy, at + window / 2);
            }
            at += window / 2;
        }
        out.push((from, best.1));
        from = best.1;
    }
    out.push((from, samples.len()));
    out
}

/// Whether a word ends a sentence.
fn ends_sentence(word: &str) -> bool {
    let w = word.trim_end_matches(['"', '\'', ')', '”', '’', '»', ']']);
    // Also the full-width stops of Chinese and Japanese (escaped: the UI fonts lack them).
    w.ends_with(['.', '?', '!', '…', '\u{3002}', '\u{FF1F}', '\u{FF01}'])
}

fn ends_clause(word: &str) -> bool {
    word.trim_end().ends_with([',', ';', ':', '\u{FF0C}', '\u{3001}', '—'])
}

/// Make subtitle lines out of the recogniser's segments for one piece: `formatted` is
/// its whole transcription (UTF-16, the units `range` counts in) and `offset` is where
/// the piece starts, in seconds into the request's audio. A line breaks after a word
/// that ends a sentence, before a pause of [`PAUSE`] or more, and when it gets long
/// (see [`SOFT_SECONDS`]). A line's text is the formatted string from its first word to
/// its last, so the spacing and punctuation are the recogniser's own.
pub fn lines(formatted: &[u16], segments: &[Segment], offset: f64) -> Vec<SubtitleLine> {
    let mut out = Vec::new();
    let mut cur: Vec<&Segment> = Vec::new();
    let flush = |cur: &mut Vec<&Segment>, out: &mut Vec<SubtitleLine>| {
        if let (Some(first), Some(last)) = (cur.first(), cur.last()) {
            let text = slice(formatted, first.range.0, last.range.1).unwrap_or_else(|| cur.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(" "));
            let text = clip(text.trim(), MAX_TEXT);
            let words: Vec<SubtitleWord> = cur
                .iter()
                .take(MAX_WORDS)
                .map(|s| SubtitleWord { start: offset + s.start, end: offset + s.start + s.duration.max(0.0), text: clip(s.text.trim(), MAX_WORD) })
                .collect();
            if !text.is_empty() {
                out.push(SubtitleLine {
                    start: offset + first.start,
                    end: offset + last.start + last.duration.max(0.0),
                    text,
                    words,
                    aligned: false,
                    source_timing: None,
                    added: false, machine: None,
                });
            }
        }
        cur.clear();
    };
    for seg in segments.iter().filter(|s| !s.text.trim().is_empty()) {
        if let (Some(first), Some(prev)) = (cur.first(), cur.last()) {
            let gap = seg.start - (prev.start + prev.duration);
            let length = seg.start + seg.duration - first.start;
            let soft = cur.len() >= SOFT_WORDS || length > SOFT_SECONDS;
            let hard = cur.len() >= HARD_WORDS || length > HARD_SECONDS;
            let breaks = ends_sentence(&prev.text) || gap >= PAUSE || hard || (soft && (ends_clause(&prev.text) || gap >= SHORT_PAUSE));
            if breaks {
                flush(&mut cur, &mut out);
            }
        }
        cur.push(seg);
    }
    flush(&mut cur, &mut out);
    out
}

/// `formatted[from..to]` as a string, if the range is inside it.
fn slice(formatted: &[u16], from: usize, to: usize) -> Option<String> {
    (from <= to && to <= formatted.len()).then(|| String::from_utf16_lossy(&formatted[from..to]))
}

/// Cut `s` to at most `max` characters, with "…" when it was cut.
fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_owned()
    } else {
        let mut t: String = s.chars().take(max.saturating_sub(1)).collect();
        t.push('…');
        t
    }
}

/// `en_US`, `en-us` → `en-US` (language lower case, region upper case).
pub fn normalize(id: &str) -> String {
    let mut parts = id.split(['-', '_']).filter(|p| !p.is_empty());
    let lang = parts.next().unwrap_or("").to_ascii_lowercase();
    let rest: Vec<String> = parts.map(|p| if p.len() == 2 { p.to_ascii_uppercase() } else { p.to_owned() }).collect();
    std::iter::once(lang).chain(rest).collect::<Vec<_>>().join("-")
}

fn language(id: &str) -> String {
    normalize(id).split('-').next().unwrap_or("").to_owned()
}

fn region(id: &str) -> Option<String> {
    normalize(id).split('-').skip(1).find(|p| p.len() == 2 && p.chars().all(|c| c.is_ascii_uppercase())).map(str::to_owned)
}

/// The recogniser locale for a language: `want` is the setting (`en`, `pt-br`, or
/// `None` for the user's own language), `current` the user's locale (`en_GB`), and
/// `supported` the recogniser's locales. An exact match wins; else the language in the
/// user's region; else the language in the region named like it (`de-DE`); else the
/// first in order. `None` when the language isn't supported.
pub fn pick_locale(want: Option<&str>, current: &str, supported: &[String]) -> Option<String> {
    let want = want.map(str::trim).filter(|w| !w.is_empty() && *w != "auto").map(normalize).unwrap_or_else(|| normalize(current));
    let mut all: Vec<String> = supported.iter().map(|s| normalize(s)).collect();
    all.sort();
    if let Some(exact) = all.iter().find(|s| s.eq_ignore_ascii_case(&want)) {
        return Some(exact.clone());
    }
    let lang = language(&want);
    let same: Vec<&String> = all.iter().filter(|s| language(s) == lang).collect();
    let in_region = |r: Option<String>| r.and_then(|r| same.iter().find(|s| region(s).as_deref() == Some(r.as_str())).map(|s| (*s).clone()));
    in_region(region(&want))
        .or_else(|| in_region(region(current)))
        .or_else(|| in_region(Some(lang.to_ascii_uppercase())))
        .or_else(|| same.first().map(|s| (*s).clone()))
}

/// Speech Recognition permission, as the framework reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Permission {
    NotDetermined,
    Denied,
    Restricted,
    Authorized,
}

impl Permission {
    pub fn name(self) -> &'static str {
        match self {
            Permission::NotDetermined => "not-asked",
            Permission::Denied => "denied",
            Permission::Restricted => "restricted",
            Permission::Authorized => "allowed",
        }
    }
}

/// Where the engine runs: the Mac app or the iPad app (for the names of their Settings).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Device {
    Mac,
    IPad,
}

impl Device {
    pub fn here() -> Device {
        if cfg!(target_os = "ios") {
            Device::IPad
        } else {
            Device::Mac
        }
    }

    fn name(self) -> &'static str {
        match self {
            Device::Mac => "this Mac",
            Device::IPad => "this iPad",
        }
    }

    fn settings(self) -> &'static str {
        match self {
            Device::Mac => "System Settings",
            Device::IPad => "Settings",
        }
    }
}

/// Why Apple Speech can't run, in the order they're checked.
#[derive(Clone, Debug, PartialEq)]
pub enum Blocker {
    /// Not running as an app bundle's own program (D-289): `cli` for leadstinger-cli.
    NotInApp { cli: bool },
    /// Permission isn't granted (D-288).
    Permission(Permission),
    /// The recogniser has no locale for the language asked for.
    NoLanguage(String),
    /// The language has no on-device model here (D-287); the name is the language's.
    NotOnDevice(String),
    /// The recogniser says it can't run right now.
    Unavailable(String),
}

impl Blocker {
    /// The sentence `open` gives: why, and what to do (it follows "Can't transcribe: ").
    pub fn message(&self, device: Device) -> String {
        let app = crate::brand::APP_NAME;
        match self {
            Blocker::NotInApp { cli: true } => format!(
                "Apple Speech works only inside the {app} app, not from {} on its own (macOS gives speech permission to apps). Run the command without --offline while the app is open, and the app transcribes",
                crate::brand::CLI_NAME
            ),
            Blocker::NotInApp { cli: false } => format!(
                "Apple Speech works only in {app}.app: this copy isn't running from an app bundle (a cargo run or a plain build), and only an app can be given speech permission"
            ),
            Blocker::Permission(Permission::NotDetermined) => {
                "Apple Speech needs your permission first: Settings ▸ Subtitles ▸ Allow Speech Recognition…".to_owned()
            }
            Blocker::Permission(Permission::Denied) => format!(
                "Speech Recognition is turned off for {app}. Turn it on in {} ▸ Privacy & Security ▸ Speech Recognition",
                device.settings()
            ),
            Blocker::Permission(Permission::Restricted) => format!(
                "Speech Recognition is restricted on {} (by Screen Time or a device profile), so {app} can't use it",
                device.name()
            ),
            Blocker::Permission(Permission::Authorized) => "Apple Speech is allowed".to_owned(),
            Blocker::NoLanguage(lang) => format!("Apple Speech doesn't recognise the language “{lang}”"),
            Blocker::NotOnDevice(name) => format!(
                "Apple Speech has no on-device model for {name} on {}, and {app} never sends audio to Apple's servers. Turning on Dictation for {name} in {} ▸ Keyboard may install one",
                device.name(),
                device.settings()
            ),
            Blocker::Unavailable(name) => format!("Apple Speech can't recognise {name} right now (are Siri and Dictation turned off?); try again later"),
        }
    }

    /// Whether Settings' "Allow Speech Recognition…" button can fix it.
    pub fn can_ask(&self) -> bool {
        matches!(self, Blocker::Permission(Permission::NotDetermined))
    }
}

/// Whether an `NSError` from the recogniser means "heard no speech", which is a result
/// (no lines), not a failure: the assistant domain's 1110 ("No speech detected").
pub fn is_no_speech(domain: &str, code: isize) -> bool {
    domain == "kAFAssistantErrorDomain" && code == 1110
}

/// The samples of a mono 16-bit PCM WAV (what [`super::prepare`] writes), as floats.
pub fn wav_samples(wav: &[u8]) -> Result<(u32, Vec<f32>), String> {
    let bad = || "the prepared audio isn't a WAV file".to_owned();
    if wav.len() < 12 || &wav[0..4] != b"RIFF" || &wav[8..12] != b"WAVE" {
        return Err(bad());
    }
    let mut at = 12;
    let mut rate = None;
    while at + 8 <= wav.len() {
        let id = &wav[at..at + 4];
        let len = u32::from_le_bytes([wav[at + 4], wav[at + 5], wav[at + 6], wav[at + 7]]) as usize;
        let body = &wav[at + 8..(at + 8 + len).min(wav.len())];
        if id == b"fmt " && body.len() >= 16 {
            let channels = u16::from_le_bytes([body[2], body[3]]);
            let bits = u16::from_le_bytes([body[14], body[15]]);
            if channels != 1 || bits != 16 {
                return Err(bad());
            }
            rate = Some(u32::from_le_bytes([body[4], body[5], body[6], body[7]]));
        } else if id == b"data" {
            let rate = rate.ok_or_else(bad)?;
            return Ok((rate, body.as_chunks::<2>().0.iter().map(|b| f32::from(i16::from_le_bytes(*b)) / 32768.0).collect()));
        }
        at += 8 + len + (len & 1);
    }
    Err(bad())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(formatted: &mut String, text: &str, start: f64, duration: f64) -> Segment {
        if !formatted.is_empty() {
            formatted.push(' ');
        }
        let from = formatted.encode_utf16().count();
        formatted.push_str(text);
        Segment { text: text.to_owned(), range: (from, formatted.encode_utf16().count()), start, duration }
    }

    fn texts(lines: &[SubtitleLine]) -> Vec<&str> {
        lines.iter().map(|l| l.text.as_str()).collect()
    }

    #[test]
    fn lines_break_at_sentence_ends_and_pauses() {
        let mut f = String::new();
        let segs = vec![
            seg(&mut f, "Welcome", 0.2, 0.4),
            seg(&mut f, "to", 0.6, 0.1),
            seg(&mut f, "the", 0.7, 0.1),
            seg(&mut f, "concert.", 0.8, 0.5),
            seg(&mut f, "Please", 1.5, 0.3),
            seg(&mut f, "sit", 1.8, 0.2),
            // A long pause before "Thanks".
            seg(&mut f, "Thanks", 3.0, 0.4),
        ];
        let utf16: Vec<u16> = f.encode_utf16().collect();
        let l = lines(&utf16, &segs, 10.0);
        assert_eq!(texts(&l), ["Welcome to the concert.", "Please sit", "Thanks"]);
        assert!((l[0].start - 10.2).abs() < 1e-9 && (l[0].end - 11.3).abs() < 1e-9);
        assert_eq!(l[0].words.len(), 4);
        assert!((l[0].words[3].start - 10.8).abs() < 1e-9 && (l[0].words[3].end - 11.3).abs() < 1e-9);
        assert!(l.iter().all(|l| !l.aligned));
    }

    #[test]
    fn long_lines_break_at_a_comma_then_hard() {
        let mut f = String::new();
        let mut segs = Vec::new();
        // 13 words of 0.3 s without a pause: the 12-word soft limit waits for a comma.
        for i in 0..13 {
            let w = if i == 12 { "twelve," } else { "word" };
            segs.push(seg(&mut f, w, i as f64 * 0.3, 0.3));
        }
        segs.push(seg(&mut f, "after", 3.9, 0.3));
        let utf16: Vec<u16> = f.encode_utf16().collect();
        let l = lines(&utf16, &segs, 0.0);
        assert_eq!(l.len(), 2);
        assert_eq!(l[0].words.len(), 13);
        assert_eq!(l[1].text, "after");

        // No comma, no pause: the hard limit cuts at 16 words.
        let mut f = String::new();
        let segs: Vec<Segment> = (0..20).map(|i| seg(&mut f, "go", i as f64 * 0.2, 0.2)).collect();
        let utf16: Vec<u16> = f.encode_utf16().collect();
        let l = lines(&utf16, &segs, 0.0);
        assert_eq!(l.iter().map(|l| l.words.len()).collect::<Vec<_>>(), [16, 4]);
        // And at 7 s for slow speech.
        let mut f = String::new();
        let segs: Vec<Segment> = (0..10).map(|i| seg(&mut f, "slow", i as f64 * 0.9, 0.6)).collect();
        let utf16: Vec<u16> = f.encode_utf16().collect();
        let l = lines(&utf16, &segs, 0.0);
        assert!(l.iter().all(|l| l.end - l.start <= HARD_SECONDS + 1e-9), "{l:?}");
    }

    #[test]
    fn text_comes_from_the_formatted_string() {
        // A language written without spaces keeps its text as the recogniser wrote it,
        // and a range outside the string falls back to the words.
        let f = "今日は晴れ\u{3002}";
        let utf16: Vec<u16> = f.encode_utf16().collect();
        let segs = vec![
            Segment { text: "今日は".into(), range: (0, 3), start: 0.0, duration: 0.5 },
            Segment { text: "晴れ\u{3002}".into(), range: (3, 6), start: 0.5, duration: 0.5 },
        ];
        assert_eq!(texts(&lines(&utf16, &segs, 0.0)), ["今日は晴れ\u{3002}"]);
        let segs = vec![Segment { text: "hello".into(), range: (40, 45), start: 0.0, duration: 0.5 }];
        assert_eq!(texts(&lines(&utf16, &segs, 0.0)), ["hello"]);
        assert!(lines(&utf16, &[], 0.0).is_empty());
    }

    #[test]
    fn pieces_cut_in_the_quiet() {
        let rate = 1000;
        assert_eq!(plan_pieces(&vec![0.5; 30 * rate], rate as u32), [(0, 30 * rate)]);
        // 120 s of loud sound with a silent gap at 45 s and another at 92 s.
        let mut s = vec![0.5f32; 120 * rate];
        for x in &mut s[45 * rate..45 * rate + 300] {
            *x = 0.0;
        }
        for x in &mut s[92 * rate..92 * rate + 300] {
            *x = 0.0;
        }
        let p = plan_pieces(&s, rate as u32);
        assert_eq!(p.len(), 3, "{p:?}");
        assert!(p[0].1 >= 45 * rate && p[0].1 <= 45 * rate + 300, "{p:?}");
        assert!(p[1].1 >= 92 * rate && p[1].1 <= 92 * rate + 300, "{p:?}");
        assert_eq!(p[2].1, s.len());
        for w in p.windows(2) {
            assert_eq!(w[0].1, w[1].0);
        }
        assert!(p.iter().all(|(a, b)| (b - a) as f64 <= MAX_PIECE * rate as f64));
        assert_eq!(plan_pieces(&[], 16_000), [(0, 0)]);
    }

    #[test]
    fn locales() {
        let sup: Vec<String> = ["en-US", "en-GB", "en-AU", "de-DE", "de-AT", "pt-BR", "pt-PT", "fr-CA", "fr-FR", "ja-JP"].iter().map(|s| s.to_string()).collect();
        assert_eq!(pick_locale(None, "en_GB", &sup).as_deref(), Some("en-GB"));
        assert_eq!(pick_locale(Some("en"), "en_GB", &sup).as_deref(), Some("en-GB"));
        assert_eq!(pick_locale(Some("en"), "de_DE", &sup).as_deref(), Some("en-AU"));
        assert_eq!(pick_locale(Some("de"), "en_US", &sup).as_deref(), Some("de-DE"));
        assert_eq!(pick_locale(Some("fr"), "en_CA", &sup).as_deref(), Some("fr-CA"));
        assert_eq!(pick_locale(Some("pt-br"), "en_US", &sup).as_deref(), Some("pt-BR"));
        assert_eq!(pick_locale(Some("pt_PT"), "en_US", &sup).as_deref(), Some("pt-PT"));
        assert_eq!(pick_locale(Some("auto"), "ja_JP", &sup).as_deref(), Some("ja-JP"));
        assert_eq!(pick_locale(Some("xx"), "en_US", &sup), None);
        assert_eq!(pick_locale(None, "nl_NL", &sup), None);
        assert_eq!(normalize("zh_Hans_CN"), "zh-Hans-CN");
    }

    #[test]
    fn messages_say_what_to_do() {
        let m = |b: Blocker, d| b.message(d);
        assert!(m(Blocker::Permission(Permission::NotDetermined), Device::Mac).contains("Allow Speech Recognition"));
        assert!(m(Blocker::Permission(Permission::Denied), Device::Mac).contains("System Settings ▸ Privacy & Security ▸ Speech Recognition"));
        assert!(m(Blocker::Permission(Permission::Denied), Device::IPad).starts_with("Speech Recognition is turned off"));
        assert!(!m(Blocker::Permission(Permission::Denied), Device::IPad).contains("System Settings"));
        assert!(m(Blocker::Permission(Permission::Restricted), Device::IPad).contains("this iPad"));
        assert!(m(Blocker::NotInApp { cli: true }, Device::Mac).contains("--offline"));
        assert!(m(Blocker::NotInApp { cli: false }, Device::Mac).contains("app bundle"));
        let n = m(Blocker::NotOnDevice("German (Germany)".into()), Device::Mac);
        assert!(n.contains("on-device") && n.contains("never sends audio"), "{n}");
        assert!(Blocker::Permission(Permission::NotDetermined).can_ask());
        assert!(!Blocker::Permission(Permission::Denied).can_ask());
        assert!(is_no_speech("kAFAssistantErrorDomain", 1110));
        assert!(!is_no_speech("kAFAssistantErrorDomain", 1700));
        assert_eq!(Permission::Authorized.name(), "allowed");
    }

    #[test]
    fn reads_the_prepared_wav() {
        let wav = crate::wav::mono_bytes(16_000, &[0.0, 0.5, -0.5, 1.0]);
        let (rate, s) = wav_samples(&wav).unwrap();
        assert_eq!(rate, 16_000);
        assert_eq!(s.len(), 4);
        assert!((s[1] - 0.5).abs() < 1e-3 && (s[2] + 0.5).abs() < 1e-3);
        assert!(wav_samples(b"not a wav file").is_err());
    }
}
