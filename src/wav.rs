//! Minimal PCM WAV writers: 16-bit for the demo library, recordings and tests, 24-bit for
//! rendered exports (src/render.rs, D-079).

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

/// Interleaved stereo as 16-bit PCM (recordings).
pub fn write_stereo(path: &Path, rate: u32, samples: &[f32]) -> std::io::Result<()> {
    let data_len = (samples.len() * 2) as u32;
    let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);
    f.write_all(b"RIFF")?;
    f.write_all(&(36 + data_len).to_le_bytes())?;
    f.write_all(b"WAVEfmt ")?;
    f.write_all(&16u32.to_le_bytes())?;
    f.write_all(&1u16.to_le_bytes())?; // PCM
    f.write_all(&2u16.to_le_bytes())?; // stereo
    f.write_all(&rate.to_le_bytes())?;
    f.write_all(&(rate * 4).to_le_bytes())?; // byte rate
    f.write_all(&4u16.to_le_bytes())?; // block align
    f.write_all(&16u16.to_le_bytes())?;
    f.write_all(b"data")?;
    f.write_all(&data_len.to_le_bytes())?;
    for s in samples {
        f.write_all(&((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).to_le_bytes())?;
    }
    f.flush()
}

/// Interleaved 24-bit PCM, `channels` wide, from samples already scaled to 24-bit
/// integers (rendered exports).
pub fn write_pcm24(path: &Path, rate: u32, channels: u16, samples: &[i32]) -> std::io::Result<()> {
    let data_len = u32::try_from(samples.len() * 3).map_err(|_| std::io::Error::other("too long for a WAV file (4 GB)"))?;
    let block = u32::from(channels) * 3;
    let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);
    f.write_all(b"RIFF")?;
    f.write_all(&(36 + data_len + data_len % 2).to_le_bytes())?;
    f.write_all(b"WAVEfmt ")?;
    f.write_all(&16u32.to_le_bytes())?;
    f.write_all(&1u16.to_le_bytes())?; // PCM
    f.write_all(&channels.to_le_bytes())?;
    f.write_all(&rate.to_le_bytes())?;
    f.write_all(&(rate * block).to_le_bytes())?; // byte rate
    f.write_all(&(block as u16).to_le_bytes())?; // block align
    f.write_all(&24u16.to_le_bytes())?;
    f.write_all(b"data")?;
    f.write_all(&data_len.to_le_bytes())?;
    for s in samples {
        f.write_all(&s.to_le_bytes()[..3])?;
    }
    if data_len % 2 == 1 {
        f.write_all(&[0])?; // chunks are padded to an even length
    }
    f.flush()
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

/// Interleaved 16-bit PCM WAV with WAVE_FORMAT_EXTENSIBLE, one channel per bit set in
/// `channel_mask` (in ascending bit order), so decoders see the real layout. For tests
/// of multichannel files.
pub fn write_multichannel(path: &Path, rate: u32, channel_mask: u32, samples: &[f32]) -> std::io::Result<()> {
    let channels = channel_mask.count_ones() as u16;
    let data_len = (samples.len() * 2) as u32;
    let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);
    f.write_all(b"RIFF")?;
    f.write_all(&(60 + data_len).to_le_bytes())?;
    f.write_all(b"WAVEfmt ")?;
    f.write_all(&40u32.to_le_bytes())?;
    f.write_all(&0xFFFEu16.to_le_bytes())?; // WAVE_FORMAT_EXTENSIBLE
    f.write_all(&channels.to_le_bytes())?;
    f.write_all(&rate.to_le_bytes())?;
    f.write_all(&(rate * u32::from(channels) * 2).to_le_bytes())?; // byte rate
    f.write_all(&(channels * 2).to_le_bytes())?; // block align
    f.write_all(&16u16.to_le_bytes())?; // bits per sample
    f.write_all(&22u16.to_le_bytes())?; // cbSize
    f.write_all(&16u16.to_le_bytes())?; // valid bits per sample
    f.write_all(&channel_mask.to_le_bytes())?;
    // Sub-format GUID for PCM: 00000001-0000-0010-8000-00aa00389b71.
    f.write_all(&[0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xaa, 0x00, 0x38, 0x9b, 0x71])?;
    f.write_all(b"data")?;
    f.write_all(&data_len.to_le_bytes())?;
    for s in samples {
        f.write_all(&((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).to_le_bytes())?;
    }
    f.flush()
}
