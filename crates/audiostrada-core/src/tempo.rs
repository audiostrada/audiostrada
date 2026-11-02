//! Tempo and time signature map: conversions between samples, seconds, beats and bars.
//!
//! Beats are quarter notes. Tempo is constant between points (no ramps yet).

use serde::{Deserialize, Serialize};

const TICKS_PER_BEAT: u32 = 960;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TempoPoint {
    /// Position in beats where this tempo starts.
    pub beat: f64,
    pub bpm: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeSignature {
    /// First bar (0-based) using this signature.
    pub bar: u32,
    pub numerator: u32,
    pub denominator: u32,
}

impl TimeSignature {
    /// Length of one bar in quarter-note beats.
    pub fn beats_per_bar(&self) -> f64 {
        f64::from(self.numerator) * 4.0 / f64::from(self.denominator)
    }
}

/// Musical position: 1-based bar and beat (in signature units), plus ticks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BarBeatTick {
    pub bar: u32,
    pub beat: u32,
    pub tick: u32,
}

/// Serialized form: only the points; derived tables are rebuilt (and validated) on load.
#[derive(Clone, Serialize, Deserialize)]
struct TempoMapData {
    tempos: Vec<TempoPoint>,
    signatures: Vec<TimeSignature>,
}

impl From<TempoMapData> for TempoMap {
    fn from(data: TempoMapData) -> Self {
        Self::new(data.tempos, data.signatures)
    }
}

impl From<TempoMap> for TempoMapData {
    fn from(map: TempoMap) -> Self {
        Self {
            tempos: map.tempos,
            signatures: map.signatures,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(from = "TempoMapData", into = "TempoMapData")]
pub struct TempoMap {
    tempos: Vec<TempoPoint>,
    /// Seconds at the start of each tempo point.
    seconds: Vec<f64>,
    signatures: Vec<TimeSignature>,
    /// Beat at the start of each signature.
    signature_beats: Vec<f64>,
}

impl Default for TempoMap {
    fn default() -> Self {
        Self::constant(120.0, 4, 4)
    }
}

impl TempoMap {
    pub fn constant(bpm: f64, numerator: u32, denominator: u32) -> Self {
        Self::new(
            vec![TempoPoint { beat: 0.0, bpm }],
            vec![TimeSignature {
                bar: 0,
                numerator,
                denominator,
            }],
        )
    }

    /// Builds a map; points are sorted, and a first point at beat/bar 0 is added if missing.
    pub fn new(mut tempos: Vec<TempoPoint>, mut signatures: Vec<TimeSignature>) -> Self {
        tempos.retain(|t| (1.0..=1000.0).contains(&t.bpm) && t.beat.is_finite() && t.beat >= 0.0);
        tempos.sort_by(|a, b| a.beat.total_cmp(&b.beat));
        tempos.dedup_by(|a, b| a.beat == b.beat);
        if tempos.first().is_none_or(|t| t.beat > 0.0) {
            let bpm = tempos.first().map_or(120.0, |t| t.bpm);
            tempos.insert(0, TempoPoint { beat: 0.0, bpm });
        }
        let mut seconds = Vec::with_capacity(tempos.len());
        let mut acc = 0.0;
        for (i, t) in tempos.iter().enumerate() {
            if i > 0 {
                let prev = &tempos[i - 1];
                acc += (t.beat - prev.beat) * 60.0 / prev.bpm;
            }
            seconds.push(acc);
        }

        signatures.retain(|s| (1..=64).contains(&s.numerator) && (1..=64).contains(&s.denominator));
        signatures.sort_by_key(|s| s.bar);
        signatures.dedup_by_key(|s| s.bar);
        if signatures.first().is_none_or(|s| s.bar > 0) {
            let (numerator, denominator) = signatures
                .first()
                .map_or((4, 4), |s| (s.numerator, s.denominator));
            signatures.insert(
                0,
                TimeSignature {
                    bar: 0,
                    numerator,
                    denominator,
                },
            );
        }
        let mut signature_beats = Vec::with_capacity(signatures.len());
        let mut beat = 0.0;
        for (i, s) in signatures.iter().enumerate() {
            if i > 0 {
                let prev = &signatures[i - 1];
                beat += f64::from(s.bar - prev.bar) * prev.beats_per_bar();
            }
            signature_beats.push(beat);
        }
        Self {
            tempos,
            seconds,
            signatures,
            signature_beats,
        }
    }

    pub fn tempos(&self) -> &[TempoPoint] {
        &self.tempos
    }

    pub fn signatures(&self) -> &[TimeSignature] {
        &self.signatures
    }

    fn tempo_index_at_beat(&self, beat: f64) -> usize {
        self.tempos
            .partition_point(|t| t.beat <= beat)
            .saturating_sub(1)
    }

    fn tempo_index_at_seconds(&self, seconds: f64) -> usize {
        self.seconds
            .partition_point(|&s| s <= seconds)
            .saturating_sub(1)
    }

    pub fn bpm_at_beat(&self, beat: f64) -> f64 {
        self.tempos[self.tempo_index_at_beat(beat)].bpm
    }

    pub fn beat_to_seconds(&self, beat: f64) -> f64 {
        let i = self.tempo_index_at_beat(beat);
        let t = &self.tempos[i];
        self.seconds[i] + (beat - t.beat) * 60.0 / t.bpm
    }

    pub fn seconds_to_beat(&self, seconds: f64) -> f64 {
        let i = self.tempo_index_at_seconds(seconds);
        let t = &self.tempos[i];
        t.beat + (seconds - self.seconds[i]) * t.bpm / 60.0
    }

    pub fn sample_to_beat(&self, sample: u64, sample_rate: u32) -> f64 {
        self.seconds_to_beat(sample as f64 / f64::from(sample_rate))
    }

    pub fn beat_to_sample(&self, beat: f64, sample_rate: u32) -> u64 {
        (self.beat_to_seconds(beat) * f64::from(sample_rate))
            .round()
            .max(0.0) as u64
    }

    fn signature_index_at_beat(&self, beat: f64) -> usize {
        self.signature_beats
            .partition_point(|&b| b <= beat)
            .saturating_sub(1)
    }

    pub fn signature_at_beat(&self, beat: f64) -> TimeSignature {
        self.signatures[self.signature_index_at_beat(beat)]
    }

    pub fn beat_to_bbt(&self, beat: f64) -> BarBeatTick {
        let beat = beat.max(0.0);
        let i = self.signature_index_at_beat(beat);
        let sig = &self.signatures[i];
        let into_sig = beat - self.signature_beats[i];
        let per_bar = sig.beats_per_bar();
        let bars = (into_sig / per_bar).floor();
        let into_bar = into_sig - bars * per_bar;
        // Beats within a bar count in signature units (e.g. eighths in 6/8).
        let unit = 4.0 / f64::from(sig.denominator);
        let beat_in_bar = (into_bar / unit).floor();
        let tick = ((into_bar - beat_in_bar * unit) / unit * f64::from(TICKS_PER_BEAT)).floor();
        BarBeatTick {
            bar: sig.bar.saturating_add(bars as u32).saturating_add(1),
            beat: (beat_in_bar as u32).saturating_add(1),
            tick: (tick as u32).min(TICKS_PER_BEAT - 1),
        }
    }

    pub fn bbt_to_beat(&self, bbt: BarBeatTick) -> f64 {
        let bar = bbt.bar.saturating_sub(1);
        let i = self
            .signatures
            .partition_point(|s| s.bar <= bar)
            .saturating_sub(1);
        let sig = &self.signatures[i];
        let unit = 4.0 / f64::from(sig.denominator);
        self.signature_beats[i]
            + f64::from(bar - sig.bar) * sig.beats_per_bar()
            + f64::from(bbt.beat.saturating_sub(1)) * unit
            + f64::from(bbt.tick) / f64::from(TICKS_PER_BEAT) * unit
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn constant_tempo_conversions() {
        let map = TempoMap::constant(120.0, 4, 4);
        assert!(close(map.beat_to_seconds(4.0), 2.0));
        assert!(close(map.seconds_to_beat(3.0), 6.0));
        assert_eq!(map.beat_to_sample(1.0, 48_000), 24_000);
        assert!(close(map.sample_to_beat(96_000, 48_000), 4.0));
    }

    #[test]
    fn tempo_changes_accumulate() {
        let map = TempoMap::new(
            vec![
                TempoPoint {
                    beat: 0.0,
                    bpm: 120.0,
                },
                TempoPoint {
                    beat: 8.0,
                    bpm: 60.0,
                },
            ],
            vec![],
        );
        assert!(close(map.beat_to_seconds(8.0), 4.0));
        assert!(close(map.beat_to_seconds(10.0), 6.0));
        assert!(close(map.seconds_to_beat(6.0), 10.0));
        assert!(close(map.bpm_at_beat(7.99), 120.0));
        assert!(close(map.bpm_at_beat(8.0), 60.0));
        for beat in [0.0, 3.3, 8.0, 12.7] {
            assert!(close(map.seconds_to_beat(map.beat_to_seconds(beat)), beat));
        }
    }

    #[test]
    fn missing_first_point_is_added() {
        let map = TempoMap::new(
            vec![TempoPoint {
                beat: 4.0,
                bpm: 90.0,
            }],
            vec![],
        );
        assert_eq!(
            map.tempos()[0],
            TempoPoint {
                beat: 0.0,
                bpm: 90.0
            }
        );
        assert_eq!(map.signatures()[0].numerator, 4);
    }

    #[test]
    fn bars_beats_ticks() {
        let map = TempoMap::new(
            vec![],
            vec![
                TimeSignature {
                    bar: 0,
                    numerator: 4,
                    denominator: 4,
                },
                TimeSignature {
                    bar: 2,
                    numerator: 6,
                    denominator: 8,
                },
            ],
        );
        assert_eq!(
            map.beat_to_bbt(0.0),
            BarBeatTick {
                bar: 1,
                beat: 1,
                tick: 0
            }
        );
        assert_eq!(
            map.beat_to_bbt(5.5),
            BarBeatTick {
                bar: 2,
                beat: 2,
                tick: 480
            }
        );
        // Bar 3 starts at beat 8 in 6/8: each eighth note is half a quarter.
        assert_eq!(
            map.beat_to_bbt(8.0),
            BarBeatTick {
                bar: 3,
                beat: 1,
                tick: 0
            }
        );
        assert_eq!(
            map.beat_to_bbt(9.5),
            BarBeatTick {
                bar: 3,
                beat: 4,
                tick: 0
            }
        );
        assert_eq!(
            map.beat_to_bbt(11.0),
            BarBeatTick {
                bar: 4,
                beat: 1,
                tick: 0
            }
        );
        for beat in [0.0, 1.25, 7.5, 8.0, 9.75, 20.0] {
            assert!(
                close(map.bbt_to_beat(map.beat_to_bbt(beat)), beat),
                "{beat}"
            );
        }
    }
}
