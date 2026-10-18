//! In-process mixer on top of cpal. The UI thread adds and controls voices through a
//! shared [`Mixer`]; the device callback renders them. Fades and volume changes are
//! applied per sample.

use crate::decode::DecodedAudio;
use crate::logic::{envelope_gain, fade_curve, play_range, PlayState};
use crate::model::Tile;
use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VoiceStatus {
    pub state: PlayState,
    /// Seconds into the trimmed region.
    pub position: f64,
    /// Length of the trimmed region in seconds.
    pub length: f64,
}

impl VoiceStatus {
    pub fn remaining(&self) -> f64 {
        (self.length - self.position).max(0.0)
    }
    pub fn progress(&self) -> f32 {
        if self.length > 0.0 {
            (self.position / self.length).clamp(0.0, 1.0) as f32
        } else {
            0.0
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum VoiceState {
    Playing,
    Paused,
    Stopping { left: u64, total: u64, from: f32 },
}

struct Voice {
    tile_id: Uuid,
    audio: Arc<DecodedAudio>,
    /// Trimmed region in source frames.
    seg_start: f64,
    seg_end: f64,
    /// Current read position in source frames.
    pos: f64,
    /// Output frames rendered so far (pauses excluded), for the fade-in.
    elapsed: u64,
    looping: bool,
    volume: f32,
    fade_in: f32,
    fade_out: f32,
    gain: f32,
    state: VoiceState,
}

impl Voice {
    fn src_rate(&self) -> f64 {
        self.audio.sample_rate.max(1) as f64
    }

    fn status(&self) -> VoiceStatus {
        let rate = self.src_rate();
        VoiceStatus {
            state: match self.state {
                VoiceState::Playing => PlayState::Playing,
                VoiceState::Paused => PlayState::Paused,
                VoiceState::Stopping { .. } => PlayState::Stopping,
            },
            position: (self.pos - self.seg_start).max(0.0) / rate,
            length: (self.seg_end - self.seg_start) / rate,
        }
    }

    fn sample_at(&self, pos: f64) -> (f32, f32) {
        let data = &self.audio.data;
        let frames = data.len() / 2;
        let i = (pos.floor() as usize).min(frames.saturating_sub(1));
        let j = (i + 1).min(frames.saturating_sub(1));
        let frac = (pos - i as f64) as f32;
        let (l0, r0) = (data[2 * i], data[2 * i + 1]);
        let (l1, r1) = (data[2 * j], data[2 * j + 1]);
        (l0 + (l1 - l0) * frac, r0 + (r1 - r0) * frac)
    }
}

/// Everything the audio callback needs. Public for tests, which render it directly.
pub struct Mixer {
    voices: Vec<Voice>,
    master: f32,
    out_rate: u32,
    /// Tiles that reached their end on their own since the UI last asked.
    finished: Vec<Uuid>,
    /// One-pole smoothing coefficient for gain changes (~5 ms).
    smooth: f32,
    /// Highest absolute sample per channel (L, R) after master gain and before
    /// clipping, since the UI last asked. Above 1.0 means the output clipped.
    peaks: [f32; 2],
}

impl Mixer {
    pub fn new(out_rate: u32) -> Self {
        let mut m = Mixer { voices: Vec::new(), master: 1.0, out_rate: 48_000, finished: Vec::new(), smooth: 0.0, peaks: [0.0; 2] };
        m.set_output_rate(out_rate);
        m
    }

    pub fn set_output_rate(&mut self, rate: u32) {
        self.out_rate = rate.max(1);
        self.smooth = 1.0 - (-1.0 / (0.005 * self.out_rate as f32)).exp();
    }

    pub fn set_master(&mut self, v: f32) {
        self.master = v.clamp(0.0, 1.0);
    }

    pub fn play(&mut self, tile: &Tile, audio: Arc<DecodedAudio>) {
        self.remove(tile.id);
        if tile.solo {
            let ids: Vec<Uuid> = self.voices.iter().map(|v| v.tile_id).collect();
            for id in ids {
                let fade = self.voices.iter().find(|v| v.tile_id == id).map_or(0.0, |v| v.fade_out).max(0.25);
                self.stop(id, fade);
            }
        }
        let (start, end) = play_range(tile, audio.duration());
        let rate = audio.sample_rate.max(1) as f64;
        if end - start < 1e-3 {
            return;
        }
        let initial = tile.volume * envelope_gain(0.0, 0.0, end - start, tile.fade_in, tile.fade_out, tile.looping);
        self.voices.push(Voice {
            tile_id: tile.id,
            audio,
            seg_start: start * rate,
            seg_end: end * rate,
            pos: start * rate,
            elapsed: 0,
            looping: tile.looping,
            volume: tile.volume,
            fade_in: tile.fade_in,
            fade_out: tile.fade_out,
            gain: initial,
            state: VoiceState::Playing,
        });
    }

    /// Live parameter changes from the inspector (volume, fades, loop).
    pub fn update_params(&mut self, tile: &Tile) {
        if let Some(v) = self.voices.iter_mut().find(|v| v.tile_id == tile.id) {
            v.volume = tile.volume;
            v.fade_in = tile.fade_in;
            v.fade_out = tile.fade_out;
            v.looping = tile.looping;
        }
    }

    pub fn stop(&mut self, id: Uuid, fade: f32) {
        let rate = self.out_rate as f32;
        let Some(idx) = self.voices.iter().position(|v| v.tile_id == id) else { return };
        let v = &mut self.voices[idx];
        let frames = (fade.max(0.0) * rate) as u64;
        if frames == 0 || v.state == VoiceState::Paused {
            self.voices.remove(idx);
        } else if !matches!(v.state, VoiceState::Stopping { .. }) {
            v.state = VoiceState::Stopping { left: frames, total: frames, from: v.gain };
        }
    }

    pub fn stop_all(&mut self, fade: f32) {
        let ids: Vec<Uuid> = self.voices.iter().map(|v| v.tile_id).collect();
        for id in ids {
            self.stop(id, fade);
        }
    }

    pub fn pause(&mut self, id: Uuid) {
        if let Some(v) = self.voices.iter_mut().find(|v| v.tile_id == id && v.state == VoiceState::Playing) {
            v.state = VoiceState::Paused;
        }
    }

    pub fn resume(&mut self, id: Uuid) {
        if let Some(v) = self.voices.iter_mut().find(|v| v.tile_id == id && v.state == VoiceState::Paused) {
            v.state = VoiceState::Playing;
        }
    }

    fn remove(&mut self, id: Uuid) {
        self.voices.retain(|v| v.tile_id != id);
    }

    pub fn state(&self, id: Uuid) -> PlayState {
        self.voices.iter().find(|v| v.tile_id == id).map_or(PlayState::Stopped, |v| v.status().state)
    }

    pub fn statuses(&self) -> HashMap<Uuid, VoiceStatus> {
        self.voices.iter().map(|v| (v.tile_id, v.status())).collect()
    }

    /// Per-channel peaks since the last call, then reset.
    pub fn take_peaks(&mut self) -> [f32; 2] {
        std::mem::take(&mut self.peaks)
    }

    pub fn output_rate(&self) -> u32 {
        self.out_rate
    }

    pub fn take_finished(&mut self) -> Vec<Uuid> {
        std::mem::take(&mut self.finished)
    }

    /// Mix every voice into `out` (interleaved, `channels` wide), replacing its contents.
    pub fn render(&mut self, out: &mut [f32], channels: usize) {
        out.iter_mut().for_each(|s| *s = 0.0);
        let channels = channels.max(1);
        let out_rate = self.out_rate as f64;
        let smooth = self.smooth;
        let mut finished = Vec::new();

        self.voices.retain_mut(|v| {
            if v.state == VoiceState::Paused {
                return true;
            }
            let src_rate = v.src_rate();
            let step = src_rate / out_rate;
            let length = (v.seg_end - v.seg_start) / src_rate;
            for frame in out.chunks_exact_mut(channels) {
                let position = (v.pos - v.seg_start) / src_rate;
                let elapsed = v.elapsed as f64 / out_rate;
                let mut target = v.volume * envelope_gain(elapsed, position, length, v.fade_in, v.fade_out, v.looping);
                if let VoiceState::Stopping { left, total, from } = &mut v.state {
                    if *left == 0 {
                        return false;
                    }
                    target = *from * fade_curve(*left as f32 / *total as f32);
                    *left -= 1;
                }
                v.gain += (target - v.gain) * smooth;
                let (l, r) = v.sample_at(v.pos);
                if channels == 1 {
                    frame[0] += (l + r) * 0.5 * v.gain;
                } else {
                    frame[0] += l * v.gain;
                    frame[1] += r * v.gain;
                }
                v.pos += step;
                v.elapsed += 1;
                if v.pos >= v.seg_end {
                    if v.looping {
                        v.pos = v.seg_start + (v.pos - v.seg_end);
                    } else {
                        finished.push(v.tile_id);
                        return false;
                    }
                }
            }
            true
        });

        let master = self.master;
        for frame in out.chunks_exact_mut(channels) {
            for (ch, s) in frame.iter_mut().enumerate() {
                *s *= master;
                let meter = if channels == 1 { 0..2 } else { ch.min(2)..(ch + 1).min(2) };
                for m in meter {
                    self.peaks[m] = self.peaks[m].max(s.abs());
                }
                *s = s.clamp(-1.0, 1.0);
            }
        }
        self.finished.extend(finished);
    }
}

fn lock(m: &Mutex<Mixer>) -> MutexGuard<'_, Mixer> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The output stream, kept alive while the engine plays.
///
/// cpal marks `Stream` as not `Send` on every platform because some backends (Android
/// AAudio, wasm) can't move it between threads. On CoreAudio and ALSA, the only
/// backends this app builds for, the handle is only held and eventually dropped, which
/// is safe from any thread. It has to be `Send` because the Controller that owns the
/// engine is shared with the control socket's threads (control.rs).
struct OutputStream(#[allow(dead_code)] cpal::Stream);

// SAFETY: see above; never touched except by Drop, and only built on macOS and Linux.
#[cfg(any(target_os = "macos", target_os = "linux"))]
unsafe impl Send for OutputStream {}

/// Owns the output stream and the shared mixer.
pub struct Engine {
    mixer: Arc<Mutex<Mixer>>,
    stream: Option<OutputStream>,
    stream_failed: Arc<AtomicBool>,
    pub device_name: Option<String>,
    pub error: Option<String>,
}

impl Engine {
    /// Open `device` (by name), falling back to the system default. Never fails: on error
    /// the engine runs silently and reports why in `error`.
    pub fn new(device: Option<&str>) -> Self {
        let mut e = Engine {
            mixer: Arc::new(Mutex::new(Mixer::new(48_000))),
            stream: None,
            stream_failed: Arc::new(AtomicBool::new(false)),
            device_name: None,
            error: None,
        };
        e.open(device);
        e
    }

    /// A silent engine that never touches an audio device (headless runs).
    pub fn disabled(reason: &str) -> Self {
        Engine {
            mixer: Arc::new(Mutex::new(Mixer::new(48_000))),
            stream: None,
            stream_failed: Arc::new(AtomicBool::new(false)),
            device_name: None,
            error: Some(reason.to_owned()),
        }
    }

    pub fn output_devices() -> Vec<String> {
        let host = cpal::default_host();
        let mut names: Vec<String> = host
            .output_devices()
            .map(|it| it.filter_map(|d| d.name().ok()).collect())
            .unwrap_or_default();
        names.dedup();
        names
    }

    pub fn open(&mut self, device: Option<&str>) {
        self.stream = None;
        lock(&self.mixer).stop_all(0.0);
        self.stream_failed.store(false, Ordering::Relaxed);
        match self.try_open(device) {
            Ok((stream, name)) => {
                self.stream = Some(OutputStream(stream));
                self.device_name = Some(name);
                self.error = None;
            }
            Err(err) => {
                self.device_name = None;
                self.error = Some(format!("Audio output unavailable: {err}"));
            }
        }
    }

    fn try_open(&self, wanted: Option<&str>) -> Result<(cpal::Stream, String)> {
        let host = cpal::default_host();
        let device = wanted
            .and_then(|w| host.output_devices().ok()?.find(|d| d.name().map(|n| n == w).unwrap_or(false)))
            .or_else(|| host.default_output_device())
            .ok_or_else(|| anyhow!("no output device found"))?;
        let name = device.name().unwrap_or_else(|_| "Unknown device".into());
        let supported = device.default_output_config()?;
        let format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();
        lock(&self.mixer).set_output_rate(config.sample_rate.0);
        let stream = match format {
            cpal::SampleFormat::F32 => self.build::<f32>(&device, &config)?,
            cpal::SampleFormat::I16 => self.build::<i16>(&device, &config)?,
            cpal::SampleFormat::U16 => self.build::<u16>(&device, &config)?,
            cpal::SampleFormat::I32 => self.build::<i32>(&device, &config)?,
            other => return Err(anyhow!("unsupported sample format {other}")),
        };
        stream.play()?;
        Ok((stream, name))
    }

    fn build<T>(&self, device: &cpal::Device, config: &cpal::StreamConfig) -> Result<cpal::Stream>
    where
        T: cpal::SizedSample + cpal::FromSample<f32>,
    {
        let channels = config.channels as usize;
        let mixer = self.mixer.clone();
        let failed = self.stream_failed.clone();
        let mut scratch: Vec<f32> = Vec::new();
        let stream = device.build_output_stream(
            config,
            move |data: &mut [T], _| {
                scratch.resize(data.len(), 0.0);
                lock(&mixer).render(&mut scratch, channels);
                for (d, s) in data.iter_mut().zip(&scratch) {
                    *d = T::from_sample(*s);
                }
            },
            move |err| {
                eprintln!("{}: audio stream error: {err}", crate::brand::SLUG);
                failed.store(true, Ordering::Relaxed);
            },
            None,
        )?;
        Ok(stream)
    }

    /// True once the device callback reported an error (e.g. the device went away).
    pub fn stream_failed(&self) -> bool {
        self.stream_failed.load(Ordering::Relaxed)
    }

    pub fn play(&self, tile: &Tile, audio: Arc<DecodedAudio>) {
        lock(&self.mixer).play(tile, audio);
    }
    pub fn stop(&self, id: Uuid, fade: f32) {
        lock(&self.mixer).stop(id, fade);
    }
    pub fn stop_all(&self, fade: f32) {
        lock(&self.mixer).stop_all(fade);
    }
    pub fn pause(&self, id: Uuid) {
        lock(&self.mixer).pause(id);
    }
    pub fn resume(&self, id: Uuid) {
        lock(&self.mixer).resume(id);
    }
    pub fn update_params(&self, tile: &Tile) {
        lock(&self.mixer).update_params(tile);
    }
    pub fn set_master(&self, v: f32) {
        lock(&self.mixer).set_master(v);
    }
    pub fn state(&self, id: Uuid) -> PlayState {
        lock(&self.mixer).state(id)
    }
    pub fn statuses(&self) -> HashMap<Uuid, VoiceStatus> {
        lock(&self.mixer).statuses()
    }
    pub fn take_peaks(&self) -> [f32; 2] {
        lock(&self.mixer).take_peaks()
    }
    pub fn sample_rate(&self) -> u32 {
        lock(&self.mixer).output_rate()
    }
    pub fn take_finished(&self) -> Vec<Uuid> {
        lock(&self.mixer).take_finished()
    }

    /// Without a device nothing drains the mixer, so advance it here (keeps progress
    /// bars and auto-advance working in headless runs).
    pub fn tick_without_device(&self, seconds: f64) {
        if self.stream.is_some() {
            return;
        }
        let mut m = lock(&self.mixer);
        let frames = (seconds * m.out_rate as f64) as usize;
        let mut buf = vec![0.0; frames.min(48_000) * 2];
        m.render(&mut buf, 2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 1000;

    /// 1 s of constant 0.5 at 1 kHz, so gains are easy to read off the output.
    fn dc(seconds: f64) -> Arc<DecodedAudio> {
        let frames = (seconds * RATE as f64) as usize;
        Arc::new(DecodedAudio { sample_rate: RATE, data: vec![0.5; frames * 2], art: None })
    }

    fn render(m: &mut Mixer, frames: usize) -> Vec<f32> {
        let mut out = vec![0.0; frames * 2];
        m.render(&mut out, 2);
        out.chunks(2).map(|f| f[0]).collect()
    }

    fn tile() -> Tile {
        Tile { file: Some("x".into()), ..Default::default() }
    }

    #[test]
    fn plays_to_the_end_and_reports_finished() {
        let mut m = Mixer::new(RATE);
        let t = tile();
        m.play(&t, dc(1.0));
        let out = render(&mut m, 1500);
        assert!((out[500] - 0.5).abs() < 1e-3);
        assert_eq!(out[1200], 0.0);
        assert_eq!(m.state(t.id), PlayState::Stopped);
        assert_eq!(m.take_finished(), vec![t.id]);
    }

    #[test]
    fn trim_limits_playback() {
        let mut m = Mixer::new(RATE);
        let t = Tile { start: 0.2, end: Some(0.5), ..tile() };
        m.play(&t, dc(1.0));
        let status = m.statuses()[&t.id];
        assert!((status.length - 0.3).abs() < 1e-9);
        let out = render(&mut m, 400);
        assert!(out[250] > 0.4);
        assert_eq!(out[350], 0.0);
    }

    #[test]
    fn loops_until_stopped_with_fade() {
        let mut m = Mixer::new(RATE);
        let t = Tile { looping: true, ..tile() };
        m.play(&t, dc(0.1));
        let out = render(&mut m, 1000);
        assert!(out[999] > 0.49, "still playing after ten passes");
        assert!(m.take_finished().is_empty());
        m.stop(t.id, 0.5);
        assert_eq!(m.state(t.id), PlayState::Stopping);
        let out = render(&mut m, 600);
        assert!(out[100] > out[400], "fading out");
        assert_eq!(out[550], 0.0);
        assert_eq!(m.state(t.id), PlayState::Stopped);
        assert!(m.take_finished().is_empty(), "a user stop is not a natural finish");
    }

    #[test]
    fn fade_in_ramps_up() {
        let mut m = Mixer::new(RATE);
        let t = Tile { fade_in: 0.5, ..tile() };
        m.play(&t, dc(1.0));
        let out = render(&mut m, 800);
        assert!(out[10] < 0.01);
        assert!(out[250] > 0.05 && out[250] < 0.25);
        assert!((out[700] - 0.5).abs() < 0.01);
    }

    #[test]
    fn pause_holds_position() {
        let mut m = Mixer::new(RATE);
        let t = tile();
        m.play(&t, dc(1.0));
        render(&mut m, 300);
        m.pause(t.id);
        let silent = render(&mut m, 300);
        assert!(silent.iter().all(|s| *s == 0.0));
        let pos = m.statuses()[&t.id].position;
        assert!((pos - 0.3).abs() < 1e-6);
        m.resume(t.id);
        render(&mut m, 100);
        assert!((m.statuses()[&t.id].position - 0.4).abs() < 1e-6);
    }

    #[test]
    fn solo_fades_others_and_master_scales() {
        let mut m = Mixer::new(RATE);
        let a = tile();
        let b = Tile { solo: true, ..tile() };
        m.play(&a, dc(1.0));
        m.play(&b, dc(1.0));
        assert_eq!(m.state(a.id), PlayState::Stopping);
        assert_eq!(m.state(b.id), PlayState::Playing);
        m.set_master(0.5);
        let out = render(&mut m, 600);
        assert!((out[500] - 0.25).abs() < 0.01, "only b at half master: {}", out[500]);
    }

    #[test]
    fn resamples_to_output_rate() {
        // 1 s source at 1 kHz played on a 2 kHz device lasts 2000 output frames.
        let mut m = Mixer::new(RATE * 2);
        let t = tile();
        m.play(&t, dc(1.0));
        let out = render(&mut m, 2100);
        assert!(out[1990] > 0.4);
        assert_eq!(out[2050], 0.0);
    }

    #[test]
    fn reports_peaks_per_channel_after_master() {
        let mut m = Mixer::new(RATE);
        m.set_master(0.5);
        m.play(&tile(), dc(1.0));
        render(&mut m, 100);
        let [l, r] = m.take_peaks();
        assert!((l - 0.25).abs() < 0.01 && (r - 0.25).abs() < 0.01, "{l} {r}");
        assert_eq!(m.take_peaks(), [0.0, 0.0], "reset after reading");
        // Four voices at full master sum past full scale: the peak shows the clip.
        m.set_master(1.0);
        for _ in 0..3 {
            m.play(&tile(), dc(1.0));
        }
        render(&mut m, 10);
        assert!(m.take_peaks()[0] > 1.0);
    }

    #[test]
    fn output_is_clamped() {
        let mut m = Mixer::new(RATE);
        for _ in 0..4 {
            m.play(&tile(), dc(1.0));
        }
        let out = render(&mut m, 10);
        assert!(out.iter().all(|s| *s <= 1.0));
    }
}
