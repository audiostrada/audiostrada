//! Polyphonic subtractive synthesizer: 16 voices, band-limited saw/square (polyBLEP), sine or
//! triangle oscillator, amplitude envelope, resonant low-pass filter with its own envelope.
//! Pitch bend, channel pressure and (in MPE mode) CC 74 follow each voice's channel, so MPE
//! controllers bend and shape notes one by one. Out of MPE, the host maps CC 74 to the cutoff.

use crate::{Effect, EffectInfo, MidiEvent, ParamInfo, choice, db_to_gain, param, switch};

const VOICES: usize = 16;

pub const INFO: EffectInfo = EffectInfo {
    id: "synth",
    name: "Strada-Synth",
    params: &PARAMS,
    meters: &[],
    instrument: true,
    files: &[],
    category: "Instrument",
    description: "Polyphonic subtractive synthesizer, MPE-aware.",
};

const PARAMS: [ParamInfo; 13] = [
    choice(
        "wave",
        "Waveform",
        &["Saw", "Square", "Sine", "Triangle"],
        0.0,
    ),
    param("attack", "Attack", 0.5, 2000.0, 5.0, "ms"),
    param("decay", "Decay", 1.0, 4000.0, 300.0, "ms"),
    param("sustain", "Sustain", 0.0, 100.0, 70.0, "%"),
    param("release", "Release", 1.0, 8000.0, 400.0, "ms"),
    param("cutoff", "Cutoff", 40.0, 18000.0, 2500.0, "Hz"),
    param("resonance", "Resonance", 0.0, 95.0, 20.0, "%"),
    param("env_amount", "Filter envelope", 0.0, 100.0, 40.0, "%"),
    param("detune", "Detune", 0.0, 50.0, 8.0, "cents"),
    param("velocity", "Velocity sensitivity", 0.0, 100.0, 70.0, "%"),
    param("volume", "Volume", -48.0, 6.0, -12.0, "dB"),
    param("bend_range", "Pitch bend range", 1.0, 24.0, 2.0, "st"),
    switch("mpe", "MPE (channel 1 global, 48 st per note)", 0.0),
];

/// Pitch bend range of MPE member channels, in semitones.
const MPE_BEND: f32 = 48.0;

/// Expression received on one MIDI channel.
#[derive(Clone, Copy, Debug)]
struct ChannelState {
    /// -1 to 1.
    bend: f32,
    /// 0 to 1.
    pressure: f32,
    /// CC 74, 0 to 1 (0.5 when never sent).
    slide: f32,
}

impl Default for ChannelState {
    fn default() -> Self {
        Self {
            bend: 0.0,
            pressure: 0.0,
            slide: 0.5,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Stage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

#[derive(Clone, Copy, Debug)]
struct Voice {
    key: u8,
    channel: u8,
    velocity: f32,
    frequency: f32,
    phase: [f32; 2],
    stage: Stage,
    level: f32,
    /// State of the state-variable filter (its two integrators).
    ic1: f32,
    ic2: f32,
    age: u64,
}

impl Default for Voice {
    fn default() -> Self {
        Self {
            key: 0,
            channel: 0,
            velocity: 0.0,
            frequency: 440.0,
            phase: [0.0; 2],
            stage: Stage::Idle,
            level: 0.0,
            ic1: 0.0,
            ic2: 0.0,
            age: 0,
        }
    }
}

/// PolyBLEP residual for a discontinuity at phase 0 (phase and increment in cycles).
#[inline]
fn blep(t: f32, dt: f32) -> f32 {
    if t < dt {
        let x = t / dt;
        x + x - x * x - 1.0
    } else if t > 1.0 - dt {
        let x = (t - 1.0) / dt;
        x * x + x + x + 1.0
    } else {
        0.0
    }
}

#[inline]
fn oscillator(wave: u32, phase: f32, dt: f32) -> f32 {
    match wave {
        0 => 2.0 * phase - 1.0 - blep(phase, dt),
        1 => {
            let square = if phase < 0.5 { 1.0 } else { -1.0 };
            square + blep(phase, dt) - blep((phase + 0.5) % 1.0, dt)
        }
        2 => (phase * std::f32::consts::TAU).sin(),
        _ => 1.0 - 4.0 * (phase - 0.5).abs(),
    }
}

#[derive(Debug)]
pub struct Synth {
    rate: f32,
    values: [f32; 13],
    voices: [Voice; VOICES],
    channels: [ChannelState; 16],
    clock: u64,
}

impl Default for Synth {
    fn default() -> Self {
        Self {
            rate: 48_000.0,
            values: PARAMS.map(|p| p.default),
            voices: [Voice::default(); VOICES],
            channels: [ChannelState::default(); 16],
            clock: 0,
        }
    }
}

impl Synth {
    fn note_on(&mut self, channel: u8, key: u8, velocity: u8) {
        self.clock += 1;
        // Reuse the same key, else a free voice, else the oldest.
        let index = self
            .voices
            .iter()
            .position(|v| v.stage != Stage::Idle && v.key == key && v.channel == channel)
            .or_else(|| self.voices.iter().position(|v| v.stage == Stage::Idle))
            .unwrap_or_else(|| (0..VOICES).min_by_key(|&i| self.voices[i].age).unwrap_or(0));
        let sensitivity = self.values[9] / 100.0;
        let v = &mut self.voices[index];
        v.key = key;
        v.channel = channel;
        v.velocity = 1.0 - sensitivity + sensitivity * f32::from(velocity) / 127.0;
        v.frequency = 440.0 * 2f32.powf((f32::from(key) - 69.0) / 12.0);
        if v.stage == Stage::Idle {
            v.phase = [0.0, 0.37];
            v.level = 0.0;
            v.ic1 = 0.0;
            v.ic2 = 0.0;
        }
        v.stage = Stage::Attack;
        v.age = self.clock;
    }

    /// Bend of a voice in semitones: its own channel, plus the global channel in MPE mode.
    fn bend(&self, channel: u8) -> f32 {
        let state = &self.channels[usize::from(channel & 15)];
        if self.values[12] >= 0.5 {
            let global = self.channels[0].bend * self.values[11];
            if channel == 0 {
                global
            } else {
                global + state.bend * MPE_BEND
            }
        } else {
            state.bend * self.values[11]
        }
    }

    fn note_off(&mut self, channel: u8, key: u8) {
        for v in self
            .voices
            .iter_mut()
            .filter(|v| v.key == key && v.channel == channel && v.stage != Stage::Idle)
        {
            v.stage = Stage::Release;
        }
    }
}

impl Effect for Synth {
    fn prepare(&mut self, sample_rate: u32, _max_frames: usize) {
        self.rate = sample_rate as f32;
        self.voices = [Voice::default(); VOICES];
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if let Some(v) = self.values.get_mut(index) {
            *v = value;
        }
    }

    fn midi(&mut self, event: &MidiEvent) {
        if let Some((key, velocity)) = event.as_note_on() {
            self.note_on(event.channel(), key, velocity);
        } else if let Some(key) = event.as_note_off() {
            self.note_off(event.channel(), key);
        } else if event
            .as_control()
            .is_some_and(|(cc, _)| cc == 120 || cc == 123)
        {
            for v in &mut self.voices {
                v.stage = if event.data[1] == 120 {
                    Stage::Idle
                } else {
                    Stage::Release
                };
            }
        } else {
            let state = &mut self.channels[usize::from(event.channel())];
            match (event.data[0] & 0xF0, event.data[1]) {
                (0xE0, _) => {
                    let value = u16::from(event.data[2]) << 7 | u16::from(event.data[1]);
                    state.bend = ((f32::from(value) - 8192.0) / 8192.0).clamp(-1.0, 1.0);
                }
                (0xD0, pressure) => state.pressure = f32::from(pressure) / 127.0,
                (0xB0, 74) => state.slide = f32::from(event.data[2]) / 127.0,
                (0xB0, 121) => *state = ChannelState::default(),
                _ => {}
            }
        }
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let [
            wave,
            attack,
            decay,
            sustain,
            release,
            cutoff,
            resonance,
            env_amount,
            detune,
            _,
            volume,
            _,
            _,
        ] = self.values;
        let spread = 2f32.powf(detune / 1200.0);
        let wave = wave.round().clamp(0.0, 3.0) as u32;
        let per_ms = 1000.0 / self.rate;
        let (a, d, r) = (per_ms / attack, per_ms / decay, per_ms / release);
        let sustain = sustain / 100.0;
        let damping = 2.0 * (1.0 - resonance / 100.0).max(0.05);
        let gain = db_to_gain(volume) * 0.5;
        left.fill(0.0);
        right.fill(0.0);
        let bends: [f32; 16] = std::array::from_fn(|c| self.bend(c as u8));
        let mpe = self.values[12] >= 0.5;
        for v in self.voices.iter_mut().filter(|v| v.stage != Stage::Idle) {
            let channel = &self.channels[usize::from(v.channel & 15)];
            let frequency = v.frequency * 2f32.powf(bends[usize::from(v.channel & 15)] / 12.0);
            let increment = [
                frequency * spread / self.rate,
                frequency / spread / self.rate,
            ];
            // Pressure and slide open the filter, up to two octaves each way for the slide.
            let slide = if mpe { channel.slide } else { 0.5 };
            let brightness = (1.0 + 3.0 * channel.pressure) * 2f32.powf((slide - 0.5) * 4.0);
            for (l, rr) in left.iter_mut().zip(right.iter_mut()) {
                match v.stage {
                    Stage::Attack => {
                        v.level += a;
                        if v.level >= 1.0 {
                            v.level = 1.0;
                            v.stage = Stage::Decay;
                        }
                    }
                    Stage::Decay => {
                        v.level -= d * (1.0 - sustain).max(0.001);
                        if v.level <= sustain {
                            v.level = sustain;
                            v.stage = Stage::Sustain;
                        }
                    }
                    Stage::Release => {
                        v.level -= r;
                        if v.level <= 0.0 {
                            v.level = 0.0;
                            v.stage = Stage::Idle;
                        }
                    }
                    Stage::Sustain | Stage::Idle => {}
                }
                let osc = 0.5
                    * (oscillator(wave, v.phase[0], increment[0])
                        + oscillator(wave, v.phase[1], increment[1]));
                for (p, inc) in v.phase.iter_mut().zip(&increment) {
                    *p += inc;
                    if *p >= 1.0 {
                        *p -= 1.0;
                    }
                }
                let fc = (cutoff * brightness * (1.0 + env_amount / 100.0 * 4.0 * v.level))
                    .min(self.rate * 0.45);
                // Trapezoidal state-variable filter (Simper), stable at any cutoff; low-pass.
                let g = (std::f32::consts::PI * fc / self.rate).tan();
                let a1 = 1.0 / (1.0 + g * (g + damping));
                let (a2, v3) = (g * a1, osc - v.ic2);
                let v1 = a1 * v.ic1 + a2 * v3;
                let v2 = v.ic2 + a2 * v.ic1 + g * a2 * v3;
                v.ic1 = 2.0 * v1 - v.ic1;
                v.ic2 = 2.0 * v2 - v.ic2;
                let mut out = v2 * v.level * v.velocity * gain;
                if !out.is_finite() {
                    // Never let a broken value reach the mix.
                    (v.ic1, v.ic2, out) = (0.0, 0.0, 0.0);
                }
                *l += out;
                *rr += out;
                if v.stage == Stage::Idle {
                    break;
                }
            }
        }
    }

    fn reset(&mut self) {
        for v in &mut self.voices {
            v.stage = Stage::Idle;
        }
        self.channels = [ChannelState::default(); 16];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(synth: &mut Synth, blocks: usize) -> Vec<f32> {
        let mut out = Vec::new();
        for _ in 0..blocks {
            let (mut l, mut r) = (vec![0.0; 256], vec![0.0; 256]);
            synth.process(&mut l, &mut r);
            out.extend(l);
        }
        out
    }

    #[test]
    fn plays_notes_and_releases_them() {
        let mut synth = Synth::default();
        synth.prepare(48_000, 256);
        assert!(
            render(&mut synth, 2).iter().all(|&s| s == 0.0),
            "silent without notes"
        );
        synth.midi(&MidiEvent::note_on(0, 0, 69, 100));
        let held = render(&mut synth, 40);
        let energy: f32 = held[4800..].iter().map(|s| s * s).sum();
        assert!(energy > 1.0, "{energy}");
        assert!(held.iter().all(|s| s.is_finite() && s.abs() < 2.0));
        synth.midi(&MidiEvent::note_off(0, 0, 69));
        let released = render(&mut synth, 200);
        assert!(
            released[released.len() - 256..].iter().all(|&s| s == 0.0),
            "voice freed"
        );
        for key in 40..80 {
            synth.midi(&MidiEvent::note_on(0, 0, key, 90));
        }
        assert_eq!(
            synth
                .voices
                .iter()
                .filter(|v| v.stage != Stage::Idle)
                .count(),
            VOICES
        );
    }

    #[test]
    fn stays_finite_with_the_filter_wide_open() {
        for resonance in [0.0, 20.0, 62.0, 95.0] {
            wide_open(resonance);
        }
    }

    fn wide_open(resonance: f32) {
        let mut synth = Synth::default();
        synth.set_param(6, resonance);
        synth.set_param(7, 100.0);
        synth.prepare(48_000, 256);
        // CC 74 at the top and full pressure, as from a controller's knobs and pads.
        synth.midi(&MidiEvent::control(0, 0, 74, 127));
        synth.midi(&MidiEvent {
            time: 0,
            data: [0xD0, 127, 0],
        });
        for key in [60, 72, 84, 96] {
            synth.midi(&MidiEvent::note_on(0, 0, key, 127));
        }
        let out = render(&mut synth, 400);
        let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(
            out.iter().all(|s| s.is_finite()) && peak < 4.0,
            "resonance {resonance}: {peak}"
        );
    }

    /// Frequency of the output from its zero crossings.
    fn pitch(out: &[f32]) -> f32 {
        let part = &out[out.len() / 2..];
        let n = part
            .windows(2)
            .filter(|w| w[0] < 0.0 && w[1] >= 0.0)
            .count();
        n as f32 * 48_000.0 / part.len() as f32
    }

    #[test]
    fn mpe_bends_each_note_on_its_channel() {
        let mut synth = Synth::default();
        synth.set_param(0, 2.0);
        synth.set_param(12, 1.0);
        synth.prepare(48_000, 256);
        // Member channel 2: +12 semitones of the 48 st range.
        synth.midi(&MidiEvent {
            time: 0,
            data: [0xE1, 0, 80],
        });
        synth.midi(&MidiEvent::note_on(0, 1, 57, 100));
        let bent = pitch(&render(&mut synth, 40));
        assert!((bent - 440.0).abs() < 8.0, "{bent}");
        synth.reset();
        // Without MPE the same bend uses the 2 st range.
        synth.set_param(12, 0.0);
        synth.midi(&MidiEvent {
            time: 0,
            data: [0xE1, 0, 80],
        });
        synth.midi(&MidiEvent::note_on(0, 1, 57, 100));
        let plain = pitch(&render(&mut synth, 40));
        assert!(
            (plain - 220.0 * 2f32.powf(2.0 / 12.0 * 0.25)).abs() < 6.0,
            "{plain}"
        );
        synth.midi(&MidiEvent::control(0, 1, 121, 0));
        let reset = pitch(&render(&mut synth, 40));
        assert!((reset - 220.0).abs() < 6.0, "{reset}");
    }
}
