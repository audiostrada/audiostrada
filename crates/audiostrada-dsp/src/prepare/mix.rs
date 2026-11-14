//! The song as a whole: what the EQ moves and filters of a track do at each frequency, and
//! the faders of a balanced mix.

use super::Instrument::*;
use super::{Character, Flavor, Instrument, Step};
use crate::biquad::{Coefficients, FilterKind};

const RATE: f64 = 48_000.0;

/// The response (dB) at `freq` of the EQ moves and the filters among `steps`.
pub fn response_db(steps: &[Step], freq: f32) -> f32 {
    let at = f64::from(freq);
    let mut db = 0.0;
    for step in steps {
        match *step {
            Step::Eq {
                band,
                freq,
                gain,
                q,
                bell,
                ..
            } => {
                let (kind, q) = match band {
                    0 if !bell => (FilterKind::LowShelf, 0.7),
                    3 if !bell => (FilterKind::HighShelf, 0.7),
                    _ => (FilterKind::Peak, q),
                };
                db += Coefficients::design(
                    kind,
                    f64::from(freq),
                    f64::from(q),
                    f64::from(gain),
                    RATE,
                )
                .magnitude_db(at, RATE);
            }
            Step::Filters { hpf, lpf, steep } => {
                let stages = if steep { 2.0 } else { 1.0 };
                for (kind, f) in [(FilterKind::HighPass, hpf), (FilterKind::LowPass, lpf)] {
                    if let Some(f) = f {
                        db += stages
                            * Coefficients::design(kind, f64::from(f), 0.707, 0.0, RATE)
                                .magnitude_db(at, RATE);
                    }
                }
            }
            _ => {}
        }
    }
    db as f32
}

/// Where an instrument sits in a balanced mix: the loudness (LU) of its tracks together
/// against the lead vocal, and whether its tracks share that place (else each takes it).
fn place(i: Instrument) -> (f32, bool) {
    match i {
        VocalMale | VocalFemale => (0.0, false),
        Kick => (2.5, false),
        Drums => (0.5, true),
        Bass => (0.5, true),
        BackingVocals => (-3.0, true),
        Whistling => (-7.0, true),
        ElectricGuitar | Cajon => (-3.5, true),
        AcousticGuitar => (-4.5, true),
        Snare => (-6.0, true),
        Synth | Brass => (-6.0, true),
        Keys => (-7.0, true),
        Overheads | Strings | HandDrums | Other => (-8.0, true),
        Toms | Claps => (-9.0, true),
        HiHat | Cowbell => (-13.0, true),
        Tambourine => (-14.0, true),
        Shaker => (-15.0, true),
    }
}

/// The highest fader of a balanced mix, dB.
pub const TOP_FADER: f32 = 6.0;

/// The most a track's loudness is taken to be under its peaks, dB: what plays all along is
/// 12 to 20 dB under them, a few hits far more.
pub const CREST: f32 = 22.0;

/// What a style moves forward or back (dB), at this intensity.
fn forward(flavor: Flavor, i: Instrument) -> f32 {
    let db = match (flavor.character, i) {
        (Character::Warm, VocalMale | VocalFemale | Bass) => 0.5,
        (Character::Warm, HiHat | Shaker | Tambourine | Cowbell) => -1.5,
        (Character::Crisp, VocalMale | VocalFemale) => 1.5,
        (Character::Crisp, BackingVocals) => 0.5,
        (Character::Crisp, ElectricGuitar | AcousticGuitar) => -0.5,
        (Character::Punchy, Kick | Snare) => 2.0,
        (Character::Punchy, Drums) => 1.5,
        (Character::Punchy, Bass | Toms | Cajon | HandDrums) => 1.0,
        (Character::Punchy, VocalMale | VocalFemale) => -0.5,
        (Character::Big, BackingVocals | Strings | Keys | Synth) => 1.0,
        _ => 0.0,
    };
    db * flavor.amount.clamp(0.0, 1.0)
}

/// The fader (dB) of each track for a balanced mix: `tracks` are their instrument, their
/// loudness (LUFS) and their peak (dBFS) before the fader, and the fader they have. The mix
/// keeps its loudness, as long as no fader goes above [`TOP_FADER`]. Sparse hits are quiet on
/// a loudness meter and would be pushed far above everything: a track is never taken for
/// quieter than [`CREST`] under its peaks.
pub fn balance(tracks: &[(Instrument, f32, f32, f32)], flavor: Flavor) -> Vec<f32> {
    let power = |db: f32| 10f64.powf(f64::from(db) / 10.0);
    let targets: Vec<f32> = tracks
        .iter()
        .map(|&(i, ..)| {
            let (level, shared) = place(i);
            let same = tracks.iter().filter(|t| t.0 == i).count().max(1) as f32;
            let split = if shared { 10.0 * same.log10() } else { 0.0 };
            level + forward(flavor, i) - split
        })
        .collect();
    let loud = |t: &(Instrument, f32, f32, f32)| t.1.max(t.2 - CREST);
    let (before, after): (f64, f64) = (
        tracks.iter().map(|t| power(loud(t) + t.3)).sum(),
        targets.iter().map(|t| power(*t)).sum(),
    );
    if before <= 0.0 || after <= 0.0 {
        return tracks.iter().map(|t| t.3).collect();
    }
    let keep = (10.0 * (before / after).log10()) as f32;
    let mut faders: Vec<f32> = tracks
        .iter()
        .zip(&targets)
        .map(|(t, target)| target - loud(t) + keep)
        .collect();
    // A few tracks far too loud would send all the others up to keep the loudness: past
    // +6 dB on a fader, the whole mix comes down instead and leaves room.
    let top = faders.iter().copied().fold(f32::MIN, f32::max);
    if top > TOP_FADER {
        faders.iter_mut().for_each(|f| *f -= top - TOP_FADER);
    }
    faders
        .into_iter()
        .map(|f| ((f * 10.0).round() / 10.0).max(-40.0))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_eq_move_is_reckoned_where_it_is() {
        let bell = [Step::Eq {
            band: 2,
            freq: 3_000.0,
            gain: 4.0,
            q: 1.0,
            bell: true,
            purpose: "Presence",
        }];
        assert!((response_db(&bell, 3_000.0) - 4.0).abs() < 0.1);
        assert!(response_db(&bell, 100.0).abs() < 0.2);
        let filter = [Step::Filters {
            hpf: Some(100.0),
            lpf: None,
            steep: false,
        }];
        assert!((response_db(&filter, 100.0) + 3.0).abs() < 0.3);
        assert!(response_db(&filter, 1_000.0).abs() < 0.1);
    }

    #[test]
    fn a_balanced_mix_keeps_its_loudness_and_puts_each_instrument_at_its_place() {
        // Instrument, loudness, peak, fader.
        let tracks = [
            (VocalFemale, -20.0, -4.0, 0.0),
            (Kick, -22.0, -8.0, 0.0),
            (ElectricGuitar, -18.0, -6.0, 0.0),
            (ElectricGuitar, -24.0, -10.0, 0.0),
        ];
        let none = Flavor::default();
        let faders = balance(&tracks, none);
        let heard: Vec<f32> = tracks.iter().zip(&faders).map(|(t, f)| t.1 + f).collect();
        // The kick 2.5 dB over the voice, the two guitars together 3.5 dB under it.
        assert!((heard[1] - heard[0] - 2.5).abs() < 0.11, "{heard:?}");
        assert!((heard[2] - heard[3]).abs() < 0.11);
        assert!((heard[0] - heard[2] - 3.5 - 3.01).abs() < 0.15, "{heard:?}");
        let power = |levels: &[f32]| levels.iter().map(|l| 10f32.powf(l / 10.0)).sum::<f32>();
        let before: Vec<f32> = tracks.iter().map(|t| t.1).collect();
        assert!((10.0 * (power(&heard) / power(&before)).log10()).abs() < 0.15);
        // A style moves its instruments forward.
        let crisp = Flavor {
            character: Character::Crisp,
            amount: 1.0,
        };
        let forward = balance(&tracks, crisp);
        assert!(forward[0] - forward[2] > faders[0] - faders[2] + 1.9);
        // A few sparse hits are not pushed far above everything.
        let sparse = [
            (VocalFemale, -20.0, -4.0, 0.0),
            (Snare, -45.0, -10.0, 0.0),
            (Bass, -20.0, -8.0, 0.0),
        ];
        let faders = balance(&sparse, none);
        assert!(faders[1] - faders[0] < 10.0, "{faders:?}");
        assert!(faders.iter().all(|f| *f <= TOP_FADER + 0.05));
    }
}
