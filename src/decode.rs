//! Decode any supported audio file fully into memory as interleaved stereo f32, so
//! triggering is instant and the mixer never touches the disk.

use anyhow::{anyhow, Context, Result};
use std::path::Path;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::{MetadataOptions, StandardVisualKey, Visual};
use symphonia::core::probe::{Hint, ProbeResult};

/// File-name extensions offered in the file picker.
pub const AUDIO_EXTENSIONS: &[&str] = &["wav", "wave", "aif", "aiff", "aifc", "mp3", "m4a", "aac", "mp4", "flac", "ogg", "oga", "caf"];

#[derive(Debug)]
pub struct DecodedAudio {
    pub sample_rate: u32,
    /// Interleaved stereo: `[l0, r0, l1, r1, ...]`.
    pub data: Vec<f32>,
    /// Cover art embedded in the file (ID3 APIC, MP4 covr, FLAC PICTURE), if any.
    pub art: Option<EmbeddedArt>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EmbeddedArt {
    pub media_type: String,
    pub data: Vec<u8>,
}

impl EmbeddedArt {
    /// File extension for saving the art, from its MIME type.
    pub fn extension(&self) -> &'static str {
        match self.media_type.to_ascii_lowercase().as_str() {
            "image/png" => "png",
            "image/gif" => "gif",
            "image/webp" => "webp",
            "image/bmp" => "bmp",
            _ => "jpg",
        }
    }
}

/// Prefer the front cover; otherwise take the first picture.
fn pick_visual(visuals: &[Visual]) -> Option<EmbeddedArt> {
    let v = visuals
        .iter()
        .find(|v| v.usage == Some(StandardVisualKey::FrontCover))
        .or_else(|| visuals.first())?;
    (!v.data.is_empty()).then(|| EmbeddedArt { media_type: v.media_type.clone(), data: v.data.to_vec() })
}

/// Cover art from tags read while probing (ID3v2 before an MP3) or from the
/// container's own metadata (FLAC, MP4).
fn embedded_art(probed: &mut ProbeResult) -> Option<EmbeddedArt> {
    let from_probe = probed.metadata.get().and_then(|m| m.current().and_then(|r| pick_visual(r.visuals())));
    from_probe.or_else(|| probed.format.metadata().current().and_then(|r| pick_visual(r.visuals())))
}

impl DecodedAudio {
    pub fn frames(&self) -> usize {
        self.data.len() / 2
    }

    pub fn duration(&self) -> f64 {
        self.frames() as f64 / self.sample_rate.max(1) as f64
    }

    /// Peak amplitude (0..=1) per bucket, for drawing waveforms.
    pub fn peaks(&self, buckets: usize) -> Vec<f32> {
        let frames = self.frames();
        if frames == 0 || buckets == 0 {
            return vec![0.0; buckets];
        }
        let mut out = vec![0.0f32; buckets];
        for (i, frame) in self.data.as_chunks::<2>().0.iter().enumerate() {
            let b = i * buckets / frames;
            let v = frame[0].abs().max(frame[1].abs());
            if v > out[b] {
                out[b] = v;
            }
        }
        out.iter_mut().for_each(|v| *v = v.min(1.0));
        out
    }
}

fn probe(path: &Path) -> Result<ProbeResult> {
    let file = std::fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| anyhow!("unsupported or unreadable audio ({e})"))
}

pub fn decode_file(path: &Path) -> Result<DecodedAudio> {
    let mut probed = probe(path)?;
    let art = embedded_art(&mut probed);
    let mut format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| anyhow!("no audio track"))?;
    let track_id = track.id;
    let mut sample_rate = track.codec_params.sample_rate.unwrap_or(44_100);
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| anyhow!("unsupported codec ({e})"))?;

    let mut data = Vec::new();
    let mut buf: Option<(SampleBuffer<f32>, usize)> = None; // buffer + its capacity in frames
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(SymError::ResetRequired) => break,
            Err(e) => return Err(anyhow!("reading audio: {e}")),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            // A corrupt packet: skip it rather than failing the whole file.
            Err(SymError::DecodeError(_)) => continue,
            Err(e) => return Err(anyhow!("decoding audio: {e}")),
        };
        let spec = *decoded.spec();
        sample_rate = spec.rate;
        let channels = spec.channels.count().max(1);
        let frames_needed = decoded.capacity();
        if buf.as_ref().is_none_or(|(_, cap)| *cap < frames_needed) {
            buf = Some((SampleBuffer::<f32>::new(frames_needed as u64, spec), frames_needed));
        }
        let (sb, _) = buf.as_mut().unwrap();
        sb.copy_interleaved_ref(decoded);
        for frame in sb.samples().chunks_exact(channels) {
            let l = frame[0];
            let r = if channels > 1 { frame[1] } else { l };
            data.push(l);
            data.push(r);
        }
    }
    if data.is_empty() {
        return Err(anyhow!("file contains no audio"));
    }
    Ok(DecodedAudio { sample_rate, data, art })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wav;

    #[test]
    fn decodes_mono_wav_to_stereo() {
        let dir = std::env::temp_dir().join(format!("sb-decode-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tone.wav");
        wav::write_tone(&path, 22_050, 0.5, &[440.0], 0.5).unwrap();
        let audio = decode_file(&path).unwrap();
        assert_eq!(audio.sample_rate, 22_050);
        assert_eq!(audio.frames(), 11_025);
        assert!((audio.duration() - 0.5).abs() < 1e-3);
        // Mono is duplicated to both channels.
        assert_eq!(audio.data[200], audio.data[201]);
        let peaks = audio.peaks(10);
        assert!(peaks.iter().all(|p| *p > 0.3 && *p <= 1.0));
        std::fs::remove_dir_all(dir).ok();
    }

    /// A FLAC stream holding only metadata: STREAMINFO plus a front-cover PICTURE.
    fn flac_with_picture(mime: &str, picture: &[u8]) -> Vec<u8> {
        let mut out = b"fLaC".to_vec();
        // STREAMINFO: block sizes, unknown frame sizes, 44.1 kHz, mono, 16-bit, 0 samples, no MD5.
        let mut info = Vec::new();
        info.extend_from_slice(&4096u16.to_be_bytes());
        info.extend_from_slice(&4096u16.to_be_bytes());
        info.extend_from_slice(&[0; 6]);
        let packed: u64 = (44_100u64 << 44) | (15u64 << 36);
        info.extend_from_slice(&packed.to_be_bytes());
        info.extend_from_slice(&[0; 16]);
        out.push(0x00);
        out.extend_from_slice(&(info.len() as u32).to_be_bytes()[1..]);
        out.extend_from_slice(&info);
        // PICTURE (type 6), marked as the last metadata block.
        let mut pic = Vec::new();
        pic.extend_from_slice(&3u32.to_be_bytes()); // front cover
        pic.extend_from_slice(&(mime.len() as u32).to_be_bytes());
        pic.extend_from_slice(mime.as_bytes());
        pic.extend_from_slice(&0u32.to_be_bytes()); // no description
        for v in [4u32, 4, 32, 0] {
            pic.extend_from_slice(&v.to_be_bytes()); // width, height, depth, colours
        }
        pic.extend_from_slice(&(picture.len() as u32).to_be_bytes());
        pic.extend_from_slice(picture);
        out.push(0x80 | 6);
        out.extend_from_slice(&(pic.len() as u32).to_be_bytes()[1..]);
        out.extend_from_slice(&pic);
        out.extend_from_slice(&silent_flac_frame());
        out
    }

    /// One FLAC frame of 192 silent mono samples: a CONSTANT subframe, with the
    /// header CRC-8 (poly 0x07) and frame CRC-16 (poly 0x8005) the format requires.
    fn silent_flac_frame() -> Vec<u8> {
        fn crc(bytes: &[u8], width: u32, poly: u32) -> u32 {
            let top = 1 << (width - 1);
            let mask = (1u64 << width) as u32 - 1;
            let mut c = 0u32;
            for b in bytes {
                c ^= (*b as u32) << (width - 8);
                for _ in 0..8 {
                    c = if c & top != 0 { (c << 1) ^ poly } else { c << 1 } & mask;
                }
            }
            c
        }
        // Sync + fixed blocking; 192-sample blocks, rate from STREAMINFO; mono, 16-bit from
        // STREAMINFO; frame number 0.
        let mut f = vec![0xFF, 0xF8, 0x10, 0x00, 0x00];
        f.push(crc(&f, 8, 0x07) as u8);
        f.extend_from_slice(&[0x00, 0x00, 0x00]); // CONSTANT subframe, value 0
        let crc16 = crc(&f, 16, 0x8005) as u16;
        f.extend_from_slice(&crc16.to_be_bytes());
        f
    }

    #[test]
    fn reads_embedded_front_cover() {
        let dir = std::env::temp_dir().join(format!("sb-decode-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let png = crate::picture::tests::tiny_png();
        let path = dir.join("cover.flac");
        std::fs::write(&path, flac_with_picture("image/png", &png)).unwrap();
        let art = embedded_art(&mut probe(&path).unwrap()).expect("art found");
        assert_eq!(art.media_type, "image/png");
        assert_eq!(art.data, png);
        assert_eq!(art.extension(), "png");
        // Plain WAV has no art.
        let wav = dir.join("tone.wav");
        wav::write_tone(&wav, 8000, 0.1, &[440.0], 0.5).unwrap();
        assert_eq!(decode_file(&wav).unwrap().art, None);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn rejects_non_audio() {
        let dir = std::env::temp_dir().join(format!("sb-decode-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("notes.wav");
        std::fs::write(&path, b"definitely not audio").unwrap();
        assert!(decode_file(&path).is_err());
        std::fs::remove_dir_all(dir).ok();
    }
}
