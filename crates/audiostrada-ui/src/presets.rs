//! Effect presets: factory settings for the built-in effects, and user presets saved as JSON
//! files per effect (parameters for built-ins, the plugin's own state for plugins).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::i18n::tr;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Preset {
    pub name: String,
    #[serde(default)]
    pub params: BTreeMap<String, f32>,
    /// Plugin state, base64.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    #[serde(skip)]
    pub factory: bool,
}

/// Where user presets live: one folder per effect under the configuration folder.
pub fn root() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("audiostrada").join("presets"))
}

fn folder(root: &Path, effect: &str) -> PathBuf {
    let name: String = effect
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || "-_.".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    root.join(name)
}

fn file_name(name: &str) -> String {
    let clean: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || " -_".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("{}.json", clean.trim())
}

/// Factory presets first, then the user's, sorted by name.
pub fn list(root: Option<&Path>, effect: &str) -> Vec<Preset> {
    let mut all = factory(effect);
    let mut user: Vec<Preset> = root
        .map(|r| folder(r, effect))
        .and_then(|dir| std::fs::read_dir(dir).ok())
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| std::fs::read(e.path()).ok())
        .filter_map(|bytes| serde_json::from_slice::<Preset>(&bytes).ok())
        .map(|mut p| {
            // Saved by the eight-band EQ of earlier versions.
            if effect == "eq" {
                audiostrada_core::legacy::upgrade_eq(&mut p.params);
            }
            p
        })
        .collect();
    user.sort_by_key(|p| p.name.to_lowercase());
    all.extend(user);
    all
}

pub fn save(root: &Path, effect: &str, preset: &Preset) -> Result<(), String> {
    if preset.name.trim().is_empty() {
        return Err(tr("give the preset a name").into());
    }
    let dir = folder(root, effect);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let bytes = serde_json::to_vec_pretty(preset).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(file_name(&preset.name)), bytes).map_err(|e| e.to_string())
}

pub fn delete(root: &Path, effect: &str, name: &str) -> Result<(), String> {
    std::fs::remove_file(folder(root, effect).join(file_name(name))).map_err(|e| e.to_string())
}

const BELL: f32 = 0.0;
const LOW_SHELF: f32 = 1.0;
const HIGH_SHELF: f32 = 2.0;
const LOW_CUT: f32 = 3.0;
const HIGH_CUT: f32 = 4.0;

/// An EQ preset from its bands: type, frequency, gain and Q.
fn eq_preset(name: &str, bands: &[(f32, f32, f32, f32)]) -> Preset {
    let mut params = std::collections::BTreeMap::new();
    for (b, &(kind, freq, gain, q)) in bands.iter().enumerate() {
        for (field, v) in [
            ("used", 1.0),
            ("type", kind),
            ("freq", freq),
            ("gain", gain),
            ("q", q),
        ] {
            params.insert(audiostrada_dsp::eq::param_id(b, field), v);
        }
    }
    Preset {
        name: name.to_owned(),
        params,
        state: None,
        factory: true,
    }
}

pub(crate) fn preset(name: &str, values: &[(&str, f32)]) -> Preset {
    Preset {
        name: name.to_owned(),
        params: values.iter().map(|&(k, v)| (k.to_owned(), v)).collect(),
        state: None,
        factory: true,
    }
}

/// Starting points for the built-in effects; parameters not listed keep their defaults.
/// Names read "Role: character", the role being the source, the bus or the job.
fn factory(effect: &str) -> Vec<Preset> {
    match effect {
        "synth" => crate::synth_editor::factory(),
        "drums" => crate::drums_editor::factory(),
        "trigger" => crate::trigger_editor::factory(),
        "eq" => vec![
            preset(tr("Default: flat"), &[]),
            eq_preset(
                tr("Vocal: presence"),
                &[
                    (LOW_CUT, 90.0, 0.0, 0.707),
                    (BELL, 300.0, -2.5, 1.0),
                    (BELL, 3500.0, 2.5, 0.8),
                    (HIGH_SHELF, 10_000.0, 2.0, 0.707),
                ],
            ),
            eq_preset(
                tr("Vocal: air 12 kHz"),
                &[
                    (LOW_CUT, 80.0, 0.0, 0.707),
                    (HIGH_SHELF, 12_000.0, 4.0, 0.707),
                ],
            ),
            eq_preset(
                tr("Kick: punch"),
                &[
                    (LOW_CUT, 30.0, 0.0, 0.707),
                    (LOW_SHELF, 60.0, 3.0, 0.707),
                    (BELL, 350.0, -4.0, 1.5),
                    (BELL, 4000.0, 3.0, 1.0),
                ],
            ),
            eq_preset(
                tr("Snare: crack"),
                &[
                    (LOW_CUT, 80.0, 0.0, 0.707),
                    (BELL, 200.0, 2.0, 1.2),
                    (BELL, 900.0, -2.5, 2.0),
                    (BELL, 5000.0, 3.0, 1.0),
                ],
            ),
            eq_preset(
                tr("Overheads: cymbal air"),
                &[
                    (LOW_CUT, 200.0, 0.0, 0.707),
                    (BELL, 400.0, -2.0, 1.0),
                    (HIGH_SHELF, 12_000.0, 2.5, 0.707),
                ],
            ),
            eq_preset(
                tr("Bass: definition"),
                &[
                    (LOW_CUT, 35.0, 0.0, 0.707),
                    (BELL, 80.0, 2.0, 1.0),
                    (BELL, 250.0, -3.0, 1.2),
                    (BELL, 800.0, 2.0, 1.5),
                ],
            ),
            eq_preset(
                tr("Guitar: fit in the mix"),
                &[
                    (LOW_CUT, 100.0, 0.0, 0.707),
                    (BELL, 400.0, -2.0, 1.2),
                    (BELL, 2500.0, 2.0, 1.0),
                    (HIGH_CUT, 9000.0, 0.0, 0.707),
                ],
            ),
            eq_preset(
                tr("Acoustic guitar: sparkle"),
                &[
                    (LOW_CUT, 90.0, 0.0, 0.707),
                    (BELL, 250.0, -3.0, 1.0),
                    (HIGH_SHELF, 10_000.0, 3.0, 0.707),
                ],
            ),
            eq_preset(
                tr("Mix bus: smile"),
                &[
                    (LOW_SHELF, 80.0, 1.5, 0.707),
                    (BELL, 300.0, -1.0, 0.8),
                    (HIGH_SHELF, 12_000.0, 1.5, 0.707),
                ],
            ),
            eq_preset(tr("Fix: low cut 100 Hz"), &[(LOW_CUT, 100.0, 0.0, 0.707)]),
            eq_preset(
                tr("FX: telephone"),
                &[
                    (LOW_CUT, 400.0, 0.0, 0.707),
                    (BELL, 1500.0, 6.0, 1.0),
                    (HIGH_CUT, 3000.0, 0.0, 0.707),
                ],
            ),
        ],
        "compressor" => vec![
            preset(
                tr("Vocal: gentle"),
                &[
                    ("threshold", -20.0),
                    ("ratio", 3.0),
                    ("attack", 8.0),
                    ("release", 120.0),
                    ("knee", 6.0),
                    ("makeup", 3.0),
                ],
            ),
            preset(
                tr("Vocal: smooth opto"),
                &[
                    ("type", 2.0),
                    ("threshold", -20.0),
                    ("ratio", 3.0),
                    ("makeup", 5.0),
                    ("color", 35.0),
                    ("sc_hpf", 60.0),
                ],
            ),
            preset(
                tr("Vocal: radio-ready"),
                &[
                    ("type", 3.0),
                    ("threshold", -22.0),
                    ("ratio", 4.0),
                    ("attack", 3.0),
                    ("release", 100.0),
                    ("makeup", 6.0),
                    ("color", 40.0),
                    ("sc_hpf", 100.0),
                ],
            ),
            preset(
                tr("Bass: control"),
                &[
                    ("threshold", -18.0),
                    ("ratio", 4.0),
                    ("attack", 15.0),
                    ("release", 150.0),
                    ("makeup", 3.0),
                ],
            ),
            preset(
                tr("Bass: opto leveler"),
                &[
                    ("type", 2.0),
                    ("threshold", -18.0),
                    ("ratio", 4.0),
                    ("makeup", 4.0),
                    ("color", 45.0),
                    ("sc_hpf", 60.0),
                ],
            ),
            preset(
                tr("Kick: punch"),
                &[
                    ("threshold", -18.0),
                    ("ratio", 4.0),
                    ("attack", 25.0),
                    ("release", 80.0),
                    ("knee", 3.0),
                    ("makeup", 3.0),
                ],
            ),
            preset(
                tr("Snare: crack"),
                &[
                    ("type", 3.0),
                    ("threshold", -20.0),
                    ("ratio", 8.0),
                    ("attack", 2.0),
                    ("release", 120.0),
                    ("makeup", 5.0),
                    ("color", 50.0),
                    ("sc_hpf", 60.0),
                ],
            ),
            preset(
                tr("Drum bus: glue"),
                &[
                    ("type", 1.0),
                    ("threshold", -16.0),
                    ("ratio", 4.0),
                    ("attack", 30.0),
                    ("auto", 1.0),
                    ("release", 300.0),
                    ("makeup", 3.0),
                    ("color", 30.0),
                    ("sc_hpf", 90.0),
                ],
            ),
            preset(
                tr("Drum bus: smash"),
                &[
                    ("type", 1.0),
                    ("threshold", -24.0),
                    ("ratio", 4.0),
                    ("attack", 3.0),
                    ("auto", 1.0),
                    ("release", 300.0),
                    ("makeup", 6.0),
                    ("color", 40.0),
                    ("sc_hpf", 60.0),
                    ("mix", 55.0),
                ],
            ),
            preset(
                tr("Drums: parallel crush"),
                &[
                    ("threshold", -35.0),
                    ("ratio", 10.0),
                    ("attack", 1.0),
                    ("release", 80.0),
                    ("makeup", 12.0),
                    ("mix", 35.0),
                ],
            ),
            preset(
                tr("Room mics: all buttons"),
                &[
                    ("type", 3.0),
                    ("threshold", -26.0),
                    ("ratio", 20.0),
                    ("attack", 0.2),
                    ("release", 200.0),
                    ("makeup", 8.0),
                    ("color", 70.0),
                    ("sc_hpf", 60.0),
                    ("mix", 60.0),
                ],
            ),
            preset(
                tr("Mix bus: glue"),
                &[
                    ("type", 1.0),
                    ("threshold", -14.0),
                    ("ratio", 2.0),
                    ("attack", 30.0),
                    ("auto", 1.0),
                    ("release", 300.0),
                    ("makeup", 2.0),
                    ("color", 20.0),
                    ("sc_hpf", 90.0),
                ],
            ),
        ],
        "reverb" => vec![
            preset(
                tr("Vocal: plate"),
                &[
                    ("type", 3.0),
                    ("decay", 1.8),
                    ("predelay", 25.0),
                    ("treble", 40.0),
                    ("low_cut", 200.0),
                    ("mix", 22.0),
                ],
            ),
            preset(
                tr("Vocal: ducked hall"),
                &[
                    ("type", 1.0),
                    ("decay", 2.2),
                    ("predelay", 30.0),
                    ("low_cut", 200.0),
                    ("duck", 60.0),
                    ("mix", 30.0),
                ],
            ),
            preset(
                tr("Drums: small room"),
                &[
                    ("type", 0.0),
                    ("size", 40.0),
                    ("decay", 0.6),
                    ("predelay", 5.0),
                    ("early", 60.0),
                    ("mix", 18.0),
                ],
            ),
            preset(
                tr("Drums: live room"),
                &[
                    ("type", 0.0),
                    ("size", 75.0),
                    ("decay", 1.1),
                    ("predelay", 10.0),
                    ("early", 55.0),
                    ("mix", 22.0),
                ],
            ),
            preset(
                tr("Snare: bright plate"),
                &[
                    ("type", 3.0),
                    ("decay", 1.2),
                    ("predelay", 5.0),
                    ("treble", 70.0),
                    ("high_cut", 14_000.0),
                    ("diffusion", 85.0),
                    ("mix", 18.0),
                ],
            ),
            preset(
                tr("Keys: dark plate"),
                &[
                    ("type", 3.0),
                    ("decay", 3.0),
                    ("treble", 25.0),
                    ("high_cut", 6000.0),
                    ("mix", 25.0),
                ],
            ),
            preset(
                tr("Strings: concert hall"),
                &[
                    ("type", 1.0),
                    ("size", 70.0),
                    ("decay", 2.6),
                    ("predelay", 25.0),
                    ("bass", 130.0),
                    ("mix", 25.0),
                ],
            ),
            preset(
                tr("Guitar: amp spring"),
                &[
                    ("type", 4.0),
                    ("decay", 1.5),
                    ("predelay", 0.0),
                    ("treble", 45.0),
                    ("high_cut", 5000.0),
                    ("mix", 22.0),
                ],
            ),
            preset(
                tr("Guitar: surf spring"),
                &[
                    ("type", 4.0),
                    ("decay", 2.5),
                    ("predelay", 0.0),
                    ("treble", 60.0),
                    ("high_cut", 6000.0),
                    ("low_cut", 150.0),
                    ("mix", 35.0),
                ],
            ),
            preset(
                tr("Pads: vast space"),
                &[
                    ("type", 2.0),
                    ("size", 90.0),
                    ("decay", 7.0),
                    ("predelay", 45.0),
                    ("treble", 35.0),
                    ("bass", 140.0),
                    ("mix", 30.0),
                ],
            ),
            preset(
                tr("Pads: shimmer"),
                &[
                    ("type", 5.0),
                    ("decay", 8.0),
                    ("predelay", 40.0),
                    ("shimmer", 60.0),
                    ("modulation", 60.0),
                    ("mix", 40.0),
                ],
            ),
            preset(
                tr("Ambient: long shimmer"),
                &[
                    ("type", 5.0),
                    ("decay", 14.0),
                    ("predelay", 60.0),
                    ("shimmer", 85.0),
                    ("high_cut", 12_000.0),
                    ("mix", 50.0),
                ],
            ),
        ],
        "delay" => vec![
            preset(
                tr("Vocal: slapback 90 ms"),
                &[
                    ("time", 90.0),
                    ("feedback", 5.0),
                    ("low_cut", 200.0),
                    ("high_cut", 6000.0),
                    ("mix", 18.0),
                ],
            ),
            preset(
                tr("Vocal: analog slap 110 ms"),
                &[
                    ("time", 110.0),
                    ("feedback", 15.0),
                    ("model", 2.0),
                    ("wow", 20.0),
                    ("mix", 25.0),
                ],
            ),
            preset(
                tr("Vocal: dark 1/8 echo"),
                &[
                    ("sync", 1.0),
                    ("note", 5.0),
                    ("feedback", 25.0),
                    ("low_cut", 300.0),
                    ("high_cut", 3500.0),
                    ("mix", 15.0),
                ],
            ),
            preset(
                tr("Vocal: 1/4 throw"),
                &[
                    ("sync", 1.0),
                    ("note", 8.0),
                    ("feedback", 30.0),
                    ("model", 1.0),
                    ("wow", 15.0),
                    ("drive", 25.0),
                    ("low_cut", 300.0),
                    ("high_cut", 5000.0),
                    ("mix", 20.0),
                ],
            ),
            preset(
                tr("Guitar: dotted eighth"),
                &[
                    ("sync", 1.0),
                    ("note", 6.0),
                    ("feedback", 35.0),
                    ("low_cut", 150.0),
                    ("high_cut", 7000.0),
                    ("mix", 25.0),
                ],
            ),
            preset(
                tr("Guitar: tape echo 330 ms"),
                &[
                    ("time", 330.0),
                    ("feedback", 45.0),
                    ("model", 1.0),
                    ("wow", 35.0),
                    ("drive", 40.0),
                    ("high_cut", 5000.0),
                    ("mix", 30.0),
                ],
            ),
            preset(
                tr("Lead: analog 1/8"),
                &[
                    ("sync", 1.0),
                    ("note", 5.0),
                    ("feedback", 30.0),
                    ("model", 2.0),
                    ("wow", 15.0),
                    ("mix", 22.0),
                ],
            ),
            preset(
                tr("Synth: 1/4 ping-pong"),
                &[
                    ("sync", 1.0),
                    ("note", 8.0),
                    ("pingpong", 1.0),
                    ("feedback", 40.0),
                    ("mix", 25.0),
                ],
            ),
            preset(
                tr("Drums: 1/16 ping-pong"),
                &[
                    ("sync", 1.0),
                    ("note", 2.0),
                    ("pingpong", 1.0),
                    ("feedback", 25.0),
                    ("low_cut", 400.0),
                    ("high_cut", 6000.0),
                    ("mix", 18.0),
                ],
            ),
            preset(
                tr("Dub: runaway tape"),
                &[
                    ("sync", 1.0),
                    ("note", 6.0),
                    ("feedback", 80.0),
                    ("model", 1.0),
                    ("wow", 40.0),
                    ("drive", 60.0),
                    ("low_cut", 300.0),
                    ("high_cut", 3000.0),
                    ("mix", 35.0),
                ],
            ),
        ],
        "gate" => vec![
            preset(
                tr("Kick: tight"),
                &[
                    ("threshold", -25.0),
                    ("range", -40.0),
                    ("attack", 0.1),
                    ("hold", 20.0),
                    ("release", 60.0),
                ],
            ),
            preset(
                tr("Snare: bleed control"),
                &[
                    ("threshold", -22.0),
                    ("range", -25.0),
                    ("attack", 0.2),
                    ("hold", 40.0),
                    ("release", 120.0),
                ],
            ),
            preset(
                tr("Toms: hits only"),
                &[
                    ("threshold", -30.0),
                    ("range", -30.0),
                    ("attack", 0.5),
                    ("hold", 80.0),
                    ("release", 250.0),
                ],
            ),
            preset(
                tr("Drums: tight"),
                &[
                    ("threshold", -30.0),
                    ("attack", 0.5),
                    ("hold", 30.0),
                    ("release", 80.0),
                ],
            ),
            // Placed after a reverb on the snare: the tail stops dead after the hold.
            preset(
                tr("Reverb: 80s gated"),
                &[
                    ("threshold", -30.0),
                    ("range", -80.0),
                    ("attack", 0.1),
                    ("hold", 250.0),
                    ("release", 20.0),
                ],
            ),
            preset(
                tr("Vocal: breath reducer"),
                &[
                    ("threshold", -45.0),
                    ("range", -12.0),
                    ("attack", 2.0),
                    ("hold", 50.0),
                    ("release", 200.0),
                ],
            ),
            preset(
                tr("Voice-over: room noise"),
                &[
                    ("threshold", -48.0),
                    ("range", -15.0),
                    ("attack", 5.0),
                    ("hold", 150.0),
                    ("release", 400.0),
                ],
            ),
            preset(
                tr("Guitar: high-gain silence"),
                &[
                    ("threshold", -50.0),
                    ("range", -40.0),
                    ("attack", 1.0),
                    ("hold", 30.0),
                    ("release", 150.0),
                ],
            ),
            preset(
                tr("Fix: noise floor"),
                &[("threshold", -55.0), ("range", -20.0), ("release", 300.0)],
            ),
        ],
        "limiter" => vec![
            preset(tr("Master: safety -1 dB"), &[("ceiling", -1.0)]),
            preset(
                tr("Master: streaming -1 dB"),
                &[("input", 3.0), ("ceiling", -1.0), ("release", 100.0)],
            ),
            preset(
                tr("Master: broadcast -2 dB"),
                &[("input", 2.0), ("ceiling", -2.0), ("release", 200.0)],
            ),
            preset(
                tr("Master: loud -0.3 dB"),
                &[("input", 6.0), ("ceiling", -0.3), ("release", 80.0)],
            ),
            preset(
                tr("Vocal: peak catcher"),
                &[("input", 2.0), ("ceiling", -6.0), ("release", 50.0)],
            ),
            preset(
                tr("Bass: even level"),
                &[("input", 4.0), ("ceiling", -6.0), ("release", 150.0)],
            ),
            preset(
                tr("Drum bus: peak shave"),
                &[("input", 3.0), ("ceiling", -3.0), ("release", 20.0)],
            ),
            preset(
                tr("Kick: clipper-style"),
                &[("input", 6.0), ("ceiling", -3.0), ("release", 5.0)],
            ),
        ],
        "drive" => vec![
            preset(
                tr("Mix bus: tape warmth"),
                &[
                    ("mode", 4.0),
                    ("drive", 6.0),
                    ("tone", 11_000.0),
                    ("level", -4.0),
                ],
            ),
            preset(
                tr("Keys: warm tube"),
                &[
                    ("mode", 3.0),
                    ("drive", 14.0),
                    ("tone", 7000.0),
                    ("level", -8.0),
                    ("bias", 30.0),
                ],
            ),
            preset(
                tr("Bass: thick transformer"),
                &[
                    ("mode", 5.0),
                    ("drive", 12.0),
                    ("tone", 9000.0),
                    ("level", -7.0),
                    ("bass", 2.0),
                ],
            ),
            preset(
                tr("Bass: parallel grit"),
                &[
                    ("mode", 0.0),
                    ("drive", 18.0),
                    ("tone", 2000.0),
                    ("mix", 40.0),
                ],
            ),
            preset(
                tr("Bass: growl"),
                &[
                    ("mode", 0.0),
                    ("drive", 22.0),
                    ("tone", 3000.0),
                    ("level", -14.0),
                    ("mid", 3.0),
                    ("mix", 60.0),
                ],
            ),
            preset(
                tr("Vocal: parallel grit"),
                &[
                    ("mode", 0.0),
                    ("drive", 20.0),
                    ("tone", 5000.0),
                    ("level", -12.0),
                    ("low_cut", 300.0),
                    ("mix", 25.0),
                ],
            ),
            preset(
                tr("Drums: parallel crunch"),
                &[
                    ("mode", 1.0),
                    ("drive", 24.0),
                    ("tone", 6000.0),
                    ("level", -16.0),
                    ("mix", 30.0),
                ],
            ),
            preset(
                tr("Guitar: light crunch"),
                &[
                    ("mode", 0.0),
                    ("drive", 12.0),
                    ("tone", 4000.0),
                    ("level", -8.0),
                ],
            ),
            preset(
                tr("Guitar: lead"),
                &[
                    ("mode", 1.0),
                    ("drive", 30.0),
                    ("tone", 3000.0),
                    ("level", -20.0),
                ],
            ),
            preset(
                tr("Guitar: tight metal"),
                &[
                    ("mode", 1.0),
                    ("drive", 36.0),
                    ("tone", 5000.0),
                    ("level", -24.0),
                    ("low_cut", 250.0),
                    ("mid", 4.0),
                ],
            ),
            preset(
                tr("Guitar: fuzz"),
                &[
                    ("mode", 2.0),
                    ("drive", 36.0),
                    ("tone", 2500.0),
                    ("level", -18.0),
                ],
            ),
            preset(
                tr("Guitar: octave fuzz"),
                &[
                    ("mode", 6.0),
                    ("drive", 30.0),
                    ("tone", 3500.0),
                    ("level", -16.0),
                    ("low_cut", 150.0),
                ],
            ),
        ],
        "chorus" => vec![
            preset(
                tr("Vocal: subtle thickening"),
                &[("rate", 0.5), ("depth", 25.0), ("mix", 35.0)],
            ),
            preset(
                tr("Acoustic guitar: 12-string"),
                &[
                    ("rate", 0.3),
                    ("depth", 20.0),
                    ("delay", 15.0),
                    ("mix", 30.0),
                ],
            ),
            preset(
                tr("Guitar: clean 80s"),
                &[
                    ("rate", 0.8),
                    ("depth", 50.0),
                    ("delay", 8.0),
                    ("mix", 50.0),
                ],
            ),
            preset(
                tr("Keys: electric piano"),
                &[
                    ("rate", 1.2),
                    ("depth", 45.0),
                    ("delay", 7.0),
                    ("mix", 45.0),
                ],
            ),
            preset(
                tr("Synth: lush ensemble"),
                &[
                    ("rate", 0.83),
                    ("depth", 70.0),
                    ("delay", 6.0),
                    ("mix", 50.0),
                ],
            ),
            preset(
                tr("Pads: wide"),
                &[
                    ("rate", 0.9),
                    ("depth", 60.0),
                    ("delay", 18.0),
                    ("mix", 50.0),
                ],
            ),
            preset(
                tr("Bass: fretless shimmer"),
                &[
                    ("rate", 0.6),
                    ("depth", 30.0),
                    ("delay", 10.0),
                    ("mix", 30.0),
                ],
            ),
            preset(
                tr("FX: vibrato"),
                &[
                    ("rate", 5.5),
                    ("depth", 40.0),
                    ("delay", 5.0),
                    ("mix", 100.0),
                ],
            ),
            preset(
                tr("FX: flanger-style"),
                &[
                    ("rate", 0.25),
                    ("depth", 80.0),
                    ("delay", 3.0),
                    ("feedback", 60.0),
                ],
            ),
        ],
        "maximizer" => vec![
            preset(
                tr("Master: acoustic, gentle"),
                &[
                    ("gain", 1.5),
                    ("ceiling", -1.0),
                    ("release", 500.0),
                    ("style", 0.0),
                ],
            ),
            preset(
                tr("Master: transparent -1 dBTP"),
                &[
                    ("gain", 3.0),
                    ("ceiling", -1.0),
                    ("release", 250.0),
                    ("style", 0.0),
                ],
            ),
            preset(
                tr("Master: streaming -1 dBTP"),
                &[
                    ("gain", 5.0),
                    ("ceiling", -1.0),
                    ("release", 150.0),
                    ("style", 0.0),
                ],
            ),
            preset(
                tr("Master: broadcast -2 dBTP"),
                &[
                    ("gain", 3.0),
                    ("ceiling", -2.0),
                    ("release", 200.0),
                    ("style", 0.0),
                ],
            ),
            preset(
                tr("Master: punchy pop"),
                &[
                    ("gain", 8.0),
                    ("ceiling", -1.0),
                    ("release", 90.0),
                    ("style", 1.0),
                ],
            ),
            preset(
                tr("Master: CD -0.3 dBTP"),
                &[
                    ("gain", 6.0),
                    ("ceiling", -0.3),
                    ("release", 100.0),
                    ("style", 1.0),
                ],
            ),
            preset(
                tr("Master: dense rock"),
                &[
                    ("gain", 9.0),
                    ("ceiling", -1.0),
                    ("release", 70.0),
                    ("style", 2.0),
                    ("link", 80.0),
                ],
            ),
            preset(
                tr("Master: EDM maximum"),
                &[
                    ("gain", 12.0),
                    ("ceiling", -0.5),
                    ("release", 60.0),
                    ("style", 2.0),
                ],
            ),
            preset(
                tr("Drum bus: peak control"),
                &[
                    ("gain", 4.0),
                    ("ceiling", -3.0),
                    ("release", 30.0),
                    ("style", 1.0),
                ],
            ),
        ],
        "tape" => vec![
            preset(
                tr("Mix bus: subtle 30 ips"),
                &[("drive", 1.0), ("speed", 2.0), ("bump", 1.0), ("wow", 5.0)],
            ),
            preset(
                tr("Mix bus: classic 15 ips"),
                &[("drive", 4.0), ("speed", 1.0), ("bump", 2.5), ("wow", 10.0)],
            ),
            preset(
                tr("Vocal: soft tape"),
                &[("drive", 3.0), ("speed", 2.0), ("bump", 0.0), ("wow", 5.0)],
            ),
            preset(
                tr("Keys: warm 7.5 ips"),
                &[("drive", 5.0), ("speed", 0.0), ("bump", 3.0), ("wow", 25.0)],
            ),
            preset(
                tr("Bass: fat 7.5 ips"),
                &[("drive", 6.0), ("speed", 0.0), ("bump", 4.0), ("wow", 8.0)],
            ),
            preset(
                tr("Drums: saturated"),
                &[
                    ("drive", 12.0),
                    ("speed", 1.0),
                    ("bump", 3.0),
                    ("bias", 30.0),
                ],
            ),
            preset(
                tr("Lo-fi: cassette"),
                &[
                    ("drive", 6.0),
                    ("speed", 0.0),
                    ("bump", 3.0),
                    ("wow", 60.0),
                    ("hiss", 35.0),
                ],
            ),
            preset(
                tr("Lo-fi: worn tape"),
                &[
                    ("drive", 8.0),
                    ("speed", 0.0),
                    ("bump", 3.0),
                    ("wow", 90.0),
                    ("hiss", 50.0),
                ],
            ),
        ],
        "flanger" => vec![
            preset(
                tr("Guitar: subtle motion"),
                &[
                    ("rate", 0.3),
                    ("depth", 40.0),
                    ("delay", 2.0),
                    ("feedback", 20.0),
                    ("mix", 30.0),
                ],
            ),
            preset(
                tr("Guitar: jet"),
                &[
                    ("rate", 0.15),
                    ("depth", 90.0),
                    ("delay", 0.8),
                    ("feedback", 75.0),
                    ("mix", 50.0),
                ],
            ),
            preset(
                tr("Vocal: psychedelic sweep"),
                &[
                    ("rate", 0.08),
                    ("depth", 70.0),
                    ("delay", 1.5),
                    ("feedback", 40.0),
                    ("mix", 35.0),
                ],
            ),
            preset(
                tr("Bass: slow flange"),
                &[
                    ("rate", 0.1),
                    ("depth", 50.0),
                    ("delay", 2.5),
                    ("feedback", 30.0),
                    ("stereo", 0.0),
                    ("mix", 35.0),
                ],
            ),
            preset(
                tr("Pads: wide motion"),
                &[
                    ("rate", 0.3),
                    ("depth", 40.0),
                    ("delay", 5.0),
                    ("feedback", 20.0),
                    ("stereo", 180.0),
                    ("mix", 40.0),
                ],
            ),
            preset(
                tr("Hi-hats: swirl"),
                &[
                    ("rate", 0.2),
                    ("depth", 80.0),
                    ("delay", 0.5),
                    ("feedback", 60.0),
                    ("stereo", 120.0),
                    ("mix", 50.0),
                ],
            ),
            preset(
                tr("Synth: metallic"),
                &[
                    ("rate", 0.5),
                    ("depth", 60.0),
                    ("delay", 4.0),
                    ("feedback", 85.0),
                    ("mix", 50.0),
                ],
            ),
            preset(
                tr("Drum bus: through-zero"),
                &[
                    ("rate", 0.1),
                    ("depth", 100.0),
                    ("delay", 0.1),
                    ("feedback", -60.0),
                    ("mix", 50.0),
                ],
            ),
        ],
        "phaser" => vec![
            preset(
                tr("Vocal: subtle movement"),
                &[
                    ("stages", 0.0),
                    ("rate", 0.2),
                    ("depth", 40.0),
                    ("feedback", 10.0),
                    ("mix", 25.0),
                ],
            ),
            preset(
                tr("Guitar: classic 4-stage"),
                &[("stages", 0.0), ("rate", 0.5), ("feedback", 30.0)],
            ),
            preset(
                tr("Guitar: funk rhythm"),
                &[
                    ("stages", 0.0),
                    ("rate", 1.5),
                    ("depth", 80.0),
                    ("feedback", 20.0),
                    ("mix", 50.0),
                ],
            ),
            preset(
                tr("Guitar: fast vibe"),
                &[
                    ("stages", 1.0),
                    ("rate", 4.5),
                    ("depth", 70.0),
                    ("feedback", 10.0),
                    ("mix", 60.0),
                ],
            ),
            preset(
                tr("Keys: electric piano swirl"),
                &[
                    ("stages", 1.0),
                    ("rate", 0.3),
                    ("depth", 70.0),
                    ("feedback", 40.0),
                    ("stereo", 120.0),
                    ("mix", 50.0),
                ],
            ),
            preset(
                tr("Synth: deep swirl"),
                &[
                    ("stages", 2.0),
                    ("rate", 0.2),
                    ("depth", 100.0),
                    ("feedback", 65.0),
                ],
            ),
            preset(
                tr("Pads: slow 12-stage"),
                &[
                    ("stages", 3.0),
                    ("rate", 0.05),
                    ("depth", 100.0),
                    ("feedback", 50.0),
                    ("stereo", 180.0),
                    ("mix", 50.0),
                ],
            ),
            preset(
                tr("Drums: breakbeat sweep"),
                &[
                    ("stages", 1.0),
                    ("rate", 0.15),
                    ("depth", 90.0),
                    ("low", 200.0),
                    ("high", 6000.0),
                    ("feedback", 60.0),
                    ("mix", 40.0),
                ],
            ),
        ],
        "imager" => vec![
            preset(
                tr("Master: mono bass 150 Hz"),
                &[
                    ("low_width", 100.0),
                    ("mid_width", 100.0),
                    ("high_width", 100.0),
                    ("mono_bass", 150.0),
                ],
            ),
            preset(
                tr("Master: wide top"),
                &[
                    ("low_width", 100.0),
                    ("mid_width", 115.0),
                    ("high_width", 135.0),
                    ("mono_bass", 120.0),
                ],
            ),
            preset(
                tr("Drum bus: wide overheads"),
                &[
                    ("low_width", 100.0),
                    ("mid_width", 110.0),
                    ("high_width", 140.0),
                    ("mono_bass", 100.0),
                ],
            ),
            preset(
                tr("Guitars: wide doubles"),
                &[
                    ("low_width", 100.0),
                    ("mid_width", 130.0),
                    ("high_width", 130.0),
                    ("low_split", 200.0),
                    ("mono_bass", 120.0),
                ],
            ),
            preset(
                tr("Bass: mono lows"),
                &[
                    ("low_width", 0.0),
                    ("mid_width", 100.0),
                    ("high_width", 120.0),
                    ("low_split", 250.0),
                    ("mono_bass", 250.0),
                ],
            ),
            preset(
                tr("Pads: super wide"),
                &[
                    ("low_width", 120.0),
                    ("mid_width", 160.0),
                    ("high_width", 180.0),
                    ("mono_bass", 200.0),
                ],
            ),
            preset(
                tr("Fix: tame a too-wide source"),
                &[
                    ("low_width", 70.0),
                    ("mid_width", 70.0),
                    ("high_width", 70.0),
                    ("mono_bass", 120.0),
                ],
            ),
            preset(
                tr("Check: mono"),
                &[("low_width", 0.0), ("mid_width", 0.0), ("high_width", 0.0)],
            ),
        ],
        "transient" => vec![
            preset(
                tr("Kick: punch"),
                &[("attack", 60.0), ("sustain", -20.0), ("speed", 8.0)],
            ),
            preset(
                tr("Snare: crack"),
                &[("attack", 50.0), ("sustain", 0.0), ("speed", 5.0)],
            ),
            preset(
                tr("Snare: fat sustain"),
                &[("attack", 10.0), ("sustain", 40.0), ("speed", 15.0)],
            ),
            preset(
                tr("Toms: shorter ring"),
                &[("attack", 20.0), ("sustain", -40.0), ("speed", 15.0)],
            ),
            preset(
                tr("Drums: tight"),
                &[("attack", 20.0), ("sustain", -60.0), ("speed", 12.0)],
            ),
            preset(
                tr("Drums: more room"),
                &[("attack", 0.0), ("sustain", 60.0), ("speed", 20.0)],
            ),
            preset(
                tr("Acoustic guitar: pick attack"),
                &[("attack", 35.0), ("sustain", -10.0), ("speed", 6.0)],
            ),
            preset(
                tr("Bass: pluck definition"),
                &[("attack", 30.0), ("sustain", 0.0), ("speed", 8.0)],
            ),
            preset(
                tr("Fix: soften attacks"),
                &[("attack", -60.0), ("sustain", 10.0)],
            ),
        ],
        "pitch_correct" => vec![
            preset(
                tr("Robot: hard tune"),
                &[("speed", 0.0), ("humanize", 0.0), ("amount", 100.0)],
            ),
            preset(
                tr("Pop vocal: tight"),
                &[("speed", 15.0), ("humanize", 20.0), ("amount", 100.0)],
            ),
            preset(
                tr("Natural: keeps the vibrato"),
                &[("speed", 60.0), ("humanize", 70.0), ("amount", 100.0)],
            ),
            preset(
                tr("Subtle: light touch"),
                &[("speed", 150.0), ("humanize", 60.0), ("amount", 60.0)],
            ),
            preset(
                tr("Backing vocals: locked"),
                &[("speed", 10.0), ("humanize", 0.0), ("amount", 100.0)],
            ),
            preset(
                tr("Octave down"),
                &[("speed", 20.0), ("humanize", 50.0), ("transpose", -12.0)],
            ),
        ],
        "deesser" => vec![
            preset(
                tr("Female vocal: standard"),
                &[("frequency", 7500.0), ("threshold", -32.0), ("range", 8.0)],
            ),
            preset(
                tr("Female vocal: bright and harsh"),
                &[("frequency", 8500.0), ("threshold", -36.0), ("range", 10.0)],
            ),
            preset(
                tr("Male vocal: standard"),
                &[("frequency", 5500.0), ("threshold", -30.0), ("range", 8.0)],
            ),
            preset(
                tr("Male vocal: gentle"),
                &[("frequency", 6000.0), ("threshold", -28.0), ("range", 4.0)],
            ),
            preset(
                tr("Vocal: wide-band, natural"),
                &[
                    ("frequency", 6500.0),
                    ("threshold", -34.0),
                    ("range", 6.0),
                    ("mode", 1.0),
                ],
            ),
            preset(
                tr("Backing vocals: strong"),
                &[("frequency", 7000.0), ("threshold", -38.0), ("range", 12.0)],
            ),
            preset(
                tr("Voice-over: broadcast"),
                &[("frequency", 6000.0), ("threshold", -32.0), ("range", 8.0)],
            ),
            preset(
                tr("Cymbals: harsh"),
                &[
                    ("frequency", 9000.0),
                    ("threshold", -28.0),
                    ("range", 6.0),
                    ("mode", 0.0),
                ],
            ),
        ],
        // The defaults boost lows and highs a little: each preset sets every boost and cut.
        "vintage_eq" => vec![
            preset(
                tr("Kick: low end trick"),
                &[
                    ("low_freq", 1.0),
                    ("low_boost", 6.0),
                    ("low_cut", 4.0),
                    ("high_boost", 0.0),
                ],
            ),
            preset(
                tr("Bass: low end trick 60 Hz"),
                &[
                    ("low_freq", 2.0),
                    ("low_boost", 5.0),
                    ("low_cut", 4.0),
                    ("high_boost", 0.0),
                    ("warmth", 30.0),
                ],
            ),
            preset(
                tr("Snare: snap"),
                &[
                    ("low_freq", 3.0),
                    ("low_boost", 3.0),
                    ("low_cut", 0.0),
                    ("high_freq", 2.0),
                    ("high_boost", 5.0),
                    ("bandwidth", 3.0),
                ],
            ),
            preset(
                tr("Vocal: presence 5 kHz"),
                &[
                    ("low_boost", 0.0),
                    ("low_cut", 0.0),
                    ("high_freq", 2.0),
                    ("high_boost", 3.0),
                    ("bandwidth", 4.0),
                ],
            ),
            preset(
                tr("Vocal: air"),
                &[
                    ("high_freq", 5.0),
                    ("high_boost", 4.0),
                    ("bandwidth", 7.0),
                    ("low_boost", 0.0),
                    ("low_cut", 0.0),
                ],
            ),
            preset(
                tr("Acoustic guitar: sparkle"),
                &[
                    ("low_freq", 3.0),
                    ("low_boost", 0.0),
                    ("low_cut", 3.0),
                    ("high_freq", 3.0),
                    ("high_boost", 4.0),
                    ("bandwidth", 6.0),
                ],
            ),
            preset(
                tr("Bus: warm and dark"),
                &[
                    ("low_boost", 3.0),
                    ("low_cut", 2.0),
                    ("high_boost", 0.0),
                    ("cut_freq", 1.0),
                    ("high_cut", 4.0),
                    ("warmth", 50.0),
                ],
            ),
            preset(
                tr("Master: sheen"),
                &[
                    ("low_freq", 0.0),
                    ("low_boost", 2.0),
                    ("low_cut", 1.0),
                    ("high_freq", 6.0),
                    ("high_boost", 2.5),
                    ("warmth", 30.0),
                ],
            ),
        ],
        "filter" => vec![
            preset(
                tr("FX: slow sweep"),
                &[
                    ("type", 0.0),
                    ("cutoff", 800.0),
                    ("resonance", 50.0),
                    ("lfo_depth", 70.0),
                    ("sync", 1.0),
                    ("note", 5.0),
                ],
            ),
            preset(
                tr("Transition: high-pass build-up"),
                &[("type", 1.0), ("cutoff", 200.0), ("resonance", 40.0)],
            ),
            preset(
                tr("Transition: low-pass breakdown"),
                &[("type", 0.0), ("cutoff", 400.0), ("resonance", 30.0)],
            ),
            preset(
                tr("Synth: 1/16 filter gate"),
                &[
                    ("type", 0.0),
                    ("cutoff", 600.0),
                    ("resonance", 40.0),
                    ("lfo_depth", 80.0),
                    ("sync", 1.0),
                    ("note", 0.0),
                ],
            ),
            preset(
                tr("Guitar: auto-wah"),
                &[
                    ("type", 2.0),
                    ("cutoff", 900.0),
                    ("resonance", 70.0),
                    ("envelope", 80.0),
                ],
            ),
            preset(
                tr("Bass: envelope funk"),
                &[
                    ("type", 0.0),
                    ("cutoff", 300.0),
                    ("resonance", 60.0),
                    ("drive", 4.0),
                    ("envelope", 70.0),
                ],
            ),
            preset(
                tr("Vocal: radio"),
                &[
                    ("type", 2.0),
                    ("cutoff", 1800.0),
                    ("resonance", 20.0),
                    ("drive", 8.0),
                ],
            ),
            preset(
                tr("Drums: next room"),
                &[("type", 0.0), ("cutoff", 700.0), ("resonance", 10.0)],
            ),
        ],
        "utility" => vec![
            preset(tr("Check: mono"), &[("mono", 1.0)]),
            preset(
                tr("Fix: flip polarity"),
                &[("invert_left", 1.0), ("invert_right", 1.0)],
            ),
            preset(tr("Fix: flip left polarity"), &[("invert_left", 1.0)]),
            preset(tr("Fix: flip right polarity"), &[("invert_right", 1.0)]),
            preset(tr("Fix: swap channels"), &[("swap", 1.0)]),
            preset(tr("Gain: -6 dB headroom"), &[("gain", -6.0)]),
            preset(tr("Gain: +6 dB"), &[("gain", 6.0)]),
            preset(tr("Width: narrow 50%"), &[("width", 50.0)]),
            preset(tr("Width: wide 130%"), &[("width", 130.0)]),
        ],
        "amp_rig" => vec![
            preset(
                tr("Clean"),
                &[
                    ("model", 1.0),
                    ("gain", 3.0),
                    ("treble", 6.0),
                    ("cab", 3.0),
                    ("p1_type", 5.0),
                    ("p1_k1", 4.0),
                    ("p2_type", 6.0),
                    ("p2_k1", 3.0),
                    ("p2_k3", 4.0),
                    ("p3_type", 11.0),
                    ("p3_k3", 3.0),
                ],
            ),
            preset(
                tr("Edge of breakup"),
                &[
                    ("model", 2.0),
                    ("gain", 6.0),
                    ("cab", 4.0),
                    ("p1_type", 13.0),
                    ("p1_k1", 3.0),
                    ("p1_k3", 6.0),
                    ("p2_type", 11.0),
                    ("p2_k3", 2.5),
                ],
            ),
            preset(
                tr("Jangle"),
                &[
                    ("model", 6.0),
                    ("gain", 4.0),
                    ("treble", 6.5),
                    ("cab", 2.0),
                    ("p1_type", 5.0),
                    ("p1_k1", 5.0),
                    ("p2_type", 9.0),
                    ("p2_k1", 4.0),
                    ("p2_k2", 3.0),
                    ("p3_type", 11.0),
                    ("p3_k3", 3.0),
                ],
            ),
            preset(
                tr("Blues"),
                &[
                    ("model", 7.0),
                    ("gain", 5.0),
                    ("cab", 1.0),
                    ("p1_type", 14.0),
                    ("p1_k1", 4.0),
                    ("p1_k3", 6.0),
                    ("p2_type", 20.0),
                    ("p2_k1", 3.5),
                    ("p2_k3", 2.5),
                ],
            ),
            preset(
                tr("Crunch"),
                &[
                    ("model", 8.0),
                    ("gain", 7.0),
                    ("mid", 6.0),
                    ("cab", 1.0),
                    ("p1_type", 2.0),
                    ("p1_k1", 3.0),
                    ("p1_k3", 7.0),
                    ("p2_type", 10.0),
                    ("p2_k2", 3.0),
                    ("p2_k3", 2.5),
                ],
            ),
            preset(
                tr("Lead"),
                &[
                    ("gate", -60.0),
                    ("model", 10.0),
                    ("gain", 7.0),
                    ("mid", 6.5),
                    ("presence", 6.0),
                    ("cab", 0.0),
                    ("p1_type", 2.0),
                    ("p1_k1", 2.0),
                    ("p1_k3", 7.0),
                    ("p2_type", 20.0),
                    ("p2_k1", 6.0),
                    ("p2_k3", 3.0),
                    ("p3_type", 11.0),
                    ("p3_k1", 6.0),
                    ("p3_k3", 2.5),
                ],
            ),
            preset(
                tr("Metal"),
                &[
                    ("gate", -50.0),
                    ("model", 13.0),
                    ("gain", 6.0),
                    ("bass", 6.0),
                    ("mid", 4.0),
                    ("treble", 6.0),
                    ("cab", 0.0),
                    ("p1_type", 2.0),
                    ("p1_k1", 0.0),
                    ("p1_k2", 6.0),
                    ("p1_k3", 8.0),
                ],
            ),
            preset(
                tr("Fuzz rock"),
                &[
                    ("model", 8.0),
                    ("gain", 4.0),
                    ("cab", 1.0),
                    ("p1_type", 16.0),
                    ("p1_k1", 7.0),
                    ("p1_k2", 4.0),
                    ("p2_type", 11.0),
                    ("p2_k3", 2.0),
                ],
            ),
            preset(
                tr("Bass"),
                &[
                    ("model", 15.0),
                    ("gain", 4.0),
                    ("bass", 6.5),
                    ("treble", 4.0),
                    ("cab", 7.0),
                    ("p1_type", 5.0),
                    ("p1_k1", 6.0),
                ],
            ),
        ],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_presets_round_trip() {
        let root = std::env::temp_dir().join(format!("audiostrada-presets-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        assert!(list(Some(&root), "eq").iter().all(|p| p.factory));
        let mine = Preset {
            name: "My / EQ".into(),
            params: [("p1_gain".to_owned(), 4.0)].into(),
            state: None,
            factory: false,
        };
        save(&root, "clap:org.example/eq", &mine).unwrap();
        let found = list(Some(&root), "clap:org.example/eq");
        assert_eq!(found, vec![mine]);
        delete(&root, "clap:org.example/eq", "My / EQ").unwrap();
        assert!(list(Some(&root), "clap:org.example/eq").is_empty());
        // Factory presets only name real parameters.
        for effect in audiostrada_dsp::EFFECTS {
            for p in factory(effect.id) {
                for key in p.params.keys() {
                    assert!(
                        effect.params.iter().any(|q| q.id == key),
                        "{} {key}",
                        effect.id
                    );
                }
            }
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn factory_presets_use_real_parameters() {
        for info in audiostrada_dsp::EFFECTS {
            for preset in factory(info.id) {
                for (id, value) in &preset.params {
                    let param = info
                        .params
                        .iter()
                        .find(|p| p.id == id)
                        .unwrap_or_else(|| panic!("{}: {} has no {id}", info.id, preset.name));
                    assert!(
                        (param.min..=param.max).contains(value),
                        "{}: {} {id} = {value}",
                        info.id,
                        preset.name
                    );
                }
            }
        }
    }
}
