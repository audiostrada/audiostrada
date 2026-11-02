//! Levels before and after an insert, and its output gain: effect windows compare the dry and
//! the processed signal. Match is a listening aid: while on, the output follows the input's
//! loudness, on the live player only (never in renders).

use std::sync::Arc;

use audiostrada_dsp::db_to_gain;
use audiostrada_dsp::loudness::LevelMeter;

use crate::param::{SharedParam, Smoothed};

/// UI side: input and output peaks (left, right, dBFS), input and output short-term loudness
/// (LUFS), the output gain (dB) to apply and the Match switch.
#[derive(Clone, Debug)]
pub struct LevelControls {
    values: Arc<[SharedParam]>,
    pub gain_db: SharedParam,
    matching: SharedParam,
    /// Measured only while shown (an effect window is open): metering costs.
    watched: SharedParam,
}

impl LevelControls {
    /// `[in L, in R, out L, out R, in LUFS, out LUFS, match dB]`; the output is measured
    /// before Match.
    pub fn read(&self) -> [f32; 7] {
        std::array::from_fn(|i| self.values[i].get())
    }

    pub fn set_matching(&self, on: bool) {
        self.matching.set(if on { 1.0 } else { 0.0 });
    }

    /// Measures the levels while `on` (their window is open).
    pub fn watch(&self, on: bool) {
        self.watched.set(if on { 1.0 } else { 0.0 });
    }
}

pub(crate) struct Levels {
    input: LevelMeter,
    output: LevelMeter,
    gain: Smoothed,
    matched: Smoothed,
    match_db: f32,
    controls: LevelControls,
}

impl Levels {
    pub fn new() -> (Self, LevelControls) {
        let controls = LevelControls {
            values: (0..7)
                .map(|i| SharedParam::new(if i == 6 { 0.0 } else { -120.0 }))
                .collect(),
            gain_db: SharedParam::new(0.0),
            matching: SharedParam::new(0.0),
            watched: SharedParam::new(0.0),
        };
        (
            Self {
                input: LevelMeter::new(48_000),
                output: LevelMeter::new(48_000),
                gain: Smoothed::new(),
                matched: Smoothed::new(),
                match_db: 0.0,
                controls: controls.clone(),
            },
            controls,
        )
    }

    pub fn prepare(&mut self, rate: u32) {
        self.input = LevelMeter::new(rate);
        self.output = LevelMeter::new(rate);
    }

    fn watched(&self) -> bool {
        self.controls.watched.get() >= 0.5
    }

    pub fn input(&mut self, left: &[f32], right: &[f32]) {
        if !self.watched() {
            return;
        }
        for (&l, &r) in left.iter().zip(right) {
            self.input.push(l, r);
        }
    }

    /// Applies the output gain to the processed block, measures it, then applies Match and
    /// publishes the levels.
    pub fn output(&mut self, left: &mut [f32], right: &mut [f32]) {
        let target = db_to_gain(self.controls.gain_db.get());
        let (mut gain, step) = self.gain.ramp(target, left.len());
        let watched = self.watched();
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            *l *= gain;
            *r *= gain;
            gain += step;
            if watched {
                self.output.push(*l, *r);
            }
        }
        if !watched {
            return;
        }
        let (i, o) = (self.input.short_term(), self.output.short_term());
        self.match_db = if self.controls.matching.get() < 0.5 {
            0.0
        } else if i > -70.0 && o > -70.0 {
            (i - o).clamp(-24.0, 24.0)
        } else {
            self.match_db
        };
        let (mut gain, step) = self.matched.ramp(db_to_gain(self.match_db), left.len());
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            *l *= gain;
            *r *= gain;
            gain += step;
        }
        self.publish(false);
    }

    /// A bypassed insert: what comes out is what went in. The effect's last measure is kept,
    /// so Match is right as soon as the effect is back.
    pub fn bypassed(&mut self, left: &[f32], right: &[f32]) {
        if !self.watched() {
            return;
        }
        for (&l, &r) in left.iter().zip(right) {
            self.input.push(l, r);
        }
        self.publish(true);
    }

    pub fn reset(&mut self) {
        self.input.reset();
        self.output.reset();
    }

    fn publish(&self, bypassed: bool) {
        let [il, ir] = self.input.peaks();
        let input = self.input.short_term();
        let ([ol, or], output) = if bypassed {
            ([il, ir], input)
        } else {
            (self.output.peaks(), self.output.short_term())
        };
        let values = [il, ir, ol, or, input, output, self.match_db];
        for (shared, v) in self.controls.values.iter().zip(values) {
            shared.set(v);
        }
    }
}
