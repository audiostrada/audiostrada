//! Parametric equalizer: up to 16 bands placed anywhere, each a bell, shelf, cut (6 to
//! 48 dB per octave), notch, band pass or tilt. A band can be dynamic (its gain moves by up to
//! its range while its frequencies are above a threshold), placed on both channels, one side,
//! the mid or the sides, and soloed (only its range is heard). Bells, shelves and high cuts
//! follow their analog curves up to Nyquist. Parameter changes glide over a few milliseconds,
//! coefficients are updated every 32 samples.

use crate::biquad::{Biquad, Coefficients, FilterKind};
use crate::{
    Effect, EffectInfo, ParamInfo, choice, db_to_gain, gain_to_db, param, switch, time_coefficient,
};

pub const BANDS: usize = 16;
/// Spectrum of the output, dynamic gain of each band, spectrum of the input.
pub const METERS: usize = 2 * crate::analyzer::BANDS + BANDS;
pub const INPUT_SPECTRUM: usize = crate::analyzer::BANDS + BANDS;
/// Parameters of each band, in the order of the `USED`..`RANGE` offsets.
const PER_BAND: usize = 10;
const USED: usize = 0;
const BYPASS: usize = 1;
const TYPE: usize = 2;
const FREQ: usize = 3;
const GAIN: usize = 4;
const Q: usize = 5;
const SLOPE: usize = 6;
const DYNAMIC: usize = 7;
const THRESHOLD: usize = 8;
const RANGE: usize = 9;
const OUTPUT: usize = BANDS * PER_BAND;
/// 1-based band heard alone, 0 for none; set by the editor without being saved.
const SOLO: usize = OUTPUT + 1;
/// Settings added after the first ones, three per band: placement, attack and release.
const EXTRA: usize = SOLO + 1;
const PER_BAND_EXTRA: usize = 3;
const PLACE: usize = 0;
const ATTACK: usize = 1;
const RELEASE: usize = 2;
const AUTO_GAIN: usize = EXTRA + BANDS * PER_BAND_EXTRA;
const COUNT: usize = AUTO_GAIN + 1;
const SUB_BLOCK: usize = 32;
const GLIDE_MS: f32 = 20.0;
/// Most filter sections of a band (a 48 dB per octave cut).
const SECTIONS: usize = 4;

pub const TYPES: [&str; 8] = [
    "Bell",
    "Low shelf",
    "High shelf",
    "Low cut",
    "High cut",
    "Notch",
    "Band pass",
    "Tilt shelf",
];
pub const SLOPES: [&str; 6] = [
    "6 dB/oct",
    "12 dB/oct",
    "18 dB/oct",
    "24 dB/oct",
    "36 dB/oct",
    "48 dB/oct",
];
pub const PLACES: [&str; 5] = ["Stereo", "Left", "Right", "Mid", "Side"];
/// Filter order of each slope.
const ORDERS: [usize; 6] = [1, 2, 3, 4, 6, 8];

pub const INFO: EffectInfo = EffectInfo {
    id: "eq",
    name: "Strada-EQ",
    params: &PARAMS,
    meters: &[""; METERS],
    instrument: false,
    files: &[],
    category: "EQ & Filter",
    description: "Equalizer with up to 16 bands: bells, shelves, steep cuts, notches, dynamic \
                  bands, band solo and an analyzer.",
};

macro_rules! band {
    ($n:literal) => {
        [
            switch(
                concat!("b", $n, "_used"),
                concat!("Band ", $n, " in use"),
                0.0,
            ),
            switch(
                concat!("b", $n, "_bypass"),
                concat!("Band ", $n, " bypass"),
                0.0,
            ),
            choice(
                concat!("b", $n, "_type"),
                concat!("Band ", $n, " type"),
                &TYPES,
                0.0,
            ),
            param(
                concat!("b", $n, "_freq"),
                concat!("Band ", $n, " frequency"),
                20.0,
                20_000.0,
                1000.0,
                "Hz",
            ),
            param(
                concat!("b", $n, "_gain"),
                concat!("Band ", $n, " gain"),
                -30.0,
                30.0,
                0.0,
                "dB",
            ),
            param(
                concat!("b", $n, "_q"),
                concat!("Band ", $n, " Q"),
                0.1,
                30.0,
                1.0,
                "",
            ),
            choice(
                concat!("b", $n, "_slope"),
                concat!("Band ", $n, " slope"),
                &SLOPES,
                1.0,
            ),
            switch(
                concat!("b", $n, "_dyn"),
                concat!("Band ", $n, " dynamic"),
                0.0,
            ),
            param(
                concat!("b", $n, "_thresh"),
                concat!("Band ", $n, " threshold"),
                -60.0,
                0.0,
                -24.0,
                "dB",
            ),
            param(
                concat!("b", $n, "_range"),
                concat!("Band ", $n, " range"),
                -30.0,
                30.0,
                -6.0,
                "dB",
            ),
        ]
    };
}

macro_rules! band_extra {
    ($n:literal) => {
        [
            choice(
                concat!("b", $n, "_place"),
                concat!("Band ", $n, " placement"),
                &PLACES,
                0.0,
            ),
            param(
                concat!("b", $n, "_attack"),
                concat!("Band ", $n, " attack"),
                0.1,
                200.0,
                5.0,
                "ms",
            ),
            param(
                concat!("b", $n, "_release"),
                concat!("Band ", $n, " release"),
                5.0,
                2000.0,
                120.0,
                "ms",
            ),
        ]
    };
}

const fn flatten(
    bands: [[ParamInfo; PER_BAND]; BANDS],
    tail: [ParamInfo; 2],
    extra: [[ParamInfo; PER_BAND_EXTRA]; BANDS],
    last: ParamInfo,
) -> [ParamInfo; COUNT] {
    let mut out = [tail[0]; COUNT];
    let mut b = 0;
    while b < BANDS {
        let mut k = 0;
        while k < PER_BAND {
            out[b * PER_BAND + k] = bands[b][k];
            k += 1;
        }
        b += 1;
    }
    out[OUTPUT] = tail[0];
    out[SOLO] = tail[1];
    b = 0;
    while b < BANDS {
        let mut k = 0;
        while k < PER_BAND_EXTRA {
            out[EXTRA + b * PER_BAND_EXTRA + k] = extra[b][k];
            k += 1;
        }
        b += 1;
    }
    out[AUTO_GAIN] = last;
    out
}

static PARAMS: [ParamInfo; COUNT] = flatten(
    [
        band!("1"),
        band!("2"),
        band!("3"),
        band!("4"),
        band!("5"),
        band!("6"),
        band!("7"),
        band!("8"),
        band!("9"),
        band!("10"),
        band!("11"),
        band!("12"),
        band!("13"),
        band!("14"),
        band!("15"),
        band!("16"),
    ],
    [
        param("output", "Output", -30.0, 30.0, 0.0, "dB"),
        param("solo", "Solo band", 0.0, BANDS as f32, 0.0, ""),
    ],
    [
        band_extra!("1"),
        band_extra!("2"),
        band_extra!("3"),
        band_extra!("4"),
        band_extra!("5"),
        band_extra!("6"),
        band_extra!("7"),
        band_extra!("8"),
        band_extra!("9"),
        band_extra!("10"),
        band_extra!("11"),
        band_extra!("12"),
        band_extra!("13"),
        band_extra!("14"),
        band_extra!("15"),
        band_extra!("16"),
    ],
    switch("auto_gain", "Auto gain", 0.0),
);

/// Identifier of a band's parameter, e.g. `param_id(2, "gain")` is `b3_gain`.
pub fn param_id(band: usize, field: &str) -> String {
    format!("b{}_{field}", band + 1)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BandType {
    Bell,
    LowShelf,
    HighShelf,
    LowCut,
    HighCut,
    Notch,
    BandPass,
    Tilt,
}

impl BandType {
    pub const ALL: [BandType; 8] = [
        BandType::Bell,
        BandType::LowShelf,
        BandType::HighShelf,
        BandType::LowCut,
        BandType::HighCut,
        BandType::Notch,
        BandType::BandPass,
        BandType::Tilt,
    ];

    pub fn from_value(v: f32) -> Self {
        Self::ALL[(v.round().max(0.0) as usize).min(Self::ALL.len() - 1)]
    }

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|&t| t == self).unwrap_or(0)
    }

    /// Bells, shelves and tilts boost or cut; the others only shape.
    pub fn has_gain(self) -> bool {
        matches!(
            self,
            BandType::Bell | BandType::LowShelf | BandType::HighShelf | BandType::Tilt
        )
    }

    pub fn has_slope(self) -> bool {
        matches!(self, BandType::LowCut | BandType::HighCut)
    }

    pub fn can_be_dynamic(self) -> bool {
        matches!(
            self,
            BandType::Bell | BandType::LowShelf | BandType::HighShelf
        )
    }
}

/// Which part of the stereo signal a band works on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Stereo,
    Left,
    Right,
    Mid,
    Side,
}

impl Place {
    pub const ALL: [Place; 5] = [
        Place::Stereo,
        Place::Left,
        Place::Right,
        Place::Mid,
        Place::Side,
    ];

    pub fn from_value(v: f32) -> Self {
        Self::ALL[(v.round().max(0.0) as usize).min(Self::ALL.len() - 1)]
    }

    /// The part of `(left, right)` the band hears.
    fn pick(self, left: f32, right: f32) -> f32 {
        match self {
            Place::Stereo | Place::Mid => 0.5 * (left + right),
            Place::Left => left,
            Place::Right => right,
            Place::Side => 0.5 * (left - right),
        }
    }
}

/// Settings of one band in use.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Band {
    pub kind: BandType,
    pub bypass: bool,
    pub freq: f32,
    pub gain: f32,
    pub q: f32,
    pub slope: usize,
    /// Threshold and range in dB, for a dynamic band.
    pub dynamic: Option<(f32, f32)>,
    pub place: Place,
    /// Attack and release of a dynamic band, in ms.
    pub times: (f32, f32),
}

/// Band `b` of `values` (in [`INFO`] order), `None` when not in use.
pub fn band(values: &[f32], b: usize) -> Option<Band> {
    let v = values.get(b * PER_BAND..(b + 1) * PER_BAND)?;
    if v[USED] < 0.5 {
        return None;
    }
    let kind = BandType::from_value(v[TYPE]);
    let extra = |k: usize| {
        let i = EXTRA + b * PER_BAND_EXTRA + k;
        values.get(i).copied().unwrap_or(PARAMS[i].default)
    };
    Some(Band {
        kind,
        bypass: v[BYPASS] >= 0.5,
        freq: v[FREQ],
        gain: if kind.has_gain() { v[GAIN] } else { 0.0 },
        q: v[Q],
        slope: (v[SLOPE].round().max(0.0) as usize).min(ORDERS.len() - 1),
        dynamic: (kind.can_be_dynamic() && v[DYNAMIC] >= 0.5).then_some((v[THRESHOLD], v[RANGE])),
        place: Place::from_value(extra(PLACE)),
        times: (extra(ATTACK), extra(RELEASE)),
    })
}

/// Filter sections of a band at `gain` dB, and how many there are.
fn sections(band: &Band, gain: f32, rate: f64) -> ([Coefficients; SECTIONS], usize) {
    let mut out = [Coefficients::IDENTITY; SECTIONS];
    let (f, q, g) = (f64::from(band.freq), f64::from(band.q), f64::from(gain));
    let design = |kind, q| Coefficients::matched(kind, f, q, g, rate);
    let n = match band.kind {
        BandType::Bell => {
            out[0] = design(FilterKind::Peak, q);
            1
        }
        BandType::LowShelf => {
            out[0] = design(FilterKind::LowShelf, q.clamp(0.3, 2.0));
            1
        }
        BandType::HighShelf => {
            out[0] = design(FilterKind::HighShelf, q.clamp(0.3, 2.0));
            1
        }
        BandType::Notch => {
            out[0] = design(FilterKind::Notch, q);
            1
        }
        BandType::BandPass => {
            out[0] = design(FilterKind::BandPass, q);
            1
        }
        BandType::Tilt => {
            let q = q.clamp(0.3, 1.0);
            out[0] = Coefficients::matched(FilterKind::LowShelf, f, q, -g / 2.0, rate);
            out[1] = Coefficients::matched(FilterKind::HighShelf, f, q, g / 2.0, rate);
            2
        }
        BandType::LowCut | BandType::HighCut => {
            let high = band.kind == BandType::LowCut;
            let kind = if high {
                FilterKind::HighPass
            } else {
                FilterKind::LowPass
            };
            let order = ORDERS[band.slope];
            let mut n = 0;
            if order % 2 == 1 {
                out[0] = Coefficients::first_order(high, f, rate);
                n = 1;
            }
            // Butterworth pairs; a single pair takes the band's Q as its resonance.
            for k in 1..=order / 2 {
                let butterworth = 1.0
                    / (2.0
                        * (std::f64::consts::PI * (2 * k - 1) as f64 / (2 * order) as f64).sin());
                let q = if order == 2 {
                    q.clamp(0.3, 10.0)
                } else {
                    butterworth
                };
                out[n] = Coefficients::matched(kind, f, q, 0.0, rate);
                n += 1;
            }
            n
        }
    };
    (out, n)
}

/// Response of band `b` in dB at `freq`, moved by `offset` dB (its dynamic gain); `None`
/// while the band is not in use.
pub fn band_db(values: &[f32], b: usize, offset: f32, freq: f64, rate: f64) -> Option<f64> {
    let band = band(values, b)?;
    if band.bypass {
        return Some(0.0);
    }
    let (sections, n) = sections(&band, band.gain + offset, rate);
    Some(
        sections[..n]
            .iter()
            .map(|c| c.magnitude_db(freq, rate))
            .sum(),
    )
}

/// Response of the equalizer at `freq` in dB, each band moved by its entry of `offsets`.
pub fn response_db(values: &[f32], offsets: &[f32], freq: f64, rate: f64) -> f64 {
    if values.len() < COUNT {
        return 0.0;
    }
    let bands: f64 = (0..BANDS)
        .filter_map(|b| {
            band_db(
                values,
                b,
                offsets.get(b).copied().unwrap_or(0.0),
                freq,
                rate,
            )
        })
        .sum();
    f64::from(values[OUTPUT]) + bands
}

/// Level change that makes up for the boosts and cuts of the bells, shelves and tilts: their
/// curve averaged over the spectrum (log spaced, 30 Hz to 16 kHz), negated.
pub fn auto_gain_db(values: &[f32], rate: f64) -> f32 {
    const POINTS: usize = 24;
    let mut sum = 0.0;
    for b in 0..BANDS {
        let Some(band) = band(values, b).filter(|b| !b.bypass && b.kind.has_gain()) else {
            continue;
        };
        let (sections, n) = sections(&band, band.gain, rate);
        for i in 0..POINTS {
            let f = 30.0 * (16_000.0f64 / 30.0).powf(i as f64 / (POINTS - 1) as f64);
            sum += sections[..n]
                .iter()
                .map(|c| c.magnitude_db(f, rate))
                .sum::<f64>();
        }
    }
    -(sum / POINTS as f64) as f32
}

/// Where the editor puts a new band: a cut near the ends of the range, a bell elsewhere.
pub fn new_band_type(freq: f32) -> BandType {
    if freq < 30.0 {
        BandType::LowCut
    } else if freq > 16_000.0 {
        BandType::HighCut
    } else {
        BandType::Bell
    }
}

#[derive(Clone, Copy, Debug)]
struct BandState {
    band: Option<Band>,
    sections: [Coefficients; SECTIONS],
    count: usize,
    filters: [[Biquad; SECTIONS]; 2],
    /// Gain the sections were designed for.
    designed: Option<f32>,
    detector: Coefficients,
    detector_state: Biquad,
    envelope: f32,
    /// Current dynamic gain change in dB.
    offset: f32,
}

impl Default for BandState {
    fn default() -> Self {
        Self {
            band: None,
            sections: [Coefficients::IDENTITY; SECTIONS],
            count: 0,
            filters: [[Biquad::default(); SECTIONS]; 2],
            designed: None,
            detector: Coefficients::IDENTITY,
            detector_state: Biquad::default(),
            envelope: 0.0,
            offset: 0.0,
        }
    }
}

#[derive(Debug)]
pub struct Equalizer {
    rate: f32,
    targets: [f32; COUNT],
    current: [f32; COUNT],
    settled: bool,
    bands: [BandState; BANDS],
    output: f32,
    /// The soloed band's range, as heard alone.
    audition: Option<Coefficients>,
    audition_state: [Biquad; 2],
    /// Spectra of the output and the input, reported as meters.
    analyzer: crate::analyzer::Analyzer,
    input_analyzer: crate::analyzer::Analyzer,
}

impl Default for Equalizer {
    fn default() -> Self {
        let defaults = PARAMS.map(|p| p.default);
        Self {
            rate: 48_000.0,
            targets: defaults,
            current: defaults,
            settled: false,
            bands: [BandState::default(); BANDS],
            output: 1.0,
            audition: None,
            audition_state: [Biquad::default(); 2],
            analyzer: crate::analyzer::Analyzer::default(),
            input_analyzer: crate::analyzer::Analyzer::default(),
        }
    }
}

/// The part of the spectrum a band works on: what its detector hears and its solo plays.
fn range_filter(band: &Band, rate: f64) -> Option<Coefficients> {
    let f = f64::from(band.freq);
    let q = f64::from(band.q).max(0.3);
    Some(match band.kind {
        BandType::Bell | BandType::Notch | BandType::BandPass => {
            Coefficients::design(FilterKind::BandPass, f, q, 0.0, rate)
        }
        BandType::LowShelf | BandType::LowCut => {
            Coefficients::design(FilterKind::LowPass, f, 0.707, 0.0, rate)
        }
        BandType::HighShelf | BandType::HighCut => {
            Coefficients::design(FilterKind::HighPass, f, 0.707, 0.0, rate)
        }
        BandType::Tilt => return None,
    })
}

impl Equalizer {
    /// Follows the settings: bands added, removed or changed.
    fn update(&mut self) {
        let rate = f64::from(self.rate);
        for (b, state) in self.bands.iter_mut().enumerate() {
            let band = band(&self.current, b).filter(|band| !band.bypass);
            if band.is_some() && state.band.map(|b| b.place) != band.map(|b| b.place) {
                state.filters = [[Biquad::default(); SECTIONS]; 2];
                state.detector_state.reset();
                state.envelope = 0.0;
            }
            if band != state.band {
                state.designed = None;
                if let Some(band) = &band {
                    state.detector = range_filter(band, rate).unwrap_or(Coefficients::IDENTITY);
                }
            }
            state.band = band;
            if band.is_none_or(|b| b.dynamic.is_none()) {
                state.offset = 0.0;
            }
        }
        let auto = if self.current[AUTO_GAIN] >= 0.5 {
            auto_gain_db(&self.current, rate)
        } else {
            0.0
        };
        self.output = db_to_gain(self.current[OUTPUT] + auto);
        let solo = self.current[SOLO].round() as usize;
        let soloed = solo
            .checked_sub(1)
            .and_then(|b| band(&self.current, b))
            .and_then(|b| range_filter(&b, rate));
        if soloed.is_some() && self.audition.is_none() {
            self.audition_state = [Biquad::default(); 2];
        }
        self.audition = soloed;
    }

    /// Moves the current values towards the targets over one sub-block; true if they moved.
    fn glide(&mut self, frames: usize) -> bool {
        if self.settled {
            return false;
        }
        let k = 1.0 - time_coefficient(GLIDE_MS, self.rate).powi(frames as i32);
        let mut settled = true;
        for ((c, &t), p) in self.current.iter_mut().zip(&self.targets).zip(&PARAMS) {
            if p.toggle || !p.choices.is_empty() || p.id == "solo" {
                *c = t;
                continue;
            }
            let delta = t - *c;
            if delta.abs() <= (p.max - p.min) * 1e-5 {
                *c = t;
            } else {
                *c += delta * k;
                settled = false;
            }
        }
        self.settled = settled;
        true
    }

    /// Detects the level of each dynamic band over the input (the part of it the band works
    /// on) and redesigns the bands whose gain changed.
    fn dynamics(&mut self, left: &[f32], right: &[f32]) {
        let rate = f64::from(self.rate);
        for state in &mut self.bands {
            let Some(band) = state.band else {
                continue;
            };
            if let Some((threshold, range)) = band.dynamic {
                let attack = time_coefficient(band.times.0, self.rate);
                let release = time_coefficient(band.times.1, self.rate);
                for (&l, &r) in left.iter().zip(right) {
                    let x = band.place.pick(l, r);
                    let level = state
                        .detector_state
                        .process(&state.detector, f64::from(x))
                        .abs() as f32;
                    let k = if level > state.envelope {
                        attack
                    } else {
                        release
                    };
                    state.envelope = level + k * (state.envelope - level);
                }
                // The full range is reached `range` dB above the threshold.
                let over = gain_to_db(state.envelope) - threshold;
                state.offset = if range.abs() < 0.01 {
                    0.0
                } else {
                    range * (over / range.abs()).clamp(0.0, 1.0)
                };
            }
            let gain = band.gain + state.offset;
            if state.designed.is_none_or(|g| (g - gain).abs() > 0.01) {
                (state.sections, state.count) = sections(&band, gain, rate);
                state.designed = Some(gain);
            }
        }
    }
}

impl Effect for Equalizer {
    fn prepare(&mut self, sample_rate: u32, max_frames: usize) {
        self.rate = sample_rate as f32;
        self.current = self.targets;
        self.settled = true;
        for state in &mut self.bands {
            *state = BandState::default();
        }
        self.update();
        self.analyzer.prepare(sample_rate, max_frames);
        self.input_analyzer.prepare(sample_rate, max_frames);
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if let Some(t) = self.targets.get_mut(index) {
            *t = value;
            self.settled = false;
        }
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len());
        self.input_analyzer.process(left, right);
        let mut start = 0;
        while start < n {
            let end = (start + SUB_BLOCK).min(n);
            if self.glide(end - start) {
                self.update();
            }
            self.dynamics(&left[start..end], &right[start..end]);
            let (left, right) = (&mut left[start..end], &mut right[start..end]);
            if let Some(audition) = &self.audition {
                for (ch, buffer) in [left, right].into_iter().enumerate() {
                    let state = &mut self.audition_state[ch];
                    for sample in buffer.iter_mut() {
                        *sample = state.process(audition, f64::from(*sample)) as f32;
                    }
                }
                start = end;
                continue;
            }
            for (l, r) in left.iter_mut().zip(right.iter_mut()) {
                let (mut x, mut y) = (f64::from(*l), f64::from(*r));
                for state in self.bands.iter_mut() {
                    let Some(band) = state.band else {
                        continue;
                    };
                    let [first, second] = &mut state.filters;
                    let run = |filters: &mut [Biquad; SECTIONS], v: f64| {
                        filters
                            .iter_mut()
                            .zip(&state.sections)
                            .take(state.count)
                            .fold(v, |v, (f, c)| f.process(c, v))
                    };
                    match band.place {
                        Place::Stereo => {
                            x = run(first, x);
                            y = run(second, y);
                        }
                        Place::Left => x = run(first, x),
                        Place::Right => y = run(second, y),
                        Place::Mid | Place::Side => {
                            let (mut mid, mut side) = (0.5 * (x + y), 0.5 * (x - y));
                            if band.place == Place::Mid {
                                mid = run(first, mid);
                            } else {
                                side = run(first, side);
                            }
                            (x, y) = (mid + side, mid - side);
                        }
                    }
                }
                *l = x as f32 * self.output;
                *r = y as f32 * self.output;
            }
            start = end;
        }
        self.analyzer.process(left, right);
    }

    fn reset(&mut self) {
        for state in &mut self.bands {
            state.filters = [[Biquad::default(); SECTIONS]; 2];
            state.detector_state.reset();
            state.envelope = 0.0;
            state.offset = 0.0;
        }
        self.audition_state = [Biquad::default(); 2];
    }

    fn meters(&self, out: &mut [f32]) {
        self.analyzer.meters(out);
        for (slot, state) in out.iter_mut().skip(crate::analyzer::BANDS).zip(&self.bands) {
            *slot = state.offset;
        }
        if let Some(input) = out.get_mut(INPUT_SPECTRUM..) {
            self.input_analyzer.meters(input);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine_gain(eq: &mut Equalizer, freq: f32, amplitude: f32) -> f32 {
        let mut peak: f32 = 0.0;
        let mut n = 0usize;
        for _ in 0..200 {
            let mut l: Vec<f32> = (0..256)
                .map(|i| {
                    amplitude * (std::f32::consts::TAU * freq * (n + i) as f32 / 48_000.0).sin()
                })
                .collect();
            let mut r = l.clone();
            n += 256;
            eq.process(&mut l, &mut r);
            if n > 48_000 {
                peak = l.iter().fold(peak, |p, s| p.max(s.abs()));
            }
        }
        crate::gain_to_db(peak / amplitude)
    }

    fn index(id: &str) -> usize {
        PARAMS.iter().position(|p| p.id == id).unwrap()
    }

    fn set(eq: &mut Equalizer, values: &mut [f32; COUNT], id: &str, v: f32) {
        eq.set_param(index(id), v);
        values[index(id)] = v;
    }

    #[test]
    fn flat_by_default_and_bands_cut_or_boost() {
        let mut eq = Equalizer::default();
        let mut values = PARAMS.map(|p| p.default);
        eq.prepare(48_000, 256);
        assert!(sine_gain(&mut eq, 1000.0, 0.5).abs() < 0.01);
        set(&mut eq, &mut values, "b3_used", 1.0);
        set(&mut eq, &mut values, "b3_freq", 800.0);
        set(&mut eq, &mut values, "b3_gain", 6.0);
        assert!((sine_gain(&mut eq, 800.0, 0.5) - 6.0).abs() < 0.1);
        assert!((response_db(&values, &[], 800.0, 48_000.0) - 6.0).abs() < 0.01);
        // A 48 dB per octave low cut, an octave below its frequency.
        set(&mut eq, &mut values, "b1_used", 1.0);
        set(
            &mut eq,
            &mut values,
            "b1_type",
            BandType::LowCut.index() as f32,
        );
        set(&mut eq, &mut values, "b1_freq", 400.0);
        set(&mut eq, &mut values, "b1_slope", 5.0);
        let below = response_db(&values, &[], 200.0, 48_000.0);
        assert!((below + 48.0).abs() < 3.0, "{below}");
        assert!(sine_gain(&mut eq, 100.0, 0.5) < -80.0);
        set(&mut eq, &mut values, "b1_slope", 0.0);
        let gentle = response_db(&values, &[], 200.0, 48_000.0);
        assert!((gentle + 7.0).abs() < 1.0, "6 dB per octave: {gentle}");
        set(&mut eq, &mut values, "b3_bypass", 1.0);
        assert_eq!(band_db(&values, 2, 0.0, 800.0, 48_000.0), Some(0.0));
    }

    #[test]
    fn a_dynamic_band_moves_with_the_level() {
        let mut eq = Equalizer::default();
        eq.prepare(48_000, 256);
        let mut values = PARAMS.map(|p| p.default);
        for (id, v) in [
            ("b1_used", 1.0),
            ("b1_freq", 1000.0),
            ("b1_dyn", 1.0),
            ("b1_thresh", -30.0),
            ("b1_range", -9.0),
        ] {
            set(&mut eq, &mut values, id, v);
        }
        assert!(
            sine_gain(&mut eq, 1000.0, 0.005).abs() < 0.2,
            "quiet: untouched"
        );
        let loud = sine_gain(&mut eq, 1000.0, 0.5);
        assert!((loud + 9.0).abs() < 0.5, "loud: the whole range, {loud}");
        let mut meters = [0.0; METERS];
        eq.meters(&mut meters);
        assert!((meters[crate::analyzer::BANDS] + 9.0).abs() < 0.5);
    }

    #[test]
    fn a_soloed_band_plays_only_its_range() {
        let mut eq = Equalizer::default();
        eq.prepare(48_000, 256);
        let mut values = PARAMS.map(|p| p.default);
        for (id, v) in [
            ("b2_used", 1.0),
            ("b2_freq", 1000.0),
            ("b2_q", 4.0),
            ("solo", 2.0),
        ] {
            set(&mut eq, &mut values, id, v);
        }
        assert!(sine_gain(&mut eq, 1000.0, 0.5).abs() < 0.2);
        assert!(sine_gain(&mut eq, 100.0, 0.5) < -20.0);
        set(&mut eq, &mut values, "solo", 0.0);
        assert!(sine_gain(&mut eq, 100.0, 0.5).abs() < 0.1);
    }

    #[test]
    fn bands_work_on_their_part_of_the_stereo_signal() {
        let mut eq = Equalizer::default();
        eq.prepare(48_000, 256);
        let mut values = PARAMS.map(|p| p.default);
        for (id, v) in [
            ("b1_used", 1.0),
            ("b1_freq", 1000.0),
            ("b1_gain", 6.0),
            ("b1_place", 1.0),
        ] {
            set(&mut eq, &mut values, id, v);
        }
        let gains = |eq: &mut Equalizer, side: bool| {
            let (mut peak_l, mut peak_r) = (0.0f32, 0.0f32);
            for block in 0..200 {
                let x: Vec<f32> = (0..256)
                    .map(|i| {
                        0.25 * (std::f32::consts::TAU * 1000.0 * (block * 256 + i) as f32
                            / 48_000.0)
                            .sin()
                    })
                    .collect();
                let mut l = x.clone();
                let mut r: Vec<f32> = x.iter().map(|v| if side { -v } else { *v }).collect();
                eq.process(&mut l, &mut r);
                if block > 150 {
                    peak_l = l.iter().fold(peak_l, |p, s| p.max(s.abs()));
                    peak_r = r.iter().fold(peak_r, |p, s| p.max(s.abs()));
                }
            }
            (
                crate::gain_to_db(peak_l / 0.25),
                crate::gain_to_db(peak_r / 0.25),
            )
        };
        let (l, r) = gains(&mut eq, false);
        assert!(
            (l - 6.0).abs() < 0.1 && r.abs() < 0.05,
            "left only: {l} {r}"
        );
        set(&mut eq, &mut values, "b1_place", 3.0);
        let (l, r) = gains(&mut eq, false);
        assert!(
            (l - 6.0).abs() < 0.1 && (r - 6.0).abs() < 0.1,
            "mid, mono: {l} {r}"
        );
        let (l, r) = gains(&mut eq, true);
        assert!(l.abs() < 0.05 && r.abs() < 0.05, "mid, sides only: {l} {r}");
        set(&mut eq, &mut values, "b1_place", 4.0);
        let (l, r) = gains(&mut eq, true);
        assert!(
            (l - 6.0).abs() < 0.1 && (r - 6.0).abs() < 0.1,
            "side: {l} {r}"
        );
    }

    #[test]
    fn a_dynamic_band_follows_its_attack() {
        let reached = |attack: f32| {
            let mut eq = Equalizer::default();
            let mut values = PARAMS.map(|p| p.default);
            for (id, v) in [
                ("b1_used", 1.0),
                ("b1_freq", 1000.0),
                ("b1_dyn", 1.0),
                ("b1_thresh", -40.0),
                ("b1_range", -12.0),
                ("b1_attack", attack),
            ] {
                set(&mut eq, &mut values, id, v);
            }
            eq.prepare(48_000, 256);
            let mut l: Vec<f32> = (0..480)
                .map(|i| 0.5 * (std::f32::consts::TAU * 1000.0 * i as f32 / 48_000.0).sin())
                .collect();
            let mut r = l.clone();
            eq.process(&mut l, &mut r);
            let mut meters = [0.0; METERS];
            eq.meters(&mut meters);
            meters[crate::analyzer::BANDS]
        };
        let (fast, slow) = (reached(0.5), reached(200.0));
        assert!(fast < -11.0, "10 ms in, a fast band has its range: {fast}");
        assert!(slow > -6.0, "a slow one is still on its way: {slow}");
    }

    #[test]
    fn auto_gain_makes_up_for_the_curve() {
        let mut eq = Equalizer::default();
        eq.prepare(48_000, 256);
        let mut values = PARAMS.map(|p| p.default);
        for (id, v) in [
            ("b1_used", 1.0),
            ("b1_type", BandType::LowShelf.index() as f32),
            ("b1_freq", 200.0),
            ("b1_gain", 9.0),
        ] {
            set(&mut eq, &mut values, id, v);
        }
        let auto = auto_gain_db(&values, 48_000.0);
        assert!(auto < -1.0 && auto > -6.0, "{auto}");
        let before = sine_gain(&mut eq, 5000.0, 0.5);
        set(&mut eq, &mut values, "auto_gain", 1.0);
        let after = sine_gain(&mut eq, 5000.0, 0.5);
        assert!(
            (after - before - auto).abs() < 0.1,
            "{before} {after} {auto}"
        );
    }
}
