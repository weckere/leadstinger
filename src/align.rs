//! Aligning a tile's subtitle words with its audio (D-267 to D-270), free of UI: the
//! onset finder the editor snaps words to, and the rules for moving and retyping words
//! and lines.
//!
//! Times are seconds into the audio file, as everywhere in [`crate::subtitles`]. A line
//! without word timings is edited through its even split ([`SubtitleLine::even_words`]):
//! the first change to its timing makes those words its own. Every change except
//! [`auto_align`] and [`resplit`] marks the line `aligned` (placed by hand), and the
//! first change of any kind keeps the source's timing in `source_timing` so [`reset`]
//! can put it back.

use crate::decode::DecodedAudio;
use crate::subtitles::{SourceTiming, SubtitleLine, SubtitleWord, MAX_WORD, MAX_WORDS};
use std::collections::VecDeque;
use std::ops::Range;

/// The shortest a word can be made, in seconds.
pub const MIN_WORD: f64 = 0.02;

/// The onset finder's hop: the audio's loudness is measured every 5 ms.
const HOP: f64 = 0.005;
/// A rise is measured against the quietest point this many hops (30 ms) before.
const RISE_HOPS: usize = 6;
/// How far the loudness must climb over that window to count as a sound starting.
const RISE_DB: f32 = 9.0;
/// The local level is the loudest point within this many seconds either side ...
const LOCAL: f64 = 1.0;
/// ... and a rise that peaks further below it than this is background, not a word.
const BELOW_LOCAL_DB: f32 = 30.0;
/// Anything quieter than this is silence.
const FLOOR_DB: f32 = -55.0;
/// Two onsets are at least this far apart.
const MIN_GAP: f64 = 0.06;
/// Two words share a boundary when one ends within this of where the next starts.
const TOUCH: f64 = 1e-6;

/// Where a sound starts: the moment the loudness begins to climb out of the quiet before
/// it, and by how many decibels it climbs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Onset {
    pub at: f64,
    pub strength: f32,
}

/// Every onset in the file. Deterministic: the same audio always gives the same list.
pub fn onsets(audio: &DecodedAudio) -> Vec<Onset> {
    onsets_in(audio, 0.0, f64::INFINITY)
}

/// The onsets from `from` to `to` seconds. The hops sit on a grid fixed to the file and
/// the local level is read a second either side, so a stretch gives the same onsets as
/// the whole file does there.
pub fn onsets_in(audio: &DecodedAudio, from: f64, to: f64) -> Vec<Onset> {
    let rate = f64::from(audio.sample_rate.max(1));
    let hop = ((rate * HOP).round() as usize).max(1);
    let hops_total = audio.frames() / hop;
    let local = (LOCAL / HOP) as usize;
    let first_hop = |sec: f64| ((sec.max(0.0) * rate) as usize / hop).min(hops_total);
    let (want_a, want_b) = (first_hop(from), if to.is_finite() { first_hop(to) } else { hops_total });
    if want_a >= want_b {
        return Vec::new();
    }
    let (a, b) = (want_a.saturating_sub(local), (want_b + local).min(hops_total));
    let pairs = audio.data.as_chunks::<2>().0;
    // Loudness per hop, in dB.
    let db: Vec<f32> = (a..b)
        .map(|k| {
            let frames = &pairs[k * hop..(k + 1) * hop];
            let power = frames.iter().map(|[l, r]| (l * l + r * r) * 0.5).sum::<f32>() / hop as f32;
            10.0 * (power + 1e-12).log10()
        })
        .collect();
    let level = sliding_max(&db, local);
    let mut out: Vec<Onset> = Vec::new();
    for k in (want_a - a).max(RISE_HOPS)..(want_b - a) {
        if db[k] < FLOOR_DB || db[k] < level[k] - BELOW_LOCAL_DB {
            continue;
        }
        let (low_at, low) = (k - RISE_HOPS..k).map(|j| (j, db[j])).fold((k, f32::INFINITY), |m, (j, v)| if v < m.1 { (j, v) } else { m });
        if db[k] - low < RISE_DB {
            continue;
        }
        // The sound starts where it first climbs 3 dB out of the quiet.
        let start = (low_at..=k).find(|&j| db[j] >= low + 3.0).unwrap_or(k);
        let at = ((a + start) * hop) as f64 / rate;
        if out.last().is_some_and(|o| at - o.at < MIN_GAP) {
            continue;
        }
        let peak = db[k..(k + 2 * RISE_HOPS).min(db.len())].iter().copied().fold(f32::MIN, f32::max);
        out.push(Onset { at, strength: peak - low });
    }
    out
}

/// The largest value within `r` places either side of each place.
fn sliding_max(v: &[f32], r: usize) -> Vec<f32> {
    let mut out = vec![0.0; v.len()];
    let mut q: VecDeque<usize> = VecDeque::new();
    let mut next = 0;
    for (i, slot) in out.iter_mut().enumerate() {
        while next < v.len() && next <= i + r {
            while q.back().is_some_and(|&j| v[j] <= v[next]) {
                q.pop_back();
            }
            q.push_back(next);
            next += 1;
        }
        while q.front().is_some_and(|&j| j + r < i) {
            q.pop_front();
        }
        *slot = q.front().map_or(f32::MIN, |&j| v[j]);
    }
    out
}

/// The onset nearest `sec` within `reach` seconds, if any.
pub fn nearest_onset(onsets: &[Onset], sec: f64, reach: f64) -> Option<f64> {
    let i = onsets.partition_point(|o| o.at < sec);
    [i.checked_sub(1), Some(i)]
        .into_iter()
        .flatten()
        .filter_map(|j| onsets.get(j))
        .map(|o| o.at)
        .filter(|at| (at - sec).abs() <= reach)
        .min_by(|a, b| (a - sec).abs().total_cmp(&(b - sec).abs()))
}

/// Which edge of a word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Start,
    End,
}

impl Edge {
    pub fn parse(s: &str) -> Option<Edge> {
        match s {
            "start" => Some(Edge::Start),
            "end" => Some(Edge::End),
            _ => None,
        }
    }
}

/// What the arrow keys move on the selected word: its start edge, its end edge, or the
/// whole word.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Grip {
    #[default]
    Start,
    End,
    Word,
}

impl Grip {
    pub fn parse(s: &str) -> Option<Grip> {
        match s {
            "start" => Some(Grip::Start),
            "end" => Some(Grip::End),
            "word" => Some(Grip::Word),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Grip::Start => "start",
            Grip::End => "end",
            Grip::Word => "word",
        }
    }

    /// The next grip along (Down), or the one before (Up): start, word, end.
    pub fn step(self, forward: bool) -> Grip {
        match (self, forward) {
            (Grip::Start, true) | (Grip::End, false) => Grip::Word,
            (Grip::Word, true) => Grip::End,
            (Grip::Word, false) => Grip::Start,
            (Grip::End, true) => Grip::Start,
            (Grip::Start, false) => Grip::End,
        }
    }
}

/// Move word `i`'s `grip` by `by` seconds: its start or end edge (a shared boundary
/// moves the neighbour's too), or the whole word. The arrow keys' nudge.
pub fn nudge(line: &mut SubtitleLine, i: usize, grip: Grip, by: f64, dur: f64) -> Result<(), String> {
    check(line, i)?;
    let w = words_of(line)[i].clone();
    match grip {
        Grip::Start => move_edge(line, i, Edge::Start, w.start + by, false, dur),
        Grip::End => move_edge(line, i, Edge::End, w.end + by, false, dur),
        Grip::Word => move_word(line, i, w.start + by, false, dur),
    }
}

/// The line's words as the editor shows them: its own, or its even split when it has
/// none (provisional).
pub fn words_of(line: &SubtitleLine) -> Vec<SubtitleWord> {
    if line.words.is_empty() {
        line.even_words()
    } else {
        line.words.clone()
    }
}

/// Before a change: keep the source's timing (once), and make the even split the
/// line's own words when it has none.
fn begin(line: &mut SubtitleLine) {
    if line.source_timing.is_none() {
        line.source_timing = Some(SourceTiming { start: line.start, end: line.end, words: line.words.clone() });
    }
    if line.words.is_empty() {
        line.words = line.even_words();
    }
}

/// After a change to the words: the line spans them, and is marked placed by hand.
fn finish(line: &mut SubtitleLine) {
    if let (Some(a), Some(b)) = (line.words.first(), line.words.last()) {
        line.start = a.start;
        line.end = b.end;
    }
    line.aligned = !line.words.is_empty();
}

fn shared(a: &SubtitleWord, b: &SubtitleWord) -> bool {
    (b.start - a.end).abs() <= TOUCH
}

fn check(line: &SubtitleLine, i: usize) -> Result<(), String> {
    let n = words_of(line).len();
    if i < n {
        Ok(())
    } else {
        Err(format!("the line has {n} word{}", if n == 1 { "" } else { "s" }))
    }
}

/// Move word `i`'s `edge` to `to` (seconds), as far as it can go: a word is never
/// shorter than [`MIN_WORD`] and never crosses its neighbours. A boundary the word
/// shares with its neighbour moves the neighbour's edge too, unless `split`, which
/// leaves the neighbour where it is (opening a gap, or refusing to overlap). `dur` is
/// the audio's length.
pub fn move_edge(line: &mut SubtitleLine, i: usize, edge: Edge, to: f64, split: bool, dur: f64) -> Result<(), String> {
    check(line, i)?;
    begin(line);
    let w = &mut line.words;
    let n = w.len();
    match edge {
        Edge::Start => {
            let follow = i > 0 && !split && shared(&w[i - 1], &w[i]);
            let lo = match i {
                0 => 0.0,
                _ if follow => w[i - 1].start + MIN_WORD,
                _ => w[i - 1].end,
            };
            let hi = (w[i].end - MIN_WORD).max(lo);
            let to = to.clamp(lo, hi);
            if follow {
                w[i - 1].end = to;
            }
            w[i].start = to;
        }
        Edge::End => {
            let follow = i + 1 < n && !split && shared(&w[i], &w[i + 1]);
            let hi = match i + 1 {
                j if j == n => dur,
                _ if follow => w[i + 1].end - MIN_WORD,
                _ => w[i + 1].start,
            };
            let lo = w[i].start + MIN_WORD;
            let to = to.clamp(lo.min(hi), hi.max(lo));
            if follow {
                w[i + 1].start = to;
            }
            w[i].end = to;
        }
    }
    finish(line);
    Ok(())
}

/// Move word `i` whole so it starts at `start`, keeping its length. Its shared
/// boundaries take the neighbours' edges along (unless `split`), so they stretch or
/// shrink, never below [`MIN_WORD`]; otherwise it stops at their edges.
pub fn move_word(line: &mut SubtitleLine, i: usize, start: f64, split: bool, dur: f64) -> Result<(), String> {
    check(line, i)?;
    begin(line);
    let w = &mut line.words;
    let n = w.len();
    let prev = i > 0 && !split && shared(&w[i - 1], &w[i]);
    let next = i + 1 < n && !split && shared(&w[i], &w[i + 1]);
    let lo = match i {
        0 => -w[i].start,
        _ if prev => w[i - 1].start + MIN_WORD - w[i].start,
        _ => w[i - 1].end - w[i].start,
    };
    let hi = match i + 1 {
        j if j == n => dur - w[i].end,
        _ if next => w[i + 1].end - MIN_WORD - w[i].end,
        _ => w[i + 1].start - w[i].end,
    };
    let d = if lo > hi { 0.0 } else { (start - w[i].start).clamp(lo, hi) };
    w[i].start += d;
    w[i].end += d;
    if prev {
        w[i - 1].end = w[i].start;
    }
    if next {
        w[i + 1].start = w[i].end;
    }
    finish(line);
    Ok(())
}

/// Word `i` starts at `at`: its start edge moves there, or, when `at` is past the
/// word's end, the whole word moves there (tapping a line in while it plays).
pub fn tap(line: &mut SubtitleLine, i: usize, at: f64, dur: f64) -> Result<(), String> {
    check(line, i)?;
    let w = words_of(line);
    if at <= w[i].end - MIN_WORD {
        move_edge(line, i, Edge::Start, at, false, dur)
    } else {
        move_word(line, i, at, false, dur)
    }
}

/// Move the whole line by `by` seconds, its words with it, staying inside the audio.
pub fn move_line(line: &mut SubtitleLine, by: f64, dur: f64) {
    begin(line);
    let first = line.words.first().map_or(line.start, |w| w.start);
    let last = line.words.last().map_or(line.end, |w| w.end);
    let d = by.clamp(-first, (dur - last).max(-first));
    for w in &mut line.words {
        w.start += d;
        w.end += d;
    }
    finish(line);
}

/// How [`auto_align`] placed the words.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    /// One onset per word.
    Onsets,
    /// More onsets than words: the first, and the strongest of the rest.
    Strongest,
    /// Fewer onsets than words: the even split.
    Even,
}

impl Method {
    pub fn name(self) -> &'static str {
        match self {
            Method::Onsets => "onsets",
            Method::Strongest => "strongest",
            Method::Even => "even",
        }
    }
}

/// A suggestion: each word of the line starts on an onset inside its span (or up to
/// 100 ms before it), running to the next word. With more onsets than words it takes the
/// first and the strongest rises of the rest; with fewer it falls back to the even split.
/// The line is not marked as placed by hand.
pub fn auto_align(line: &mut SubtitleLine, onsets: &[Onset]) -> Method {
    let texts: Vec<String> = words_of(line).into_iter().map(|w| w.text).collect();
    let n = texts.len();
    let mut found: Vec<Onset> = onsets.iter().copied().filter(|o| o.at >= line.start - 0.1 && o.at < line.end - MIN_WORD).collect();
    let method = match found.len() {
        k if k == n => Method::Onsets,
        k if k > n && n > 0 => {
            let first = found.remove(0);
            found.sort_by(|a, b| b.strength.total_cmp(&a.strength).then(a.at.total_cmp(&b.at)));
            found.truncate(n - 1);
            found.insert(0, first);
            found.sort_by(|a, b| a.at.total_cmp(&b.at));
            Method::Strongest
        }
        _ => Method::Even,
    };
    if line.source_timing.is_none() {
        line.source_timing = Some(SourceTiming { start: line.start, end: line.end, words: line.words.clone() });
    }
    line.words = match method {
        Method::Even => {
            let mut even = line.clone();
            even.words.clear();
            even.even_words()
        }
        _ => {
            let end = line.end.max(found[n - 1].at + MIN_WORD);
            (0..n).map(|i| SubtitleWord { start: found[i].at, end: found.get(i + 1).map_or(end, |o| o.at), text: texts[i].clone() }).collect()
        }
    };
    finish(line);
    line.aligned = false;
    method
}

/// Put the line's timing back as its source gave it. False when there's nothing to put
/// back (the line was never changed in the editor).
pub fn reset(line: &mut SubtitleLine) -> bool {
    let Some(st) = line.source_timing.take() else { return false };
    line.start = st.start;
    line.end = st.end;
    line.words = st.words;
    line.aligned = false;
    true
}

/// The byte ranges of the words in `text`.
fn token_spans(text: &str) -> Vec<Range<usize>> {
    let mut spans = Vec::new();
    let mut start = None;
    for (i, c) in text.char_indices() {
        match (c.is_whitespace(), start) {
            (true, Some(s)) => {
                spans.push(s..i);
                start = None;
            }
            (false, None) => start = Some(i),
            _ => {}
        }
    }
    if let Some(s) = start {
        spans.push(s..text.len());
    }
    spans
}

/// Text cleaned to words: no control characters, each at most [`MAX_WORD`] characters.
fn clean_words(text: &str) -> Vec<String> {
    text.split_whitespace().map(|w| w.chars().filter(|c| !c.is_control()).take(MAX_WORD).collect::<String>()).filter(|w| !w.is_empty()).collect()
}

/// Replace words `i..i + count` of the line's text with `with`, keeping the rest of the
/// text (its line breaks too) as it is.
fn replace_words(text: &str, i: usize, count: usize, with: &str) -> String {
    let spans = token_spans(text);
    match (spans.get(i), spans.get(i + count - 1)) {
        (Some(a), Some(b)) => format!("{}{with}{}", &text[..a.start], &text[b.end..]),
        _ => text.to_owned(),
    }
}

/// Retype word `i`. Text with spaces in it splits the word, its time shared out by
/// length. A line without word timings keeps none: only its text changes.
pub fn set_word_text(line: &mut SubtitleLine, i: usize, text: &str) -> Result<(), String> {
    check(line, i)?;
    let new = clean_words(text);
    if new.is_empty() {
        return Err("a word needs some text".into());
    }
    if words_of(line).len() - 1 + new.len() > MAX_WORDS {
        return Err(format!("a line has at most {MAX_WORDS} words"));
    }
    if line.source_timing.is_none() {
        line.source_timing = Some(SourceTiming { start: line.start, end: line.end, words: line.words.clone() });
    }
    let timed = !line.words.is_empty();
    // When the words match the text one for one, edit the text in place; otherwise it
    // is rebuilt from the words.
    let in_place = token_spans(&line.text).len() == words_of(line).len();
    if timed {
        let w = line.words.remove(i);
        let total: usize = new.iter().map(|s| s.chars().count().max(1)).sum();
        let mut at = w.start;
        for (k, s) in new.iter().enumerate() {
            let len = (w.end - w.start) * s.chars().count().max(1) as f64 / total as f64;
            let end = if k + 1 == new.len() { w.end } else { at + len };
            line.words.insert(i + k, SubtitleWord { start: at, end, text: s.clone() });
            at = end;
        }
    }
    line.text = if in_place || !timed { replace_words(&line.text, i, 1, &new.join(" ")) } else { line.words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ") };
    Ok(())
}

/// Join word `i` and the next into one word, spanning both.
pub fn join_words(line: &mut SubtitleLine, i: usize) -> Result<(), String> {
    check(line, i)?;
    check(line, i + 1).map_err(|_| "there's no word after it to join".to_owned())?;
    if line.source_timing.is_none() {
        line.source_timing = Some(SourceTiming { start: line.start, end: line.end, words: line.words.clone() });
    }
    let in_place = token_spans(&line.text).len() == words_of(line).len();
    let joined: String = words_of(line)[i..=i + 1].iter().map(|w| w.text.as_str()).collect::<String>().chars().take(MAX_WORD).collect();
    if !line.words.is_empty() {
        let next = line.words.remove(i + 1);
        line.words[i].end = next.end;
        line.words[i].text = joined.clone();
    }
    line.text = if in_place || line.words.is_empty() { replace_words(&line.text, i, 2, &joined) } else { line.words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ") };
    Ok(())
}

/// Typed text made a line's text: its rows (a line break is a new row) cleaned to words,
/// at most [`crate::subtitles::MAX_TEXT`] characters and [`MAX_WORDS`] words.
pub fn line_text(t: &str) -> Result<String, String> {
    let rows: Vec<String> = t.lines().map(|r| clean_words(r).join(" ")).filter(|r| !r.is_empty()).collect();
    if rows.is_empty() {
        return Err("a line needs some text".into());
    }
    let text = rows.join("\n");
    if text.chars().count() > crate::subtitles::MAX_TEXT {
        return Err(format!("a line has at most {} characters", crate::subtitles::MAX_TEXT));
    }
    if text.split_whitespace().count() > MAX_WORDS {
        return Err(format!("a line has at most {MAX_WORDS} words"));
    }
    Ok(text)
}

/// How long a new line of `words` words lasts when only its start is given: about a
/// third of a second a word, from 0.8 s to 6 s (D-301).
pub fn spoken_length(words: usize) -> f64 {
    (0.3 * words as f64).clamp(0.8, 6.0)
}

/// Where a new line starting at `at` ends: after [`spoken_length`], but before the next
/// line that starts later (unless that leaves it shorter than its words need) and inside
/// the audio.
pub fn new_line_end(lines: &[SubtitleLine], at: f64, words: usize, dur: f64) -> f64 {
    let least = MIN_WORD * words.max(1) as f64;
    let next = lines.iter().map(|l| l.start).filter(|&s| s >= at + least).fold(f64::INFINITY, f64::min);
    (at + spoken_length(words)).min(next).min(dur)
}

/// Split the line into words again from its text (`text` replaces it first): the word
/// timings are dropped, so it shows the even split until it's aligned again.
pub fn resplit(line: &mut SubtitleLine, text: Option<&str>) -> Result<(), String> {
    if let Some(t) = text {
        line.text = line_text(t)?;
    }
    if line.source_timing.is_none() {
        line.source_timing = Some(SourceTiming { start: line.start, end: line.end, words: line.words.clone() });
    }
    line.words.clear();
    line.aligned = false;
    Ok(())
}

/// Where `sec` falls: the line spoken then and the word within it, if any.
pub fn at_time(lines: &[SubtitleLine], sec: f64) -> Option<(usize, Option<usize>)> {
    let li = lines.iter().position(|l| l.start <= sec && sec < l.end)?;
    let wi = words_of(&lines[li]).iter().position(|w| w.start <= sec && sec < w.end);
    Some((li, wi))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demo::synth;

    fn line(start: f64, end: f64, text: &str) -> SubtitleLine {
        SubtitleLine { start, end, text: text.into(), words: vec![], aligned: false, source_timing: None, added: false, machine: None }
    }

    fn word(start: f64, end: f64, text: &str) -> SubtitleWord {
        SubtitleWord { start, end, text: text.into() }
    }

    fn spans(l: &SubtitleLine) -> Vec<(f64, f64)> {
        l.words.iter().map(|w| ((w.start * 1000.0).round() / 1000.0, (w.end * 1000.0).round() / 1000.0)).collect()
    }

    fn timed() -> SubtitleLine {
        let mut l = line(1.0, 2.0, "one two three");
        l.words = vec![word(1.0, 1.3, "one"), word(1.3, 1.6, "two"), word(1.7, 2.0, "three")];
        l
    }

    #[test]
    fn a_shared_edge_moves_both_words_unless_split() {
        let mut l = timed();
        move_edge(&mut l, 1, Edge::Start, 1.2, false, 10.0).unwrap();
        assert_eq!(spans(&l), [(1.0, 1.2), (1.2, 1.6), (1.7, 2.0)]);
        assert!(l.aligned, "moving anything marks the line aligned");
        assert_eq!(l.source_timing.as_ref().unwrap().words, timed().words, "the source's timing is kept");
        move_edge(&mut l, 1, Edge::Start, 1.25, true, 10.0).unwrap();
        assert_eq!(spans(&l), [(1.0, 1.2), (1.25, 1.6), (1.7, 2.0)], "split: the gap opens");
        move_edge(&mut l, 1, Edge::Start, 1.1, true, 10.0).unwrap();
        assert_eq!(spans(&l), [(1.0, 1.2), (1.2, 1.6), (1.7, 2.0)], "split: it stops at the neighbour, no overlap");
        // A gap's edge doesn't pull the neighbour: two moves to 1.65 touch, then stop.
        move_edge(&mut l, 1, Edge::End, 1.9, false, 10.0).unwrap();
        assert_eq!(spans(&l)[1], (1.2, 1.7));
    }

    #[test]
    fn words_keep_their_order_and_a_minimum_length() {
        let mut l = timed();
        move_edge(&mut l, 1, Edge::Start, 0.0, false, 10.0).unwrap();
        assert_eq!(spans(&l)[..2], [(1.0, 1.02), (1.02, 1.6)], "the neighbour keeps MIN_WORD");
        move_edge(&mut l, 1, Edge::End, 5.0, false, 10.0).unwrap();
        assert_eq!(spans(&l)[1], (1.02, 1.7), "stops at the next word across a gap");
        move_edge(&mut l, 2, Edge::End, 50.0, false, 10.0).unwrap();
        assert_eq!(spans(&l)[2], (1.7, 10.0), "the last word stops at the end of the audio");
        assert_eq!((l.start, l.end), (1.0, 10.0), "the line follows its first and last word");
        move_edge(&mut l, 0, Edge::Start, -3.0, false, 10.0).unwrap();
        assert_eq!(l.start, 0.0, "nothing before the file");
        move_edge(&mut l, 2, Edge::Start, 99.0, false, 10.0).unwrap();
        assert_eq!(spans(&l)[2], (9.98, 10.0));
        for w in l.words.windows(2) {
            assert!(w[0].end <= w[1].start + 1e-9 && w[0].start < w[0].end);
        }
        assert!(move_edge(&mut l, 3, Edge::Start, 1.0, false, 10.0).is_err());
    }

    #[test]
    fn moving_a_word_drags_shared_edges_or_stops_at_gaps() {
        let mut l = timed();
        move_word(&mut l, 1, 1.4, false, 10.0).unwrap();
        // Shared on the left (one stretches), a gap on the right (stops at 1.7).
        assert_eq!(spans(&l), [(1.0, 1.4), (1.4, 1.7), (1.7, 2.0)]);
        move_word(&mut l, 1, 0.0, false, 10.0).unwrap();
        assert_eq!(spans(&l)[..2], [(1.0, 1.02), (1.02, 1.32)]);
        let mut s = timed();
        move_word(&mut s, 1, 1.35, true, 10.0).unwrap();
        assert_eq!(spans(&s), [(1.0, 1.3), (1.35, 1.65), (1.7, 2.0)], "split: the neighbour stays");
        move_word(&mut s, 1, 1.0, true, 10.0).unwrap();
        assert_eq!(spans(&s)[1], (1.3, 1.6));
    }

    #[test]
    fn a_line_moves_whole_and_inside_the_audio() {
        let mut l = line(1.0, 2.0, "a provisional line");
        move_line(&mut l, 0.5, 10.0);
        assert_eq!((l.start, l.end, l.words.len(), l.aligned), (1.5, 2.5, 3, true));
        move_line(&mut l, -9.0, 10.0);
        assert_eq!((l.start, l.end), (0.0, 1.0));
        move_line(&mut l, 99.0, 10.0);
        assert_eq!((l.start, l.end), (9.0, 10.0));
    }

    #[test]
    fn tapping_sets_the_start_or_moves_the_word() {
        let mut l = timed();
        tap(&mut l, 0, 1.1, 10.0).unwrap();
        assert_eq!(spans(&l)[0], (1.1, 1.3));
        tap(&mut l, 2, 1.9, 10.0).unwrap();
        assert_eq!(spans(&l)[2], (1.9, 2.0));
        tap(&mut l, 2, 2.5, 10.0).unwrap();
        assert_eq!(spans(&l)[2], (2.5, 2.6), "past its end: the whole word moves");
    }

    #[test]
    fn reset_puts_the_source_timing_back() {
        let mut l = line(1.0, 2.0, "no word timings");
        assert!(!reset(&mut l), "nothing changed yet");
        move_edge(&mut l, 0, Edge::End, 1.5, false, 10.0).unwrap();
        move_line(&mut l, 2.0, 10.0);
        assert!(reset(&mut l));
        assert_eq!(l, line(1.0, 2.0, "no word timings"));
    }

    #[test]
    fn text_edits_split_join_and_resplit() {
        let mut l = timed();
        set_word_text(&mut l, 1, "too").unwrap();
        assert_eq!((l.text.as_str(), l.words[1].text.as_str()), ("one too three", "too"));
        set_word_text(&mut l, 2, "th ree").unwrap();
        assert_eq!(l.text, "one too th ree");
        assert_eq!(spans(&l)[2..], [(1.7, 1.82), (1.82, 2.0)], "split by length, inside the old word");
        join_words(&mut l, 2).unwrap();
        assert_eq!((l.text.as_str(), spans(&l)[2]), ("one too three", (1.7, 2.0)));
        assert!(join_words(&mut l, 2).is_err(), "nothing after the last word");
        assert!(set_word_text(&mut l, 0, "  ").is_err());
        // Without word timings only the text changes, and a line break stays.
        let mut p = line(0.0, 1.0, "Tonight's first\nround begins");
        set_word_text(&mut p, 1, "1st").unwrap();
        join_words(&mut p, 2).unwrap();
        assert_eq!((p.text.as_str(), p.words.is_empty()), ("Tonight's 1st\nroundbegins", true));
        let mut r = timed();
        move_edge(&mut r, 0, Edge::End, 1.2, false, 10.0).unwrap();
        resplit(&mut r, Some("one two\nthree four")).unwrap();
        assert_eq!((r.words.is_empty(), r.aligned, r.text.as_str()), (true, false, "one two\nthree four"));
        assert!(resplit(&mut r, Some(" \n ")).is_err());
    }

    #[test]
    fn auto_align_uses_the_onsets_or_falls_back() {
        let o = |at: f64, strength: f32| Onset { at, strength };
        let mut l = line(1.0, 2.0, "one two three");
        let m = auto_align(&mut l, &[o(0.5, 20.0), o(1.02, 20.0), o(1.4, 15.0), o(1.7, 12.0), o(2.5, 20.0)]);
        assert_eq!((m, spans(&l), l.aligned), (Method::Onsets, vec![(1.02, 1.4), (1.4, 1.7), (1.7, 2.0)], false), "never marked hand-aligned");
        let mut l = line(1.0, 2.0, "one two");
        let m = auto_align(&mut l, &[o(1.0, 10.0), o(1.2, 9.0), o(1.5, 25.0)]);
        assert_eq!((m, spans(&l)), (Method::Strongest, vec![(1.0, 1.5), (1.5, 2.0)]));
        let mut l = line(1.0, 2.0, "one two three");
        assert_eq!(auto_align(&mut l, &[o(1.0, 10.0)]), Method::Even);
        assert_eq!(l.words, line(1.0, 2.0, "one two three").even_words());
        assert!(reset(&mut l) && l.words.is_empty());
    }

    fn audio(mono: &[f32], rate: u32) -> DecodedAudio {
        DecodedAudio { sample_rate: rate, data: mono.iter().flat_map(|&v| [v, v]).collect(), art: None, native: None }
    }

    #[test]
    fn onsets_are_where_sound_rises_out_of_quiet() {
        let rate = 48_000;
        // Silence, then three tone bursts at 0.5 s, 1.0 s and 1.6 s (the last one quieter).
        let mut mono = vec![0.0f32; rate as usize * 2];
        for (at, g) in [(0.5, 0.5), (1.0, 0.5), (1.6, 0.2)] {
            let a = (at * f64::from(rate)) as usize;
            for k in 0..(rate as usize / 5) {
                mono[a + k] = g * (k as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin();
            }
        }
        let a = audio(&mono, rate);
        let found: Vec<f64> = onsets(&a).iter().map(|o| (o.at * 1000.0).round()).collect();
        assert_eq!(found.len(), 3, "{found:?}");
        for (f, want) in found.iter().zip([500.0, 1000.0, 1600.0]) {
            assert!((f - want).abs() <= 10.0, "{f} vs {want}");
        }
        assert_eq!(onsets(&a), onsets(&a), "deterministic");
        let part = onsets_in(&a, 0.9, 1.7);
        assert_eq!(part, onsets(&a)[1..].to_vec(), "a stretch finds what the whole file finds there");
        assert!(onsets(&audio(&vec![0.0; 48_000], rate)).is_empty(), "silence has none");
        assert_eq!(nearest_onset(&onsets(&a), 1.03, 0.05).map(|s| (s * 100.0).round()), Some(100.0));
        assert_eq!(nearest_onset(&onsets(&a), 1.3, 0.05), None);
    }

    #[test]
    fn onsets_find_the_words_of_synthesized_speech() {
        // Words of one syllable with the synthesizer's short gaps between them: the
        // hardest case for a finder that looks for quiet before each word.
        for (seed, shape) in [(3, "1 1 1 1 | 1 1 1"), (11, "2 1 1 | 1 2"), (29, "1 1 1 1 1 1")] {
            let (mono, words) = synth::speech_with_words(shape, 120.0, 0.5, 4.0, seed);
            let a = audio(&mono, synth::RATE);
            let found = onsets(&a);
            let hits = words.iter().filter(|&&w| found.iter().any(|o| (o.at - w).abs() <= 0.05)).count();
            assert!(hits * 10 >= words.len() * 8, "seed {seed}: {hits} of {} words found: words {words:?}, onsets {:?}", words.len(), found.iter().map(|o| o.at).collect::<Vec<_>>());
            // Every onset is on a syllable, not in the middle of a vowel or in the quiet.
            assert!(found.len() <= words.len() * 3, "seed {seed}: {} onsets for {} words", found.len(), words.len());
        }
    }

    #[test]
    fn auto_align_on_synthesized_speech_lands_near_the_words() {
        let (mono, words) = synth::speech_with_words("1 1 1 1", 140.0, 0.4, 2.5, 5);
        let a = audio(&mono, synth::RATE);
        let mut l = line(words[0], words[3] + 0.3, "we have a show");
        let m = auto_align(&mut l, &onsets(&a));
        assert_ne!(m, Method::Even, "the four words have four rises");
        for (w, want) in l.words.iter().zip(&words) {
            assert!((w.start - want).abs() <= 0.06, "{} at {} vs {want} ({m:?})", w.text, w.start);
        }
    }

    #[test]
    fn nudging_moves_the_chosen_grip() {
        let mut l = timed();
        nudge(&mut l, 1, Grip::Start, 0.01, 10.0).unwrap();
        assert_eq!(spans(&l)[..2], [(1.0, 1.31), (1.31, 1.6)], "the shared edge moves both");
        nudge(&mut l, 1, Grip::End, 0.1, 10.0).unwrap();
        assert_eq!(spans(&l)[1], (1.31, 1.7), "stops at the next word across the gap");
        nudge(&mut l, 2, Grip::Word, -0.1, 10.0).unwrap();
        assert_eq!(spans(&l)[1..], [(1.31, 1.6), (1.6, 1.9)], "now shared: the neighbour's end follows");
        assert_eq!((Grip::Start.step(true), Grip::Word.step(true), Grip::End.step(true)), (Grip::Word, Grip::End, Grip::Start));
        assert_eq!((Grip::Start.step(false), Grip::End.step(false)), (Grip::End, Grip::Word));
        assert!(nudge(&mut l, 9, Grip::Start, 0.1, 10.0).is_err());
    }

    #[test]
    fn the_line_and_word_at_a_time() {
        let lines = vec![timed(), line(3.0, 4.0, "next line")];
        assert_eq!(at_time(&lines, 1.35), Some((0, Some(1))));
        assert_eq!(at_time(&lines, 1.65), Some((0, None)), "in a gap between words");
        assert_eq!(at_time(&lines, 3.9), Some((1, Some(1))), "a provisional word");
        assert_eq!(at_time(&lines, 2.5), None);
    }
}
