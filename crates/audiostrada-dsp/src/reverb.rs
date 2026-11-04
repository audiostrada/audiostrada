//! Reverb: a room, a hall and a vast space from a modulated feedback delay network with early
//! reflections, a studio plate (Dattorro), a spring tank, and a shimmer that feeds the space
//! back an octave up. Shared controls drive every engine, with freeze and ducking of the tail
//! while the input plays.

use crate::delay::{NOTE_NAMES, synced_seconds};
use crate::hall::{self, Hall};
use crate::plate::{self, Plate};
use crate::spring::Spring;
use crate::{Effect, EffectInfo, ParamInfo, choice, param, switch, time_coefficient};

pub const INFO: EffectInfo = EffectInfo {
    id: "reverb",
    name: "Strada-Verb",
    params: &PARAMS,
    meters: &[],
    instrument: false,
    files: &[],
    category: "Space",
    description: "Room, hall, space, plate, spring and shimmer, with freeze and ducking.",
};

/// Spaces, in parameter order.
pub const TYPES: [&str; 6] = ["Room", "Hall", "Space", "Plate", "Spring", "Shimmer"];

/// What each space sounds like.
pub const TYPE_HINTS: [&str; 6] = [
    "A room: close early reflections, a short and natural tail",
    "A concert hall: wide, with a long and soft tail",
    "A vast space: huge, with long reflections and a dark, endless tail",
    "A studio plate: dense and bright from the start, no early reflections",
    "A spring tank, as in guitar amps: twangy, with a metallic splash",
    "A vast space whose tail rises an octave as it rings, for pads and ambient sounds",
];

pub const ROOM: usize = 0;
pub const HALL: usize = 1;
pub const SPACE: usize = 2;
pub const PLATE: usize = 3;
pub const SPRING: usize = 4;
pub const SHIMMER: usize = 5;

const PARAMS: [ParamInfo; COUNT] = [
    choice("type", "Space", &TYPES, HALL as f32),
    param("decay", "Decay", 0.2, 20.0, 2.0, "s"),
    param("predelay", "Pre-delay", 0.0, 200.0, 20.0, "ms"),
    param("low_cut", "Low cut", 20.0, 1000.0, 100.0, "Hz"),
    param("high_cut", "High cut", 1000.0, 20000.0, 9000.0, "Hz"),
    param("width", "Width", 0.0, 150.0, 100.0, "%"),
    param("mix", "Mix", 0.0, 100.0, 25.0, "%"),
    param("duck", "Ducking", 0.0, 100.0, 0.0, "%"),
    param("size", "Size", 0.0, 100.0, 60.0, "%"),
    param("early", "Early / late", 0.0, 100.0, 40.0, "%"),
    param("treble", "Treble decay", 10.0, 100.0, 45.0, "%"),
    param("bass", "Bass decay", 50.0, 200.0, 120.0, "%"),
    param("diffusion", "Diffusion", 0.0, 100.0, 80.0, "%"),
    param("modulation", "Modulation", 0.0, 100.0, 40.0, "%"),
    switch("freeze", "Freeze", 0.0),
    param("shimmer", "Shimmer", 0.0, 100.0, 50.0, "%"),
    switch("predelay_sync", "Pre-delay sync", 0.0),
    choice("predelay_note", "Pre-delay note", &NOTE_NAMES, 2.0),
    param("mod_rate", "Modulation rate", 0.1, 5.0, 0.37, "Hz"),
    param("crossover", "Bass crossover", 80.0, 1000.0, 250.0, "Hz"),
    param("duck_release", "Ducking release", 50.0, 2000.0, 300.0, "ms"),
];

const TYPE: usize = 0;
const DECAY: usize = 1;
const PREDELAY: usize = 2;
const LOW_CUT: usize = 3;
const HIGH_CUT: usize = 4;
const WIDTH: usize = 5;
const MIX: usize = 6;
const DUCK: usize = 7;
const SIZE: usize = 8;
const EARLY: usize = 9;
const TREBLE: usize = 10;
const BASS: usize = 11;
const DIFFUSION: usize = 12;
const MODULATION: usize = 13;
const FREEZE: usize = 14;
const AMOUNT: usize = 15;
const PREDELAY_SYNC: usize = 16;
const PREDELAY_NOTE: usize = 17;
const MOD_RATE: usize = 18;
const CROSSOVER: usize = 19;
const DUCK_RELEASE: usize = 20;
const COUNT: usize = 21;

/// Input level from which ducking is at its full depth.
const DUCK_FULL: f32 = 0.25;
/// Block in which the shimmer's octave is fed back, in samples.
const SHIMMER_BLOCK: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Engine {
    Rooms,
    Plate,
    Spring,
}

fn engine(kind: usize) -> Engine {
    match kind {
        PLATE => Engine::Plate,
        SPRING => Engine::Spring,
        _ => Engine::Rooms,
    }
}

/// Whether the space `kind` uses the parameter `id` (the editor greys the others).
pub fn uses(kind: usize, id: &str) -> bool {
    match id {
        "size" | "early" | "bass" | "mod_rate" | "crossover" => engine(kind) == Engine::Rooms,
        "modulation" => engine(kind) != Engine::Spring,
        "shimmer" => kind == SHIMMER,
        _ => true,
    }
}

/// Approximate decay of the lows and of the highs in seconds, for the editor's display.
pub fn decay_seconds(kind: usize, decay: f32, bass: f32, treble: f32) -> (f32, f32) {
    match engine(kind) {
        Engine::Plate => {
            let (_, highs) = plate::decay_seconds(plate::decay_percent(decay), damping(treble));
            (decay, highs)
        }
        Engine::Spring => (decay, decay * (0.3 + 0.7 * treble / 100.0)),
        Engine::Rooms => {
            let (lows, mid, highs) = hall::decay_seconds(decay, bass, treble);
            (lows.max(mid), highs)
        }
    }
}

/// The plate's damping frequency for a treble decay (10 to 100 %).
fn damping(treble: f32) -> f32 {
    1000.0 + (treble - 10.0) / 90.0 * 17_000.0
}

/// Pitch shifter an octave up: two taps sweep a delay line at twice the speed it is written,
/// crossfaded so that each jump back happens in silence. Reads lag the writes by at least
/// [`SHIMMER_BLOCK`], so a block can be read before it is written.
#[derive(Debug, Default)]
struct Octave {
    line: Vec<f32>,
    write: usize,
    phase: f32,
    window: f32,
}

impl Octave {
    fn prepare(&mut self, rate: f32) {
        self.window = (0.05 * rate).round();
        self.line = vec![0.0; self.window as usize + SHIMMER_BLOCK + 8];
        self.reset();
    }

    fn reset(&mut self) {
        self.line.fill(0.0);
        self.write = 0;
        self.phase = 0.0;
    }

    fn push(&mut self, x: f32) {
        self.line[self.write] = x;
        self.write = (self.write + 1) % self.line.len();
    }

    /// The shifted sample `ahead` samples past the last one written.
    fn read(&mut self, ahead: usize) -> f32 {
        let len = self.line.len();
        let mut out = 0.0;
        for tap in [0.0, 0.5] {
            let p = (self.phase + tap).fract();
            let delay = (SHIMMER_BLOCK as f32 + self.window * (1.0 - p) - ahead as f32).max(1.0);
            let i = delay.floor();
            let frac = delay - i;
            let at = |d: usize| self.line[(self.write + len - d.min(len - 1)) % len];
            let v = at(i as usize) + (at(i as usize + 1) - at(i as usize)) * frac;
            let fade = (std::f32::consts::PI * p).sin();
            out += v * fade * fade;
        }
        self.phase = (self.phase + 1.0 / self.window).fract();
        out
    }
}

pub struct Reverb {
    values: [f32; COUNT],
    rate: f32,
    bpm: f64,
    hall: Hall,
    plate: Plate,
    spring: Spring,
    octave: Octave,
    wet: [Vec<f32>; 2],
    envelope: f32,
    attack: f32,
    release: f32,
}

impl std::fmt::Debug for Reverb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Reverb")
            .field("values", &self.values)
            .finish_non_exhaustive()
    }
}

impl Default for Reverb {
    fn default() -> Self {
        let mut reverb = Self {
            values: PARAMS.map(|p| p.default),
            rate: 48_000.0,
            bpm: 120.0,
            hall: Hall::default(),
            plate: Plate::default(),
            spring: Spring::default(),
            octave: Octave::default(),
            wet: [vec![0.0; 1024], vec![0.0; 1024]],
            envelope: 0.0,
            attack: 0.0,
            release: 0.0,
        };
        // The engines give the tail alone; the dry signal is mixed here.
        reverb.hall.set_param(12, 100.0);
        reverb.plate.set_param(8, 100.0);
        for index in 0..PARAMS.len() {
            reverb.forward(index);
        }
        reverb.set_rate(48_000.0);
        reverb
    }
}

impl Reverb {
    fn kind(&self) -> usize {
        (self.values[TYPE].round().max(0.0) as usize).min(SHIMMER)
    }

    fn set_rate(&mut self, rate: f32) {
        self.rate = rate;
        self.attack = 1.0 - time_coefficient(1.0, rate);
        self.release = 1.0 - time_coefficient(self.values[DUCK_RELEASE], rate);
        self.octave.prepare(rate);
    }

    /// Pre-delay in ms: the note's length at the tempo when synced, within the engines' 200 ms.
    fn predelay_ms(&self) -> f32 {
        if self.values[PREDELAY_SYNC] >= 0.5 {
            (synced_seconds(self.values[PREDELAY_NOTE], self.bpm) * 1000.0).min(200.0)
        } else {
            self.values[PREDELAY]
        }
    }

    /// Hands a parameter to the engine settings it maps to.
    fn forward(&mut self, index: usize) {
        let v = self.values[index];
        let (h, p, s) = (&mut self.hall, &mut self.plate, &mut self.spring);
        match index {
            TYPE => {
                let kind = match (v.round().max(0.0) as usize).min(SHIMMER) {
                    ROOM => 0.0,
                    HALL => 1.0,
                    _ => 2.0,
                };
                h.set_param(0, kind);
            }
            DECAY => {
                h.set_param(2, v);
                p.set_param(1, plate::decay_percent(v));
                s.decay = v;
            }
            PREDELAY | PREDELAY_SYNC | PREDELAY_NOTE => {
                let ms = {
                    let v = self.predelay_ms();
                    self.hall.set_param(3, v);
                    self.plate.set_param(0, v);
                    v
                };
                self.spring.predelay_ms = ms;
            }
            MOD_RATE => h.set_param(14, v),
            CROSSOVER => h.set_param(15, v),
            DUCK_RELEASE => self.release = 1.0 - time_coefficient(v, self.rate),
            LOW_CUT => {
                h.set_param(9, v);
                p.set_param(6, v);
                s.low_cut = v;
            }
            HIGH_CUT => {
                h.set_param(10, v);
                p.set_param(3, v);
                s.high_cut = v;
            }
            WIDTH => {
                h.set_param(11, v);
                p.set_param(7, v);
                s.width = v;
            }
            SIZE => h.set_param(1, v),
            EARLY => h.set_param(4, v),
            TREBLE => {
                h.set_param(5, v);
                p.set_param(2, damping(v));
                s.treble = v;
            }
            BASS => h.set_param(6, v),
            DIFFUSION => {
                h.set_param(7, v);
                p.set_param(4, v);
                s.diffusion = v;
            }
            MODULATION => {
                h.set_param(8, v);
                p.set_param(5, v);
            }
            FREEZE => {
                h.set_param(13, v);
                p.set_param(9, v);
                s.freeze = v >= 0.5;
            }
            _ => {}
        }
    }

    /// The rooms with the octave of their own tail fed back into them, block by block.
    fn shimmer(&mut self, left: &mut [f32], right: &mut [f32]) {
        // A frozen tail is held as it is: feeding it more would only make it grow.
        let feedback = if self.values[FREEZE] >= 0.5 {
            0.0
        } else {
            self.values[AMOUNT] / 100.0 * 0.6
        };
        for (l, r) in left
            .chunks_mut(SHIMMER_BLOCK)
            .zip(right.chunks_mut(SHIMMER_BLOCK))
        {
            for (i, (a, b)) in l.iter_mut().zip(r.iter_mut()).enumerate() {
                let up = feedback * self.octave.read(i).tanh();
                *a += up;
                *b += up;
            }
            self.hall.process(l, r);
            for (a, b) in l.iter().zip(r.iter()) {
                self.octave.push(0.5 * (a + b));
            }
        }
    }
}

impl Effect for Reverb {
    fn prepare(&mut self, sample_rate: u32, max_frames: usize) {
        self.hall.prepare(sample_rate, max_frames);
        self.plate.prepare(sample_rate, max_frames);
        self.spring.prepare(sample_rate);
        self.wet = [vec![0.0; max_frames.max(1)], vec![0.0; max_frames.max(1)]];
        self.envelope = 0.0;
        self.set_rate(sample_rate as f32);
    }

    fn set_tempo(&mut self, bpm: f64) {
        if (bpm - self.bpm).abs() > 1e-6 {
            self.bpm = bpm;
            self.forward(PREDELAY);
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        let before = self.kind();
        let Some(v) = self.values.get_mut(index) else {
            return;
        };
        *v = value;
        self.forward(index);
        // The engine coming in starts silent rather than with an old tail.
        let kind = self.kind();
        if engine(kind) != engine(before) {
            match engine(kind) {
                Engine::Rooms => self.hall.reset(),
                Engine::Plate => self.plate.reset(),
                Engine::Spring => self.spring.reset(),
            }
        }
        if kind != before && kind == SHIMMER {
            self.octave.reset();
        }
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let mix = self.values[MIX] / 100.0;
        let duck = self.values[DUCK] / 100.0;
        let kind = self.kind();
        let step = self.wet[0].len();
        for (left, right) in left.chunks_mut(step).zip(right.chunks_mut(step)) {
            let n = left.len();
            let mut wet = std::mem::take(&mut self.wet);
            let [wet_l, wet_r] = &mut wet;
            let (wet_l, wet_r) = (&mut wet_l[..n], &mut wet_r[..n]);
            wet_l.copy_from_slice(left);
            wet_r.copy_from_slice(right);
            match kind {
                SHIMMER => self.shimmer(wet_l, wet_r),
                PLATE => self.plate.process(wet_l, wet_r),
                SPRING => self.spring.process(wet_l, wet_r),
                _ => self.hall.process(wet_l, wet_r),
            }
            for i in 0..n {
                let level = left[i].abs().max(right[i].abs());
                let coefficient = if level > self.envelope {
                    self.attack
                } else {
                    self.release
                };
                self.envelope += coefficient * (level - self.envelope);
                let gain = mix * (1.0 - duck * (self.envelope / DUCK_FULL).min(1.0));
                left[i] = left[i] * (1.0 - mix) + wet_l[i] * gain;
                right[i] = right[i] * (1.0 - mix) + wet_r[i] * gain;
            }
            self.wet = wet;
        }
    }

    fn reset(&mut self) {
        self.hall.reset();
        self.plate.reset();
        self.spring.reset();
        self.octave.reset();
        self.envelope = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reverb(settings: &[(usize, f32)]) -> Reverb {
        let mut reverb = Reverb::default();
        reverb.set_param(MIX, 100.0);
        for &(index, value) in settings {
            reverb.set_param(index, value);
        }
        reverb.prepare(48_000, 256);
        reverb
    }

    fn impulse(reverb: &mut Reverb, seconds: f32) -> (Vec<f32>, Vec<f32>) {
        let n = (48_000.0 * seconds) as usize;
        let (mut l, mut r) = (vec![0.0f32; n], vec![0.0f32; n]);
        l[0] = 1.0;
        r[0] = 1.0;
        for (cl, cr) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
            reverb.process(cl, cr);
        }
        (l, r)
    }

    fn energy(x: &[f32]) -> f32 {
        x.iter().map(|s| s * s).sum()
    }

    #[test]
    fn every_space_is_stable_stereo_and_follows_the_decay() {
        for (kind, name) in TYPES.iter().enumerate() {
            let ring = |decay: f32| {
                let mut reverb = reverb(&[(TYPE, kind as f32), (DECAY, decay)]);
                let (l, r) = impulse(&mut reverb, 3.0);
                assert!(
                    l.iter().chain(&r).all(|s| s.is_finite() && s.abs() < 4.0),
                    "{name}"
                );
                let different: f32 = l.iter().zip(&r).map(|(a, b)| (a - b).abs()).sum();
                assert!(different > 0.5, "{name} is stereo");
                energy(&l[72_000..])
            };
            let (short, long) = (ring(0.5), ring(8.0));
            assert!(long > short * 100.0, "{name}: {short} vs {long}");
        }
    }

    #[test]
    fn plate_decay_matches_the_seconds() {
        for seconds in [0.5, 2.0, 8.0] {
            let (rt, _) = plate::decay_seconds(plate::decay_percent(seconds), 20_000.0);
            assert!(
                (rt - seconds).abs() < seconds * 0.05,
                "{seconds} s gives {rt} s"
            );
        }
    }

    #[test]
    fn freeze_holds_the_tail_in_every_engine() {
        for kind in [HALL, PLATE, SPRING, SHIMMER].map(|k| k as f32) {
            let mut reverb = reverb(&[(TYPE, kind), (DECAY, 1.0)]);
            let (mut l, mut r) = (vec![0.0f32; 4800], vec![0.0f32; 4800]);
            l[0] = 1.0;
            r[0] = 1.0;
            reverb.process(&mut l, &mut r);
            reverb.set_param(FREEZE, 1.0);
            let mut last = 0.0;
            for _ in 0..60 {
                let (mut l, mut r) = (vec![0.0f32; 4800], vec![0.0f32; 4800]);
                reverb.process(&mut l, &mut r);
                last = energy(&l);
            }
            assert!(last > 1e-4 && last.is_finite(), "{kind}: {last}");
        }
    }

    #[test]
    fn shimmer_rises_an_octave_and_stays_bounded() {
        let octave = |amount: f32| {
            let mut reverb = reverb(&[(TYPE, SHIMMER as f32), (DECAY, 8.0), (AMOUNT, amount)]);
            let n = 48_000 * 6;
            let (mut l, mut r): (Vec<f32>, Vec<f32>) = (0..n)
                .map(|i| {
                    let s = if i < 24_000 {
                        0.3 * (i as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin()
                    } else {
                        0.0
                    };
                    (s, s)
                })
                .unzip();
            for (cl, cr) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
                reverb.process(cl, cr);
            }
            assert!(l.iter().all(|s| s.is_finite() && s.abs() < 4.0), "{amount}");
            let tail = &l[96_000..144_000];
            let level = |f: f32| {
                let (mut re, mut im) = (0.0f32, 0.0f32);
                for (i, s) in tail.iter().enumerate() {
                    let a = i as f32 * f * std::f32::consts::TAU / 48_000.0;
                    re += s * a.cos();
                    im += s * a.sin();
                }
                (re * re + im * im).sqrt()
            };
            level(880.0) / level(440.0)
        };
        let (plain, shimmering) = (octave(0.0), octave(100.0));
        assert!(shimmering > plain * 3.0, "{plain} vs {shimmering}");

        let mut reverb = reverb(&[(TYPE, SHIMMER as f32), (DECAY, 20.0), (AMOUNT, 100.0)]);
        let mut seed = 1u32;
        let mut peak = 0.0f32;
        for block in 0..(48_000 * 20 / 256) {
            let (mut l, mut r): (Vec<f32>, Vec<f32>) = (0..256)
                .map(|_| {
                    seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    let s = if block < 400 {
                        (seed >> 9) as f32 / 4_194_304.0 - 1.0
                    } else {
                        0.0
                    };
                    (0.5 * s, 0.5 * s)
                })
                .unzip();
            reverb.process(&mut l, &mut r);
            peak = l.iter().fold(peak, |m, s| m.max(s.abs()));
        }
        assert!(
            peak.is_finite() && peak < 4.0,
            "longest shimmer stays bounded: {peak}"
        );
    }

    #[test]
    fn ducking_lowers_the_tail_while_the_input_plays() {
        let tail = |duck: f32| {
            let mut reverb = reverb(&[(MIX, 50.0), (DUCK, duck)]);
            let (mut l, mut r) = (vec![0.5f32; 48_000], vec![0.5f32; 48_000]);
            reverb.process(&mut l, &mut r);
            // The dry half is a constant 0.25: what moves around it is the tail.
            energy(&l[24_000..].iter().map(|s| s - 0.25).collect::<Vec<_>>())
        };
        assert!(tail(100.0) < tail(0.0) * 0.01);
    }

    #[test]
    fn dry_passes_untouched_at_zero_mix() {
        let mut reverb = reverb(&[(MIX, 0.0)]);
        let input: Vec<f32> = (0..1000).map(|i| (i as f32 * 0.01).sin()).collect();
        let (mut l, mut r) = (input.clone(), input.clone());
        reverb.process(&mut l, &mut r);
        assert_eq!(l, input);
    }

    #[test]
    fn editor_helpers_follow_the_engines() {
        assert!(uses(ROOM, "size") && uses(SPACE, "bass") && !uses(PLATE, "early"));
        assert!(uses(PLATE, "modulation") && !uses(SPRING, "modulation"));
        assert!(uses(SHIMMER, "shimmer") && !uses(HALL, "shimmer") && uses(SPRING, "decay"));
        assert_eq!(decay_seconds(HALL, 2.0, 150.0, 50.0), (3.0, 1.0));
        assert_eq!(decay_seconds(SPRING, 2.0, 100.0, 100.0), (2.0, 2.0));
        let (lows, highs) = decay_seconds(PLATE, 2.0, 100.0, 10.0);
        assert!(lows == 2.0 && highs < 1.0, "a dark plate: {highs}");
        assert!(format!("{:?}", Reverb::default()).starts_with("Reverb"));
    }

    #[test]
    fn reset_silences_every_engine_and_out_of_range_params_are_ignored() {
        for kind in [HALL, PLATE, SPRING, SHIMMER] {
            let mut reverb = reverb(&[(TYPE, kind as f32), (DECAY, 8.0)]);
            reverb.set_param(99, 1.0);
            impulse(&mut reverb, 0.2);
            reverb.reset();
            let (mut l, mut r) = (vec![0.0f32; 4800], vec![0.0f32; 4800]);
            reverb.process(&mut l, &mut r);
            assert_eq!(energy(&l) + energy(&r), 0.0, "{kind}");
        }
    }

    #[test]
    fn synced_pre_delay_follows_the_tempo() {
        let onset = |bpm: f64| {
            let mut r = Reverb::default();
            r.set_param(MIX, 100.0);
            r.set_param(TYPE, PLATE as f32);
            r.set_param(PREDELAY_SYNC, 1.0);
            // A sixteenth note.
            r.set_param(PREDELAY_NOTE, 2.0);
            r.set_tempo(bpm);
            r.prepare(48_000, 256);
            let (l, _) = impulse(&mut r, 0.5);
            l.iter().position(|s| s.abs() > 1e-4).unwrap_or(0)
        };
        let (fast, slow) = (onset(120.0), onset(80.0));
        assert!((6000..6600).contains(&fast), "125 ms at 120 BPM: {fast}");
        assert!((9000..9600).contains(&slow), "187.5 ms at 80 BPM: {slow}");
    }

    #[test]
    fn modulation_rate_and_crossover_stay_stable() {
        for (rate, crossover) in [(0.1, 80.0), (5.0, 1000.0)] {
            let mut r = reverb(&[
                (MODULATION, 100.0),
                (MOD_RATE, rate),
                (CROSSOVER, crossover),
                (DECAY, 8.0),
            ]);
            let (l, r_) = impulse(&mut r, 3.0);
            assert!(l.iter().chain(&r_).all(|s| s.is_finite() && s.abs() < 2.0));
            assert!(energy(&l[96_000..]) > 0.0);
        }
    }
}
