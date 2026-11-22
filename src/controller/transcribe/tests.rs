use crate::audio::Engine as AudioEngine;
use crate::controller::Controller;
use crate::store::Store;
use crate::subtitles::{SubtitleLine, SubtitleWord};
use crate::transcode::Progress;
use crate::transcribe::{Choice, Context, Engine, EngineKind, Heard, Request};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// A stand-in engine: hears `words` (seconds into what it's given), after `wait`
/// unless cancelled; counts how many run at once.
struct Mock {
    words: Vec<(f64, &'static str)>,
    wait: Duration,
    running: Arc<AtomicUsize>,
    most: Arc<AtomicUsize>,
}

impl Engine for Mock {
    fn kind(&self) -> EngineKind {
        EngineKind::WhisperCli
    }

    fn transcribe(&self, req: &Request, progress: &Arc<Progress>) -> Result<Heard, String> {
        let now = self.running.fetch_add(1, Ordering::SeqCst) + 1;
        self.most.fetch_max(now, Ordering::SeqCst);
        let t0 = Instant::now();
        let out = loop {
            if progress.is_cancelled() {
                break Err(crate::transcribe::CANCELLED.to_owned());
            }
            if t0.elapsed() >= self.wait {
                let words: Vec<SubtitleWord> = self.words.iter().filter(|(at, _)| *at < req.seconds).map(|&(at, t)| SubtitleWord { start: at, end: at + 0.2, text: t.into() }).collect();
                let text = words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ");
                let line = SubtitleLine { start: words[0].start, end: words[words.len() - 1].end, text, words, aligned: false, source_timing: None, added: false, machine: None };
                break Ok(Heard { lines: vec![line], model: Some("mock".into()), lang: Some("en".into()) });
            }
            progress.set_fraction(Some(0.5));
            std::thread::sleep(Duration::from_millis(5));
        };
        self.running.fetch_sub(1, Ordering::SeqCst);
        out
    }
}

struct Setup {
    c: Controller,
    starts: Vec<f64>,
    most: Arc<AtomicUsize>,
}

/// A controller with tiles "Talk" and "Chat" (synthesized speech, no subtitles) whose
/// engine is a [`Mock`] hearing six words at the true word starts (a little late).
fn setup(wait: Duration) -> Setup {
    let dir = std::env::temp_dir().join(format!("sb-transcribe-c-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut c = Controller::new(Store::open(dir.join("data")).unwrap(), AudioEngine::disabled("test"));
    let (mono, starts) = crate::demo::synth::speech_with_words("1 1 1 1 | 1 1", 130.0, 0.4, 4.0, 5);
    let mut paths = Vec::new();
    for name in ["Talk", "Chat"] {
        let wav = dir.join(format!("{name}.wav"));
        crate::wav::write_mono(&wav, crate::demo::synth::RATE, &mono).unwrap();
        paths.push(wav);
    }
    c.execute("tile.add", json!({"paths": paths})).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while (c.store.is_loading() || c.store.library.sets[0].tiles.iter().any(|t| c.store.import_of(t).is_some())) && Instant::now() < deadline {
        c.store.poll();
        std::thread::sleep(Duration::from_millis(10));
    }
    c.tick(0.0);
    let texts = ["we", "have", "a", "show", "go", "on"];
    let words: Vec<(f64, &'static str)> = starts.iter().zip(texts).map(|(s, t)| (s + 0.04, t)).collect();
    let (running, most) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let m = most.clone();
    c.transcriber.opener = Arc::new(move |_: Choice, _: &Context| {
        Ok(Box::new(Mock { words: words.clone(), wait, running: running.clone(), most: m.clone() }) as Box<dyn Engine>)
    });
    Setup { c, starts, most }
}

/// Tick until the job ends; its status.
fn wait(c: &mut Controller, job: &Value) -> Value {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let s = c.execute("transcribe.status", json!({"job": job["job"]})).unwrap()["jobs"][0].clone();
        if s["finished"] == json!(true) {
            return s;
        }
        assert!(Instant::now() < deadline, "the job didn't finish: {s}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn subs(c: &mut Controller, tile: &str) -> Value {
    c.execute("tile.subtitles", json!({"tile": tile})).unwrap()
}

#[test]
fn a_tile_without_subtitles_gets_machine_lines_as_one_undo_step() {
    let Setup { mut c, starts, .. } = setup(Duration::ZERO);
    let job = c.execute("tile.transcribe", json!({"tile": "Talk"})).unwrap();
    assert_eq!((job["tiles"].as_u64(), job["engine"].as_str()), (Some(1), Some("whisper")));
    let done = wait(&mut c, &job);
    let r = &done["results"][0];
    assert_eq!((r["replaced"].as_bool(), r["lines"].as_u64(), r["heard"]["words"].as_u64()), (Some(true), Some(1), Some(6)), "{done}");
    assert!(r["refined"]["moved"].as_u64().unwrap() >= 4, "refine moved the late words: {r}");
    let s = subs(&mut c, "Talk");
    assert_eq!(s["cues"][0]["text"], "we have a show go on");
    assert_eq!(s["cues_source"], json!({"kind": "machine", "lang": "en", "engine": "whisper.cpp", "model": "mock"}));
    assert!(s["cues"][0].get("aligned").is_none(), "machine words aren't placed by hand");
    let got: Vec<f64> = s["cues"][0]["words"].as_array().unwrap().iter().map(|w| w["start"].as_f64().unwrap()).collect();
    let off = got.iter().zip(&starts).map(|(g, t)| (g - t).abs()).sum::<f64>() / got.len() as f64;
    assert!(off < 0.03, "heard 40 ms late; refined, the words sit nearer their sounds ({off:.3}: {got:?} vs {starts:?})");
    assert!(c.notice.as_ref().unwrap().text.starts_with("Transcribed “Talk”"), "{}", c.notice.as_ref().unwrap().text);

    // The search box finds it by a spoken word.
    let found = c.execute("tile.search", json!({"query": "show"})).unwrap();
    assert_eq!(found.as_array().unwrap().iter().filter(|m| m["title"] == "Talk").count(), 1, "{found}");

    // One undo step takes them away; the label says what it was.
    assert_eq!(c.history.undo_label(), Some("Transcribe"));
    c.execute("edit.undo", json!({})).unwrap();
    assert_eq!(subs(&mut c, "Talk")["cues"], json!([]));
    c.execute("edit.redo", json!({})).unwrap();
    assert_eq!(subs(&mut c, "Talk")["cues"][0]["text"], "we have a show go on");
}

#[test]
fn known_text_is_timed_never_replaced_and_hand_work_survives() {
    let Setup { mut c, starts, .. } = setup(Duration::ZERO);
    let cues = json!([{"start": 0.0, "end": starts[3] + 0.5, "text": "We have a SHOW!"}, {"start": starts[4], "end": starts[5] + 0.5, "text": "go on"}]);
    c.execute("tile.subtitlesSet", json!({"tile": "Talk", "cues": cues, "cues_source": {"kind": "srt"}})).unwrap();
    // The second line placed by hand.
    c.execute("tile.subtitleTap", json!({"tile": "Talk", "line": 2, "word": 2, "at": starts[5] + 0.1})).unwrap();
    let hand = subs(&mut c, "Talk")["cues"][1].clone();
    let job = c.execute("tile.transcribe", json!({"tile": "Talk"})).unwrap();
    let done = wait(&mut c, &job);
    let r = &done["results"][0];
    assert_eq!((r["replaced"].as_bool(), r["aligned"]["lines"].as_u64(), r["aligned"]["matched"].as_u64()), (Some(false), Some(1), Some(4)), "{done}");
    let s = subs(&mut c, "Talk");
    assert_eq!(s["cues"][0]["text"], "We have a SHOW!", "the known text stays");
    let words: Vec<&str> = s["cues"][0]["words"].as_array().unwrap().iter().map(|w| w["text"].as_str().unwrap()).collect();
    assert_eq!(words, ["We", "have", "a", "SHOW!"]);
    assert!(s["cues"][0]["source_timing"].is_object(), "Reset puts the old timing back");
    assert_eq!(s["cues"][1], hand, "the line placed by hand is untouched");
    assert_eq!(s["cues_source"]["kind"], "srt");
    assert_eq!(s["cues_source"]["word_timing"]["engine"], "whisper.cpp");

    // Every line timed now: without gaps there's nothing left for tile.transcribe to do.
    let e = c.execute("tile.transcribe", json!({"tile": "Talk", "gaps": false})).unwrap_err();
    assert_eq!(e.code, "nothing_to_do");
    assert!(e.message.contains("word timings already"), "{}", e.message);
    // alignWords times one line again from its text.
    c.execute("tile.subtitleReset", json!({"tile": "Talk", "line": 1})).unwrap();
    let job = c.execute("tile.alignWords", json!({"tile": "Talk", "line": 1})).unwrap();
    let done = wait(&mut c, &job);
    assert_eq!(done["results"][0]["aligned"]["lines"], 1, "{done}");
    let e = c.execute("tile.alignWords", json!({"tile": "Talk", "line": 2})).unwrap_err();
    assert!(e.message.contains("placed by hand"), "{}", e.message);
}

#[test]
fn one_tile_at_a_time_a_set_at_once_and_locked_sets_refuse() {
    let Setup { mut c, most, .. } = setup(Duration::from_millis(60));
    let job = c.execute("set.transcribe", json!({})).unwrap();
    assert_eq!(job["tiles"], 2);
    // Both busy: one running, one queued.
    let ids: Vec<uuid::Uuid> = c.store.library.sets[0].tiles.iter().map(|t| t.id).collect();
    assert!(c.transcriber.busy(ids[0]).is_some() && c.transcriber.busy(ids[1]).is_some());
    assert!(c.transcribe_summary().unwrap().contains("(+1)"), "{:?}", c.transcribe_summary());
    let e = c.execute("tile.transcribe", json!({"tile": "Chat"})).unwrap_err();
    assert!(e.message.contains("already being transcribed"), "{}", e.message);
    let done = wait(&mut c, &job);
    assert_eq!((done["done"].as_u64(), done["results"].as_array().unwrap().len()), (Some(2), 2));
    assert_eq!(most.load(Ordering::SeqCst), 1, "never two at once");
    assert_eq!(c.notice.as_ref().unwrap().text, "Transcribed 2 tiles");
    // Two undo steps, one per tile.
    c.execute("edit.undo", json!({})).unwrap();
    assert_eq!(c.store.library.sets[0].tiles.iter().filter(|t| t.subtitles.is_empty()).count(), 1);
    c.execute("edit.redo", json!({})).unwrap();

    // Every tile has subtitles now.
    assert_eq!(c.execute("set.transcribe", json!({})).unwrap_err().code, "nothing_to_do");

    // A locked set refuses all three.
    c.execute("set.lock", json!({})).unwrap();
    for (id, p) in [("tile.transcribe", json!({"tile": "Talk"})), ("set.transcribe", json!({})), ("tile.alignWords", json!({"tile": "Talk"}))] {
        assert_eq!(c.execute(id, p).unwrap_err().code, "locked", "{id}");
    }
}

#[test]
fn cancel_stops_it_and_changes_nothing() {
    let Setup { mut c, .. } = setup(Duration::from_secs(30));
    let job = c.execute("tile.transcribe", json!({"tiles": ["Talk", "Chat"]})).unwrap();
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(c.execute("transcribe.cancel", json!({})).unwrap()["cancelled"], 2);
    let t0 = Instant::now();
    let done = wait(&mut c, &job);
    assert!(t0.elapsed() < Duration::from_secs(5));
    assert_eq!(done["cancelled"], true);
    assert!(c.store.library.sets[0].tiles.iter().all(|t| t.subtitles.is_empty()));
    assert_eq!(c.notice.as_ref().unwrap().text, "Transcription cancelled");
    assert_eq!(c.history.undo_label(), Some("Add Sounds"), "no undo step");
    assert!(c.transcriber.is_idle());
}

#[test]
fn no_engine_says_why_and_settings_are_checked() {
    let Setup { mut c, .. } = setup(Duration::ZERO);
    c.transcriber.opener = Arc::new(crate::transcribe::pick);
    // A model that isn't there (whisper-cli may or may not be installed here).
    let r = c.execute("settings.transcribe", json!({"engine": "whisper", "lang": "EN", "whisperModel": "/nowhere/ggml-base.en.bin"})).unwrap();
    assert_eq!(r, json!({"engine": "whisper", "lang": "en", "whisperModel": "/nowhere/ggml-base.en.bin", "quipServer": null}));
    let e = c.execute("tile.transcribe", json!({"tile": "Talk"})).unwrap_err();
    assert_eq!(e.code, "unavailable");
    assert!(e.message.starts_with("Can't transcribe: ") && (e.message.contains("isn't installed") || e.message.contains("isn't there")), "{}", e.message);
    let engines = c.execute("transcribe.engines", json!({})).unwrap();
    assert_eq!((engines["setting"].as_str(), engines["uses"].as_str()), (Some("whisper"), None));
    let whisper = engines["engines"].as_array().unwrap().iter().find(|e| e["engine"] == "whisper").unwrap();
    assert_eq!(whisper["available"], false);
    if cfg!(any(target_os = "macos", target_os = "ios")) {
        // A test binary isn't an app bundle's program: Apple Speech says so, reports no
        // permission (it never asked), and transcribe.allowSpeech refuses without
        // showing the prompt (D-288, D-289).
        assert_eq!(engines["engines"][0]["engine"], "apple");
        assert!(engines["engines"][0]["reason"].as_str().unwrap().contains("app bundle"), "{engines}");
        assert!(engines["apple"]["permission"].is_null() && engines["apple"]["canAsk"] == false, "{engines}");
        let e = c.execute("transcribe.allowSpeech", json!({})).unwrap_err();
        assert_eq!(e.code, "unavailable");
        assert!(e.message.contains("app bundle"), "{}", e.message);
    } else {
        assert!(engines["apple"].is_null());
        assert_eq!(c.execute("transcribe.allowSpeech", json!({})).unwrap_err().code, "unavailable");
    }
    assert!(engines["reason"].as_str().is_some());
    assert_eq!(c.execute("settings.transcribe", json!({"engine": "siri"})).unwrap_err().code, "bad_params");
    assert_eq!(c.execute("settings.transcribe", json!({"lang": "en; rm"})).unwrap_err().code, "bad_params");
    let r = c.execute("settings.transcribe", json!({"engine": "auto", "lang": "", "whisperModel": ""})).unwrap();
    assert_eq!(r, json!({"engine": "auto", "lang": null, "whisperModel": null, "quipServer": null}));
}

#[test]
fn the_window_asks_before_a_whole_set() {
    let Setup { mut c, .. } = setup(Duration::ZERO);
    c.has_window = true;
    let r = c.execute("set.transcribe", json!({"ask": true})).unwrap();
    assert_eq!((r["asked"].as_bool(), r["tiles"].as_u64(), r["engine"].as_str()), (Some(true), Some(2), Some("whisper")));
    assert!(c.effects.iter().any(|e| matches!(e, crate::controller::Effect::ConfirmTranscribe { tiles: 2, subtitled: 0, .. })));
    assert!(c.transcriber.is_idle(), "nothing starts until the answer");
}

#[test]
fn the_lines_heard_between_known_subtitles_are_added_once_as_one_undo_step() {
    let Setup { mut c, starts, .. } = setup(Duration::ZERO);
    // Only the first four words are subtitled; "go on", after the pause, isn't.
    let cues = json!([{"start": 0.0, "end": starts[3] + 0.3, "text": "We have a show."}]);
    c.execute("tile.subtitlesSet", json!({"tile": "Talk", "cues": cues, "cues_source": {"kind": "srt", "lang": "en"}})).unwrap();
    let before = c.history.undo.len();
    let job = c.execute("tile.transcribe", json!({"tile": "Talk"})).unwrap();
    let done = wait(&mut c, &job);
    let r = &done["results"][0];
    assert_eq!(r["gaps"], json!({"added": 1, "replaced": 0, "kept": 0}), "{done}");
    assert_eq!(r["aligned"]["lines"], 1, "the known line is timed as before");
    let s = subs(&mut c, "Talk");
    assert_eq!(s["cues"][0]["text"], "We have a show.", "known text exactly as it was");
    assert!(s["cues"][0].get("machine").is_none());
    let gap = &s["cues"][1];
    assert_eq!(gap["text"], "go on");
    assert_eq!(gap["machine"], json!({"kind": "machine", "lang": "en", "engine": "whisper.cpp", "model": "mock"}));
    assert!(gap.get("aligned").is_none(), "never marked aligned");
    assert!(gap["start"].as_f64().unwrap() >= s["cues"][0]["end"].as_f64().unwrap(), "after the known line, never over it: {s}");
    let first: f64 = gap["words"][0]["start"].as_f64().unwrap();
    assert!((first - starts[4]).abs() < 0.03, "refined onto its sound like other machine words ({first} vs {})", starts[4]);
    assert_eq!(s["cues_source"]["kind"], "srt");
    let note = &c.notice.as_ref().unwrap().text;
    assert!(note.contains("added 1 line heard between them"), "{note}");
    let found = c.execute("tile.search", json!({"query": "go on"})).unwrap();
    assert!(found.as_array().unwrap().iter().any(|m| m["title"] == "Talk"), "the gap line is searchable: {found}");

    // One undo step takes the gap line and the word timings away together.
    assert_eq!((c.history.undo.len(), c.history.undo_label()), (before + 1, Some("Transcribe")));
    c.execute("edit.undo", json!({})).unwrap();
    assert_eq!(subs(&mut c, "Talk")["cues"].as_array().unwrap().len(), 1);
    c.execute("edit.redo", json!({})).unwrap();

    // Again: nothing doubled, nothing changed, no new undo step.
    let job = c.execute("tile.transcribe", json!({"tile": "Talk"})).unwrap();
    let done = wait(&mut c, &job);
    assert_eq!(done["results"][0]["gaps"], json!({"added": 1, "replaced": 1, "kept": 0}), "{done}");
    assert_eq!(done["results"][0]["changed"], false);
    assert_eq!(subs(&mut c, "Talk")["cues"], s["cues"]);
    assert_eq!(c.history.undo.len(), before + 1);

    // A person fixes the gap line by hand: a later run leaves it as it is.
    c.execute("tile.subtitleTap", json!({"tile": "Talk", "line": 2, "word": 2, "at": starts[5] + 0.05})).unwrap();
    let hand = subs(&mut c, "Talk")["cues"][1].clone();
    assert_eq!(hand["aligned"], true);
    let job = c.execute("tile.transcribe", json!({"tile": "Talk"})).unwrap();
    let done = wait(&mut c, &job);
    assert_eq!(done["results"][0]["gaps"], json!({"added": 0, "replaced": 0, "kept": 1}), "{done}");
    let s = subs(&mut c, "Talk");
    assert_eq!((s["cues"].as_array().unwrap().len(), &s["cues"][1]), (2, &hand));

    // set.transcribe takes the subtitled tile only with gaps; a locked set refuses it.
    let r = c.execute("set.transcribe", json!({"gaps": true})).unwrap();
    assert_eq!(r["tiles"], 2, "Chat (no subtitles) and Talk (subtitles from elsewhere): {r}");
    wait(&mut c, &r);
    c.has_window = true;
    let ask = c.execute("set.transcribe", json!({"ask": true})).unwrap();
    assert_eq!((ask["tiles"].as_u64(), ask["subtitled"].as_u64()), (Some(0), Some(1)), "Chat is machine-made now, Talk is subtitled: {ask}");
    c.has_window = false;
    c.execute("set.lock", json!({})).unwrap();
    for p in [json!({"gaps": true}), json!({})] {
        assert_eq!(c.execute("set.transcribe", p).unwrap_err().code, "locked");
    }
    assert_eq!(c.execute("tile.transcribe", json!({"tile": "Talk", "gaps": true})).unwrap_err().code, "locked");
}

/// The real engine type with a stand-in whisper-cli script, through the commands.
#[cfg(unix)]
#[test]
fn whisper_cli_stand_in_end_to_end() {
    let Setup { mut c, .. } = setup(Duration::ZERO);
    let dir = std::env::temp_dir().join(format!("sb-whisper-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let model = crate::transcribe::tests::fake_model(&dir);
    let program = crate::transcribe::tests::stand_in(&dir, include_str!("../../transcribe/fixtures/whisper-full.json"), "");
    c.transcriber.opener = Arc::new(move |_, _| Ok(Box::new(crate::transcribe::whisper_cli::WhisperCli { program: program.clone(), model: model.clone() }) as Box<dyn Engine>));
    let job = c.execute("tile.transcribe", json!({"tile": "Talk"})).unwrap();
    let done = wait(&mut c, &job);
    assert_eq!(done["results"][0]["model"], "tiny.en", "{done}");
    let s = subs(&mut c, "Talk");
    assert!(s["cues"][0]["text"].as_str().unwrap().starts_with("Welcome"), "{s}");
    std::fs::remove_dir_all(dir).ok();
}
