//! Aligning a tile's subtitle words with its audio, as commands (D-267): moving word
//! edges, words and lines, tapping a word in, auto-align, reset and the small text edits.
//! The rules are in src/align.rs; the waveform editor sends these on each drag's end.

use super::{bad, CmdError, Controller, Res, P};
use crate::align::{self, Edge, Grip};
use crate::subtitles::SubtitleLine;
use serde_json::{json, Value};
use std::sync::Arc;

/// The word selected in the editor's subtitle lane: which tile, line and word (from 0),
/// and what the arrow keys move on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EditorWord {
    pub tile: uuid::Uuid,
    pub line: usize,
    pub word: usize,
    pub grip: Grip,
}

/// A 1-based number from `p[k]` (`3` or `"#3"`), checked against `len`, as an index.
fn number(p: &P, k: &str, len: usize, what: &str) -> Result<usize, CmdError> {
    let v = p.get(k).ok_or_else(|| bad(format!("{}: {k} is required (a {what} number, from 1)", p.id)))?;
    let n = v.as_u64().or_else(|| v.as_str().and_then(|s| s.trim().trim_start_matches('#').parse().ok()));
    match n {
        Some(n) if n >= 1 && (n as usize) <= len => Ok(n as usize - 1),
        _ => Err(bad(format!("{}: {k} must be a {what} number from 1 to {len}", p.id))),
    }
}

/// A line as the commands give it back: `line` numbered from 1.
pub fn line_json(tile: uuid::Uuid, n: usize, l: &SubtitleLine) -> Value {
    let words: Vec<Value> = align::words_of(l).iter().map(|w| json!({"start": w.start, "end": w.end, "text": w.text})).collect();
    json!({
        "tile": tile, "line": n + 1, "start": l.start, "end": l.end, "text": l.text, "aligned": l.aligned, "added": l.added,
        "provisional": l.words.is_empty(), "words": words,
    })
}

impl Controller {
    /// `view.editorWord`: select a word in the open editor's subtitle lane, and what
    /// the arrows move on it; `clear` selects none; with nothing, it reports.
    pub(super) fn editor_word_command(&mut self, p: &P) -> Res {
        let (tile, _) = self.editor_tile(p)?;
        let t = self.store.library.tile(tile).expect("open");
        if p.bool("clear")? == Some(true) {
            self.editor_word = None;
        } else if p.has("line") || p.has("word") || p.has("grip") {
            let current = self.editor_word.filter(|w| w.tile == tile);
            if t.subtitles.is_empty() {
                return Err(bad(format!("view.editorWord: “{}” has no subtitles", t.title)));
            }
            let line = match p.get("line") {
                Some(_) => number(p, "line", t.subtitles.len(), "line")?,
                None => current.map(|w| w.line).ok_or_else(|| bad("view.editorWord: give line and word"))?,
            };
            let n = align::words_of(&t.subtitles[line]).len();
            let word = match p.get("word") {
                Some(_) => number(p, "word", n, "word")?,
                None => current.filter(|w| w.line == line).map_or(0, |w| w.word),
            };
            let grip = match p.str("grip")? {
                Some(g) => Grip::parse(g).ok_or_else(|| bad("view.editorWord: grip must be start, end or word"))?,
                None => current.map_or(Grip::Start, |w| w.grip),
            };
            self.editor_word = Some(EditorWord { tile, line, word, grip });
        }
        // A selection left behind by an edit that renumbered or removed words reads as none.
        let sel = self.editor_word.filter(|w| w.tile == tile && t.subtitles.get(w.line).is_some_and(|l| w.word < align::words_of(l).len()));
        Ok(match sel {
            Some(w) => {
                let word = &align::words_of(&t.subtitles[w.line])[w.word];
                json!({"tile": tile, "line": w.line + 1, "word": w.word + 1, "grip": w.grip.name(), "text": word.text, "start": word.start, "end": word.end})
            }
            None => json!({"tile": tile, "selected": null}),
        })
    }

    /// `tile.subtitleAdd` (D-301): a new line, made here, from `start` to `end`, or from
    /// `at` for as long as its words take to say (stopping before the next line). Its
    /// words start where sounds rise inside it (`words: onsets`, the default, as
    /// Auto-Align does) or are spread evenly (`even`); either way they're a suggestion,
    /// not aligned, and the line is marked `added` so a new copy from the source keeps it.
    pub(super) fn subtitle_add(&mut self, p: &P) -> Res {
        let id = "tile.subtitleAdd";
        let tile = self.tile_param(p)?;
        let t = self.store.library.tile(tile).cloned().expect("resolved");
        let fail = |e: String| bad(format!("{id}: {e}"));
        let text = align::line_text(p.need_str("text")?).map_err(fail)?;
        if t.subtitles.len() >= crate::subtitles::MAX_LINES {
            return Err(bad(format!("{id}: a tile has at most {} subtitle lines", crate::subtitles::MAX_LINES)));
        }
        let loaded = self.store.audio(&t);
        let dur = loaded.as_ref().map(|a| a.duration()).or(t.duration).unwrap_or(f64::INFINITY);
        let n_words = text.split_whitespace().count();
        let least = align::MIN_WORD * n_words as f64;
        let (start, end) = match (p.f64("start")?, p.f64("end")?, p.f64("at")?) {
            (Some(a), Some(b), None) => (a.max(0.0), b.min(dur)),
            (None, None, Some(at)) => (at.max(0.0), align::new_line_end(&t.subtitles, at.max(0.0), n_words, dur)),
            _ => return Err(bad(format!("{id}: give start and end (seconds into the file), or at"))),
        };
        if !(start.is_finite() && end.is_finite()) || end - start < least - 1e-9 {
            return Err(bad(format!("{id}: the line needs at least {:.2} s inside the sound (from {start:.2} to {end:.2} s)", least.max(align::MIN_WORD))));
        }
        let even = match p.str("words")? {
            None | Some("onsets") => false,
            Some("even") => true,
            Some(_) => return Err(bad(format!("{id}: words must be onsets or even"))),
        };
        let mut line = SubtitleLine { start, end, text, words: Vec::new(), aligned: false, source_timing: None, added: true, machine: None };
        let mut method = align::Method::Even;
        if !even {
            let audio = match (loaded, &t.file) {
                (Some(a), _) => Some(a),
                (None, Some(f)) => crate::decode::decode_file(&self.store.media_dir.join(f)).ok().map(Arc::new),
                (None, None) => None,
            };
            if let Some(audio) = audio {
                // Only the sounds inside the stretch: the line stays where it was put.
                let found: Vec<_> = align::onsets_in(&audio, start, end).into_iter().filter(|o| o.at >= start).collect();
                method = align::auto_align(&mut line, &found);
            }
        }
        // The even split is the provisional one, a guess drawn dashed; a line made here
        // has no source timing to go back to.
        if method == align::Method::Even {
            line.words.clear();
            line.start = start;
            line.end = end;
        }
        line.aligned = false;
        line.source_timing = None;
        let t = self.store.library.tile_mut(tile).expect("resolved");
        t.subtitles.push(line.clone());
        let last = t.subtitles.len() - 1;
        let n = sort_lines(&mut t.subtitles, last);
        self.store.mark_dirty();
        self.lines_moved(tile, |l| if l >= n { l + 1 } else { l });
        if self.editor == Some(tile) {
            self.editor_word = Some(EditorWord { tile, line: n, word: 0, grip: Grip::Start });
        }
        let mut out = line_json(tile, n, &line);
        merge(&mut out, json!({"method": method.name(), "lines": self.store.library.tile(tile).map_or(0, |t| t.subtitles.len())}));
        Ok(out)
    }

    /// `tile.subtitleDelete`: remove one line (one undo step).
    pub(super) fn subtitle_delete(&mut self, p: &P) -> Res {
        let id = "tile.subtitleDelete";
        let tile = self.tile_param(p)?;
        let t = self.store.library.tile_mut(tile).expect("resolved");
        if t.subtitles.is_empty() {
            return Err(bad(format!("{id}: “{}” has no subtitles", t.title)));
        }
        let li = number(p, "line", t.subtitles.len(), "line")?;
        let gone = t.subtitles.remove(li);
        let left = t.subtitles.len();
        self.store.mark_dirty();
        // The selection follows its word, or goes with the line.
        if let Some(w) = self.editor_word.filter(|w| w.tile == tile) {
            self.editor_word = match w.line.cmp(&li) {
                std::cmp::Ordering::Less => Some(w),
                std::cmp::Ordering::Equal => None,
                std::cmp::Ordering::Greater => Some(EditorWord { line: w.line - 1, ..w }),
            };
        }
        Ok(json!({"tile": tile, "line": li + 1, "removed": {"start": gone.start, "end": gone.end, "text": gone.text, "added": gone.added}, "lines": left}))
    }

    /// After a line was added at `n`: the selection keeps naming its word.
    fn lines_moved(&mut self, tile: uuid::Uuid, f: impl Fn(usize) -> usize) {
        if let Some(w) = self.editor_word.as_mut().filter(|w| w.tile == tile) {
            w.line = f(w.line);
        }
    }

    pub(super) fn subtitle_edit(&mut self, id: &str, p: &P) -> Res {
        let tile = self.tile_param(p)?;
        let t = self.store.library.tile(tile).cloned().expect("resolved");
        if t.subtitles.is_empty() {
            return Err(bad(format!("{id}: “{}” has no subtitles", t.title)));
        }
        let li = number(p, "line", t.subtitles.len(), "line")?;
        let mut line = t.subtitles[li].clone();
        let n_words = align::words_of(&line).len();
        let word = |p: &P| number(p, "word", n_words, "word");
        // The audio's length bounds every move; auto-align needs the audio itself.
        let loaded = self.store.audio(&t);
        let dur = loaded.as_ref().map(|a| a.duration()).or(t.duration).unwrap_or(f64::INFINITY);
        let at = |p: &P, line_start: f64| -> Result<f64, CmdError> {
            match (p.f64("at")?, p.f64("by")?) {
                (Some(at), None) => Ok(at),
                (None, Some(by)) => Ok(line_start + by),
                _ => Err(bad(format!("{}: give at (seconds into the file) or by (seconds)", p.id))),
            }
        };
        let fail = |e: String| bad(format!("{id}: {e}"));
        let split = p.bool("split")?.unwrap_or(false);
        let mut extra = json!({});
        match id {
            "tile.subtitleNudge" => {
                let w = word(p)?;
                let grip = p.need_str("grip")?;
                let grip = Grip::parse(grip).ok_or_else(|| bad(format!("{id}: grip must be start, end or word")))?;
                align::nudge(&mut line, w, grip, p.need_f64("by")?, dur).map_err(fail)?;
            }
            "tile.subtitleEdge" => {
                let w = word(p)?;
                let edge = p.need_str("edge")?;
                let edge = Edge::parse(edge).ok_or_else(|| bad(format!("{id}: edge must be start or end")))?;
                align::move_edge(&mut line, w, edge, p.need_f64("at")?, split, dur).map_err(fail)?;
            }
            "tile.subtitleWordMove" => {
                let w = word(p)?;
                let start = align::words_of(&line)[w].start;
                align::move_word(&mut line, w, at(p, start)?, split, dur).map_err(fail)?;
            }
            "tile.subtitleTap" => {
                let w = word(p)?;
                align::tap(&mut line, w, p.need_f64("at")?, dur).map_err(fail)?;
            }
            "tile.subtitleLineMove" => {
                let from = line.start;
                let to = at(p, from)?;
                align::move_line(&mut line, to - from, dur);
            }
            "tile.subtitleAutoAlign" => {
                // The decoded clip when it's in memory; otherwise (the offline CLI, or a
                // clip still loading) decode it here.
                let audio = match (loaded, &t.file) {
                    (Some(a), _) => a,
                    (None, Some(f)) => Arc::new(crate::decode::decode_file(&self.store.media_dir.join(f)).map_err(|e| bad(format!("{id}: can't read the audio: {e}")))?),
                    (None, None) => return Err(bad(format!("{id}: “{}” has no sound", t.title))),
                };
                let found = align::onsets_in(&audio, (line.start - 0.2).max(0.0), line.end);
                let method = align::auto_align(&mut line, &found);
                extra = json!({"method": method.name()});
            }
            "tile.subtitleReset" => {
                extra = json!({"reset": align::reset(&mut line)});
            }
            "tile.subtitleWordText" => {
                let w = word(p)?;
                align::set_word_text(&mut line, w, p.need_str("text")?).map_err(fail)?;
            }
            "tile.subtitleWordJoin" => {
                let w = word(p)?;
                align::join_words(&mut line, w).map_err(fail)?;
            }
            "tile.subtitleText" => {
                align::resplit(&mut line, p.str("text")?).map_err(fail)?;
            }
            _ => unreachable!("{id}"),
        }
        let t = self.store.library.tile_mut(tile).expect("resolved");
        // A heard line whose text a person retyped is theirs now (D-303): it loses its
        // machine mark, so a later transcription leaves it alone, and counts as added.
        if line.machine.is_some() && line.text != t.subtitles[li].text {
            line.machine = None;
            line.added = true;
        }
        if t.subtitles[li] != line {
            t.subtitles[li] = line.clone();
            let now = sort_lines(&mut t.subtitles, li);
            self.store.mark_dirty();
            let mut out = line_json(tile, now, &line);
            merge(&mut out, extra);
            return Ok(out);
        }
        let mut out = line_json(tile, li, &line);
        merge(&mut out, extra);
        Ok(out)
    }
}

/// Put the lines in time order (a moved or new line may take another number) and say
/// where line `li` went.
fn sort_lines(lines: &mut Vec<SubtitleLine>, li: usize) -> usize {
    let mut order: Vec<usize> = (0..lines.len()).collect();
    order.sort_by(|&a, &b| lines[a].start.total_cmp(&lines[b].start).then(lines[a].end.total_cmp(&lines[b].end)).then(a.cmp(&b)));
    let now = order.iter().position(|&i| i == li).unwrap_or(li);
    *lines = order.into_iter().map(|i| lines[i].clone()).collect();
    now
}

fn merge(out: &mut Value, extra: Value) {
    if let (Some(o), Value::Object(e)) = (out.as_object_mut(), extra) {
        o.extend(e);
    }
}

#[cfg(test)]
mod tests {
    use crate::audio::Engine;
    use crate::controller::Controller;
    use crate::store::Store;
    use serde_json::{json, Value};
    use std::time::{Duration, Instant};

    /// A controller with one tile, "Talk", holding four seconds of synthesized speech and
    /// two subtitle lines, the first without word timings.
    fn talk() -> (Controller, Vec<f64>) {
        let dir = std::env::temp_dir().join(format!("sb-align-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut c = Controller::new(Store::open(dir.join("data")).unwrap(), Engine::disabled("test"));
        let (mono, words) = crate::demo::synth::speech_with_words("1 1 1 1 | 1 1", 130.0, 0.4, 4.0, 5);
        let wav = dir.join("Talk.wav");
        crate::wav::write_mono(&wav, crate::demo::synth::RATE, &mono).unwrap();
        c.execute("tile.add", json!({"paths": [wav]})).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while c.store.is_loading() && Instant::now() < deadline {
            c.store.poll();
            std::thread::sleep(Duration::from_millis(10));
        }
        c.tick(0.0);
        let cues = json!([
            {"start": words[0] - 0.05, "end": words[3] + 0.3, "text": "we have a show"},
            {"start": words[4], "end": words[5] + 0.4, "text": "go on", "words": [
                {"start": words[4], "end": words[5], "text": "go"}, {"start": words[5], "end": words[5] + 0.4, "text": "on"}]}
        ]);
        c.execute("tile.subtitlesSet", json!({"tile": "Talk", "cues": cues, "cues_source": {"kind": "srt"}})).unwrap();
        (c, words)
    }

    fn line(c: &mut Controller, n: usize) -> Value {
        c.execute("tile.subtitles", json!({"tile": "Talk"})).unwrap()["cues"][n - 1].clone()
    }

    /// D-303: retyping a heard line's text makes it the person's own line.
    #[test]
    fn a_retyped_heard_line_is_no_longer_machine_made() {
        let (mut c, _) = talk();
        let cues = json!([{"start": 0.2, "end": 1.0, "text": "herd wrong", "machine": true}]);
        c.execute("tile.subtitlesSet", json!({"tile": "Talk", "cues": cues, "keepAligned": false})).unwrap();
        assert_eq!(line(&mut c, 1)["machine"]["kind"], "machine");
        let r = c.execute("tile.subtitleText", json!({"tile": "Talk", "line": 1, "text": "heard right"})).unwrap();
        assert_eq!(r["added"], true);
        let l = line(&mut c, 1);
        assert!(l.get("machine").is_none() && l["added"] == true && l["text"] == "heard right", "{l}");
    }

    #[test]
    fn edits_are_json_in_and_out_and_one_undo_step_each() {
        let (mut c, words) = talk();
        let r = c.execute("tile.subtitleEdge", json!({"tile": "Talk", "line": 1, "word": 2, "edge": "start", "at": words[1]})).unwrap();
        assert_eq!((r["line"].as_u64(), r["aligned"].as_bool(), r["provisional"].as_bool()), (Some(1), Some(true), Some(false)));
        assert_eq!(r["words"][1]["start"].as_f64(), Some(words[1]));
        assert_eq!(r["words"][0]["end"].as_f64(), Some(words[1]), "the shared boundary moved both");
        assert_eq!(line(&mut c, 1)["aligned"], true, "stored");
        c.execute("edit.undo", json!({})).unwrap();
        assert!(line(&mut c, 1).get("words").is_none(), "one undo step puts the provisional line back");

        let r = c.execute("tile.subtitleWordMove", json!({"tile": "Talk", "line": "#2", "word": 1, "by": -0.05, "split": true})).unwrap();
        assert!((r["words"][0]["start"].as_f64().unwrap() - (words[4] - 0.05)).abs() < 1e-9);
        assert!((r["words"][0]["end"].as_f64().unwrap() - (words[5] - 0.05)).abs() < 1e-9, "split: a gap opens to the next word");
        assert_eq!(r["words"][1]["start"].as_f64(), Some(words[5]));
        let r = c.execute("tile.subtitleTap", json!({"tile": "Talk", "line": 2, "word": 2, "at": words[5] + 0.02})).unwrap();
        assert_eq!(r["words"][1]["start"].as_f64(), Some(words[5] + 0.02));

        // Moving the second line before the first renumbers it.
        let r = c.execute("tile.subtitleLineMove", json!({"tile": "Talk", "line": 2, "at": 0.0})).unwrap();
        assert_eq!((r["line"].as_u64(), r["start"].as_f64()), (Some(1), Some(0.0)));
        assert_eq!(line(&mut c, 2)["text"], "we have a show");
        let r = c.execute("tile.subtitleReset", json!({"tile": "Talk", "line": 1})).unwrap();
        assert_eq!((r["reset"].as_bool(), r["aligned"].as_bool(), r["start"].as_f64()), (Some(true), Some(false), Some(words[4])));
        assert_eq!(r["line"], 2, "back in its place");
        assert_eq!(c.execute("tile.subtitleReset", json!({"tile": "Talk", "line": 2})).unwrap()["reset"], false);

        let r = c.execute("tile.subtitleWordText", json!({"tile": "Talk", "line": 1, "word": 4, "text": "sh ow"})).unwrap();
        assert_eq!(r["text"], "we have a sh ow");
        let r = c.execute("tile.subtitleWordJoin", json!({"tile": "Talk", "line": 1, "word": 4})).unwrap();
        assert_eq!(r["text"], "we have a show");
        let r = c.execute("tile.subtitleText", json!({"tile": "Talk", "line": 1, "text": "we've got\na show"})).unwrap();
        assert_eq!((r["text"].as_str(), r["provisional"].as_bool(), r["words"].as_array().map(Vec::len)), (Some("we've got\na show"), Some(true), Some(4)));
    }

    #[test]
    fn the_editor_selects_a_word_and_nudges_its_grip() {
        let (mut c, words) = talk();
        assert_eq!(c.execute("view.editorWord", json!({"line": 1, "word": 1})).unwrap_err().code, "bad_params", "the editor isn't open");
        c.execute("view.editor", json!({"tile": "Talk"})).unwrap();
        let r = c.execute("view.editorWord", json!({"line": 2, "word": 2})).unwrap();
        assert_eq!((r["line"].as_u64(), r["word"].as_u64(), r["grip"].as_str(), r["text"].as_str()), (Some(2), Some(2), Some("start"), Some("on")));
        assert_eq!(c.execute("view.editorWord", json!({"grip": "end"})).unwrap()["grip"], "end", "the grip changes, the word stays");
        assert_eq!(c.execute("view.editorWord", json!({})).unwrap()["word"], 2);
        assert_eq!(c.execute("view.editorWord", json!({"grip": "middle"})).unwrap_err().code, "bad_params");
        let r = c.execute("tile.subtitleNudge", json!({"tile": "Talk", "line": 2, "word": 2, "grip": "end", "by": 0.1})).unwrap();
        assert!((r["words"][1]["end"].as_f64().unwrap() - (words[5] + 0.5)).abs() < 1e-9);
        let r = c.execute("tile.subtitleNudge", json!({"tile": "Talk", "line": 2, "word": 2, "grip": "start", "by": -0.01})).unwrap();
        assert!((r["words"][0]["end"].as_f64().unwrap() - (words[5] - 0.01)).abs() < 1e-9, "a shared edge moves both");
        c.execute("edit.undo", json!({})).unwrap();
        assert_eq!(line(&mut c, 2)["words"][1]["start"].as_f64(), Some(words[5]), "one nudge, one undo step");
        assert_eq!(c.execute("view.editorWord", json!({"clear": true})).unwrap()["selected"], Value::Null);
        c.execute("view.editor", json!({"open": false})).unwrap();
        assert!(c.editor_word.is_none(), "closing the editor forgets it");
    }

    #[test]
    fn auto_align_suggests_and_bad_params_are_refused() {
        let (mut c, words) = talk();
        let r = c.execute("tile.subtitleAutoAlign", json!({"tile": "Talk", "line": 1})).unwrap();
        assert_ne!(r["method"], "even", "{r}");
        assert_eq!(r["aligned"], false, "a suggestion, never hand-aligned");
        for (w, want) in r["words"].as_array().unwrap().iter().zip(&words) {
            assert!((w["start"].as_f64().unwrap() - want).abs() < 0.06, "{w} vs {want}");
        }
        for bad in [
            json!({"tile": "Talk", "line": 3}),
            json!({"tile": "Talk", "line": 0}),
            json!({"tile": "Talk"}),
            json!({"tile": "Talk", "line": 1, "word": 9, "edge": "start", "at": 1.0}),
            json!({"tile": "Talk", "line": 1, "word": 1, "edge": "middle", "at": 1.0}),
            json!({"tile": "Talk", "line": 1, "word": 1, "edge": "start"}),
        ] {
            let id = if bad.get("edge").is_some() || bad.get("word").is_some() { "tile.subtitleEdge" } else { "tile.subtitleReset" };
            assert_eq!(c.execute(id, bad.clone()).unwrap_err().code, "bad_params", "{id} {bad}");
        }
        assert_eq!(c.execute("tile.subtitleWordMove", json!({"tile": "Talk", "line": 1, "word": 1})).unwrap_err().code, "bad_params", "at or by");
        c.execute("tile.subtitlesClear", json!({"tile": "Talk"})).unwrap();
        assert_eq!(c.execute("tile.subtitleReset", json!({"tile": "Talk", "line": 1})).unwrap_err().code, "bad_params");
    }

    #[test]
    fn a_locked_set_refuses_every_edit() {
        let (mut c, _) = talk();
        c.execute("set.lock", json!({})).unwrap();
        for (id, p) in [
            ("tile.subtitleEdge", json!({"line": 1, "word": 1, "edge": "end", "at": 1.0})),
            ("tile.subtitleNudge", json!({"line": 1, "word": 1, "grip": "word", "by": 0.01})),
            ("tile.subtitleWordMove", json!({"line": 1, "word": 1, "by": 0.1})),
            ("tile.subtitleTap", json!({"line": 1, "word": 1, "at": 1.0})),
            ("tile.subtitleLineMove", json!({"line": 1, "by": 0.1})),
            ("tile.subtitleAutoAlign", json!({"line": 1})),
            ("tile.subtitleReset", json!({"line": 1})),
            ("tile.subtitleWordText", json!({"line": 1, "word": 1, "text": "x"})),
            ("tile.subtitleWordJoin", json!({"line": 1, "word": 1})),
            ("tile.subtitleText", json!({"line": 1})),
        ] {
            let mut p = p;
            p["tile"] = json!("Talk");
            assert_eq!(c.execute(id, p).unwrap_err().code, "locked", "{id}");
        }
    }

    /// D-301: a line added here, at a time or over a stretch, its words on the sounds or
    /// spread evenly; retyped, deleted, each one undo step; a new copy from the source
    /// keeps it, and a locked set refuses both.
    #[test]
    fn lines_are_added_retyped_and_deleted() {
        let (mut c, words) = talk();
        c.execute("view.editor", json!({"tile": "Talk"})).unwrap();
        let end = words[3] + 0.3;
        let r = c.execute("tile.subtitleAdd", json!({"tile": "Talk", "text": "here  we\ngo", "start": end + 0.05, "end": words[4] - 0.02, "words": "even"})).unwrap();
        assert_eq!((r["line"].as_u64(), r["text"].as_str(), r["added"].as_bool(), r["aligned"].as_bool(), r["provisional"].as_bool()), (Some(2), Some("here we\ngo"), Some(true), Some(false), Some(true)), "{r}");
        assert_eq!((r["method"].as_str(), r["lines"].as_u64()), (Some("even"), Some(3)));
        assert_eq!(c.execute("view.editorWord", json!({})).unwrap()["line"], 2, "the new line is selected");
        c.execute("edit.undo", json!({})).unwrap();
        assert_eq!(c.execute("tile.subtitles", json!({"tile": "Talk"})).unwrap()["cues"].as_array().unwrap().len(), 2, "one undo step");
        c.execute("edit.redo", json!({})).unwrap();

        // At a time: it lasts as its words take to say, stopping before the next line;
        // its words start on the sounds.
        let r = c.execute("tile.subtitleAdd", json!({"tile": "Talk", "text": "we have a show", "at": words[0] - 0.1})).unwrap();
        let cues = c.execute("tile.subtitles", json!({"tile": "Talk"})).unwrap()["cues"].clone();
        let starts: Vec<f64> = cues.as_array().unwrap().iter().map(|l| l["start"].as_f64().unwrap()).collect();
        assert!(starts.windows(2).all(|w| w[0] <= w[1]), "lines stay in time order: {starts:?}");
        assert_eq!(cues[r["line"].as_u64().unwrap() as usize - 1]["text"], "we have a show");
        assert!(r["start"].as_f64().unwrap() >= words[0] - 0.1 - 1e-9, "inside where it was put: {r}");
        assert_ne!(r["method"], "even", "{r}");
        assert_eq!(r["provisional"], false);
        assert!(r["end"].as_f64().unwrap() <= words[0] - 0.1 + crate::align::spoken_length(4) + 0.2, "{r}");
        let r = c.execute("tile.subtitleAdd", json!({"tile": "Talk", "text": "a b c d e f g h", "at": end + 0.01})).unwrap();
        assert!(r["end"].as_f64().unwrap() <= words[4] + 1e-9, "stops before the next line with room for its words (not the one 40 ms on): {r}");
        let r = c.execute("tile.subtitleAdd", json!({"tile": "Talk", "text": "x", "at": end + 0.01, "words": "even"})).unwrap();
        assert!((r["end"].as_f64().unwrap() - (end + 0.05)).abs() < 1e-9, "one word fits in 40 ms: {r}");
        c.execute("edit.undo", json!({})).unwrap();

        // Retyping it keeps it added.
        let r = c.execute("tile.subtitleText", json!({"tile": "Talk", "line": 4, "text": "here we go again"})).unwrap();
        assert_eq!((r["text"].as_str(), r["added"].as_bool()), (Some("here we go again"), Some(true)));
        let r = c.execute("tile.subtitleWordText", json!({"tile": "Talk", "line": 4, "word": 4, "text": "now"})).unwrap();
        assert_eq!(r["text"], "here we go now");

        // QuipClipper sends the clip's two lines again: the added ones stay.
        let again = json!([
            {"start": words[0] - 0.05, "end": words[3] + 0.3, "text": "we have a show"},
            {"start": words[4], "end": words[5] + 0.4, "text": "go on"}
        ]);
        let r = c.execute("tile.subtitlesSet", json!({"tile": "Talk", "cues": again, "cues_source": {"kind": "srt"}})).unwrap();
        let cues = c.execute("tile.subtitles", json!({"tile": "Talk"})).unwrap()["cues"].clone();
        let texts: Vec<&str> = cues.as_array().unwrap().iter().map(|l| l["text"].as_str().unwrap()).collect();
        assert_eq!(texts, ["we have a show", "a b c d e f g h", "here we go now", "go on"], "the added line with the source's text stood in for it: {cues}");
        assert_eq!((r["lines"].as_u64(), r["kept_aligned"].as_u64()), (Some(4), Some(3)));
        assert_eq!(cues[0]["added"], true);

        // Delete: the selection goes with the line; one undo step.
        c.execute("view.editorWord", json!({"line": 4, "word": 1})).unwrap();
        let r = c.execute("tile.subtitleDelete", json!({"tile": "Talk", "line": 3})).unwrap();
        assert_eq!((r["removed"]["text"].as_str(), r["removed"]["added"].as_bool(), r["lines"].as_u64()), (Some("here we go now"), Some(true), Some(3)));
        assert_eq!(c.execute("view.editorWord", json!({})).unwrap()["line"], 3, "the selected word follows its line");
        c.execute("tile.subtitleDelete", json!({"tile": "Talk", "line": 3})).unwrap();
        assert_eq!(c.execute("view.editorWord", json!({})).unwrap()["selected"], Value::Null, "or goes with it");
        c.execute("edit.undo", json!({})).unwrap();
        assert_eq!(line(&mut c, 3)["text"], "go on", "one undo step");

        for bad in [
            json!({"tile": "Talk", "text": "x"}),
            json!({"tile": "Talk", "text": "  ", "at": 1.0}),
            json!({"tile": "Talk", "text": "x", "start": 1.0}),
            json!({"tile": "Talk", "text": "x", "start": 1.0, "end": 1.0}),
            json!({"tile": "Talk", "text": "x", "at": 100.0}),
            json!({"tile": "Talk", "text": "x", "at": 1.0, "words": "sideways"}),
        ] {
            assert_eq!(c.execute("tile.subtitleAdd", bad.clone()).unwrap_err().code, "bad_params", "{bad}");
        }
        assert_eq!(c.execute("tile.subtitleDelete", json!({"tile": "Talk", "line": 9})).unwrap_err().code, "bad_params");
        c.execute("set.lock", json!({})).unwrap();
        assert_eq!(c.execute("tile.subtitleAdd", json!({"tile": "Talk", "text": "x", "at": 0.1})).unwrap_err().code, "locked");
        assert_eq!(c.execute("tile.subtitleDelete", json!({"tile": "Talk", "line": 1})).unwrap_err().code, "locked");
    }

    /// The first line of a tile without subtitles can be added too.
    #[test]
    fn a_tile_without_subtitles_gets_its_first_line() {
        let (mut c, _) = talk();
        c.execute("tile.subtitlesClear", json!({"tile": "Talk"})).unwrap();
        let r = c.execute("tile.subtitleAdd", json!({"tile": "Talk", "text": "hello", "start": 0.0, "end": 0.5})).unwrap();
        assert_eq!((r["line"].as_u64(), r["lines"].as_u64()), (Some(1), Some(1)));
    }

    /// The words placed here survive a set file's export and import, and QuipClipper
    /// sending the same clip's subtitles again.
    #[test]
    fn aligned_words_survive_a_set_file_and_a_new_copy_from_the_source() {
        let (mut c, words) = talk();
        c.execute("tile.subtitleEdge", json!({"tile": "Talk", "line": 1, "word": 2, "edge": "start", "at": words[1]})).unwrap();
        c.execute("tile.subtitleEdge", json!({"tile": "Talk", "line": 2, "word": 1, "edge": "end", "at": words[5] - 0.01, "split": true})).unwrap();
        let before = c.execute("tile.subtitles", json!({"tile": "Talk"})).unwrap()["cues"].clone();

        let file = std::env::temp_dir().join(format!("sb-align-{}.lsset", uuid::Uuid::new_v4()));
        c.execute("set.export", json!({"path": file})).unwrap();
        let r = c.execute("set.import", json!({"path": file})).unwrap();
        let imported = r["sets"][0]["name"].as_str().map(str::to_owned).unwrap_or_default();
        let copy = c.execute("tile.subtitles", json!({"tile": format!("{imported}/Talk")})).unwrap()["cues"].clone();
        assert!(close(&copy, &before), "a set file keeps the words, aligned and the source's timing: {copy} vs {before}");

        // QuipClipper sends the clip's lines again, without word timings.
        let again = json!([
            {"start": words[0] - 0.05, "end": words[3] + 0.3, "text": "We have a show"},
            {"start": words[4], "end": words[5] + 0.4, "text": "go on"}
        ]);
        let r = c.execute("tile.subtitlesSet", json!({"tile": "Talk", "cues": again, "cues_source": {"kind": "srt"}})).unwrap();
        assert_eq!(r["kept_aligned"], 2);
        let now = c.execute("tile.subtitles", json!({"tile": "Talk"})).unwrap()["cues"].clone();
        assert!(close(&now, &before), "{now} vs {before}");
    }

    /// Equal but for the last digits of numbers (JSON text doesn't always read back a
    /// float's last bit).
    fn close(a: &Value, b: &Value) -> bool {
        match (a, b) {
            (Value::Number(x), Value::Number(y)) => (x.as_f64().unwrap() - y.as_f64().unwrap()).abs() < 1e-9,
            (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(x, y)| close(x, y)),
            (Value::Object(x), Value::Object(y)) => x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| close(v, w))),
            _ => a == b,
        }
    }
}
