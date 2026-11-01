//! Measuring a clip's level for `tile.normalize`: its sample peak in dBFS, or its EBU R128
//! integrated loudness in LUFS. Both look only at the region the tile plays (its trim).

use crate::decode::DecodedAudio;
use crate::model::GAIN_DB_MAX;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// The loudest sample reaches `level` dBFS.
    Peak,
    /// The integrated loudness (EBU R128 / ITU-R BS.1770) is `level` LUFS.
    Loudness,
}

impl Target {
    pub fn parse(s: &str) -> Option<Target> {
        match s {
            "peak" => Some(Target::Peak),
            "loudness" | "lufs" => Some(Target::Loudness),
            _ => None,
        }
    }

    /// The level aimed at when none is given: −1 dBFS, or −16 LUFS (a common level for
    /// podcasts and streams).
    pub fn default_level(self) -> f32 {
        match self {
            Target::Peak => -1.0,
            Target::Loudness => -16.0,
        }
    }

    pub fn unit(self) -> &'static str {
        match self {
            Target::Peak => "dBFS",
            Target::Loudness => "LUFS",
        }
    }
}

/// The level of `audio` between `start` and `end` seconds, or `None` when that part is
/// silent (or too short for a loudness reading, under 0.4 s).
pub fn measure(audio: &DecodedAudio, start: f64, end: f64, target: Target) -> Option<f32> {
    let rate = audio.sample_rate.max(1) as f64;
    let frames = audio.data.len() / 2;
    let a = ((start * rate) as usize).min(frames);
    let b = ((end * rate).ceil() as usize).clamp(a, frames);
    let region = &audio.data[2 * a..2 * b];
    let level = match target {
        Target::Peak => {
            let peak = region.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            (peak > 0.0).then(|| 20.0 * peak.log10())?
        }
        Target::Loudness => {
            let mut meter = ebur128::EbuR128::new(2, audio.sample_rate.max(1), ebur128::Mode::I).ok()?;
            meter.add_frames_f32(region).ok()?;
            meter.loudness_global().ok()? as f32
        }
    };
    level.is_finite().then_some(level)
}

/// The dB gain that brings a clip measured at `measured` to `level`, within the tile's
/// gain range.
pub fn gain_for(measured: f32, level: f32) -> f32 {
    (level - measured).clamp(-GAIN_DB_MAX, GAIN_DB_MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(rate: u32, seconds: f32, amp: f32) -> DecodedAudio {
        let n = (rate as f32 * seconds) as usize;
        let data = (0..n)
            .flat_map(|i| {
                let s = amp * (i as f32 / rate as f32 * 1000.0 * std::f32::consts::TAU).sin();
                [s, s]
            })
            .collect();
        DecodedAudio { sample_rate: rate, data, art: None, native: None }
    }

    #[test]
    fn peak_in_dbfs() {
        let a = sine(48_000, 1.0, 0.5);
        let db = measure(&a, 0.0, 1.0, Target::Peak).unwrap();
        assert!((db + 6.02).abs() < 0.05, "{db}");
        assert!((gain_for(db, -1.0) - 5.02).abs() < 0.05);
    }

    #[test]
    fn loudness_of_a_full_scale_sine() {
        // A full-scale 1 kHz sine reads −3 LUFS in one channel, so 0 LUFS in both
        // (BS.1770's −0.691 offset cancels the K-weighting's gain at 1 kHz).
        let a = sine(48_000, 3.0, 1.0);
        let lufs = measure(&a, 0.0, 3.0, Target::Loudness).unwrap();
        assert!(lufs.abs() < 0.2, "{lufs}");
        // Half the amplitude is 6 dB quieter.
        let b = sine(48_000, 3.0, 0.5);
        let quieter = measure(&b, 0.0, 3.0, Target::Loudness).unwrap();
        assert!((lufs - quieter - 6.02).abs() < 0.1, "{lufs} {quieter}");
    }

    #[test]
    fn silence_and_trim() {
        let silent = DecodedAudio { sample_rate: 8000, data: vec![0.0; 16_000], art: None, native: None };
        assert_eq!(measure(&silent, 0.0, 1.0, Target::Peak), None);
        assert_eq!(measure(&silent, 0.0, 1.0, Target::Loudness), None);
        // Only the trimmed region counts: loud first half, quiet second half.
        let mut a = sine(8000, 2.0, 1.0);
        a.data[16_000..].iter_mut().for_each(|s| *s *= 0.1);
        let late = measure(&a, 1.0, 2.0, Target::Peak).unwrap();
        assert!((late + 20.0).abs() < 0.1, "{late}");
        assert_eq!(gain_for(-80.0, -1.0), GAIN_DB_MAX);
    }
}
