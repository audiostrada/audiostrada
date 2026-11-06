//! AudioStrada built-in audio effects. Kernels are plain stereo processors, free of engine types,
//! described by static tables so that hosts can build generic editors.

pub use audiostrada_midi::MidiEvent;

pub mod amp;
pub mod analysis;
pub mod analyzer;
pub mod biquad;
pub mod channel;
pub mod chorus;
pub mod compressor;
pub mod console;
pub mod convolver;
pub mod deesser;
pub mod delay;
pub mod doubler;
pub mod drive;
pub mod drums;
pub mod eq;
pub mod filter;
pub mod flanger;
pub mod gate;
pub mod groove;
mod hall;
pub mod imager;
pub mod limiter;
mod line;
pub mod loudness;
pub mod maximizer;
pub mod modulator;
pub mod multiband;
mod nam;
pub mod phaser;
pub mod pitch;
pub mod pitch_correct;
mod plate;
pub mod report;
pub mod resample;
pub mod reverb;
pub mod rig;
mod shift;
mod spring;
pub mod stomp;
pub mod stretch;
pub mod synth;
pub mod tape;
pub mod tempo;
pub mod transient;
pub mod trigger;
pub mod tuner;
mod utility;
mod vintage;
pub mod vintage_eq;
pub mod wavetable;

/// A parameter of an effect, as shown in the editor and stored in projects.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParamInfo {
    /// Stable identifier used in project files.
    pub id: &'static str,
    pub name: &'static str,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    pub unit: &'static str,
    /// On/off switch (0 or 1).
    pub toggle: bool,
    /// Names of the whole values from `min`, for a parameter that picks one of a list.
    pub choices: &'static [&'static str],
}

#[derive(Debug)]
pub struct EffectInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub params: &'static [ParamInfo],
    /// Values the effect reports for display (gain reduction, spectrum bands...).
    pub meters: &'static [&'static str],
    /// Makes sound from MIDI notes rather than processing its input.
    pub instrument: bool,
    /// Files the effect can load (amp model, impulse response).
    pub files: &'static [FileSlot],
    /// Family shown in the effect browser.
    pub category: &'static str,
    /// One sentence for the effect browser.
    pub description: &'static str,
}

/// Families of the effect browser, in display order.
pub const CATEGORIES: &[&str] = &[
    "Dynamics",
    "EQ & Filter",
    "Space",
    "Modulation",
    "Color",
    "Stereo",
    "Amp & Cab",
    "Utility",
    "Instrument",
];

#[derive(Debug)]
pub struct FileSlot {
    /// Stable identifier used in project files.
    pub id: &'static str,
    pub name: &'static str,
    pub kind: FileKind,
    pub extensions: &'static [&'static str],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileKind {
    /// Handed over as raw bytes.
    Data,
    /// Decoded by the host.
    Audio,
    /// A folder (or a single file) whose files with the slot's extensions are handed over
    /// together, as the captures of an amp at several settings.
    Pack,
}

#[derive(Debug)]
pub enum FileData {
    Data(Vec<u8>),
    Audio { channels: Vec<Vec<f32>>, rate: u32 },
    Pack(Vec<Vec<u8>>),
}

/// A stereo effect. `process` runs on the audio thread: no allocation, no locks.
pub trait Effect: Send {
    fn prepare(&mut self, sample_rate: u32, max_frames: usize);
    /// Sets the parameter at `index` in the effect's [`EffectInfo::params`].
    fn set_param(&mut self, index: usize, value: f32);
    fn process(&mut self, left: &mut [f32], right: &mut [f32]);
    /// Clears internal state (tails) after a transport jump.
    fn reset(&mut self) {}
    /// Delay added to the signal, in samples.
    fn latency(&self) -> u32 {
        0
    }
    /// Writes the values named in [`EffectInfo::meters`].
    fn meters(&self, _out: &mut [f32]) {}
    /// A MIDI message, applied before processing the frames that follow it.
    fn midi(&mut self, _event: &MidiEvent) {}
    /// Project tempo at the current block, for effects synced to it.
    fn set_tempo(&mut self, _bpm: f64) {}
    /// Processes with a sidechain key driving the detection (see [`supports_sidechain`]).
    fn process_keyed(&mut self, left: &mut [f32], right: &mut [f32], _key: (&[f32], &[f32])) {
        self.process(left, right);
    }
    /// Loads a file into one of [`EffectInfo::files`], before `prepare`; returns a short
    /// description of what was loaded.
    fn load(&mut self, _slot: &str, _file: FileData) -> Result<String, String> {
        Err("this effect loads no files".into())
    }
}

/// Builds a [`ParamInfo`] (shorthand for the effect tables).
pub(crate) const fn param(
    id: &'static str,
    name: &'static str,
    min: f32,
    max: f32,
    default: f32,
    unit: &'static str,
) -> ParamInfo {
    ParamInfo {
        id,
        name,
        min,
        max,
        default,
        unit,
        toggle: false,
        choices: &[],
    }
}

/// A parameter picking one of `choices` (0, 1, 2...).
pub(crate) const fn choice(
    id: &'static str,
    name: &'static str,
    choices: &'static [&'static str],
    default: f32,
) -> ParamInfo {
    ParamInfo {
        id,
        name,
        min: 0.0,
        max: (choices.len() - 1) as f32,
        default,
        unit: "",
        toggle: false,
        choices,
    }
}

/// A switch parameter (0 or 1).
pub(crate) const fn switch(id: &'static str, name: &'static str, default: f32) -> ParamInfo {
    ParamInfo {
        id,
        name,
        min: 0.0,
        max: 1.0,
        default,
        unit: "",
        toggle: true,
        choices: &[],
    }
}

/// One-pole smoothing coefficient reaching ~63% in `ms` milliseconds.
#[inline]
pub(crate) fn time_coefficient(ms: f32, rate: f32) -> f32 {
    if ms <= 0.0 {
        0.0
    } else {
        (-1.0 / (ms * 0.001 * rate)).exp()
    }
}

/// One-pole low-pass coefficient (`y += c * (x - y)`) for a cutoff, kept below Nyquist.
#[inline]
pub(crate) fn lowpass_coefficient(hz: f32, rate: f32) -> f32 {
    1.0 - pole(hz.min(rate * 0.45), rate)
}

/// Pole of a one-pole filter at `hz` (`y = x + p * (y - x)`; a high-pass uses it on the input).
#[inline]
pub(crate) fn pole(hz: f32, rate: f32) -> f32 {
    (-std::f32::consts::TAU * hz / rate).exp()
}

/// Peak of frame `i` of the sidechain `key` when given, else of `l` and `r`.
#[inline]
pub(crate) fn key_peak(key: Option<(&[f32], &[f32])>, i: usize, l: f32, r: f32) -> f32 {
    match key {
        Some((kl, kr)) => kl
            .get(i)
            .map_or(0.0, |v| v.abs())
            .max(kr.get(i).map_or(0.0, |v| v.abs())),
        None => l.abs().max(r.abs()),
    }
}

/// Converts a linear gain to decibels (floored at -200 dB).
#[inline]
pub fn gain_to_db(gain: f32) -> f32 {
    20.0 * gain.max(1e-10).log10()
}

pub const EFFECTS: &[&EffectInfo] = &[
    &synth::INFO,
    &drums::INFO,
    &channel::INFO,
    &eq::INFO,
    &vintage_eq::INFO,
    &filter::INFO,
    &compressor::INFO,
    &gate::INFO,
    &transient::INFO,
    &deesser::INFO,
    &limiter::INFO,
    &maximizer::INFO,
    &multiband::INFO,
    &reverb::INFO,
    &delay::INFO,
    &imager::INFO,
    &utility::INFO,
    &analyzer::INFO,
    &rig::INFO,
    &tuner::INFO,
    &pitch_correct::INFO,
    &drive::INFO,
    &tape::INFO,
    &chorus::INFO,
    &flanger::INFO,
    &phaser::INFO,
    &modulator::INFO,
    &pitch::INFO,
    &doubler::INFO,
    &trigger::INFO,
];

/// The delay an effect adds with these parameter values (in its order), when its settings
/// change it; `None` when it never changes.
pub fn declared_latency(id: &str, values: &[f32], rate: u32) -> Option<u32> {
    match id {
        "compressor" => Some(compressor::latency_of(values, rate as f32)),
        "gate" => Some(gate::latency_of(values, rate as f32)),
        "deesser" => Some(deesser::latency_of(values, rate as f32)),
        "limiter" => Some(limiter::latency_of(values, rate as f32)),
        "flanger" => Some(flanger::latency_of(values, rate as f32)),
        "pitch" => Some(pitch::latency_of(values, rate as f32)),
        "channel" => {
            let index = |id: &str| channel::INFO.params.iter().position(|p| p.id == id);
            let value = |id: &str| {
                index(id)
                    .and_then(|i| values.get(i))
                    .copied()
                    .unwrap_or(0.0)
            };
            let colored =
                value("comp") >= 0.5 && value("comp_type").round() as usize != compressor::CLEAN;
            Some(if colored {
                compressor::COLOR_LATENCY
            } else {
                0
            })
        }
        _ => None,
    }
}

/// Effects whose detection can listen to another track.
pub fn supports_sidechain(id: &str) -> bool {
    matches!(id, "compressor" | "gate")
}

pub fn info(id: &str) -> Option<&'static EffectInfo> {
    EFFECTS.iter().copied().find(|e| e.id == id)
}

pub fn create(id: &str) -> Option<Box<dyn Effect>> {
    match id {
        "utility" => Some(Box::new(utility::Utility::default())),
        "imager" => Some(Box::new(imager::Imager::default())),
        "channel" => Some(Box::new(channel::Channel::default())),
        "eq" => Some(Box::new(eq::Equalizer::default())),
        "vintage_eq" => Some(Box::new(vintage_eq::VintageEq::default())),
        "filter" => Some(Box::new(filter::Filter::default())),
        "compressor" => Some(Box::new(compressor::Compressor::default())),
        "gate" => Some(Box::new(gate::Gate::default())),
        "transient" => Some(Box::new(transient::Transient::default())),
        "deesser" => Some(Box::new(deesser::DeEsser::default())),
        "limiter" => Some(Box::new(limiter::Limiter::default())),
        "maximizer" => Some(Box::new(maximizer::Maximizer::default())),
        "mb_comp" => Some(Box::new(multiband::MultibandComp::default())),
        "reverb" => Some(Box::new(reverb::Reverb::default())),
        "delay" => Some(Box::new(delay::Delay::default())),
        "analyzer" => Some(Box::new(analyzer::Analyzer::default())),
        "drums" => Some(Box::new(drums::Drums::default())),
        "synth" => Some(Box::new(synth::Synth::default())),
        "amp_rig" => Some(Box::new(rig::Rig::default())),
        "tuner" => Some(Box::new(tuner::Tuner::default())),
        "pitch_correct" => Some(Box::new(pitch_correct::PitchCorrect::default())),
        "drive" => Some(Box::new(drive::Drive::default())),
        "tape" => Some(Box::new(tape::Tape::default())),
        "chorus" => Some(Box::new(chorus::Chorus::default())),
        "flanger" => Some(Box::new(flanger::Flanger::default())),
        "phaser" => Some(Box::new(phaser::Phaser::default())),
        "modulator" => Some(Box::new(modulator::Modulator::default())),
        "pitch" => Some(Box::new(pitch::Pitch::default())),
        "doubler" => Some(Box::new(doubler::Doubler::default())),
        "trigger" => Some(Box::new(trigger::Trigger::default())),
        _ => None,
    }
}

/// Converts decibels to a linear gain.
#[inline]
pub fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_listed_effect_can_be_created() {
        for info in EFFECTS {
            let mut effect = create(info.id).unwrap();
            effect.prepare(48_000, 64);
            for (i, p) in info.params.iter().enumerate() {
                assert!(p.min <= p.default && p.default <= p.max, "{}", p.id);
                effect.set_param(i, p.default);
            }
            let (mut l, mut r) = (vec![0.25; 64], vec![-0.25; 64]);
            effect.process(&mut l, &mut r);
            assert!(l.iter().chain(&r).all(|s| s.is_finite()));
        }
        assert!(create("nope").is_none());
    }

    #[test]
    fn every_effect_has_a_family_and_a_description() {
        for info in EFFECTS {
            // The channel strip lives on the strips, not in the browser.
            let family = CATEGORIES.contains(&info.category) || info.id == "channel";
            assert!(family, "{}", info.id);
            assert!(info.description.len() > 10, "{}", info.id);
        }
    }

    #[test]
    fn trait_defaults_and_shared_helpers() {
        let mut effect = create("utility").unwrap();
        effect.prepare(48_000, 4);
        effect.reset();
        effect.set_tempo(90.0);
        effect.midi(&MidiEvent::note_on(0, 0, 60, 100));
        assert_eq!(effect.latency(), 0);
        let mut meters = [7.0];
        create("chorus").unwrap().meters(&mut meters);
        assert_eq!(meters, [7.0], "no meters by default");
        let (mut l, mut r) = ([0.5f32; 4], [0.5f32; 4]);
        effect.process_keyed(&mut l, &mut r, (&[1.0; 4], &[1.0; 4]));
        assert_eq!(l, [0.5; 4], "the key is ignored without sidechain");
        assert!(effect.load("ir", FileData::Data(Vec::new())).is_err());
        assert!(supports_sidechain("gate") && !supports_sidechain("reverb"));
        assert_eq!(info("delay").unwrap().name, "Strada-Delay");

        assert_eq!(key_peak(None, 0, -0.5, 0.25), 0.5);
        assert_eq!(key_peak(Some((&[0.1], &[-0.3])), 0, 1.0, 1.0), 0.3);
        assert_eq!(key_peak(Some((&[0.1], &[])), 3, 1.0, 1.0), 0.0);
        assert_eq!(time_coefficient(0.0, 48_000.0), 0.0);
        assert!((time_coefficient(1.0, 1000.0) - (-1.0f32).exp()).abs() < 1e-6);
        assert!(
            (pole(1000.0, 48_000.0) + lowpass_coefficient(1000.0, 48_000.0) - 1.0).abs() < 1e-6
        );
        assert_eq!(
            lowpass_coefficient(40_000.0, 48_000.0),
            lowpass_coefficient(21_600.0, 48_000.0),
            "kept below Nyquist"
        );
        assert!((gain_to_db(db_to_gain(-12.0)) + 12.0).abs() < 1e-4);
        assert_eq!(gain_to_db(0.0), -200.0);
    }
}
