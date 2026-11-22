//! A tile's subtitles: the timed lines of dialogue in its audio, as QuipClipper sends
//! them with a clip (its `cues`, `words` and `cues_source`, D-260).
//!
//! The code says "subtitles" because a tile's `title` is its name and a "cue" is a step
//! of a cue list; on disk they keep QuipClipper's field names, so a library, a set file
//! and a QuipClipper sidecar read the same way. Times are seconds into the tile's audio
//! file, the frame of `start`, `end`, markers and the vamp, so a trim never moves them.
//!
//! Everything that comes in goes through [`parse_lines`] and [`parse_source`]: what is
//! valid is kept, the rest is counted as dropped, and nothing fails because of them.

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};

/// The most lines a tile keeps; a clip is a few seconds, a song or a scene a few hundred.
pub const MAX_LINES: usize = 2000;
/// The longest line, in characters; a longer one is cut short with "…".
pub const MAX_TEXT: usize = 500;
/// The most words in one line.
pub const MAX_WORDS: usize = 200;
/// The longest word, in characters; a longer one is cut short.
pub const MAX_WORD: usize = 64;
/// The longest `kind` or `lang` of a source, and the most bytes of its other fields.
const MAX_SOURCE_NAME: usize = 64;
const MAX_SOURCE_EXTRA: usize = 4096;

/// One subtitle line: when it is spoken and what is said.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SubtitleLine {
    /// Seconds into the audio file.
    pub start: f64,
    pub end: f64,
    /// The line, which may hold a `\n` (a two-row subtitle).
    pub text: String,
    /// Each word's time, in order: from the source when it had word timings, or set by
    /// hand in the editor (then `aligned`). Empty: only the line's time is known.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub words: Vec<SubtitleWord>,
    /// The words (and the line's start and end around them) were placed by hand in
    /// LeadStinger. A new copy of the subtitles from the source keeps such a line rather
    /// than overwrite it ([`merge`]). A LeadStinger field; QuipClipper never writes it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub aligned: bool,
    /// The line's timing as its source gave it, kept by the editor's first change to the
    /// line so Reset can put it back (D-267). A LeadStinger field; QuipClipper never
    /// writes it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_timing: Option<SourceTiming>,
    /// The line was added in LeadStinger's editor, not given by the source: a new copy
    /// of the subtitles from the source keeps it ([`merge`], D-300). A LeadStinger field;
    /// QuipClipper never writes it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub added: bool,
    /// A line an engine heard where none of the tile's own subtitles is spoken (D-296):
    /// LeadStinger added it, machine-made, to subtitles from elsewhere, and this says
    /// which engine (`{kind: machine, engine, model, lang}`). `None` for the subtitles'
    /// own lines, and for lines whose whole set is machine-made (the tile's
    /// `cues_source` says so). QuipClipper's `machine: true` reads as `{kind: machine}`.
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient_machine")]
    pub machine: Option<SubtitleSource>,
}

/// A line's start, end and words as they were before the editor changed them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SourceTiming {
    pub start: f64,
    pub end: f64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub words: Vec<SubtitleWord>,
}

/// One word of a line, in seconds into the audio file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SubtitleWord {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

/// Where the subtitles came from: `kind` (e.g. `srt`, `embedded`, `whisper`), `lang`,
/// and whatever else the source said, kept as it came.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SubtitleSource {
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// What [`parse_lines`] made of a list: the lines it kept, and how many lines and words
/// it couldn't use.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Parsed {
    pub lines: Vec<SubtitleLine>,
    pub dropped_lines: usize,
    pub dropped_words: usize,
}

impl Parsed {
    /// What was dropped, for an import's report; none when everything was used.
    pub fn problem(&self) -> Option<String> {
        let plural = |n: usize, what: &str| format!("{n} {what}{}", if n == 1 { "" } else { "s" });
        match (self.dropped_lines, self.dropped_words) {
            (0, 0) => None,
            (l, 0) => Some(format!("skipped {} it couldn't read", plural(l, "subtitle line"))),
            (0, w) => Some(format!("skipped {} it couldn't read", plural(w, "subtitle word"))),
            (l, w) => Some(format!("skipped {} and {} it couldn't read", plural(l, "subtitle line"), plural(w, "word"))),
        }
    }
}

/// A time: a finite number of seconds, not negative.
fn seconds(v: Option<&Value>) -> Option<f64> {
    v?.as_f64().filter(|s| s.is_finite() && *s >= 0.0)
}

/// Text made safe to keep: line breaks as `\n` (none when `one_row`), no other control
/// characters, trimmed, at most `max` characters.
fn clean_text(s: &str, max: usize, one_row: bool) -> String {
    let s = s.replace("\r\n", "\n").replace('\r', "\n");
    let rows: Vec<String> = s.split('\n').map(|r| r.chars().filter(|c| !c.is_control()).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ")).filter(|r| !r.is_empty()).collect();
    let joined = rows.join(if one_row { " " } else { "\n" });
    if joined.chars().count() <= max {
        return joined;
    }
    let mut cut: String = joined.chars().take(max.saturating_sub(1)).collect();
    cut = cut.trim_end().to_owned();
    cut.push('…');
    cut
}

/// `{start, end, text}` with start before end and some text, or none.
fn timed(v: &Value, max: usize, one_row: bool) -> Option<(f64, f64, String)> {
    let o = v.as_object()?;
    let (start, end) = (seconds(o.get("start"))?, seconds(o.get("end"))?);
    let text = clean_text(o.get("text")?.as_str()?, max, one_row);
    (end > start && !text.is_empty()).then_some((start, end, text))
}

/// Read a list of lines as QuipClipper writes `cues` (or as a library or set file keeps
/// them), keeping every valid line and word, in time order. Not a list (or null) is no
/// lines; anything but null that isn't a list counts as one dropped line.
pub fn parse_lines(v: &Value) -> Parsed {
    let mut out = Parsed::default();
    let items = match v {
        Value::Null => return out,
        Value::Array(a) => a,
        _ => {
            out.dropped_lines = 1;
            return out;
        }
    };
    for item in items {
        let Some((start, end, text)) = timed(item, MAX_TEXT, false) else {
            out.dropped_lines += 1;
            continue;
        };
        let words = parse_words(item.get("words"), &mut out.dropped_words);
        let aligned = !words.is_empty() && item.get("aligned").and_then(Value::as_bool) == Some(true);
        // What the editor kept of the source's timing: all of it, or none.
        let source_timing = item.get("source_timing").and_then(|st| {
            let (start, end) = (seconds(st.get("start"))?, seconds(st.get("end"))?);
            let mut ignored = 0;
            (end > start).then(|| SourceTiming { start, end, words: parse_words(st.get("words"), &mut ignored) })
        });
        let added = item.get("added").and_then(Value::as_bool) == Some(true);
        let machine = machine_mark(item.get("machine"));
        let line = SubtitleLine { start, end, text, words, aligned, source_timing, added, machine };
        if out.lines.contains(&line) {
            out.dropped_lines += 1;
            continue;
        }
        out.lines.push(line);
    }
    out.lines.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.end.total_cmp(&b.end)));
    if out.lines.len() > MAX_LINES {
        out.dropped_lines += out.lines.len() - MAX_LINES;
        out.lines.truncate(MAX_LINES);
    }
    out
}

/// A line's `words`, in time order: the valid ones, at most [`MAX_WORDS`], counting the
/// rest in `dropped`.
fn parse_words(v: Option<&Value>, dropped: &mut usize) -> Vec<SubtitleWord> {
    let mut words = Vec::new();
    match v {
        None | Some(Value::Null) => {}
        Some(Value::Array(ws)) => {
            for w in ws {
                match timed(w, MAX_WORD, true) {
                    Some((start, end, text)) if words.len() < MAX_WORDS => words.push(SubtitleWord { start, end, text }),
                    _ => *dropped += 1,
                }
            }
        }
        Some(_) => *dropped += 1,
    }
    words.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.end.total_cmp(&b.end)));
    words
}

/// Read `cues_source`: an object with a `kind`. Anything else is none. `lang` is kept
/// when it's short text; the other fields are kept as they came while they're small.
pub fn parse_source(v: &Value) -> Option<SubtitleSource> {
    let o = v.as_object()?;
    let short = |k: &str| o.get(k).and_then(Value::as_str).map(|s| clean_text(s, MAX_SOURCE_NAME, true)).filter(|s| !s.is_empty());
    let kind = short("kind")?;
    let lang = short("lang");
    let mut extra: Map<String, Value> = o.iter().filter(|(k, _)| *k != "kind" && *k != "lang").map(|(k, v)| (k.clone(), v.clone())).collect();
    if serde_json::to_vec(&extra).map_or(true, |b| b.len() > MAX_SOURCE_EXTRA) {
        extra.clear();
    }
    Some(SubtitleSource { kind, lang, extra })
}

/// A line's `machine` mark: a source object, or `true` (QuipClipper's flag) for one that
/// says only that it's machine-made.
fn machine_mark(v: Option<&Value>) -> Option<SubtitleSource> {
    match v? {
        Value::Bool(true) => Some(SubtitleSource { kind: "machine".to_owned(), lang: None, extra: Map::new() }),
        v => parse_source(v),
    }
}

fn lenient_machine<'de, D: Deserializer<'de>>(d: D) -> Result<Option<SubtitleSource>, D::Error> {
    Ok(machine_mark(Some(&Value::deserialize(d)?)))
}

/// Serde: a tile's `cues` read leniently, so a bad line never stops a library loading.
pub fn lenient_lines<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<SubtitleLine>, D::Error> {
    Ok(parse_lines(&Value::deserialize(d)?).lines)
}

/// Serde: a tile's `cues_source` read leniently.
pub fn lenient_source<'de, D: Deserializer<'de>>(d: D) -> Result<Option<SubtitleSource>, D::Error> {
    Ok(parse_source(&Value::deserialize(d)?))
}

/// The lines as JSON, the shape they are stored in (`cues`).
pub fn to_value(lines: &[SubtitleLine]) -> Value {
    serde_json::to_value(lines).unwrap_or(Value::Null)
}

/// A new copy of the subtitles from their source, keeping the work done here: an incoming
/// line whose text matches an aligned line that overlaps it in time is replaced by that
/// aligned line, and the lines added in the editor stay (D-300), standing in for an
/// incoming line with the same text at the same time. Lines an engine added between the
/// old lines (`machine`, D-299) stay where no other line overlaps them. Returns the lines
/// and how many of the library's aligned or added lines were kept.
pub fn merge(current: &[SubtitleLine], incoming: Vec<SubtitleLine>) -> (Vec<SubtitleLine>, usize) {
    let mut taken = vec![false; current.len()];
    let mut kept = 0;
    let mut lines: Vec<SubtitleLine> = incoming
        .into_iter()
        .map(|line| {
            let mine = current.iter().enumerate().position(|(i, c)| !taken[i] && keeps(c, &line));
            match mine {
                Some(i) => {
                    taken[i] = true;
                    kept += 1;
                    current[i].clone()
                }
                None => line,
            }
        })
        .collect();
    for (i, c) in current.iter().enumerate() {
        if c.added && !taken[i] && lines.len() < MAX_LINES {
            kept += 1;
            lines.push(c.clone());
        }
    }
    let heard: Vec<SubtitleLine> = current
        .iter()
        .enumerate()
        .filter(|(i, c)| !taken[*i] && c.machine.is_some() && !lines.iter().any(|l| l.start < c.end && c.start < l.end))
        .map(|(_, c)| c.clone())
        .collect();
    lines.extend(heard);
    lines.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.end.total_cmp(&b.end)));
    (lines, kept)
}

/// Whether `mine`, a line as the library has it, stands in for `incoming` from the
/// source: a line aligned by hand whose text matches and that overlaps it (within a
/// second), or a line changed in the editor, its text too (a spelling fixed), whose
/// source timing is `incoming`'s (D-273).
fn keeps(mine: &SubtitleLine, incoming: &SubtitleLine) -> bool {
    let near = |a: f64, b: f64| (a - b).abs() <= SAME_TIME;
    let overlaps = mine.start < incoming.end + 1.0 && incoming.start < mine.end + 1.0;
    if mine.added {
        // Only the same words at the same time: its source timing is its own, not the source's.
        return same_text(&mine.text, &incoming.text) && overlaps;
    }
    let by_text = mine.aligned && same_text(&mine.text, &incoming.text) && mine.start < incoming.end + 1.0 && incoming.start < mine.end + 1.0;
    let by_source = !same_text(&mine.text, &incoming.text) && mine.source_timing.as_ref().is_some_and(|st| near(st.start, incoming.start) && near(st.end, incoming.end));
    by_text || by_source
}

/// Two times this close are the same moment of a source's timing.
const SAME_TIME: f64 = 0.05;

/// Text equal but for case, spacing and line breaks.
fn same_text(a: &str, b: &str) -> bool {
    let norm = |s: &str| s.split_whitespace().map(str::to_lowercase).collect::<Vec<_>>();
    norm(a) == norm(b)
}

/// The lines that are heard between `start` and `end` (a tile's trim): those that overlap
/// it.
pub fn heard(lines: &[SubtitleLine], start: f64, end: f64) -> impl Iterator<Item = &SubtitleLine> {
    lines.iter().filter(move |l| l.end > start && l.start < end)
}

/// All the lines' text on one line, for search and summaries.
pub fn plain_text(lines: &[SubtitleLine]) -> String {
    lines.iter().map(|l| l.text.split_whitespace().collect::<Vec<_>>().join(" ")).collect::<Vec<_>>().join(" ")
}

/// Whether any line contains `query` (case doesn't matter; spacing and line breaks are
/// one space).
pub fn contains(lines: &[SubtitleLine], query: &str) -> bool {
    let q = query.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
    !q.is_empty() && plain_text(lines).to_lowercase().contains(&q)
}

impl SubtitleLine {
    /// The line's words spread evenly over its time, each in proportion to its length:
    /// where the editor starts when a line has no word timings.
    pub fn even_words(&self) -> Vec<SubtitleWord> {
        let words: Vec<&str> = self.text.split_whitespace().take(MAX_WORDS).collect();
        let total: usize = words.iter().map(|w| w.chars().count().max(1)).sum();
        let span = self.end - self.start;
        let mut at = self.start;
        words
            .iter()
            .map(|w| {
                let len = span * w.chars().count().max(1) as f64 / total.max(1) as f64;
                let word = SubtitleWord { start: at, end: at + len, text: w.chars().take(MAX_WORD).collect() };
                at += len;
                word
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn keeps_what_is_valid_in_time_order() {
        let p = parse_lines(&json!([
            {"start": 2.0, "end": 3.0, "text": "Second"},
            {"start": 0.5, "end": 1.5, "text": "  First\r\nrow two  "},
            {"start": 1.0, "end": 2.5, "text": "Overlaps"},
            {"start": 1.0, "end": 0.5, "text": "Backwards"},
            {"start": -1.0, "end": 1.0, "text": "Before the file"},
            {"start": 1.0, "end": 2.0, "text": "   "},
            {"start": "1", "end": 2.0, "text": "A string time"},
            {"end": 2.0, "text": "No start"},
            "not a line",
            {"start": 2.0, "end": 3.0, "text": "Second"}
        ]));
        let texts: Vec<_> = p.lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts, ["First\nrow two", "Overlaps", "Second"]);
        assert_eq!((p.dropped_lines, p.dropped_words), (7, 0), "six bad lines and one repeat");
        assert_eq!(p.problem().as_deref(), Some("skipped 7 subtitle lines it couldn't read"));
        assert_eq!(parse_lines(&json!(null)), Parsed::default());
        assert_eq!(parse_lines(&json!("cues")).dropped_lines, 1);
        assert_eq!(parse_lines(&json!([])).problem(), None);
    }

    #[test]
    fn words_are_checked_sorted_and_bounded() {
        let p = parse_lines(&json!([{"start": 0.0, "end": 2.0, "text": "Hello there", "aligned": true, "words": [
            {"start": 1.0, "end": 2.0, "text": "there"},
            {"start": 0.0, "end": 0.9, "text": "Hello"},
            {"start": 0.0, "end": 0.9, "text": ""},
            {"start": 0.5, "text": "no end"}
        ]}, {"start": 3.0, "end": 4.0, "text": "Hm", "aligned": true, "words": "bad"}]));
        let w: Vec<_> = p.lines[0].words.iter().map(|w| w.text.as_str()).collect();
        assert_eq!(w, ["Hello", "there"]);
        assert!(p.lines[0].aligned);
        assert!(!p.lines[1].aligned, "no words, nothing aligned");
        assert_eq!((p.dropped_lines, p.dropped_words), (0, 3));
        let many: Vec<Value> = (0..MAX_WORDS + 5).map(|i| json!({"start": i as f64, "end": i as f64 + 0.5, "text": "w"})).collect();
        let p = parse_lines(&json!([{"start": 0.0, "end": 500.0, "text": "long", "words": many}]));
        assert_eq!((p.lines[0].words.len(), p.dropped_words), (MAX_WORDS, 5));
    }

    #[test]
    fn sizes_are_bounded() {
        let long = "x".repeat(MAX_TEXT + 50);
        let p = parse_lines(&json!([{"start": 0.0, "end": 1.0, "text": long}]));
        assert_eq!(p.lines[0].text.chars().count(), MAX_TEXT);
        assert!(p.lines[0].text.ends_with('…'));
        let lines: Vec<Value> = (0..MAX_LINES + 3).map(|i| json!({"start": i as f64, "end": i as f64 + 0.5, "text": format!("{i}")})).collect();
        let p = parse_lines(&Value::Array(lines));
        assert_eq!((p.lines.len(), p.dropped_lines), (MAX_LINES, 3));
    }

    #[test]
    fn sources_keep_their_fields_while_small() {
        let s = parse_source(&json!({"kind": "srt", "lang": "en", "file": "Heat.en.srt", "offset": 0.0})).unwrap();
        assert_eq!((s.kind.as_str(), s.lang.as_deref(), s.extra.len()), ("srt", Some("en"), 2));
        assert_eq!(serde_json::to_value(&s).unwrap(), json!({"kind": "srt", "lang": "en", "file": "Heat.en.srt", "offset": 0.0}));
        assert_eq!(parse_source(&json!({"lang": "en"})), None, "a source needs a kind");
        assert_eq!(parse_source(&json!("srt")), None);
        let big = parse_source(&json!({"kind": "srt", "blob": "x".repeat(MAX_SOURCE_EXTRA)})).unwrap();
        assert!(big.extra.is_empty());
    }

    #[test]
    fn gap_lines_keep_their_mark_and_a_new_copy_keeps_them_where_nothing_new_is_said() {
        // QuipClipper's flag and LeadStinger's source both read as a machine mark (D-296).
        let p = parse_lines(&json!([
            {"start": 0.0, "end": 1.0, "text": "Known"},
            {"start": 2.0, "end": 3.0, "text": "Heard", "machine": {"kind": "machine", "engine": "whisper.cpp", "model": "tiny.en"}},
            {"start": 4.0, "end": 5.0, "text": "Auto", "machine": true},
            {"start": 6.0, "end": 7.0, "text": "Not a mark", "machine": "yes"}
        ]));
        let marks: Vec<Option<&str>> = p.lines.iter().map(|l| l.machine.as_ref().map(|m| m.kind.as_str())).collect();
        assert_eq!(marks, [None, Some("machine"), Some("machine"), None]);
        assert_eq!(p.lines[1].machine.as_ref().unwrap().extra["model"], "tiny.en");
        let stored = to_value(&p.lines);
        assert_eq!(stored[1]["machine"], json!({"kind": "machine", "engine": "whisper.cpp", "model": "tiny.en"}));
        assert!(stored[0].get("machine").is_none(), "known lines carry no mark");
        let back: Vec<SubtitleLine> = serde_json::from_value(stored).unwrap();
        assert_eq!(back, p.lines, "the mark survives the library file");

        // A new copy of the known lines: a gap line nothing new overlaps stays; one the
        // new copy now says something over goes.
        let current = p.lines[..3].to_vec();
        let line = |s: f64, e: f64, t: &str| SubtitleLine { start: s, end: e, text: t.into(), words: vec![], aligned: false, source_timing: None, added: false, machine: None };
        let (lines, _) = merge(&current, vec![line(0.0, 1.0, "Known"), line(3.8, 4.5, "Now subtitled")]);
        let texts: Vec<&str> = lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts, ["Known", "Heard", "Now subtitled"]);
    }

    #[test]
    fn a_new_copy_keeps_hand_aligned_lines() {
        let line = |s: f64, t: &str| SubtitleLine { start: s, end: s + 1.0, text: t.into(), words: vec![], aligned: false, source_timing: None, added: false, machine: None };
        let mut mine = line(0.1, "Don't let yourself");
        mine.words = mine.even_words();
        mine.aligned = true;
        let current = vec![mine.clone(), line(2.0, "get attached")];
        let incoming = vec![line(0.0, "don't  let yourself"), line(2.0, "get attached to anything")];
        let (lines, kept) = merge(&current, incoming);
        assert_eq!(kept, 1);
        assert_eq!(lines[0], mine);
        assert_eq!(lines[1].text, "get attached to anything");
        let (lines, kept) = merge(&current, vec![line(30.0, "Don't let yourself")]);
        assert_eq!((kept, lines[0].aligned), (0, false), "far away in time is another line");
        // A line whose spelling was fixed here is kept by its source timing, aligned or not.
        let mut fixed = line(2.0, "get atached");
        fixed.source_timing = Some(SourceTiming { start: 2.0, end: 3.0, words: vec![] });
        fixed.text = "get attached".into();
        let (lines, kept) = merge(&[fixed.clone()], vec![line(2.0, "get atached")]);
        assert_eq!((kept, &lines[0]), (1, &fixed));
        let (lines, kept) = merge(&[fixed.clone()], vec![line(2.5, "get atached")]);
        assert_eq!((kept, lines[0].text.as_str()), (0, "get atached"), "the source's timing changed: a new line");
        // An auto-aligned suggestion with the same text isn't kept (D-269).
        let mut suggested = line(2.0, "get attached");
        suggested.source_timing = Some(SourceTiming { start: 2.0, end: 3.0, words: vec![] });
        suggested.words = suggested.even_words();
        assert_eq!(merge(&[suggested], vec![line(2.0, "get attached")]).1, 0);
    }

    /// D-300: lines added in the editor stay through a new copy, standing in for an
    /// incoming line with their text at their time and never for another.
    #[test]
    fn a_new_copy_keeps_added_lines() {
        let line = |s: f64, t: &str| SubtitleLine { start: s, end: s + 1.0, text: t.into(), words: vec![], aligned: false, source_timing: None, added: false, machine: None };
        let mut mine = line(5.0, "(laughter)");
        mine.added = true;
        let mut moved = line(2.0, "my words");
        moved.added = true;
        moved.source_timing = Some(SourceTiming { start: 0.0, end: 1.0, words: vec![] });
        let (lines, kept) = merge(&[mine.clone(), moved.clone()], vec![line(0.0, "theirs"), line(2.2, "My  words")]);
        assert_eq!(kept, 2);
        assert_eq!(lines, vec![line(0.0, "theirs"), moved, mine], "the source's line isn't replaced by an added line's own source timing");
        let p = parse_lines(&to_value(&lines));
        assert!(p.lines[2].added && !p.lines[0].added, "kept in the library");
    }

    #[test]
    fn helpers_for_search_and_the_editor() {
        let l = SubtitleLine { start: 1.0, end: 2.0, text: "Hi\nthere you".into(), words: vec![], aligned: false, source_timing: None, added: false, machine: None };
        let w = l.even_words();
        assert_eq!(w.iter().map(|w| w.text.as_str()).collect::<Vec<_>>(), ["Hi", "there", "you"]);
        assert_eq!((w[0].start, (w[2].end - 2.0).abs() < 1e-9), (1.0, true));
        let lines = vec![l];
        assert!(contains(&lines, "HI THERE"));
        assert!(!contains(&lines, "  "));
        assert_eq!(heard(&lines, 1.9, 5.0).count(), 1);
        assert_eq!(heard(&lines, 2.0, 5.0).count(), 0);
    }
}
