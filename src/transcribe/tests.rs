use super::*;
use crate::demo::synth;
use crate::subtitles::SubtitleWord;

fn tone(rate: u32, seconds: f64, hz: f64, amp: f32) -> Vec<f32> {
    let n = (f64::from(rate) * seconds) as usize;
    (0..n).map(|i| amp * (std::f64::consts::TAU * hz * i as f64 / f64::from(rate)).sin() as f32).collect()
}

/// Stereo audio from mono samples, as the decoder gives it.
fn stereo(rate: u32, mono: &[f32]) -> DecodedAudio {
    DecodedAudio { sample_rate: rate, data: mono.iter().flat_map(|&s| [s, s]).collect(), art: None, native: None }
}

/// Frequency from upward zero crossings, over the middle of the signal (away from the
/// kernel's edges).
fn pitch(samples: &[f32], rate: u32) -> f64 {
    let mid = &samples[samples.len() / 10..samples.len() * 9 / 10];
    let ups: Vec<usize> = (1..mid.len()).filter(|&i| mid[i - 1] < 0.0 && mid[i] >= 0.0).collect();
    let (first, last) = (ups[0], ups[ups.len() - 1]);
    (ups.len() - 1) as f64 * f64::from(rate) / (last - first) as f64
}

fn rms(s: &[f32]) -> f64 {
    let mid = &s[s.len() / 10..s.len() * 9 / 10];
    (mid.iter().map(|x| f64::from(*x) * f64::from(*x)).sum::<f64>() / mid.len() as f64).sqrt()
}

#[test]
fn resampling_keeps_pitch_length_and_level_and_drops_what_cant_be_kept() {
    for (from, hz) in [(44_100, 1000.0), (48_000, 440.0), (8_000, 500.0), (22_050, 3000.0)] {
        let input = tone(from, 2.0, hz, 0.5);
        let out = resample(&input, from, RATE);
        assert_eq!(out.len(), (input.len() as f64 * f64::from(RATE) / f64::from(from)).round() as usize, "{from} Hz: the length scales exactly");
        let got = pitch(&out, RATE);
        assert!((got - hz).abs() < hz * 0.002, "{from} Hz: a {hz} Hz tone stays {hz} Hz (got {got:.2})");
        let level = rms(&out) / rms(&input);
        assert!((level - 1.0).abs() < 0.02, "{from} Hz: the level stays (ratio {level:.3})");
    }
    // 12 kHz can't be kept at 16 kHz: it's filtered out, not folded back to 4 kHz.
    let high = resample(&tone(44_100, 1.0, 12_000.0, 0.5), 44_100, RATE);
    assert!(rms(&high) < 0.01, "an inaudible-at-16k tone is removed (rms {:.4})", rms(&high));
    assert_eq!(resample(&[0.1, 0.2], RATE, RATE), vec![0.1, 0.2], "the same rate is a copy");
}

#[test]
fn prepare_makes_mono_16k_wav_of_the_range_with_its_offset() {
    let mut mono = tone(48_000, 3.0, 440.0, 0.4);
    // Left and right differ: the mix is their mean.
    let mut audio = stereo(48_000, &mono);
    for (i, pair) in audio.data.as_chunks_mut::<2>().0.iter_mut().enumerate() {
        pair[1] = if i % 2 == 0 { 0.2 } else { -0.2 };
    }
    let whole = prepare(&audio, None);
    assert_eq!((whole.offset, (whole.seconds * 1000.0).round()), (0.0, 3000.0));
    let w = &whole.wav;
    assert_eq!(&w[0..4], b"RIFF");
    assert_eq!(&w[8..16], b"WAVEfmt ");
    assert_eq!(u16::from_le_bytes([w[22], w[23]]), 1, "mono");
    assert_eq!(u32::from_le_bytes([w[24], w[25], w[26], w[27]]), RATE);
    assert_eq!(u16::from_le_bytes([w[34], w[35]]), 16, "16-bit");
    assert_eq!(w.len(), 44 + 3 * RATE as usize * 2);

    let part = prepare(&audio, Some((0.5, 1.75)));
    assert_eq!(part.offset, 0.5, "times in it are offset + t in the file");
    assert!((part.seconds - 1.25).abs() < 1e-3, "{}", part.seconds);
    // Out-of-range asks are clamped, never a panic.
    assert_eq!(prepare(&audio, Some((5.0, 9.0))).seconds, 0.0);
    mono.clear();
}

#[test]
fn engines_are_named_chosen_and_explain_themselves() {
    assert_eq!(Choice::parse("auto"), Some(Choice::Auto));
    assert_eq!(Choice::parse(""), Some(Choice::Auto));
    assert_eq!(Choice::parse("Whisper"), Some(Choice::One(EngineKind::WhisperCli)));
    assert_eq!(Choice::parse("whisper-cli").map(Choice::name), Some("whisper"));
    assert_eq!(Choice::parse("quip"), Some(Choice::One(EngineKind::Quip)));
    assert_eq!(Choice::parse("apple"), Some(Choice::One(EngineKind::AppleSpeech)));
    assert_eq!(Choice::parse("siri"), None);
    // Only what this build has is listed; the others say so when asked for by name.
    let ctx = Context { whisper_model: Some("/nowhere/ggml-base.en.bin".into()), ..Context::default() };
    let listed = availability(&ctx);
    // Apple Speech is listed first on macOS and iPadOS, and can't run in a test binary,
    // which isn't an app bundle's own program (D-289).
    let apple = cfg!(any(target_os = "macos", target_os = "ios"));
    let kinds: Vec<EngineKind> = listed.iter().map(|a| a.kind).collect();
    if apple {
        assert_eq!(kinds, vec![EngineKind::AppleSpeech, EngineKind::WhisperCli, EngineKind::Quip]);
        let why = listed[0].reason.clone().unwrap();
        assert!(!listed[0].available && why.contains("app bundle"), "{why}");
    } else {
        assert_eq!(kinds, vec![EngineKind::WhisperCli, EngineKind::Quip]);
    }
    let whisper = listed.iter().find(|a| a.kind == EngineKind::WhisperCli).unwrap();
    assert!(!whisper.available && whisper.reason.is_some(), "no such model, so whisper-cli can't run here");
    let why = pick(Choice::One(EngineKind::Quip), &ctx).err().unwrap();
    assert!(why.contains("QuipClipper"), "{why}");
    let why = pick(Choice::Auto, &ctx).err().unwrap();
    assert_eq!(Some(why), listed[0].reason.clone(), "auto gives the first engine's reason");
}

/// Words heard `err` seconds off each true start, a short word each.
fn heard_at(starts: &[f64], errs: &[f64]) -> Vec<SubtitleLine> {
    let words: Vec<SubtitleWord> = starts.iter().zip(errs).enumerate().map(|(i, (s, e))| SubtitleWord { start: s + e, end: s + e + 0.12, text: format!("w{i}") }).collect();
    vec![SubtitleLine { start: words[0].start, end: words[words.len() - 1].end, text: "x".into(), words, aligned: false, source_timing: None, added: false, machine: None }]
}

#[test]
fn refine_puts_word_starts_on_the_sounds() {
    let (mono, starts) = synth::speech_with_words("1 2 1 1 | 2 1 1", 130.0, 0.4, 4.0, 11);
    let audio = stereo(synth::RATE, &mono);
    let found = crate::align::onsets(&audio);
    // An engine's starts are off by up to a tenth of a second: most late, a few early.
    let errs = [0.07, -0.03, 0.09, 0.08, 0.04, -0.02, 0.06];
    let mut lines = heard_at(&starts, &errs);
    let moved = refine(&mut lines, &found, REFINE_REACH);
    assert_eq!(moved.len(), starts.len());
    let words = &lines[0].words;
    let off: Vec<f64> = words.iter().zip(&starts).map(|(w, t)| (w.start - t).abs()).collect();
    let before = errs.iter().map(|e| e.abs()).sum::<f64>() / errs.len() as f64;
    let after = off.iter().sum::<f64>() / off.len() as f64;
    // The finder also marks syllables inside words, so a word heard far off can land on
    // one; most land on their own start.
    assert!(off.iter().filter(|&&o| o < 0.03).count() >= 6, "most words land on their sound: {off:?}");
    assert!(after < before / 2.0, "closer on the whole: {before:.3} → {after:.3} ({off:?})");
    for pair in words.windows(2) {
        assert!(pair[0].start < pair[1].start && pair[0].end <= pair[1].start + 1e-9, "in order, not overlapping");
    }
    assert!(words.iter().all(|w| w.end - w.start >= crate::align::MIN_WORD - 1e-9));
    assert!(!lines[0].aligned, "refined is not placed by hand");
    assert_eq!((lines[0].start, lines[0].end), (words[0].start, words[words.len() - 1].end), "the line spans its words");
    let mean = moved.iter().map(|m| m.abs()).sum::<f64>() / moved.len() as f64;
    assert!(mean > 0.03, "it did move them ({mean:.3})");
}

#[test]
fn refine_keeps_order_when_onsets_crowd_and_leaves_far_words() {
    let onset = |at| crate::align::Onset { at, strength: 20.0 };
    // Both words want the same onset; the second can't pass the first.
    let mut lines = heard_at(&[1.0, 1.05], &[0.0, 0.0]);
    refine(&mut lines, &[onset(1.02)], Reach { earlier: 0.1, later: 0.1 });
    let w = &lines[0].words;
    assert!((w[0].start - 1.02).abs() < 1e-9);
    assert!(w[1].start >= w[0].start + crate::align::MIN_WORD - 1e-9, "{:?}", w);
    // Nothing within reach: a word stays put. Later sounds are reached only a little way.
    let mut lines = heard_at(&[3.0], &[0.0]);
    let moved = refine(&mut lines, &[onset(2.0), onset(3.1), onset(3.5)], REFINE_REACH);
    assert_eq!((lines[0].words[0].start, moved), (3.0, vec![0.0]));
    let mut lines = heard_at(&[3.0], &[0.0]);
    refine(&mut lines, &[onset(2.9), onset(3.04)], REFINE_REACH);
    assert!((lines[0].words[0].start - 3.04).abs() < 1e-9, "the nearest within reach");
    let mut lines = heard_at(&[3.0], &[0.0]);
    refine(&mut lines, &[onset(2.88), onset(3.08)], REFINE_REACH);
    assert!((lines[0].words[0].start - 2.88).abs() < 1e-9, "back as far as 0.15 s, on only 0.05 s");
}

fn known(start: f64, end: f64, text: &str) -> SubtitleLine {
    SubtitleLine { start, end, text: text.into(), words: Vec::new(), aligned: false, source_timing: None, added: false, machine: None }
}

fn w(start: f64, text: &str) -> SubtitleWord {
    SubtitleWord { start, end: start + 0.25, text: text.into() }
}

fn starts(l: &SubtitleLine) -> Vec<f64> {
    l.words.iter().map(|w| (w.start * 1000.0).round() / 1000.0).collect()
}

#[test]
fn known_text_takes_the_heard_times_and_keeps_its_words() {
    // Punctuation and case differ; the text is kept exactly.
    let mut lines = vec![known(0.8, 2.6, "Good evening, everyone!"), known(3.0, 4.5, "Doors open at seven.")];
    let heard = [w(1.0, "good"), w(1.3, "Evening"), w(1.7, "everyone"), w(3.2, "doors"), w(3.5, "open"), w(3.8, "at"), w(4.0, "seven")];
    let r = align_text(&mut lines, &[0, 1], &heard);
    assert_eq!((r.lines, r.matched, r.placed, r.missed), (2, 7, 0, 0));
    assert_eq!(lines[0].text, "Good evening, everyone!");
    assert_eq!(lines[0].words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>(), ["Good", "evening,", "everyone!"]);
    assert_eq!(starts(&lines[0]), [1.0, 1.3, 1.7]);
    assert_eq!(starts(&lines[1]), [3.2, 3.5, 3.8, 4.0]);
    assert_eq!((lines[0].start, lines[1].end), (1.0, 4.25), "the lines span their words");
    assert!(lines.iter().all(|l| !l.aligned), "not placed by hand");
    let st = lines[0].source_timing.as_ref().unwrap();
    assert_eq!((st.start, st.end), (0.8, 2.6), "Reset can put the old timing back");
}

#[test]
fn awkward_hearing_still_times_the_known_words() {
    // An extra word heard ("um"), a word missed ("very"), one misheard ("bright" as
    // "right"), and a number spoken that the text spells out.
    let mut lines = vec![known(0.0, 4.0, "It was a very bright morning with forty two guests")];
    let heard = [w(0.2, "it"), w(0.45, "um"), w(0.7, "was"), w(0.9, "a"), w(1.3, "right"), w(1.6, "morning"), w(2.0, "with"), w(2.4, "42"), w(2.9, "guests")];
    let r = align_text(&mut lines, &[0], &heard);
    assert_eq!(r.lines, 1);
    let l = &lines[0];
    assert_eq!(l.text, "It was a very bright morning with forty two guests", "the text is never replaced");
    let s = starts(l);
    assert_eq!(s.len(), 10);
    assert_eq!((s[0], s[1], s[2]), (0.2, 0.7, 0.9), "the extra word is skipped");
    assert!(s[3] > 0.9 && s[3] < 1.3, "the missed word fits between its neighbours ({})", s[3]);
    assert_eq!((s[4], s[5], s[6]), (1.3, 1.6, 2.0), "the misheard word keeps its place");
    assert!(s[7] >= 2.25 && s[8] > s[7] && s[8] < 2.9, "the number's words share its time ({}, {})", s[7], s[8]);
    assert_eq!(s[9], 2.9);
    for pair in l.words.windows(2) {
        assert!(pair[0].end <= pair[1].start + 1e-9 && pair[0].start < pair[1].start, "{:?}", l.words);
    }
}

#[test]
fn lines_placed_by_hand_or_unheard_are_left_alone() {
    let mut hand = known(0.0, 1.0, "hello there");
    hand.words = vec![SubtitleWord { start: 0.1, end: 0.4, text: "hello".into() }, SubtitleWord { start: 0.5, end: 0.9, text: "there".into() }];
    hand.aligned = true;
    let mut lines = vec![hand.clone(), known(5.0, 6.0, "nothing like it")];
    let r = align_text(&mut lines, &[0, 1], &[w(0.2, "hello"), w(0.6, "there"), w(9.0, "zebra")]);
    assert_eq!(lines[0], hand, "placed by hand: untouched");
    assert_eq!((r.lines, r.matched, r.placed, r.missed), (0, 0, 0, 1));
    assert!(lines[1].words.is_empty(), "nothing heard lines up with it");
}

#[test]
fn applying_makes_machine_lines_or_times_known_ones() {
    let heard = Heard {
        lines: vec![SubtitleLine { start: 0.5, end: 1.5, text: "hello there".into(), words: vec![w(0.5, "hello"), w(1.0, "there")], aligned: false, source_timing: None, added: false, machine: None }],
        model: Some("base.en".into()),
        lang: Some("en".into()),
    };
    // No subtitles: the engine's lines, marked machine-made.
    let a = apply(&[], None, EngineKind::WhisperCli, heard.clone(), None, false);
    assert!(a.replaced);
    assert_eq!(a.lines, heard.lines);
    let src = a.source.clone().unwrap();
    assert_eq!((src.kind.as_str(), src.lang.as_deref(), src.extra["engine"].as_str(), src.extra["model"].as_str()), ("machine", Some("en"), Some("whisper.cpp"), Some("base.en")));
    assert_eq!(source_label(&src), "Machine-made · whisper.cpp · base.en · en");

    // Machine-made lines are replaced by a new transcription; a line aligned by hand stays.
    let mut mine = heard.lines[0].clone();
    mine.words[1].start = 1.1;
    mine.aligned = true;
    let a2 = apply(&[mine.clone()], Some(&src), EngineKind::WhisperCli, heard.clone(), None, false);
    assert_eq!((a2.lines, a2.kept_aligned), (vec![mine], 1));

    // Subtitles from elsewhere keep their text; the words get times, and the source says who timed them.
    let srt = crate::subtitles::SubtitleSource { kind: "srt".into(), lang: Some("en".into()), extra: Default::default() };
    let a3 = apply(&[known(0.4, 1.6, "Hello, there.")], Some(&srt), EngineKind::WhisperCli, heard, None, true);
    assert!(!a3.replaced);
    assert_eq!(a3.lines[0].text, "Hello, there.");
    assert_eq!((a3.lines.len(), a3.gaps.added), (1, 0), "the words heard were the known line's: no gap line");
    assert_eq!(starts(&a3.lines[0]), [0.5, 1.0]);
    let s3 = a3.source.unwrap();
    assert_eq!((s3.kind.as_str(), s3.extra["word_timing"]["engine"].as_str()), ("srt", Some("whisper.cpp")));
    assert_eq!(source_label(&s3), "SRT · en · words by whisper.cpp");
}

#[test]
fn heard_times_move_into_the_files_frame() {
    let mut h = Heard { lines: vec![SubtitleLine { start: 0.1, end: 0.5, text: "a b".into(), words: vec![w(0.1, "a")], aligned: false, source_timing: None, added: false, machine: None }], ..Default::default() };
    shift(&mut h, 2.0);
    assert_eq!((h.lines[0].start, h.lines[0].end, h.lines[0].words[0].start), (2.1, 2.5, 2.1));
}

// ── whisper-cli ─────────────────────────────────────────────────────────────

/// whisper-cli's full JSON from a real run (whisper.cpp, tiny.en, a spoken sentence;
/// see the test below for how it was made).
const FIXTURE: &str = include_str!("fixtures/whisper-full.json");

#[test]
fn whisper_full_json_becomes_lines_and_words() {
    let json: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
    let heard = whisper_cli::parse_json(&json);
    assert_eq!(heard.model.as_deref(), Some("tiny.en"));
    assert_eq!(heard.lang.as_deref(), Some("en"));
    assert!(!heard.lines.is_empty());
    let all: Vec<&str> = heard.lines.iter().flat_map(|l| l.words.iter().map(|w| w.text.as_str())).collect();
    assert!(all.iter().all(|w| !w.starts_with('[') && !w.is_empty()), "no markers: {all:?}");
    for l in &heard.lines {
        assert!(l.start < l.end && !l.text.is_empty());
        let joined: String = l.words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ");
        assert_eq!(match_key(&joined), match_key(&l.text), "the words spell the line");
        for pair in l.words.windows(2) {
            assert!(pair[0].start <= pair[1].start, "{:?}", l.words);
        }
    }
}

#[test]
fn whisper_json_skips_markers_and_non_speech() {
    let json = serde_json::json!({
        "params": {"model": "/m/ggml-base.en.bin", "language": "en"},
        "result": {"language": "en"},
        "transcription": [
            {"offsets": {"from": 0, "to": 1000}, "text": " [BLANK_AUDIO]", "tokens": []},
            {"offsets": {"from": 1000, "to": 2500}, "text": " Don't stop now.", "tokens": [
                {"text": "[_BEG_]", "offsets": {"from": 1000, "to": 1000}},
                {"text": " Don", "offsets": {"from": 1000, "to": 1200}},
                {"text": "'t", "offsets": {"from": 1200, "to": 1300}},
                {"text": " stop", "offsets": {"from": 1400, "to": 1800}},
                {"text": " now", "offsets": {"from": 1900, "to": 2300}},
                {"text": ".", "offsets": {"from": 2300, "to": 2400}},
                {"text": "[_TT_125]", "offsets": {"from": 2500, "to": 2500}}
            ]},
            {"offsets": {"from": 3000, "to": 4000}, "text": " (music)"},
            {"offsets": {"from": 4000, "to": 5000}, "text": " Plain segment"}
        ]
    });
    let h = whisper_cli::parse_json(&json);
    assert_eq!(h.model.as_deref(), Some("base.en"));
    assert_eq!(h.lines.len(), 2);
    let words: Vec<(&str, f64, f64)> = h.lines[0].words.iter().map(|w| (w.text.as_str(), w.start, w.end)).collect();
    assert_eq!(words, [("Don't", 1.0, 1.3), ("stop", 1.4, 1.8), ("now.", 1.9, 2.3)], "punctuation doesn't stretch a word");
    assert!(h.lines[1].words.is_empty(), "a segment without tokens: split evenly later");
    assert_eq!(whisper_cli::parse_progress("whisper_print_progress_callback: progress =  45%"), Some(0.45));
    assert_eq!(whisper_cli::parse_progress("progress = 100%"), Some(1.0));
    assert_eq!(whisper_cli::parse_progress("whisper_init: loading"), None);
}

#[test]
fn whisper_cli_language_and_model_names() {
    use whisper_cli::{language, model_name};
    assert_eq!(model_name(std::path::Path::new("/x/ggml-small.en.bin")), "small.en");
    assert_eq!(model_name(std::path::Path::new("tiny.bin")), "tiny");
    assert_eq!(language(None, "base.en"), "en");
    assert_eq!(language(None, "base"), "auto");
    assert_eq!(language(Some("DE"), "base"), "de");
    assert_eq!(language(Some(""), "small.en-tdrz"), "en");
}

#[test]
fn missing_whisper_or_model_says_what_to_do() {
    let e = whisper_cli::find_program_in(None, None, &[]).unwrap_err();
    assert_eq!(e, "whisper-cli isn't installed");
    let hint = whisper_cli::install_hint();
    assert!(hint.contains("whisper-cpp") && hint.contains(&crate::brand::env_name("WHISPER")), "{hint}");
    if cfg!(target_os = "macos") {
        assert!(hint.contains("brew install whisper-cpp"), "{hint}");
    }
    let e = whisper_cli::find_model(None, None).unwrap_err();
    assert!(e.contains("needs a model file") && e.contains("Settings ▸ Subtitles") && e.contains(&crate::brand::env_name("WHISPER_MODEL")), "{e}");
    let e = whisper_cli::find_model(None, Some(std::path::Path::new("/nowhere/ggml-x.bin"))).unwrap_err();
    assert!(e.contains("isn't there"), "{e}");
    let dir = scratch();
    let not_model = dir.join("notes.bin");
    std::fs::write(&not_model, b"hello").unwrap();
    let e = whisper_cli::find_model(None, Some(&not_model)).unwrap_err();
    assert!(e.contains("isn't a whisper.cpp model"), "{e}");
    let model = fake_model(&dir);
    assert_eq!(whisper_cli::find_model(None, Some(&model)), Ok(model.clone()));
    let env = std::ffi::OsString::from(&model);
    assert_eq!(whisper_cli::find_model(Some(env), Some(&not_model)), Ok(model), "the setting in the environment wins");
    std::fs::remove_dir_all(dir).ok();
}

fn scratch() -> PathBuf {
    let d = std::env::temp_dir().join(format!("sb-transcribe-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A file that starts like a ggml model.
pub(crate) fn fake_model(dir: &std::path::Path) -> PathBuf {
    let p = dir.join("ggml-tiny.en.bin");
    std::fs::write(&p, b"lmgg-not-really-a-model").unwrap();
    p
}

/// A stand-in for whisper-cli: a shell script that reports progress, checks its
/// arguments and writes `json` where `-of` says, or fails as `fail` says.
#[cfg(unix)]
pub(crate) fn stand_in(dir: &std::path::Path, json: &str, behaviour: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let fixture = dir.join("fixture.json");
    std::fs::write(&fixture, json).unwrap();
    let script = dir.join("whisper-cli");
    let body = format!(
        r#"#!/bin/sh
out=""; wav=""; lang=""
while [ $# -gt 0 ]; do
  case "$1" in
    -of) out="$2"; shift ;;
    -f) wav="$2"; shift ;;
    -l) lang="$2"; shift ;;
  esac
  shift
done
echo "$lang" > "{dir}/lang"
head -c 4 "$wav" > "{dir}/head"
{behaviour}
echo "whisper_print_progress_callback: progress =  50%" >&2
cp "{fixture}" "$out.json"
"#,
        dir = dir.display(),
        fixture = fixture.display()
    );
    std::fs::write(&script, body).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    script
}

#[cfg(unix)]
#[test]
fn whisper_cli_runs_on_the_prepared_wav_and_cancels() {
    let dir = scratch();
    let model = fake_model(&dir);
    let engine = whisper_cli::WhisperCli { program: stand_in(&dir, FIXTURE, ""), model };
    let (mono, _) = synth::speech_with_words("1 1 1", 130.0, 0.4, 2.0, 3);
    let prepared = prepare(&stereo(synth::RATE, &mono), None);
    let req = Request { wav: Arc::new(prepared.wav), seconds: prepared.seconds, lang: None };
    let progress = Arc::new(Progress::default());
    let heard = engine.transcribe(&req, &progress).unwrap();
    assert_eq!(heard, whisper_cli::parse_json(&serde_json::from_str(FIXTURE).unwrap()));
    assert_eq!(std::fs::read_to_string(dir.join("lang")).unwrap().trim(), "en", "an English-only model hears English");
    assert_eq!(std::fs::read(dir.join("head")).unwrap(), b"RIFF", "it got a WAV file");
    assert_eq!(progress.fraction(), Some(0.5), "progress from its -pp lines");

    // A failure says why, from what it printed.
    let failing = whisper_cli::WhisperCli { program: stand_in(&dir, FIXTURE, "echo 'whisper_init_from_file: failed to load model' >&2; exit 1"), model: engine.model.clone() };
    let e = failing.transcribe(&req, &Arc::new(Progress::default())).unwrap_err();
    assert_eq!(e, "whisper-cli couldn't load the model");

    // Cancel stops it at once.
    let slow = whisper_cli::WhisperCli { program: stand_in(&dir, FIXTURE, "sleep 30"), model: engine.model.clone() };
    let progress = Arc::new(Progress::default());
    let p2 = progress.clone();
    let t0 = std::time::Instant::now();
    let h = std::thread::spawn(move || slow.transcribe(&req, &p2));
    std::thread::sleep(std::time::Duration::from_millis(200));
    progress.cancel();
    assert_eq!(h.join().unwrap().unwrap_err(), CANCELLED);
    assert!(t0.elapsed() < std::time::Duration::from_secs(5));
    std::fs::remove_dir_all(dir).ok();
}

// ── gap lines (D-296) ───────────────────────────────────────────────────────

fn heard_line(words: &[(f64, f64, &str)]) -> SubtitleLine {
    let words: Vec<SubtitleWord> = words.iter().map(|&(s, e, t)| SubtitleWord { start: s, end: e, text: t.into() }).collect();
    let text = words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ");
    SubtitleLine { start: words[0].start, end: words[words.len() - 1].end, text, words, aligned: false, source_timing: None, added: false, machine: None }
}

fn texts(lines: &[SubtitleLine]) -> Vec<&str> {
    lines.iter().map(|l| l.text.as_str()).collect()
}

fn mock_source() -> SubtitleSource {
    machine_source(EngineKind::WhisperCli, &Heard { lines: vec![], model: Some("tiny.en".into()), lang: Some("en".into()) })
}

#[test]
fn gap_lines_are_what_is_heard_where_no_known_line_is_spoken() {
    let covered = [known(1.0, 2.0, "Known one"), known(5.0, 6.0, "Known two")];
    // One heard line running from the first known line through the gap into the second.
    let heard = [heard_line(&[(1.1, 1.5, "known"), (1.5, 1.9, "one"), (2.6, 3.0, "and"), (3.0, 3.4, "something"), (3.5, 3.9, "else."), (5.1, 5.5, "known")]),
        heard_line(&[(6.8, 7.2, "After"), (7.2, 7.6, "it.")])];
    let gaps = gap_lines(&covered, &heard, &mock_source());
    assert_eq!(texts(&gaps), ["and something else.", "After it."], "the words in each gap, as lines of their own");
    let g = &gaps[0];
    assert_eq!((g.start, g.end, g.words.len()), (2.6, 3.9, 3));
    assert!(!g.aligned && g.source_timing.is_none());
    let m = g.machine.as_ref().unwrap();
    assert_eq!((m.kind.as_str(), m.extra["engine"].as_str(), m.extra["model"].as_str(), m.lang.as_deref()), ("machine", Some("whisper.cpp"), Some("tiny.en"), Some("en")));
    for g in &gaps {
        assert!(covered.iter().all(|c| g.end <= c.start || g.start >= c.end), "never over a known line: {g:?}");
    }

    // Two heard lines that run into each other don't overlap as gap lines either.
    let touching = gap_lines(&covered, &[heard_line(&[(2.6, 3.0, "one"), (3.0, 3.6, "two")]), heard_line(&[(3.5, 3.9, "three")])], &mock_source());
    assert_eq!(texts(&touching), ["one two", "three"]);
    assert!(touching[0].end <= touching[1].start && touching[0].words[1].end <= touching[1].start, "{touching:?}");

    // Nothing known: everything heard is a gap line.
    assert_eq!(texts(&gap_lines(&[], &heard, &mock_source())), ["known one and something else. known", "After it."]);
}

#[test]
fn a_word_at_a_known_lines_edge_is_that_lines_and_short_gaps_stay_empty() {
    let covered = [known(1.0, 2.0, "Hello there"), known(4.0, 5.0, "Goodbye")];
    let heard = [heard_line(&[
        // Middle 2.2: within 0.25 s of the line's end, so the line's word, heard late.
        (2.0, 2.4, "there"),
        // Middle 2.5, clear of it: a gap word.
        (2.3, 2.7, "well"),
        (2.8, 3.2, "then"),
        // Starts in the gap and runs into the next line: cut at its start.
        (3.2, 4.1, "so"),
    ])];
    let gaps = gap_lines(&covered, &heard, &mock_source());
    assert_eq!(texts(&gaps), ["well then so"]);
    let w = &gaps[0].words;
    assert!((w[0].start - 2.3).abs() < 1e-9 && (w[2].end - 4.0).abs() < 1e-9, "cut at the next line's start: {w:?}");
    assert!(gaps[0].end <= 4.0);

    // Right after a line, a word that's in it but isn't its last is the gap's ("the" of
    // "enjoy the music", then "The interval…", as heard in the real run).
    let music = [known(1.0, 2.0, "enjoy the music.")];
    assert_eq!(texts(&gap_lines(&music, &[heard_line(&[(2.0, 2.2, "The"), (2.2, 2.6, "interval")])], &mock_source())), ["The interval"]);
    // Just before "Goodbye" but another word: the gap's, cut at the line's start.
    let early = gap_lines(&covered, &[heard_line(&[(3.85, 4.05, "and")])], &mock_source());
    assert_eq!(texts(&early), ["and"]);
    assert!(early[0].end <= 4.0);

    // Between two lines 0.4 s apart, nothing is added: a breath, or the engine's slop.
    let close = [known(1.0, 2.0, "One"), known(2.4, 3.0, "Two")];
    assert!(gap_lines(&close, &[heard_line(&[(2.05, 2.35, "uh")])], &mock_source()).is_empty());
    // Punctuation alone is no line.
    assert!(gap_lines(&covered, &[heard_line(&[(3.0, 3.2, "—")])], &mock_source()).is_empty());
}

#[test]
fn gaps_are_filled_once_and_hand_edited_gap_lines_stay() {
    let srt = crate::subtitles::SubtitleSource { kind: "srt".into(), lang: Some("en".into()), extra: Default::default() };
    let heard = Heard {
        lines: vec![heard_line(&[(1.0, 1.4, "Hello"), (1.4, 1.9, "there."), (3.0, 3.4, "Who's"), (3.4, 3.8, "that?")])],
        model: Some("tiny.en".into()),
        lang: Some("en".into()),
    };
    let current = vec![known(0.9, 2.0, "Hello there.")];
    let a = apply(&current, Some(&srt), EngineKind::WhisperCli, heard.clone(), None, true);
    assert_eq!(texts(&a.lines), ["Hello there.", "Who's that?"]);
    assert_eq!((a.gaps.added, a.gaps.replaced, a.gaps.kept, a.aligned.lines), (1, 0, 0, 1));
    assert!(a.lines[0].machine.is_none() && a.lines[1].machine.is_some());
    assert_eq!(a.source.as_ref().unwrap().kind, "srt", "the tile's source is still its subtitles'");

    // Off: only the known words are timed.
    let off = apply(&current, Some(&srt), EngineKind::WhisperCli, heard.clone(), None, false);
    assert_eq!((texts(&off.lines), off.gaps.added), (vec!["Hello there."], 0));

    // Again: the gap line it added is replaced, not doubled.
    let again = apply(&a.lines, a.source.as_ref(), EngineKind::WhisperCli, heard.clone(), None, true);
    assert_eq!((again.lines.clone(), again.gaps.replaced, again.gaps.added), (a.lines.clone(), 1, 1));

    // A person changed the gap line (placed by hand): it stays as it is, and what was
    // heard over it isn't added again.
    let mut edited = a.lines.clone();
    edited[1].text = "Who is that?".into();
    edited[1].words = edited[1].even_words();
    edited[1].aligned = true;
    let kept = apply(&edited, a.source.as_ref(), EngineKind::WhisperCli, heard.clone(), None, true);
    assert_eq!(kept.lines, edited);
    assert_eq!((kept.gaps.kept, kept.gaps.added, kept.gaps.replaced), (1, 0, 0));
    // Changed in the editor without being placed by hand (it kept the old timing) counts too.
    let mut moved = a.lines.clone();
    moved[1].source_timing = Some(SourceTiming { start: moved[1].start, end: moved[1].end, words: moved[1].words.clone() });
    moved[1].start += 0.1;
    assert_eq!(apply(&moved, a.source.as_ref(), EngineKind::WhisperCli, heard, None, true).lines, moved);
}
