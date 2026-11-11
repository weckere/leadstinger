//! A small synthesizer for the demo library: speech-like babble, drum and bell hits,
//! shaped noise and music beds, so the demo board's waveforms look like a real board's.
//!
//! Every sound comes from a fixed seed and only `+ - * /` and `floor` on `f64` (the sine
//! and exponential below are polynomials, not the platform's libm), so the samples are
//! the same on every run and every platform.

pub const RATE: u32 = 44_100;
const FS: f64 = RATE as f64;

/// A deterministic random source (xorshift64*).
#[derive(Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    /// Uniform in [0, 1).
    fn f(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    /// Uniform in [lo, hi).
    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.f()
    }
    /// White noise in [-1, 1).
    fn bi(&mut self) -> f64 {
        self.f() * 2.0 - 1.0
    }
    /// A new, independent source.
    fn fork(&mut self) -> Rng {
        Rng::new(self.next())
    }
}

const PI: f64 = std::f64::consts::PI;

/// sin(2π·`cycles`).
pub fn sin1(cycles: f64) -> f64 {
    let r = cycles - cycles.floor(); // [0, 1)
    // To [-π, π), then fold to [-π/2, π/2] where the series converges fast.
    let mut x = (r - if r >= 0.5 { 1.0 } else { 0.0 }) * 2.0 * PI;
    if x > PI / 2.0 {
        x = PI - x;
    } else if x < -PI / 2.0 {
        x = -PI - x;
    }
    let x2 = x * x;
    let mut term = x;
    let mut sum = x;
    for k in 1..8 {
        let k = k as f64;
        term *= -x2 / ((2.0 * k) * (2.0 * k + 1.0));
        sum += term;
    }
    sum
}

fn cos1(cycles: f64) -> f64 {
    sin1(cycles + 0.25)
}

/// e^`x`.
pub fn exp(x: f64) -> f64 {
    if x < -700.0 {
        return 0.0;
    }
    let n = (x / std::f64::consts::LN_2 + 0.5).floor();
    let r = x - n * std::f64::consts::LN_2; // |r| <= ln2/2
    let mut term = 1.0;
    let mut sum = 1.0;
    for k in 1..14 {
        term *= r / k as f64;
        sum += term;
    }
    // Scaling by a power of two is exact.
    let mut scale = 1.0;
    let (mut b, mut e) = (if n < 0.0 { 0.5 } else { 2.0 }, n.abs() as u32);
    while e > 0 {
        if e & 1 == 1 {
            scale *= b;
        }
        b *= b;
        e >>= 1;
    }
    sum * scale
}

/// A note's frequency, from semitones above A4 (440 Hz).
fn note(semis: f64) -> f64 {
    440.0 * exp(semis / 12.0 * std::f64::consts::LN_2)
}

/// One-pole low-pass.
#[derive(Default, Clone)]
struct Lp {
    y: f64,
}

impl Lp {
    fn run(&mut self, x: f64, hz: f64) -> f64 {
        let a = 1.0 - exp(-2.0 * PI * hz / FS);
        self.y += a * (x - self.y);
        self.y
    }
}

/// One-pole high-pass.
#[derive(Default, Clone)]
struct Hp {
    lp: Lp,
}

impl Hp {
    fn run(&mut self, x: f64, hz: f64) -> f64 {
        x - self.lp.run(x, hz)
    }
}

/// A band-pass biquad (RBJ, 0 dB peak gain).
#[derive(Default, Clone)]
struct Bp {
    c: [f64; 5],
    x: [f64; 2],
    y: [f64; 2],
}

impl Bp {
    fn tune(&mut self, hz: f64, q: f64) {
        let w = hz / FS;
        let alpha = sin1(w) / (2.0 * q);
        let a0 = 1.0 + alpha;
        self.c = [alpha / a0, 0.0, -alpha / a0, -2.0 * cos1(w) / a0, (1.0 - alpha) / a0];
    }
    fn new(hz: f64, q: f64) -> Bp {
        let mut b = Bp::default();
        b.tune(hz, q);
        b
    }
    fn run(&mut self, x: f64) -> f64 {
        let [b0, b1, b2, a1, a2] = self.c;
        let y = b0 * x + b1 * self.x[0] + b2 * self.x[1] - a1 * self.y[0] - a2 * self.y[1];
        self.x = [x, self.x[0]];
        self.y = [y, self.y[0]];
        y
    }
}

/// A mono buffer that sounds are mixed into.
pub struct Mix {
    s: Vec<f64>,
}

impl Mix {
    pub fn new(seconds: f64) -> Mix {
        Mix { s: vec![0.0; (seconds * FS) as usize] }
    }
    fn seconds(&self) -> f64 {
        self.s.len() as f64 / FS
    }
    /// Add `f(t)` for `len` seconds from `at`, `t` counting from 0.
    fn add(&mut self, at: f64, len: f64, mut f: impl FnMut(f64) -> f64) {
        let start = (at.max(0.0) * FS) as usize;
        let end = (((at + len) * FS) as usize).min(self.s.len());
        for i in start..end {
            self.s[i] += f((i - start) as f64 / FS);
        }
    }
    /// Multiply everything by `f(t)`.
    fn shape(&mut self, f: impl Fn(f64) -> f64) {
        for (i, s) in self.s.iter_mut().enumerate() {
            *s *= f(i as f64 / FS);
        }
    }
    /// Scale to `peak`, with 5 ms ramps at both ends so nothing clicks.
    pub fn finish(self, peak: f64) -> Vec<f32> {
        let max = self.s.iter().fold(0.0f64, |m, s| m.max(s.abs()));
        let n = self.s.len();
        let ramp = (FS * 0.005) as usize;
        let g = if max > 0.0 { peak / max } else { 0.0 };
        self.s
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let edge = (i.min(n - 1 - i) as f64 / ramp as f64).min(1.0);
                (s * g * edge) as f32
            })
            .collect()
    }
}

/// Attack, hold at 1, then release over `rel`, all in seconds; 0 outside.
fn env(t: f64, att: f64, hold: f64, rel: f64) -> f64 {
    if t < 0.0 {
        0.0
    } else if t < att {
        t / att
    } else if t < att + hold {
        1.0
    } else if t < att + hold + rel {
        1.0 - (t - att - hold) / rel
    } else {
        0.0
    }
}

/// A rising-then-falling bump between `a` and `b`: 0 at the ends, 1 in the middle.
fn bump(t: f64, a: f64, b: f64) -> f64 {
    if t <= a || t >= b {
        0.0
    } else {
        let x = (t - a) / (b - a);
        (sin1(x * 0.5)).powi(2)
    }
}

// --- percussion ---

fn kick(m: &mut Mix, at: f64, g: f64) {
    m.add(at, 0.45, |t| {
        let phase = 48.0 * t + 110.0 * (1.0 - exp(-t * 28.0)) / 28.0;
        sin1(phase) * exp(-t * 9.0) * g
    });
}

fn snare(m: &mut Mix, at: f64, g: f64, rng: &mut Rng) {
    let mut r = rng.fork();
    let mut hp = Hp::default();
    m.add(at, 0.3, move |t| {
        let n = hp.run(r.bi(), 1800.0);
        (n * 0.9 * exp(-t * 20.0) + sin1(190.0 * t) * 0.6 * exp(-t * 32.0)) * g
    });
}

fn hat(m: &mut Mix, at: f64, g: f64, rng: &mut Rng) {
    let mut r = rng.fork();
    let mut hp = Hp::default();
    m.add(at, 0.08, move |t| hp.run(r.bi(), 7000.0) * exp(-t * 60.0) * g);
}

fn tom(m: &mut Mix, at: f64, hz: f64, g: f64) {
    m.add(at, 0.35, |t| {
        let phase = hz * t + hz * 0.6 * (1.0 - exp(-t * 18.0)) / 18.0;
        sin1(phase) * exp(-t * 11.0) * g
    });
}

fn crash(m: &mut Mix, at: f64, len: f64, g: f64, rng: &mut Rng) {
    let mut r = rng.fork();
    let (mut hp, mut lp) = (Hp::default(), Lp::default());
    m.add(at, len, move |t| {
        let n = lp.run(hp.run(r.bi(), 3000.0), 11_000.0);
        let e = exp(-t * 4.5 / len) * (1.0 - exp(-t * 400.0));
        n * e * g
    });
}

/// A hand clap: three quick bursts of band-passed noise, then a short tail.
fn clap(m: &mut Mix, at: f64, g: f64, hz: f64, rng: &mut Rng) {
    let mut r = rng.fork();
    let mut bp = Bp::new(hz, 1.4);
    m.add(at, 0.12, move |t| {
        let bursts = if t < 0.022 { exp(-(t % 0.0075) * 300.0) } else { exp(-(t - 0.022) * 45.0) };
        bp.run(r.bi()) * bursts * g
    });
}

fn bell(m: &mut Mix, at: f64, hz: f64, len: f64, g: f64) {
    const PARTIALS: [(f64, f64, f64); 5] = [(1.0, 1.0, 1.0), (2.0, 0.5, 1.6), (2.76, 0.4, 2.3), (5.4, 0.25, 3.5), (8.93, 0.12, 5.0)];
    m.add(at, len, |t| {
        let s: f64 = PARTIALS.iter().map(|&(r, a, d)| sin1(hz * r * t) * a * exp(-t * d * 4.0 / len)).sum();
        s * (1.0 - exp(-t * 900.0)) * g
    });
}

// --- pitched voices ---

/// A brass-like note: harmonics that brighten as it gets louder.
fn brass(m: &mut Mix, at: f64, hz: f64, len: f64, g: f64, vibrato: f64) {
    let mut phase = 0.0;
    m.add(at, len + 0.15, move |t| {
        let e = env(t, 0.04, len - 0.04, 0.15).min(1.0) * (1.0 - 0.15 * (t / len.max(0.1)).min(1.0));
        let wobble = sin1(5.5 * t) * (t / len.max(0.1)).min(1.0);
        let vib = 1.0 + vibrato * wobble;
        // A wide vibrato swells and sags the note too.
        let e = e * (1.0 + 10.0 * vibrato * wobble);
        phase += hz * vib / FS;
        let bright = 0.25 + 0.6 * e;
        let mut s = 0.0;
        let mut w = 1.0;
        for k in 1..9 {
            s += sin1(phase * k as f64) * w;
            w *= bright;
        }
        s * e * g * 0.5
    });
}

/// A plucked or piano-ish note.
fn pluck(m: &mut Mix, at: f64, hz: f64, len: f64, g: f64) {
    m.add(at, len, |t| {
        let s = sin1(hz * t) + 0.45 * sin1(hz * 2.0 * t) * exp(-t * 6.0) + 0.2 * sin1(hz * 3.0 * t) * exp(-t * 9.0);
        s * exp(-t * 3.2 / len) * (1.0 - exp(-t * 600.0)) * g
    });
}

/// A soft pad chord with a slow swell.
fn pad(m: &mut Mix, at: f64, len: f64, hzs: &[f64], g: f64) {
    let hzs = hzs.to_vec();
    let mut lp = Lp::default();
    m.add(at, len, move |t| {
        let s: f64 = hzs.iter().map(|&f| sin1(f * t) + 0.3 * sin1(f * 2.003 * t) + 0.12 * sin1(f * 3.01 * t)).sum();
        lp.run(s, 1500.0) * bump(t, 0.0, len).sqrt() * g
    });
}

fn bass(m: &mut Mix, at: f64, hz: f64, len: f64, g: f64) {
    m.add(at, len, |t| (sin1(hz * t) + 0.3 * sin1(hz * 2.0 * t)) * env(t, 0.01, len * 0.6, len * 0.35) * g);
}

/// A music bed: chords, bass, an arpeggio and (if `drums`) a kit, with phrase dynamics.
fn music(m: &mut Mix, bpm: f64, chords: &[[i32; 3]], drums: bool, seed: u64) {
    let mut rng = Rng::new(seed);
    let beat = 60.0 / bpm;
    let bar = beat * 4.0;
    let total = m.seconds();
    let bars = (total / bar).ceil() as usize;
    for b in 0..bars {
        let t0 = b as f64 * bar;
        let chord = chords[b % chords.len()];
        let hz: Vec<f64> = chord.iter().map(|&n| note(n as f64 - 12.0)).collect();
        // Four-bar phrases that build, and a lift every eighth bar.
        let phrase = 0.65 + 0.35 * ((b % 4) as f64 / 3.0) + if b % 8 >= 4 { 0.15 } else { 0.0 };
        pad(m, t0, bar * 1.05, &hz, 0.10 * phrase);
        for i in 0..4 {
            let bt = t0 + i as f64 * beat;
            let root = note(chord[0] as f64 - 36.0);
            if i % 2 == 0 || drums {
                bass(m, bt, if i == 3 { root * 1.5 } else { root }, beat * 0.9, 0.35 * phrase);
            }
            if drums {
                kick(m, bt, 0.9 * phrase);
                if i % 2 == 1 {
                    snare(m, bt, 0.45 * phrase, &mut rng);
                }
                hat(m, bt + beat / 2.0, 0.18 * phrase, &mut rng);
            }
        }
        // An eighth-note arpeggio, a few notes left out at random so it breathes.
        for e in 0..8 {
            if rng.f() < 0.2 {
                continue;
            }
            let n = chord[[0, 1, 2, 1, 0, 2, 1, 2][e]] as f64 + if e >= 4 && b % 2 == 1 { 12.0 } else { 0.0 };
            pluck(m, t0 + e as f64 * beat / 2.0, note(n), beat * 1.5, rng.range(0.15, 0.3) * phrase);
        }
    }
}

// --- voices ---

/// Vowel formants (Hz).
const VOWELS: [[f64; 3]; 6] = [
    [730.0, 1090.0, 2440.0], // a
    [530.0, 1840.0, 2480.0], // e
    [300.0, 2200.0, 3000.0], // i
    [570.0, 840.0, 2410.0],  // o
    [330.0, 950.0, 2250.0],  // u
    [640.0, 1190.0, 2390.0], // uh
];

/// Speech-like babble: `shape` gives syllables per word ("2 1 3"), with `|` between
/// phrases. `f0` is the voice's pitch; `lively` how much it moves.
fn speech(m: &mut Mix, at: f64, shape: &str, f0: f64, lively: f64, g: f64, rng: &mut Rng) -> f64 {
    // Plan the syllables: (start, length, loudness, pitch, vowel, consonant kind).
    struct Syl {
        at: f64,
        len: f64,
        amp: f64,
        pitch: f64,
        vowel: usize,
        onset: u8,
    }
    let mut syls = Vec::new();
    let mut t = at;
    let phrases: Vec<&str> = shape.split('|').collect();
    for (pi, phrase) in phrases.iter().enumerate() {
        let words: Vec<usize> = phrase.split_whitespace().filter_map(|w| w.parse().ok()).collect();
        let n_syl: usize = words.iter().sum::<usize>().max(1);
        let mut k = 0;
        let last_phrase = pi + 1 == phrases.len();
        for (wi, &count) in words.iter().enumerate() {
            let stress = rng.next() as usize % count.max(1);
            for s in 0..count {
                let pos = k as f64 / n_syl as f64;
                // Pitch falls over a phrase; the last one may rise (a question, an exclamation).
                let contour = if last_phrase && pos > 0.75 { 1.0 + lively * 0.6 * (pos - 0.75) * 4.0 } else { 1.0 - 0.18 * pos };
                let stressed = s == stress;
                let len = rng.range(0.11, 0.2) + if stressed { 0.06 } else { 0.0 } + if k + 1 == n_syl { 0.12 } else { 0.0 };
                syls.push(Syl {
                    at: t,
                    len,
                    amp: if stressed { rng.range(0.85, 1.0) } else { rng.range(0.4, 0.75) },
                    pitch: f0 * contour * (1.0 + if stressed { lively * 0.25 } else { 0.0 }) * rng.range(0.96, 1.04),
                    vowel: rng.next() as usize % VOWELS.len(),
                    onset: (rng.next() % 4) as u8,
                });
                t += len + rng.range(0.0, 0.03);
                k += 1;
            }
            if wi + 1 < words.len() {
                t += rng.range(0.03, 0.12);
            }
        }
        t += rng.range(0.3, 0.5);
    }
    let end = t;
    let len = end - at + 0.2;
    // One pass over the whole line, so pitch and formants glide between syllables.
    let mut src_lp = Lp::default();
    let mut fs: Vec<Bp> = VOWELS[0].iter().map(|&f| Bp::new(f, 6.0)).collect();
    let mut cur = VOWELS[0];
    let mut pitch = syls.first().map_or(f0, |s| s.pitch);
    let mut phase = 0.0;
    let mut noise_hp = Hp::default();
    let mut r = rng.fork();
    let mut i = 0usize;
    let mut idx = 0usize;
    m.add(at, len, move |tt| {
        let t = at + tt;
        while idx + 1 < syls.len() && t >= syls[idx + 1].at {
            idx += 1;
        }
        let s = &syls[idx];
        let local = t - s.at;
        let target = VOWELS[s.vowel];
        if i.is_multiple_of(64) {
            for (j, f) in fs.iter_mut().enumerate() {
                cur[j] += (target[j] - cur[j]) * 0.08;
                f.tune(cur[j], 5.0 + j as f64 * 2.0);
            }
        }
        i += 1;
        pitch += (s.pitch - pitch) * 0.002;
        let jitter = 1.0 + 0.01 * r.bi();
        phase += pitch * jitter / FS;
        let glottal = 2.0 * (phase - phase.floor()) - 1.0;
        let src = src_lp.run(glottal, 3000.0) + 0.04 * r.bi();
        let voiced = fs[0].run(src) * 1.0 + fs[1].run(src) * 0.6 + fs[2].run(src) * 0.3;
        // Voicing: a quick rise after the consonant, sagging through the syllable.
        let sag = 1.0 - 0.45 * ((local - 0.03) / s.len).clamp(0.0, 1.0);
        let ve = env(local - 0.03, 0.035, (s.len - 0.12).max(0.02), 0.06) * sag * (1.0 + 0.12 * sin1(local * 9.0)) * s.amp;
        // Consonants: 0 = none, 1 = a plosive tick, 2 = a hiss, 3 = a soft hiss.
        let ce = match s.onset {
            1 => exp(-local * 120.0) * 0.9,
            2 => env(local, 0.01, 0.04, 0.02) * 0.45,
            3 => env(local, 0.01, 0.025, 0.015) * 0.25,
            _ => 0.0,
        } * s.amp;
        let hiss = noise_hp.run(r.bi(), 3500.0);
        (voiced * ve * 1.6 + hiss * ce) * g
    });
    end
}

// --- noise textures ---

/// Lowpassed noise whose loudness follows `shape(t)`.
fn rumble(m: &mut Mix, at: f64, len: f64, hz: f64, g: f64, rng: &mut Rng, shape: impl Fn(f64) -> f64) {
    let mut r = rng.fork();
    let (mut a, mut b) = (Lp::default(), Lp::default());
    m.add(at, len, move |t| b.run(a.run(r.bi(), hz), hz) * shape(t) * g);
}

/// What a demo tile sounds like.
#[derive(Clone, Copy, Debug)]
pub enum Kind {
    /// A spoken line: syllable shape, pitch, liveliness.
    Speech(&'static str, f64, f64),
    Applause,
    Rimshot,
    SadTrombone,
    AirHorn,
    Crickets,
    DrumRoll,
    Ding,
    Crowd,
    Buzzer,
    Fanfare,
    /// A music bed: tempo, drums, chords (semitones above A4, three notes a bar).
    Music(f64, bool, &'static [[i32; 3]]),
    Sweep,
    DoorSlam,
    PhoneRing,
    Thunder,
}

/// Render `kind`, `seconds` long, from `seed`.
pub fn render(kind: Kind, seconds: f64, seed: u64) -> Vec<f32> {
    let mut m = Mix::new(seconds);
    let mut rng = Rng::new(seed);
    let peak = match kind {
        Kind::Speech(shape, f0, lively) => {
            speech(&mut m, 0.08, shape, f0, lively, 1.0, &mut rng);
            0.82
        }
        Kind::Applause => {
            // Claps arrive faster as the crowd joins in, then thin out.
            let mut t = 0.02;
            while t < seconds - 0.2 {
                let density = bump(t, 0.0, seconds * 0.98).sqrt() * if t < 0.6 { t / 0.6 } else { 1.0 };
                let rate = 8.0 + 90.0 * density;
                let hz = rng.range(900.0, 2200.0);
                let g = rng.range(0.3, 1.0) * (0.25 + density);
                clap(&mut m, t, g, hz, &mut rng);
                t += -((1.0 - rng.f()).max(1e-9)).ln_approx() / rate;
            }
            // A woo near the peak.
            rumble(&mut m, 0.0, seconds, 1200.0, 0.3, &mut rng, |t| bump(t, 0.4, seconds * 0.8));
            0.8
        }
        Kind::Rimshot => {
            tom(&mut m, 0.0, 170.0, 0.8);
            snare(&mut m, 0.0, 0.25, &mut rng);
            tom(&mut m, 0.17, 120.0, 0.85);
            kick(&mut m, 0.36, 0.9);
            snare(&mut m, 0.36, 0.6, &mut rng);
            crash(&mut m, 0.36, 0.85, 0.9, &mut rng);
            0.88
        }
        Kind::SadTrombone => {
            let notes = [-7.0, -8.0, -9.0, -10.0]; // D4 C#4 C4 B3, falling
            for (i, n) in notes.iter().enumerate() {
                let last = i == 3;
                brass(&mut m, 0.05 + i as f64 * 0.6, note(n - 12.0), if last { 1.9 } else { 0.42 }, if last { 1.0 } else { 0.85 }, if last { 0.025 } else { 0.004 });
            }
            // The last note gives up.
            m.shape(|t| 1.0 - 0.7 * ((t - 2.0) / 1.5).clamp(0.0, 1.0));
            0.8
        }
        Kind::AirHorn => {
            for &(at, len) in &[(0.0, 0.22), (0.3, 0.22), (0.62, 1.15)] {
                let mut lp = Lp::default();
                m.add(at, len + 0.08, move |t| {
                    let e = if t < 0.004 { t / 0.004 } else if t < len { 1.0 } else { 1.0 - (t - len) / 0.08 };
                    let droop = 1.0 - 0.06 * ((t - len + 0.15).max(0.0) / 0.15).min(1.0);
                    let s: f64 = [466.2, 587.3, 698.5].iter().map(|f| {
                        let p = f * droop * t;
                        2.0 * (p - p.floor()) - 1.0
                    }).sum();
                    lp.run(s, 4000.0) * e.max(0.0)
                });
            }
            0.85
        }
        Kind::Crickets => {
            for &(hz, period, g) in &[(4500.0, 0.48, 1.0), (4900.0, 0.63, 0.55), (4150.0, 0.81, 0.35)] {
                let mut t = rng.range(0.0, period);
                while t < seconds - 0.2 {
                    let pulses = 3 + (rng.next() % 2) as usize;
                    let gg = g * rng.range(0.7, 1.0);
                    for p in 0..pulses {
                        m.add(t + p as f64 * 0.028, 0.02, |x| sin1(hz * x) * bump(x, 0.0, 0.02) * gg);
                    }
                    t += period * rng.range(0.85, 1.15);
                }
            }
            rumble(&mut m, 0.0, seconds, 600.0, 0.15, &mut rng, |_| 1.0);
            0.5
        }
        Kind::DrumRoll => {
            let end = seconds - 1.0;
            let mut t = 0.0;
            while t < end {
                let x = t / end;
                let rate = 9.0 + 16.0 * x.min(0.3) / 0.3;
                let g = (0.15 + 0.85 * x * x) * rng.range(0.75, 1.0);
                snare(&mut m, t, g, &mut rng);
                t += 1.0 / rate;
            }
            kick(&mut m, end, 1.0);
            snare(&mut m, end, 1.1, &mut rng);
            crash(&mut m, end, 1.0, 1.2, &mut rng);
            0.88
        }
        Kind::Ding => {
            bell(&mut m, 0.0, 1318.5, seconds, 1.0);
            0.85
        }
        Kind::Crowd => {
            // A room full of people talking, quieter and nearer by turns.
            let mut t = 0.0;
            while t < seconds {
                let f0 = if rng.f() < 0.5 { rng.range(95.0, 130.0) } else { rng.range(180.0, 240.0) };
                let words: Vec<String> = (0..rng.range(3.0, 8.0) as usize).map(|_| (1 + rng.next() % 3).to_string()).collect();
                let g = rng.range(0.15, 0.5);
                speech(&mut m, t, &words.join(" "), f0, 0.4, g, &mut rng);
                t += rng.range(0.15, 0.6);
            }
            rumble(&mut m, 0.0, seconds, 400.0, 2.0, &mut rng, |t| 0.6 + 0.4 * sin1(t / 11.0));
            m.shape(|t| 0.55 + 0.3 * sin1(t / 7.5) + 0.15 * sin1(t / 3.1 + 0.3));
            0.5
        }
        Kind::Buzzer => {
            for &(at, len) in &[(0.0, 0.3), (0.42, 0.85)] {
                let mut lp = Lp::default();
                m.add(at, len, move |t| {
                    let sq = |f: f64| if (f * t) - (f * t).floor() < 0.5 { 1.0 } else { -1.0 };
                    lp.run(sq(110.0) + sq(116.5), 1800.0) * env(t, 0.005, len - 0.045, 0.04)
                });
            }
            0.75
        }
        Kind::Fanfare => {
            // Three pickups, a held note, a turn, then the big chord over a timpani roll.
            let line: [(f64, f64, f64); 6] = [(0.0, -9.0, 0.14), (0.2, -5.0, 0.14), (0.4, -2.0, 0.14), (0.6, 3.0, 0.75), (1.5, 0.0, 0.2), (1.8, 3.0, 1.0)];
            for &(at, n, len) in &line {
                brass(&mut m, at, note(n), len, 0.9, 0.006);
                brass(&mut m, at, note(n - 12.0), len, 0.4, 0.0);
            }
            for (i, &n) in [-9.0, -5.0, -2.0, 3.0].iter().enumerate() {
                brass(&mut m, 3.1 + i as f64 * 0.02, note(n), seconds - 3.6, 0.55, 0.012);
            }
            for &at in &[0.6, 1.8] {
                tom(&mut m, at, 90.0, 0.9);
            }
            let mut t = 2.7;
            while t < 3.1 {
                tom(&mut m, t, 85.0, 0.3 + (t - 2.7) * 1.5);
                t += 0.05;
            }
            tom(&mut m, 3.1, 80.0, 1.2);
            crash(&mut m, 3.1, 2.5, 0.5, &mut rng);
            m.shape(|t| if t > seconds - 1.5 { (seconds - t) / 1.5 } else { 1.0 });
            0.85
        }
        Kind::Music(bpm, drums, chords) => {
            music(&mut m, bpm, chords, drums, seed);
            // Fade in over a bar and out over the last three seconds.
            let fade_in = 240.0 / bpm;
            m.shape(|t| (t / fade_in).min(1.0) * ((seconds - t) / 3.0).clamp(0.0, 1.0));
            0.7
        }
        Kind::Sweep => {
            // Low air that swells and settles, as the house goes dark.
            let mut r = rng.fork();
            let (mut a, mut b) = (Lp::default(), Lp::default());
            m.add(0.0, seconds, move |t| {
                let hz = 200.0 + 2400.0 * bump(t, 0.0, seconds * 0.7);
                b.run(a.run(r.bi(), hz), hz) * bump(t, 0.0, seconds).sqrt() * (1.0 + 0.5 * bump(t, 0.4, 1.6))
            });
            m.add(0.0, seconds, |t| sin1(55.0 * t) * 0.12 * bump(t, 0.0, seconds));
            0.6
        }
        Kind::DoorSlam => {
            kick(&mut m, 0.0, 1.0);
            let mut r = rng.fork();
            let mut lp = Lp::default();
            m.add(0.0, 0.25, move |t| lp.run(r.bi(), 1500.0) * exp(-t * 30.0) * 1.6);
            // The latch rattles.
            tom(&mut m, 0.09, 240.0, 0.3);
            snare(&mut m, 0.09, 0.12, &mut rng);
            tom(&mut m, 0.16, 260.0, 0.12);
            0.88
        }
        Kind::PhoneRing => {
            let mut t = 0.1;
            while t < seconds - 0.5 {
                for k in 0..2 {
                    let at = t + k as f64 * 0.6;
                    m.add(at, 0.45, |x| {
                        let tone = sin1(440.0 * x) + sin1(480.0 * x);
                        // The bell's clapper: a 20 Hz flutter.
                        tone * (0.55 + 0.45 * sin1(20.0 * x)) * env(x, 0.01, 0.4, 0.04)
                    });
                }
                t += 2.6;
            }
            0.7
        }
        Kind::Thunder => {
            let mut lp = Lp::default();
            let mut r = rng.fork();
            m.add(0.0, 0.5, move |t| lp.run(r.bi(), 5000.0) * exp(-t * 9.0) * 0.6);
            let rolls: Vec<(f64, f64, f64)> = (0..7).map(|i| (0.1 + i as f64 * rng.range(0.4, 0.8), rng.range(0.8, 2.2), rng.range(0.4, 1.0) / (1.0 + i as f64 * 0.35))).collect();
            for (at, len, g) in rolls {
                rumble(&mut m, at, len, 220.0, g * 6.0, &mut rng, move |t| bump(t, 0.0, len) * (1.0 - exp(-t * 40.0)));
            }
            0.85
        }
    };
    m.finish(peak)
}

/// ln(x) for x in (0, 1], for exponential gaps between random events.
trait LnApprox {
    fn ln_approx(self) -> f64;
}

impl LnApprox for f64 {
    fn ln_approx(self) -> f64 {
        // Newton on e^y = x.
        let mut y = (self - 1.0) - 0.5 * (self - 1.0) * (self - 1.0);
        for _ in 0..30 {
            y -= 1.0 - self / exp(y);
        }
        y
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sine_and_exp_are_close_to_libm() {
        for i in 0..1000 {
            let x = i as f64 * 0.0137 - 5.0;
            assert!((sin1(x) - (x * std::f64::consts::TAU).sin()).abs() < 1e-7, "sin {x}");
            assert!((exp(x) - x.exp()).abs() < 1e-9 * x.exp().max(1.0), "exp {x}");
        }
        assert!((0.3f64.ln_approx() - 0.3f64.ln()).abs() < 1e-9);
    }
}
