//! Pitch shift that keeps the speed (D-043): a tile's audio is rendered once, transposed
//! by Signalsmith Stretch, when its pitch setting changes or the library loads. The mixer
//! then plays the rendered copy like any other clip, so the audio callback stays as cheap
//! as it is and triggering stays instant.

use crate::decode::{DecodedAudio, NativeChannels};
use signalsmith_stretch::Stretch;

/// The largest shift either way, in semitones.
pub const PITCH_MAX: f32 = 12.0;

/// A pitch setting as a cache key: hundredths of a semitone.
pub fn cents(semitones: f32) -> i32 {
    (semitones * 100.0).round() as i32
}

/// `audio` transposed by `semitones`, the same length. A multichannel file's native
/// channels are transposed too, so its tiles' channel mixes still apply.
pub fn shift(audio: &DecodedAudio, semitones: f32) -> DecodedAudio {
    let rate = audio.sample_rate.max(1);
    let semitones = semitones.clamp(-PITCH_MAX, PITCH_MAX);
    let render = |data: &[f32], channels: usize| {
        let mut st = Stretch::preset_default(channels as u32, rate);
        st.set_transpose_factor_semitones(semitones, None);
        let mut out = vec![0.0; data.len()];
        st.exact(data, &mut out);
        out
    };
    let native = audio.native.as_ref().map(|n| NativeChannels { data: render(&n.data, n.count()), labels: n.labels.clone(), downmix: n.downmix.clone() });
    DecodedAudio { sample_rate: audio.sample_rate, data: render(&audio.data, 2), art: None, native }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Frequency from upward zero crossings in the middle of the clip.
    fn frequency(a: &DecodedAudio) -> f32 {
        let left: Vec<f32> = a.data.chunks(2).map(|f| f[0]).collect();
        let (lo, hi) = (left.len() / 4, 3 * left.len() / 4);
        let ups = (lo..hi).filter(|&i| left[i - 1] < 0.0 && left[i] >= 0.0).count();
        ups as f32 / ((hi - lo) as f32 / a.sample_rate as f32)
    }

    #[test]
    fn an_octave_up_doubles_the_frequency_and_keeps_the_length() {
        let rate = 44_100;
        let data: Vec<f32> = (0..rate)
            .flat_map(|i| {
                let s = 0.5 * (i as f32 / rate as f32 * 440.0 * std::f32::consts::TAU).sin();
                [s, s]
            })
            .collect();
        let a = DecodedAudio { sample_rate: rate, data, art: None, native: None };
        assert!((frequency(&a) - 440.0).abs() < 5.0);
        let up = shift(&a, 12.0);
        assert_eq!(up.data.len(), a.data.len());
        assert!((frequency(&up) - 880.0).abs() < 15.0, "{}", frequency(&up));
        let down = shift(&a, -5.0);
        let want = 440.0 * 2f32.powf(-5.0 / 12.0);
        assert!((frequency(&down) - want).abs() < 8.0, "{} vs {want}", frequency(&down));
        assert_eq!(cents(-5.0), -500);
    }

    #[test]
    fn keeps_native_channels() {
        let rate = 8000;
        let native = NativeChannels { data: vec![0.1; rate * 3], labels: vec!["L".into(), "R".into(), "C".into()], downmix: vec![(1.0, 0.0), (0.0, 1.0), (0.5, 0.5)] };
        let a = DecodedAudio { sample_rate: rate as u32, data: vec![0.1; rate * 2], art: None, native: Some(native) };
        let up = shift(&a, 3.0);
        let n = up.native.expect("still multichannel");
        assert_eq!((n.data.len(), n.labels.len()), (rate * 3, 3));
    }
}
