//! Click on every beat while the transport plays: the first beat of the bar and the starts
//! of accented groups stand out, subdivisions are softer, compound meters click their dotted
//! quarters. Goes straight to the hardware outputs: never in the mix or in exports.

use std::f32::consts::TAU;

use crate::param::SharedParam;
use crate::{ProcessContext, Processor};

/// UI side of the metronome.
#[derive(Clone, Debug)]
pub struct MetronomeControls {
    /// 1 when the click sounds.
    pub enabled: SharedParam,
    /// Linear gain.
    pub gain: SharedParam,
    /// Index of the sound (`audiostrada_core::ClickSound` order).
    pub sound: SharedParam,
    /// Clicks per beat.
    pub subdivision: SharedParam,
    /// 1 to accent the first beat.
    pub accent: SharedParam,
    /// 1 to click dotted quarters in 6/8, 9/8, 12/8.
    pub compound: SharedParam,
    /// Beats starting an accented group, one bit per beat.
    pub groups: SharedParam,
}

/// How a click stands out.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Level {
    Bar,
    Group,
    Beat,
    Sub,
}

#[derive(Clone, Copy, Debug)]
struct Voice {
    phases: [f32; 2],
    increments: [f32; 2],
    levels: [f32; 2],
    decays: [f32; 2],
    /// Share of filtered noise (hi-hat).
    noise: f32,
    seed: u32,
    last: f32,
    remaining: usize,
}

#[derive(Debug)]
pub struct Metronome {
    controls: MetronomeControls,
    rate: f32,
    voice: Option<Voice>,
}

impl Metronome {
    pub fn new() -> (Self, MetronomeControls) {
        let controls = MetronomeControls {
            enabled: SharedParam::new(0.0),
            gain: SharedParam::new(0.5),
            sound: SharedParam::new(0.0),
            subdivision: SharedParam::new(1.0),
            accent: SharedParam::new(1.0),
            compound: SharedParam::new(1.0),
            groups: SharedParam::new(0.0),
        };
        (
            Self {
                controls: controls.clone(),
                rate: 48_000.0,
                voice: None,
            },
            controls,
        )
    }

    fn click(&self, level: Level) -> Voice {
        let (pitch, gain) = match level {
            Level::Bar => (1.33, 1.0),
            Level::Group => (1.15, 0.85),
            Level::Beat => (1.0, 0.6),
            Level::Sub => (1.0, 0.3),
        };
        // Partials (Hz), their levels, their decay times (s), the share of noise.
        let (freqs, mix, times, noise): ([f32; 2], [f32; 2], [f32; 2], f32) =
            match self.controls.sound.get().round() as u32 {
                1 => ([880.0, 2_300.0], [1.0, 0.5], [0.03, 0.012], 0.0),
                2 => ([2_500.0, 5_000.0], [1.0, 0.15], [0.02, 0.008], 0.0),
                3 => ([540.0, 800.0], [0.7, 0.7], [0.09, 0.06], 0.0),
                4 => ([8_000.0, 1.0], [0.0, 0.0], [0.04, 0.04], 1.0),
                _ => ([1_320.0, 1.0], [1.0, 0.0], [0.035, 0.035], 0.0),
            };
        let length =
            times[0].max(times[1]) * self.rate * if level == Level::Sub { 0.7 } else { 1.0 };
        // Each partial falls by 60 dB over its time.
        let decay = |t: f32| 0.001f32.powf(1.0 / (t * self.rate).max(1.0));
        Voice {
            phases: [0.0; 2],
            increments: freqs.map(|f| (f * pitch).min(self.rate * 0.45) / self.rate),
            levels: mix.map(|m| m * gain),
            decays: times.map(decay),
            noise: noise * gain,
            seed: 0x1234_5678,
            last: 0.0,
            remaining: length as usize,
        }
    }

    fn render(&mut self, left: &mut [f32], right: &mut [f32], gain: f32) {
        let Some(v) = &mut self.voice else {
            return;
        };
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            if v.remaining == 0 {
                self.voice = None;
                return;
            }
            let mut s = 0.0;
            for k in 0..2 {
                s += (v.phases[k] * TAU).sin() * v.levels[k];
                v.phases[k] = (v.phases[k] + v.increments[k]).fract();
                v.levels[k] *= v.decays[k];
            }
            if v.noise > 0.0 {
                v.seed = v.seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let white = (v.seed >> 8) as f32 / (1 << 24) as f32 * 2.0 - 1.0;
                // A first difference keeps the highs.
                s += (white - v.last) * 0.5 * v.noise;
                v.last = white;
                v.noise *= v.decays[0];
            }
            *l += s * gain;
            *r += s * gain;
            v.remaining -= 1;
        }
    }
}

/// The click grid of a bar: ticks per signature unit, and how each tick sounds.
#[derive(Clone, Copy, Debug)]
struct Grid {
    ticks_per_unit: u32,
    units: u32,
    /// Beats are dotted quarters (6/8, 9/8, 12/8).
    compound: bool,
    /// In a compound meter, the eighths between the beats click too.
    eighths: bool,
    accent: bool,
    groups: u32,
}

impl Grid {
    fn new(controls: &MetronomeControls, units: u32, unit: f64) -> Self {
        let subdivision = (controls.subdivision.get().round() as u32).clamp(1, 4);
        let compound = controls.compound.get() >= 0.5
            && (unit - 0.5).abs() < 1e-9
            && units.is_multiple_of(3)
            && units > 3;
        Self {
            ticks_per_unit: if compound { 1 } else { subdivision },
            units: units.max(1),
            compound,
            eighths: compound && subdivision > 1,
            accent: controls.accent.get() >= 0.5,
            groups: controls.groups.get() as u32,
        }
    }

    fn ticks(&self) -> u64 {
        u64::from(self.units) * u64::from(self.ticks_per_unit)
    }

    fn level(&self, tick: u64) -> Option<Level> {
        let unit = (tick / u64::from(self.ticks_per_unit)) as u32;
        let on_unit = tick.is_multiple_of(u64::from(self.ticks_per_unit));
        let first = if self.accent { Level::Bar } else { Level::Beat };
        if self.compound {
            // Dotted quarters are the beats; the eighths between only when subdivided.
            return match (unit, unit.is_multiple_of(3)) {
                (0, _) => Some(first),
                (_, true) => Some(Level::Beat),
                _ if self.eighths => Some(Level::Sub),
                _ => None,
            };
        }
        Some(match (unit, on_unit) {
            (_, false) => Level::Sub,
            (0, true) => first,
            (u, true) if u < 32 && self.groups & (1 << u) != 0 => Level::Group,
            _ => Level::Beat,
        })
    }
}

impl Processor for Metronome {
    fn prepare(&mut self, sample_rate: u32, _max_frames: usize) {
        self.rate = sample_rate as f32;
        self.voice = None;
    }

    fn process(&mut self, ctx: &mut ProcessContext<'_>) {
        let b = &mut ctx.buffers;
        if b.output_channels() < 2 {
            return;
        }
        let (left, right) = b.output_pair(0, 1);
        left.fill(0.0);
        right.fill(0.0);
        let n = left.len();
        let gain = self.controls.gain.get();
        let t = ctx.transport;
        let mut start = 0;
        if self.controls.enabled.get() >= 0.5 && t.playing && t.bpm > 0.0 {
            let bar = t.bar;
            let grid = Grid::new(&self.controls, bar.beats_per_bar, bar.unit);
            let per_unit = f64::from(grid.ticks_per_unit);
            let frames_per_tick = bar.unit * 60.0 / t.bpm * f64::from(t.sample_rate) / per_unit;
            // Ticks since the start of the bar, and the next one at or after this block.
            let now = (f64::from(bar.beat.max(1)) - 1.0 + bar.fraction) * per_unit;
            let next = if (now - now.round()).abs() < 1e-9 {
                now.round()
            } else {
                now.ceil()
            };
            let mut offset = ((next - now) * frames_per_tick).max(0.0);
            let mut tick = next as u64 % grid.ticks();
            while offset < n as f64 {
                if let Some(level) = grid.level(tick) {
                    let at = offset as usize;
                    self.render(&mut left[start..at], &mut right[start..at], gain);
                    self.voice = Some(self.click(level));
                    start = at;
                }
                offset += frames_per_tick;
                tick = (tick + 1) % grid.ticks();
            }
        }
        self.render(&mut left[start..], &mut right[start..], gain);
    }

    fn reset(&mut self) {
        self.voice = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(units: u32, unit: f64, subdivision: f32, groups: u32) -> Grid {
        let (_, controls) = Metronome::new();
        controls.subdivision.set(subdivision);
        controls.groups.set(groups as f32);
        Grid::new(&controls, units, unit)
    }

    fn levels(grid: &Grid) -> Vec<Option<Level>> {
        (0..grid.ticks()).map(|t| grid.level(t)).collect()
    }

    /// Peak and length of one click.
    fn sound(metronome: &mut Metronome, level: Level) -> (f32, usize) {
        metronome.voice = Some(metronome.click(level));
        let (mut left, mut right) = (vec![0.0; 48_000], vec![0.0; 48_000]);
        metronome.render(&mut left, &mut right, 1.0);
        assert_eq!(left, right);
        assert!(metronome.voice.is_none(), "a click ends");
        let peak = left.iter().fold(0f32, |m, s| m.max(s.abs()));
        (peak, left.iter().rposition(|s| *s != 0.0).unwrap_or(0) + 1)
    }

    #[test]
    fn every_sound_clicks_louder_on_accents_and_shorter_on_subdivisions() {
        let (mut metronome, controls) = Metronome::new();
        for index in 0..audiostrada_core::ClickSound::ALL.len() {
            controls.sound.set(index as f32);
            let (bar, bar_length) = sound(&mut metronome, Level::Bar);
            let (group, _) = sound(&mut metronome, Level::Group);
            let (beat, beat_length) = sound(&mut metronome, Level::Beat);
            let (sub, sub_length) = sound(&mut metronome, Level::Sub);
            assert!(bar.is_finite() && bar > 0.0 && bar < 2.0, "sound {index}");
            assert!(bar > group && group > beat && beat > sub, "sound {index}");
            assert!(sub_length < beat_length && beat_length <= bar_length);
        }
    }

    #[test]
    fn compound_meters_click_dotted_quarters() {
        use Level::*;
        // 6/8: beats on eighths 1 and 4, the others silent unless subdivided.
        let six = grid(6, 0.5, 1.0, 0);
        assert_eq!(
            levels(&six),
            [Some(Bar), None, None, Some(Beat), None, None]
        );
        let six = grid(6, 0.5, 2.0, 0);
        assert_eq!(levels(&six)[1], Some(Sub));
        // 3/8 and 3/4 stay simple.
        assert_eq!(
            levels(&grid(3, 0.5, 1.0, 0)),
            [Some(Bar), Some(Beat), Some(Beat)]
        );
    }

    #[test]
    fn groups_and_subdivisions_shape_the_bar() {
        use Level::*;
        // 7/8 as 2+2+3: accents on eighths 1, 3 and 5.
        let seven = grid(7, 0.5, 1.0, 0b10101);
        assert_eq!(
            levels(&seven),
            [
                Some(Bar),
                Some(Beat),
                Some(Group),
                Some(Beat),
                Some(Group),
                Some(Beat),
                Some(Beat)
            ]
        );
        // 4/4 in sixteenths: 16 ticks, softer between the beats.
        let four = grid(4, 1.0, 4.0, 0);
        assert_eq!(four.ticks(), 16);
        assert_eq!(
            levels(&four)[..5],
            [Some(Bar), Some(Sub), Some(Sub), Some(Sub), Some(Beat)]
        );
    }
    use crate::{BarPosition, TransportInfo};
    use audiostrada_io::ChannelBuffers;

    #[test]
    fn clicks_on_beats_with_an_accent() {
        let (mut metronome, controls) = Metronome::new();
        metronome.prepare(48_000, 480);
        controls.enabled.set(1.0);
        let mut storage = ChannelBuffers::new(0, 2, 480);
        let mut onsets = Vec::new();
        let mut peaks = Vec::new();
        // 120 BPM in 3/4: a beat every 24000 frames, starting halfway through beat 3.
        let mut beat = 3;
        let mut fraction = 0.5;
        for block in 0..300u64 {
            let transport = TransportInfo {
                playing: true,
                position: block * 480,
                sample_rate: 48_000,
                beat: 0.0,
                bpm: 120.0,
                bar: BarPosition {
                    beat,
                    fraction,
                    beats_per_bar: 3,
                    unit: 1.0,
                },
            };
            metronome.process(&mut ProcessContext {
                buffers: storage.buffers(480),
                transport: &transport,
            });
            let left = storage.output(0);
            if let Some(i) = left.iter().position(|s| s.abs() > 1e-6)
                && onsets
                    .last()
                    .is_none_or(|&o: &u64| block * 480 + i as u64 > o + 2000)
            {
                onsets.push(block * 480 + i as u64);
                peaks.push(left.iter().fold(0.0f32, |m, s| m.max(s.abs())));
            }
            fraction += 480.0 / 24_000.0;
            if fraction >= 1.0 - 1e-9 {
                fraction -= 1.0;
                beat = beat % 3 + 1;
            }
        }
        for (onset, expected) in onsets.iter().zip([12_000, 36_000, 60_000, 84_000]) {
            assert!(onset.abs_diff(expected) <= 2, "{onsets:?}");
        }
        assert!(peaks[0] > peaks[1] * 1.3, "downbeat accented: {peaks:?}");
        assert!(peaks[3] > peaks[2] * 1.3);

        controls.enabled.set(0.0);
        metronome.reset();
        let transport = TransportInfo {
            playing: true,
            position: 0,
            sample_rate: 48_000,
            beat: 0.0,
            bpm: 120.0,
            bar: BarPosition::default(),
        };
        metronome.process(&mut ProcessContext {
            buffers: storage.buffers(480),
            transport: &transport,
        });
        assert!(storage.output(0).iter().all(|&s| s == 0.0));
    }
}
