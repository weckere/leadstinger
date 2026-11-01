//! Microphone input: one cpal input stream, opened only while something needs it (voice
//! ducking now; recording later). Its callback follows the level and, for voice ducking,
//! opens a gate while the voice is above a threshold. The mixer reads the gate (an atomic
//! flag) and dips the beds, so ducking needs no lock between the two audio threads.
//!
//! macOS asks for microphone access the first time the stream opens (the bundle carries
//! `NSMicrophoneUsageDescription`). PipeWire and CoreAudio let other apps (OBS, a call)
//! keep using the same microphone.

use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

/// The quietest level reported, dBFS.
pub const LEVEL_FLOOR: f32 = -90.0;

/// Shared between the input callback and the app.
pub struct InputShared {
    /// Smoothed level, dBFS (f32 bits).
    level: AtomicU32,
    /// The voice threshold, dBFS (f32 bits); NaN turns the gate off.
    threshold: AtomicU32,
    /// Open while the voice is above the threshold, and for a moment after.
    voice: Arc<AtomicBool>,
    /// While set, every block of samples (interleaved, as captured) goes here: recording.
    tap: Mutex<Option<Sender<Vec<f32>>>>,
}

impl InputShared {
    pub fn level_db(&self) -> f32 {
        f32::from_bits(self.level.load(Ordering::Relaxed))
    }
}

/// Follows a signal's level and gates on it: opens at once above the threshold, closes
/// once the level has stayed 3 dB below it for the hold time.
pub struct Follower {
    /// Mean square, smoothed (~10 ms).
    env: f32,
    coef: f32,
    hold_frames: u32,
    left: u32,
    pub open: bool,
}

impl Follower {
    pub fn new(rate: u32) -> Follower {
        let rate = rate.max(1) as f32;
        Follower { env: 0.0, coef: 1.0 - (-1.0 / (0.01 * rate)).exp(), hold_frames: (0.35 * rate) as u32, left: 0, open: false }
    }

    /// Take a block of interleaved samples; returns the level in dBFS afterwards.
    pub fn process(&mut self, samples: &[f32], channels: usize, threshold: Option<f32>) -> f32 {
        let channels = channels.max(1);
        let frames = samples.len() / channels;
        for frame in samples.chunks_exact(channels) {
            let x = frame.iter().sum::<f32>() / channels as f32;
            self.env += (x * x - self.env) * self.coef;
        }
        let db = if self.env > 0.0 { (10.0 * self.env.log10()).max(LEVEL_FLOOR) } else { LEVEL_FLOOR };
        match threshold {
            None => self.open = false,
            Some(t) if db > t => {
                self.open = true;
                self.left = self.hold_frames;
            }
            Some(t) if self.open && db < t - 3.0 => {
                self.left = self.left.saturating_sub(frames as u32);
                self.open = self.left > 0;
            }
            Some(_) => {}
        }
        db
    }
}

/// The input stream, kept alive while it's open. Like the output stream (audio.rs), it
/// is only held and dropped, so moving it between threads is safe on CoreAudio and ALSA.
struct InputStream(#[allow(dead_code)] cpal::Stream);

// SAFETY: see above; never touched except by Drop, and only built on macOS and Linux.
#[cfg(any(target_os = "macos", target_os = "linux"))]
unsafe impl Send for InputStream {}

/// Opens an input: `Input::open`, or a stand-in in tests. The controller calls it on a
/// thread of its own, because macOS holds the open for as long as its microphone
/// permission prompt is up.
pub type Opener = std::sync::Arc<dyn Fn(Option<&str>, Arc<AtomicBool>) -> Result<Input> + Send + Sync>;

pub struct Input {
    _stream: Option<InputStream>,
    pub shared: Arc<InputShared>,
    /// The device's name, as `input.list` shows it.
    pub device: String,
    /// The setting it was opened for (`None`: the system default).
    pub wanted: Option<String>,
    pub rate: u32,
    pub channels: usize,
}

impl Drop for Input {
    fn drop(&mut self) {
        self.shared.voice.store(false, Ordering::Relaxed);
    }
}

impl Input {
    /// The inputs' names, and the system default input's.
    pub fn devices() -> (Vec<String>, Option<String>) {
        let host = cpal::default_host();
        let names = host.input_devices().map(crate::audio::device_names).unwrap_or_default();
        (names, host.default_input_device().and_then(|d| crate::audio::device_name(&d)))
    }

    /// Open `device` (by name, else the system default). `voice` is the mixer's flag.
    pub fn open(device: Option<&str>, voice: Arc<AtomicBool>) -> Result<Input> {
        let host = cpal::default_host();
        let dev = match device {
            Some(name) => host
                .input_devices()?
                .find(|d| crate::audio::device_name(d).as_deref() == Some(name))
                .ok_or_else(|| anyhow!("no input called {name:?}"))?,
            None => host.default_input_device().ok_or_else(|| anyhow!("no microphone or other input"))?,
        };
        let name = crate::audio::device_name(&dev).unwrap_or_else(|| "Input".into());
        let (config, format) = crate::audio::stream_config(&dev, false)?;
        let shared = Arc::new(InputShared {
            level: AtomicU32::new(LEVEL_FLOOR.to_bits()),
            threshold: AtomicU32::new(f32::NAN.to_bits()),
            voice,
            tap: Mutex::new(None),
        });
        let stream = match format {
            cpal::SampleFormat::F32 => build::<f32>(&dev, &config, shared.clone())?,
            cpal::SampleFormat::I16 => build::<i16>(&dev, &config, shared.clone())?,
            cpal::SampleFormat::U16 => build::<u16>(&dev, &config, shared.clone())?,
            cpal::SampleFormat::I32 => build::<i32>(&dev, &config, shared.clone())?,
            f => return Err(anyhow!("unsupported input format {f:?}")),
        };
        stream.play()?;
        Ok(Input {
            _stream: Some(InputStream(stream)),
            shared,
            device: name,
            wanted: device.map(str::to_owned),
            rate: config.sample_rate,
            channels: config.channels as usize,
        })
    }

    /// An input with no device behind it: tests stand it in for a microphone.
    #[cfg(test)]
    pub fn fake(device: &str, wanted: Option<&str>, voice: Arc<AtomicBool>) -> Input {
        let shared = Arc::new(InputShared {
            level: AtomicU32::new(LEVEL_FLOOR.to_bits()),
            threshold: AtomicU32::new(f32::NAN.to_bits()),
            voice,
            tap: Mutex::new(None),
        });
        Input { _stream: None, shared, device: device.to_owned(), wanted: wanted.map(str::to_owned), rate: 48_000, channels: 1 }
    }

    /// Start receiving the captured samples (interleaved, `channels` wide, at `rate`).
    pub fn tap(&self) -> Receiver<Vec<f32>> {
        let (tx, rx) = channel();
        *self.shared.tap.lock().unwrap_or_else(|e| e.into_inner()) = Some(tx);
        rx
    }

    pub fn untap(&self) {
        *self.shared.tap.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }

    /// The voice gate's threshold in dBFS, or `None` to keep it closed.
    pub fn set_threshold(&self, db: Option<f32>) {
        self.shared.threshold.store(db.unwrap_or(f32::NAN).to_bits(), Ordering::Relaxed);
    }
}

/// Whether the input hears the board (D-127): a microphone near the speakers picks up the
/// tiles, so voice ducking dips the beds at their own sound and they pump. Ten times a
/// second the controller gives it the board's level and the input's; over the last eight
/// seconds, while the board was playing, an input that rises and falls with the board
/// (correlation 0.6 or more) and gets within 6 dB of the threshold hears it. Talking over
/// the board doesn't count: the voice comes and goes on its own, and ducking pulls the
/// board down while it's there. Once heard, it stays heard for 20 s, so the warning
/// doesn't flicker.
#[derive(Default)]
pub struct HearsBoard {
    /// (board dB, input dB), oldest first.
    samples: std::collections::VecDeque<(f32, f32)>,
    clock: f64,
    /// Seconds since the input last heard the board.
    since: Option<f64>,
}

impl HearsBoard {
    /// How often it samples, seconds.
    pub const EVERY: f64 = 0.1;
    const WINDOW: usize = 80;
    const LATCH: f64 = 20.0;
    /// The board counts as playing above this.
    const PLAYING_DB: f32 = -60.0;

    /// Advance `dt` seconds; sample the levels (dBFS) when it's time. True the moment the
    /// input starts hearing the board.
    pub fn feed(&mut self, dt: f64, board_db: f32, input_db: f32, threshold_db: f32) -> bool {
        let was = self.hears();
        if let Some(s) = &mut self.since {
            *s += dt;
        }
        self.clock += dt;
        if self.clock < Self::EVERY {
            return false;
        }
        self.clock = 0.0;
        let floor = crate::logic::DB_FLOOR;
        self.samples.push_back((board_db.max(floor), input_db.max(floor)));
        if self.samples.len() > Self::WINDOW {
            self.samples.pop_front();
        }
        if echoes(&self.samples, threshold_db) {
            self.since = Some(0.0);
        }
        !was && self.hears()
    }

    pub fn hears(&self) -> bool {
        self.since.is_some_and(|s| s < Self::LATCH)
    }

    pub fn clear(&mut self) {
        *self = HearsBoard::default();
    }
}

/// The input follows the board: enough playing, the board's level moving, the input's
/// moving with it, and loud enough to matter.
fn echoes(samples: &std::collections::VecDeque<(f32, f32)>, threshold_db: f32) -> bool {
    let playing = samples.iter().filter(|(b, _)| *b > HearsBoard::PLAYING_DB).count();
    if playing < 20 {
        return false;
    }
    let loud = samples.iter().filter(|(b, _)| *b > HearsBoard::PLAYING_DB).map(|(_, i)| *i).fold(f32::MIN, f32::max);
    if loud < threshold_db - 6.0 {
        return false;
    }
    let n = samples.len() as f32;
    let (mb, mi) = samples.iter().fold((0.0, 0.0), |(b, i), (sb, si)| (b + sb / n, i + si / n));
    let (mut vb, mut vi, mut cov) = (0.0_f32, 0.0_f32, 0.0_f32);
    for (b, i) in samples {
        vb += (b - mb) * (b - mb) / n;
        vi += (i - mi) * (i - mi) / n;
        cov += (b - mb) * (i - mi) / n;
    }
    // The board must move by a few dB, and the input with it.
    vb.sqrt() >= 3.0 && vi.sqrt() >= 1.5 && cov / (vb * vi).sqrt() >= 0.6
}

fn build<T>(dev: &cpal::Device, config: &cpal::StreamConfig, shared: Arc<InputShared>) -> Result<cpal::Stream>
where
    T: cpal::SizedSample,
    f32: cpal::FromSample<T>,
{
    let channels = config.channels as usize;
    let mut follower = Follower::new(config.sample_rate);
    let mut buf: Vec<f32> = Vec::with_capacity(8192);
    let stream = dev.build_input_stream(
        *config,
        move |data: &[T], _| {
            buf.clear();
            buf.extend(data.iter().map(|s| <f32 as cpal::FromSample<T>>::from_sample_(*s)));
            let t = f32::from_bits(shared.threshold.load(Ordering::Relaxed));
            let db = follower.process(&buf, channels, (!t.is_nan()).then_some(t));
            shared.level.store(db.to_bits(), Ordering::Relaxed);
            shared.voice.store(follower.open, Ordering::Relaxed);
            // Only contended while a recording starts or stops; skipping one block then
            // is better than making the audio thread wait.
            if let Ok(tap) = shared.tap.try_lock() {
                if let Some(tx) = tap.as_ref() {
                    tx.send(buf.clone()).ok();
                }
            }
        },
        |err| {
            if err.kind() != cpal::ErrorKind::Xrun {
                eprintln!("{}: input stream error: {err}", crate::brand::SLUG);
            }
        },
        None,
    )?;
    Ok(stream)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(frames: usize, amp: f32) -> Vec<f32> {
        (0..frames)
            .flat_map(|i| {
                let s = amp * (i as f32 * 0.1).sin();
                [s, s]
            })
            .collect()
    }

    #[test]
    fn follows_the_level_and_gates_with_a_hold() {
        let mut f = Follower::new(1000);
        // A sine at 0.1 reads about -23 dBFS (RMS 0.0707); the 10 ms follower ripples a
        // little on a tone this slow.
        let db = f.process(&tone(200, 0.1), 2, Some(-30.0));
        assert!((db + 23.0).abs() < 2.0, "{db}");
        assert!(f.open);
        // Silence: the level falls, but the gate holds for 0.35 s before it closes.
        f.process(&vec![0.0; 2 * 100], 2, Some(-30.0));
        assert!(f.open, "still in the hold");
        f.process(&vec![0.0; 2 * 400], 2, Some(-30.0));
        assert!(!f.open);
        // No threshold: never opens.
        f.process(&tone(200, 0.5), 2, None);
        assert!(!f.open);
    }

    /// Feed `secs` of levels from `f(t)` → (board, input).
    fn run(h: &mut HearsBoard, secs: f64, threshold: f32, f: impl Fn(f64) -> (f32, f32)) -> bool {
        let mut rose = false;
        let mut t = 0.0;
        while t < secs {
            let (b, i) = f(t);
            rose |= h.feed(0.05, b, i, threshold);
            t += 0.05;
        }
        rose
    }

    #[test]
    fn hears_a_board_the_microphone_picks_up() {
        // A hardware test: a laptop's built-in mic heard the speakers at -32..-39 dB with
        // the threshold at -34. A bed starts and stops, and ducking makes it pump.
        let mut h = HearsBoard::default();
        let pumping = |t: f64| {
            let board = if (t as u32 / 2).is_multiple_of(2) { -14.0 } else { -26.0 };
            (board, board - 20.0 + (t * 7.0).sin() as f32)
        };
        assert!(run(&mut h, 8.0, -34.0, pumping), "it starts hearing it");
        assert!(h.hears());
        // It stays heard for 20 s after the board leaves the 8 s window, then goes.
        run(&mut h, 20.0, -34.0, |_| (-80.0, -62.0));
        assert!(h.hears(), "latched");
        run(&mut h, 10.0, -34.0, |_| (-80.0, -62.0));
        assert!(!h.hears());
    }

    #[test]
    fn talking_over_the_board_is_not_hearing_it() {
        let mut h = HearsBoard::default();
        // Headphones: the bed plays and ducks while the voice is there; the mic hears only
        // the voice.
        let talk = |t: f64| {
            let voice = (t as u32 / 3).is_multiple_of(2);
            if voice { (-26.0, -20.0) } else { (-14.0, -58.0) }
        };
        assert!(!run(&mut h, 12.0, -34.0, talk));
        h.clear();
        // A mic that follows the board, but far under the threshold, doesn't matter.
        let faint = |t: f64| {
            let board = if (t as u32 / 2).is_multiple_of(2) { -14.0 } else { -40.0 };
            (board, board - 50.0)
        };
        assert!(!run(&mut h, 12.0, -34.0, faint));
        h.clear();
        // Nothing playing: nothing to hear.
        assert!(!run(&mut h, 12.0, -34.0, |t| (-80.0, -40.0 + (t * 3.0).sin() as f32 * 10.0)));
        assert!(!h.hears());
    }
}
