//! Tape machine: four machines (a 2-inch studio deck, a half-inch mastering deck, a vintage
//! four-track and a cassette), three tape formulations and three speeds. Saturation with
//! memory (soft hysteresis) and bias, highs that fade when the level rises (self-erasure), the
//! head bump with its dip below, the speed's high-frequency loss, each machine's wow, flutter
//! and hiss.
//!
//! Subtle by default: the tape has headroom above full scale, the input changes the color and
//! not the level, and the highs are emphasized before the tape and restored after it, as on a
//! real machine, so they saturate first while the lows stay clear. 0 VU is -18 dBFS.

use crate::biquad::{Biquad, Coefficients, FilterKind};
use crate::drive::{Delay, LATENCY, Oversampler, Taps};
use crate::line::Line;
use crate::{Effect, EffectInfo, ParamInfo, choice, db_to_gain, gain_to_db, param};

pub const INFO: EffectInfo = EffectInfo {
    id: "tape",
    name: "Strada-Tape",
    params: &PARAMS,
    meters: &["Input", "Output"],
    instrument: false,
    files: &[],
    category: "Color",
    description: "Tape machine: four machines, three tapes, warm saturation, head bump, wow and flutter, hiss.",
};

pub const SPEEDS: [&str; 3] = ["7.5 ips", "15 ips", "30 ips"];
pub const MACHINES: [&str; 4] = ["Studio A80", "Mastering 102", "Vintage J37", "Cassette"];
pub const MACHINE_HINTS: [&str; 4] = [
    "2-inch studio deck: punchy lows, firm mids, the sound of countless records",
    "Half-inch mastering deck: wide, clean and smooth, the lightest touch",
    "Vintage four-track: darker, rounder, saturates early, wobbles a little",
    "Cassette deck: soft top, warble and hiss, for lo-fi color",
];
pub const TAPES: [&str; 3] = ["GP9", "456", "250"];
pub const TAPE_HINTS: [&str; 3] = [
    "Modern high-output tape: most headroom, cleanest",
    "The classic studio tape: compresses and warms when pushed",
    "Vintage tape: saturates early, softer and rounder",
];

pub mod p {
    pub const DRIVE: usize = 0;
    pub const SPEED: usize = 1;
    pub const BUMP: usize = 2;
    pub const WOW: usize = 3;
    pub const HISS: usize = 4;
    pub const BIAS: usize = 5;
    pub const OUTPUT: usize = 6;
    pub const MIX: usize = 7;
    pub const MACHINE: usize = 8;
    pub const TAPE: usize = 9;
}
const COUNT: usize = 10;

const PARAMS: [ParamInfo; COUNT] = [
    param("drive", "Input", -12.0, 24.0, 0.0, "dB"),
    choice("speed", "Speed", &SPEEDS, 1.0),
    param("bump", "Head bump", 0.0, 6.0, 1.5, "dB"),
    param("wow", "Wow & flutter", 0.0, 100.0, 0.0, "%"),
    param("hiss", "Hiss", 0.0, 100.0, 0.0, "%"),
    param("bias", "Bias", -100.0, 100.0, 0.0, "%"),
    param("output", "Output", -18.0, 12.0, 0.0, "dB"),
    param("mix", "Mix", 0.0, 100.0, 100.0, "%"),
    choice("machine", "Machine", &MACHINES, 0.0),
    choice("tape", "Tape", &TAPES, 1.0),
];

/// What a machine sounds like at each speed.
struct Machine {
    /// Head bump frequency (Hz) per speed, and how strong it is.
    bump: [f32; 3],
    bump_scale: f32,
    /// High-frequency loss (Hz) per speed.
    rolloff: [f32; 3],
    /// Where the tape bends, above full scale, and how much it remembers.
    headroom: f32,
    hysteresis: f32,
    /// How much loud highs erase themselves.
    erasure: f32,
    /// Wow and flutter: rates (Hz) and depths (ms at 100 %).
    wow: (f32, f32),
    flutter: (f32, f32),
    /// Hiss at 100 % (dB) and its brightness (0 dark, 1 bright).
    hiss: (f32, f32),
    emphasis: f64,
}

const MACHINE_SHAPES: [Machine; 4] = [
    Machine {
        bump: [35.0, 55.0, 100.0],
        bump_scale: 1.0,
        rolloff: [11_000.0, 18_000.0, 22_000.0],
        headroom: 1.6,
        hysteresis: 0.05,
        erasure: 0.5,
        wow: (0.55, 4.0),
        flutter: (6.5, 0.8),
        hiss: (-34.0, 0.6),
        emphasis: 6.0,
    },
    Machine {
        bump: [30.0, 45.0, 80.0],
        bump_scale: 0.7,
        rolloff: [13_000.0, 20_000.0, 23_000.0],
        headroom: 2.0,
        hysteresis: 0.03,
        erasure: 0.3,
        wow: (0.4, 2.5),
        flutter: (8.0, 0.5),
        hiss: (-40.0, 0.7),
        emphasis: 5.0,
    },
    Machine {
        bump: [45.0, 70.0, 110.0],
        bump_scale: 1.25,
        rolloff: [8_000.0, 12_500.0, 15_000.0],
        headroom: 1.2,
        hysteresis: 0.08,
        erasure: 0.8,
        wow: (0.7, 6.0),
        flutter: (5.5, 1.4),
        hiss: (-30.0, 0.45),
        emphasis: 7.0,
    },
    Machine {
        bump: [70.0, 80.0, 90.0],
        bump_scale: 0.8,
        rolloff: [7_000.0, 9_500.0, 12_000.0],
        headroom: 0.85,
        hysteresis: 0.1,
        erasure: 1.2,
        wow: (1.1, 9.0),
        flutter: (12.0, 2.0),
        hiss: (-24.0, 0.85),
        emphasis: 8.0,
    },
];

/// Each tape: its headroom and memory relative to the machine's.
const TAPE_SHAPES: [(f32, f32); 3] = [(1.35, 0.7), (1.0, 1.0), (0.75, 1.4)];
const MAX_WOW_MS: f32 = 12.0;
const EMPHASIS_HZ: f64 = 3_000.0;
/// 0 VU, as RMS.
const REFERENCE: f32 = 0.125_9;

#[derive(Debug)]
pub struct Tape {
    rate: f32,
    values: [f32; COUNT],
    stages: [Oversampler; 2],
    /// The dry signal, delayed as much as the taped one without wow.
    dry: [Delay; 2],
    /// Last output of the saturator per channel, its memory.
    memory: [f32; 2],
    bump: Coefficients,
    dip: Coefficients,
    rolloff: Coefficients,
    emphasis: Coefficients,
    deemphasis: Coefficients,
    filters: [[Biquad; 5]; 2],
    /// Self-erasure: the highs' envelope, a high-pass state and the dynamic low pass.
    erase_env: [f32; 2],
    erase_hp: [f32; 2],
    erase_lp: [f32; 2],
    wow: [Line; 2],
    phase: [f32; 3],
    drift: f32,
    noise: u32,
    hiss_lp: [f32; 2],
    levels: [f32; 2],
}

impl Default for Tape {
    fn default() -> Self {
        let mut tape = Self {
            rate: 48_000.0,
            values: PARAMS.map(|p| p.default),
            stages: [Oversampler::new(Taps::Long), Oversampler::new(Taps::Long)],
            dry: [Delay::default(); 2],
            memory: [0.0; 2],
            bump: Coefficients::IDENTITY,
            dip: Coefficients::IDENTITY,
            rolloff: Coefficients::IDENTITY,
            emphasis: Coefficients::IDENTITY,
            deemphasis: Coefficients::IDENTITY,
            filters: [[Biquad::default(); 5]; 2],
            erase_env: [0.0; 2],
            erase_hp: [0.0; 2],
            erase_lp: [0.0; 2],
            wow: Default::default(),
            phase: [0.0, 1.7, 0.4],
            drift: 0.0,
            noise: 0x1234_5678,
            hiss_lp: [0.0; 2],
            levels: [0.0; 2],
        };
        tape.allocate();
        tape
    }
}

/// Saturation with memory: the previous output pulls the curve a little, a soft hysteresis.
/// Linear at low levels, it bends gently towards the headroom; the bias tilts it.
#[inline]
fn saturate(x: f32, memory: &mut f32, bias: f32, headroom: f32, hysteresis: f32) -> f32 {
    let v = (x + hysteresis * *memory) / headroom + bias;
    let y = (v.tanh() - bias.tanh()) * headroom;
    *memory = y;
    y
}

impl Tape {
    fn machine(&self) -> &'static Machine {
        &MACHINE_SHAPES[(self.values[p::MACHINE].round().max(0.0) as usize).min(3)]
    }

    fn speed(&self) -> usize {
        self.values[p::SPEED].round().clamp(0.0, 2.0) as usize
    }

    fn tape(&self) -> (f32, f32) {
        TAPE_SHAPES[(self.values[p::TAPE].round().max(0.0) as usize).min(2)]
    }

    fn allocate(&mut self) {
        let frames = (MAX_WOW_MS * 2.0 * 0.001 * self.rate) as usize + 8;
        self.wow = [Line::new(frames), Line::new(frames)];
        self.update();
    }

    fn update(&mut self) {
        let machine = self.machine();
        let speed = self.speed();
        let rate = f64::from(self.rate);
        let bump_hz = f64::from(machine.bump[speed]);
        let bump_db = f64::from(self.values[p::BUMP] * machine.bump_scale);
        self.bump = Coefficients::design(FilterKind::Peak, bump_hz, 0.9, bump_db, rate);
        // Below the bump, the playback head loses the lows: a dip, deeper with the bump.
        self.dip =
            Coefficients::design(FilterKind::Peak, bump_hz * 0.45, 1.1, -0.6 * bump_db, rate);
        self.rolloff = Coefficients::design(
            FilterKind::LowPass,
            f64::from(machine.rolloff[speed].min(self.rate * 0.45)),
            0.6,
            0.0,
            rate,
        );
        self.emphasis = Coefficients::design(
            FilterKind::HighShelf,
            EMPHASIS_HZ,
            0.7,
            machine.emphasis,
            rate,
        );
        self.deemphasis = Coefficients::design(
            FilterKind::HighShelf,
            EMPHASIS_HZ,
            0.7,
            -machine.emphasis,
            rate,
        );
    }

    #[inline]
    fn white(&mut self) -> f32 {
        self.noise ^= self.noise << 13;
        self.noise ^= self.noise >> 17;
        self.noise ^= self.noise << 5;
        (self.noise as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

impl Effect for Tape {
    fn prepare(&mut self, sample_rate: u32, _max_frames: usize) {
        self.rate = sample_rate as f32;
        self.allocate();
        self.reset();
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if let Some(v) = self.values.get_mut(index) {
            *v = value;
            self.update();
        }
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let machine = self.machine();
        let speed = self.speed();
        let (tape_headroom, tape_memory) = self.tape();
        let headroom = machine.headroom * tape_headroom;
        let hysteresis = machine.hysteresis * tape_memory;
        let v = self.values;
        let gain = db_to_gain(v[p::DRIVE]);
        // Level-compensated: the input changes the color, not the level.
        let makeup = db_to_gain(v[p::OUTPUT]) / gain;
        let bias = v[p::BIAS] / 100.0 * 0.3;
        // Both grow slowly at first, so small settings stay subtle and the top end goes far.
        let hiss = (v[p::HISS] / 100.0).powi(2) * db_to_gain(machine.hiss.0 - 12.0);
        let hiss_tone = machine.hiss.1;
        let amount = (v[p::WOW] / 100.0).powi(2);
        let wow_depth = amount * machine.wow.1 * 0.001 * self.rate;
        let flutter_depth = amount * machine.flutter.1 * 0.001 * self.rate;
        let base = wow_depth + flutter_depth + 2.0;
        let tau = std::f32::consts::TAU;
        let steps = [
            tau * machine.wow.0 / self.rate,
            tau * machine.flutter.0 / self.rate,
            tau * machine.wow.0 * 0.37 / self.rate,
        ];
        // Loud highs fade: more at the slower speeds.
        let erasure = machine.erasure * [1.5, 1.0, 0.6][speed];
        let rolloff = machine.rolloff[speed];
        let erase_hp = 1.0 - (-tau * 4_000.0 / self.rate).exp();
        let attack = 1.0 - (-1.0 / (0.002 * self.rate)).exp();
        let release = 1.0 - (-1.0 / (0.08 * self.rate)).exp();
        let meter = 1.0 - (-1.0 / (0.3 * self.rate)).exp();
        let wet = v[p::MIX] / 100.0;
        let aligned = self.latency() as usize;
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let dry = [*l, *r];
            for (ph, step) in self.phase.iter_mut().zip(steps) {
                *ph = (*ph + step) % tau;
            }
            // A slow random drift on top of the wow, as a real transport wanders.
            self.drift += 0.0005 * (self.white() - self.drift);
            let modulation = wow_depth
                * (0.7 * self.phase[0].sin() + 0.3 * self.phase[2].sin() + 4.0 * self.drift)
                + flutter_depth * self.phase[1].sin();
            let delay = base + modulation;
            let mut out = [0.0f32; 2];
            let mut power_in = 0.0f32;
            for (ch, o) in out.iter_mut().enumerate() {
                let x = dry[ch] * gain;
                power_in += 0.5 * x * x;
                let x = self.filters[ch][2].process(&self.emphasis, f64::from(x)) as f32;
                // Self-erasure: a low pass that closes as the highs grow louder.
                self.erase_hp[ch] += erase_hp * (x - self.erase_hp[ch]);
                let highs = (x - self.erase_hp[ch]).abs();
                let k = if highs > self.erase_env[ch] {
                    attack
                } else {
                    release
                };
                self.erase_env[ch] += k * (highs - self.erase_env[ch]);
                let closing = (erasure * self.erase_env[ch] / headroom).min(0.85);
                let cutoff = (rolloff * 1.4 * (1.0 - closing)).max(1_500.0);
                let a = 1.0 - (-tau * cutoff / self.rate).exp();
                self.erase_lp[ch] += a * (x - self.erase_lp[ch]);
                let x = self.erase_lp[ch];
                let memory = std::cell::Cell::new(self.memory[ch]);
                let y = self.stages[ch].run(x, |v| {
                    let mut m = memory.get();
                    let y = saturate(v, &mut m, bias, headroom, hysteresis);
                    memory.set(m);
                    y
                });
                self.memory[ch] = memory.get();
                let y = self.filters[ch][3].process(&self.deemphasis, f64::from(y));
                let y = self.filters[ch][0].process(&self.bump, y);
                let y = self.filters[ch][4].process(&self.dip, y);
                let y = self.filters[ch][1].process(&self.rolloff, y) as f32;
                // Wow and flutter: the signal goes through a gently modulated delay.
                self.wow[ch].push(y);
                *o = self.wow[ch].read(delay + 1.0);
            }
            if hiss > 0.0 {
                for (ch, o) in out.iter_mut().enumerate() {
                    let n = self.white();
                    self.hiss_lp[ch] += 0.35 * (n - self.hiss_lp[ch]);
                    *o += (n * hiss_tone + self.hiss_lp[ch] * (1.0 - hiss_tone)) * hiss;
                }
            }
            let power_out = 0.5 * (out[0] * out[0] + out[1] * out[1]);
            self.levels[0] += meter * (power_in - self.levels[0]);
            self.levels[1] += meter * (power_out - self.levels[1]);
            let dry = [
                self.dry[0].tick(dry[0], aligned),
                self.dry[1].tick(dry[1], aligned),
            ];
            *l = dry[0] * (1.0 - wet) + out[0] * makeup * wet;
            *r = dry[1] * (1.0 - wet) + out[1] * makeup * wet;
        }
    }

    /// The oversampling, and the 2 samples the wow line reads behind when still.
    fn latency(&self) -> u32 {
        LATENCY + 2
    }

    fn reset(&mut self) {
        for stage in &mut self.stages {
            stage.clear();
        }
        self.dry = [Delay::default(); 2];
        self.memory = [0.0; 2];
        self.filters = [[Biquad::default(); 5]; 2];
        for line in &mut self.wow {
            line.clear();
        }
        self.hiss_lp = [0.0; 2];
        self.erase_env = [0.0; 2];
        self.erase_hp = [0.0; 2];
        self.erase_lp = [0.0; 2];
        self.drift = 0.0;
        self.levels = [0.0; 2];
    }

    /// Input and output, in VU (dB from 0 VU, RMS).
    fn meters(&self, out: &mut [f32]) {
        for (o, level) in out.iter_mut().zip(self.levels) {
            *o = gain_to_db(level.max(1e-12).sqrt() / REFERENCE);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(tape: &mut Tape, input: &[f32]) -> Vec<f32> {
        let (mut l, mut r) = (input.to_vec(), input.to_vec());
        for (cl, cr) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
            tape.process(cl, cr);
        }
        l
    }

    fn sine(f: f32, a: f32) -> Vec<f32> {
        (0..48_000)
            .map(|i| a * (i as f32 * f * std::f32::consts::TAU / 48_000.0).sin())
            .collect()
    }

    fn peak(x: &[f32]) -> f32 {
        x[24_000..].iter().fold(0.0f32, |m, s| m.max(s.abs()))
    }

    /// Energy left once the input sine is removed, relative to it (distortion and noise).
    fn residue(out: &[f32], f: f32) -> f32 {
        let part = &out[24_000..];
        let (mut c, mut s) = (0.0f32, 0.0f32);
        for (i, v) in part.iter().enumerate() {
            let t = (24_000 + i) as f32 * f * std::f32::consts::TAU / 48_000.0;
            c += v * t.cos();
            s += v * t.sin();
        }
        let n = part.len() as f32;
        let (c, s) = (2.0 * c / n, 2.0 * s / n);
        let fitted: f32 = part
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let t = (24_000 + i) as f32 * f * std::f32::consts::TAU / 48_000.0;
                (v - c * t.cos() - s * t.sin()).powi(2)
            })
            .sum();
        (fitted / (c * c + s * s) / n * 2.0).sqrt()
    }

    #[test]
    fn subtle_by_default_and_strong_when_pushed() {
        let input = sine(1000.0, 0.5);
        let gentle = run(&mut Tape::default(), &input);
        let level = peak(&gentle) / 0.5;
        assert!((0.9..1.1).contains(&level), "same level: {level}");
        assert!(residue(&gentle, 1000.0) < 0.01, "clean by default");
        let mut pushed = Tape::default();
        pushed.set_param(0, 18.0);
        pushed.prepare(48_000, 256);
        let hot = run(&mut pushed, &input);
        assert!(residue(&hot, 1000.0) > 0.05, "saturated when driven");
        assert!(peak(&hot) < 1.2, "the drive keeps the level");
    }

    #[test]
    fn bumps_the_lows_and_stays_finite() {
        let mut tape = Tape::default();
        tape.set_param(2, 6.0);
        tape.prepare(48_000, 256);
        let low = run(&mut tape, &sine(85.0, 0.05));
        let mid = run(&mut Tape::default(), &sine(1000.0, 0.05));
        assert!(
            peak(&low) > peak(&mid),
            "head bump: {} vs {}",
            peak(&low),
            peak(&mid)
        );
        let mut hot = Tape::default();
        hot.set_param(0, 24.0);
        hot.set_param(4, 100.0);
        hot.set_param(3, 100.0);
        hot.prepare(48_000, 256);
        let out = run(&mut hot, &sine(200.0, 0.9));
        assert!(out.iter().all(|s| s.is_finite() && s.abs() < 2.0));
    }

    fn shaped(values: &[(usize, f32)]) -> Tape {
        let mut tape = Tape::default();
        for (i, v) in values {
            tape.set_param(*i, *v);
        }
        tape.prepare(48_000, 256);
        tape
    }

    #[test]
    fn machines_and_tapes_have_their_own_sound() {
        // The cassette loses more highs than the mastering deck.
        let highs = |machine: f32| {
            peak(&run(
                &mut shaped(&[(p::MACHINE, machine)]),
                &sine(9_000.0, 0.05),
            ))
        };
        assert!(
            highs(3.0) < highs(1.0) * 0.6,
            "{} {}",
            highs(3.0),
            highs(1.0)
        );
        // Vintage tape saturates before modern tape.
        let distortion = |tape: f32| {
            residue(
                &run(
                    &mut shaped(&[(p::TAPE, tape), (p::DRIVE, 12.0)]),
                    &sine(1_000.0, 0.5),
                ),
                1_000.0,
            )
        };
        assert!(
            distortion(2.0) > distortion(0.0) * 1.5,
            "{} {}",
            distortion(2.0),
            distortion(0.0)
        );
        // The cassette hisses more than the studio deck at the same setting.
        let noise = |machine: f32| {
            let out = run(
                &mut shaped(&[(p::MACHINE, machine), (p::HISS, 100.0)]),
                &vec![0.0; 48_000],
            );
            peak(&out)
        };
        assert!(noise(3.0) > noise(0.0) * 2.0);
    }

    #[test]
    fn loud_highs_erase_themselves() {
        // At 7.5 ips, a loud 8 kHz tone comes out relatively softer than a quiet one.
        let ratio = |level: f32| {
            let mut tape = shaped(&[(p::SPEED, 0.0), (p::MACHINE, 2.0)]);
            peak(&run(&mut tape, &sine(8_000.0, level))) / level
        };
        assert!(
            ratio(0.8) < ratio(0.02) * 0.8,
            "{} {}",
            ratio(0.8),
            ratio(0.02)
        );
    }

    #[test]
    fn the_meters_read_0_vu_at_minus_18_dbfs() {
        let mut tape = Tape::default();
        let amplitude = REFERENCE * std::f32::consts::SQRT_2;
        run(&mut tape, &sine(1_000.0, amplitude));
        let mut meters = [0.0; 2];
        tape.meters(&mut meters);
        assert!(meters[0].abs() < 0.5, "{meters:?}");
        assert!(meters[1].abs() < 1.5, "{meters:?}");
    }

    #[test]
    fn the_dry_signal_lines_up_with_the_processed_one() {
        let mut fx = Tape::default();
        fx.set_param(7, 0.0);
        fx.prepare(48_000, 256);
        let impulse = |fx: &mut Tape| {
            let (mut l, mut r) = (vec![0.0f32; 64], vec![0.0f32; 64]);
            l[0] = 0.1;
            r[0] = 0.1;
            fx.process(&mut l, &mut r);
            (0..64).max_by(|&a, &b| l[a].abs().total_cmp(&l[b].abs()))
        };
        let latency = fx.latency() as usize;
        assert_eq!(impulse(&mut fx), Some(latency), "dry");
        fx.set_param(7, 100.0);
        fx.reset();
        // Its tone filters move the peak a little.
        let wet = impulse(&mut fx).unwrap();
        assert!(
            wet.abs_diff(latency) <= 3,
            "wet at {wet}, latency {latency}"
        );
    }
}
