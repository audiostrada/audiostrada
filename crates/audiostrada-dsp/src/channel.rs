//! Channel strip: the processing of a console channel, ahead of the inserts. Filters (high and
//! low pass), gate, compressor, a four-band EQ and a soft drive, each switched on alone; off,
//! a module is not processed at all, so a strip left alone is transparent.

use crate::biquad::{Biquad, Coefficients, FilterKind};
use crate::{Effect, EffectInfo, choice, db_to_gain, gain_to_db, param, switch};

pub const INFO: EffectInfo = EffectInfo {
    id: "channel",
    name: "Strada-Channel",
    description: "Filters, gate, compressor, four-band EQ and drive, as on a console channel.",
    params: &[
        switch("filters", "Filters", 0.0),
        param("hpf", "High pass", 20.0, 1000.0, 80.0, "Hz"),
        param("lpf", "Low pass", 1000.0, 20000.0, 20000.0, "Hz"),
        choice("slope", "Slope", &SLOPES, 0.0),
        switch("gate", "Gate", 0.0),
        param("gate_threshold", "Gate threshold", -80.0, 0.0, -50.0, "dB"),
        param("gate_range", "Gate range", -80.0, 0.0, -40.0, "dB"),
        param("gate_attack", "Gate attack", 0.05, 50.0, 1.0, "ms"),
        param("gate_release", "Gate release", 5.0, 2000.0, 120.0, "ms"),
        switch("comp", "Compressor", 0.0),
        choice("comp_type", "Character", &crate::compressor::TYPES, 0.0),
        param("comp_threshold", "Threshold", -60.0, 0.0, -18.0, "dB"),
        param("comp_ratio", "Ratio", 1.0, 20.0, 3.0, ":1"),
        param("comp_attack", "Attack", 0.05, 100.0, 10.0, "ms"),
        param("comp_release", "Release", 10.0, 1200.0, 150.0, "ms"),
        param("comp_makeup", "Makeup", 0.0, 24.0, 0.0, "dB"),
        param("comp_mix", "Mix", 0.0, 100.0, 100.0, "%"),
        switch("eq", "EQ", 0.0),
        param("low_freq", "Low", 30.0, 400.0, 100.0, "Hz"),
        param("low_gain", "Low gain", -15.0, 15.0, 0.0, "dB"),
        param("lmf_freq", "Low mid", 150.0, 2000.0, 400.0, "Hz"),
        param("lmf_gain", "Low mid gain", -15.0, 15.0, 0.0, "dB"),
        param("lmf_q", "Low mid Q", 0.3, 4.0, 0.9, ""),
        param("hmf_freq", "High mid", 600.0, 8000.0, 2500.0, "Hz"),
        param("hmf_gain", "High mid gain", -15.0, 15.0, 0.0, "dB"),
        param("hmf_q", "High mid Q", 0.3, 4.0, 0.9, ""),
        param("high_freq", "High", 2000.0, 16000.0, 8000.0, "Hz"),
        param("high_gain", "High gain", -15.0, 15.0, 0.0, "dB"),
        param("order", "Module order", 0.0, 119.0, 0.0, ""),
        switch("drive", "Drive", 0.0),
        param("drive_amount", "Drive amount", 0.0, 100.0, 30.0, "%"),
        param("output", "Output", -18.0, 18.0, 0.0, "dB"),
        // Set by the engine from the track's console color.
        choice("character", "Character", &CHARACTERS, 0.0),
    ],
    meters: &["Gate", "Reduction", "Input", "Output"],
    instrument: false,
    files: &[],
    category: "Channel",
};

pub const SLOPES: [&str; 2] = ["12 dB/oct", "24 dB/oct"];

/// The drive's and the EQ's character: soft, or that of a console (as in
/// [`crate::console::MODELS`]).
pub const CHARACTERS: [&str; 5] = [
    "Soft",
    "British Warm",
    "British Bright",
    "American Punch",
    "Nashville",
];

/// The modules, by the id of their switch, in their default order.
pub const MODULES: [&str; 5] = ["filters", "gate", "comp", "eq", "drive"];

/// The order the modules play in, from the `order` parameter: one of the 120 arrangements
/// (indices into [`MODULES`]).
pub fn module_order(code: f32) -> [usize; 5] {
    let mut code = (code.round().max(0.0) as usize).min(119);
    let mut left = [0, 1, 2, 3, 4];
    let mut order = [0; 5];
    for (i, slot) in order.iter_mut().enumerate() {
        let f = FACTORIALS[4 - i];
        let pick = code / f;
        *slot = left[pick];
        left.copy_within(pick + 1.., pick);
        code %= f;
    }
    order
}

/// The `order` parameter of an arrangement of the modules.
pub fn order_code(order: [usize; 5]) -> f32 {
    let mut code = 0;
    for i in 0..5 {
        let smaller = order[i + 1..].iter().filter(|&&m| m < order[i]).count();
        code += smaller * FACTORIALS[4 - i];
    }
    code as f32
}

const FACTORIALS: [usize; 5] = [1, 1, 2, 6, 24];

/// Indices into [`INFO`]'s parameters.
mod p {
    pub const FILTERS: usize = 0;
    pub const HPF: usize = 1;
    pub const LPF: usize = 2;
    pub const SLOPE: usize = 3;
    pub const GATE: usize = 4;
    pub const GATE_FIRST: usize = 5;
    pub const COMP: usize = 9;
    pub const COMP_FIRST: usize = 10;
    pub const EQ: usize = 17;
    pub const EQ_FIRST: usize = 18;
    pub const EQ_LAST: usize = 27;
    pub const ORDER: usize = 28;
    pub const DRIVE: usize = 29;
    pub const DRIVE_AMOUNT: usize = 30;
    pub const OUTPUT: usize = 31;
    pub const CHARACTER: usize = 32;
}

/// Butterworth sections: one for 12 dB/oct, two for 24.
const Q12: [f64; 1] = [std::f64::consts::FRAC_1_SQRT_2];
const Q24: [f64; 2] = [0.541_196_1, 1.306_563];

#[derive(Debug)]
pub struct Channel {
    values: [f32; 33],
    rate: f64,
    /// High pass then low pass sections, and their state per side.
    filters: Vec<Coefficients>,
    filter_state: [[Biquad; 4]; 2],
    bands: [Coefficients; 4],
    band_state: [[Biquad; 4]; 2],
    gate: crate::gate::Gate,
    comp: crate::compressor::Compressor,
    /// Gate gain and compressor reduction, for the meters.
    gate_db: f32,
    reduction: f32,
    levels: [f32; 2],
    order: [usize; 5],
}

impl Default for Channel {
    fn default() -> Self {
        let mut values = [0.0; 33];
        for (v, info) in values.iter_mut().zip(INFO.params) {
            *v = info.default;
        }
        let mut channel = Self {
            values,
            rate: 48_000.0,
            filters: Vec::with_capacity(4),
            filter_state: Default::default(),
            bands: [Coefficients::design(FilterKind::Peak, 1000.0, 1.0, 0.0, 48_000.0); 4],
            band_state: Default::default(),
            gate: Default::default(),
            comp: Default::default(),
            gate_db: 0.0,
            reduction: 0.0,
            levels: [-120.0; 2],
            order: [0, 1, 2, 3, 4],
        };
        channel.design();
        channel.forward_all();
        channel
    }
}

impl Channel {
    fn on(&self, index: usize) -> bool {
        self.values[index] >= 0.5
    }

    fn design(&mut self) {
        let v = &self.values;
        let rate = self.rate;
        let nyquist = rate * 0.45;
        let qs: &[f64] = if v[p::SLOPE] >= 0.5 { &Q24 } else { &Q12 };
        self.filters.clear();
        for &q in qs {
            self.filters.push(Coefficients::design(
                FilterKind::HighPass,
                f64::from(v[p::HPF]).min(nyquist),
                q,
                0.0,
                rate,
            ));
        }
        for &q in qs {
            self.filters.push(Coefficients::design(
                FilterKind::LowPass,
                f64::from(v[p::LPF]).min(nyquist),
                q,
                0.0,
                rate,
            ));
        }
        let band = |kind, freq: f32, gain: f32, q: f64| {
            Coefficients::design(kind, f64::from(freq).min(nyquist), q, f64::from(gain), rate)
        };
        // Each console's EQ: broad bells, or a Q that narrows as the gain grows; its shelves.
        let character = v[p::CHARACTER].round() as usize;
        let (shelf, broad, proportional) = match character {
            1 => (1.0, 0.7, 0.0),
            2 => (0.8, 1.0, 1.2),
            3 => (0.7, 1.0, 2.0),
            4 => (0.6, 0.85, 0.0),
            _ => (0.7, 1.0, 0.0),
        };
        let bell = |q: f32, gain: f32| {
            f64::from(q) * broad * (1.0 + f64::from(gain.abs()) / 15.0 * proportional)
        };
        self.bands = [
            band(FilterKind::LowShelf, v[18], v[19], shelf),
            band(FilterKind::Peak, v[20], v[21], bell(v[22], v[21])),
            band(FilterKind::Peak, v[23], v[24], bell(v[25], v[24])),
            band(FilterKind::HighShelf, v[26], v[27], shelf),
        ];
    }

    /// Hands the gate's and the compressor's settings to them.
    fn forward_all(&mut self) {
        for i in p::GATE_FIRST..p::COMP {
            self.forward(i);
        }
        for i in p::COMP_FIRST..p::EQ {
            self.forward(i);
        }
        // Fixed: a short hold for the gate; fixed release (not automatic) and a soft knee.
        self.gate.set_param(3, 30.0);
        self.comp.set_param(5, 0.0);
        self.comp.set_param(6, 6.0);
    }

    fn forward(&mut self, index: usize) {
        let value = self.values[index];
        match index {
            5 => self.gate.set_param(0, value),
            6 => self.gate.set_param(1, value),
            7 => self.gate.set_param(2, value),
            8 => self.gate.set_param(4, value),
            10 => self.comp.set_param(0, value),
            11 => self.comp.set_param(1, value),
            12 => self.comp.set_param(2, value),
            13 => self.comp.set_param(3, value),
            14 => self.comp.set_param(4, value),
            15 => self.comp.set_param(7, value),
            16 => self.comp.set_param(10, value),
            _ => {}
        }
    }

    fn filters(&mut self, left: &mut [f32], right: &mut [f32]) {
        for (side, buffer) in [left, right].into_iter().enumerate() {
            let state = &mut self.filter_state[side];
            for x in buffer.iter_mut() {
                let mut y = f64::from(*x);
                for (c, s) in self.filters.iter().zip(state.iter_mut()) {
                    y = s.process(c, y);
                }
                *x = y as f32;
            }
        }
    }

    fn eq(&mut self, left: &mut [f32], right: &mut [f32]) {
        for (side, buffer) in [left, right].into_iter().enumerate() {
            let state = &mut self.band_state[side];
            for x in buffer.iter_mut() {
                let mut y = f64::from(*x);
                for (c, s) in self.bands.iter().zip(state.iter_mut()) {
                    y = s.process(c, y);
                }
                *x = y as f32;
            }
        }
    }

    fn compress(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.comp.process(left, right);
        let mut out = [0.0; 3];
        self.comp.meters(&mut out);
        self.reduction = out[0];
    }

    /// A saturation in the console's character (gentle without one): unchanged for small
    /// signals, rounder peaks, its harmonics, at about the same loudness.
    fn drive(&mut self, left: &mut [f32], right: &mut [f32]) {
        // A -12 dBFS signal keeps its level; louder ones round off, softer ones barely rise.
        const REFERENCE: f32 = 0.25;
        let amount = self.values[p::DRIVE_AMOUNT].clamp(0.0, 100.0) / 100.0;
        let voice = crate::console::voice(self.values[p::CHARACTER].round() as usize);
        let (push, even) = voice.map_or((1.0, 0.05), |v| (v.drive, v.even));
        let g = 1.0 + amount * 2.0 * push;
        let bias = even * amount;
        let offset = (g * bias).tanh();
        let k = REFERENCE * g / ((g * (REFERENCE + bias)).tanh() - offset);
        for x in left.iter_mut().chain(right.iter_mut()) {
            *x = ((g * (*x + bias)).tanh() - offset) / g * k;
        }
    }

    pub fn eq_response_db(&self, freq: f64) -> f64 {
        let mut db = 0.0;
        if self.on(p::EQ) {
            db += self
                .bands
                .iter()
                .map(|c| c.magnitude_db(freq, self.rate))
                .sum::<f64>();
        }
        if self.on(p::FILTERS) {
            db += self
                .filters
                .iter()
                .map(|c| c.magnitude_db(freq, self.rate))
                .sum::<f64>();
        }
        db
    }
}

fn peak(buffer: &[f32]) -> f32 {
    buffer.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

impl Effect for Channel {
    fn prepare(&mut self, sample_rate: u32, max_frames: usize) {
        self.rate = f64::from(sample_rate);
        self.gate.prepare(sample_rate, max_frames);
        self.comp.prepare(sample_rate, max_frames);
        self.forward_all();
        self.design();
        self.reset();
    }

    fn set_param(&mut self, index: usize, value: f32) {
        let Some(v) = self.values.get_mut(index) else {
            return;
        };
        if *v == value {
            return;
        }
        *v = value;
        match index {
            p::HPF | p::LPF | p::SLOPE | p::CHARACTER => self.design(),
            i if (p::EQ_FIRST..=p::EQ_LAST).contains(&i) => self.design(),
            p::ORDER => self.order = module_order(value),
            _ => self.forward(index),
        }
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let input = peak(left).max(peak(right));
        self.gate_db = 0.0;
        if !self.on(p::COMP) {
            self.reduction = 0.0;
        }
        for module in self.order {
            match module {
                0 if self.on(p::FILTERS) => self.filters(left, right),
                1 if self.on(p::GATE) => {
                    self.gate.process(left, right);
                    let mut out = [0.0; 2];
                    self.gate.meters(&mut out);
                    self.gate_db = out[0];
                }
                2 if self.on(p::COMP) => self.compress(left, right),
                3 if self.on(p::EQ) => self.eq(left, right),
                4 if self.on(p::DRIVE) => self.drive(left, right),
                _ => {}
            }
        }
        let output = self.values[p::OUTPUT];
        if output != 0.0 {
            let g = db_to_gain(output);
            for x in left.iter_mut().chain(right.iter_mut()) {
                *x *= g;
            }
        }
        self.levels = [gain_to_db(input), gain_to_db(peak(left).max(peak(right)))];
    }

    fn reset(&mut self) {
        for side in &mut self.filter_state {
            side.iter_mut().for_each(Biquad::reset);
        }
        for side in &mut self.band_state {
            side.iter_mut().for_each(Biquad::reset);
        }
        self.gate.reset();
        self.comp.reset();
    }

    fn latency(&self) -> u32 {
        // A module off is not processed: it adds nothing.
        let comp = if self.on(p::COMP) {
            self.comp.latency()
        } else {
            0
        };
        let gate = if self.on(p::GATE) {
            self.gate.latency()
        } else {
            0
        };
        comp + gate
    }

    fn meters(&self, out: &mut [f32]) {
        for (o, v) in
            out.iter_mut()
                .zip([self.gate_db, self.reduction, self.levels[0], self.levels[1]])
        {
            *o = v;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index(id: &str) -> usize {
        INFO.params.iter().position(|p| p.id == id).unwrap()
    }

    fn sine(freq: f32, amplitude: f32) -> Vec<f32> {
        (0..48_000)
            .map(|i| amplitude * (i as f32 * freq * std::f32::consts::TAU / 48_000.0).sin())
            .collect()
    }

    fn run(channel: &mut Channel, freq: f32, amplitude: f32) -> f32 {
        let mut left = sine(freq, amplitude);
        let mut right = left.clone();
        for (l, r) in left.chunks_mut(256).zip(right.chunks_mut(256)) {
            channel.process(l, r);
        }
        peak(&left[24_000..])
    }

    #[test]
    fn the_indices_match_the_table_and_off_is_transparent() {
        assert_eq!(INFO.params.len(), 33);
        for (name, i) in [
            ("filters", p::FILTERS),
            ("hpf", p::HPF),
            ("lpf", p::LPF),
            ("slope", p::SLOPE),
            ("gate", p::GATE),
            ("gate_threshold", p::GATE_FIRST),
            ("comp", p::COMP),
            ("comp_type", p::COMP_FIRST),
            ("eq", p::EQ),
            ("low_freq", p::EQ_FIRST),
            ("high_gain", p::EQ_LAST),
            ("order", p::ORDER),
            ("drive", p::DRIVE),
            ("drive_amount", p::DRIVE_AMOUNT),
            ("output", p::OUTPUT),
            ("character", p::CHARACTER),
        ] {
            assert_eq!(index(name), i, "{name}");
        }
        let mut channel = Channel::default();
        channel.prepare(48_000, 256);
        let mut left = sine(440.0, 0.5);
        let mut right = left.clone();
        let original = left.clone();
        channel.process(&mut left, &mut right);
        assert_eq!(left, original, "untouched while every module is off");
    }

    #[test]
    fn every_order_has_its_own_code() {
        for code in 0..120 {
            let order = module_order(code as f32);
            let mut sorted = order;
            sorted.sort_unstable();
            assert_eq!(sorted, [0, 1, 2, 3, 4]);
            assert_eq!(order_code(order), code as f32);
        }
        assert_eq!(module_order(0.0), [0, 1, 2, 3, 4]);
        // The drive first: the compressor then hears the saturated signal.
        let mut channel = Channel::default();
        channel.prepare(48_000, 256);
        channel.set_param(p::ORDER, order_code([4, 0, 1, 2, 3]));
        assert_eq!(channel.order, [4, 0, 1, 2, 3]);
    }

    #[test]
    fn modules_filter_shape_and_compress() {
        let mut channel = Channel::default();
        channel.prepare(48_000, 256);
        channel.set_param(p::FILTERS, 1.0);
        channel.set_param(p::HPF, 200.0);
        channel.set_param(p::SLOPE, 1.0);
        assert!(
            run(&mut channel, 40.0, 0.5) < 0.01,
            "the high pass cuts the lows"
        );
        assert!(
            run(&mut channel, 2_000.0, 0.5) > 0.45,
            "and lets the rest through"
        );
        let mut eq = Channel::default();
        eq.prepare(48_000, 256);
        eq.set_param(p::EQ, 1.0);
        eq.set_param(index("hmf_gain"), 6.0);
        let boosted = run(&mut eq, 2_500.0, 0.25);
        assert!((gain_to_db(boosted / 0.25) - 6.0).abs() < 0.5, "{boosted}");
        assert!((eq.eq_response_db(2_500.0) - 6.0).abs() < 0.2);
        let mut comp = Channel::default();
        comp.prepare(48_000, 256);
        comp.set_param(p::COMP, 1.0);
        comp.set_param(index("comp_threshold"), -30.0);
        comp.set_param(index("comp_ratio"), 8.0);
        let squeezed = run(&mut comp, 200.0, 0.9);
        assert!(squeezed < 0.6, "{squeezed}");
        let mut meters = [0.0; 4];
        comp.meters(&mut meters);
        assert!(meters[1].abs() > 3.0, "the reduction shows: {}", meters[1]);
    }

    #[test]
    fn a_console_character_shapes_the_eq() {
        let mut soft = Channel::default();
        soft.prepare(48_000, 256);
        soft.set_param(p::EQ, 1.0);
        soft.set_param(index("lmf_gain"), 12.0);
        let mut discrete = Channel::default();
        discrete.prepare(48_000, 256);
        discrete.set_param(p::EQ, 1.0);
        discrete.set_param(index("lmf_gain"), 12.0);
        discrete.set_param(p::CHARACTER, 3.0);
        // Same peak, narrower bell: less boost an octave away.
        assert!((soft.eq_response_db(400.0) - discrete.eq_response_db(400.0)).abs() < 0.2);
        assert!(discrete.eq_response_db(800.0) < soft.eq_response_db(800.0) - 1.0);
    }

    #[test]
    fn the_drive_keeps_small_signals_and_rounds_the_peaks() {
        let mut channel = Channel::default();
        channel.prepare(48_000, 256);
        channel.set_param(p::DRIVE, 1.0);
        channel.set_param(p::DRIVE_AMOUNT, 60.0);
        let small = run(&mut channel, 1_000.0, 0.01);
        assert!(
            (gain_to_db(small / 0.01)).abs() < 3.0,
            "about the same level: {small}"
        );
        let loud = run(&mut channel, 1_000.0, 1.0);
        assert!(loud < 1.0, "peaks rounded: {loud}");
    }
}
