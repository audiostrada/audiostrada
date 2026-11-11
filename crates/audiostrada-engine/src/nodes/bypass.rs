//! A short crossfade between an effect's sound and its input when it is switched off or on,
//! rather than a click.

pub(crate) struct BypassFade {
    /// Share of the effect's sound: 1 playing, 0 bypassed.
    mix: f32,
    step: f32,
    dry: [Vec<f32>; 2],
    /// The dry sound was kept for the block being processed.
    kept: bool,
}

impl BypassFade {
    pub fn new() -> Self {
        Self {
            mix: 1.0,
            step: 1.0 / 480.0,
            dry: [Vec::new(), Vec::new()],
            kept: false,
        }
    }

    /// Fades of 10 ms.
    pub fn prepare(&mut self, sample_rate: u32, max_frames: usize) {
        self.step = 1.0 / (sample_rate as f32 * 0.01).max(1.0);
        self.dry = [vec![0.0; max_frames], vec![0.0; max_frames]];
    }

    /// Bypassed and faded out: the effect needs not run.
    pub fn idle(&self, bypass: bool) -> bool {
        bypass && self.mix == 0.0
    }

    /// Switched on again after a full bypass: the effect should start afresh.
    pub fn waking(&self, bypass: bool) -> bool {
        !bypass && self.mix == 0.0
    }

    /// Keeps the input aside when the block will be blended with it.
    pub fn keep(&mut self, left: &[f32], right: &[f32], bypass: bool) {
        let target = if bypass { 0.0 } else { 1.0 };
        self.kept = self.mix != target && left.len() <= self.dry[0].len();
        if self.kept {
            self.dry[0][..left.len()].copy_from_slice(left);
            self.dry[1][..right.len()].copy_from_slice(right);
        } else {
            self.mix = target;
        }
    }

    /// Blends the effect's sound with the input kept, moving toward the switch's side.
    pub fn blend(&mut self, left: &mut [f32], right: &mut [f32], bypass: bool) {
        if !std::mem::take(&mut self.kept) {
            return;
        }
        let step = if bypass { -self.step } else { self.step };
        let (start, frames) = (self.mix, left.len());
        for (out, dry) in [(left, &self.dry[0]), (right, &self.dry[1])] {
            let mut mix = start;
            for (s, d) in out.iter_mut().zip(dry.iter()) {
                mix = (mix + step).clamp(0.0, 1.0);
                *s = d + (*s - d) * mix;
            }
        }
        self.mix = (start + step * frames as f32).clamp(0.0, 1.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switching_an_effect_off_and_on_crossfades_with_its_input() {
        let mut fade = BypassFade::new();
        fade.prepare(48_000, 64);
        let mut heard = Vec::new();
        // The effect turns its input of 1 into -1; switched off, then on again.
        for bypass in [
            false, true, true, true, true, true, true, true, true, true, false,
        ] {
            if fade.idle(bypass) {
                heard.extend([1.0; 64]);
                continue;
            }
            let (mut left, mut right) = ([1.0f32; 64], [1.0f32; 64]);
            fade.keep(&left, &right, bypass);
            left.fill(-1.0);
            right.fill(-1.0);
            fade.blend(&mut left, &mut right, bypass);
            heard.extend(left);
        }
        assert_eq!(heard[63], -1.0);
        assert_eq!(heard[64 * 9], 1.0, "bypassed after 10 ms");
        let jump = heard
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0, f32::max);
        assert!(jump < 0.01, "largest step: {jump}");
    }
}
