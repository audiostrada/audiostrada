//! Drum machine in the spirit of the 808: twelve analog-style voices synthesized from sines,
//! squares and noise, on the General MIDI drum notes. Each voice has its level, tune, decay and
//! tone; the closed hat chokes the open one.

use crate::{Effect, EffectInfo, MidiEvent, ParamInfo, db_to_gain, param};

/// Voices: identifier, name, and the MIDI note of its pad.
pub const VOICES: [(&str, &str, u8); 12] = [
    ("kick", "Kick", 36),
    ("rim", "Rim shot", 37),
    ("snare", "Snare", 38),
    ("clap", "Clap", 39),
    ("closed", "Closed hat", 42),
    ("open", "Open hat", 46),
    ("low_tom", "Low tom", 45),
    ("mid_tom", "Mid tom", 47),
    ("high_tom", "High tom", 50),
    ("cymbal", "Cymbal", 49),
    ("cowbell", "Cowbell", 56),
    ("clave", "Clave", 75),
];

const KICK: usize = 0;
const RIM: usize = 1;
const SNARE: usize = 2;
const CLAP: usize = 3;
const CLOSED: usize = 4;
const OPEN: usize = 5;
const LOW_TOM: usize = 6;
const MID_TOM: usize = 7;
const HIGH_TOM: usize = 8;
const CYMBAL: usize = 9;
const COWBELL: usize = 10;
const CLAVE: usize = 11;

/// Parameters per voice, in this order: level, tune, decay, tone.
pub const PER_VOICE: usize = 4;
const GLOBALS: usize = 3;

macro_rules! drum_params {
    ($($id:literal, $name:literal;)*) => {
        [
            $(
                param(concat!($id, "_level"), concat!($name, " level"), 0.0, 100.0, 80.0, "%"),
                param(concat!($id, "_tune"), concat!($name, " tune"), -12.0, 12.0, 0.0, "st"),
                param(concat!($id, "_decay"), concat!($name, " decay"), 10.0, 400.0, 100.0, "%"),
                param(concat!($id, "_tone"), concat!($name, " tone"), 0.0, 100.0, 50.0, "%"),
            )*
            param("accent", "Accent", 0.0, 100.0, 60.0, "%"),
            param("drive", "Drive", 0.0, 100.0, 0.0, "%"),
            param("volume", "Volume", -48.0, 6.0, -6.0, "dB"),
        ]
    };
}

const PARAMS: [ParamInfo; VOICES.len() * PER_VOICE + GLOBALS] = drum_params! {
    "kick", "Kick";
    "rim", "Rim shot";
    "snare", "Snare";
    "clap", "Clap";
    "closed", "Closed hat";
    "open", "Open hat";
    "low_tom", "Low tom";
    "mid_tom", "Mid tom";
    "high_tom", "High tom";
    "cymbal", "Cymbal";
    "cowbell", "Cowbell";
    "clave", "Clave";
};

pub const INFO: EffectInfo = EffectInfo {
    id: "drums",
    name: "Drum Machine",
    params: &PARAMS,
    meters: &[""; VOICES.len()],
    instrument: true,
    files: &[],
    category: "Instrument",
    description: "Analog drum machine in the spirit of the 808, on the General MIDI drum notes.",
};

/// The voice a MIDI note plays: its pad, or the General MIDI neighbors of the same drum.
pub fn voice_of(key: u8) -> Option<usize> {
    Some(match key {
        35 | 36 => KICK,
        37 => RIM,
        38 | 40 => SNARE,
        39 => CLAP,
        42 | 44 => CLOSED,
        46 => OPEN,
        41 | 43 | 45 => LOW_TOM,
        47 | 48 => MID_TOM,
        50 => HIGH_TOM,
        49 | 51 | 52 | 55 | 57 | 59 => CYMBAL,
        56 => COWBELL,
        75..=77 => CLAVE,
        _ => return None,
    })
}

/// Level of each voice at the same setting, so that a kit sounds balanced from the start.
const TRIM: [f32; 12] = [
    2.6, 2.2, 1.5, 0.9, 1.3, 1.3, 1.6, 1.6, 1.6, 1.15, 0.43, 1.55,
];

/// Frequencies of the six square oscillators behind the hats and the cymbal.
const METAL: [f32; 6] = [205.3, 304.4, 369.6, 522.7, 540.0, 800.0];

/// Trapezoidal state-variable filter (Simper), with coefficients set once per hit.
#[derive(Clone, Copy, Debug, Default)]
struct Filter {
    a1: f32,
    a2: f32,
    a3: f32,
    k: f32,
    ic1: f32,
    ic2: f32,
}

impl Filter {
    fn set(&mut self, hz: f32, q: f32, rate: f32) {
        let g = (std::f32::consts::PI * hz.min(rate * 0.45) / rate).tan();
        self.k = 1.0 / q;
        self.a1 = 1.0 / (1.0 + g * (g + self.k));
        self.a2 = g * self.a1;
        self.a3 = g * self.a2;
    }

    /// Low-pass, band-pass and high-pass outputs.
    fn run(&mut self, x: f32) -> (f32, f32, f32) {
        let v3 = x - self.ic2;
        let v1 = self.a1 * self.ic1 + self.a2 * v3;
        let v2 = self.ic2 + self.a2 * self.ic1 + self.a3 * v3;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;
        (v2, v1, x - self.k * v1 - v2)
    }
}

/// One sounding drum.
#[derive(Clone, Copy, Debug, Default)]
struct Voice {
    active: bool,
    /// Samples since the hit.
    time: u32,
    gain: f32,
    phase: [f32; 6],
    /// Per-sample decay multipliers of the body and of the noise (or second part).
    body: f32,
    noise: f32,
    body_level: f32,
    noise_level: f32,
    frequency: f32,
    filter: Filter,
    filter2: Filter,
}

#[derive(Debug)]
pub struct Drums {
    rate: f32,
    values: [f32; PARAMS.len()],
    voices: [Voice; VOICES.len()],
    seed: u32,
}

impl Default for Drums {
    fn default() -> Self {
        Self {
            rate: 48_000.0,
            values: PARAMS.map(|p| p.default),
            voices: [Voice::default(); VOICES.len()],
            seed: 0x1234_5678,
        }
    }
}

/// Per-sample multiplier of an exponential decay lasting `seconds` (to about -60 dB).
fn falloff(seconds: f32, rate: f32) -> f32 {
    (-6.9 / (seconds.max(0.001) * rate)).exp()
}

fn square(phase: f32) -> f32 {
    if phase < 0.5 { 1.0 } else { -1.0 }
}

impl Drums {
    fn value(&self, voice: usize, field: usize) -> f32 {
        self.values[voice * PER_VOICE + field]
    }

    fn hit(&mut self, index: usize, velocity: u8) {
        if index == CLOSED {
            // The closed hat chokes the open one, as on the original.
            self.voices[OPEN].active = false;
        }
        let rate = self.rate;
        let accent = self.values[VOICES.len() * PER_VOICE] / 100.0;
        let level = self.value(index, 0) / 100.0;
        let ratio = 2f32.powf(self.value(index, 1) / 12.0);
        let length = self.value(index, 2) / 100.0;
        let tone = self.value(index, 3) / 100.0;
        let velocity = 1.0 - accent + accent * f32::from(velocity) / 127.0;
        let v = &mut self.voices[index];
        *v = Voice {
            active: true,
            gain: level * level * velocity * 1.4 * TRIM[index],
            ..Voice::default()
        };
        // (body seconds, noise seconds, body level, noise level, frequency)
        let (body, noise, body_level, noise_level, frequency) = match index {
            KICK => (0.5, 0.004, 1.0, 0.3 * tone, 50.0),
            RIM => (0.012, 0.006, 0.8, 0.4, 1700.0),
            SNARE => (0.12, 0.2, 1.0 - 0.6 * tone, 0.4 + 0.6 * tone, 180.0),
            CLAP => (0.0, 0.18, 0.0, 1.0, 1.0),
            CLOSED => (0.05, 0.0, 1.0, 0.0, 1.0),
            OPEN => (0.4, 0.0, 1.0, 0.0, 1.0),
            LOW_TOM | MID_TOM | HIGH_TOM => (
                0.3,
                0.05,
                1.0,
                0.25 * tone,
                [90.0, 130.0, 180.0][index - LOW_TOM],
            ),
            CYMBAL => (1.4, 0.0, 1.0, 0.0, 1.0),
            COWBELL => (0.25, 0.03, 0.7, 0.5, 1.0),
            _ => (0.03, 0.0, 1.0, 0.0, 2500.0),
        };
        v.body = falloff(body * length, rate);
        v.noise = falloff(noise * length.sqrt(), rate);
        v.body_level = body_level;
        v.noise_level = noise_level;
        v.frequency = frequency * ratio;
        match index {
            SNARE => v.filter.set(1500.0 + 4000.0 * tone, 0.7, rate),
            CLAP => v.filter.set((700.0 + 1600.0 * tone) * ratio, 2.0, rate),
            CLOSED | OPEN => v.filter.set((6000.0 + 5000.0 * tone) * ratio, 1.2, rate),
            CYMBAL => v.filter.set((4000.0 + 5000.0 * tone) * ratio, 0.9, rate),
            COWBELL => {
                v.filter.set(2600.0 * ratio, 3.0, rate);
                v.filter2.set(800.0 + 6000.0 * tone, 0.7, rate);
            }
            RIM | CLAVE => v.filter.set(500.0 + 3000.0 * tone, 0.7, rate),
            _ => v.filter.set(3000.0 + 6000.0 * tone, 0.7, rate),
        }
    }

    fn noise(&mut self) -> f32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        self.seed as f32 / u32::MAX as f32 * 2.0 - 1.0
    }

    /// The next sample of a voice.
    fn render(&mut self, index: usize) -> f32 {
        let rate = self.rate;
        let white = self.noise();
        let v = &mut self.voices[index];
        let t = v.time as f32 / rate;
        v.time = v.time.saturating_add(1);
        let tau = std::f32::consts::TAU;
        let (body, noise) = (v.body_level, v.noise_level);
        let out = match index {
            KICK => {
                // The pitch falls from about three times the note in a few tens of ms.
                let f = v.frequency * (1.0 + 2.0 * (-t / 0.025).exp());
                v.phase[0] = (v.phase[0] + f / rate).fract();
                (tau * v.phase[0]).sin() * body + white * noise
            }
            SNARE => {
                let mut drum = 0.0;
                for (k, ratio) in [1.0, 1.83].into_iter().enumerate() {
                    v.phase[k] = (v.phase[k] + v.frequency * ratio / rate).fract();
                    drum += (tau * v.phase[k]).sin() * 0.5;
                }
                let (_, _, high) = v.filter.run(white);
                drum * body + high * noise
            }
            CLAP => {
                // Three quick bursts, then the tail.
                let burst = (t / 0.011) as u32;
                let envelope = if burst < 3 {
                    (-(t - burst as f32 * 0.011) / 0.003).exp()
                } else {
                    noise * 0.6
                };
                let (_, band, _) = v.filter.run(white);
                band * envelope * 2.5
            }
            CLOSED | OPEN | CYMBAL => {
                let mut metal = 0.0;
                for (phase, f) in v.phase.iter_mut().zip(METAL) {
                    *phase = (*phase + f * v.frequency / rate).fract();
                    metal += square(*phase);
                }
                let (_, _, high) = v.filter.run(metal / 6.0 + 0.15 * white);
                high * 1.5 * body
            }
            LOW_TOM | MID_TOM | HIGH_TOM => {
                let f = v.frequency * (1.0 + 0.35 * (-t / 0.06).exp());
                v.phase[0] = (v.phase[0] + f / rate).fract();
                let (low, _, _) = v.filter.run(white);
                (tau * v.phase[0]).sin() * body + low * noise
            }
            COWBELL => {
                let mut ring = 0.0;
                for (k, f) in [540.0, 800.0].into_iter().enumerate() {
                    v.phase[k] = (v.phase[k] + f * v.frequency / rate).fract();
                    ring += square(v.phase[k]);
                }
                let (_, band, _) = v.filter.run(ring * 0.5);
                let (low, _, _) = v.filter2.run(band);
                // A sharp first drop, then the ring.
                low * (body + noise) * 2.0
            }
            _ => {
                // Rim shot and clave: short tuned pings through a high-pass.
                let mut ping = 0.0;
                for (k, ratio) in [1.0, 0.29].into_iter().enumerate() {
                    v.phase[k] = (v.phase[k] + v.frequency * ratio / rate).fract();
                    ping += (tau * v.phase[k]).sin();
                }
                let (_, _, high) = v.filter.run(ping * body + white * noise);
                high
            }
        };
        v.body_level *= v.body;
        v.noise_level *= v.noise;
        let bursting = index == CLAP && t < 0.04;
        if (!bursting && v.body_level.max(v.noise_level) < 1e-4) || !out.is_finite() {
            v.active = false;
            return 0.0;
        }
        out * v.gain
    }
}

impl Effect for Drums {
    fn prepare(&mut self, sample_rate: u32, _max_frames: usize) {
        self.rate = sample_rate as f32;
        self.voices = [Voice::default(); VOICES.len()];
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if let Some(v) = self.values.get_mut(index) {
            *v = value;
        }
    }

    fn midi(&mut self, event: &MidiEvent) {
        if let Some((key, velocity)) = event.as_note_on()
            && let Some(index) = voice_of(key)
        {
            self.hit(index, velocity);
        } else if event
            .as_control()
            .is_some_and(|(cc, _)| cc == 120 || cc == 123)
        {
            self.reset();
        }
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let globals = VOICES.len() * PER_VOICE;
        let drive = self.values[globals + 1] / 100.0;
        let volume = db_to_gain(self.values[globals + 2]);
        let push = 1.0 + 6.0 * drive;
        // Keeps the level about the same as the drive rises.
        let makeup = 1.0 / push.tanh().max(0.2) / (1.0 + drive);
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let mut mix = 0.0;
            for index in 0..VOICES.len() {
                if self.voices[index].active {
                    mix += self.render(index);
                }
            }
            if drive > 0.0 {
                mix = (mix * push).tanh() * makeup;
            }
            let out = mix * volume * 0.5;
            *l = out;
            *r = out;
        }
    }

    fn reset(&mut self) {
        for v in &mut self.voices {
            v.active = false;
        }
    }

    fn meters(&self, out: &mut [f32]) {
        for (o, v) in out.iter_mut().zip(&self.voices) {
            *o = if v.active {
                (v.body_level.max(v.noise_level) * v.gain).min(1.0)
            } else {
                0.0
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn play(drums: &mut Drums, key: u8, blocks: usize) -> Vec<f32> {
        drums.midi(&MidiEvent::note_on(0, 9, key, 120));
        let mut out = Vec::new();
        for _ in 0..blocks {
            let (mut l, mut r) = (vec![0.0; 480], vec![0.0; 480]);
            drums.process(&mut l, &mut r);
            out.extend(l);
        }
        out
    }

    fn peak(samples: &[f32]) -> f32 {
        samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
    }

    #[test]
    fn every_pad_sounds_then_dies_out() {
        for (_, name, key) in VOICES {
            let mut drums = Drums::default();
            drums.prepare(48_000, 480);
            let out = play(&mut drums, key, 400);
            assert!(out.iter().all(|s| s.is_finite()), "{name}");
            assert!(peak(&out[..4800]) > 0.02, "{name} is heard");
            assert!(peak(&out[out.len() - 480..]) < 1e-3, "{name} ends");
            assert!(drums.voices.iter().all(|v| !v.active), "{name} stops");
        }
    }

    #[test]
    fn the_closed_hat_chokes_the_open_one() {
        let mut drums = Drums::default();
        drums.prepare(48_000, 480);
        play(&mut drums, 46, 2);
        assert!(drums.voices[OPEN].active);
        play(&mut drums, 42, 1);
        assert!(!drums.voices[OPEN].active);
    }

    #[test]
    fn the_kick_follows_tune_and_decay() {
        let length = |tune: f32, decay: f32| {
            let mut drums = Drums::default();
            drums.prepare(48_000, 480);
            drums.set_param(1, tune);
            drums.set_param(2, decay);
            let out = play(&mut drums, 36, 300);
            out.iter().rposition(|s| s.abs() > 1e-3).unwrap_or(0)
        };
        assert!(length(0.0, 300.0) > 2 * length(0.0, 100.0));
        // Zero crossings late in the hit: the tuned body.
        let crossings = |tune: f32| {
            let mut drums = Drums::default();
            drums.prepare(48_000, 480);
            drums.set_param(1, tune);
            let out = play(&mut drums, 36, 40);
            out[9600..19200]
                .windows(2)
                .filter(|w| w[0] < 0.0 && w[1] >= 0.0)
                .count()
        };
        let (low, high) = (crossings(0.0), crossings(12.0));
        assert!((high as f32 / low as f32 - 2.0).abs() < 0.2, "{low} {high}");
    }

    #[test]
    fn general_midi_neighbors_play_the_same_drum() {
        assert_eq!(voice_of(35), voice_of(36));
        assert_eq!(voice_of(40), voice_of(38));
        assert_eq!(voice_of(44), voice_of(42));
        assert_eq!(voice_of(60), None);
        for (index, (_, _, key)) in VOICES.iter().enumerate() {
            assert_eq!(voice_of(*key), Some(index));
        }
    }

    #[test]
    fn accent_follows_velocity() {
        let level = |velocity: u8| {
            let mut drums = Drums::default();
            drums.prepare(48_000, 480);
            drums.midi(&MidiEvent::note_on(0, 9, 38, velocity));
            let (mut l, mut r) = (vec![0.0; 4800], vec![0.0; 4800]);
            drums.process(&mut l, &mut r);
            peak(&l)
        };
        assert!(level(127) > 1.5 * level(30));
    }
}
