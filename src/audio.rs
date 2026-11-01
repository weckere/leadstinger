//! In-process mixer on top of cpal. The UI thread adds and controls voices through a
//! shared [`Mixer`]; the device callback renders them. Fades and volume changes are
//! applied per sample.

use crate::decode::DecodedAudio;
use crate::logic::{db_to_gain, envelope_gain, play_range, vamp_range, PlayState};
use crate::model::{channel_gains, Duck, FadeCurve, Tile, DECLICK_SECONDS, MAIN_OUTPUT, DUCK_DB_DEFAULT};
use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VoiceStatus {
    pub state: PlayState,
    /// Seconds into the trimmed region, as heard (a fast tile gets there sooner).
    pub position: f64,
    /// Length of the trimmed region in seconds, as heard.
    pub length: f64,
    /// The tile's speed: `position * speed` is the place in the file.
    pub speed: f64,
    /// Looping its vamp section, waiting for Play On (D-153).
    pub vamping: bool,
    /// Had a vamp and is past it: let go, or jumped beyond it.
    pub played_on: bool,
    /// Seconds into the file where the trimmed region starts: `from + position * speed`
    /// is where it's playing (a stretch preview starts away from the trim, D-104).
    pub from: f64,
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
    /// Fading to silence (the de-click ramp, a raised cosine), then paused back at `at`,
    /// the source frame where the pause was pressed. Already reported as paused.
    Pausing { left: u64, total: u64, from: f32, at: f64 },
    /// Fading to silence along `curve`, then removed.
    Stopping { left: u64, total: u64, from: f32, curve: FadeCurve },
}

struct Voice {
    tile_id: Uuid,
    audio: Arc<DecodedAudio>,
    /// Trimmed region in source frames.
    seg_start: f64,
    seg_end: f64,
    /// Current read position in source frames.
    pos: f64,
    /// The vamp section in source frames, looped until `released`.
    vamp: Option<(f64, f64)>,
    released: bool,
    /// The tail of a jump: fades out unseen (no status, no finish).
    ghost: bool,
    /// After a jump: frames left of the fade back in, and its length.
    xfade: Option<(u64, u64)>,
    /// Output frames rendered so far (pauses excluded), for the fade-in.
    elapsed: u64,
    looping: bool,
    /// The tile's linear gain (volume × dB gain; above 1 when boosted).
    volume: f32,
    fade_in: f32,
    fade_out: f32,
    /// Seconds to fade when stopped (0: at once, over the 0.1 s de-click).
    stop_fade: f32,
    curve: FadeCurve,
    duck: Duck,
    speed: f64,
    gain: f32,
    state: VoiceState,
    /// The send's linear level, on top of `volume`.
    send: f32,
    /// The tile's channel mix, (left, right) per native channel; `None` plays the
    /// downmix.
    mix: Option<Arc<[(f32, f32)]>>,
    /// Highest absolute sample this voice added since the UI last asked, before master
    /// gain (the tile's meter).
    peak: f32,
    /// The voice of the tile's first send (its Output), or the one on Main standing in
    /// for it: when its output's device is lost it moves to Main.
    lead: bool,
}

impl Voice {
    /// Source frames per second of playback: the file's rate times the speed.
    fn src_rate(&self) -> f64 {
        self.audio.sample_rate.max(1) as f64
    }

    fn status(&self) -> VoiceStatus {
        let rate = self.src_rate() * self.speed;
        VoiceStatus {
            state: match self.state {
                VoiceState::Playing => PlayState::Playing,
                VoiceState::Paused | VoiceState::Pausing { .. } => PlayState::Paused,
                VoiceState::Stopping { .. } => PlayState::Stopping,
            },
            position: (self.held_pos() - self.seg_start).max(0.0) / rate,
            length: (self.seg_end - self.seg_start) / rate,
            vamping: self.vamp.is_some() && !self.released,
            played_on: self.vamp.is_some() && self.released,
            speed: self.speed,
            from: self.seg_start / self.src_rate(),
        }
    }

    /// Where the voice is, as reported: a pausing voice is already where it will resume.
    fn held_pos(&self) -> f64 {
        match self.state {
            VoiceState::Pausing { at, .. } => at,
            _ => self.pos,
        }
    }

    fn sample_at(&self, pos: f64) -> (f32, f32) {
        if let (Some(mix), Some(native)) = (&self.mix, &self.audio.native) {
            return mixed_at(&native.data, mix, pos);
        }
        let data = &self.audio.data;
        let frames = data.len() / 2;
        let i = (pos.floor() as usize).min(frames.saturating_sub(1));
        let j = (i + 1).min(frames.saturating_sub(1));
        let frac = (pos - i as f64) as f32;
        let (l0, r0) = (data[2 * i], data[2 * i + 1]);
        let (l1, r1) = (data[2 * j], data[2 * j + 1]);
        (l0 + (l1 - l0) * frac, r0 + (r1 - r0) * frac)
    }

    /// Carry the counts kept in output frames (the fade-in clock, a stop, a crossfade)
    /// over to a mixer at another output rate.
    fn rescale(&mut self, from: u32, to: u32) {
        if from == to {
            return;
        }
        let k = to.max(1) as f64 / from.max(1) as f64;
        let scale = |n: u64| ((n as f64 * k) as u64).max(1);
        self.elapsed = (self.elapsed as f64 * k) as u64;
        if let VoiceState::Stopping { left, total, .. } | VoiceState::Pausing { left, total, .. } = &mut self.state {
            *left = scale(*left);
            *total = scale(*total);
        }
        if let Some((left, total)) = &mut self.xfade {
            *left = scale(*left);
            *total = scale(*total);
        }
    }
}

/// The tile's channel mix of the native channels at `pos` (interpolated like the
/// downmix), as a stereo pair.
fn mixed_at(data: &[f32], mix: &[(f32, f32)], pos: f64) -> (f32, f32) {
    let width = mix.len().max(1);
    let frames = data.len() / width;
    let i = (pos.floor() as usize).min(frames.saturating_sub(1));
    let j = (i + 1).min(frames.saturating_sub(1));
    let frac = (pos - i as f64) as f32;
    let (a, b) = (&data[width * i..width * (i + 1)], &data[width * j..width * (j + 1)]);
    let (mut l, mut r) = (0.0, 0.0);
    for ((s0, s1), (gl, gr)) in a.iter().zip(b).zip(mix) {
        let s = s0 + (s1 - s0) * frac;
        l += s * gl;
        r += s * gr;
    }
    (l, r)
}

/// The voice's channel mix for this audio, if the tile has one that fits it.
fn mix_for(tile: &Tile, audio: &DecodedAudio) -> Option<Arc<[(f32, f32)]>> {
    audio.native.as_ref().and_then(|n| channel_gains(&tile.channel_mix, n.count())).map(Arc::from)
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
    /// The output limiter's current gain (1 = not limiting).
    limit: f32,
    /// The limiter's per-frame release coefficient (~150 ms).
    release: f32,
    duck: Ducker,
    /// Set by the input stream while a voice is above the threshold (voice ducking).
    voice: Arc<AtomicBool>,
    /// One bit per mixer, shared by the engine's mixers: set while a tile that ducks
    /// sounds here, so a bed sent to several outputs dips on all of them together.
    duck_bus: Arc<AtomicU64>,
    duck_bit: u64,
    /// Frames of the last block that any voice sounded in (an offline render trims the
    /// silence after its voice ends to this).
    sounded: usize,
}

/// The duck envelope: one gain, shared by every ducked voice, that falls toward
/// `level` while something ducks and climbs back to 1 when nothing does.
struct Ducker {
    /// Linear gain at full duck.
    level: f32,
    attack: f32,
    release: f32,
    /// Seconds, kept so the coefficients can follow a new output rate.
    attack_s: f32,
    release_s: f32,
    /// The talk-over (`duck.hold`).
    hold: bool,
    /// Whether anything ducked in the last block.
    active: bool,
    gain: f32,
    /// The envelope for the block being rendered, one gain per frame.
    env: Vec<f32>,
}

impl Ducker {
    fn coefficients(&mut self, rate: u32) {
        let coef = |s: f32| if s <= 0.0 { 1.0 } else { 1.0 - (-1.0 / (s * rate as f32)).exp() };
        self.attack = coef(self.attack_s);
        self.release = coef(self.release_s);
    }

    /// Fill `env` for `frames` frames.
    fn block(&mut self, frames: usize, active: bool) {
        self.active = active;
        let target = if active { self.level } else { 1.0 };
        self.env.clear();
        for _ in 0..frames {
            let c = if target < self.gain { self.attack } else { self.release };
            self.gain += (target - self.gain) * c;
            self.env.push(self.gain);
        }
    }
}

impl Mixer {
    pub fn new(out_rate: u32) -> Self {
        let duck = Ducker {
            level: db_to_gain(DUCK_DB_DEFAULT),
            attack: 1.0,
            release: 1.0,
            attack_s: 0.05,
            release_s: 0.4,
            hold: false,
            active: false,
            gain: 1.0,
            env: Vec::with_capacity(8192),
        };
        let mut m = Mixer {
            voices: Vec::new(),
            master: 1.0,
            out_rate: 48_000,
            finished: Vec::new(),
            smooth: 0.0,
            peaks: [0.0; 2],
            limit: 1.0,
            release: 0.0,
            duck,
            voice: Arc::new(AtomicBool::new(false)),
            duck_bus: Arc::new(AtomicU64::new(0)),
            duck_bit: 1,
            sounded: 0,
        };
        m.set_output_rate(out_rate);
        m
    }

    pub fn set_output_rate(&mut self, rate: u32) {
        self.out_rate = rate.max(1);
        self.smooth = 1.0 - (-1.0 / (0.005 * self.out_rate as f32)).exp();
        self.release = 1.0 - (-1.0 / (0.15 * self.out_rate as f32)).exp();
        self.duck.coefficients(self.out_rate);
    }

    /// How ducked tiles dip: `db` below their level, over `attack` seconds, coming back
    /// over `release`.
    pub fn set_duck(&mut self, db: f32, attack: f32, release: f32) {
        self.duck.level = db_to_gain(db.min(0.0));
        self.duck.attack_s = attack.max(0.0);
        self.duck.release_s = release.max(0.0);
        self.duck.coefficients(self.out_rate);
    }

    /// The talk-over: hold the duck down while `on`.
    pub fn set_duck_hold(&mut self, on: bool) {
        self.duck.hold = on;
    }

    pub fn duck_hold(&self) -> bool {
        self.duck.hold
    }

    /// Whether ducked tiles are dipped (or dipping) right now.
    pub fn ducking(&self) -> bool {
        self.duck.active || self.duck.hold
    }

    /// The flag the input stream raises while a voice is speaking.
    pub fn voice_flag(&self) -> Arc<AtomicBool> {
        self.voice.clone()
    }

    pub fn set_master(&mut self, v: f32) {
        self.master = v.clamp(0.0, 1.0);
    }

    pub fn play(&mut self, tile: &Tile, audio: Arc<DecodedAudio>) {
        self.play_send(tile, audio, 1.0);
    }

    /// Stop every voice but `keep`'s, each with its own stop fade (a solo tile starting).
    fn solo(&mut self, keep: Uuid) {
        let ids: Vec<(Uuid, f32)> = self.voices.iter().filter(|v| v.tile_id != keep && !v.ghost).map(|v| (v.tile_id, v.stop_fade)).collect();
        for (id, fade) in ids {
            self.stop(id, fade);
        }
    }

    /// Start one of a tile's voices, at its send's linear level `send`.
    pub fn play_send(&mut self, tile: &Tile, audio: Arc<DecodedAudio>, send: f32) {
        self.retire(tile.id);
        if tile.solo {
            self.solo(tile.id);
        }
        let (start, end) = play_range(tile, audio.duration());
        let rate = audio.sample_rate.max(1) as f64;
        if end - start < 1e-3 {
            return;
        }
        let initial = tile.gain() * send * envelope_gain(0.0, 0.0, end - start, tile.fade_in, tile.fade_out, tile.looping, tile.fade_curve);
        let vamp = vamp_range(tile, audio.duration()).map(|(a, b)| (a * rate, b * rate));
        let mix = mix_for(tile, &audio);
        self.voices.push(Voice {
            tile_id: tile.id,
            audio,
            seg_start: start * rate,
            seg_end: end * rate,
            pos: start * rate,
            vamp,
            released: false,
            ghost: false,
            xfade: None,
            elapsed: 0,
            looping: tile.looping,
            volume: tile.gain(),
            fade_in: tile.fade_in,
            fade_out: tile.fade_out,
            stop_fade: tile.stop_fade,
            curve: tile.fade_curve,
            duck: tile.duck,
            speed: tile.speed.clamp(0.5, 2.0) as f64,
            gain: initial,
            state: VoiceState::Playing,
            send,
            mix,
            peak: 0.0,
            lead: false,
        });
    }

    /// Live parameter changes from the inspector (volume, gain, fades, loop, ducking).
    pub fn update_params(&mut self, tile: &Tile) {
        self.update_send(tile, None);
    }

    /// The same, with the send's new linear level when it has one.
    pub fn update_send(&mut self, tile: &Tile, send: Option<f32>) {
        if let Some(v) = self.live_mut(tile.id) {
            if let Some(g) = send {
                v.send = g;
            }
            v.volume = tile.gain();
            v.mix = mix_for(tile, &v.audio);
            v.fade_in = tile.fade_in;
            v.fade_out = tile.fade_out;
            v.stop_fade = tile.stop_fade;
            v.looping = tile.looping;
            v.duck = tile.duck;
            v.curve = tile.fade_curve;
            v.speed = tile.speed.clamp(0.5, 2.0) as f64;
            if !v.released {
                let rate = v.src_rate();
                v.vamp = vamp_range(tile, v.audio.duration()).map(|(a, b)| (a * rate, b * rate));
            }
        }
    }

    /// The tile's sounding voice (not a ghost).
    fn live_mut(&mut self, id: Uuid) -> Option<&mut Voice> {
        self.voices.iter_mut().find(|v| v.tile_id == id && !v.ghost)
    }

    /// Frames of the de-click ramp at this output rate (at least one).
    fn declick_frames(&self) -> u64 {
        ((DECLICK_SECONDS * self.out_rate as f32) as u64).max(1)
    }

    /// Stop a tile over `fade` seconds along its fade curve. Anything shorter than the
    /// de-click ramp (0.1 s: no stop fade, Stop All at once) gets that ramp, a raised
    /// cosine, so it can't click or pop. A stop that would finish sooner than one already
    /// running takes over (the second press of Esc); a slower one leaves it alone. A
    /// paused voice is silent, so it goes; a pausing one finishes its ramp as a stop.
    pub fn stop(&mut self, id: Uuid, fade: f32) {
        let declick = self.declick_frames();
        let wanted = (fade.max(0.0) * self.out_rate as f32) as u64;
        let Some(idx) = self.voices.iter().position(|v| v.tile_id == id && !v.ghost) else { return };
        let v = &mut self.voices[idx];
        let (frames, curve) = if wanted <= declick { (declick, FadeCurve::SCurve) } else { (wanted, v.curve) };
        match v.state {
            VoiceState::Paused => {
                self.voices.remove(idx);
            }
            VoiceState::Stopping { left, .. } if left <= frames => {}
            VoiceState::Pausing { left, total, from, .. } if left <= frames => {
                v.state = VoiceState::Stopping { left, total, from, curve: FadeCurve::SCurve };
            }
            _ => v.state = VoiceState::Stopping { left: frames, total: frames, from: v.gain, curve },
        }
    }

    pub fn stop_all(&mut self, fade: f32) {
        let ids: Vec<Uuid> = self.voices.iter().filter(|v| !v.ghost).map(|v| v.tile_id).collect();
        for id in ids {
            self.stop(id, fade);
        }
    }

    /// Move a playing voice to `to` seconds into the file, with a short crossfade: the old
    /// read head fades out as a ghost while the voice comes back in at the new place.
    /// Jumping past the vamp section releases it. False if the tile isn't sounding.
    pub fn jump(&mut self, id: Uuid, to: f64) -> bool {
        let fade = (0.02 * self.out_rate as f64) as u64;
        let Some(i) = self.voices.iter().position(|v| v.tile_id == id && !v.ghost && v.state == VoiceState::Playing) else {
            return false;
        };
        let v = &mut self.voices[i];
        // A linear, equal-gain crossfade: the two read heads always add up to one.
        let fade = fade.max(1);
        let ghost = Voice {
            ghost: true,
            state: VoiceState::Stopping { left: fade, total: fade, from: v.gain, curve: FadeCurve::Linear },
            audio: v.audio.clone(),
            mix: v.mix.clone(),
            vamp: None,
            xfade: None,
            ..*v
        };
        let rate = v.src_rate();
        v.pos = (to * rate).clamp(v.seg_start, (v.seg_end - 1.0).max(v.seg_start));
        v.xfade = Some((fade, fade));
        if v.vamp.is_some_and(|(_, b)| v.pos >= b) {
            v.released = true;
        }
        self.voices.push(ghost);
        true
    }

    /// Let a vamping voice play on past its vamp section. True if it was vamping.
    pub fn play_on_vamp(&mut self, id: Uuid) -> bool {
        let vamping = |v: &&mut Voice| v.tile_id == id && !v.ghost && !matches!(v.state, VoiceState::Stopping { .. }) && v.vamp.is_some() && !v.released;
        match self.voices.iter_mut().find(vamping) {
            Some(v) => {
                v.released = true;
                true
            }
            None => false,
        }
    }

    /// Pause over the de-click ramp; it resumes from where it was paused.
    pub fn pause(&mut self, id: Uuid) {
        let declick = self.declick_frames();
        if let Some(v) = self.live_mut(id).filter(|v| v.state == VoiceState::Playing) {
            v.state = VoiceState::Pausing { left: declick, total: declick, from: v.gain, at: v.pos };
        }
    }

    /// Resume a paused voice where it was paused. Its gain comes back up through the
    /// smoothing (~5 ms) from the silence the pause left, so it doesn't click either.
    pub fn resume(&mut self, id: Uuid) {
        if let Some(v) = self.live_mut(id) {
            match v.state {
                VoiceState::Paused => v.state = VoiceState::Playing,
                VoiceState::Pausing { at, .. } => {
                    v.pos = at;
                    v.state = VoiceState::Playing;
                }
                _ => {}
            }
        }
    }

    /// Make way for a restart: the tile's sounding voice becomes a ghost that ramps out
    /// over the de-click time (or the rest of its own stop or pause, if that's sooner)
    /// under the new voice, which starts at once, so a One-shot restart doesn't cut the
    /// old one mid-cycle. A paused voice is silent and just goes.
    fn retire(&mut self, id: Uuid) {
        let declick = self.declick_frames();
        self.voices.retain(|v| !(v.tile_id == id && !v.ghost && v.state == VoiceState::Paused));
        for v in self.voices.iter_mut().filter(|v| v.tile_id == id && !v.ghost) {
            v.ghost = true;
            v.xfade = None;
            v.state = match v.state {
                VoiceState::Stopping { left, total, from, curve } if left <= declick => VoiceState::Stopping { left, total, from, curve },
                VoiceState::Pausing { left, total, from, .. } if left <= declick => VoiceState::Stopping { left, total, from, curve: FadeCurve::SCurve },
                _ => VoiceState::Stopping { left: declick, total: declick, from: v.gain, curve: FadeCurve::SCurve },
            };
        }
    }

    pub fn state(&self, id: Uuid) -> PlayState {
        self.voices.iter().find(|v| v.tile_id == id && !v.ghost).map_or(PlayState::Stopped, |v| v.status().state)
    }

    /// The tile is looping its vamp section (playing or paused there; a restart's
    /// fading tail doesn't count).
    pub fn vamping(&self, id: Uuid) -> bool {
        self.voices.iter().any(|v| v.tile_id == id && !v.ghost && !matches!(v.state, VoiceState::Stopping { .. }) && v.status().vamping)
    }

    pub fn statuses(&self) -> HashMap<Uuid, VoiceStatus> {
        self.voices.iter().filter(|v| !v.ghost).map(|v| (v.tile_id, v.status())).collect()
    }

    /// Per-channel peaks since the last call, then reset.
    pub fn take_peaks(&mut self) -> [f32; 2] {
        std::mem::take(&mut self.peaks)
    }

    /// Each sounding tile's peak since the last call, after master gain like Main's meter,
    /// merged into `into` (the loudest of a tile's voices: a restart's tail, its sends),
    /// then reset.
    pub fn take_levels(&mut self, into: &mut HashMap<Uuid, f32>) {
        for v in &mut self.voices {
            let peak = std::mem::take(&mut v.peak) * self.master;
            let level = into.entry(v.tile_id).or_insert(0.0);
            *level = level.max(peak);
        }
    }

    pub fn output_rate(&self) -> u32 {
        self.out_rate
    }

    pub fn take_finished(&mut self) -> Vec<Uuid> {
        std::mem::take(&mut self.finished)
    }

    /// Nothing left to play (src/render.rs renders until then).
    pub fn is_idle(&self) -> bool {
        self.voices.is_empty()
    }

    /// How many frames of the last [`render`](Self::render) block a voice sounded in:
    /// the whole block while any plays on, fewer once the last one ended inside it.
    pub fn sounded(&self) -> usize {
        self.sounded
    }

    /// Mix every voice into `out` (interleaved, `channels` wide), replacing its contents.
    pub fn render(&mut self, out: &mut [f32], channels: usize) {
        out.iter_mut().for_each(|s| *s = 0.0);
        let channels = channels.max(1);
        let out_rate = self.out_rate as f64;
        let smooth = self.smooth;
        let mut finished = Vec::new();

        // Anything that ducks and is sounding (a fade-out included), on any output, holds
        // the duck down.
        if self.voices.iter().any(|v| v.duck == Duck::Ducks && v.state != VoiceState::Paused) {
            self.duck_bus.fetch_or(self.duck_bit, Ordering::Relaxed);
        } else {
            self.duck_bus.fetch_and(!self.duck_bit, Ordering::Relaxed);
        }
        let ducks = self.duck.hold || self.voice.load(Ordering::Relaxed) || self.duck_bus.load(Ordering::Relaxed) != 0;
        self.duck.block(out.len() / channels, ducks);
        let env = &self.duck.env;
        let frames = out.len() / channels;
        let mut sounded = 0;

        self.voices.retain_mut(|v| {
            if v.state == VoiceState::Paused {
                return true;
            }
            let src_rate = v.src_rate();
            // Fades and the envelope run in heard seconds; the read head moves `speed`
            // times as fast through the file.
            let step = src_rate * v.speed / out_rate;
            let heard = src_rate * v.speed;
            let length = (v.seg_end - v.seg_start) / heard;
            let ducked = v.duck == Duck::Ducked;
            for (i, frame) in out.chunks_exact_mut(channels).enumerate() {
                let position = (v.pos - v.seg_start) / heard;
                let elapsed = v.elapsed as f64 / out_rate;
                let mut target = v.volume * v.send * envelope_gain(elapsed, position, length, v.fade_in, v.fade_out, v.looping, v.curve);
                let mut exact = v.ghost;
                if let VoiceState::Stopping { left, total, from, curve } = &mut v.state {
                    if *left == 0 {
                        sounded = sounded.max(i);
                        return false;
                    }
                    *left -= 1;
                    target = *from * curve.gain(*left as f32 / *total as f32);
                    // A stop's ramp is followed exactly: the smoothing would stretch the
                    // de-click and leave a step at its end.
                    exact = true;
                }
                if let VoiceState::Pausing { left, total, from, at } = &mut v.state {
                    if *left == 0 {
                        // Silent now: paused where it was pressed, rendering nothing more.
                        v.pos = *at;
                        v.elapsed = v.elapsed.saturating_sub(*total);
                        v.gain = 0.0;
                        v.state = VoiceState::Paused;
                        sounded = sounded.max(i);
                        return true;
                    }
                    *left -= 1;
                    target = *from * FadeCurve::SCurve.gain(*left as f32 / *total as f32);
                    exact = true;
                }
                // A jump's ghost follows its fade exactly, so the crossfade adds up.
                if exact {
                    v.gain = target;
                } else {
                    v.gain += (target - v.gain) * smooth;
                }
                let mut gain = if ducked { v.gain * env[i] } else { v.gain };
                if let Some((left, total)) = &mut v.xfade {
                    // Counted down first, like the ghost's stop, so the two add up to one.
                    *left -= 1;
                    gain *= 1.0 - *left as f32 / *total as f32;
                    if *left == 0 {
                        v.xfade = None;
                    }
                }
                let (l, r) = v.sample_at(v.pos);
                // The tile's meter: one compare per frame, nothing allocated.
                v.peak = v.peak.max(l.abs().max(r.abs()) * gain);
                if channels == 1 {
                    frame[0] += (l + r) * 0.5 * gain;
                } else {
                    frame[0] += l * gain;
                    frame[1] += r * gain;
                }
                v.pos += step;
                v.elapsed += 1;
                if let Some((a, b)) = v.vamp {
                    if !v.released && v.pos >= b {
                        v.pos = a + (v.pos - b);
                    }
                }
                if v.pos >= v.seg_end {
                    if v.looping {
                        v.pos = v.seg_start + (v.pos - v.seg_end);
                    } else {
                        if !v.ghost {
                            finished.push(v.tile_id);
                        }
                        sounded = sounded.max(i + 1);
                        return false;
                    }
                }
            }
            sounded = frames;
            true
        });
        self.sounded = sounded;

        // Master gain, the meter (before limiting, so the clip light still warns), then
        // a peak limiter: it drops its gain at once to keep the frame at full scale and
        // recovers over ~150 ms, so a boosted tile or a pile of voices doesn't hard-clip.
        let master = self.master;
        for frame in out.chunks_exact_mut(channels) {
            let mut peak = 0.0f32;
            for (ch, s) in frame.iter_mut().enumerate() {
                *s *= master;
                let meter = if channels == 1 { 0..2 } else { ch.min(2)..(ch + 1).min(2) };
                for m in meter {
                    self.peaks[m] = self.peaks[m].max(s.abs());
                }
                peak = peak.max(s.abs());
            }
            let needed = if peak > 1.0 { 1.0 / peak } else { 1.0 };
            self.limit = (self.limit + (1.0 - self.limit) * self.release).min(needed);
            for s in frame.iter_mut() {
                *s = (*s * self.limit).clamp(-1.0, 1.0);
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

/// A device's name, as libraries store it (outputs, routes, the input). cpal 0.18 dropped
/// `name()`: on CoreAudio the description's name is the same string, and on ALSA the PCM
/// id (`default`, `hw:CARD=PCH,DEV=0`) is what 0.15 called the name, so saved names still
/// match (D-100).
pub fn device_name(d: &cpal::Device) -> Option<String> {
    #[cfg(target_os = "linux")]
    return d.id().ok().map(|id| id.id().to_owned());
    #[cfg(not(target_os = "linux"))]
    return d.description().ok().map(|desc| desc.name().to_owned());
}

/// The names of `devices`, as Settings lists them. On ALSA, cpal 0.18 adds a numeric
/// `hw:CARD=0,DEV=0` / `plughw:` twin of every card's own entries, and lists `null`; 0.15
/// listed neither.
pub fn device_names(devices: impl Iterator<Item = cpal::Device>) -> Vec<String> {
    let mut names: Vec<String> = devices.filter_map(|d| device_name(&d)).filter(|n| listed(n)).collect();
    names.dedup();
    names
}

fn listed(name: &str) -> bool {
    if !cfg!(target_os = "linux") {
        return true;
    }
    let numeric_twin = ["hw:CARD=", "plughw:CARD="].iter().any(|p| {
        name.strip_prefix(p)
            .and_then(|rest| rest.split(',').next())
            .is_some_and(|card| !card.is_empty() && card.bytes().all(|b| b.is_ascii_digit()))
    });
    name != "null" && !numeric_twin
}

/// The config a device opens with, chosen as cpal 0.15 did so the upgrade keeps each
/// device's rate, format and latency (D-100). CoreAudio reports the device's current
/// format, which both versions use. ALSA has none: 0.15 took stereo (then mono, then the
/// most channels), F32 > I16 > U16, at 44.1 kHz when the range has it, else its top rate,
/// where 0.18 prefers 48 kHz and I32/I24; and it asked for 100 ms of output buffer and
/// 25 ms capture periods, where 0.18 takes the device's period twice over.
pub fn stream_config(dev: &cpal::Device, output: bool) -> Result<(cpal::StreamConfig, cpal::SampleFormat)> {
    #[cfg(not(target_os = "linux"))]
    {
        let supported = if output { dev.default_output_config()? } else { dev.default_input_config()? };
        Ok((supported.config(), supported.sample_format()))
    }
    #[cfg(target_os = "linux")]
    {
        let ranges: Vec<cpal::SupportedStreamConfigRange> =
            if output { dev.supported_output_configs()?.collect() } else { dev.supported_input_configs()?.collect() };
        let best = pick_like_0_15(&ranges).ok_or_else(|| anyhow!("no supported configuration"))?;
        let rate = if best.contains_rate(44_100) { 44_100 } else { best.max_sample_rate() };
        // Output latency is the whole buffer (cpal 0.18: two periods); capture latency is
        // one period.
        let frames = if output { rate / 20 } else { rate / 40 };
        let buffer_size = match best.buffer_size() {
            cpal::SupportedBufferSize::Range { min, max } if (*min..=*max).contains(&frames) => cpal::BufferSize::Fixed(frames),
            _ => cpal::BufferSize::Default,
        };
        Ok((cpal::StreamConfig { channels: best.channels(), sample_rate: rate, buffer_size }, best.sample_format()))
    }
}

/// The range cpal 0.15's `cmp_default_heuristics` ranked first, among the formats this app
/// builds streams for.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn pick_like_0_15(ranges: &[cpal::SupportedStreamConfigRange]) -> Option<cpal::SupportedStreamConfigRange> {
    use cpal::SampleFormat::{F32, I16, I32, U16};
    let key = |r: &cpal::SupportedStreamConfigRange| {
        let f = r.sample_format();
        (
            r.channels() == 2,
            r.channels() == 1,
            r.channels(),
            f == F32,
            f == I16,
            f == U16,
            r.contains_rate(44_100),
            r.max_sample_rate(),
        )
    };
    ranges.iter().filter(|r| matches!(r.sample_format(), F32 | I16 | U16 | I32)).max_by_key(|r| key(r)).cloned()
}

/// Whether a stream error means the stream is gone and has to be reopened. cpal 0.18
/// also reports glitches (`Xrun`), a default output that followed a new device
/// (`DeviceChanged`) and a refused real-time thread through the same callback; the stream
/// keeps playing after those.
pub fn stream_lost(err: &cpal::Error) -> bool {
    !matches!(err.kind(), cpal::ErrorKind::Xrun | cpal::ErrorKind::DeviceChanged | cpal::ErrorKind::RealtimeDenied)
}

/// Where a mixer's output goes: a device (by name; `None` follows the system default)
/// and the first of the two channels it uses there (0-based).
#[derive(Clone, Debug, PartialEq)]
pub struct Route {
    pub name: String,
    pub device: Option<String>,
    pub channel: u16,
}

/// Add a mixer's stereo block into a device buffer `channels` wide, starting at channel
/// `offset`: mono devices get the sum, and a pair that runs off the end keeps what fits.
pub fn mix_into(out: &mut [f32], channels: usize, stereo: &[f32], offset: usize) {
    let channels = channels.max(1);
    for (frame, lr) in out.chunks_exact_mut(channels).zip(stereo.as_chunks::<2>().0) {
        if channels == 1 {
            frame[0] += (lr[0] + lr[1]) * 0.5;
            continue;
        }
        let off = offset.min(channels - 1);
        frame[off] += lr[0];
        if off + 1 < channels {
            frame[off + 1] += lr[1];
        }
    }
}

/// Render a device's mixers into `scratch` (`channels` wide, `frames` long), holding all
/// of their locks at once: a change from the UI (a start, a pause, a jump, a release)
/// then lands on every one of them in the same block, so a tile's sends on one device
/// stay sample-aligned. Locks are always taken in mixer order (the engine's too), so
/// this can't deadlock.
fn render_group(parts: &[(Arc<Mutex<Mixer>>, usize)], frames: usize, channels: usize, stereo: &mut Vec<f32>, scratch: &mut [f32]) {
    let Some(((mixer, offset), rest)) = parts.split_first() else { return };
    let mut m = lock(mixer);
    render_group(rest, frames, channels, stereo, scratch);
    stereo.clear();
    stereo.resize(frames * 2, 0.0);
    m.render(stereo, 2);
    mix_into(scratch, channels, stereo, *offset);
}

/// One device's stream and the outputs (mixer indexes) it plays.
struct Group {
    /// The device's name, as [`device_name`] gives it.
    device: String,
    members: Vec<usize>,
    /// `None` for a test's stand-in device.
    #[allow(dead_code)]
    stream: Option<OutputStream>,
    /// Set by the stream's error callback once the device is gone.
    failed: Arc<AtomicBool>,
}

/// A device found for an output: the cpal device (none for a test's stand-in) and its name.
struct Found {
    device: Option<cpal::Device>,
    name: String,
}

/// Something that happened to the outputs on its own (a device went or came back), for
/// the status bar.
#[derive(Clone, Debug, PartialEq)]
pub enum OutputChange {
    /// An output's device went away. Tiles whose Output it was play on Main; its other
    /// voices stop.
    Lost { output: String, device: String },
    /// A closed output opened on its device again.
    Back { output: String, device: String },
    /// Main moved off `from` (lost) or onto `to` (`None`: no device to play on yet).
    Main { from: Option<String>, to: Option<String> },
}

/// Owns the output streams and their mixers: Main first, then each output the library
/// defines, then (when a preview device is set) Preview. Outputs on the same device share
/// one stream, each on its own channel pair. A stream that fails closes only its own
/// outputs (D-122).
pub struct Engine {
    mixers: Vec<Arc<Mutex<Mixer>>>,
    routes: Vec<Route>,
    /// Mixers whose device opened (or Main, always): a tile routed to a dead output plays
    /// on Main instead of going silent.
    alive: Vec<bool>,
    preview: Option<usize>,
    groups: Vec<Group>,
    /// When each device's stream last failed. A second failure within moments (a device
    /// that fails as soon as it opens) isn't retried at once: it waits for the next
    /// [`reopen_closed`](Self::reopen_closed).
    failures: HashMap<String, Instant>,
    /// The voice-ducking flag, shared by every mixer.
    voice: Arc<AtomicBool>,
    disabled: bool,
    /// Stand-in devices for tests, the system default first: outputs open on them
    /// without a stream.
    #[cfg(test)]
    fake: Option<Vec<String>>,
    pub device_name: Option<String>,
    pub error: Option<String>,
    /// Outputs that aren't open, with why.
    pub output_errors: Vec<(String, String)>,
}

impl Engine {
    fn empty(disabled: bool, error: Option<String>) -> Self {
        let main = Mixer::new(48_000);
        let voice = main.voice_flag();
        Engine {
            mixers: vec![Arc::new(Mutex::new(main))],
            routes: vec![Route { name: MAIN_OUTPUT.into(), device: None, channel: 0 }],
            alive: vec![true],
            preview: None,
            groups: Vec::new(),
            failures: HashMap::new(),
            voice,
            disabled,
            #[cfg(test)]
            fake: None,
            device_name: None,
            error,
            output_errors: Vec::new(),
        }
    }

    /// Open `device` (by name), falling back to the system default. Never fails: on error
    /// the engine runs silently and reports why in `error`.
    pub fn new(device: Option<&str>) -> Self {
        let mut e = Engine::empty(false, None);
        e.open(device);
        e
    }

    /// A silent engine that never touches an audio device (headless runs).
    pub fn disabled(reason: &str) -> Self {
        Engine::empty(true, Some(reason.to_owned()))
    }

    /// An engine whose devices are stand-ins called `devices` (the first is the system
    /// default), for tests of device loss and return.
    #[cfg(test)]
    pub fn fake(devices: &[&str]) -> Self {
        let mut e = Engine::empty(false, None);
        e.fake = Some(devices.iter().map(|d| d.to_string()).collect());
        e.open(None);
        e
    }

    /// Take a stand-in device away, as pulling its cable does: its stream fails.
    #[cfg(test)]
    pub fn unplug(&mut self, device: &str) {
        if let Some(f) = &mut self.fake {
            f.retain(|d| d != device);
        }
        for g in self.groups.iter().filter(|g| g.device == device) {
            g.failed.store(true, Ordering::Relaxed);
        }
    }

    /// A stand-in device's stream fails while the device stays (its sample rate changed).
    #[cfg(test)]
    pub fn glitch(&mut self, device: &str) {
        for g in self.groups.iter().filter(|g| g.device == device) {
            g.failed.store(true, Ordering::Relaxed);
        }
    }

    /// Plug a stand-in device (back) in.
    #[cfg(test)]
    pub fn plug(&mut self, device: &str) {
        if let Some(f) = &mut self.fake {
            f.push(device.to_owned());
        }
    }

    pub fn output_devices() -> Vec<String> {
        cpal::default_host().output_devices().map(device_names).unwrap_or_default()
    }

    fn main(&self) -> MutexGuard<'_, Mixer> {
        lock(&self.mixers[0])
    }

    /// Reopen Main on `device`, keeping the other outputs.
    pub fn open(&mut self, device: Option<&str>) {
        let extra: Vec<Route> = self.routes.iter().skip(1).take(self.preview.map_or(usize::MAX, |p| p - 1)).cloned().collect();
        let preview = self.preview.map(|p| self.routes[p].device.clone());
        self.configure(device, &extra, preview.as_ref().map(|d| d.as_deref()));
    }

    /// Set up every output: Main on `main`, the library's `outputs`, and Preview when
    /// `preview` is `Some(device)`. Every voice stops.
    pub fn configure(&mut self, main: Option<&str>, outputs: &[Route], preview: Option<Option<&str>>) {
        self.groups.clear();
        for m in self.mixers.iter().skip(1) {
            lock(m).stop_all(0.0);
        }
        // One guard at a time: a second lock in the same statement would deadlock.
        let (master, duck, hold) = {
            let mut main = self.main();
            main.stop_all(0.0);
            (main.master, (main.duck.level, main.duck.attack_s, main.duck.release_s), main.duck.hold)
        };
        self.mixers.truncate(1);
        self.routes.truncate(1);
        self.routes[0].device = main.map(str::to_owned);
        for r in outputs {
            self.routes.push(r.clone());
        }
        self.preview = preview.map(|d| {
            self.routes.push(Route { name: "Preview".into(), device: d.map(str::to_owned), channel: 0 });
            self.routes.len() - 1
        });
        let bus = Arc::new(AtomicU64::new(0));
        {
            let mut main = self.main();
            main.duck_bus = bus.clone();
            main.duck_bit = 1;
        }
        for i in 1..self.routes.len() {
            let mut m = Mixer::new(48_000);
            m.voice = self.voice.clone();
            m.duck_bus = bus.clone();
            m.duck_bit = 1 << i.min(63);
            m.set_master(master);
            m.set_duck(gain_to_db_floor(duck.0), duck.1, duck.2);
            m.set_duck_hold(hold);
            self.mixers.push(Arc::new(Mutex::new(m)));
        }
        self.alive = vec![false; self.routes.len()];
        self.alive[0] = true;
        self.output_errors.clear();
        if self.disabled {
            return;
        }
        // One stream per device, Main's first.
        let mut groups: Vec<(Found, Vec<usize>)> = Vec::new();
        for i in 0..self.routes.len() {
            let Some(found) = self.find(i) else {
                if i == 0 {
                    self.device_name = None;
                    self.error = Some("Audio output unavailable: no output device found".into());
                } else {
                    let r = &self.routes[i];
                    self.output_errors.push((r.name.clone(), format!("no device called {:?}", r.device.as_deref().unwrap_or("default"))));
                }
                continue;
            };
            match groups.iter_mut().find(|g| g.0.name == found.name) {
                Some(g) => g.1.push(i),
                None => groups.push((found, vec![i])),
            }
        }
        for (found, members) in groups {
            match self.open_members(&found, &members) {
                Ok(()) => {
                    if members.contains(&0) {
                        self.device_name = Some(found.name);
                        self.error = None;
                    }
                }
                Err(err) => {
                    for &i in &members {
                        if i == 0 {
                            self.device_name = None;
                            self.error = Some(format!("Audio output unavailable: {err}"));
                        } else {
                            self.output_errors.push((self.routes[i].name.clone(), err.to_string()));
                        }
                    }
                }
            }
        }
    }

    /// The device output `i` plays on: its own, by name, or the system default when it
    /// names none. Main takes the default when its own isn't there.
    fn find(&self, i: usize) -> Option<Found> {
        let r = &self.routes[i];
        let fallback = r.device.is_none() || i == 0;
        r.device.as_deref().and_then(|w| self.find_named(w)).or_else(|| fallback.then(|| self.find_default()).flatten())
    }

    /// The output device called `name`, if it's there.
    fn find_named(&self, name: &str) -> Option<Found> {
        #[cfg(test)]
        if let Some(fake) = &self.fake {
            return fake.iter().any(|d| d == name).then(|| Found { device: None, name: name.to_owned() });
        }
        let device = cpal::default_host().output_devices().ok()?.find(|d| device_name(d).as_deref() == Some(name))?;
        Some(Found { device: Some(device), name: name.to_owned() })
    }

    /// The system default output device.
    fn find_default(&self) -> Option<Found> {
        #[cfg(test)]
        if let Some(fake) = &self.fake {
            return fake.first().map(|name| Found { device: None, name: name.clone() });
        }
        let device = cpal::default_host().default_output_device()?;
        let name = device_name(&device).unwrap_or_else(|| "Unknown device".into());
        Some(Found { device: Some(device), name })
    }

    /// Open one stream on `found` for the outputs `members`, which come alive.
    fn open_members(&mut self, found: &Found, members: &[usize]) -> Result<()> {
        let failed = Arc::new(AtomicBool::new(false));
        let stream = match &found.device {
            Some(dev) => Some(OutputStream(self.open_group(dev, members, failed.clone())?)),
            None => None,
        };
        for &i in members {
            self.alive[i] = true;
            let name = &self.routes[i].name;
            self.output_errors.retain(|(n, _)| n != name);
        }
        self.groups.push(Group { device: found.name.clone(), members: members.to_vec(), stream, failed });
        Ok(())
    }

    /// Open the outputs `new` on `found`. Outputs already open on that device share its
    /// stream, which is rebuilt with them in it (they skip a block or two); if that fails,
    /// they reopen as they were, or close.
    fn attach(&mut self, new: &[usize], found: &Found, changes: &mut Vec<OutputChange>) -> Result<()> {
        let others: Vec<usize> = match self.groups.iter().position(|g| g.device == found.name) {
            Some(g) => self.groups.remove(g).members,
            None => Vec::new(),
        };
        let mut members = others.clone();
        members.extend_from_slice(new);
        members.sort_unstable();
        let err = match self.open_members(found, &members) {
            Ok(()) => return Ok(()),
            Err(e) => e,
        };
        if !others.is_empty() && self.open_members(found, &others).is_err() {
            self.close(&others, &found.name, changes);
        }
        Err(err)
    }

    /// Take Main out of its stream, leaving the outputs it shared a device with playing.
    fn detach_main(&mut self, changes: &mut Vec<OutputChange>) {
        let Some(g) = self.groups.iter().position(|g| g.members.contains(&0)) else { return };
        let g = self.groups.remove(g);
        let others: Vec<usize> = g.members.iter().copied().filter(|&i| i != 0).collect();
        drop(g.stream);
        if others.is_empty() {
            return;
        }
        let found = self.find(others[0]).filter(|f| f.name == g.device);
        if found.is_none_or(|f| self.open_members(&f, &others).is_err()) {
            self.close(&others, &g.device, changes);
        }
    }

    /// Outputs `members` lost `device`. Each closes; tiles whose Output it was move to
    /// Main (D-046) and its other voices stop. Main itself keeps its voices, to play them
    /// once it has a device again.
    fn close(&mut self, members: &[usize], device: &str, changes: &mut Vec<OutputChange>) {
        let lost: Vec<usize> = members.iter().copied().filter(|&i| i != 0).collect();
        for &i in &lost {
            self.alive[i] = false;
            let name = self.routes[i].name.clone();
            self.output_errors.retain(|(n, _)| *n != name);
            self.output_errors.push((name.clone(), format!("lost {device:?}; it reopens when the device is back")));
            changes.push(OutputChange::Lost { output: name, device: device.to_owned() });
        }
        if members.contains(&0) {
            self.device_name = None;
            self.error = Some(format!("Audio output lost: {device}"));
        }
        let mut all = self.lock_all();
        let main_rate = all[0].out_rate;
        for &i in &lost {
            let rate = all[i].out_rate;
            let voices = std::mem::take(&mut all[i].voices);
            if Some(i) == self.preview {
                continue;
            }
            for mut v in voices {
                let on_main = all[0].voices.iter().any(|m| m.tile_id == v.tile_id && !m.ghost);
                if v.lead && !v.ghost && !on_main {
                    v.rescale(rate, main_rate);
                    all[0].voices.push(v);
                }
            }
        }
    }

    /// Deal with the streams that failed, leaving the others playing. A stream whose
    /// device is still there (CoreAudio stops one when its device's sample rate changes)
    /// opens again in place, its voices untouched. Otherwise the device is gone: its
    /// outputs close, and if it was Main's, Main moves to another device at once.
    pub fn check_streams(&mut self) -> Vec<OutputChange> {
        let mut changes = Vec::new();
        while let Some(g) = self.groups.iter().position(|g| g.failed.load(Ordering::Relaxed)) {
            let Group { device, members, stream, .. } = self.groups.remove(g);
            drop(stream);
            self.failures.retain(|_, t| t.elapsed() < Duration::from_secs(3));
            let soon = self.failures.insert(device.clone(), Instant::now()).is_some();
            if !soon && self.find_named(&device).is_some_and(|f| self.open_members(&f, &members).is_ok()) {
                eprintln!("{}: reopened the stream on {device}", crate::brand::SLUG);
                continue;
            }
            eprintln!("{}: lost audio device {device}", crate::brand::SLUG);
            self.close(&members, &device, &mut changes);
            if members.contains(&0) {
                let to = if soon { None } else { self.reopen_main(&mut changes) };
                changes.push(OutputChange::Main { from: Some(device), to });
            }
        }
        changes
    }

    /// Open Main on its device, or the system default; its name if it opened.
    fn reopen_main(&mut self, changes: &mut Vec<OutputChange>) -> Option<String> {
        let Some(found) = self.find(0) else {
            self.error = Some("Audio output unavailable: no output device found".into());
            return None;
        };
        match self.attach(&[0], &found, changes) {
            Ok(()) => {
                self.device_name = Some(found.name.clone());
                self.error = None;
                Some(found.name)
            }
            Err(e) => {
                self.error = Some(format!("Audio output unavailable: {e}"));
                None
            }
        }
    }

    /// Whether an output is closed, or Main isn't on its own device: worth looking for
    /// devices that came back.
    pub fn wants_reopen(&self) -> bool {
        if self.disabled {
            return false;
        }
        let main_on = self.groups.iter().find(|g| g.members.contains(&0)).map(|g| &g.device);
        main_on.is_none() || self.routes[0].device.as_ref().is_some_and(|w| Some(w) != main_on) || self.alive.iter().any(|a| !a)
    }

    /// Reopen the outputs whose device is back, and move Main back onto its own device.
    /// Outputs on one device open together, in one stream. A tile already playing picks
    /// up a reopened send at its next start (D-122).
    pub fn reopen_closed(&mut self) -> Vec<OutputChange> {
        let mut changes = Vec::new();
        if self.disabled {
            return changes;
        }
        let mut pending: Vec<(Found, Vec<usize>)> = Vec::new();
        let main_on = self.groups.iter().find(|g| g.members.contains(&0)).map(|g| g.device.clone());
        if let Some(found) = self.find(0) {
            let home = self.routes[0].device.as_ref() == Some(&found.name);
            match &main_on {
                None => pending.push((found, vec![0])),
                Some(on) if home && found.name != *on => {
                    self.detach_main(&mut changes);
                    pending.push((found, vec![0]));
                }
                Some(_) => {}
            }
        }
        for i in 1..self.routes.len() {
            if self.alive[i] {
                continue;
            }
            let Some(found) = self.find(i) else { continue };
            match pending.iter_mut().find(|p| p.0.name == found.name) {
                Some(p) => p.1.push(i),
                None => pending.push((found, vec![i])),
            }
        }
        for (found, members) in pending {
            let opened = self.attach(&members, &found, &mut changes);
            for &i in &members {
                match (&opened, i) {
                    (Ok(()), 0) => {
                        self.device_name = Some(found.name.clone());
                        self.error = None;
                        changes.push(OutputChange::Main { from: None, to: Some(found.name.clone()) });
                    }
                    (Ok(()), _) => changes.push(OutputChange::Back { output: self.routes[i].name.clone(), device: found.name.clone() }),
                    (Err(e), 0) => {
                        self.error = Some(format!("Audio output unavailable: {e}"));
                        // It left a device that still works: go back to that.
                        if main_on.is_some() {
                            let to = self.reopen_main(&mut changes);
                            changes.push(OutputChange::Main { from: None, to });
                        }
                    }
                    (Err(e), _) => {
                        let name = self.routes[i].name.clone();
                        self.output_errors.retain(|(n, _)| *n != name);
                        self.output_errors.push((name, e.to_string()));
                    }
                }
            }
        }
        changes
    }

    fn open_group(&self, device: &cpal::Device, members: &[usize], failed: Arc<AtomicBool>) -> Result<cpal::Stream> {
        let (config, format) = stream_config(device, true)?;
        let parts: Vec<(Arc<Mutex<Mixer>>, usize)> =
            members.iter().map(|&i| (self.mixers[i].clone(), self.routes[i].channel as usize)).collect();
        for (m, _) in &parts {
            lock(m).set_output_rate(config.sample_rate);
        }
        let stream = match format {
            cpal::SampleFormat::F32 => self.build::<f32>(device, &config, parts, failed)?,
            cpal::SampleFormat::I16 => self.build::<i16>(device, &config, parts, failed)?,
            cpal::SampleFormat::U16 => self.build::<u16>(device, &config, parts, failed)?,
            cpal::SampleFormat::I32 => self.build::<i32>(device, &config, parts, failed)?,
            other => return Err(anyhow!("unsupported sample format {other}")),
        };
        stream.play()?;
        Ok(stream)
    }

    fn build<T>(&self, device: &cpal::Device, config: &cpal::StreamConfig, parts: Vec<(Arc<Mutex<Mixer>>, usize)>, failed: Arc<AtomicBool>) -> Result<cpal::Stream>
    where
        T: cpal::SizedSample + cpal::FromSample<f32>,
    {
        let channels = config.channels as usize;
        let mut scratch: Vec<f32> = Vec::new();
        let mut stereo: Vec<f32> = Vec::new();
        let stream = device.build_output_stream(
            *config,
            move |data: &mut [T], _| {
                scratch.clear();
                scratch.resize(data.len(), 0.0);
                let frames = data.len() / channels.max(1);
                render_group(&parts, frames, channels, &mut stereo, &mut scratch);
                for (d, s) in data.iter_mut().zip(&scratch) {
                    *d = T::from_sample(s.clamp(-1.0, 1.0));
                }
            },
            move |err| {
                if err.kind() == cpal::ErrorKind::Xrun {
                    return;
                }
                eprintln!("{}: audio stream error: {err}", crate::brand::SLUG);
                if stream_lost(&err) {
                    failed.store(true, Ordering::Relaxed);
                }
            },
            None,
        )?;
        Ok(stream)
    }

    /// The mixers a tile plays on, one per send, with each send's linear level. A send
    /// whose output didn't open is skipped, except the first (the tile's Output), which
    /// plays on Main instead of going silent (D-046) unless Main already has a send. No
    /// mixer gets two voices of one tile.
    fn mixers_for(&self, tile: &Tile) -> Vec<(usize, f32)> {
        let sends = tile.routes();
        let open = |name: &str| {
            self.routes.iter().position(|r| r.name.eq_ignore_ascii_case(name)).filter(|&i| Some(i) != self.preview && self.alive[i])
        };
        let mut to: Vec<(usize, f32)> = Vec::new();
        for s in &sends {
            if let Some(i) = open(&s.output).filter(|i| !to.iter().any(|(j, _)| j == i)) {
                to.push((i, db_to_gain(s.gain_db)));
            }
        }
        if open(&sends[0].output).is_none() && !to.iter().any(|(j, _)| *j == 0) {
            to.insert(0, (0, db_to_gain(sends[0].gain_db)));
        }
        to
    }

    /// Every mixer, locked together in mixer order (as the device callbacks take them),
    /// so a change reaches all of a tile's voices in the same block.
    fn lock_all(&self) -> Vec<MutexGuard<'_, Mixer>> {
        self.mixers.iter().map(|m| lock(m)).collect()
    }

    fn each<R>(&self, mut f: impl FnMut(&mut Mixer) -> R) -> Vec<R> {
        self.lock_all().iter_mut().map(|m| f(m)).collect()
    }

    fn play_on(&self, to: &[(usize, f32)], tile: &Tile, audio: Arc<DecodedAudio>) {
        let mut all = self.lock_all();
        for m in all.iter_mut() {
            m.retire(tile.id);
            // Solo fades every other tile, whatever output it's on.
            if tile.solo {
                m.solo(tile.id);
            }
        }
        for (n, &(i, send)) in to.iter().enumerate() {
            all[i].play_send(tile, audio.clone(), send);
            if let Some(v) = all[i].live_mut(tile.id).filter(|_| n == 0) {
                v.lead = true;
            }
        }
    }

    pub fn play(&self, tile: &Tile, audio: Arc<DecodedAudio>) {
        self.play_on(&self.mixers_for(tile), tile, audio);
    }
    /// Play on the preview output when there is one (headphones), else where the tile goes.
    /// A preview output that isn't open plays nothing (an operator's preview never goes to
    /// the house, D-136): the error says why, for the status bar.
    pub fn preview(&self, tile: &Tile, audio: Arc<DecodedAudio>) -> Result<(), String> {
        if let Some(why) = self.preview_unavailable() {
            return Err(why);
        }
        let to = match self.preview {
            Some(p) => vec![(p, 1.0)],
            None => self.mixers_for(tile),
        };
        self.play_on(&to, tile, audio);
        Ok(())
    }

    /// Why a preview can't play now: the preview output is on but its device isn't open.
    /// A disabled engine opens no device, so its Preview plays (silently) as if it had.
    pub fn preview_unavailable(&self) -> Option<String> {
        let p = self.preview.filter(|&p| !self.alive[p] && !self.disabled)?;
        let device = self.routes[p].device.as_deref().unwrap_or("the system default");
        let why = self.output_errors.iter().find(|(n, _)| *n == self.routes[p].name).map(|(_, e)| e.as_str());
        Some(match why {
            Some(e) if !e.starts_with("lost ") && !e.starts_with("no device") => format!("Preview's device {device} didn't open ({e}): nothing played"),
            _ => format!("Preview's device {device} isn't connected: nothing played"),
        })
    }
    /// The first output (by name) a tile's voices are on, if it's sounding.
    pub fn output_of(&self, id: Uuid) -> Option<String> {
        self.outputs_of(id).into_iter().next()
    }
    /// Every output (by name) a tile's voices are on, Main first.
    pub fn outputs_of(&self, id: Uuid) -> Vec<String> {
        let states = self.each(|m| m.state(id));
        states.iter().zip(&self.routes).filter(|(s, _)| **s != PlayState::Stopped).map(|(_, r)| r.name.clone()).collect()
    }
    /// The outputs: name, device, first channel (0-based), and whether it opened.
    pub fn routes(&self) -> Vec<(Route, bool)> {
        self.routes.iter().cloned().zip(self.alive.iter().copied()).collect()
    }
    pub fn stop(&self, id: Uuid, fade: f32) {
        self.each(|m| m.stop(id, fade));
    }
    pub fn stop_all(&self, fade: f32) {
        self.each(|m| m.stop_all(fade));
    }
    pub fn pause(&self, id: Uuid) {
        self.each(|m| m.pause(id));
    }
    pub fn play_on_vamp(&self, id: Uuid) -> bool {
        self.each(|m| m.play_on_vamp(id)).into_iter().any(|b| b)
    }
    pub fn jump(&self, id: Uuid, to: f64) -> bool {
        self.each(|m| m.jump(id, to)).into_iter().any(|b| b)
    }
    pub fn resume(&self, id: Uuid) {
        self.each(|m| m.resume(id));
    }
    /// Live changes, send levels included. Which outputs a tile plays on changes at its
    /// next start.
    pub fn update_params(&self, tile: &Tile) {
        let to = self.mixers_for(tile);
        for (i, m) in self.lock_all().iter_mut().enumerate() {
            m.update_send(tile, to.iter().find(|(j, _)| *j == i).map(|(_, g)| *g));
        }
    }
    pub fn set_master(&self, v: f32) {
        self.each(|m| m.set_master(v));
    }
    pub fn set_duck(&self, db: f32, attack: f32, release: f32) {
        self.each(|m| m.set_duck(db, attack, release));
    }
    pub fn set_duck_hold(&self, on: bool) {
        self.each(|m| m.set_duck_hold(on));
    }
    pub fn duck_hold(&self) -> bool {
        self.main().duck_hold()
    }
    pub fn ducking(&self) -> bool {
        self.each(|m| m.ducking()).into_iter().any(|b| b)
    }
    pub fn voice_flag(&self) -> Arc<AtomicBool> {
        self.voice.clone()
    }
    /// A voice is speaking above the voice-ducking threshold.
    pub fn voice(&self) -> bool {
        self.voice.load(Ordering::Relaxed)
    }
    pub fn state(&self, id: Uuid) -> PlayState {
        self.each(|m| m.state(id)).into_iter().find(|s| *s != PlayState::Stopped).unwrap_or(PlayState::Stopped)
    }
    pub fn vamping(&self, id: Uuid) -> bool {
        self.each(|m| m.vamping(id)).into_iter().any(|b| b)
    }
    pub fn statuses(&self) -> HashMap<Uuid, VoiceStatus> {
        self.each(|m| m.statuses()).into_iter().flatten().collect()
    }
    /// Main's meter.
    pub fn take_peaks(&self) -> [f32; 2] {
        self.main().take_peaks()
    }
    /// Each sounding tile's peak since the last call, across every output it plays on.
    pub fn take_levels(&self) -> HashMap<Uuid, f32> {
        let mut levels = HashMap::new();
        self.each(|m| m.take_levels(&mut levels));
        levels
    }
    pub fn sample_rate(&self) -> u32 {
        self.main().output_rate()
    }
    /// Tiles whose last voice reached its end on its own. Sends on different devices end
    /// a little apart; the tile finishes once all of them have.
    pub fn take_finished(&self) -> Vec<Uuid> {
        let mut all = self.lock_all();
        let mut done: Vec<Uuid> = Vec::new();
        for m in all.iter_mut() {
            for id in m.take_finished() {
                if !done.contains(&id) {
                    done.push(id);
                }
            }
        }
        done.retain(|id| all.iter().all(|m| m.state(*id) == PlayState::Stopped));
        done
    }

    /// Without a device nothing drains the mixers, so advance them here (keeps progress
    /// bars and auto-advance working in headless runs).
    pub fn tick_without_device(&self, seconds: f64) {
        for (i, m) in self.mixers.iter().enumerate() {
            if self.groups.iter().any(|g| g.stream.is_some() && g.members.contains(&i)) {
                continue;
            }
            let mut m = lock(m);
            let frames = (seconds * m.out_rate as f64) as usize;
            let mut buf = vec![0.0; frames.min(48_000) * 2];
            m.render(&mut buf, 2);
        }
    }
}

/// dB from a duck level's linear gain (for copying a mixer's duck settings).
fn gain_to_db_floor(g: f32) -> f32 {
    if g <= 0.0 {
        -80.0
    } else {
        20.0 * g.log10()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 1000;

    /// 1 s of constant 0.5 at 1 kHz, so gains are easy to read off the output.
    fn dc(seconds: f64) -> Arc<DecodedAudio> {
        let frames = (seconds * RATE as f64) as usize;
        Arc::new(DecodedAudio { sample_rate: RATE, data: vec![0.5; frames * 2], art: None, native: None })
    }

    #[test]
    fn picks_configs_as_cpal_0_15_did() {
        use cpal::SampleFormat::{F32, I16, I24, I32};
        let r = |channels, format, min, max| {
            cpal::SupportedStreamConfigRange::new(channels, min, max, cpal::SupportedBufferSize::Range { min: 64, max: 8192 }, format)
        };
        // I16 over I32 and I24 (0.18 would take I32), stereo over more channels.
        let best = pick_like_0_15(&[r(2, I32, 8000, 192_000), r(2, I16, 8000, 96_000), r(2, I24, 8000, 192_000), r(6, F32, 8000, 48_000)]).unwrap();
        assert_eq!((best.channels(), best.sample_format()), (2, I16));
        // F32 first; among equals the range with 44.1 kHz.
        let best = pick_like_0_15(&[r(2, F32, 48_000, 48_000), r(2, F32, 8000, 96_000)]).unwrap();
        assert!(best.contains_rate(44_100));
        // Formats it can't build are never picked.
        assert!(pick_like_0_15(&[r(2, I24, 8000, 48_000)]).is_none());
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn lists_alsa_devices_as_cpal_0_15_did() {
        assert!(listed("default"));
        assert!(listed("hw:CARD=PCH,DEV=0"));
        assert!(listed("plughw:CARD=Device,DEV=0"));
        assert!(!listed("hw:CARD=0,DEV=0"));
        assert!(!listed("plughw:CARD=1,DEV=3"));
        assert!(!listed("null"));
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
        // Paused at once, as far as anyone can see; heard, it falls over 0.1 s, then nothing.
        assert_eq!(m.state(t.id), PlayState::Paused);
        let out = render(&mut m, 300);
        assert!(out[0] > 0.49 && out[50] > 0.1 && out[50] < 0.4, "a 0.1 s ramp: {} {}", out[0], out[50]);
        assert!(biggest_step(&out) < 0.5 / 50.0, "no click: {}", biggest_step(&out));
        assert!(out[100..].iter().all(|s| *s == 0.0));
        let pos = m.statuses()[&t.id].position;
        assert!((pos - 0.3).abs() < 1e-6, "it resumes where it was paused: {pos}");
        m.resume(t.id);
        let out = render(&mut m, 100);
        assert!(out[0] < 0.2 && out[99] > 0.49, "back up through the smoothing: {} {}", out[0], out[99]);
        assert!((m.statuses()[&t.id].position - 0.4).abs() < 1e-6);
        // Resumed during the ramp: from where it was paused, at once.
        m.pause(t.id);
        render(&mut m, 30);
        m.resume(t.id);
        assert_eq!(m.state(t.id), PlayState::Playing);
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

    fn levels(m: &mut Mixer) -> HashMap<Uuid, f32> {
        let mut out = HashMap::new();
        m.take_levels(&mut out);
        out
    }

    #[test]
    fn reports_each_tiles_peak_after_its_gain_and_master() {
        let mut m = Mixer::new(RATE);
        m.set_master(0.5);
        let (a, b) = (tile(), Tile { volume: 0.5, ..tile() });
        m.play(&a, dc(1.0));
        m.play(&b, dc(1.0));
        render(&mut m, 100);
        let l = levels(&mut m);
        assert!((l[&a.id] - 0.25).abs() < 0.01, "0.5 × master 0.5: {}", l[&a.id]);
        assert!(l[&b.id] < l[&a.id] && l[&b.id] > 0.0, "b's own volume shows: {}", l[&b.id]);
        assert_eq!(levels(&mut m)[&a.id], 0.0, "reset after reading");
        // A paused tile is silent once its 0.1 s ramp is over, and a finished one is gone.
        m.pause(a.id);
        render(&mut m, 100);
        levels(&mut m);
        render(&mut m, 100);
        assert_eq!(levels(&mut m)[&a.id], 0.0);
        render(&mut m, 1000);
        assert!(!levels(&mut m).contains_key(&b.id));
    }

    #[test]
    fn a_tiles_level_takes_the_loudest_of_its_voices() {
        let mut m = Mixer::new(RATE);
        let t = tile();
        m.play(&t, dc(1.0));
        render(&mut m, 100);
        levels(&mut m);
        // Restarted with a fade-in: the old voice ramps out as a ghost from full level
        // while the new one comes in from silence; the tile reads the louder one.
        m.play(&Tile { fade_in: 0.5, ..t.clone() }, dc(1.0));
        render(&mut m, 1);
        assert!(levels(&mut m)[&t.id] > 0.4);
    }

    #[test]
    fn engine_merges_levels_across_mixers() {
        let e = Engine::disabled("test");
        let t = tile();
        e.play(&t, dc(1.0));
        e.tick_without_device(0.01);
        let l = e.take_levels();
        assert!(l[&t.id] > 0.4, "{l:?}");
        assert_eq!(e.take_levels()[&t.id], 0.0);
    }

    #[test]
    fn gain_boosts_above_unity() {
        let mut m = Mixer::new(RATE);
        // 0.5 × +5.9 dB ≈ 0.99, just under the limiter.
        m.play(&Tile { gain_db: 5.9, ..tile() }, dc(1.0));
        let out = render(&mut m, 100);
        assert!((out[50] - 0.5 * db_to_gain(5.9)).abs() < 0.01, "{}", out[50]);
    }

    #[test]
    fn limiter_holds_full_scale_then_recovers() {
        let mut m = Mixer::new(RATE);
        // 0.5 × +12 dB ≈ 2.0: limited to full scale, not clipped into a square.
        m.play(&Tile { gain_db: 12.0, ..tile() }, dc(0.2));
        let out = render(&mut m, 200);
        assert!(out[100] <= 1.0 && out[100] > 0.95, "{}", out[100]);
        assert!(m.limit < 0.6);
        // Once the loud tile ends, the limiter lets go and a quiet tile plays at its level.
        m.play(&tile(), dc(2.0));
        let out = render(&mut m, 1500);
        assert!((out[1400] - 0.5).abs() < 0.01, "{}", out[1400]);
    }

    #[test]
    fn ducked_tiles_dip_while_a_ducker_plays() {
        let mut m = Mixer::new(RATE);
        m.set_duck(-12.0, 0.0, 0.1);
        let bed = Tile { duck: Duck::Ducked, looping: true, ..tile() };
        let other = Tile { looping: true, ..tile() };
        m.play(&bed, dc(1.0));
        m.play(&other, dc(1.0));
        let out = render(&mut m, 100);
        assert!((out[90] - 1.0).abs() < 0.01, "both at full level: {}", out[90]);
        assert!(!m.ducking());
        // A 0.2 s voice drop that ducks: the bed falls to −12 dB, the other tile doesn't.
        let voice = Tile { duck: Duck::Ducks, ..tile() };
        m.play(&voice, Arc::new(DecodedAudio { sample_rate: RATE, data: vec![0.0; 400], art: None, native: None }));
        let out = render(&mut m, 150);
        assert!(m.ducking());
        assert!((out[140] - (0.5 + 0.5 * db_to_gain(-12.0))).abs() < 0.01, "{}", out[140]);
        // The voice ends at 0.2 s (the duck is decided per block); the bed comes back
        // over the 0.1 s release.
        render(&mut m, 100);
        let out = render(&mut m, 700);
        assert!(!m.ducking());
        assert!((out[690] - 1.0).abs() < 0.01, "{}", out[690]);
    }

    #[test]
    fn talk_over_holds_the_duck() {
        let mut m = Mixer::new(RATE);
        m.set_duck(-20.0, 0.0, 0.0);
        let bed = Tile { duck: Duck::Ducked, looping: true, ..tile() };
        m.play(&bed, dc(1.0));
        m.set_duck_hold(true);
        let out = render(&mut m, 100);
        assert!((out[90] - 0.05).abs() < 0.005, "{}", out[90]);
        m.set_duck_hold(false);
        let out = render(&mut m, 100);
        assert!((out[90] - 0.5).abs() < 0.005, "{}", out[90]);
    }

    #[test]
    fn a_voice_on_the_microphone_ducks_the_beds() {
        let mut m = Mixer::new(RATE);
        m.set_duck(-20.0, 0.0, 0.0);
        m.play(&Tile { duck: Duck::Ducked, looping: true, ..tile() }, dc(1.0));
        let voice = m.voice_flag();
        voice.store(true, Ordering::Relaxed);
        let out = render(&mut m, 50);
        assert!((out[40] - 0.05).abs() < 0.005, "{}", out[40]);
        assert!(m.ducking());
        voice.store(false, Ordering::Relaxed);
        let out = render(&mut m, 50);
        assert!((out[40] - 0.5).abs() < 0.005, "{}", out[40]);
    }

    #[test]
    fn vamps_until_played_on_then_plays_out() {
        let mut m = Mixer::new(RATE);
        // 1 s file; the vamp loops 0.2-0.4 s.
        let t = Tile { vamp: Some((0.2, 0.4)), ..tile() };
        m.play(&t, dc(1.0));
        render(&mut m, 2000);
        let st = m.statuses()[&t.id];
        assert!(st.vamping && !st.played_on && st.position >= 0.2 && st.position < 0.4, "{st:?}");
        assert!(m.vamping(t.id));
        assert!(m.take_finished().is_empty(), "still vamping after 2 s");
        assert!(m.play_on_vamp(t.id));
        assert!(!m.play_on_vamp(t.id), "only once");
        let st = m.statuses()[&t.id];
        assert!(!st.vamping && st.played_on && !m.vamping(t.id), "{st:?}");
        render(&mut m, 1000);
        assert_eq!(m.take_finished(), vec![t.id], "played out to the end");
    }

    #[test]
    fn speed_changes_length_and_reports_heard_time() {
        let mut m = Mixer::new(RATE);
        let fast = Tile { speed: 2.0, ..tile() };
        m.play(&fast, dc(1.0));
        render(&mut m, 250);
        let st = m.statuses()[&fast.id];
        assert!((st.length - 0.5).abs() < 1e-9 && (st.position - 0.25).abs() < 1e-3, "{st:?}");
        render(&mut m, 300);
        assert_eq!(m.take_finished(), vec![fast.id], "a 1 s clip at 2x lasts 0.5 s");
        let slow = Tile { speed: 0.5, ..tile() };
        m.play(&slow, dc(1.0));
        render(&mut m, 1900);
        assert!(m.take_finished().is_empty(), "at 0.5x it lasts 2 s");
    }

    #[test]
    fn jumps_with_a_crossfade() {
        let mut m = Mixer::new(RATE);
        let t = Tile { vamp: Some((0.1, 0.2)), ..tile() };
        m.play(&t, dc(1.0));
        render(&mut m, 150);
        assert!(m.jump(t.id, 0.8), "to past the vamp");
        assert_eq!(m.statuses().len(), 1, "the ghost isn't reported");
        let st = m.statuses()[&t.id];
        assert!((st.position - 0.8).abs() < 1e-3 && !st.vamping, "{st:?}");
        // Old and new overlap briefly but never jump past full level.
        let out = render(&mut m, 30);
        assert!(out.iter().all(|s| *s <= 0.51), "{out:?}");
        render(&mut m, 200);
        assert_eq!(m.take_finished(), vec![t.id], "only the real voice finishes");
        assert!(!m.jump(t.id, 0.1), "not playing any more");
    }

    #[test]
    fn mixes_a_stereo_block_onto_a_channel_pair() {
        let stereo = [0.1, 0.2, 0.3, 0.4];
        let mut quad = vec![0.0; 8];
        mix_into(&mut quad, 4, &stereo, 2);
        assert_eq!(quad, [0.0, 0.0, 0.1, 0.2, 0.0, 0.0, 0.3, 0.4], "channels 3 and 4");
        let mut mono = vec![0.0; 2];
        mix_into(&mut mono, 1, &stereo, 0);
        assert!((mono[0] - 0.15).abs() < 1e-6 && (mono[1] - 0.35).abs() < 1e-6);
        let mut stereo_dev = vec![0.0; 4];
        mix_into(&mut stereo_dev, 2, &stereo, 5);
        assert_eq!(stereo_dev, [0.0, 0.1, 0.0, 0.3], "a pair past the end keeps what fits");
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

    /// A rising ramp, so a voice that slipped by even one frame shows.
    fn ramp(frames: usize) -> Arc<DecodedAudio> {
        let data = (0..frames).flat_map(|i| [i as f32 / frames as f32, i as f32 / frames as f32]).collect();
        Arc::new(DecodedAudio { sample_rate: RATE, data, art: None, native: None })
    }

    /// A disabled engine with outputs on one (pretend) device that opened.
    fn engine_with(outputs: &[(&str, u16)]) -> Engine {
        let mut e = Engine::disabled("test");
        let routes: Vec<Route> = outputs.iter().map(|(n, c)| Route { name: (*n).into(), device: None, channel: *c }).collect();
        e.configure(None, &routes, None);
        e.alive = vec![true; e.routes.len()];
        for m in &e.mixers {
            lock(m).set_output_rate(RATE);
        }
        e
    }

    fn parts(e: &Engine) -> Vec<(Arc<Mutex<Mixer>>, usize)> {
        e.mixers.iter().cloned().zip(e.routes.iter().map(|r| r.channel as usize)).collect()
    }

    fn sends(list: &[(&str, f32)]) -> Vec<crate::model::TileSend> {
        list.iter().map(|(o, g)| crate::model::TileSend { output: (*o).into(), gain_db: *g }).collect()
    }

    #[test]
    fn sends_on_one_device_stay_sample_aligned() {
        let e = engine_with(&[("Phones", 2)]);
        let t = Tile { sends: sends(&[("Main", 0.0), ("Phones", -6.0)]), vamp: Some((0.2, 0.3)), ..tile() };
        e.play(&t, ramp(1000));
        assert_eq!(e.outputs_of(t.id), ["Main", "Phones"]);
        let parts = parts(&e);
        let (mut stereo, mut out) = (Vec::new(), Vec::new());
        let mut block = |frames: usize| {
            out.clear();
            out.resize(frames * 4, 0.0);
            render_group(&parts, frames, 4, &mut stereo, &mut out);
            out.chunks(4).map(|f| (f[0], f[2])).collect::<Vec<_>>()
        };
        let mut heard = block(150);
        // Pause, resume, a jump and the vamp's release all reach both voices together.
        e.pause(t.id);
        heard.extend(block(50));
        e.resume(t.id);
        heard.extend(block(170));
        assert!(e.play_on_vamp(t.id));
        heard.extend(block(100));
        assert!(e.jump(t.id, 0.7));
        heard.extend(block(350));
        let g = db_to_gain(-6.0);
        for (i, (main, phones)) in heard.iter().enumerate() {
            assert!((main * g - phones).abs() < 1e-6, "frame {i}: Main {main}, Phones {phones}");
        }
        assert!(heard.iter().any(|(m, _)| *m > 0.5), "it played");
        assert_eq!(e.take_finished(), vec![t.id], "one tile, finished once");
    }

    #[test]
    fn dead_sends_fall_back_to_main_only_for_the_first() {
        let mut e = engine_with(&[("House", 0), ("Phones", 2), ("Stream", 4)]);
        // House and Phones didn't open.
        e.alive[1] = false;
        e.alive[2] = false;
        let to = |list: &[(&str, f32)]| e.mixers_for(&Tile { sends: sends(list), ..tile() });
        assert_eq!(to(&[]), [(0, 1.0)], "no sends: Main");
        assert_eq!(to(&[("House", -6.0), ("Stream", 0.0)]), [(0, db_to_gain(-6.0)), (3, 1.0)], "the Output falls back to Main");
        assert_eq!(to(&[("Stream", 0.0), ("Phones", 0.0)]), [(3, 1.0)], "a dead extra send is skipped");
        assert_eq!(to(&[("House", -6.0), ("Main", 0.0)]), [(0, 1.0)], "Main already has a send: no second voice");
        assert_eq!(to(&[("Main", -3.0), ("main", 0.0)]), [(0, db_to_gain(-3.0))]);
    }

    #[test]
    fn send_levels_change_live() {
        let e = engine_with(&[("Stream", 2)]);
        let mut t = Tile { looping: true, sends: sends(&[("Main", 0.0), ("Stream", 0.0)]), ..tile() };
        e.play(&t, dc(1.0));
        t.sends[1].gain_db = -12.0;
        e.update_params(&t);
        e.tick_without_device(0.2);
        let mut out = vec![0.0; 8];
        render_group(&parts(&e), 2, 4, &mut Vec::new(), &mut out);
        assert!((out[0] - 0.5).abs() < 1e-3 && (out[2] - 0.5 * db_to_gain(-12.0)).abs() < 1e-3, "{out:?}");
    }

    #[test]
    fn solo_and_ducking_act_on_every_output() {
        let e = engine_with(&[("Phones", 2)]);
        e.set_duck(-20.0, 0.0, 0.0);
        let bed = Tile { duck: Duck::Ducked, looping: true, sends: sends(&[("Main", 0.0), ("Phones", 0.0)]), ..tile() };
        e.play(&bed, dc(1.0));
        // A ducking tile on Phones alone dips the bed on Main too.
        let voice = Tile { duck: Duck::Ducks, looping: true, sends: sends(&[("Phones", 0.0)]), ..tile() };
        e.play(&voice, dc(1.0));
        let parts = parts(&e);
        render_group(&parts, 10, 4, &mut Vec::new(), &mut [0.0; 40]);
        let mut out = vec![0.0; 40];
        render_group(&parts, 10, 4, &mut Vec::new(), &mut out);
        assert!((out[36] - 0.05).abs() < 0.005, "the bed on Main is ducked: {}", out[36]);
        assert!(e.ducking());
        // A solo tile on Main fades the others on Phones as well.
        let solo = Tile { solo: true, ..tile() };
        e.play(&solo, dc(1.0));
        assert_eq!(e.state(voice.id), PlayState::Stopping);
        assert_eq!(lock(&e.mixers[1]).state(bed.id), PlayState::Stopping);
        // Stop reaches every voice.
        e.stop(solo.id, 0.0);
        e.stop(bed.id, 0.0);
        assert_eq!(e.outputs_of(bed.id), ["Main", "Phones"], "de-clicking on both");
        render_group(&parts, 90, 4, &mut Vec::new(), &mut [0.0; 360]);
        assert_eq!(e.outputs_of(bed.id), ["Main", "Phones"], "still fading after 90 ms");
        render_group(&parts, 20, 4, &mut Vec::new(), &mut [0.0; 80]);
        assert!(e.outputs_of(bed.id).is_empty());
    }

    /// Frame-to-frame jumps in `out`, the biggest one: what a click is.
    fn biggest_step(out: &[f32]) -> f32 {
        out.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max)
    }

    #[test]
    fn a_hard_stop_ramps_out_over_the_declick_time() {
        // 48 kHz so the 0.1 s ramp is 4800 frames, as on a device.
        let rate = 48_000;
        let audio = Arc::new(DecodedAudio { sample_rate: rate, data: vec![0.8; 96_000 * 2], art: None, native: None });
        let mut m = Mixer::new(rate);
        let t = tile();
        m.play(&t, audio.clone());
        render(&mut m, 1000);
        m.stop(t.id, 0.0);
        assert_eq!(m.state(t.id), PlayState::Stopping, "not cut at once");
        let out = render(&mut m, 5000);
        let ramp = out.iter().take_while(|s| **s > 0.0).count();
        assert!((4795..=4801).contains(&ramp), "0.1 s: {ramp} frames");
        assert!((out[2400] - 0.4).abs() < 0.01, "a raised cosine, halfway down: {}", out[2400]);
        assert!(biggest_step(&out) < 0.8 / 1000.0, "no step bigger than 0.1% of the level: {}", biggest_step(&out));
        assert_eq!(out[ramp], 0.0);
        assert_eq!(m.state(t.id), PlayState::Stopped);
        assert!(m.take_finished().is_empty());

        // A stop fade longer than the ramp keeps the tile's own curve and length.
        let t = Tile { stop_fade: 0.5, fade_curve: FadeCurve::Linear, ..tile() };
        m.play(&t, audio);
        render(&mut m, 100);
        m.stop(t.id, t.stop_fade);
        let out = render(&mut m, 24_100);
        assert!((out[12_000] - 0.4).abs() < 0.01, "linear, halfway down: {}", out[12_000]);
        assert_eq!(out[24_050], 0.0);
    }

    #[test]
    fn a_second_faster_stop_takes_over_a_fade() {
        let mut m = Mixer::new(48_000);
        let t = tile();
        let audio = Arc::new(DecodedAudio { sample_rate: 48_000, data: vec![0.5; 96_000 * 2], art: None, native: None });
        m.play(&t, audio);
        m.stop(t.id, 1.0);
        render(&mut m, 4800);
        m.stop(t.id, 2.0);
        let out = render(&mut m, 4800);
        assert!(out[4799] < out[0], "a slower stop leaves the fade running");
        // Esc's second press: from wherever the fade is, a 0.1 s de-click ramp to silence.
        m.stop(t.id, 0.0);
        let out = render(&mut m, 5000);
        assert!(biggest_step(&out) < 0.001);
        assert!(out[4700] > 0.0);
        assert_eq!(out[4850], 0.0);
        assert_eq!(m.state(t.id), PlayState::Stopped);
    }

    #[test]
    fn restarting_ramps_the_old_voice_out_under_the_new_one() {
        let rate = 48_000;
        // A square wave at full scale: cutting it mid-cycle is the worst case.
        let data: Vec<f32> = (0..96_000).flat_map(|i| { let s = if (i / 50) % 2 == 0 { 0.5 } else { -0.5 }; [s, s] }).collect();
        let audio = Arc::new(DecodedAudio { sample_rate: rate, data, art: None, native: None });
        let mut m = Mixer::new(rate);
        let t = Tile { fade_in: 0.01, press: crate::model::PressOptions { one_shot: true, ..Default::default() }, ..tile() };
        m.play(&t, audio.clone());
        render(&mut m, 1025);
        m.play(&t, audio);
        assert_eq!(m.voices.len(), 2, "the old voice lingers as a ghost");
        assert_eq!(m.statuses().len(), 1, "but only the new one is the tile");
        // The new voice starts at once, from the top …
        let out = render(&mut m, 300);
        assert!((m.statuses()[&t.id].position - 300.0 / 48_000.0).abs() < 1e-6);
        // … while the old one, at +0.5, falls over 0.1 s under it, from no cut.
        assert!(out[0] > 0.45, "no cut: {}", out[0]);
        render(&mut m, 4400);
        assert_eq!(m.voices.len(), 2, "the old voice is still fading");
        render(&mut m, 200);
        assert_eq!(m.voices.len(), 1, "the ghost is gone after its ramp");
        // A stop now reaches the new voice, not the ghost.
        m.stop(t.id, 0.0);
        render(&mut m, 4900);
        assert_eq!(m.state(t.id), PlayState::Stopped);
    }

    #[test]
    fn solo_stops_others_with_their_own_stop_fade() {
        let mut m = Mixer::new(RATE);
        let hard = tile();
        let soft = Tile { stop_fade: 0.5, ..tile() };
        m.play(&hard, dc(2.0));
        m.play(&soft, dc(2.0));
        m.play(&Tile { solo: true, ..tile() }, dc(2.0));
        render(&mut m, 90);
        assert_eq!(m.state(hard.id), PlayState::Stopping, "no stop fade: still de-clicking at 90 ms");
        render(&mut m, 20);
        assert_eq!(m.state(hard.id), PlayState::Stopped, "gone after the 0.1 s de-click");
        assert_eq!(m.state(soft.id), PlayState::Stopping);
        render(&mut m, 500);
        assert_eq!(m.state(soft.id), PlayState::Stopped);
    }

    /// 1 s of three constant channels (0.1, 0.2, 0.4), downmixed to a constant 0.3 pair.
    fn three_channels() -> Arc<DecodedAudio> {
        let frames = RATE as usize;
        let native = crate::decode::NativeChannels {
            data: [0.1f32, 0.2, 0.4].repeat(frames),
            labels: vec!["L".into(), "R".into(), "C".into()],
            downmix: vec![(1.0, 0.0), (0.0, 1.0), (0.5, 0.5)],
        };
        Arc::new(DecodedAudio { sample_rate: RATE, data: vec![0.3; frames * 2], art: None, native: Some(native) })
    }

    /// The last frame of 0.15 s, after a restart's old voice has faded out under it.
    fn pair(m: &mut Mixer) -> (f32, f32) {
        let mut out = vec![0.0; 300];
        m.render(&mut out, 2);
        (out[298], out[299])
    }

    #[test]
    fn a_channel_mix_routes_each_channel() {
        use crate::model::{ChannelRoute, ChannelTo};
        let route = |to, gain_db| ChannelRoute { to, gain_db };
        let mut m = Mixer::new(RATE);
        // Auto plays the downmix.
        let mut t = Tile { looping: true, ..tile() };
        m.play(&t, three_channels());
        let (l, r) = pair(&mut m);
        assert!((l - 0.3).abs() < 1e-4 && (r - 0.3).abs() < 1e-4, "{l} {r}");
        // L to the left, R muted, C to the right at −6 dB; it changes live.
        t.channel_mix = vec![route(ChannelTo::Left, 0.0), route(ChannelTo::Mute, 0.0), route(ChannelTo::Right, -6.0)];
        m.update_params(&t);
        let (l, r) = pair(&mut m);
        assert!((l - 0.1).abs() < 1e-4 && (r - 0.4 * db_to_gain(-6.0)).abs() < 1e-4, "{l} {r}");
        // Both: one channel to both sides.
        t.channel_mix = vec![route(ChannelTo::Mute, 0.0), route(ChannelTo::Both, 0.0), route(ChannelTo::Mute, 0.0)];
        m.play(&t, three_channels());
        let (l, r) = pair(&mut m);
        assert!((l - 0.2).abs() < 1e-4 && (r - 0.2).abs() < 1e-4, "{l} {r}");
        // A mix made for another channel count plays as Auto.
        t.channel_mix.pop();
        m.update_params(&t);
        let (l, _) = pair(&mut m);
        assert!((l - 0.3).abs() < 1e-4, "{l}");
    }

    #[test]
    fn every_send_plays_the_channel_mix() {
        use crate::model::{ChannelRoute, ChannelTo};
        let e = engine_with(&[("Phones", 2)]);
        let mix = vec![ChannelRoute { to: ChannelTo::Left, gain_db: 0.0 }, ChannelRoute { to: ChannelTo::Mute, gain_db: 0.0 }, ChannelRoute { to: ChannelTo::Right, gain_db: 0.0 }];
        let t = Tile { looping: true, channel_mix: mix, sends: sends(&[("Main", 0.0), ("Phones", -6.0)]), ..tile() };
        e.play(&t, three_channels());
        let mut out = vec![0.0; 200];
        render_group(&parts(&e), 50, 4, &mut Vec::new(), &mut out);
        let g = db_to_gain(-6.0);
        let f = &out[196..200];
        assert!((f[0] - 0.1).abs() < 1e-4 && (f[1] - 0.4).abs() < 1e-4, "{f:?}");
        assert!((f[2] - 0.1 * g).abs() < 1e-4 && (f[3] - 0.4 * g).abs() < 1e-4, "{f:?}");
    }

    fn route(name: &str, device: &str, channel: u16) -> Route {
        Route { name: name.into(), device: Some(device.into()), channel }
    }

    /// Each sounding tile's place, to show a voice carried on without a restart.
    fn position(e: &Engine, id: Uuid) -> f64 {
        e.statuses()[&id].position
    }

    #[test]
    fn a_lost_output_closes_alone_and_its_tiles_carry_on() {
        let mut e = Engine::fake(&["Speakers", "Headphones"]);
        e.configure(None, &[route("Phones", "Headphones", 0), route("Booth", "Speakers", 2)], None);
        assert_eq!(e.device_name.as_deref(), Some("Speakers"));
        assert!(e.routes().iter().all(|(_, open)| *open));
        assert!(!e.wants_reopen());
        // Main with a headphone send, one on Phones alone, one on Main alone, one on the
        // Booth with a headphone send.
        let send = Tile { sends: sends(&[("Main", 0.0), ("Phones", -6.0)]), ..tile() };
        let phones = Tile { sends: sends(&[("Phones", -3.0)]), ..tile() };
        let main = tile();
        let booth = Tile { sends: sends(&[("Booth", 0.0), ("Phones", 0.0)]), ..tile() };
        for t in [&send, &phones, &main, &booth] {
            e.play(t, dc(4.0));
        }
        e.tick_without_device(0.5);
        let before: Vec<f64> = [&send, &phones, &main, &booth].iter().map(|t| position(&e, t.id)).collect();

        e.unplug("Headphones");
        assert_eq!(e.check_streams(), [OutputChange::Lost { output: "Phones".into(), device: "Headphones".into() }]);
        assert!(e.check_streams().is_empty(), "reported once");
        assert_eq!(e.device_name.as_deref(), Some("Speakers"), "Main stays where it was");
        assert_eq!(e.outputs_of(send.id), ["Main"], "the headphone send drops");
        assert_eq!(e.outputs_of(phones.id), ["Main"], "a tile whose Output was lost plays on Main");
        assert_eq!(e.outputs_of(main.id), ["Main"]);
        assert_eq!(e.outputs_of(booth.id), ["Booth"], "the Booth keeps its tile, and Main gets no copy");
        let after: Vec<f64> = [&send, &phones, &main, &booth].iter().map(|t| position(&e, t.id)).collect();
        assert_eq!(before, after, "nothing restarted");
        // The moved voice keeps its send level on Main.
        let lead = e.main().voices.iter().find(|v| v.tile_id == phones.id).map(|v| v.send);
        assert_eq!(lead, Some(db_to_gain(-3.0)));
        let open: Vec<bool> = e.routes().iter().map(|(_, o)| *o).collect();
        assert_eq!(open, [true, false, true]);
        assert!(e.output_errors.iter().any(|(n, why)| n == "Phones" && why.contains("Headphones")), "{:?}", e.output_errors);

        // Nothing comes back while the device is away; then it does.
        assert!(e.wants_reopen());
        assert!(e.reopen_closed().is_empty());
        e.plug("Headphones");
        assert_eq!(e.reopen_closed(), [OutputChange::Back { output: "Phones".into(), device: "Headphones".into() }]);
        assert!(e.routes().iter().all(|(_, open)| *open));
        assert!(e.output_errors.is_empty(), "{:?}", e.output_errors);
        assert!(!e.wants_reopen());
        // A playing tile picks the send up at its next start.
        assert_eq!(e.outputs_of(send.id), ["Main"]);
        e.play(&send, dc(4.0));
        assert_eq!(e.outputs_of(send.id), ["Main", "Phones"]);
    }

    #[test]
    fn losing_mains_device_keeps_its_voices_and_it_comes_back() {
        let mut e = Engine::fake(&["Speakers", "Dongle"]);
        e.configure(Some("Dongle"), &[route("Booth", "Speakers", 2), route("Desk", "Dongle", 2)], None);
        assert_eq!(e.device_name.as_deref(), Some("Dongle"));
        let bed = Tile { looping: true, ..tile() };
        let booth = Tile { sends: sends(&[("Booth", 0.0)]), ..tile() };
        let desk = Tile { sends: sends(&[("Desk", 0.0)]), ..tile() };
        for t in [&bed, &booth, &desk] {
            e.play(t, dc(4.0));
        }
        e.tick_without_device(0.5);
        let at = position(&e, bed.id);

        e.unplug("Dongle");
        let changes = e.check_streams();
        assert_eq!(
            changes,
            [
                OutputChange::Lost { output: "Desk".into(), device: "Dongle".into() },
                OutputChange::Main { from: Some("Dongle".into()), to: Some("Speakers".into()) },
            ]
        );
        assert_eq!(e.device_name.as_deref(), Some("Speakers"), "on the system default");
        assert_eq!(e.error, None);
        assert_eq!(position(&e, bed.id), at, "Main's voices carry on");
        assert_eq!(e.outputs_of(desk.id), ["Main"], "the Desk's tile moved to Main too");
        assert_eq!(e.outputs_of(booth.id), ["Booth"]);
        assert!(e.routes()[1].1, "the Booth, on the default device already, stays open");
        assert!(e.wants_reopen(), "Main isn't on its own device");

        e.plug("Dongle");
        let changes = e.reopen_closed();
        assert_eq!(
            changes,
            [
                OutputChange::Main { from: None, to: Some("Dongle".into()) },
                OutputChange::Back { output: "Desk".into(), device: "Dongle".into() },
            ]
        );
        assert_eq!(e.device_name.as_deref(), Some("Dongle"));
        assert_eq!(position(&e, bed.id), at, "still the same voice");
        assert!(e.routes().iter().all(|(_, open)| *open));
        assert_eq!(e.outputs_of(booth.id), ["Booth"]);
        assert!(!e.wants_reopen());
    }

    #[test]
    fn main_with_no_device_left_waits_for_one() {
        let mut e = Engine::fake(&["Speakers"]);
        e.configure(None, &[], None);
        let bed = tile();
        e.play(&bed, dc(4.0));
        e.unplug("Speakers");
        assert_eq!(e.check_streams(), [OutputChange::Main { from: Some("Speakers".into()), to: None }]);
        assert!(e.error.is_some());
        assert_eq!(e.state(bed.id), PlayState::Playing, "kept, and the clock still moves it");
        e.tick_without_device(0.5);
        assert!(position(&e, bed.id) > 0.4);
        assert!(e.reopen_closed().is_empty());
        e.plug("Speakers");
        assert_eq!(e.reopen_closed(), [OutputChange::Main { from: None, to: Some("Speakers".into()) }]);
        assert_eq!((e.device_name.as_deref(), e.error.as_deref()), (Some("Speakers"), None));
        assert_eq!(e.state(bed.id), PlayState::Playing);
    }

    #[test]
    fn a_lost_preview_stops_its_preview_and_reopens() {
        let mut e = Engine::fake(&["Speakers", "Headphones"]);
        e.configure(None, &[], Some(Some("Headphones")));
        let t = tile();
        e.preview(&t, dc(4.0)).unwrap();
        assert_eq!(e.outputs_of(t.id), ["Preview"]);
        e.unplug("Headphones");
        assert_eq!(e.check_streams(), [OutputChange::Lost { output: "Preview".into(), device: "Headphones".into() }]);
        assert_eq!(e.state(t.id), PlayState::Stopped, "a preview never moves to Main");
        e.plug("Headphones");
        assert_eq!(e.reopen_closed(), [OutputChange::Back { output: "Preview".into(), device: "Headphones".into() }]);
        e.preview(&t, dc(4.0)).unwrap();
        assert_eq!(e.outputs_of(t.id), ["Preview"]);
    }

    #[test]
    fn a_preview_with_its_device_missing_plays_nowhere() {
        let mut e = Engine::fake(&["Speakers", "Headphones"]);
        e.configure(None, &[], Some(Some("Headphones")));
        let t = tile();
        e.unplug("Headphones");
        e.check_streams();
        let err = e.preview(&t, dc(4.0)).unwrap_err();
        assert_eq!(err, "Preview's device Headphones isn't connected: nothing played");
        assert_eq!(e.state(t.id), PlayState::Stopped, "never on Main, the house");
        // A device that was never there is the same.
        e.configure(None, &[], Some(Some("Booth cans")));
        assert!(e.preview(&t, dc(4.0)).unwrap_err().contains("Booth cans isn't connected"));
        assert_eq!(e.state(t.id), PlayState::Stopped);
        // It comes back: previews play there again.
        e.configure(None, &[], Some(Some("Headphones")));
        e.unplug("Headphones");
        e.check_streams();
        e.plug("Headphones");
        e.reopen_closed();
        e.preview(&t, dc(4.0)).unwrap();
        assert_eq!(e.outputs_of(t.id), ["Preview"]);
        // Preview off: where the tile goes, as ever.
        e.configure(None, &[], None);
        e.preview(&t, dc(4.0)).unwrap();
        assert_eq!(e.outputs_of(t.id), ["Main"]);
    }

    #[test]
    fn an_output_whose_device_was_never_there_opens_when_it_arrives() {
        let mut e = Engine::fake(&["Speakers"]);
        e.configure(None, &[route("Phones", "Headphones", 0)], None);
        assert_eq!(e.output_errors, [("Phones".to_string(), "no device called \"Headphones\"".to_string())]);
        assert!(e.wants_reopen());
        e.plug("Headphones");
        assert_eq!(e.reopen_closed(), [OutputChange::Back { output: "Phones".into(), device: "Headphones".into() }]);
        assert!(e.output_errors.is_empty());
    }

    #[test]
    fn a_stream_that_stops_on_a_device_still_there_reopens_in_place() {
        let mut e = Engine::fake(&["Speakers", "Headphones"]);
        e.configure(None, &[route("Phones", "Headphones", 0)], None);
        let t = Tile { sends: sends(&[("Main", 0.0), ("Phones", -6.0)]), ..tile() };
        e.play(&t, dc(4.0));
        e.tick_without_device(0.5);
        let at = position(&e, t.id);
        e.glitch("Speakers");
        e.glitch("Headphones");
        assert!(e.check_streams().is_empty(), "nothing to tell");
        assert!(e.routes().iter().all(|(_, open)| *open));
        assert_eq!(e.outputs_of(t.id), ["Main", "Phones"]);
        assert_eq!(position(&e, t.id), at);
        // Failing again at once: the device is treated as gone until the next look.
        e.glitch("Headphones");
        assert_eq!(e.check_streams(), [OutputChange::Lost { output: "Phones".into(), device: "Headphones".into() }]);
        assert_eq!(e.reopen_closed(), [OutputChange::Back { output: "Phones".into(), device: "Headphones".into() }]);
    }

    #[test]
    fn a_moved_voice_keeps_its_fades_at_the_new_rate() {
        let mut v = {
            let mut m = Mixer::new(48_000);
            m.play(&tile(), dc(1.0));
            m.stop(m.voices[0].tile_id, 1.0);
            m.voices.remove(0)
        };
        v.elapsed = 4800;
        v.rescale(48_000, 44_100);
        assert_eq!(v.elapsed, 4410);
        assert!(matches!(v.state, VoiceState::Stopping { left: 44_100, total: 44_100, .. }), "{:?}", v.state);
    }
}
