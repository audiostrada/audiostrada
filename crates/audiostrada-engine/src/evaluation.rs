//! Evaluation mode of the official builds without a license, as Ardour does: after a grace
//! period, the output drops out for a moment every few minutes. Nothing else is limited.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use audiostrada_io::AudioBuffers;

/// Test builds made with `AUDIOSTRADA_EVALUATION_QUICK` drop out every minute from the first.
const QUICK: bool = option_env!("AUDIOSTRADA_EVALUATION_QUICK").is_some();
/// Session time before the first dropout.
pub const GRACE: f64 = if QUICK { 60.0 } else { 20.0 * 60.0 };
/// Time between dropouts.
pub const PERIOD: f64 = if QUICK { 60.0 } else { 10.0 * 60.0 };
/// Length of a dropout, fades included.
pub const LENGTH: f64 = 1.5;
const FADE: f64 = 0.005;
/// In an export, position of the first dropout.
const EXPORT_FIRST: f64 = 30.0;

static ACTIVE: AtomicBool = AtomicBool::new(false);
static STARTED: OnceLock<Instant> = OnceLock::new();

/// Switches evaluation mode; the session clock starts at the first call.
pub fn set_active(active: bool) {
    STARTED.get_or_init(Instant::now);
    ACTIVE.store(active, Ordering::Relaxed);
}

pub fn active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

/// Seconds since the session started.
pub fn session() -> f64 {
    STARTED.get_or_init(Instant::now).elapsed().as_secs_f64()
}

/// Gain at `t` seconds of the evaluation clock.
pub fn gain(t: f64) -> f32 {
    if t < GRACE {
        return 1.0;
    }
    let phase = (t - GRACE) % PERIOD;
    if phase >= LENGTH {
        1.0
    } else if phase < FADE {
        (1.0 - phase / FADE) as f32
    } else if phase > LENGTH - FADE {
        ((phase - (LENGTH - FADE)) / FADE) as f32
    } else {
        0.0
    }
}

/// Applies the dropouts to `frames` output frames starting at `t` seconds.
pub fn apply(buffers: &mut AudioBuffers<'_>, frames: usize, t: f64, rate: u32) {
    let step = 1.0 / f64::from(rate.max(1));
    let end = t + frames as f64 * step;
    // Blocks are far shorter than a dropout: one in the block shows at either end.
    if gain(t) == 1.0 && gain(end) == 1.0 {
        return;
    }
    for channel in 0..buffers.output_channels() {
        for (i, sample) in buffers.output(channel)[..frames].iter_mut().enumerate() {
            *sample *= gain(t + i as f64 * step);
        }
    }
}

/// Evaluation clock of an export at `seconds` into it: exports made after the grace period
/// drop out early, then every period.
pub fn export_clock(seconds: f64) -> Option<f64> {
    (active() && session() >= GRACE).then_some(GRACE + PERIOD - EXPORT_FIRST + seconds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_drops_during_the_grace_period() {
        assert_eq!(gain(0.0), 1.0);
        assert_eq!(gain(GRACE - 0.001), 1.0);
    }

    #[test]
    fn dropouts_come_back_every_period_with_fades() {
        for start in [GRACE, GRACE + PERIOD, GRACE + 5.0 * PERIOD] {
            assert_eq!(gain(start + LENGTH / 2.0), 0.0);
            let fading = gain(start + FADE / 2.0);
            assert!(fading > 0.0 && fading < 1.0);
            assert_eq!(gain(start + LENGTH + 0.01), 1.0);
            assert_eq!(gain(start - 1.0), 1.0);
        }
    }

    #[test]
    fn exports_drop_out_at_their_first_half_minute() {
        let clock = GRACE + PERIOD - EXPORT_FIRST;
        assert_eq!(gain(clock + 10.0), 1.0);
        assert_eq!(gain(clock + EXPORT_FIRST + 0.5), 0.0);
        assert_eq!(gain(clock + EXPORT_FIRST + PERIOD + 0.5), 0.0);
    }

    #[test]
    fn apply_silences_the_output_inside_a_dropout() {
        let mut storage = audiostrada_io::ChannelBuffers::new(0, 2, 480);
        for (t, expected) in [(GRACE + 0.5, 0.0), (GRACE + 10.0, 1.0), (GRACE - 1.0, 1.0)] {
            let mut buffers = storage.buffers(480);
            for channel in 0..2 {
                buffers.output(channel).fill(1.0);
            }
            apply(&mut buffers, 480, t, 48_000);
            for channel in 0..2 {
                assert!(buffers.output(channel).iter().all(|&s| s == expected));
            }
        }
    }
}
