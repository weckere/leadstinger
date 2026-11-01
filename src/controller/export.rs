//! Rendered export: `tile.render`, `set.render`, `render.status` and `render.cancel`
//! (D-077, D-080). The commands only gather what to render and start a
//! [`Job`]; the rendering and writing happen on its thread (src/render.rs).

use super::{bad, ok, CmdError, Controller, Effect, Res, P};
use crate::model::Tile;
use crate::render::{folder_names, with_extension, Format, Item, Job, Source, RATE_MAX, RATE_MIN};
use crate::store::Media;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Finished jobs kept for `render.status`.
const KEEP_FINISHED: usize = 16;

fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn plural(n: usize, one: &str) -> String {
    format!("{n} {one}{}", if n == 1 { "" } else { "s" })
}

impl Controller {
    pub(super) fn render_command(&mut self, id: &str, p: &P) -> Res {
        match id {
            "tile.render" => {
                let ids = match (p.get("tiles"), p.str("tile")?) {
                    (Some(list), _) => self.tile_refs(list, id)?,
                    (None, Some(r)) => vec![self.resolve_tile(r)?],
                    (None, None) => self.selected(),
                };
                let (format, rate) = (render_format(p)?, render_rate(p)?);
                let Some(path) = p.str("path")? else {
                    self.needs_window(id, "a path")?;
                    self.effects.push(Effect::ExportAudio { tiles: ids, set: self.current_set().id });
                    return ok();
                };
                if ids.is_empty() {
                    return Err(bad("tile.render: which tile? Give tile (or tiles), or select one"));
                }
                let path = PathBuf::from(path);
                if ids.len() == 1 && !path.is_dir() {
                    let format = format.or_else(|| Format::of_path(&path)).unwrap_or(Format::Wav);
                    let file = with_extension(&path, format);
                    if file.parent().is_some_and(|d| !d.as_os_str().is_empty() && !d.is_dir()) {
                        return Err(CmdError::new("failed", format!("tile.render: there's no folder {}", file.parent().unwrap_or(&file).display())));
                    }
                    self.start_render(vec![(ids[0], file)], format, rate)
                } else {
                    self.render_into(&ids, &path, format.unwrap_or(Format::Wav), rate)
                }
            }
            "set.render" => {
                let si = self.set_param(p)?;
                let set = &self.store.library.sets[si];
                let (set_id, ids): (Uuid, Vec<Uuid>) = (set.id, set.tiles.iter().map(|t| t.id).collect());
                let (format, rate) = (render_format(p)?, render_rate(p)?);
                let Some(folder) = p.str("folder")?.or(p.str("path")?) else {
                    self.needs_window(id, "a folder")?;
                    self.effects.push(Effect::ExportAudio { tiles: Vec::new(), set: set_id });
                    return ok();
                };
                self.render_into(&ids, Path::new(folder), format.unwrap_or(Format::Wav), rate)
            }
            "render.status" => {
                let jobs: Vec<&Job> = match p.f64("job")? {
                    Some(j) => vec![self.renders.iter().find(|r| f64::from(r.id) == j).ok_or_else(|| CmdError::new("not_found", format!("render.status: no render job {j}")))?],
                    None => self.renders.iter().collect(),
                };
                Ok(json!({"renders": jobs.iter().map(|j| job_json(j)).collect::<Vec<_>>()}))
            }
            "render.cancel" => {
                let job = p.f64("job")?;
                let mut n = 0;
                for r in self.renders.iter().filter(|r| !r.is_finished() && job.is_none_or(|j| f64::from(r.id) == j)) {
                    r.cancel();
                    n += 1;
                }
                Ok(json!({"cancelled": n}))
            }
            _ => unreachable!("{id}"),
        }
    }

    /// Every tile of `ids` that has audio, into `folder` (made if missing) as
    /// `NN Title.ext`, never over an existing file.
    fn render_into(&mut self, ids: &[Uuid], folder: &Path, format: Format, rate: Option<u32>) -> Res {
        if folder.exists() && !folder.is_dir() {
            return Err(CmdError::new("failed", format!("{} is a file, not a folder", folder.display())));
        }
        std::fs::create_dir_all(folder).map_err(|e| CmdError::new("failed", format!("Couldn't make the folder {}: {e}", folder.display())))?;
        let lib = &self.store.library;
        let tiles: Vec<(usize, &str)> =
            ids.iter().map(|&id| (lib.locate(id).map_or(0, |(_, ti)| ti + 1), lib.tile(id).map_or("", |t| t.title.as_str()))).collect();
        let names = folder_names(folder, &tiles, format);
        self.start_render(ids.iter().copied().zip(names).collect(), format, rate)
    }

    /// Start a job for these tiles and files. Tiles with nothing to render are skipped
    /// (and listed); the command fails when that leaves nothing.
    fn start_render(&mut self, wanted: Vec<(Uuid, PathBuf)>, format: Format, rate: Option<u32>) -> Res {
        let mut items = Vec::new();
        let mut skipped = Vec::new();
        for (id, path) in wanted {
            let Some(tile) = self.store.library.tile(id).cloned() else { continue };
            match self.render_source(&tile) {
                Ok(source) => items.push(Item { tile, source, path }),
                Err(why) => skipped.push(json!({"tile": id.to_string(), "title": tile.title, "reason": why})),
            }
        }
        if items.is_empty() {
            let why = match skipped.as_slice() {
                [one] => format!("“{}”: {}", one["title"].as_str().unwrap_or_default(), one["reason"].as_str().unwrap_or_default()),
                _ => "none of the tiles has audio".to_owned(),
            };
            return Err(CmdError::new("no_audio", format!("Nothing to export: {why}")));
        }
        let id = self.next_render;
        self.next_render += 1;
        let job = Job::start(id, items, format, rate);
        let out = json!({"job": id, "format": format.extension(), "files": job.files, "skipped": skipped});
        self.notify(match job.files.as_slice() {
            [one] => format!("Exporting “{}”…", file_name(one)),
            many => format!("Exporting {}…", plural(many.len(), "file")),
        });
        // Forget the oldest finished jobs.
        while self.renders.iter().filter(|r| r.is_finished() && r.reported).count() >= KEEP_FINISHED {
            let Some(i) = self.renders.iter().position(|r| r.is_finished() && r.reported) else { break };
            self.renders.remove(i);
        }
        self.renders.push(job);
        Ok(out)
    }

    /// Where a tile's audio comes from for a render, or why it has none: the copy the
    /// app plays when it has it, else the decoded clip (transposed on the job's thread),
    /// else the file (the offline CLI decodes nothing up front).
    fn render_source(&self, tile: &Tile) -> Result<Source, String> {
        if tile.action.is_some() {
            return Err("an action tile has no audio".into());
        }
        let Some(file) = &tile.file else { return Err("no audio".into()) };
        if self.store.import_of(tile).is_some() {
            return Err("still being imported".into());
        }
        match self.store.playable(tile) {
            Ok(audio) => Ok(Source::Playable(audio)),
            Err(true) => self.store.audio(tile).map(Source::Clip).ok_or_else(|| "not decoded".into()),
            Err(false) => match self.store.media(tile) {
                Some(Media::Failed(e)) => Err(format!("can't be read ({e})")),
                _ => Ok(Source::File(self.store.media_dir.join(file))),
            },
        }
    }

    /// Tell the user about renders that finished since the last tick.
    pub(super) fn poll_renders(&mut self) {
        let mut notes = Vec::new();
        for r in self.renders.iter_mut().filter(|r| r.is_finished() && !r.reported) {
            r.reported = true;
            let out = r.outcome();
            let folder = out.written.first().and_then(|f| f.parent()).map(file_name).unwrap_or_default();
            let text = match (out.written.as_slice(), out.errors.first()) {
                (_, Some(e)) => (format!("Couldn't export {e}"), true),
                ([], None) => ("Export cancelled".to_owned(), false),
                ([one], None) => (format!("Exported “{}”", file_name(one)), false),
                (many, None) => (format!("Exported {} to {folder}", plural(many.len(), "file")), false),
            };
            notes.push(text);
        }
        for (text, error) in notes {
            if error {
                self.notify_error(text);
            } else {
                self.notify(text);
            }
        }
    }

    /// For quitting: stop every render and wait for its thread to clean up.
    pub fn stop_renders(&mut self) {
        for r in &self.renders {
            r.cancel();
        }
        for r in &self.renders {
            r.wait();
        }
    }

    /// Renders still running.
    pub fn rendering(&self) -> bool {
        self.renders.iter().any(|r| !r.is_finished())
    }

    /// "Exporting “Intro” 34% (2 of 5)" while a render runs, for the status bar.
    pub fn render_summary(&self) -> Option<String> {
        let r = self.renders.iter().find(|r| !r.is_finished())?;
        let pct = (r.fraction() * 100.0) as u32;
        let of = if r.files.len() > 1 { format!(" ({} of {})", (r.done() + 1).min(r.files.len()), r.files.len()) } else { String::new() };
        let more = self.renders.iter().filter(|r| !r.is_finished()).count() - 1;
        let more = if more > 0 { format!(" +{more}") } else { String::new() };
        Some(format!("Exporting “{}” {pct}%{of}{more}", r.current()))
    }
}

fn render_format(p: &P) -> Result<Option<Format>, CmdError> {
    p.str("format")?.map(|f| Format::parse(f).ok_or_else(|| bad(format!("{}: format must be wav or flac (got {f:?})", p.id)))).transpose()
}

fn render_rate(p: &P) -> Result<Option<u32>, CmdError> {
    match p.f64("rate")? {
        None => Ok(None),
        Some(r) if r.fract() == 0.0 && (f64::from(RATE_MIN)..=f64::from(RATE_MAX)).contains(&r) => Ok(Some(r as u32)),
        Some(r) => Err(bad(format!("{}: rate must be a whole number of Hz, {RATE_MIN}-{RATE_MAX} (got {r})", p.id))),
    }
}

fn job_json(r: &Job) -> Value {
    let out = r.outcome();
    json!({
        "job": r.id,
        "format": r.format.extension(),
        "files": r.files,
        "done": r.done(),
        "total": r.files.len(),
        "fraction": r.fraction(),
        "current": r.current(),
        "finished": r.is_finished(),
        "cancelled": out.cancelled,
        "written": out.written,
        "errors": out.errors,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::Engine;
    use crate::render::render_tile;
    use crate::store::Store;
    use std::time::{Duration, Instant};

    fn controller() -> (Controller, PathBuf) {
        let root = std::env::temp_dir().join(format!("sb-render-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let store = Store::open(root.join("data")).unwrap();
        (Controller::new(store, Engine::disabled("test")), root)
    }

    /// Tiles playing 0.5 s tones (8 kHz), decoded.
    fn add(c: &mut Controller, root: &Path, names: &[&str]) -> Vec<Uuid> {
        let paths: Vec<PathBuf> = (0..names.len())
            .map(|i| {
                let p = root.join(format!("in-{i}.wav"));
                crate::wav::write_tone(&p, 8000, 0.5, &[330.0 + 110.0 * i as f32], 0.4).unwrap();
                p
            })
            .collect();
        let r = c.execute("tile.add", json!({"paths": paths})).unwrap();
        let ids: Vec<Uuid> = serde_json::from_value(r["added"].clone()).unwrap();
        for (id, n) in ids.iter().zip(names) {
            c.execute("tile.set", json!({"tile": id.to_string(), "title": n})).unwrap();
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        while c.store.is_loading() && Instant::now() < deadline {
            c.store.poll();
            std::thread::sleep(Duration::from_millis(10));
        }
        ids
    }

    /// Wait for the job a render command started, and tell the controller.
    fn finish(c: &mut Controller, started: &Value) -> Value {
        let job = started["job"].as_u64().unwrap();
        c.renders.iter().find(|r| u64::from(r.id) == job).unwrap().wait();
        c.tick(0.0);
        c.execute("render.status", json!({"job": job})).unwrap()["renders"][0].clone()
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        v.sort();
        v
    }

    #[test]
    fn tile_render_writes_what_the_tile_plays() {
        let (mut c, root) = controller();
        let ids = add(&mut c, &root, &["Voice"]);
        c.execute("tile.set", json!({"tile": "Voice", "trimStart": 0.1, "fadeIn": 0.05, "fadeOut": 0.1, "gainDb": 3, "speed": 1.2, "pitch": -2, "loop": true})).unwrap();
        let before = serde_json::to_value(&c.store.library).unwrap();
        // Straight away: the app's transposed copy may not be ready, so the job makes its own.
        let started = c.execute("tile.render", json!({"tile": "Voice", "path": root.join("out")})).unwrap();
        assert_eq!(started["files"], json!([root.join("out.wav")]), "the extension comes from the format");
        let status = finish(&mut c, &started);
        assert_eq!(status["written"], json!([root.join("out.wav")]));
        assert_eq!(status["finished"], true);
        assert_eq!(c.notice.as_ref().unwrap().text, "Exported “out.wav”");

        // The same as the live mixer playing the app's own copy, to 24-bit rounding.
        let tile = c.store.library.tile(ids[0]).unwrap().clone();
        let deadline = Instant::now() + Duration::from_secs(10);
        while c.store.playable(&tile).is_err() && Instant::now() < deadline {
            c.store.poll();
            std::thread::sleep(Duration::from_millis(10));
        }
        let want = render_tile(&tile, c.store.playable(&tile).unwrap(), 8000, |_| true).unwrap();
        let got = crate::decode::decode_file(&root.join("out.wav")).unwrap();
        assert_eq!((got.sample_rate, got.data.len()), (8000, want.len()));
        let worst = got.data.iter().zip(&want).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
        assert!(worst <= 2.0 / 8_388_607.0, "{worst}");

        // FLAC at another rate, by extension; the library never changed.
        let started = c.execute("tile.render", json!({"tile": "Voice", "path": root.join("out.flac"), "rate": 16000})).unwrap();
        assert_eq!(started["format"], "flac");
        finish(&mut c, &started);
        assert_eq!(crate::decode::decode_file(&root.join("out.flac")).unwrap().sample_rate, 16000);
        assert_eq!(serde_json::to_value(&c.store.library).unwrap(), before);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn set_render_names_files_safely_and_uniquely() {
        let (mut c, root) = controller();
        add(&mut c, &root, &["Intro", "Intro", "a/b: c?"]);
        c.execute("tile.addAction", json!({"kind": "stop", "title": "Stop it"})).unwrap();
        let out = root.join("export");
        let started = c.execute("set.render", json!({"folder": out, "format": "flac"})).unwrap();
        assert_eq!(started["skipped"][0]["title"], "Stop it");
        finish(&mut c, &started);
        assert_eq!(names(&out), ["01 Intro.flac", "02 Intro.flac", "03 a-b- c-.flac"]);
        assert_eq!(c.notice.as_ref().unwrap().text, "Exported 3 files to export");
        // Again: nothing is overwritten.
        let again = c.execute("set.render", json!({"folder": out, "format": "flac"})).unwrap();
        finish(&mut c, &again);
        assert_eq!(names(&out), ["01 Intro (2).flac", "01 Intro.flac", "02 Intro (2).flac", "02 Intro.flac", "03 a-b- c- (2).flac", "03 a-b- c-.flac"]);
        // Several tiles to tile.render go into a folder the same way.
        let some = c.execute("tile.render", json!({"tiles": ["#1", "#3"], "path": root.join("two")})).unwrap();
        finish(&mut c, &some);
        assert_eq!(names(&root.join("two")), ["01 Intro.wav", "03 a-b- c-.wav"]);
        // Only the action tile: nothing to do.
        assert_eq!(c.execute("tile.render", json!({"tile": "Stop it", "path": root.join("x.wav")})).unwrap_err().code, "no_audio");
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn asks_the_window_and_checks_its_parameters() {
        let (mut c, root) = controller();
        let ids = add(&mut c, &root, &["A", "B"]);
        let set = c.current_set().id;
        c.execute("view.select", json!({"tile": "A"})).unwrap();
        c.effects.clear();
        c.execute("tile.render", json!({})).unwrap();
        assert_eq!(c.effects, [Effect::ExportAudio { tiles: vec![ids[0]], set }]);
        c.execute("set.render", json!({})).unwrap();
        assert_eq!(c.effects[1], Effect::ExportAudio { tiles: vec![], set });
        for bad in [json!({"tile": "A", "path": "/tmp/x", "format": "mp3"}), json!({"tile": "A", "path": "/tmp/x", "rate": 1000}), json!({"tile": "A", "path": "/tmp/x", "rate": 44100.5})] {
            assert_eq!(c.execute("tile.render", bad).unwrap_err().code, "bad_params");
        }
        let missing = root.join("no/such/folder/a.wav");
        assert_eq!(c.execute("tile.render", json!({"tile": "A", "path": missing})).unwrap_err().code, "failed");
        assert_eq!(c.execute("render.status", json!({"job": 99})).unwrap_err().code, "not_found");
        assert_eq!(c.execute("render.cancel", json!({})).unwrap()["cancelled"], 0);
        c.has_window = false;
        assert_eq!(c.execute("tile.render", json!({"tile": "A"})).unwrap_err().code, "no_window");
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn renders_from_the_file_when_nothing_is_decoded() {
        // The offline CLI opens the library without decoding.
        let (mut c, root) = controller();
        add(&mut c, &root, &["Bell"]);
        c.store.save_now().unwrap();
        drop(c);
        let store = Store::open_with(root.join("data"), false).unwrap();
        let mut c = Controller::new(store, Engine::disabled("test"));
        c.has_window = false;
        let started = c.execute("tile.render", json!({"tile": "Bell", "path": root.join("bell.wav")})).unwrap();
        let status = finish(&mut c, &started);
        assert_eq!(status["errors"], json!([]));
        let a = crate::decode::decode_file(&root.join("bell.wav")).unwrap();
        assert_eq!(a.data.len(), 4000 * 2);
        std::fs::remove_dir_all(root).ok();
    }
}
