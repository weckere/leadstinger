//! Minimal 16-bit PCM WAV writer, used for the demo library and tests.

use std::io::Write;
use std::path::Path;

/// Write a mono tone: the sum of `freqs` (Hz), `seconds` long, with short ramps at
/// both ends so it doesn't click.
pub fn write_tone(path: &Path, rate: u32, seconds: f32, freqs: &[f32], amp: f32) -> std::io::Result<()> {
    let n = (rate as f32 * seconds) as usize;
    let ramp = (rate as f32 * 0.01) as usize;
    let samples: Vec<f32> = (0..n)
        .map(|i| {
            let t = i as f32 / rate as f32;
            let s: f32 = freqs.iter().map(|f| (t * f * std::f32::consts::TAU).sin()).sum::<f32>() / freqs.len().max(1) as f32;
            let edge = (i.min(n - 1 - i) as f32 / ramp.max(1) as f32).min(1.0);
            s * amp * edge
        })
        .collect();
    write_mono(path, rate, &samples)
}

pub fn write_mono(path: &Path, rate: u32, samples: &[f32]) -> std::io::Result<()> {
    let data_len = (samples.len() * 2) as u32;
    let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);
    f.write_all(b"RIFF")?;
    f.write_all(&(36 + data_len).to_le_bytes())?;
    f.write_all(b"WAVEfmt ")?;
    f.write_all(&16u32.to_le_bytes())?; // fmt chunk size
    f.write_all(&1u16.to_le_bytes())?; // PCM
    f.write_all(&1u16.to_le_bytes())?; // mono
    f.write_all(&rate.to_le_bytes())?;
    f.write_all(&(rate * 2).to_le_bytes())?; // byte rate
    f.write_all(&2u16.to_le_bytes())?; // block align
    f.write_all(&16u16.to_le_bytes())?; // bits per sample
    f.write_all(b"data")?;
    f.write_all(&data_len.to_le_bytes())?;
    for s in samples {
        f.write_all(&((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).to_le_bytes())?;
    }
    f.flush()
}
