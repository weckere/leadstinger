//! Decode any supported audio file fully into memory as interleaved stereo f32, so
//! triggering is instant and the mixer never touches the disk. Multi-channel audio
//! is downmixed to stereo per ITU-R BS.775, by channel position, and its native channels
//! are kept beside the downmix for a tile's own channel mix.

use anyhow::{anyhow, Context, Result};
use std::path::Path;
use symphonia::core::audio::{Channels, SampleBuffer};
use symphonia::core::codecs::{Decoder, DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::{FormatOptions, FormatReader, Track};
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
    /// A file with more than two channels keeps them here too (about 3× the memory of
    /// the downmix for 5.1), for a tile's channel mix.
    pub native: Option<NativeChannels>,
}

/// A multichannel file's own channels, as decoded.
#[derive(Debug, Clone, PartialEq)]
pub struct NativeChannels {
    /// Interleaved, `labels.len()` wide, as many frames as the downmix.
    pub data: Vec<f32>,
    /// Each channel's place: "L", "R", "C", "LFE", "Ls", "Rs" and so on, or "Ch 7".
    pub labels: Vec<String>,
    /// Each channel's (left, right) gain in the BS.775 downmix.
    pub downmix: Vec<(f32, f32)>,
}

impl NativeChannels {
    pub fn count(&self) -> usize {
        self.labels.len()
    }
}

/// A short name for a channel position.
fn position_label(pos: Channels) -> &'static str {
    match pos {
        Channels::FRONT_LEFT => "L",
        Channels::FRONT_RIGHT => "R",
        Channels::FRONT_CENTRE => "C",
        Channels::LFE1 => "LFE",
        Channels::LFE2 => "LFE2",
        Channels::REAR_LEFT => "Lb",
        Channels::REAR_RIGHT => "Rb",
        Channels::REAR_CENTRE => "Cb",
        Channels::SIDE_LEFT => "Ls",
        Channels::SIDE_RIGHT => "Rs",
        Channels::FRONT_LEFT_CENTRE => "Lc",
        Channels::FRONT_RIGHT_CENTRE => "Rc",
        Channels::FRONT_LEFT_WIDE => "Lw",
        Channels::FRONT_RIGHT_WIDE => "Rw",
        Channels::FRONT_LEFT_HIGH | Channels::TOP_FRONT_LEFT => "Ltf",
        Channels::FRONT_RIGHT_HIGH | Channels::TOP_FRONT_RIGHT => "Rtf",
        Channels::TOP_REAR_LEFT => "Ltr",
        Channels::TOP_REAR_RIGHT => "Rtr",
        Channels::TOP_CENTRE => "Tc",
        _ => "",
    }
}

fn channel_labels(channels: Channels, count: usize) -> Vec<String> {
    let mut labels: Vec<String> = channels.iter().map(|c| position_label(c).to_owned()).collect();
    labels.resize(count, String::new());
    for (i, l) in labels.iter_mut().enumerate() {
        if l.is_empty() {
            *l = format!("Ch {}", i + 1);
        }
    }
    labels
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

/// Stereo gain pair for one channel position in the BS.775 downmix.
fn position_gain(pos: Channels) -> (f32, f32) {
    const K: f32 = std::f32::consts::FRAC_1_SQRT_2; // -3 dB
    const KK: f32 = K * K; // centred, then the surround gain
    match pos {
        Channels::FRONT_LEFT => (1.0, 0.0),
        Channels::FRONT_RIGHT => (0.0, 1.0),
        Channels::FRONT_CENTRE => (K, K),
        // BS.775 discards the LFE in a stereo downmix.
        Channels::LFE1 | Channels::LFE2 => (0.0, 0.0),
        Channels::REAR_LEFT | Channels::SIDE_LEFT => (K, 0.0),
        Channels::REAR_RIGHT | Channels::SIDE_RIGHT => (0.0, K),
        // A single mono surround splits evenly across both sides.
        Channels::REAR_CENTRE => (KK, KK),
        // Anything else folds in by side.
        Channels::FRONT_LEFT_CENTRE
        | Channels::FRONT_LEFT_WIDE
        | Channels::FRONT_LEFT_HIGH
        | Channels::TOP_FRONT_LEFT
        | Channels::TOP_REAR_LEFT
        | Channels::REAR_LEFT_CENTRE => (K, 0.0),
        Channels::FRONT_RIGHT_CENTRE
        | Channels::FRONT_RIGHT_WIDE
        | Channels::FRONT_RIGHT_HIGH
        | Channels::TOP_FRONT_RIGHT
        | Channels::TOP_REAR_RIGHT
        | Channels::REAR_RIGHT_CENTRE => (0.0, K),
        Channels::TOP_CENTRE
        | Channels::TOP_FRONT_CENTRE
        | Channels::TOP_REAR_CENTRE
        | Channels::FRONT_CENTRE_HIGH => (KK, KK),
        _ => (0.0, 0.0),
    }
}

/// One `(left, right)` gain pair per interleaved slot for the BS.775 downmix.
/// Levels are not normalised; the mixer limits the output instead.
fn downmix_gains(channels: Channels, count: usize) -> Vec<(f32, f32)> {
    match count {
        // Mono and stereo keep their old behaviour whatever the positions are.
        0 => Vec::new(),
        1 => vec![(1.0, 1.0)],
        2 => vec![(1.0, 0.0), (0.0, 1.0)],
        // No layout at all: fall back to the old first-two-channels behaviour.
        _ if channels.is_empty() => {
            let mut gains = vec![(0.0f32, 0.0f32); count];
            gains[0] = (1.0, 0.0);
            gains[1] = (0.0, 1.0);
            gains
        }
        _ => {
            let mut gains: Vec<(f32, f32)> = channels.iter().map(position_gain).collect();
            gains.resize(count, (0.0, 0.0));
            gains
        }
    }
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
    probe_source(Box::new(file), path.extension().and_then(|e| e.to_str()))
}

fn probe_source(source: Box<dyn symphonia::core::io::MediaSource>, ext: Option<&str>) -> Result<ProbeResult> {
    let mss = MediaSourceStream::new(source, Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = ext {
        hint.with_extension(ext);
    }
    symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| anyhow!("unsupported or unreadable audio ({e})"))
}

/// The first audio track and a decoder for it.
fn open_track(format: &dyn FormatReader) -> Result<(&Track, Box<dyn Decoder>)> {
    let track = format.tracks().iter().find(|t| t.codec_params.codec != CODEC_TYPE_NULL).ok_or_else(|| anyhow!("no audio track"))?;
    let decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| anyhow!("unsupported codec ({e})"))?;
    Ok((track, decoder))
}

/// Whether [`decode_file`] can open `path`: its container and codec are known. Reads
/// only the headers, so it's quick enough to ask at import.
pub fn can_read(path: &Path) -> bool {
    probe(path).is_ok_and(|p| open_track(p.format.as_ref()).is_ok())
}

pub fn decode_file(path: &Path) -> Result<DecodedAudio> {
    decode(probe(path)?)
}

/// Decode a whole file held in memory (a QuipClipper preview, which is never saved);
/// `ext` is its file name's extension, a hint for the container.
pub fn decode_bytes(bytes: Vec<u8>, ext: Option<&str>) -> Result<DecodedAudio> {
    decode(probe_source(Box::new(std::io::Cursor::new(bytes)), ext)?)
}

fn decode(mut probed: ProbeResult) -> Result<DecodedAudio> {
    let art = embedded_art(&mut probed);
    let mut format = probed.format;
    let (track, mut decoder) = open_track(format.as_ref())?;
    let track_id = track.id;
    let mut sample_rate = track.codec_params.sample_rate.unwrap_or(44_100);

    let mut data = Vec::new();
    let mut buf: Option<(SampleBuffer<f32>, usize)> = None; // buffer + its capacity in frames
    // Downmix gains for the current packet spec; rebuilt when the spec changes.
    let mut gains: Vec<(f32, f32)> = Vec::new();
    let mut gain_spec: Option<Channels> = None;
    // The native channels, while the layout stays the same (a file that changes its
    // channel count part-way keeps only the downmix).
    let mut native: Option<NativeChannels> = None;
    let mut native_ok = true;
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
        let channel_count = spec.channels.count().max(1);
        let frames_needed = decoded.capacity();
        // A new channel layout needs new gains and a buffer of the new width.
        let new_spec = gain_spec != Some(spec.channels);
        if new_spec {
            gains = downmix_gains(spec.channels, channel_count);
            if gain_spec.is_some() {
                native_ok = false;
            } else if channel_count > 2 {
                native = Some(NativeChannels { data: Vec::new(), labels: channel_labels(spec.channels, channel_count), downmix: gains.clone() });
            }
            gain_spec = Some(spec.channels);
        }
        if new_spec || buf.as_ref().is_none_or(|(_, cap)| *cap < frames_needed) {
            buf = Some((SampleBuffer::<f32>::new(frames_needed as u64, spec), frames_needed));
        }
        let (sb, _) = buf.as_mut().unwrap();
        sb.copy_interleaved_ref(decoded);
        for frame in sb.samples().chunks_exact(channel_count) {
            let mut l = 0.0f32;
            let mut r = 0.0f32;
            for (sample, &(gl, gr)) in frame.iter().zip(gains.iter()) {
                l += sample * gl;
                r += sample * gr;
            }
            data.push(l);
            data.push(r);
        }
        if let Some(n) = native.as_mut().filter(|_| native_ok) {
            n.data.extend_from_slice(sb.samples());
        }
    }
    if data.is_empty() {
        return Err(anyhow!("file contains no audio"));
    }
    let native = native.filter(|_| native_ok);
    Ok(DecodedAudio { sample_rate, data, art, native })
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

    fn assert_gains_close(actual: &[(f32, f32)], expected: &[(f32, f32)]) {
        assert_eq!(actual.len(), expected.len(), "gain count: {actual:?}");
        for (i, (a, e)) in actual.iter().zip(expected).enumerate() {
            assert!((a.0 - e.0).abs() < 1e-6, "slot {i} left: {} != {}", a.0, e.0);
            assert!((a.1 - e.1).abs() < 1e-6, "slot {i} right: {} != {}", a.1, e.1);
        }
    }

    #[test]
    fn downmix_gains_mono() {
        // One channel always goes to both outputs at full level, whatever it is.
        assert_gains_close(&downmix_gains(Channels::FRONT_CENTRE, 1), &[(1.0, 1.0)]);
        assert_gains_close(&downmix_gains(Channels::empty(), 1), &[(1.0, 1.0)]);
    }

    #[test]
    fn downmix_gains_stereo() {
        // Two channels pass through unchanged, whatever their positions.
        assert_gains_close(
            &downmix_gains(Channels::FRONT_LEFT | Channels::FRONT_RIGHT, 2),
            &[(1.0, 0.0), (0.0, 1.0)],
        );
        assert_gains_close(
            &downmix_gains(Channels::REAR_LEFT | Channels::REAR_RIGHT, 2),
            &[(1.0, 0.0), (0.0, 1.0)],
        );
    }

    #[test]
    fn downmix_gains_5_1() {
        let k = std::f32::consts::FRAC_1_SQRT_2;
        // Slots follow ascending bit order: FL FR FC LFE BL BR.
        let layout = Channels::FRONT_LEFT
            | Channels::FRONT_RIGHT
            | Channels::FRONT_CENTRE
            | Channels::LFE1
            | Channels::REAR_LEFT
            | Channels::REAR_RIGHT;
        assert_gains_close(
            &downmix_gains(layout, 6),
            &[(1.0, 0.0), (0.0, 1.0), (k, k), (0.0, 0.0), (k, 0.0), (0.0, k)],
        );
    }

    #[test]
    fn downmix_gains_5_1_with_sides() {
        let k = std::f32::consts::FRAC_1_SQRT_2;
        let layout = Channels::FRONT_LEFT
            | Channels::FRONT_RIGHT
            | Channels::FRONT_CENTRE
            | Channels::LFE1
            | Channels::SIDE_LEFT
            | Channels::SIDE_RIGHT;
        assert_gains_close(
            &downmix_gains(layout, 6),
            &[(1.0, 0.0), (0.0, 1.0), (k, k), (0.0, 0.0), (k, 0.0), (0.0, k)],
        );
    }

    #[test]
    fn downmix_gains_7_1() {
        let k = std::f32::consts::FRAC_1_SQRT_2;
        let layout = Channels::FRONT_LEFT
            | Channels::FRONT_RIGHT
            | Channels::FRONT_CENTRE
            | Channels::LFE1
            | Channels::REAR_LEFT
            | Channels::REAR_RIGHT
            | Channels::SIDE_LEFT
            | Channels::SIDE_RIGHT;
        assert_gains_close(
            &downmix_gains(layout, 8),
            &[
                (1.0, 0.0),
                (0.0, 1.0),
                (k, k),
                (0.0, 0.0),
                (k, 0.0),
                (0.0, k),
                (k, 0.0),
                (0.0, k),
            ],
        );
    }

    #[test]
    fn downmix_gains_empty_layout_falls_back_to_first_two() {
        assert_gains_close(
            &downmix_gains(Channels::empty(), 4),
            &[(1.0, 0.0), (0.0, 1.0), (0.0, 0.0), (0.0, 0.0)],
        );
    }

    #[test]
    fn downmix_gains_other_positions_fold_in_by_side() {
        let k = std::f32::consts::FRAC_1_SQRT_2;
        let kk = k * k;
        // Ascending bit order: REAR_CENTRE, TOP_CENTRE, FRONT_LEFT_WIDE, FRONT_RIGHT_HIGH.
        let layout = Channels::REAR_CENTRE
            | Channels::TOP_CENTRE
            | Channels::FRONT_LEFT_WIDE
            | Channels::FRONT_RIGHT_HIGH;
        assert_gains_close(&downmix_gains(layout, 4), &[(kk, kk), (kk, kk), (k, 0.0), (0.0, k)]);
    }

    #[test]
    fn downmixes_5_1_wav_to_stereo() {
        let dir = std::env::temp_dir().join(format!("sb-decode-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("surround.wav");
        // FL FR FC LFE BL BR, channel mask 0x3F, one constant per channel.
        let frame = [0.1f32, 0.2, 0.3, 0.9, 0.05, 0.15];
        wav::write_multichannel(&path, 44_100, 0x3F, &[frame; 8].concat()).unwrap();
        let audio = decode_file(&path).unwrap();
        assert_eq!(audio.sample_rate, 44_100);
        assert_eq!(audio.frames(), 8);
        let k = std::f32::consts::FRAC_1_SQRT_2;
        // LFE is dropped; the rest follows the BS.775 coefficients.
        let expected_l = 0.1 + (0.3 + 0.05) * k;
        let expected_r = 0.2 + (0.3 + 0.15) * k;
        for (i, pair) in audio.data.as_chunks::<2>().0.iter().enumerate() {
            assert!((pair[0] - expected_l).abs() < 1e-3, "frame {i} left: {} != {expected_l}", pair[0]);
            assert!((pair[1] - expected_r).abs() < 1e-3, "frame {i} right: {} != {expected_r}", pair[1]);
        }
        // The native channels are kept too, with their places and downmix gains.
        let native = audio.native.as_ref().expect("six channels kept");
        assert_eq!(native.labels, ["L", "R", "C", "LFE", "Lb", "Rb"]);
        assert_eq!(native.data.len(), 8 * 6);
        for (got, want) in native.data[6..12].iter().zip(frame) {
            assert!((got - want).abs() < 1e-3, "{got} != {want}");
        }
        assert_eq!(native.downmix[3], (0.0, 0.0), "LFE left out of the downmix");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn stereo_keeps_no_native_copy() {
        let dir = std::env::temp_dir().join(format!("sb-decode-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("stereo.wav");
        wav::write_stereo(&path, 8000, &[0.1, 0.2, 0.3, 0.4]).unwrap();
        assert!(decode_file(&path).unwrap().native.is_none());
        std::fs::remove_dir_all(dir).ok();
    }
}
