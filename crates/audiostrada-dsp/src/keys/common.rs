//! What the electric pianos share after their voices: the mechanical noises, the amplifier
//! (bass and treble, drive), the tremolo, then phaser, chorus, delay and reverb.

use crate::biquad::{Biquad, Coefficients, FilterKind};
use crate::{Effect, db_to_gain};

pub(crate) const TREMOLOS: [&str; 2] = ["Stereo", "Amplitude"];

/// Filtered noise bursts: the hammer's thump, the damper's fall.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Thump {
    seed: u32,
    level: f32,
    fall: f32,
    low: f32,
    smooth: f32,
}

impl Thump {
    /// A burst of `level`, falling in `seconds`, below `hz`.
    pub fn hit(&mut self, level: f32, seconds: f32, hz: f32, rate: f32) {
        if self.seed == 0 {
            self.seed = 0x2545_f491;
        }
        self.level += level;
        self.fall = crate::time_coefficient(seconds * 1000.0, rate);
        self.smooth = crate::lowpass_coefficient(hz, rate);
    }

    #[inline]
    pub fn tick(&mut self) -> f32 {
        if self.level < 1e-6 {
            self.level = 0.0;
            return 0.0;
        }
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        let noise = self.seed as f32 / u32::MAX as f32 * 2.0 - 1.0;
        self.low += self.smooth * (noise - self.low);
        self.level *= self.fall;
        self.low * self.level
    }

    pub fn active(&self) -> bool {
        self.level >= 1e-6
    }
}

/// The amplifier's settings, read at each block.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct AmpSettings {
    pub bass: f32,
    pub treble: f32,
    /// 0 to 1.
    pub drive: f32,
    pub tremolo: usize,
    pub rate: f32,
    /// 0 to 1.
    pub depth: f32,
    /// Mixes of the phaser, the chorus, the delay and the reverb, in percent.
    pub phaser: f32,
    pub chorus: f32,
    pub delay: f32,
    pub reverb: f32,
    /// The delay's note (in `delay::NOTE_NAMES`) and feedback, the reverb's decay in seconds.
    pub delay_note: f32,
    pub feedback: f32,
    pub decay: f32,
    pub volume_db: f32,
    /// Through the small speakers built into the instrument rather than its line out.
    pub speaker: bool,
}

/// The amplifier and effects after the voices.
pub(crate) struct Amp {
    rate: f32,
    started: bool,
    /// Bass and treble as heard (dB), gliding to the settings, and the shelves they give.
    tone: [f32; 2],
    bass: Coefficients,
    treble: Coefficients,
    filters: [[Biquad; 2]; 2],
    /// The built-in speakers: no deep bass, a forward middle, rolled-off highs.
    cabinet: [Coefficients; 3],
    cabinets: [[Biquad; 3]; 2],
    phase: f32,
    /// Settings as heard, gliding to their values so that nothing steps: drive, tremolo
    /// depth, its stereo share, the speakers' share and the volume.
    drive: f32,
    depth: f32,
    stereo: f32,
    speaker: f32,
    volume: f32,
    /// The phaser, chorus, delay and reverb, their mixes as heard, and the delay's note and
    /// feedback and the reverb's decay as last given.
    effects: [Box<dyn Effect>; 4],
    mixes: [f32; 4],
    given: [f32; 3],
    /// The output limiter's gain, and the peak it follows, held over a few milliseconds so
    /// that the gain stays steady through each cycle instead of shaping it.
    limit: f32,
    peak: crate::PeakHold,
}

impl std::fmt::Debug for Amp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Amp")
    }
}

/// An effect of the chain, set as an electric piano wants it.
fn effect(id: &str, settings: &[(&str, f32)]) -> Box<dyn Effect> {
    let mut effect = crate::create(id).expect("built-in effect");
    for &(name, value) in settings {
        set(effect.as_mut(), id, name, value);
    }
    effect
}

fn set(effect: &mut dyn Effect, id: &str, name: &str, value: f32) {
    if let Some(i) = crate::info(id).and_then(|info| info.params.iter().position(|p| p.id == name))
    {
        effect.set_param(i, value);
    }
}

impl Default for Amp {
    fn default() -> Self {
        Self {
            rate: 48_000.0,
            started: false,
            tone: [0.0; 2],
            bass: Coefficients::IDENTITY,
            treble: Coefficients::IDENTITY,
            filters: [[Biquad::default(); 2]; 2],
            cabinet: [Coefficients::IDENTITY; 3],
            cabinets: [[Biquad::default(); 3]; 2],
            phase: 0.0,
            drive: 0.0,
            depth: 0.0,
            stereo: 1.0,
            speaker: 0.0,
            volume: 1.0,
            effects: [
                effect(
                    "phaser",
                    &[
                        ("rate", 0.35),
                        ("depth", 70.0),
                        ("feedback", 35.0),
                        ("mix", 0.0),
                    ],
                ),
                effect(
                    "chorus",
                    &[("rate", 0.6), ("depth", 35.0), ("delay", 9.0), ("mix", 0.0)],
                ),
                effect(
                    "delay",
                    &[
                        ("sync", 1.0),
                        ("pingpong", 1.0),
                        ("low_cut", 150.0),
                        ("high_cut", 5000.0),
                        ("mix", 0.0),
                    ],
                ),
                effect(
                    "reverb",
                    &[
                        ("type", crate::reverb::PLATE as f32),
                        ("predelay", 0.0),
                        ("low_cut", 150.0),
                        ("mix", 0.0),
                    ],
                ),
            ],
            mixes: [0.0; 4],
            given: [f32::NAN; 3],
            limit: 1.0,
            peak: crate::PeakHold::new(240),
        }
    }
}

const GLIDE_MS: f32 = 20.0;
/// Samples between two steps of what glides by stretches (the tone, the effects' mixes).
const STRETCH: usize = 32;
/// The limiter keeps loud chords under this level by turning them down, not by clipping.
const CEILING: f32 = 0.75;
const LIMIT_HOLD_MS: f32 = 5.0;
const LIMIT_ATTACK_MS: f32 = 1.5;
const LIMIT_RELEASE_MS: f32 = 150.0;

impl Amp {
    pub fn prepare(&mut self, rate: u32, max_frames: usize) {
        self.rate = rate as f32;
        self.started = false;
        self.peak = crate::PeakHold::new((LIMIT_HOLD_MS * 0.001 * self.rate) as usize);
        let rate = f64::from(rate);
        self.cabinet = [
            Coefficients::design(FilterKind::HighPass, 120.0, 0.8, 0.0, rate),
            Coefficients::design(FilterKind::Peak, 1800.0, 1.0, 4.0, rate),
            Coefficients::design(FilterKind::LowPass, 5500.0, 0.8, 0.0, rate),
        ];
        for effect in &mut self.effects {
            effect.prepare(self.rate as u32, max_frames);
        }
    }

    pub fn set_tempo(&mut self, bpm: f64) {
        for effect in &mut self.effects {
            effect.set_tempo(bpm);
        }
    }

    fn shelves(&mut self) {
        let rate = f64::from(self.rate);
        self.bass =
            Coefficients::design(FilterKind::LowShelf, 200.0, 0.7, self.tone[0].into(), rate);
        self.treble = Coefficients::design(
            FilterKind::HighShelf,
            3000.0,
            0.7,
            self.tone[1].into(),
            rate,
        );
    }

    pub fn process(&mut self, s: AmpSettings, left: &mut [f32], right: &mut [f32]) {
        self.process_with(s, left, right, |_, _| {});
    }

    /// `process`, with `between` run on each stretch after the amplifier and before the
    /// effects (an organ's rotary speaker).
    pub fn process_with(
        &mut self,
        s: AmpSettings,
        left: &mut [f32],
        right: &mut [f32],
        mut between: impl FnMut(&mut [f32], &mut [f32]),
    ) {
        let stereo = if s.tremolo == 0 { 1.0 } else { 0.0 };
        let speaker = if s.speaker { 1.0 } else { 0.0 };
        let volume = db_to_gain(s.volume_db);
        if !self.started {
            self.started = true;
            self.tone = [s.bass, s.treble];
            self.shelves();
            (self.drive, self.depth, self.stereo) = (s.drive, s.depth, stereo);
            (self.speaker, self.volume) = (speaker, volume);
        }
        let wanted = [s.delay_note, s.feedback, s.decay];
        for (k, (effect, name)) in [(2, "note"), (2, "feedback"), (3, "decay")]
            .into_iter()
            .enumerate()
        {
            if wanted[k] != self.given[k] {
                self.given[k] = wanted[k];
                set(self.effects[effect].as_mut(), IDS[effect], name, wanted[k]);
            }
        }
        let glide = 1.0 - crate::time_coefficient(GLIDE_MS, self.rate);
        let stretch_glide = 1.0 - crate::time_coefficient(GLIDE_MS, self.rate / STRETCH as f32);
        let step = s.rate / self.rate;
        let release = crate::time_coefficient(LIMIT_RELEASE_MS, self.rate);
        let attack = crate::time_coefficient(LIMIT_ATTACK_MS, self.rate);
        let mut at = 0;
        while at < left.len() {
            let n = STRETCH.min(left.len() - at);
            let (l, r) = (&mut left[at..at + n], &mut right[at..at + n]);
            at += n;
            let target = [s.bass, s.treble];
            if (0..2).any(|k| (target[k] - self.tone[k]).abs() > 0.01) {
                for k in 0..2 {
                    self.tone[k] += stretch_glide * (target[k] - self.tone[k]);
                }
                self.shelves();
            }
            for (l, r) in l.iter_mut().zip(r.iter_mut()) {
                self.drive += glide * (s.drive - self.drive);
                self.depth += glide * (s.depth - self.depth);
                self.stereo += glide * (stereo - self.stereo);
                self.speaker += glide * (speaker - self.speaker);
                self.volume += glide * (volume - self.volume);
                let mut sides = [*l, *r];
                for (k, x) in sides.iter_mut().enumerate() {
                    let [b, t] = &mut self.filters[k];
                    let y = t.process(&self.treble, b.process(&self.bass, f64::from(*x)));
                    let direct = f64::from(drive(y as f32, self.drive));
                    let mut boxed = direct;
                    if self.speaker > 1e-4 {
                        for (f, c) in self.cabinets[k].iter_mut().zip(&self.cabinet) {
                            boxed = f.process(c, boxed);
                        }
                    }
                    let share = f64::from(self.speaker);
                    *x = (direct + share * (boxed - direct)) as f32;
                }
                let wave = (self.phase * std::f32::consts::TAU).sin();
                self.phase = (self.phase + step).fract();
                // The stereo tremolo pans, the other one swells both sides together.
                let swing = self.depth * 0.5 * wave;
                let center = 1.0 - self.depth * 0.5;
                let gl = center - swing;
                let gr = center + swing * (2.0 * self.stereo - 1.0);
                *l = sides[0] * gl * self.volume;
                *r = sides[1] * gr * self.volume;
            }
            between(l, r);
            let mixes = [s.phaser, s.chorus, s.delay, s.reverb];
            for k in 0..4 {
                let heard = self.mixes[k];
                if heard <= 0.0 && mixes[k] <= 0.0 {
                    continue;
                }
                // One that comes back in starts clean.
                if heard <= 0.0 {
                    self.effects[k].reset();
                }
                let mut next = heard + stretch_glide * (mixes[k] - heard);
                if mixes[k] <= 0.0 && next < 0.05 {
                    next = 0.0;
                }
                if next != heard {
                    set(self.effects[k].as_mut(), IDS[k], "mix", next);
                    self.mixes[k] = next;
                }
                if next > 0.0 {
                    self.effects[k].process(l, r);
                }
            }
            for (l, r) in l.iter_mut().zip(r.iter_mut()) {
                let peak = self.peak.push(l.abs().max(r.abs()));
                let want = if peak > CEILING { CEILING / peak } else { 1.0 };
                // Down in a moment, back up slowly; what passes meanwhile is rounded off.
                let k = if want < self.limit { attack } else { release };
                self.limit = want + k * (self.limit - want);
                *l = headroom(*l * self.limit);
                *r = headroom(*r * self.limit);
            }
        }
    }

    pub fn reset(&mut self) {
        self.filters = [[Biquad::default(); 2]; 2];
        self.cabinets = [[Biquad::default(); 3]; 2];
        self.phase = 0.0;
        self.limit = 1.0;
        self.peak.clear();
        for effect in &mut self.effects {
            effect.reset();
        }
    }
}

const IDS: [&str; 4] = ["phaser", "chorus", "delay", "reverb"];

/// The last stage's headroom: untouched below -1.4 dBFS, then rounded off under full scale.
#[inline]
fn headroom(x: f32) -> f32 {
    const KNEE: f32 = 0.85;
    const ROOM: f32 = 0.13;
    let a = x.abs();
    if a <= KNEE {
        return x;
    }
    (KNEE + ROOM * ((a - KNEE) / ROOM).tanh()).copysign(x)
}

/// The power amp pushed: asymmetric soft clipping, about as loud as it came in.
#[inline]
fn drive(x: f32, amount: f32) -> f32 {
    if amount <= 1e-4 {
        return x;
    }
    const BIAS: f32 = 0.2;
    let gain = db_to_gain(amount * 30.0);
    let y = (gain * x + BIAS).tanh() - BIAS.tanh();
    y / gain.powf(0.75) * (1.0 + amount)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> AmpSettings {
        AmpSettings {
            bass: 0.0,
            treble: 0.0,
            drive: 0.0,
            tremolo: 0,
            rate: 5.0,
            depth: 0.0,
            phaser: 0.0,
            chorus: 0.0,
            delay: 0.0,
            reverb: 0.0,
            delay_note: 6.0,
            feedback: 30.0,
            decay: 1.5,
            volume_db: 0.0,
            speaker: false,
        }
    }

    #[test]
    fn the_amplifier_is_clean_at_rest_and_the_stereo_tremolo_pans() {
        let mut amp = Amp::default();
        amp.prepare(48_000, 512);
        let (mut l, mut r) = (vec![0.5; 512], vec![0.5; 512]);
        amp.process(settings(), &mut l, &mut r);
        assert!((l[500] - 0.5).abs() < 1e-3 && (r[500] - 0.5).abs() < 1e-3);
        let s = AmpSettings {
            depth: 1.0,
            ..settings()
        };
        amp.prepare(48_000, 4800);
        let (mut l, mut r) = (vec![0.5; 4800], vec![0.5; 4800]);
        amp.process(s, &mut l, &mut r);
        // A quarter of a cycle in, the tremolo has panned the piano to the right.
        let i = 48_000 / 5 / 4;
        assert!(l[i] < 0.05 && r[i] > 0.45, "{} {}", l[i], r[i]);
        assert!(drive(0.9, 1.0).abs() < 1.6);
        assert!(headroom(3.0) < 1.0 && headroom(-3.0) > -1.0 && headroom(0.5) == 0.5);
    }

    #[test]
    fn loud_chords_are_turned_down_rather_than_clipped() {
        let mut amp = Amp::default();
        amp.prepare(48_000, 4800);
        let tone: Vec<f32> = (0..4800).map(|i| 2.0 * (i as f32 * 0.05).sin()).collect();
        let (mut l, mut r) = (tone.clone(), tone);
        amp.process(settings(), &mut l, &mut r);
        let late = &l[2400..];
        let peak = late.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        assert!(peak < CEILING * 1.05, "{peak}");
        // Turned down whole, the sine keeps its shape: its peak sits where a sine's would.
        let quarter = late.iter().filter(|x| x.abs() > peak * 0.7).count() as f32;
        assert!((quarter / late.len() as f32 - 0.5).abs() < 0.06);
    }

    #[test]
    fn a_thump_dies_away() {
        let mut thump = Thump::default();
        thump.hit(1.0, 0.01, 1000.0, 48_000.0);
        let early: f32 = (0..480).map(|_| thump.tick().abs()).sum();
        for _ in 0..48_000 {
            thump.tick();
        }
        assert!(early > 0.0 && !thump.active());
    }
}
