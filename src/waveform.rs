//! Peak pyramids and zero-crossing snapping for drawing and editing the waveform at any zoom.

/// Lowest and highest sample over a stretch of audio, both channels together.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MinMax {
    pub min: f32,
    pub max: f32,
}

impl MinMax {
    fn of_frame(p: &[f32; 2]) -> Self {
        MinMax { min: p[0].min(p[1]), max: p[0].max(p[1]) }
    }

    fn merge(self, other: MinMax) -> Self {
        MinMax { min: self.min.min(other.min), max: self.max.max(other.max) }
    }
}

/// Min/max peaks at power-of-two resolutions, so a waveform of any length draws at
/// any zoom without scanning every sample.
pub struct PeakPyramid {
    frames: usize,
    // levels[0] holds one entry per BASE frames; each later level merges pairs of the
    // previous one. The samples live with the caller, which passes the same slice to
    // `range` and `columns`.
    levels: Vec<Vec<MinMax>>,
}

impl PeakPyramid {
    /// Frames per bucket at the finest level.
    pub const BASE: usize = 64;

    /// Build from interleaved stereo samples.
    pub fn new(data: &[f32]) -> Self {
        let frames = data.len() / 2;
        let pairs = data.as_chunks::<2>().0;
        let mut levels: Vec<Vec<MinMax>> = Vec::new();
        if frames > 0 {
            let first: Vec<MinMax> = pairs
                .chunks(Self::BASE)
                .map(|bucket| bucket.iter().map(MinMax::of_frame).reduce(MinMax::merge).unwrap())
                .collect();
            levels.push(first);
            // Each level merges pairs of the one below; a lone last bucket carries over.
            loop {
                let below = &levels[levels.len() - 1];
                if below.len() <= 1 {
                    break;
                }
                let merged: Vec<MinMax> = below
                    .chunks(2)
                    .map(|pair| pair.iter().copied().reduce(MinMax::merge).unwrap())
                    .collect();
                levels.push(merged);
            }
        }
        Self { frames, levels }
    }

    /// Number of frames it was built from.
    pub fn frames(&self) -> usize {
        self.frames
    }

    /// Exact min/max over frames `from..to` (clamped to the audio), or `None` when that
    /// range is empty. Costs about O(BASE + log n): raw samples cover the ragged head
    /// and tail, while the aligned middle uses the coarsest buckets that fit inside.
    pub fn range(&self, data: &[f32], from: usize, to: usize) -> Option<MinMax> {
        debug_assert_eq!(data.len() / 2, self.frames());
        let to = to.min(self.frames());
        if from >= to {
            return None;
        }
        let pairs = data.as_chunks::<2>().0;
        let mut acc: Option<MinMax> = None;
        let mut merge = |mm: MinMax| acc = Some(acc.map_or(mm, |a| a.merge(mm)));
        let mut pos = from;
        // Ragged head: scan raw frames up to the next level-0 boundary.
        while pos < to && !pos.is_multiple_of(Self::BASE) {
            merge(MinMax::of_frame(&pairs[pos]));
            pos += 1;
        }
        // Aligned middle: repeatedly take the coarsest bucket starting at `pos` that
        // still ends inside the range; anything left over is a short ragged tail.
        while pos < to {
            let mut k = 0;
            while k + 1 < self.levels.len() {
                let width = Self::BASE << (k + 1);
                if !pos.is_multiple_of(width) || pos + width > to {
                    break;
                }
                k += 1;
            }
            if k == 0 && pos + Self::BASE > to {
                merge(MinMax::of_frame(&pairs[pos]));
                pos += 1;
            } else {
                merge(self.levels[k][pos / (Self::BASE << k)]);
                pos += Self::BASE << k;
            }
        }
        acc
    }

    /// One entry per screen column, or `None` where a column shows no audio. Column `i`
    /// covers `floor(start + i * fpc)..floor(start + (i + 1) * fpc)`; zoomed past one
    /// frame per column it collapses to that single frame. Always exactly `count` long.
    pub fn columns(
        &self,
        data: &[f32],
        start: f64,
        frames_per_column: f64,
        count: usize,
    ) -> Vec<Option<MinMax>> {
        debug_assert_eq!(data.len() / 2, self.frames());
        if !start.is_finite() || !frames_per_column.is_finite() || frames_per_column <= 0.0 {
            return vec![None; count];
        }
        let frames = self.frames() as i64;
        let mut out = Vec::with_capacity(count);
        for i in 0..count {
            let lo = (start + i as f64 * frames_per_column).floor() as i64;
            let hi = (start + (i + 1) as f64 * frames_per_column).floor() as i64;
            // An empty span means we are zoomed past one frame per column.
            // Saturating: a huge `start` floors to i64::MAX.
            let (from, to) = if lo == hi { (lo, lo.saturating_add(1)) } else { (lo, hi) };
            let from = from.clamp(0, frames) as usize;
            let to = to.clamp(0, frames) as usize;
            out.push(if from >= to {
                None
            } else {
                self.range(data, from, to)
            });
        }
        out
    }
}

/// The zero crossing nearest `frame`, or `None` when there is none within
/// `max_distance`. Works on the channel sum `l + r` and searches outwards, so it costs
/// O(`max_distance`) instead of scanning the file. Ties go to the earlier frame.
pub fn nearest_zero_crossing(data: &[f32], frame: usize, max_distance: usize) -> Option<usize> {
    let frames = data.len() / 2;
    if frames == 0 {
        return None;
    }
    let frame = frame.min(frames - 1);
    let pairs = data.as_chunks::<2>().0;
    let sum = |i: usize| pairs[i][0] + pairs[i][1];
    let is_crossing = |i: usize| {
        let s = sum(i);
        if s == 0.0 {
            return true;
        }
        if i == 0 {
            return false;
        }
        let prev = sum(i - 1);
        (s < 0.0 && prev > 0.0) || (s > 0.0 && prev < 0.0)
    };
    for d in 0..=max_distance {
        let lo = frame.checked_sub(d);
        let hi = frame.checked_add(d).filter(|&i| i < frames);
        if lo.is_none() && hi.is_none() {
            break; // whole file searched; only reachable for d > 0
        }
        if let Some(i) = lo {
            if is_crossing(i) {
                return Some(i);
            }
        }
        if let Some(i) = hi.filter(|_| d > 0) {
            if is_crossing(i) {
                return Some(i);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic pseudo-random generator, so tests need no `rand` crate.
    struct Lcg(u64);

    impl Lcg {
        fn next_u64(&mut self) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            self.0
        }

        /// Sample in -1..1.
        fn next_sample(&mut self) -> f32 {
            (self.next_u64() >> 32) as f32 / u32::MAX as f32 * 2.0 - 1.0
        }
    }

    fn brute_range(data: &[f32], from: usize, to: usize) -> Option<MinMax> {
        let to = to.min(data.len() / 2);
        if from >= to {
            return None;
        }
        let pairs = data.as_chunks::<2>().0;
        let mut mm = MinMax { min: f32::INFINITY, max: f32::NEG_INFINITY };
        for p in &pairs[from..to] {
            mm.min = mm.min.min(p[0].min(p[1]));
            mm.max = mm.max.max(p[0].max(p[1]));
        }
        Some(mm)
    }

    #[test]
    fn range_matches_brute_force() {
        for &frames in &[0usize, 1, 63, 64, 65, 1000, 10_007] {
            let mut rng = Lcg(0x9E37_79B9_7F4A_7C15 ^ frames as u64);
            let data: Vec<f32> = (0..frames * 2).map(|_| rng.next_sample()).collect();
            let pyramid = PeakPyramid::new(&data);
            assert_eq!(pyramid.frames(), frames);
            if frames == 0 {
                assert!(pyramid.levels.is_empty(), "empty audio builds no levels");
            }
            // Whole range, empty ranges, and `to` past the end.
            assert_eq!(
                pyramid.range(&data, 0, frames),
                brute_range(&data, 0, frames)
            );
            assert_eq!(pyramid.range(&data, 0, 0), None);
            assert_eq!(pyramid.range(&data, frames, frames), None);
            assert_eq!(pyramid.range(&data, 3, 3), None);
            assert_eq!(
                pyramid.range(&data, 0, frames + 1000),
                brute_range(&data, 0, frames + 1000)
            );
            assert_eq!(pyramid.range(&data, frames + 1, frames + 10), None);
            for _ in 0..500 {
                let a = rng.next_u64() as usize % (frames + 128);
                let b = rng.next_u64() as usize % (frames + 128);
                let (from, to) = if a <= b { (a, b) } else { (b, a) };
                assert_eq!(
                    pyramid.range(&data, from, to),
                    brute_range(&data, from, to),
                    "frames={frames} from={from} to={to}"
                );
            }
        }
    }

    #[test]
    fn pyramid_level_bucket_counts() {
        let data = vec![0.1f32; 64 * 5 * 2];
        let pyramid = PeakPyramid::new(&data);
        let counts: Vec<usize> = pyramid.levels.iter().map(Vec::len).collect();
        assert_eq!(counts, vec![5, 3, 2, 1]);
    }

    #[test]
    fn columns_zoom_clamp_and_degenerate() {
        // 8 frames; frame i holds [2i, 2i + 1], so its min/max is (2i, 2i + 1).
        let data: Vec<f32> = (0..8)
            .flat_map(|i| [2.0 * i as f32, 2.0 * i as f32 + 1.0])
            .collect();
        let pyramid = PeakPyramid::new(&data);

        // Zoomed past one frame per column: each column shows its single frame raw.
        let zoomed = pyramid.columns(&data, 2.0, 0.25, 6);
        assert_eq!(
            zoomed,
            vec![
                Some(MinMax { min: 4.0, max: 5.0 }),
                Some(MinMax { min: 4.0, max: 5.0 }),
                Some(MinMax { min: 4.0, max: 5.0 }),
                Some(MinMax { min: 4.0, max: 5.0 }),
                Some(MinMax { min: 6.0, max: 7.0 }),
                Some(MinMax { min: 6.0, max: 7.0 }),
            ]
        );

        // One column over the whole file matches the global min/max.
        let whole = pyramid.columns(&data, 0.0, 8.0, 1);
        assert_eq!(whole, vec![Some(MinMax { min: 0.0, max: 15.0 })]);
        assert_eq!(whole[0], pyramid.range(&data, 0, 8));

        // Wholly outside the audio: None.
        assert_eq!(pyramid.columns(&data, -4.0, 1.0, 3), vec![None; 3]);
        assert_eq!(pyramid.columns(&data, 8.0, 1.0, 2), vec![None; 2]);

        // Columns take frames floor(a)..floor(b): frame 0 lands in [0.5, 1.5), not
        // [-0.5, 0.5), so each frame shows in exactly one column.
        let partial = pyramid.columns(&data, -0.5, 1.0, 2);
        assert_eq!(partial[0], None); // frames -1..0
        assert_eq!(partial[1], Some(MinMax { min: 0.0, max: 1.0 })); // frames 0..1
        // A column straddling the start clamps to the audio instead of vanishing.
        let straddle = pyramid.columns(&data, -2.0, 4.0, 1);
        assert_eq!(straddle[0], Some(MinMax { min: 0.0, max: 3.0 })); // frames -2..2
        // A huge but finite start must not overflow.
        assert_eq!(pyramid.columns(&data, 1e300, 0.5, 2), vec![None; 2]);
        let tail = pyramid.columns(&data, 7.5, 1.0, 2);
        assert_eq!(tail[0], Some(MinMax { min: 14.0, max: 15.0 })); // [7.5, 8.5)
        assert_eq!(tail[1], None);

        // Degenerate geometry gives all None.
        for &bad_fpc in &[0.0, -2.5, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(
                pyramid.columns(&data, 0.0, bad_fpc, 3),
                vec![None; 3],
                "fpc={bad_fpc}"
            );
        }
        assert_eq!(pyramid.columns(&data, f64::NAN, 1.0, 2), vec![None; 2]);

        // The result is always exactly `count` long.
        assert!(pyramid.columns(&data, 0.0, 1.0, 0).is_empty());
        assert_eq!(pyramid.columns(&data, 0.0, 1.0, 5).len(), 5);
    }

    #[test]
    fn zero_crossing_known_pattern() {
        // Channel sums per frame: crossings at 0 (exact zero), 2 and 4 (sign flips).
        let data: Vec<f32> = [0.0f32, 0.5, -0.5, -0.25, 0.25, 1.0]
            .iter()
            .flat_map(|&s| [s, 0.0])
            .collect();
        let zc = |frame, dist| nearest_zero_crossing(&data, frame, dist);

        assert_eq!(zc(3, 10), Some(2), "tie goes to the earlier frame");
        assert_eq!(zc(1, 10), Some(0));
        assert_eq!(zc(5, 10), Some(4));
        assert_eq!(zc(2, 0), Some(2));
        assert_eq!(zc(3, 0), None, "frame 3 is not a crossing");
        assert_eq!(zc(5, 0), None);
        assert_eq!(zc(100, 10), Some(4), "frame past the end clamps to the last");
        assert_eq!(zc(0, 0), Some(0), "exact 0.0 counts as a crossing");
    }

    #[test]
    fn zero_crossing_sign_change_only() {
        // No exact zeros; the sign flips at frames 1 and 2.
        let data: Vec<f32> = [1.0f32, -1.0, 1.0]
            .iter()
            .flat_map(|&s| [s, 0.0])
            .collect();
        assert_eq!(nearest_zero_crossing(&data, 0, 5), Some(1));
        assert_eq!(nearest_zero_crossing(&data, 2, 5), Some(2));
    }

    #[test]
    fn zero_crossing_none_found() {
        // All sums positive: no crossing anywhere in reach.
        let data: Vec<f32> = [0.5f32, 1.0, 2.0]
            .iter()
            .flat_map(|&s| [s, 0.0])
            .collect();
        assert_eq!(nearest_zero_crossing(&data, 0, 100), None);
        assert_eq!(nearest_zero_crossing(&data, 2, 100), None);
        // Empty audio.
        assert_eq!(nearest_zero_crossing(&[], 0, 10), None);
        assert_eq!(nearest_zero_crossing(&[], 99, 10), None);
    }
}
