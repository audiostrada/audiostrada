//! Pure buffer size negotiation helpers shared by the platform backends.

/// Shared-mode engine period (WASAPI): the smallest multiple of `fundamental` covering
/// `requested`, kept within `min..=max`.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn engine_period(fundamental: u32, min: u32, max: u32, requested: u32) -> u32 {
    if fundamental == 0 {
        return min.max(requested.min(max));
    }
    requested
        .div_ceil(fundamental)
        .saturating_mul(fundamental)
        .clamp(min, max)
}

/// ASIO buffer size closest to `requested` among the sizes a driver accepts.
///
/// A granularity of -1 means powers of two, 0 means only the preferred size.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn asio_buffer_size(
    min: i32,
    max: i32,
    preferred: i32,
    granularity: i32,
    requested: i32,
) -> i32 {
    let distance = |size: i32| (i64::from(size) - i64::from(requested)).abs();
    match granularity {
        0 => preferred,
        -1 => {
            let mut best = preferred;
            let mut size = 1;
            while size <= max {
                if size >= min && distance(size) < distance(best) {
                    best = size;
                }
                size *= 2;
            }
            best
        }
        g if g > 0 => {
            let clamped = requested.clamp(min, max);
            let steps = (clamped - min + g / 2) / g;
            (min + steps * g).min(max)
        }
        _ => preferred,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_period_rounds_up_to_fundamental() {
        assert_eq!(engine_period(48, 144, 480, 128), 144);
        assert_eq!(engine_period(48, 144, 480, 200), 240);
        assert_eq!(engine_period(48, 144, 480, 4096), 480);
        assert_eq!(engine_period(0, 480, 480, 128), 480);
    }

    #[test]
    fn asio_sizes_follow_granularity() {
        assert_eq!(asio_buffer_size(64, 2048, 256, -1, 100), 128);
        assert_eq!(asio_buffer_size(64, 2048, 256, -1, 20), 64);
        assert_eq!(asio_buffer_size(48, 1024, 480, 48, 130), 144);
        assert_eq!(asio_buffer_size(512, 512, 512, 0, 64), 512);
        assert_eq!(asio_buffer_size(32, 1024, 256, 32, 5000), 1024);
    }
}
