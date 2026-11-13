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
    crate::config_dir().map(|d| d.join("audiostrada").join("presets"))
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
            // Saved before Strada-Amp's models were renumbered.
            if effect == "amp_rig" {
                audiostrada_core::legacy::upgrade_amp_rig(&mut p.params);
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
const NOTCH: f32 = 5.0;
const TILT: f32 = 7.0;

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

/// An EQ preset with other settings of its bands too (slopes, dynamics, placement).
fn eq_extra(name: &str, bands: &[(f32, f32, f32, f32)], extra: &[(&str, f32)]) -> Preset {
    let mut preset = eq_preset(name, bands);
    preset
        .params
        .extend(extra.iter().map(|&(k, v)| (k.to_owned(), v)));
    preset
}

/// A cutter preset: its 16 steps written as `x` (open) and `.` (closed).
fn cutter(name: &str, values: &[(&str, f32)], steps: &str) -> Preset {
    let mut preset = preset(name, values);
    for (i, c) in steps.chars().enumerate() {
        let open = if c == 'x' { 1.0 } else { 0.0 };
        preset.params.insert(format!("s{}", i + 1), open);
    }
    preset
}

/// The factory presets of a built-in effect, by name: the parameters each one sets.
pub fn factory_presets(effect: &str) -> Vec<(String, BTreeMap<String, f32>)> {
    builtin_or_instrument(effect)
        .into_iter()
        .map(|p| (p.name, p.params))
        .collect()
}

fn builtin_or_instrument(effect: &str) -> Vec<Preset> {
    match effect {
        "synth" => crate::synth_presets::factory(),
        "drums" => crate::drums_editor::factory(),
        "trigger" => crate::trigger_editor::factory(),
        _ => builtin(effect),
    }
}

/// Starting points for the built-in effects; parameters not listed keep their defaults.
/// Names read "Role: character", the role being the source, the bus or the job; each list
/// opens with the effect's defaults.
fn factory(effect: &str) -> Vec<Preset> {
    let mut list = match effect {
        "synth" => return crate::synth_presets::factory(),
        "drums" => return crate::drums_editor::factory(),
        "trigger" => return crate::trigger_editor::factory(),
        _ => builtin(effect),
    };
    if !list.is_empty() {
        list.insert(0, preset(tr("Default"), &[]));
    }
    list
}

fn builtin(effect: &str) -> Vec<Preset> {
    match effect {
        "channel" => vec![
            preset(
                tr("Vocal: pop lead"),
                &[
                    ("character", 2.0),
                    ("comp_type", 2.0),
                    ("comp_ratio", 3.0),
                    ("comp_makeup", 4.0),
                    ("hpf", 90.0),
                    ("lmf_freq", 300.0),
                    ("lmf_gain", -2.5),
                    ("lmf_q", 1.0),
                    ("hmf_freq", 3500.0),
                    ("hmf_gain", 2.5),
                    ("hmf_q", 0.8),
                    ("high_freq", 10_000.0),
                    ("high_gain", 2.5),
                    ("eq", 1.0),
                    ("filters", 1.0),
                    ("comp", 1.0),
                    ("comp_threshold", -21.0),
                    ("output", 1.0),
                ],
            ),
            preset(
                tr("Vocal: warm ballad"),
                &[
                    ("character", 1.0),
                    ("comp_type", 2.0),
                    ("comp_ratio", 2.5),
                    ("comp_makeup", 3.0),
                    ("drive", 1.0),
                    ("drive_amount", 20.0),
                    ("hpf", 80.0),
                    ("low_freq", 150.0),
                    ("low_gain", 1.0),
                    ("lmf_freq", 400.0),
                    ("lmf_gain", -1.5),
                    ("hmf_freq", 2500.0),
                    ("hmf_gain", 1.5),
                    ("high_freq", 12_000.0),
                    ("high_gain", 2.0),
                    ("eq", 1.0),
                    ("filters", 1.0),
                    ("comp", 1.0),
                    ("comp_threshold", -17.0),
                    ("output", 0.5),
                ],
            ),
            preset(
                tr("Vocal: rap, upfront"),
                &[
                    ("character", 2.0),
                    ("comp_type", 3.0),
                    ("comp_ratio", 6.0),
                    ("comp_attack", 2.0),
                    ("comp_release", 80.0),
                    ("comp_makeup", 7.0),
                    ("hpf", 100.0),
                    ("lmf_freq", 250.0),
                    ("lmf_gain", -3.0),
                    ("hmf_freq", 4000.0),
                    ("hmf_gain", 3.0),
                    ("high_freq", 10_000.0),
                    ("high_gain", 2.0),
                    ("eq", 1.0),
                    ("filters", 1.0),
                    ("comp", 1.0),
                    ("comp_threshold", -18.0),
                    ("output", 1.0),
                ],
            ),
            preset(
                tr("Vocal: backing, tucked in"),
                &[
                    ("character", 2.0),
                    ("comp_type", 2.0),
                    ("comp_ratio", 4.0),
                    ("comp_makeup", 5.0),
                    ("hpf", 150.0),
                    ("lmf_freq", 300.0),
                    ("lmf_gain", -3.0),
                    ("hmf_freq", 3000.0),
                    ("hmf_gain", -1.5),
                    ("high_freq", 12_000.0),
                    ("high_gain", 2.0),
                    ("eq", 1.0),
                    ("filters", 1.0),
                    ("comp", 1.0),
                    ("comp_threshold", -24.0),
                    ("output", 3.0),
                ],
            ),
            preset(
                tr("Vocal: voice-over, podcast"),
                &[
                    ("comp_type", 0.0),
                    ("comp_ratio", 3.0),
                    ("comp_attack", 5.0),
                    ("comp_release", 120.0),
                    ("comp_makeup", 6.0),
                    ("gate", 1.0),
                    ("gate_threshold", -50.0),
                    ("gate_range", -15.0),
                    ("gate_release", 300.0),
                    ("hpf", 80.0),
                    ("slope", 1.0),
                    ("lmf_freq", 250.0),
                    ("lmf_gain", -2.0),
                    ("hmf_freq", 4000.0),
                    ("hmf_gain", 2.0),
                    ("high_freq", 10_000.0),
                    ("high_gain", 1.5),
                    ("eq", 1.0),
                    ("filters", 1.0),
                    ("comp", 1.0),
                    ("comp_threshold", -18.0),
                    ("output", 1.0),
                ],
            ),
            preset(
                tr("Drums: kick punch"),
                &[
                    ("character", 3.0),
                    ("comp_type", 0.0),
                    ("comp_ratio", 4.0),
                    ("comp_attack", 25.0),
                    ("comp_release", 80.0),
                    ("comp_makeup", 3.0),
                    ("gate", 1.0),
                    ("gate_threshold", -30.0),
                    ("gate_range", -30.0),
                    ("gate_attack", 0.1),
                    ("gate_release", 80.0),
                    ("hpf", 30.0),
                    ("low_freq", 60.0),
                    ("low_gain", 3.0),
                    ("lmf_freq", 350.0),
                    ("lmf_gain", -4.0),
                    ("lmf_q", 1.5),
                    ("hmf_freq", 4000.0),
                    ("hmf_gain", 3.0),
                    ("eq", 1.0),
                    ("filters", 1.0),
                    ("comp", 1.0),
                    ("comp_threshold", -20.0),
                ],
            ),
            preset(
                tr("Drums: snare crack"),
                &[
                    ("character", 3.0),
                    ("comp_type", 3.0),
                    ("comp_ratio", 4.0),
                    ("comp_attack", 8.0),
                    ("comp_release", 100.0),
                    ("comp_makeup", 3.0),
                    ("gate", 1.0),
                    ("gate_threshold", -25.0),
                    ("gate_range", -20.0),
                    ("gate_attack", 0.2),
                    ("gate_release", 150.0),
                    ("hpf", 80.0),
                    ("low_bell", 1.0),
                    ("low_freq", 200.0),
                    ("low_gain", 2.0),
                    ("lmf_freq", 900.0),
                    ("lmf_gain", -2.5),
                    ("lmf_q", 2.0),
                    ("hmf_freq", 5000.0),
                    ("hmf_gain", 3.0),
                    ("eq", 1.0),
                    ("filters", 1.0),
                    ("comp", 1.0),
                    ("comp_threshold", -15.0),
                ],
            ),
            preset(
                tr("Drums: toms, full and clear"),
                &[
                    ("character", 3.0),
                    ("gate", 1.0),
                    ("gate_threshold", -30.0),
                    ("gate_range", -30.0),
                    ("gate_attack", 0.5),
                    ("gate_release", 300.0),
                    ("hpf", 60.0),
                    ("low_bell", 1.0),
                    ("low_freq", 100.0),
                    ("low_gain", 2.0),
                    ("lmf_freq", 500.0),
                    ("lmf_gain", -4.0),
                    ("lmf_q", 1.5),
                    ("hmf_freq", 4000.0),
                    ("hmf_gain", 2.0),
                    ("eq", 1.0),
                    ("filters", 1.0),
                ],
            ),
            preset(
                tr("Drums: overheads, open"),
                &[
                    ("character", 2.0),
                    ("comp_type", 1.0),
                    ("comp_ratio", 2.0),
                    ("comp_attack", 30.0),
                    ("comp_makeup", 1.5),
                    ("hpf", 150.0),
                    ("lmf_freq", 400.0),
                    ("lmf_gain", -2.0),
                    ("high_freq", 12_000.0),
                    ("high_gain", 2.5),
                    ("eq", 1.0),
                    ("filters", 1.0),
                    ("comp", 1.0),
                    ("comp_threshold", -19.0),
                    ("output", 2.5),
                ],
            ),
            preset(
                tr("Drums: bus, glue and weight"),
                &[
                    ("character", 3.0),
                    ("filters", 0.0),
                    ("comp_type", 1.0),
                    ("comp_ratio", 4.0),
                    ("comp_attack", 30.0),
                    ("comp_release", 300.0),
                    ("comp_makeup", 2.0),
                    ("drive", 1.0),
                    ("drive_amount", 20.0),
                    ("low_freq", 60.0),
                    ("low_gain", 1.5),
                    ("high_freq", 10_000.0),
                    ("high_gain", 1.5),
                    ("eq", 1.0),
                    ("comp", 1.0),
                    ("comp_threshold", -13.0),
                    ("output", -1.0),
                ],
            ),
            preset(
                tr("Drums: parallel crush"),
                &[
                    ("character", 3.0),
                    ("filters", 0.0),
                    ("eq", 0.0),
                    ("comp_type", 3.0),
                    ("comp_ratio", 20.0),
                    ("comp_attack", 0.5),
                    ("comp_release", 150.0),
                    ("comp_makeup", 10.0),
                    ("comp_mix", 40.0),
                    ("drive", 1.0),
                    ("drive_amount", 40.0),
                    ("comp", 1.0),
                    ("comp_threshold", -25.0),
                    ("output", 1.0),
                ],
            ),
            preset(
                tr("Bass: DI, round and present"),
                &[
                    ("character", 1.0),
                    ("comp_type", 2.0),
                    ("comp_ratio", 4.0),
                    ("comp_makeup", 4.0),
                    ("drive", 1.0),
                    ("drive_amount", 25.0),
                    ("hpf", 35.0),
                    ("low_bell", 1.0),
                    ("low_freq", 80.0),
                    ("low_gain", 2.0),
                    ("lmf_freq", 250.0),
                    ("lmf_gain", -3.0),
                    ("lmf_q", 1.2),
                    ("hmf_freq", 800.0),
                    ("hmf_gain", 2.0),
                    ("hmf_q", 1.5),
                    ("eq", 1.0),
                    ("filters", 1.0),
                    ("comp", 1.0),
                    ("comp_threshold", -22.0),
                    ("output", -0.5),
                ],
            ),
            preset(
                tr("Bass: pick, rock grit"),
                &[
                    ("character", 3.0),
                    ("comp_type", 3.0),
                    ("comp_ratio", 4.0),
                    ("comp_attack", 10.0),
                    ("comp_release", 100.0),
                    ("comp_makeup", 4.0),
                    ("drive", 1.0),
                    ("drive_amount", 50.0),
                    ("hpf", 40.0),
                    ("low_freq", 80.0),
                    ("low_gain", 1.5),
                    ("lmf_freq", 300.0),
                    ("lmf_gain", -2.0),
                    ("hmf_freq", 1200.0),
                    ("hmf_gain", 3.0),
                    ("hmf_q", 1.2),
                    ("high_freq", 4000.0),
                    ("high_gain", 1.5),
                    ("eq", 1.0),
                    ("filters", 1.0),
                    ("comp", 1.0),
                    ("comp_threshold", -18.0),
                    ("output", -1.5),
                ],
            ),
            preset(
                tr("Bass: sub, clean and deep"),
                &[
                    ("character", 0.0),
                    ("comp_type", 0.0),
                    ("comp_ratio", 3.0),
                    ("comp_attack", 20.0),
                    ("comp_release", 150.0),
                    ("comp_makeup", 2.0),
                    ("hpf", 25.0),
                    ("lpf", 6000.0),
                    ("low_bell", 1.0),
                    ("low_freq", 60.0),
                    ("low_gain", 2.0),
                    ("lmf_freq", 250.0),
                    ("lmf_gain", -2.0),
                    ("lmf_q", 1.2),
                    ("eq", 1.0),
                    ("filters", 1.0),
                    ("comp", 1.0),
                    ("comp_threshold", -15.0),
                    ("output", -1.0),
                ],
            ),
            preset(
                tr("Guitar: electric, in the mix"),
                &[
                    ("character", 1.0),
                    ("hpf", 100.0),
                    ("lpf", 9000.0),
                    ("lmf_freq", 400.0),
                    ("lmf_gain", -2.0),
                    ("lmf_q", 1.2),
                    ("hmf_freq", 2500.0),
                    ("hmf_gain", 2.0),
                    ("eq", 1.0),
                    ("filters", 1.0),
                    ("output", 1.0),
                ],
            ),
            preset(
                tr("Guitar: clean, sparkle"),
                &[
                    ("character", 4.0),
                    ("comp_type", 0.0),
                    ("comp_ratio", 3.0),
                    ("comp_attack", 15.0),
                    ("comp_release", 150.0),
                    ("comp_makeup", 3.0),
                    ("hpf", 90.0),
                    ("lmf_freq", 300.0),
                    ("lmf_gain", -2.0),
                    ("hmf_freq", 3000.0),
                    ("hmf_gain", 1.5),
                    ("high_freq", 8000.0),
                    ("high_gain", 3.0),
                    ("eq", 1.0),
                    ("filters", 1.0),
                    ("comp", 1.0),
                    ("comp_threshold", -19.0),
                    ("output", -1.0),
                ],
            ),
            preset(
                tr("Guitar: acoustic, strummed"),
                &[
                    ("character", 4.0),
                    ("comp_type", 2.0),
                    ("comp_ratio", 3.0),
                    ("comp_makeup", 3.0),
                    ("hpf", 90.0),
                    ("lmf_freq", 250.0),
                    ("lmf_gain", -3.0),
                    ("hmf_freq", 3000.0),
                    ("hmf_gain", 1.0),
                    ("high_freq", 10_000.0),
                    ("high_gain", 3.0),
                    ("eq", 1.0),
                    ("filters", 1.0),
                    ("comp", 1.0),
                    ("comp_threshold", -19.0),
                    ("output", 1.5),
                ],
            ),
            preset(
                tr("Guitar: acoustic, fingerpicked"),
                &[
                    ("character", 1.0),
                    ("comp_type", 2.0),
                    ("comp_ratio", 2.5),
                    ("comp_makeup", 2.0),
                    ("hpf", 70.0),
                    ("low_freq", 120.0),
                    ("low_gain", 1.5),
                    ("lmf_freq", 250.0),
                    ("lmf_gain", -1.5),
                    ("hmf_freq", 5000.0),
                    ("hmf_gain", 1.5),
                    ("high_freq", 12_000.0),
                    ("high_gain", 2.0),
                    ("eq", 1.0),
                    ("filters", 1.0),
                    ("comp", 1.0),
                    ("comp_threshold", -17.0),
                    ("output", 1.0),
                ],
            ),
            preset(
                tr("Keys: piano, clear"),
                &[
                    ("character", 4.0),
                    ("comp_type", 1.0),
                    ("comp_ratio", 2.0),
                    ("comp_attack", 30.0),
                    ("comp_makeup", 1.5),
                    ("hpf", 40.0),
                    ("lmf_freq", 300.0),
                    ("lmf_gain", -1.5),
                    ("hmf_freq", 3000.0),
                    ("hmf_gain", 1.5),
                    ("hmf_q", 0.8),
                    ("high_freq", 10_000.0),
                    ("high_gain", 2.0),
                    ("eq", 1.0),
                    ("filters", 1.0),
                    ("comp", 1.0),
                    ("comp_threshold", -14.0),
                    ("output", 0.5),
                ],
            ),
            preset(
                tr("Keys: electric piano, warm"),
                &[
                    ("character", 1.0),
                    ("comp_type", 2.0),
                    ("comp_ratio", 3.0),
                    ("comp_makeup", 2.0),
                    ("drive", 1.0),
                    ("drive_amount", 25.0),
                    ("hpf", 40.0),
                    ("lpf", 12_000.0),
                    ("low_freq", 100.0),
                    ("low_gain", 1.5),
                    ("hmf_freq", 2000.0),
                    ("hmf_gain", 1.5),
                    ("eq", 1.0),
                    ("filters", 1.0),
                    ("comp", 1.0),
                    ("comp_threshold", -17.0),
                    ("output", 0.0),
                ],
            ),
            preset(
                tr("Keys: synth, make room"),
                &[
                    ("hpf", 150.0),
                    ("slope", 1.0),
                    ("lmf_freq", 400.0),
                    ("lmf_gain", -2.0),
                    ("hmf_freq", 2500.0),
                    ("hmf_gain", -2.0),
                    ("hmf_q", 0.8),
                    ("high_freq", 10_000.0),
                    ("high_gain", 1.0),
                    ("eq", 1.0),
                    ("filters", 1.0),
                    ("output", 1.5),
                ],
            ),
        ],
        "eq" => vec![
            eq_extra(
                tr("Vocal: presence"),
                &[
                    (LOW_CUT, 90.0, 0.0, 0.707),
                    (BELL, 300.0, -2.5, 1.0),
                    (BELL, 3500.0, 2.5, 0.8),
                    (HIGH_SHELF, 10_000.0, 2.0, 0.707),
                ],
                &[("output", -1.5)],
            ),
            eq_extra(
                tr("Vocal: air 12 kHz"),
                &[
                    (LOW_CUT, 80.0, 0.0, 0.707),
                    (HIGH_SHELF, 12_000.0, 4.0, 0.707),
                ],
                &[("output", -0.5)],
            ),
            eq_extra(
                tr("Vocal: less mud"),
                &[(LOW_CUT, 100.0, 0.0, 0.707), (BELL, 300.0, -3.5, 1.0)],
                &[("b1_slope", 3.0)],
            ),
            eq_extra(
                tr("Vocal: soften harsh notes"),
                &[(LOW_CUT, 80.0, 0.0, 0.707), (BELL, 3200.0, 0.0, 2.0)],
                &[
                    ("b2_dyn", 1.0),
                    ("b2_thresh", -30.0),
                    ("b2_range", -5.0),
                    ("output", 1.5),
                ],
            ),
            eq_extra(
                tr("Vocal: backing, sit behind"),
                &[
                    (LOW_CUT, 150.0, 0.0, 0.707),
                    (BELL, 350.0, -2.0, 1.0),
                    (BELL, 3000.0, -2.0, 0.8),
                    (HIGH_SHELF, 12_000.0, 2.5, 0.707),
                ],
                &[("b1_slope", 3.0), ("output", 1.0)],
            ),
            eq_extra(
                tr("Vocal: voice-over, podcast"),
                &[
                    (LOW_CUT, 80.0, 0.0, 0.707),
                    (BELL, 250.0, -2.0, 1.0),
                    (BELL, 4000.0, 2.0, 0.8),
                    (HIGH_SHELF, 10_000.0, 1.5, 0.707),
                ],
                &[("b1_slope", 3.0), ("output", -1.0)],
            ),
            eq_extra(
                tr("Drums: kick punch"),
                &[
                    (LOW_CUT, 30.0, 0.0, 0.707),
                    (LOW_SHELF, 60.0, 3.0, 0.707),
                    (BELL, 350.0, -4.0, 1.5),
                    (BELL, 4000.0, 3.0, 1.0),
                ],
                &[("output", -0.5)],
            ),
            eq_extra(
                tr("Drums: kick deep sub"),
                &[
                    (LOW_CUT, 25.0, 0.0, 0.707),
                    (BELL, 55.0, 3.0, 1.2),
                    (BELL, 300.0, -5.0, 1.5),
                    (BELL, 3500.0, 3.0, 1.5),
                ],
                &[("b1_slope", 3.0), ("output", -1.5)],
            ),
            eq_extra(
                tr("Drums: snare crack"),
                &[
                    (LOW_CUT, 80.0, 0.0, 0.707),
                    (BELL, 200.0, 2.0, 1.2),
                    (BELL, 900.0, -2.5, 2.0),
                    (BELL, 5000.0, 3.0, 1.0),
                ],
                &[("output", -1.5)],
            ),
            eq_preset(
                tr("Drums: toms, full and clear"),
                &[
                    (LOW_CUT, 60.0, 0.0, 0.707),
                    (BELL, 100.0, 2.0, 1.0),
                    (BELL, 500.0, -4.0, 1.5),
                    (BELL, 4000.0, 2.0, 1.0),
                ],
            ),
            eq_extra(
                tr("Drums: overheads, cymbal air"),
                &[
                    (LOW_CUT, 150.0, 0.0, 0.707),
                    (BELL, 400.0, -2.0, 1.0),
                    (HIGH_SHELF, 12_000.0, 2.5, 0.707),
                ],
                &[("output", 2.5)],
            ),
            eq_preset(
                tr("Drums: bus, weight and air"),
                &[
                    (LOW_SHELF, 70.0, 1.5, 0.707),
                    (BELL, 400.0, -1.5, 0.8),
                    (HIGH_SHELF, 10_000.0, 2.0, 0.707),
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
            eq_extra(
                tr("Bass: sub weight"),
                &[
                    (LOW_CUT, 30.0, 0.0, 0.707),
                    (BELL, 60.0, 3.0, 1.0),
                    (BELL, 200.0, -2.0, 1.0),
                    (HIGH_CUT, 5000.0, 0.0, 0.707),
                ],
                &[("b1_slope", 3.0), ("output", -0.5)],
            ),
            eq_extra(
                tr("Bass: heard on small speakers"),
                &[
                    (LOW_CUT, 40.0, 0.0, 0.707),
                    (BELL, 250.0, -2.5, 1.0),
                    (BELL, 1000.0, 4.0, 1.2),
                    (BELL, 2500.0, 2.0, 1.0),
                ],
                &[("output", 0.5)],
            ),
            eq_extra(
                tr("Guitar: fit in the mix"),
                &[
                    (LOW_CUT, 100.0, 0.0, 0.707),
                    (BELL, 400.0, -2.0, 1.2),
                    (BELL, 2500.0, 2.0, 1.0),
                    (HIGH_CUT, 9000.0, 0.0, 0.707),
                ],
                &[("output", 1.0)],
            ),
            eq_extra(
                tr("Guitar: tame the fizz"),
                &[
                    (LOW_CUT, 90.0, 0.0, 0.707),
                    (BELL, 4000.0, -2.5, 2.0),
                    (HIGH_CUT, 7000.0, 0.0, 0.707),
                ],
                &[("b3_slope", 3.0)],
            ),
            eq_extra(
                tr("Guitar: acoustic, sparkle"),
                &[
                    (LOW_CUT, 90.0, 0.0, 0.707),
                    (BELL, 250.0, -3.0, 1.0),
                    (HIGH_SHELF, 10_000.0, 3.0, 0.707),
                ],
                &[("output", 1.5)],
            ),
            eq_extra(
                tr("Guitar: acoustic, less boom"),
                &[
                    (LOW_CUT, 80.0, 0.0, 0.707),
                    (BELL, 180.0, 0.0, 1.5),
                    (BELL, 5000.0, 1.0, 0.8),
                ],
                &[
                    ("b1_slope", 3.0),
                    ("b2_dyn", 1.0),
                    ("b2_thresh", -30.0),
                    ("b2_range", -6.0),
                    ("output", 2.0),
                ],
            ),
            eq_extra(
                tr("Keys: piano, clarity"),
                &[
                    (LOW_CUT, 40.0, 0.0, 0.707),
                    (BELL, 250.0, -2.0, 1.0),
                    (BELL, 3000.0, 1.5, 0.8),
                    (HIGH_SHELF, 10_000.0, 2.0, 0.707),
                ],
                &[("output", 0.5)],
            ),
            eq_extra(
                tr("Keys: synth, make room for the vocal"),
                &[
                    (LOW_CUT, 150.0, 0.0, 0.707),
                    (BELL, 400.0, -2.0, 1.0),
                    (BELL, 2500.0, -2.5, 0.8),
                ],
                &[("b1_slope", 3.0), ("output", 1.5)],
            ),
            eq_extra(
                tr("Keys: pad, soft and dark"),
                &[
                    (LOW_CUT, 100.0, 0.0, 0.707),
                    (HIGH_SHELF, 4000.0, -3.0, 0.707),
                    (HIGH_CUT, 9000.0, 0.0, 0.707),
                ],
                &[("output", 0.5)],
            ),
            eq_extra(
                tr("Master: gentle smile"),
                &[
                    (LOW_SHELF, 80.0, 1.5, 0.707),
                    (BELL, 300.0, -1.0, 0.8),
                    (HIGH_SHELF, 12_000.0, 1.5, 0.707),
                ],
                &[("output", 0.5)],
            ),
            eq_extra(
                tr("Master: tilt brighter"),
                &[(TILT, 1000.0, 2.0, 0.707)],
                &[("output", 0.5)],
            ),
            eq_extra(
                tr("Master: tilt warmer"),
                &[(TILT, 1000.0, -2.0, 0.707)],
                &[("output", -1.0)],
            ),
            eq_extra(
                tr("Master: clear the low mids"),
                &[
                    (LOW_CUT, 25.0, 0.0, 0.707),
                    (BELL, 250.0, -1.5, 0.7),
                    (HIGH_SHELF, 14_000.0, 1.0, 0.707),
                ],
                &[("b1_slope", 3.0), ("output", 0.5)],
            ),
            eq_extra(
                tr("Master: mono lows below 120 Hz"),
                &[(LOW_CUT, 120.0, 0.0, 0.707)],
                &[("b1_slope", 3.0), ("b1_place", 4.0)],
            ),
            eq_extra(
                tr("Master: wider air"),
                &[(HIGH_SHELF, 9000.0, 2.5, 0.707)],
                &[("b1_place", 4.0)],
            ),
            eq_preset(tr("Fix: low cut 100 Hz"), &[(LOW_CUT, 100.0, 0.0, 0.707)]),
            eq_extra(
                tr("Fix: rumble below 30 Hz"),
                &[(LOW_CUT, 30.0, 0.0, 0.707)],
                &[("b1_slope", 5.0)],
            ),
            eq_preset(
                tr("Fix: mains hum 50 Hz"),
                &[
                    (NOTCH, 50.0, 0.0, 10.0),
                    (NOTCH, 100.0, 0.0, 10.0),
                    (NOTCH, 150.0, 0.0, 10.0),
                ],
            ),
            eq_preset(
                tr("Fix: mains hum 60 Hz"),
                &[
                    (NOTCH, 60.0, 0.0, 10.0),
                    (NOTCH, 120.0, 0.0, 10.0),
                    (NOTCH, 180.0, 0.0, 10.0),
                ],
            ),
            eq_extra(
                tr("Fix: harsh 3 kHz, only when loud"),
                &[(BELL, 3000.0, 0.0, 2.5)],
                &[("b1_dyn", 1.0), ("b1_thresh", -40.0), ("b1_range", -6.0)],
            ),
            eq_extra(
                tr("FX: telephone"),
                &[
                    (LOW_CUT, 400.0, 0.0, 0.707),
                    (BELL, 1500.0, 6.0, 1.0),
                    (HIGH_CUT, 3000.0, 0.0, 0.707),
                ],
                &[("b1_slope", 3.0), ("b3_slope", 3.0)],
            ),
            eq_extra(
                tr("FX: old radio"),
                &[
                    (LOW_CUT, 250.0, 0.0, 0.707),
                    (BELL, 1200.0, 4.0, 0.8),
                    (HIGH_CUT, 5000.0, 0.0, 0.707),
                ],
                &[("b1_slope", 3.0), ("b3_slope", 3.0), ("output", -0.5)],
            ),
            eq_extra(
                tr("FX: next room"),
                &[
                    (LOW_SHELF, 150.0, 2.0, 0.707),
                    (HIGH_CUT, 500.0, 0.0, 0.707),
                ],
                &[("b2_slope", 3.0), ("output", 0.5)],
            ),
        ],
        "compressor" => vec![
            preset(
                tr("Vocal: gentle"),
                &[
                    ("type", 0.0),
                    ("ratio", 2.5),
                    ("attack", 10.0),
                    ("release", 120.0),
                    ("knee", 8.0),
                    ("sc_hpf", 80.0),
                    ("threshold", -13.0),
                    ("makeup", 4.5),
                ],
            ),
            preset(
                tr("Vocal: smooth opto"),
                &[
                    ("type", 2.0),
                    ("ratio", 3.0),
                    ("color", 30.0),
                    ("sc_hpf", 80.0),
                    ("threshold", -21.0),
                    ("makeup", 6.5),
                ],
            ),
            preset(
                tr("Vocal: radio-ready"),
                &[
                    ("type", 3.0),
                    ("ratio", 4.0),
                    ("attack", 4.0),
                    ("release", 100.0),
                    ("color", 35.0),
                    ("sc_hpf", 100.0),
                    ("threshold", -17.0),
                    ("makeup", 8.0),
                ],
            ),
            preset(
                tr("Vocal: rap, upfront"),
                &[
                    ("type", 3.0),
                    ("ratio", 8.0),
                    ("attack", 1.5),
                    ("release", 70.0),
                    ("color", 45.0),
                    ("sc_hpf", 100.0),
                    ("threshold", -20.0),
                    ("makeup", 11.0),
                ],
            ),
            preset(
                tr("Vocal: backing, tucked in"),
                &[
                    ("type", 2.0),
                    ("ratio", 5.0),
                    ("color", 25.0),
                    ("sc_hpf", 120.0),
                    ("threshold", -26.0),
                    ("makeup", 8.0),
                ],
            ),
            preset(
                tr("Vocal: voice-over, steady"),
                &[
                    ("type", 0.0),
                    ("ratio", 3.5),
                    ("attack", 5.0),
                    ("release", 150.0),
                    ("knee", 10.0),
                    ("sc_hpf", 80.0),
                    ("threshold", -17.0),
                    ("makeup", 8.0),
                ],
            ),
            preset(
                tr("Drums: kick punch"),
                &[
                    ("type", 0.0),
                    ("ratio", 4.0),
                    ("attack", 25.0),
                    ("release", 80.0),
                    ("auto", 0.0),
                    ("knee", 3.0),
                    ("threshold", -20.0),
                    ("makeup", 2.0),
                ],
            ),
            preset(
                tr("Drums: snare crack"),
                &[
                    ("type", 3.0),
                    ("ratio", 4.0),
                    ("attack", 8.0),
                    ("release", 90.0),
                    ("color", 40.0),
                    ("sc_hpf", 80.0),
                    ("threshold", -16.0),
                    ("makeup", 4.0),
                ],
            ),
            preset(
                tr("Drums: toms, full"),
                &[
                    ("type", 0.0),
                    ("ratio", 4.0),
                    ("attack", 15.0),
                    ("release", 180.0),
                    ("auto", 0.0),
                    ("knee", 4.0),
                    ("threshold", -17.0),
                    ("makeup", 2.0),
                ],
            ),
            preset(
                tr("Drums: overheads, tamed"),
                &[
                    ("type", 1.0),
                    ("ratio", 3.0),
                    ("attack", 10.0),
                    ("release", 200.0),
                    ("color", 20.0),
                    ("sc_hpf", 150.0),
                    ("threshold", -18.0),
                    ("makeup", 1.5),
                ],
            ),
            preset(
                tr("Drums: room, all buttons"),
                &[
                    ("type", 3.0),
                    ("ratio", 20.0),
                    ("attack", 0.3),
                    ("release", 200.0),
                    ("color", 70.0),
                    ("sc_hpf", 60.0),
                    ("mix", 70.0),
                    ("threshold", -24.0),
                    ("makeup", 11.0),
                ],
            ),
            preset(
                tr("Drums: parallel crush"),
                &[
                    ("type", 0.0),
                    ("ratio", 10.0),
                    ("attack", 1.0),
                    ("release", 80.0),
                    ("auto", 0.0),
                    ("knee", 0.0),
                    ("mix", 35.0),
                    ("makeup", 12.0),
                    ("threshold", -31.0),
                ],
            ),
            preset(
                tr("Bass: control"),
                &[
                    ("type", 0.0),
                    ("ratio", 4.0),
                    ("attack", 20.0),
                    ("release", 150.0),
                    ("knee", 6.0),
                    ("threshold", -17.0),
                    ("makeup", 3.5),
                ],
            ),
            preset(
                tr("Bass: opto leveler"),
                &[
                    ("type", 2.0),
                    ("ratio", 4.0),
                    ("color", 40.0),
                    ("sc_hpf", 40.0),
                    ("threshold", -24.0),
                    ("makeup", 5.0),
                ],
            ),
            preset(
                tr("Bass: slap tamer"),
                &[
                    ("type", 3.0),
                    ("ratio", 8.0),
                    ("attack", 1.0),
                    ("release", 80.0),
                    ("color", 25.0),
                    ("threshold", -16.0),
                    ("makeup", 3.0),
                ],
            ),
            preset(
                tr("Guitar: sustain"),
                &[
                    ("type", 0.0),
                    ("ratio", 6.0),
                    ("attack", 5.0),
                    ("release", 300.0),
                    ("auto", 0.0),
                    ("knee", 6.0),
                    ("threshold", -23.0),
                    ("makeup", 7.0),
                ],
            ),
            preset(
                tr("Guitar: funk snap"),
                &[
                    ("type", 3.0),
                    ("ratio", 4.0),
                    ("attack", 12.0),
                    ("release", 60.0),
                    ("color", 30.0),
                    ("threshold", -16.0),
                    ("makeup", 1.5),
                ],
            ),
            preset(
                tr("Guitar: acoustic, even strumming"),
                &[
                    ("type", 2.0),
                    ("ratio", 3.0),
                    ("color", 20.0),
                    ("sc_hpf", 80.0),
                    ("threshold", -22.0),
                    ("makeup", 4.5),
                ],
            ),
            preset(
                tr("Guitar: acoustic, fingerpicking"),
                &[
                    ("type", 0.0),
                    ("ratio", 2.5),
                    ("attack", 20.0),
                    ("release", 250.0),
                    ("knee", 10.0),
                    ("sc_hpf", 60.0),
                    ("threshold", -15.0),
                    ("makeup", 2.5),
                ],
            ),
            preset(
                tr("Keys: piano, gentle"),
                &[
                    ("type", 1.0),
                    ("ratio", 2.0),
                    ("attack", 30.0),
                    ("color", 15.0),
                    ("threshold", -13.0),
                    ("makeup", 1.5),
                ],
            ),
            preset(
                tr("Keys: piano, pop upfront"),
                &[
                    ("type", 0.0),
                    ("ratio", 4.0),
                    ("attack", 10.0),
                    ("release", 140.0),
                    ("knee", 6.0),
                    ("threshold", -16.0),
                    ("makeup", 4.5),
                ],
            ),
            preset(
                tr("Keys: electric piano, even"),
                &[
                    ("type", 2.0),
                    ("ratio", 3.0),
                    ("color", 30.0),
                    ("threshold", -19.0),
                    ("makeup", 3.5),
                ],
            ),
            preset(
                tr("Bus: drum glue"),
                &[
                    ("type", 1.0),
                    ("ratio", 4.0),
                    ("attack", 30.0),
                    ("release", 300.0),
                    ("color", 30.0),
                    ("sc_hpf", 90.0),
                    ("threshold", -14.0),
                    ("makeup", 2.0),
                ],
            ),
            preset(
                tr("Bus: drum smash"),
                &[
                    ("type", 1.0),
                    ("ratio", 4.0),
                    ("attack", 3.0),
                    ("release", 300.0),
                    ("color", 40.0),
                    ("sc_hpf", 60.0),
                    ("mix", 55.0),
                    ("threshold", -21.0),
                    ("makeup", 5.5),
                ],
            ),
            preset(
                tr("Bus: mix glue"),
                &[
                    ("type", 1.0),
                    ("ratio", 2.0),
                    ("attack", 30.0),
                    ("release", 300.0),
                    ("color", 20.0),
                    ("sc_hpf", 90.0),
                    ("threshold", -12.0),
                    ("makeup", 2.0),
                ],
            ),
            preset(
                tr("Bus: mix, light touch"),
                &[
                    ("type", 1.0),
                    ("ratio", 1.5),
                    ("attack", 30.0),
                    ("release", 600.0),
                    ("color", 10.0),
                    ("sc_hpf", 120.0),
                    ("threshold", -11.0),
                    ("makeup", 1.0),
                ],
            ),
            preset(
                tr("Bus: vocals together"),
                &[
                    ("type", 2.0),
                    ("ratio", 2.5),
                    ("color", 20.0),
                    ("sc_hpf", 100.0),
                    ("threshold", -17.0),
                    ("makeup", 3.5),
                ],
            ),
            preset(
                tr("Sidechain: duck under the kick"),
                &[
                    ("type", 0.0),
                    ("threshold", -30.0),
                    ("ratio", 6.0),
                    ("attack", 1.0),
                    ("release", 120.0),
                    ("auto", 0.0),
                    ("knee", 3.0),
                    ("range", 8.0),
                ],
            ),
            preset(
                tr("Sidechain: bass under the kick"),
                &[
                    ("type", 0.0),
                    ("threshold", -30.0),
                    ("ratio", 4.0),
                    ("attack", 0.5),
                    ("release", 70.0),
                    ("auto", 0.0),
                    ("knee", 3.0),
                    ("range", 4.0),
                ],
            ),
            preset(
                tr("Sidechain: pumping"),
                &[
                    ("type", 0.0),
                    ("threshold", -36.0),
                    ("ratio", 20.0),
                    ("attack", 0.5),
                    ("release", 200.0),
                    ("auto", 0.0),
                    ("knee", 0.0),
                    ("range", 18.0),
                ],
            ),
            preset(
                tr("Sidechain: slow breathing"),
                &[
                    ("type", 0.0),
                    ("threshold", -34.0),
                    ("ratio", 8.0),
                    ("attack", 5.0),
                    ("release", 450.0),
                    ("auto", 0.0),
                    ("knee", 6.0),
                    ("range", 10.0),
                    ("hold", 40.0),
                ],
            ),
        ],
        "reverb" => vec![
            preset(
                tr("Vocal: plate"),
                &[
                    ("type", 3.0),
                    ("decay", 2.0),
                    ("predelay", 30.0),
                    ("low_cut", 200.0),
                    ("treble", 40.0),
                    ("mix", 22.0),
                ],
            ),
            preset(
                tr("Vocal: plate, 1/16 pre-delay"),
                &[
                    ("type", 3.0),
                    ("decay", 2.2),
                    ("predelay_sync", 1.0),
                    ("predelay_note", 2.0),
                    ("low_cut", 220.0),
                    ("treble", 45.0),
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
                tr("Vocal: ballad hall"),
                &[
                    ("type", 1.0),
                    ("decay", 3.2),
                    ("predelay", 60.0),
                    ("low_cut", 250.0),
                    ("size", 75.0),
                    ("duck", 30.0),
                    ("mix", 25.0),
                ],
            ),
            preset(
                tr("Vocal: close ambience"),
                &[
                    ("type", 0.0),
                    ("decay", 0.4),
                    ("predelay", 0.0),
                    ("low_cut", 200.0),
                    ("size", 25.0),
                    ("early", 75.0),
                    ("mix", 15.0),
                ],
            ),
            preset(
                tr("Drums: small room"),
                &[
                    ("type", 0.0),
                    ("decay", 0.6),
                    ("predelay", 5.0),
                    ("size", 40.0),
                    ("early", 60.0),
                    ("mix", 18.0),
                ],
            ),
            preset(
                tr("Drums: live room"),
                &[
                    ("type", 0.0),
                    ("decay", 1.1),
                    ("predelay", 10.0),
                    ("size", 75.0),
                    ("early", 55.0),
                    ("mix", 22.0),
                ],
            ),
            preset(
                tr("Drums: tight ambience"),
                &[
                    ("type", 0.0),
                    ("decay", 0.3),
                    ("predelay", 0.0),
                    ("size", 20.0),
                    ("early", 85.0),
                    ("low_cut", 150.0),
                    ("mix", 15.0),
                ],
            ),
            preset(
                tr("Drums: snare, bright plate"),
                &[
                    ("type", 3.0),
                    ("decay", 1.2),
                    ("predelay", 5.0),
                    ("diffusion", 85.0),
                    ("high_cut", 14_000.0),
                    ("treble", 70.0),
                    ("mix", 18.0),
                ],
            ),
            preset(
                tr("Keys: dark plate"),
                &[
                    ("type", 3.0),
                    ("decay", 3.0),
                    ("high_cut", 6000.0),
                    ("treble", 25.0),
                    ("mix", 25.0),
                ],
            ),
            preset(
                tr("Keys: piano hall"),
                &[
                    ("type", 1.0),
                    ("decay", 2.2),
                    ("predelay", 20.0),
                    ("low_cut", 120.0),
                    ("size", 65.0),
                    ("mix", 22.0),
                ],
            ),
            preset(
                tr("Keys: electric piano, soft room"),
                &[
                    ("type", 0.0),
                    ("decay", 0.9),
                    ("predelay", 10.0),
                    ("size", 55.0),
                    ("high_cut", 7000.0),
                    ("treble", 35.0),
                    ("modulation", 55.0),
                    ("mix", 20.0),
                ],
            ),
            preset(
                tr("Guitar: amp spring"),
                &[
                    ("type", 4.0),
                    ("decay", 1.5),
                    ("predelay", 0.0),
                    ("high_cut", 5000.0),
                    ("treble", 45.0),
                    ("mix", 22.0),
                ],
            ),
            preset(
                tr("Guitar: surf spring"),
                &[
                    ("type", 4.0),
                    ("decay", 2.5),
                    ("predelay", 0.0),
                    ("low_cut", 150.0),
                    ("high_cut", 6000.0),
                    ("treble", 60.0),
                    ("mix", 35.0),
                ],
            ),
            preset(
                tr("Guitar: acoustic, wood room"),
                &[
                    ("type", 0.0),
                    ("decay", 0.8),
                    ("predelay", 8.0),
                    ("size", 50.0),
                    ("early", 50.0),
                    ("low_cut", 150.0),
                    ("high_cut", 10_000.0),
                    ("mix", 16.0),
                ],
            ),
            preset(
                tr("Pads: vast space"),
                &[
                    ("type", 2.0),
                    ("decay", 7.0),
                    ("predelay", 45.0),
                    ("size", 90.0),
                    ("treble", 35.0),
                    ("bass", 140.0),
                    ("mix", 30.0),
                ],
            ),
            preset(
                tr("Pads: strings, concert hall"),
                &[
                    ("type", 1.0),
                    ("decay", 2.6),
                    ("predelay", 25.0),
                    ("size", 70.0),
                    ("bass", 130.0),
                    ("mix", 25.0),
                ],
            ),
            preset(
                tr("Pads: shimmer"),
                &[
                    ("type", 5.0),
                    ("decay", 8.0),
                    ("predelay", 40.0),
                    ("modulation", 60.0),
                    ("shimmer", 60.0),
                    ("mix", 40.0),
                ],
            ),
            preset(
                tr("Pads: endless shimmer"),
                &[
                    ("type", 5.0),
                    ("decay", 14.0),
                    ("predelay", 60.0),
                    ("high_cut", 12_000.0),
                    ("shimmer", 85.0),
                    ("mix", 50.0),
                ],
            ),
            preset(
                tr("Send: vocal plate"),
                &[
                    ("type", 3.0),
                    ("decay", 2.2),
                    ("predelay", 40.0),
                    ("low_cut", 250.0),
                    ("treble", 40.0),
                    ("mix", 100.0),
                ],
            ),
            preset(
                tr("Send: drum room"),
                &[
                    ("type", 0.0),
                    ("decay", 0.9),
                    ("predelay", 5.0),
                    ("low_cut", 150.0),
                    ("size", 60.0),
                    ("early", 60.0),
                    ("mix", 100.0),
                ],
            ),
            preset(
                tr("Send: medium hall"),
                &[
                    ("type", 1.0),
                    ("decay", 2.0),
                    ("predelay", 25.0),
                    ("low_cut", 200.0),
                    ("size", 60.0),
                    ("mix", 100.0),
                ],
            ),
            preset(
                tr("Send: large dark hall"),
                &[
                    ("type", 1.0),
                    ("decay", 3.5),
                    ("predelay", 40.0),
                    ("low_cut", 200.0),
                    ("high_cut", 6000.0),
                    ("size", 85.0),
                    ("treble", 25.0),
                    ("mix", 100.0),
                ],
            ),
            preset(
                tr("Send: plate, 1/8 pre-delay"),
                &[
                    ("type", 3.0),
                    ("decay", 1.8),
                    ("predelay_sync", 1.0),
                    ("predelay_note", 5.0),
                    ("low_cut", 250.0),
                    ("mix", 100.0),
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
                    ("model", 2.0),
                    ("time", 110.0),
                    ("feedback", 15.0),
                    ("wow", 20.0),
                    ("mix", 22.0),
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
                tr("Vocal: 1/4 throw, ducked"),
                &[
                    ("model", 1.0),
                    ("sync", 1.0),
                    ("note", 8.0),
                    ("feedback", 30.0),
                    ("low_cut", 300.0),
                    ("high_cut", 5000.0),
                    ("drive", 25.0),
                    ("wow", 15.0),
                    ("duck", 60.0),
                    ("mix", 25.0),
                ],
            ),
            preset(
                tr("Guitar: slapback 120 ms"),
                &[
                    ("model", 1.0),
                    ("time", 120.0),
                    ("feedback", 10.0),
                    ("high_cut", 5000.0),
                    ("drive", 35.0),
                    ("wow", 15.0),
                    ("mix", 28.0),
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
                    ("model", 1.0),
                    ("time", 330.0),
                    ("feedback", 45.0),
                    ("high_cut", 5000.0),
                    ("drive", 40.0),
                    ("wow", 35.0),
                    ("mix", 28.0),
                ],
            ),
            preset(
                tr("Guitar: wide solo, 1/4 and offset"),
                &[
                    ("sync", 1.0),
                    ("note", 8.0),
                    ("feedback", 30.0),
                    ("low_cut", 200.0),
                    ("high_cut", 6000.0),
                    ("offset", 20.0),
                    ("duck", 40.0),
                    ("mix", 22.0),
                ],
            ),
            preset(
                tr("Synth: 1/4 ping-pong"),
                &[
                    ("sync", 1.0),
                    ("note", 8.0),
                    ("feedback", 40.0),
                    ("pingpong", 1.0),
                    ("mix", 25.0),
                ],
            ),
            preset(
                tr("Synth: 1/8 triplet ping-pong"),
                &[
                    ("sync", 1.0),
                    ("note", 4.0),
                    ("feedback", 40.0),
                    ("low_cut", 200.0),
                    ("pingpong", 1.0),
                    ("mix", 25.0),
                ],
            ),
            preset(
                tr("Synth: lead, analog 1/8"),
                &[
                    ("model", 2.0),
                    ("sync", 1.0),
                    ("note", 5.0),
                    ("feedback", 30.0),
                    ("wow", 15.0),
                    ("mix", 22.0),
                ],
            ),
            preset(
                tr("FX: drums, 1/16 ping-pong"),
                &[
                    ("sync", 1.0),
                    ("note", 2.0),
                    ("feedback", 25.0),
                    ("low_cut", 400.0),
                    ("high_cut", 6000.0),
                    ("pingpong", 1.0),
                    ("mix", 18.0),
                ],
            ),
            preset(
                tr("FX: dub, runaway tape"),
                &[
                    ("model", 1.0),
                    ("sync", 1.0),
                    ("note", 6.0),
                    ("feedback", 80.0),
                    ("low_cut", 300.0),
                    ("high_cut", 3000.0),
                    ("drive", 60.0),
                    ("wow", 40.0),
                    ("mix", 35.0),
                ],
            ),
            preset(
                tr("FX: stereo widener 25 ms"),
                &[
                    ("time", 12.0),
                    ("feedback", 0.0),
                    ("offset", 13.0),
                    ("low_cut", 150.0),
                    ("high_cut", 9000.0),
                    ("mix", 35.0),
                ],
            ),
            preset(
                tr("FX: worn tape slap"),
                &[
                    ("model", 1.0),
                    ("time", 140.0),
                    ("feedback", 20.0),
                    ("low_cut", 250.0),
                    ("high_cut", 2800.0),
                    ("drive", 70.0),
                    ("wow", 70.0),
                    ("mix", 30.0),
                ],
            ),
            preset(
                tr("Send: 1/4 echo"),
                &[
                    ("sync", 1.0),
                    ("note", 8.0),
                    ("feedback", 30.0),
                    ("low_cut", 200.0),
                    ("high_cut", 6000.0),
                    ("mix", 100.0),
                ],
            ),
            preset(
                tr("Send: dotted 1/8"),
                &[
                    ("sync", 1.0),
                    ("note", 6.0),
                    ("feedback", 35.0),
                    ("low_cut", 200.0),
                    ("high_cut", 6000.0),
                    ("mix", 100.0),
                ],
            ),
            preset(
                tr("Send: slapback"),
                &[
                    ("model", 1.0),
                    ("time", 100.0),
                    ("feedback", 5.0),
                    ("low_cut", 200.0),
                    ("high_cut", 6000.0),
                    ("wow", 10.0),
                    ("mix", 100.0),
                ],
            ),
            preset(
                tr("Send: tape echo"),
                &[
                    ("model", 1.0),
                    ("sync", 1.0),
                    ("note", 8.0),
                    ("feedback", 45.0),
                    ("low_cut", 250.0),
                    ("high_cut", 4500.0),
                    ("drive", 40.0),
                    ("wow", 30.0),
                    ("mix", 100.0),
                ],
            ),
        ],
        "gate" => vec![
            preset(
                tr("Drums: kick, tight"),
                &[
                    ("threshold", -22.0),
                    ("range", -40.0),
                    ("attack", 0.1),
                    ("hold", 20.0),
                    ("release", 60.0),
                    ("lookahead", 1.0),
                    ("sc_lpf", 1000.0),
                ],
            ),
            preset(
                tr("Drums: snare, bleed control"),
                &[
                    ("threshold", -18.0),
                    ("range", -25.0),
                    ("attack", 0.2),
                    ("hold", 40.0),
                    ("release", 120.0),
                    ("lookahead", 1.0),
                    ("sc_hpf", 120.0),
                    ("sc_lpf", 6000.0),
                ],
            ),
            preset(
                tr("Drums: toms, hits only"),
                &[
                    ("threshold", -22.0),
                    ("range", -30.0),
                    ("attack", 0.5),
                    ("hold", 80.0),
                    ("release", 250.0),
                    ("lookahead", 1.0),
                    ("sc_lpf", 3000.0),
                ],
            ),
            preset(
                tr("Drums: bus, tight"),
                &[
                    ("threshold", -26.0),
                    ("range", -12.0),
                    ("attack", 0.5),
                    ("hold", 30.0),
                    ("release", 80.0),
                    ("mode", 1.0),
                    ("ratio", 2.0),
                ],
            ),
            preset(
                tr("Drums: 80s gated reverb"),
                &[
                    ("threshold", -22.0),
                    ("range", -80.0),
                    ("attack", 0.1),
                    ("hold", 250.0),
                    ("release", 20.0),
                    ("lookahead", 1.0),
                ],
            ),
            preset(
                tr("Vocal: breath reducer"),
                &[
                    ("threshold", -32.0),
                    ("range", -10.0),
                    ("attack", 2.0),
                    ("hold", 50.0),
                    ("release", 200.0),
                    ("mode", 1.0),
                    ("ratio", 2.0),
                ],
            ),
            preset(
                tr("Vocal: silence between phrases"),
                &[
                    ("threshold", -38.0),
                    ("range", -30.0),
                    ("attack", 1.0),
                    ("hold", 150.0),
                    ("release", 250.0),
                    ("lookahead", 2.0),
                    ("hysteresis", 6.0),
                ],
            ),
            preset(
                tr("Vocal: voice-over, room noise"),
                &[
                    ("threshold", -40.0),
                    ("range", -15.0),
                    ("attack", 5.0),
                    ("hold", 150.0),
                    ("release", 400.0),
                    ("mode", 1.0),
                    ("ratio", 2.0),
                ],
            ),
            preset(
                tr("Guitar: high-gain silence"),
                &[
                    ("threshold", -40.0),
                    ("range", -50.0),
                    ("attack", 1.0),
                    ("hold", 30.0),
                    ("release", 150.0),
                ],
            ),
            preset(
                tr("Guitar: tight palm mutes"),
                &[
                    ("threshold", -28.0),
                    ("range", -60.0),
                    ("attack", 0.3),
                    ("hold", 10.0),
                    ("release", 40.0),
                    ("lookahead", 1.0),
                ],
            ),
            preset(
                tr("Guitar: amp hum between riffs"),
                &[
                    ("threshold", -36.0),
                    ("range", -18.0),
                    ("attack", 2.0),
                    ("hold", 80.0),
                    ("release", 300.0),
                    ("mode", 1.0),
                    ("ratio", 2.5),
                ],
            ),
            preset(
                tr("Ducker: music under the voice"),
                &[
                    ("threshold", -35.0),
                    ("range", -12.0),
                    ("attack", 10.0),
                    ("hold", 200.0),
                    ("release", 500.0),
                    ("mode", 2.0),
                ],
            ),
            preset(
                tr("Ducker: bass under the kick"),
                &[
                    ("threshold", -30.0),
                    ("range", -6.0),
                    ("attack", 1.0),
                    ("hold", 20.0),
                    ("release", 120.0),
                    ("mode", 2.0),
                ],
            ),
            preset(
                tr("Ducker: reverb under the vocal"),
                &[
                    ("threshold", -35.0),
                    ("range", -9.0),
                    ("attack", 20.0),
                    ("hold", 100.0),
                    ("release", 400.0),
                    ("mode", 2.0),
                ],
            ),
            preset(
                tr("Fix: noise floor"),
                &[
                    ("threshold", -50.0),
                    ("range", -20.0),
                    ("release", 300.0),
                    ("mode", 1.0),
                    ("ratio", 1.5),
                ],
            ),
            preset(
                tr("Fix: headphone bleed"),
                &[
                    ("threshold", -42.0),
                    ("range", -24.0),
                    ("attack", 1.0),
                    ("hold", 120.0),
                    ("release", 350.0),
                    ("mode", 1.0),
                    ("ratio", 3.0),
                ],
            ),
            preset(
                tr("Fix: hiss between notes"),
                &[
                    ("threshold", -48.0),
                    ("range", -12.0),
                    ("attack", 3.0),
                    ("hold", 100.0),
                    ("release", 500.0),
                    ("mode", 1.0),
                    ("ratio", 2.0),
                    ("sc_hpf", 200.0),
                ],
            ),
        ],
        "limiter" => vec![
            preset(
                tr("Master: safety -1 dBTP"),
                &[("auto", 1.0), ("ceiling", -1.0), ("true_peak", 1.0)],
            ),
            preset(
                tr("Master: streaming -1 dBTP"),
                &[
                    ("auto", 1.0),
                    ("ceiling", -1.0),
                    ("release", 100.0),
                    ("true_peak", 1.0),
                    ("input", 5.0),
                ],
            ),
            preset(
                tr("Master: broadcast -2 dBTP"),
                &[
                    ("auto", 1.0),
                    ("ceiling", -2.0),
                    ("release", 200.0),
                    ("true_peak", 1.0),
                    ("input", 3.0),
                ],
            ),
            preset(
                tr("Master: loud -0.3 dB"),
                &[("ceiling", -0.3), ("release", 80.0), ("input", 8.0)],
            ),
            preset(
                tr("Master: wide, unlinked"),
                &[
                    ("ceiling", -1.0),
                    ("link", 50.0),
                    ("release", 100.0),
                    ("true_peak", 1.0),
                    ("input", 5.0),
                ],
            ),
            preset(
                tr("Track: vocal, peak catcher"),
                &[("ceiling", -6.0), ("release", 50.0), ("input", 3.0)],
            ),
            preset(
                tr("Track: bass, even level"),
                &[("ceiling", -6.0), ("release", 150.0), ("input", 4.0)],
            ),
            preset(
                tr("Track: acoustic guitar, strum peaks"),
                &[("ceiling", -6.0), ("release", 80.0), ("input", 4.0)],
            ),
            preset(
                tr("Drums: bus, peak shave"),
                &[("ceiling", -3.0), ("release", 20.0), ("input", 4.0)],
            ),
            preset(
                tr("Drums: kick, clipper-style"),
                &[
                    ("ceiling", -3.0),
                    ("release", 5.0),
                    ("lookahead", 0.1),
                    ("input", 8.0),
                ],
            ),
            preset(
                tr("Drums: snare, shaved crack"),
                &[
                    ("ceiling", -4.0),
                    ("release", 10.0),
                    ("lookahead", 0.5),
                    ("input", 6.0),
                ],
            ),
        ],
        "drive" => vec![
            preset(
                tr("Bass: thick transformer"),
                &[
                    ("mode", 5.0),
                    ("drive", 12.0),
                    ("tone", 9000.0),
                    ("bass", 2.0),
                    ("level", -9.0),
                ],
            ),
            preset(
                tr("Bass: parallel grit"),
                &[
                    ("mode", 0.0),
                    ("drive", 18.0),
                    ("tone", 2000.0),
                    ("mix", 40.0),
                    ("level", -10.5),
                ],
            ),
            preset(
                tr("Bass: growl"),
                &[
                    ("mode", 0.0),
                    ("drive", 22.0),
                    ("tone", 3000.0),
                    ("mid", 3.0),
                    ("mix", 60.0),
                    ("level", -14.0),
                ],
            ),
            preset(
                tr("Vocal: parallel grit"),
                &[
                    ("mode", 0.0),
                    ("drive", 20.0),
                    ("tone", 5000.0),
                    ("low_cut", 300.0),
                    ("mix", 25.0),
                    ("level", -6.5),
                ],
            ),
            preset(
                tr("Vocal: tube sheen"),
                &[
                    ("mode", 3.0),
                    ("drive", 9.0),
                    ("tone", 10_000.0),
                    ("bias", 15.0),
                    ("level", -5.0),
                ],
            ),
            preset(
                tr("Vocal: megaphone"),
                &[
                    ("mode", 1.0),
                    ("drive", 24.0),
                    ("tone", 3000.0),
                    ("low_cut", 500.0),
                    ("bass", -12.0),
                    ("mid", 6.0),
                    ("level", -16.5),
                ],
            ),
            preset(
                tr("Drums: parallel crunch"),
                &[
                    ("mode", 1.0),
                    ("drive", 24.0),
                    ("tone", 6000.0),
                    ("mix", 30.0),
                    ("level", -14.0),
                ],
            ),
            preset(
                tr("Drums: warm tape"),
                &[
                    ("mode", 4.0),
                    ("drive", 12.0),
                    ("tone", 9000.0),
                    ("level", -9.5),
                ],
            ),
            preset(
                tr("Drums: snare bite"),
                &[
                    ("mode", 5.0),
                    ("drive", 16.0),
                    ("tone", 8000.0),
                    ("mid", 2.0),
                    ("low_cut", 80.0),
                    ("level", -11.5),
                ],
            ),
            preset(
                tr("Guitar: light crunch"),
                &[
                    ("mode", 0.0),
                    ("drive", 12.0),
                    ("tone", 4000.0),
                    ("level", -8.5),
                ],
            ),
            preset(
                tr("Guitar: lead"),
                &[
                    ("mode", 1.0),
                    ("drive", 30.0),
                    ("tone", 3000.0),
                    ("level", -25.0),
                ],
            ),
            preset(
                tr("Guitar: tight metal"),
                &[
                    ("mode", 1.0),
                    ("drive", 36.0),
                    ("tone", 5000.0),
                    ("low_cut", 250.0),
                    ("mid", 4.0),
                    ("level", -29.5),
                ],
            ),
            preset(
                tr("Guitar: fuzz"),
                &[
                    ("mode", 2.0),
                    ("drive", 36.0),
                    ("tone", 2500.0),
                    ("level", -27.0),
                ],
            ),
            preset(
                tr("Guitar: octave fuzz"),
                &[
                    ("mode", 6.0),
                    ("drive", 30.0),
                    ("tone", 3500.0),
                    ("low_cut", 150.0),
                    ("level", -21.0),
                ],
            ),
            preset(
                tr("Keys: warm tube"),
                &[
                    ("mode", 3.0),
                    ("drive", 14.0),
                    ("tone", 7000.0),
                    ("bias", 30.0),
                    ("level", -10.5),
                ],
            ),
            preset(
                tr("Keys: electric piano bark"),
                &[
                    ("mode", 0.0),
                    ("drive", 14.0),
                    ("tone", 3000.0),
                    ("mix", 70.0),
                    ("level", -10.5),
                ],
            ),
            preset(
                tr("Keys: organ grind"),
                &[
                    ("mode", 3.0),
                    ("drive", 24.0),
                    ("tone", 5000.0),
                    ("bias", 20.0),
                    ("mid", 2.0),
                    ("level", -17.0),
                ],
            ),
            preset(
                tr("Bus: mix, tape warmth"),
                &[
                    ("mode", 4.0),
                    ("drive", 6.0),
                    ("tone", 11_000.0),
                    ("level", -5.0),
                ],
            ),
            preset(
                tr("Bus: mix, transformer weight"),
                &[
                    ("mode", 5.0),
                    ("drive", 6.0),
                    ("tone", 12_000.0),
                    ("bass", 1.0),
                    ("level", -4.5),
                ],
            ),
            preset(
                tr("Bus: drums, tube glow"),
                &[
                    ("mode", 3.0),
                    ("drive", 8.0),
                    ("tone", 10_000.0),
                    ("bias", 10.0),
                    ("level", -5.0),
                ],
            ),
            preset(
                tr("Lo-fi: blown speaker"),
                &[
                    ("mode", 2.0),
                    ("drive", 30.0),
                    ("tone", 2000.0),
                    ("low_cut", 300.0),
                    ("bias", 40.0),
                    ("level", -17.0),
                ],
            ),
            preset(
                tr("Lo-fi: transistor radio"),
                &[
                    ("mode", 1.0),
                    ("drive", 18.0),
                    ("tone", 2500.0),
                    ("low_cut", 400.0),
                    ("bass", -8.0),
                    ("mid", 6.0),
                    ("level", -16.5),
                ],
            ),
            preset(
                tr("Lo-fi: dying battery"),
                &[
                    ("mode", 2.0),
                    ("drive", 24.0),
                    ("tone", 3000.0),
                    ("bias", 85.0),
                    ("level", -22.5),
                ],
            ),
        ],
        "chorus" => vec![
            preset(
                tr("Vocal: subtle thickening"),
                &[("rate", 0.5), ("depth", 25.0), ("mix", 35.0)],
            ),
            preset(
                tr("Vocal: backing, wide"),
                &[
                    ("rate", 0.4),
                    ("depth", 35.0),
                    ("delay", 14.0),
                    ("mix", 45.0),
                    ("voices", 1.0),
                    ("stereo", 180.0),
                ],
            ),
            preset(
                tr("Vocal: dreamy double"),
                &[
                    ("rate", 0.25),
                    ("depth", 30.0),
                    ("delay", 22.0),
                    ("mix", 40.0),
                    ("tone", 9000.0),
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
                tr("Guitar: acoustic, 12-string"),
                &[
                    ("rate", 0.3),
                    ("depth", 20.0),
                    ("delay", 15.0),
                    ("mix", 30.0),
                ],
            ),
            preset(
                tr("Guitar: slow shimmer"),
                &[
                    ("rate", 0.25),
                    ("depth", 55.0),
                    ("delay", 10.0),
                    ("mix", 40.0),
                    ("voices", 2.0),
                ],
            ),
            preset(
                tr("Guitar: bass, fretless shimmer"),
                &[
                    ("rate", 0.6),
                    ("depth", 30.0),
                    ("delay", 10.0),
                    ("mix", 30.0),
                ],
            ),
            preset(
                tr("Keys: electric piano"),
                &[
                    ("rate", 1.2),
                    ("depth", 45.0),
                    ("delay", 7.0),
                    ("mix", 45.0),
                    ("tone", 9000.0),
                ],
            ),
            preset(
                tr("Keys: organ, slow rotor"),
                &[
                    ("rate", 0.9),
                    ("depth", 35.0),
                    ("delay", 6.0),
                    ("mix", 50.0),
                    ("stereo", 180.0),
                    ("tone", 8000.0),
                ],
            ),
            preset(
                tr("Keys: piano, honky-tonk"),
                &[
                    ("rate", 2.5),
                    ("depth", 18.0),
                    ("delay", 9.0),
                    ("mix", 30.0),
                ],
            ),
            preset(
                tr("Synth: lush ensemble"),
                &[
                    ("rate", 0.6),
                    ("depth", 60.0),
                    ("delay", 8.0),
                    ("mix", 50.0),
                    ("voices", 2.0),
                ],
            ),
            preset(
                tr("Synth: 80s poly chorus"),
                &[
                    ("rate", 0.5),
                    ("depth", 50.0),
                    ("delay", 7.0),
                    ("mix", 50.0),
                    ("voices", 1.0),
                    ("stereo", 180.0),
                ],
            ),
            preset(
                tr("Synth: string ensemble"),
                &[
                    ("rate", 0.4),
                    ("depth", 45.0),
                    ("delay", 12.0),
                    ("mix", 45.0),
                    ("voices", 2.0),
                    ("tone", 10_000.0),
                ],
            ),
            preset(
                tr("Synth: wide pad"),
                &[
                    ("rate", 0.9),
                    ("depth", 60.0),
                    ("delay", 18.0),
                    ("mix", 50.0),
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
                tr("FX: resonant sweep"),
                &[
                    ("rate", 0.25),
                    ("depth", 80.0),
                    ("delay", 3.0),
                    ("feedback", 60.0),
                ],
            ),
            preset(
                tr("FX: synced 1 bar swirl"),
                &[
                    ("sync", 1.0),
                    ("note", 11.0),
                    ("depth", 70.0),
                    ("delay", 10.0),
                    ("feedback", 30.0),
                    ("mix", 50.0),
                    ("voices", 1.0),
                ],
            ),
        ],
        "mb_comp" => vec![
            preset(
                tr("Master: gentle glue"),
                &[
                    ("count", 1.0),
                    ("x1", 120.0),
                    ("x2", 3000.0),
                    ("b1_ratio", 1.6),
                    ("b1_attack", 30.0),
                    ("b1_release", 250.0),
                    ("b2_ratio", 1.5),
                    ("b2_attack", 15.0),
                    ("b2_release", 160.0),
                    ("b3_ratio", 1.6),
                    ("b3_attack", 6.0),
                    ("b3_release", 90.0),
                    ("b1_thresh", -23.0),
                    ("b2_thresh", -19.0),
                    ("b3_thresh", -30.0),
                    ("output", 1.0),
                ],
            ),
            preset(
                tr("Master: tame harshness"),
                &[
                    ("count", 2.0),
                    ("x1", 150.0),
                    ("x2", 2000.0),
                    ("x3", 6000.0),
                    ("b1_ratio", 1.0),
                    ("b2_ratio", 1.0),
                    ("b3_ratio", 3.0),
                    ("b3_attack", 3.0),
                    ("b3_release", 60.0),
                    ("b4_ratio", 2.0),
                    ("b4_attack", 2.0),
                    ("b4_release", 50.0),
                    ("b3_thresh", -30.0),
                    ("b4_thresh", -30.0),
                    ("output", 0.0),
                ],
            ),
            preset(
                tr("Master: tight low end"),
                &[
                    ("count", 1.0),
                    ("x1", 120.0),
                    ("x2", 3000.0),
                    ("b1_ratio", 3.0),
                    ("b1_attack", 25.0),
                    ("b1_release", 120.0),
                    ("b2_ratio", 1.0),
                    ("b3_ratio", 1.0),
                    ("b1_thresh", -23.0),
                    ("output", 0.0),
                ],
            ),
            preset(
                tr("Master: bring out details"),
                &[
                    ("count", 1.0),
                    ("x1", 200.0),
                    ("x2", 3000.0),
                    ("b1_thresh", -6.0),
                    ("b1_ratio", 2.0),
                    ("b1_up", 1.4),
                    ("b1_attack", 20.0),
                    ("b1_release", 250.0),
                    ("b2_thresh", -6.0),
                    ("b2_ratio", 2.0),
                    ("b2_up", 1.4),
                    ("b2_attack", 10.0),
                    ("b2_release", 200.0),
                    ("b3_thresh", -6.0),
                    ("b3_ratio", 2.0),
                    ("b3_up", 1.4),
                    ("b3_attack", 5.0),
                    ("b3_release", 150.0),
                    ("b1_below", -16.0),
                    ("b2_below", -13.5),
                    ("b3_below", -33.0),
                    ("output", -0.5),
                ],
            ),
            preset(
                tr("Master: broadcast, loud and even"),
                &[
                    ("count", 2.0),
                    ("x1", 150.0),
                    ("x2", 800.0),
                    ("x3", 4000.0),
                    ("time", 70.0),
                    ("b1_ratio", 3.0),
                    ("b1_below", -45.0),
                    ("b1_up", 1.5),
                    ("b2_ratio", 3.0),
                    ("b2_below", -45.0),
                    ("b2_up", 1.5),
                    ("b3_ratio", 3.0),
                    ("b3_below", -45.0),
                    ("b3_up", 1.5),
                    ("b4_ratio", 3.0),
                    ("b4_below", -45.0),
                    ("b4_up", 1.5),
                    ("b1_thresh", -22.0),
                    ("b2_thresh", -20.0),
                    ("b3_thresh", -26.0),
                    ("b4_thresh", -31.0),
                    ("output", 2.0),
                ],
            ),
            preset(
                tr("Bus: mix glue"),
                &[
                    ("count", 1.0),
                    ("x1", 200.0),
                    ("x2", 4000.0),
                    ("b1_ratio", 2.0),
                    ("b1_attack", 30.0),
                    ("b1_release", 200.0),
                    ("b2_ratio", 1.5),
                    ("b2_attack", 15.0),
                    ("b2_release", 150.0),
                    ("b3_ratio", 2.0),
                    ("b3_attack", 5.0),
                    ("b3_release", 100.0),
                    ("b1_thresh", -22.0),
                    ("b2_thresh", -21.0),
                    ("b3_thresh", -31.0),
                    ("output", 1.5),
                ],
            ),
            preset(
                tr("Bus: mix, clear the mud"),
                &[
                    ("count", 1.0),
                    ("x1", 180.0),
                    ("x2", 600.0),
                    ("b1_ratio", 1.0),
                    ("b2_ratio", 3.0),
                    ("b2_attack", 10.0),
                    ("b2_release", 120.0),
                    ("b3_ratio", 1.0),
                    ("b2_thresh", -21.0),
                    ("output", 0.5),
                ],
            ),
            preset(
                tr("Bus: drum punch"),
                &[
                    ("count", 1.0),
                    ("x1", 120.0),
                    ("x2", 4000.0),
                    ("b1_ratio", 3.0),
                    ("b1_attack", 30.0),
                    ("b1_release", 120.0),
                    ("b2_ratio", 2.5),
                    ("b2_attack", 15.0),
                    ("b2_release", 100.0),
                    ("b3_ratio", 2.0),
                    ("b3_attack", 2.0),
                    ("b3_release", 60.0),
                    ("b3_gain", 1.5),
                    ("b1_thresh", -26.0),
                    ("b2_thresh", -22.0),
                    ("b3_thresh", -38.0),
                    ("output", 1.0),
                ],
            ),
            preset(
                tr("Bus: drum smash"),
                &[
                    ("count", 1.0),
                    ("x1", 100.0),
                    ("x2", 3000.0),
                    ("mix", 60.0),
                    ("b1_ratio", 6.0),
                    ("b1_attack", 10.0),
                    ("b1_release", 80.0),
                    ("b2_ratio", 8.0),
                    ("b2_attack", 5.0),
                    ("b2_release", 60.0),
                    ("b3_ratio", 6.0),
                    ("b3_attack", 1.0),
                    ("b3_release", 40.0),
                    ("b1_thresh", -31.0),
                    ("b2_thresh", -25.0),
                    ("b3_thresh", -39.0),
                    ("output", 2.5),
                ],
            ),
            preset(
                tr("Track: vocal, control"),
                &[
                    ("count", 2.0),
                    ("x1", 200.0),
                    ("x2", 1500.0),
                    ("x3", 6000.0),
                    ("b1_ratio", 3.0),
                    ("b1_attack", 15.0),
                    ("b1_release", 150.0),
                    ("b2_ratio", 2.0),
                    ("b2_attack", 8.0),
                    ("b2_release", 120.0),
                    ("b3_ratio", 3.0),
                    ("b3_attack", 2.0),
                    ("b3_release", 60.0),
                    ("b4_ratio", 4.0),
                    ("b4_attack", 1.0),
                    ("b4_release", 50.0),
                    ("b1_thresh", -35.0),
                    ("b2_thresh", -24.0),
                    ("b3_thresh", -21.0),
                    ("b4_thresh", -18.0),
                    ("output", 2.5),
                ],
            ),
            preset(
                tr("Track: vocal, de-ess and smooth"),
                &[
                    ("count", 1.0),
                    ("x1", 250.0),
                    ("x2", 5500.0),
                    ("b1_ratio", 1.0),
                    ("b2_ratio", 1.8),
                    ("b2_attack", 10.0),
                    ("b2_release", 120.0),
                    ("b3_ratio", 5.0),
                    ("b3_attack", 0.5),
                    ("b3_release", 40.0),
                    ("b2_thresh", -20.0),
                    ("b3_thresh", -17.0),
                    ("output", 2.0),
                ],
            ),
            preset(
                tr("Track: bass, even"),
                &[
                    ("count", 1.0),
                    ("x1", 80.0),
                    ("x2", 600.0),
                    ("b1_ratio", 3.0),
                    ("b1_attack", 30.0),
                    ("b1_release", 200.0),
                    ("b2_ratio", 4.0),
                    ("b2_attack", 10.0),
                    ("b2_release", 120.0),
                    ("b3_ratio", 2.0),
                    ("b3_attack", 5.0),
                    ("b3_release", 80.0),
                    ("b1_thresh", -31.0),
                    ("b2_thresh", -26.0),
                    ("b3_thresh", -29.0),
                    ("output", 3.0),
                ],
            ),
            preset(
                tr("Track: guitars, smooth"),
                &[
                    ("count", 1.0),
                    ("x1", 250.0),
                    ("x2", 3000.0),
                    ("b1_ratio", 2.0),
                    ("b1_attack", 20.0),
                    ("b1_release", 150.0),
                    ("b2_ratio", 1.0),
                    ("b3_ratio", 3.0),
                    ("b3_attack", 3.0),
                    ("b3_release", 70.0),
                    ("b1_thresh", -33.0),
                    ("b3_thresh", -47.0),
                    ("output", 0.0),
                ],
            ),
            preset(
                tr("OTT: light"),
                &[
                    ("count", 1.0),
                    ("x1", 88.0),
                    ("x2", 2500.0),
                    ("mix", 30.0),
                    ("b1_ratio", 10.0),
                    ("b1_below", -42.0),
                    ("b1_up", 3.0),
                    ("b1_attack", 20.0),
                    ("b1_release", 200.0),
                    ("b2_ratio", 10.0),
                    ("b2_below", -42.0),
                    ("b2_up", 3.0),
                    ("b2_attack", 10.0),
                    ("b2_release", 120.0),
                    ("b3_ratio", 10.0),
                    ("b3_below", -42.0),
                    ("b3_up", 3.0),
                    ("b3_attack", 5.0),
                    ("b3_release", 80.0),
                    ("b1_thresh", -25.0),
                    ("b2_thresh", -18.0),
                    ("b3_thresh", -30.0),
                    ("output", 0.5),
                ],
            ),
            preset(
                tr("OTT: full"),
                &[
                    ("count", 1.0),
                    ("x1", 88.0),
                    ("x2", 2500.0),
                    ("mix", 100.0),
                    ("b1_ratio", 20.0),
                    ("b1_below", -42.0),
                    ("b1_up", 4.0),
                    ("b1_attack", 15.0),
                    ("b1_release", 180.0),
                    ("b2_ratio", 20.0),
                    ("b2_below", -42.0),
                    ("b2_up", 4.0),
                    ("b2_attack", 8.0),
                    ("b2_release", 110.0),
                    ("b3_ratio", 20.0),
                    ("b3_below", -42.0),
                    ("b3_up", 4.0),
                    ("b3_attack", 3.0),
                    ("b3_release", 70.0),
                    ("b1_thresh", -30.0),
                    ("b2_thresh", -24.0),
                    ("b3_thresh", -36.0),
                    ("output", 8.0),
                ],
            ),
            preset(
                tr("OTT: synth, bright and dense"),
                &[
                    ("count", 1.0),
                    ("x1", 120.0),
                    ("x2", 2500.0),
                    ("mix", 60.0),
                    ("b3_gain", 1.5),
                    ("b1_thresh", -32.0),
                    ("b1_ratio", 8.0),
                    ("b1_below", -45.0),
                    ("b1_up", 3.0),
                    ("b1_attack", 15.0),
                    ("b1_release", 180.0),
                    ("b2_thresh", -24.0),
                    ("b2_ratio", 8.0),
                    ("b2_below", -45.0),
                    ("b2_up", 3.0),
                    ("b2_attack", 8.0),
                    ("b2_release", 110.0),
                    ("b3_thresh", -38.0),
                    ("b3_ratio", 8.0),
                    ("b3_below", -45.0),
                    ("b3_up", 3.0),
                    ("b3_attack", 3.0),
                    ("b3_release", 70.0),
                    ("output", 3.0),
                ],
            ),
            preset(
                tr("OTT: drums, explosive"),
                &[
                    ("count", 1.0),
                    ("x1", 100.0),
                    ("x2", 3000.0),
                    ("mix", 45.0),
                    ("b1_ratio", 12.0),
                    ("b1_below", -45.0),
                    ("b1_up", 3.5),
                    ("b1_attack", 10.0),
                    ("b1_release", 150.0),
                    ("b2_ratio", 12.0),
                    ("b2_below", -45.0),
                    ("b2_up", 3.5),
                    ("b2_attack", 5.0),
                    ("b2_release", 90.0),
                    ("b3_ratio", 12.0),
                    ("b3_below", -45.0),
                    ("b3_up", 3.5),
                    ("b3_attack", 2.0),
                    ("b3_release", 60.0),
                    ("b1_thresh", -29.0),
                    ("b2_thresh", -25.0),
                    ("b3_thresh", -40.0),
                    ("output", 2.0),
                ],
            ),
        ],
        "modulator" => vec![
            preset(
                tr("Tremolo: vintage amp"),
                &[
                    ("mode", 0.0),
                    ("note", 5.0),
                    ("depth", 45.0),
                    ("shape", 0.0),
                    ("smooth", 10.0),
                ],
            ),
            preset(
                tr("Tremolo: surf"),
                &[
                    ("mode", 0.0),
                    ("note", 4.0),
                    ("depth", 70.0),
                    ("shape", 1.0),
                    ("smooth", 5.0),
                ],
            ),
            preset(
                tr("Tremolo: choppy 1/16"),
                &[
                    ("mode", 0.0),
                    ("note", 2.0),
                    ("depth", 100.0),
                    ("shape", 4.0),
                    ("smooth", 8.0),
                ],
            ),
            preset(
                tr("Tremolo: stereo shimmer"),
                &[
                    ("mode", 0.0),
                    ("note", 5.0),
                    ("depth", 60.0),
                    ("shape", 0.0),
                    ("phase", 180.0),
                ],
            ),
            preset(
                tr("Tremolo: slow swell"),
                &[
                    ("mode", 0.0),
                    ("note", 11.0),
                    ("depth", 70.0),
                    ("shape", 0.0),
                    ("smooth", 30.0),
                ],
            ),
            preset(
                tr("Pump: sidechain 1/4"),
                &[
                    ("mode", 0.0),
                    ("note", 8.0),
                    ("depth", 90.0),
                    ("shape", 2.0),
                    ("smooth", 6.0),
                ],
            ),
            preset(
                tr("Pump: soft 1/8"),
                &[
                    ("mode", 0.0),
                    ("note", 5.0),
                    ("depth", 55.0),
                    ("shape", 2.0),
                    ("smooth", 12.0),
                ],
            ),
            preset(
                tr("Auto-pan: gentle 1/2"),
                &[
                    ("mode", 1.0),
                    ("note", 10.0),
                    ("depth", 50.0),
                    ("shape", 0.0),
                    ("smooth", 20.0),
                ],
            ),
            preset(
                tr("Auto-pan: wide circle 1/8"),
                &[
                    ("mode", 1.0),
                    ("note", 5.0),
                    ("depth", 100.0),
                    ("shape", 0.0),
                ],
            ),
            preset(
                tr("Auto-pan: ping-pong 1/4"),
                &[
                    ("mode", 1.0),
                    ("note", 8.0),
                    ("depth", 90.0),
                    ("shape", 4.0),
                    ("smooth", 12.0),
                ],
            ),
            preset(
                tr("Auto-pan: random drift"),
                &[
                    ("mode", 1.0),
                    ("note", 8.0),
                    ("depth", 70.0),
                    ("shape", 5.0),
                    ("smooth", 60.0),
                ],
            ),
            cutter(
                tr("Cutter: trance gate"),
                &[
                    ("mode", 2.0),
                    ("note", 2.0),
                    ("steps", 2.0),
                    ("gate", 60.0),
                    ("depth", 100.0),
                    ("smooth", 6.0),
                ],
                "x.x.xx.xx.x.xxx.",
            ),
            cutter(
                tr("Cutter: offbeat stabs"),
                &[
                    ("mode", 2.0),
                    ("note", 2.0),
                    ("steps", 2.0),
                    ("gate", 70.0),
                    ("depth", 100.0),
                    ("smooth", 5.0),
                ],
                "..x...x...x...x.",
            ),
            cutter(
                tr("Cutter: stutter 1/32"),
                &[
                    ("mode", 2.0),
                    ("note", 0.0),
                    ("steps", 2.0),
                    ("gate", 50.0),
                    ("depth", 100.0),
                    ("smooth", 3.0),
                ],
                "xxxxxxxxx.x.x.x.",
            ),
            cutter(
                tr("Cutter: dotted groove"),
                &[
                    ("mode", 2.0),
                    ("note", 2.0),
                    ("steps", 2.0),
                    ("gate", 80.0),
                    ("depth", 100.0),
                    ("smooth", 8.0),
                ],
                "x..x..x..x..x.x.",
            ),
            cutter(
                tr("Cutter: half-time duck"),
                &[
                    ("mode", 2.0),
                    ("note", 8.0),
                    ("steps", 0.0),
                    ("gate", 90.0),
                    ("depth", 70.0),
                    ("smooth", 25.0),
                ],
                "x.xxxxxxxxxxxxxx",
            ),
            cutter(
                tr("Cutter: glitch"),
                &[
                    ("mode", 2.0),
                    ("note", 1.0),
                    ("steps", 2.0),
                    ("gate", 35.0),
                    ("depth", 100.0),
                    ("smooth", 2.0),
                ],
                "xx.x.xxxx..xxx.x",
            ),
            preset(
                tr("Vibrato: subtle"),
                &[
                    ("mode", 3.0),
                    ("sync", 0.0),
                    ("rate", 5.5),
                    ("depth", 18.0),
                    ("shape", 0.0),
                ],
            ),
            preset(
                tr("Vibrato: warped tape"),
                &[
                    ("mode", 3.0),
                    ("sync", 0.0),
                    ("rate", 0.6),
                    ("depth", 45.0),
                    ("shape", 5.0),
                    ("smooth", 80.0),
                ],
            ),
            preset(
                tr("Vibrato: chorus-like"),
                &[
                    ("mode", 3.0),
                    ("sync", 0.0),
                    ("rate", 0.9),
                    ("depth", 40.0),
                    ("shape", 0.0),
                    ("phase", 90.0),
                    ("mix", 50.0),
                ],
            ),
            preset(
                tr("Vibrato: seasick"),
                &[
                    ("mode", 3.0),
                    ("sync", 0.0),
                    ("rate", 2.0),
                    ("depth", 100.0),
                    ("shape", 1.0),
                    ("phase", 180.0),
                ],
            ),
            preset(
                tr("Ring: robot voice"),
                &[
                    ("mode", 4.0),
                    ("freq", 140.0),
                    ("depth", 80.0),
                    ("mix", 80.0),
                    ("output", 3.0),
                ],
            ),
            preset(
                tr("Ring: metallic bell"),
                &[
                    ("mode", 4.0),
                    ("freq", 880.0),
                    ("depth", 70.0),
                    ("mix", 60.0),
                    ("output", 3.0),
                ],
            ),
            preset(
                tr("Ring: dalek"),
                &[("mode", 4.0), ("freq", 30.0), ("depth", 100.0)],
            ),
            preset(
                tr("Ring: alien radio"),
                &[
                    ("mode", 4.0),
                    ("freq", 2300.0),
                    ("depth", 100.0),
                    ("mix", 70.0),
                    ("output", 3.0),
                ],
            ),
        ],
        "pitch" => vec![
            preset(tr("Octave: up"), &[("a_semis", 12.0)]),
            preset(tr("Octave: down"), &[("a_semis", -12.0)]),
            preset(
                tr("Octave: up and down"),
                &[
                    ("a_semis", 12.0),
                    ("a_level", 70.0),
                    ("b_semis", -12.0),
                    ("b_level", 70.0),
                    ("mix", 60.0),
                    ("output", 3.5),
                ],
            ),
            preset(
                tr("Octave: vocal, octave below, blended"),
                &[
                    ("a_semis", -12.0),
                    ("a_level", 70.0),
                    ("mix", 35.0),
                    ("tone", 4000.0),
                    ("output", 3.5),
                ],
            ),
            preset(
                tr("Octave: bass, sub octave"),
                &[
                    ("a_semis", -12.0),
                    ("mix", 40.0),
                    ("tone", 1500.0),
                    ("window", 60.0),
                    ("output", 3.5),
                ],
            ),
            preset(
                tr("Harmony: fifth above"),
                &[("a_semis", 7.0), ("mix", 50.0), ("output", 3.5)],
            ),
            preset(
                tr("Harmony: major third above"),
                &[("a_semis", 4.0), ("mix", 50.0), ("output", 3.5)],
            ),
            preset(
                tr("Harmony: fourth below"),
                &[("a_semis", -5.0), ("mix", 45.0), ("output", 3.5)],
            ),
            preset(
                tr("Harmony: third and fifth"),
                &[
                    ("a_semis", 4.0),
                    ("a_level", 80.0),
                    ("a_pan", -50.0),
                    ("b_semis", 7.0),
                    ("b_level", 80.0),
                    ("b_pan", 50.0),
                    ("mix", 50.0),
                    ("output", 4.0),
                ],
            ),
            preset(tr("FX: chipmunk"), &[("a_semis", 12.0), ("window", 25.0)]),
            preset(
                tr("FX: monster voice"),
                &[("a_semis", -7.0), ("window", 60.0)],
            ),
            preset(
                tr("FX: demon and child"),
                &[
                    ("a_semis", -12.0),
                    ("a_pan", -40.0),
                    ("b_semis", 12.0),
                    ("b_level", 60.0),
                    ("b_pan", 40.0),
                    ("window", 50.0),
                    ("output", 2.0),
                ],
            ),
            preset(
                tr("FX: micro-shift widener"),
                &[
                    ("a_semis", 0.0),
                    ("a_cents", 9.0),
                    ("a_pan", -100.0),
                    ("b_semis", 0.0),
                    ("b_cents", -9.0),
                    ("b_level", 100.0),
                    ("b_pan", 100.0),
                    ("mix", 40.0),
                    ("window", 30.0),
                    ("output", 3.0),
                ],
            ),
            preset(
                tr("Spiral: rising shimmer"),
                &[
                    ("a_semis", 12.0),
                    ("delay", 250.0),
                    ("feedback", 70.0),
                    ("mix", 40.0),
                    ("tone", 8000.0),
                    ("output", 1.5),
                ],
            ),
            preset(
                tr("Spiral: falling"),
                &[
                    ("a_semis", -12.0),
                    ("delay", 180.0),
                    ("feedback", 65.0),
                    ("mix", 40.0),
                    ("output", 3.0),
                ],
            ),
            preset(
                tr("Spiral: fifths staircase"),
                &[
                    ("a_semis", 7.0),
                    ("delay", 300.0),
                    ("feedback", 75.0),
                    ("mix", 35.0),
                    ("output", 2.0),
                ],
            ),
            preset(
                tr("Spiral: glitch cascade"),
                &[
                    ("a_semis", 5.0),
                    ("a_level", 70.0),
                    ("b_semis", -7.0),
                    ("b_level", 60.0),
                    ("delay", 90.0),
                    ("feedback", 55.0),
                    ("mix", 50.0),
                    ("window", 15.0),
                    ("output", 3.5),
                ],
            ),
        ],
        "doubler" => vec![
            preset(
                tr("Vocal: natural double"),
                &[
                    ("voices", 0.0),
                    ("detune", 6.0),
                    ("delay", 18.0),
                    ("width", 80.0),
                    ("drift", 40.0),
                    ("low_cut", 150.0),
                    ("mix", 45.0),
                    ("output", -0.5),
                ],
            ),
            preset(
                tr("Vocal: tight widen"),
                &[
                    ("voices", 0.0),
                    ("detune", 9.0),
                    ("delay", 4.0),
                    ("width", 100.0),
                    ("drift", 10.0),
                    ("mix", 40.0),
                    ("output", -0.5),
                ],
            ),
            preset(
                tr("Vocal: stack of takes"),
                &[
                    ("voices", 1.0),
                    ("detune", 12.0),
                    ("delay", 22.0),
                    ("width", 100.0),
                    ("drift", 60.0),
                    ("mix", 55.0),
                    ("output", -1.0),
                ],
            ),
            preset(
                tr("Vocal: vintage ADT"),
                &[
                    ("voices", 0.0),
                    ("detune", 4.0),
                    ("delay", 30.0),
                    ("width", 60.0),
                    ("drift", 70.0),
                    ("mix", 50.0),
                    ("output", -0.5),
                ],
            ),
            preset(
                tr("Vocal: slapback double"),
                &[
                    ("voices", 0.0),
                    ("detune", 3.0),
                    ("delay", 60.0),
                    ("width", 90.0),
                    ("drift", 20.0),
                    ("mix", 45.0),
                    ("output", -0.5),
                ],
            ),
            preset(
                tr("Guitar: wall"),
                &[
                    ("voices", 1.0),
                    ("detune", 10.0),
                    ("delay", 15.0),
                    ("width", 100.0),
                    ("drift", 40.0),
                    ("low_cut", 120.0),
                    ("mix", 60.0),
                    ("output", -1.0),
                ],
            ),
            preset(
                tr("Guitar: subtle stereo"),
                &[
                    ("voices", 0.0),
                    ("detune", 5.0),
                    ("delay", 12.0),
                    ("width", 70.0),
                    ("drift", 25.0),
                    ("mix", 35.0),
                    ("output", -0.5),
                ],
            ),
            preset(
                tr("Guitar: acoustic, twin takes"),
                &[
                    ("voices", 0.0),
                    ("detune", 7.0),
                    ("delay", 20.0),
                    ("width", 90.0),
                    ("drift", 50.0),
                    ("mix", 45.0),
                    ("output", -0.5),
                ],
            ),
            preset(
                tr("Synth: wide pad"),
                &[
                    ("voices", 1.0),
                    ("detune", 18.0),
                    ("delay", 25.0),
                    ("width", 100.0),
                    ("drift", 50.0),
                    ("low_cut", 200.0),
                    ("mix", 60.0),
                    ("output", -0.5),
                ],
            ),
            preset(
                tr("Synth: lead, fat unison"),
                &[
                    ("voices", 1.0),
                    ("detune", 14.0),
                    ("delay", 6.0),
                    ("width", 85.0),
                    ("drift", 15.0),
                    ("mix", 50.0),
                    ("output", -1.0),
                ],
            ),
            preset(
                tr("Synth: bass, wide tops"),
                &[
                    ("voices", 0.0),
                    ("detune", 10.0),
                    ("delay", 8.0),
                    ("width", 100.0),
                    ("drift", 20.0),
                    ("low_cut", 300.0),
                    ("mix", 40.0),
                    ("output", 0.0),
                ],
            ),
            preset(
                tr("FX: out-of-tune choir"),
                &[
                    ("voices", 1.0),
                    ("detune", 35.0),
                    ("delay", 30.0),
                    ("width", 100.0),
                    ("drift", 80.0),
                    ("mix", 60.0),
                    ("output", -1.0),
                ],
            ),
            preset(
                tr("FX: drunken double"),
                &[
                    ("voices", 0.0),
                    ("detune", 22.0),
                    ("delay", 45.0),
                    ("width", 80.0),
                    ("drift", 100.0),
                    ("mix", 50.0),
                    ("output", -0.5),
                ],
            ),
            preset(
                tr("FX: crowd of four"),
                &[
                    ("voices", 1.0),
                    ("detune", 25.0),
                    ("delay", 40.0),
                    ("width", 100.0),
                    ("drift", 90.0),
                    ("mix", 70.0),
                    ("output", -1.5),
                ],
            ),
        ],
        "maximizer" => vec![
            preset(
                tr("Master: acoustic, gentle"),
                &[
                    ("ceiling", -1.0),
                    ("release", 500.0),
                    ("style", 0.0),
                    ("gain", 3.0),
                ],
            ),
            preset(
                tr("Master: transparent -1 dBTP"),
                &[
                    ("ceiling", -1.0),
                    ("release", 250.0),
                    ("style", 0.0),
                    ("gain", 4.0),
                ],
            ),
            preset(
                tr("Master: streaming -1 dBTP"),
                &[
                    ("ceiling", -1.0),
                    ("release", 150.0),
                    ("style", 0.0),
                    ("gain", 5.0),
                ],
            ),
            preset(
                tr("Master: broadcast -2 dBTP"),
                &[
                    ("ceiling", -2.0),
                    ("release", 200.0),
                    ("style", 0.0),
                    ("gain", 3.0),
                ],
            ),
            preset(
                tr("Master: CD -0.3 dBTP"),
                &[
                    ("ceiling", -0.3),
                    ("release", 100.0),
                    ("style", 1.0),
                    ("gain", 5.0),
                ],
            ),
            preset(
                tr("Loud: punchy pop"),
                &[
                    ("ceiling", -1.0),
                    ("release", 90.0),
                    ("style", 1.0),
                    ("gain", 6.5),
                ],
            ),
            preset(
                tr("Loud: dense rock"),
                &[
                    ("ceiling", -1.0),
                    ("release", 70.0),
                    ("style", 2.0),
                    ("link", 80.0),
                    ("gain", 7.5),
                ],
            ),
            preset(
                tr("Loud: EDM maximum"),
                &[
                    ("ceiling", -0.5),
                    ("release", 60.0),
                    ("style", 2.0),
                    ("gain", 9.5),
                ],
            ),
            preset(
                tr("Bus: drums, peak control"),
                &[
                    ("ceiling", -3.0),
                    ("release", 30.0),
                    ("style", 1.0),
                    ("gain", 3.0),
                ],
            ),
            preset(
                tr("Bus: instruments, firm"),
                &[
                    ("ceiling", -3.0),
                    ("release", 120.0),
                    ("style", 0.0),
                    ("gain", 3.0),
                ],
            ),
            preset(
                tr("Bus: vocals, upfront"),
                &[
                    ("ceiling", -3.0),
                    ("release", 80.0),
                    ("style", 1.0),
                    ("gain", 4.0),
                ],
            ),
        ],
        "tape" => vec![
            preset(
                tr("Mix bus: glue 30 ips"),
                &[
                    ("machine", 1.0),
                    ("speed", 2.0),
                    ("tape", 0.0),
                    ("drive", 2.0),
                    ("bump", 1.0),
                    ("wow", 4.0),
                    ("output", -0.5),
                ],
            ),
            preset(
                tr("Mix bus: warm 15 ips"),
                &[
                    ("machine", 0.0),
                    ("speed", 1.0),
                    ("tape", 1.0),
                    ("drive", 4.0),
                    ("bump", 2.0),
                    ("wow", 6.0),
                    ("output", -0.5),
                ],
            ),
            preset(
                tr("Mix bus: vintage"),
                &[
                    ("machine", 2.0),
                    ("speed", 1.0),
                    ("tape", 1.0),
                    ("drive", 5.0),
                    ("bump", 2.5),
                    ("wow", 10.0),
                    ("hiss", 8.0),
                    ("output", -1.0),
                ],
            ),
            preset(
                tr("Mastering: transparent 30 ips"),
                &[
                    ("machine", 1.0),
                    ("speed", 2.0),
                    ("tape", 0.0),
                    ("drive", 0.0),
                    ("bump", 0.8),
                    ("wow", 2.0),
                    ("output", -0.5),
                ],
            ),
            preset(
                tr("Mastering: analog sheen"),
                &[
                    ("machine", 1.0),
                    ("speed", 2.0),
                    ("tape", 1.0),
                    ("drive", 3.0),
                    ("bump", 1.2),
                    ("wow", 3.0),
                    ("bias", 10.0),
                    ("output", -0.5),
                ],
            ),
            preset(
                tr("Mastering: round 15 ips"),
                &[
                    ("machine", 1.0),
                    ("speed", 1.0),
                    ("tape", 1.0),
                    ("drive", 2.0),
                    ("bump", 1.8),
                    ("wow", 3.0),
                    ("output", -0.5),
                ],
            ),
            preset(
                tr("Drums: punchy 15 ips"),
                &[
                    ("machine", 0.0),
                    ("speed", 1.0),
                    ("tape", 1.0),
                    ("drive", 8.0),
                    ("bump", 2.5),
                    ("bias", 15.0),
                    ("output", -1.0),
                ],
            ),
            preset(
                tr("Drums: crushed room"),
                &[
                    ("machine", 2.0),
                    ("speed", 0.0),
                    ("tape", 2.0),
                    ("drive", 16.0),
                    ("bump", 3.0),
                    ("wow", 8.0),
                    ("mix", 70.0),
                    ("output", 2.0),
                ],
            ),
            preset(
                tr("Drums: parallel smash"),
                &[
                    ("machine", 0.0),
                    ("speed", 1.0),
                    ("tape", 2.0),
                    ("drive", 18.0),
                    ("bump", 3.0),
                    ("mix", 45.0),
                    ("output", 1.5),
                ],
            ),
            preset(
                tr("Bass: round 15 ips"),
                &[
                    ("machine", 0.0),
                    ("speed", 1.0),
                    ("tape", 1.0),
                    ("drive", 6.0),
                    ("bump", 3.5),
                    ("output", -1.5),
                ],
            ),
            preset(
                tr("Bass: thick 7.5 ips"),
                &[
                    ("machine", 2.0),
                    ("speed", 0.0),
                    ("tape", 1.0),
                    ("drive", 8.0),
                    ("bump", 4.5),
                    ("output", -2.0),
                ],
            ),
            preset(
                tr("Bass: tight 30 ips"),
                &[
                    ("machine", 0.0),
                    ("speed", 2.0),
                    ("tape", 0.0),
                    ("drive", 5.0),
                    ("bump", 1.5),
                    ("output", -1.0),
                ],
            ),
            preset(
                tr("Vocal: smooth 30 ips"),
                &[
                    ("machine", 1.0),
                    ("speed", 2.0),
                    ("tape", 1.0),
                    ("drive", 3.0),
                    ("bump", 0.0),
                    ("wow", 3.0),
                    ("output", 0.0),
                ],
            ),
            preset(
                tr("Vocal: vintage warmth"),
                &[
                    ("machine", 2.0),
                    ("speed", 1.0),
                    ("tape", 1.0),
                    ("drive", 5.0),
                    ("bump", 1.0),
                    ("wow", 8.0),
                    ("output", 1.5),
                ],
            ),
            preset(
                tr("Vocal: pushed hard"),
                &[
                    ("machine", 0.0),
                    ("speed", 1.0),
                    ("tape", 2.0),
                    ("drive", 10.0),
                    ("bump", 0.5),
                    ("bias", 10.0),
                    ("output", 2.0),
                ],
            ),
            preset(
                tr("Instrument: guitars, glue 15 ips"),
                &[
                    ("machine", 0.0),
                    ("speed", 1.0),
                    ("tape", 1.0),
                    ("drive", 5.0),
                    ("bump", 1.5),
                    ("output", -0.5),
                ],
            ),
            preset(
                tr("Instrument: keys, warm wobble"),
                &[
                    ("machine", 2.0),
                    ("speed", 0.0),
                    ("tape", 1.0),
                    ("drive", 4.0),
                    ("bump", 2.5),
                    ("wow", 25.0),
                    ("output", -0.5),
                ],
            ),
            preset(
                tr("Instrument: acoustic guitar, soft top"),
                &[
                    ("machine", 2.0),
                    ("speed", 1.0),
                    ("tape", 2.0),
                    ("drive", 4.0),
                    ("bump", 1.0),
                    ("wow", 5.0),
                    ("output", -0.5),
                ],
            ),
            preset(
                tr("Lo-fi: cassette demo"),
                &[
                    ("machine", 3.0),
                    ("speed", 1.0),
                    ("tape", 1.0),
                    ("drive", 6.0),
                    ("bump", 2.0),
                    ("wow", 10.0),
                    ("hiss", 35.0),
                    ("output", -0.5),
                ],
            ),
            preset(
                tr("Lo-fi: worn cassette"),
                &[
                    ("machine", 3.0),
                    ("speed", 0.0),
                    ("tape", 2.0),
                    ("drive", 9.0),
                    ("bump", 2.5),
                    ("wow", 25.0),
                    ("hiss", 55.0),
                    ("bias", -30.0),
                    ("output", 1.0),
                ],
            ),
            preset(
                tr("Lo-fi: warbly tape"),
                &[
                    ("machine", 2.0),
                    ("speed", 0.0),
                    ("tape", 2.0),
                    ("drive", 7.0),
                    ("wow", 45.0),
                    ("hiss", 25.0),
                    ("output", 0.0),
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
                    ("output", 0.0),
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
                    ("output", 0.0),
                ],
            ),
            preset(
                tr("Guitar: bass, slow flange"),
                &[
                    ("rate", 0.1),
                    ("depth", 50.0),
                    ("delay", 2.5),
                    ("feedback", 30.0),
                    ("mix", 35.0),
                    ("stereo", 0.0),
                    ("output", 1.0),
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
                    ("output", 0.0),
                ],
            ),
            preset(
                tr("Vocal: soft doubling"),
                &[
                    ("rate", 0.4),
                    ("depth", 25.0),
                    ("delay", 4.0),
                    ("feedback", 10.0),
                    ("mix", 25.0),
                    ("output", 0.0),
                ],
            ),
            preset(
                tr("Vocal: robot tube"),
                &[
                    ("rate", 0.05),
                    ("depth", 10.0),
                    ("delay", 6.0),
                    ("feedback", 88.0),
                    ("mix", 50.0),
                    ("output", -0.5),
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
                    ("output", -0.5),
                ],
            ),
            preset(
                tr("Synth: 1 bar sweep"),
                &[
                    ("sync", 1.0),
                    ("note", 11.0),
                    ("depth", 80.0),
                    ("delay", 1.5),
                    ("feedback", 65.0),
                    ("mix", 50.0),
                    ("output", 0.0),
                ],
            ),
            preset(
                tr("Synth: pad, wide motion"),
                &[
                    ("rate", 0.3),
                    ("depth", 40.0),
                    ("delay", 5.0),
                    ("feedback", 20.0),
                    ("mix", 40.0),
                    ("stereo", 180.0),
                    ("output", 0.0),
                ],
            ),
            preset(
                tr("Synth: hollow drain"),
                &[
                    ("rate", 0.12),
                    ("depth", 75.0),
                    ("delay", 3.0),
                    ("feedback", -80.0),
                    ("mix", 50.0),
                    ("output", -0.5),
                ],
            ),
            preset(
                tr("Drums: hi-hats, swirl"),
                &[
                    ("rate", 0.2),
                    ("depth", 80.0),
                    ("delay", 0.5),
                    ("feedback", 60.0),
                    ("mix", 50.0),
                    ("stereo", 120.0),
                    ("output", 1.0),
                ],
            ),
            preset(
                tr("Drums: bus, through-zero"),
                &[
                    ("rate", 0.1),
                    ("depth", 100.0),
                    ("delay", 1.0),
                    ("feedback", -40.0),
                    ("mix", 50.0),
                    ("stereo", 0.0),
                    ("through_zero", 1.0),
                    ("output", -0.5),
                ],
            ),
            preset(
                tr("Drums: snare, tape flange"),
                &[
                    ("rate", 0.25),
                    ("depth", 60.0),
                    ("delay", 1.5),
                    ("feedback", 0.0),
                    ("mix", 50.0),
                    ("stereo", 0.0),
                    ("through_zero", 1.0),
                    ("output", -1.0),
                ],
            ),
        ],
        "phaser" => vec![
            preset(
                tr("Guitar: classic 4-stage"),
                &[("rate", 0.5), ("feedback", 30.0), ("stages", 0.0)],
            ),
            preset(
                tr("Guitar: funk rhythm"),
                &[
                    ("rate", 1.5),
                    ("depth", 80.0),
                    ("feedback", 20.0),
                    ("mix", 50.0),
                    ("stages", 0.0),
                ],
            ),
            preset(
                tr("Guitar: fast vibe"),
                &[
                    ("rate", 4.5),
                    ("depth", 70.0),
                    ("feedback", 10.0),
                    ("mix", 60.0),
                    ("stages", 1.0),
                ],
            ),
            preset(
                tr("Keys: electric piano swirl"),
                &[
                    ("rate", 0.3),
                    ("depth", 70.0),
                    ("feedback", 40.0),
                    ("mix", 50.0),
                    ("stages", 1.0),
                    ("stereo", 120.0),
                ],
            ),
            preset(
                tr("Keys: clav funk"),
                &[
                    ("rate", 2.2),
                    ("depth", 85.0),
                    ("feedback", 35.0),
                    ("mix", 55.0),
                    ("stages", 0.0),
                ],
            ),
            preset(
                tr("Keys: organ, slow rotor"),
                &[
                    ("rate", 0.9),
                    ("depth", 60.0),
                    ("feedback", 15.0),
                    ("mix", 45.0),
                    ("stages", 1.0),
                    ("stereo", 180.0),
                ],
            ),
            preset(
                tr("Synth: deep swirl"),
                &[
                    ("rate", 0.2),
                    ("depth", 100.0),
                    ("feedback", 65.0),
                    ("stages", 2.0),
                ],
            ),
            preset(
                tr("Synth: 1/2 synced sweep"),
                &[
                    ("sync", 1.0),
                    ("note", 10.0),
                    ("depth", 90.0),
                    ("feedback", 50.0),
                    ("mix", 50.0),
                    ("stages", 2.0),
                ],
            ),
            preset(
                tr("Synth: pad, slow 12-stage"),
                &[
                    ("rate", 0.05),
                    ("depth", 100.0),
                    ("feedback", 50.0),
                    ("mix", 50.0),
                    ("stages", 3.0),
                    ("stereo", 180.0),
                ],
            ),
            preset(
                tr("Synth: resonant 1 bar"),
                &[
                    ("sync", 1.0),
                    ("note", 11.0),
                    ("depth", 95.0),
                    ("feedback", 75.0),
                    ("mix", 50.0),
                    ("stages", 2.0),
                    ("low", 300.0),
                    ("high", 5000.0),
                ],
            ),
            preset(
                tr("FX: vocal, subtle movement"),
                &[
                    ("rate", 0.2),
                    ("depth", 40.0),
                    ("feedback", 10.0),
                    ("mix", 25.0),
                    ("stages", 0.0),
                ],
            ),
            preset(
                tr("FX: drums, breakbeat sweep"),
                &[
                    ("rate", 0.15),
                    ("depth", 90.0),
                    ("feedback", 60.0),
                    ("low", 200.0),
                    ("high", 6000.0),
                    ("mix", 40.0),
                    ("stages", 1.0),
                ],
            ),
            preset(
                tr("FX: hi-hats, jet stream"),
                &[
                    ("rate", 0.1),
                    ("depth", 100.0),
                    ("feedback", 70.0),
                    ("low", 800.0),
                    ("high", 10_000.0),
                    ("mix", 50.0),
                    ("stages", 3.0),
                ],
            ),
            preset(
                tr("FX: whole mix, slow notch"),
                &[
                    ("rate", 0.05),
                    ("depth", 70.0),
                    ("feedback", -30.0),
                    ("mix", 35.0),
                    ("stages", 0.0),
                    ("stereo", 0.0),
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
                tr("Master: gentle widen"),
                &[
                    ("low_width", 100.0),
                    ("mid_width", 108.0),
                    ("high_width", 118.0),
                    ("mono_bass", 100.0),
                ],
            ),
            preset(
                tr("Master: focus the center"),
                &[
                    ("low_width", 90.0),
                    ("mid_width", 88.0),
                    ("high_width", 100.0),
                    ("mono_bass", 150.0),
                ],
            ),
            preset(
                tr("Bus: drums, wide overheads"),
                &[
                    ("low_width", 100.0),
                    ("mid_width", 110.0),
                    ("high_width", 140.0),
                    ("mono_bass", 100.0),
                ],
            ),
            preset(
                tr("Bus: guitars, wide doubles"),
                &[
                    ("low_width", 100.0),
                    ("mid_width", 130.0),
                    ("high_width", 130.0),
                    ("low_split", 200.0),
                    ("mono_bass", 120.0),
                ],
            ),
            preset(
                tr("Bus: pads, super wide"),
                &[
                    ("low_width", 120.0),
                    ("mid_width", 160.0),
                    ("high_width", 180.0),
                    ("mono_bass", 200.0),
                ],
            ),
            preset(
                tr("Bus: backing vocals, spread"),
                &[
                    ("low_width", 100.0),
                    ("mid_width", 140.0),
                    ("high_width", 150.0),
                    ("low_split", 300.0),
                    ("mono_bass", 180.0),
                ],
            ),
            preset(
                tr("Fix: mono source, stereoize"),
                &[
                    ("mid_width", 100.0),
                    ("high_width", 100.0),
                    ("mono_bass", 150.0),
                    ("stereoize", 60.0),
                    ("stereoize_time", 12.0),
                ],
            ),
            preset(
                tr("Fix: bass, mono lows"),
                &[
                    ("low_width", 0.0),
                    ("mid_width", 100.0),
                    ("high_width", 120.0),
                    ("low_split", 250.0),
                    ("mono_bass", 250.0),
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
                tr("Fix: check in mono"),
                &[("low_width", 0.0), ("mid_width", 0.0), ("high_width", 0.0)],
            ),
        ],
        "transient" => vec![
            preset(
                tr("Drums: kick punch"),
                &[
                    ("attack", 22.0),
                    ("sustain", -20.0),
                    ("speed", 8.0),
                    ("output", -5.5),
                ],
            ),
            preset(
                tr("Drums: snare crack"),
                &[
                    ("attack", 22.0),
                    ("sustain", 0.0),
                    ("speed", 5.0),
                    ("output", -4.5),
                ],
            ),
            preset(
                tr("Drums: snare, fat sustain"),
                &[
                    ("attack", 8.0),
                    ("sustain", 40.0),
                    ("speed", 15.0),
                    ("output", -3.0),
                ],
            ),
            preset(
                tr("Drums: toms, shorter ring"),
                &[
                    ("attack", 12.0),
                    ("sustain", -40.0),
                    ("speed", 15.0),
                    ("output", 0.0),
                ],
            ),
            preset(
                tr("Drums: bus, tight and punchy"),
                &[
                    ("attack", 15.0),
                    ("sustain", -35.0),
                    ("speed", 12.0),
                    ("output", -1.5),
                ],
            ),
            preset(
                tr("Drums: bus, more room"),
                &[
                    ("attack", 0.0),
                    ("sustain", 60.0),
                    ("speed", 20.0),
                    ("output", -4.0),
                ],
            ),
            preset(
                tr("Guitar: acoustic, pick attack"),
                &[
                    ("attack", 20.0),
                    ("sustain", -10.0),
                    ("speed", 6.0),
                    ("output", -1.0),
                ],
            ),
            preset(
                tr("Guitar: funk bite"),
                &[
                    ("attack", 22.0),
                    ("sustain", -15.0),
                    ("speed", 5.0),
                    ("output", -1.0),
                ],
            ),
            preset(
                tr("Guitar: softer strumming"),
                &[
                    ("attack", -30.0),
                    ("sustain", 10.0),
                    ("speed", 10.0),
                    ("output", 2.0),
                ],
            ),
            preset(
                tr("Bass: pluck definition"),
                &[
                    ("attack", 18.0),
                    ("sustain", 0.0),
                    ("speed", 8.0),
                    ("output", -1.5),
                ],
            ),
            preset(
                tr("Bass: longer notes"),
                &[
                    ("attack", 0.0),
                    ("sustain", 50.0),
                    ("speed", 20.0),
                    ("output", -3.0),
                ],
            ),
            preset(
                tr("Bass: slap, tamed"),
                &[
                    ("attack", -35.0),
                    ("sustain", 0.0),
                    ("speed", 6.0),
                    ("output", 2.5),
                ],
            ),
            preset(
                tr("Keys: piano, more hammer"),
                &[
                    ("attack", 18.0),
                    ("sustain", 0.0),
                    ("speed", 8.0),
                    ("output", -1.0),
                ],
            ),
            preset(
                tr("Keys: piano, softer touch"),
                &[
                    ("attack", -30.0),
                    ("sustain", 15.0),
                    ("speed", 10.0),
                    ("output", 1.5),
                ],
            ),
            preset(
                tr("Keys: plucks pop out"),
                &[
                    ("attack", 28.0),
                    ("sustain", -30.0),
                    ("speed", 6.0),
                    ("output", 0.0),
                ],
            ),
            preset(
                tr("Fix: soften attacks"),
                &[("attack", -40.0), ("sustain", 10.0), ("output", 2.0)],
            ),
            preset(
                tr("Fix: less room"),
                &[
                    ("attack", 0.0),
                    ("sustain", -60.0),
                    ("speed", 20.0),
                    ("output", 2.5),
                ],
            ),
            preset(
                tr("Fix: clicky kick, tamed"),
                &[
                    ("attack", -25.0),
                    ("sustain", 0.0),
                    ("speed", 3.0),
                    ("output", 3.0),
                ],
            ),
        ],
        "pitch_correct" => vec![
            preset(
                tr("Vocal: hard tune"),
                &[("speed", 0.0), ("humanize", 0.0), ("amount", 100.0)],
            ),
            preset(
                tr("Vocal: pop, tight"),
                &[("speed", 15.0), ("humanize", 20.0), ("amount", 100.0)],
            ),
            preset(
                tr("Vocal: natural, keeps the vibrato"),
                &[("speed", 60.0), ("humanize", 70.0), ("amount", 100.0)],
            ),
            preset(
                tr("Vocal: light touch"),
                &[("speed", 150.0), ("humanize", 60.0), ("amount", 60.0)],
            ),
            preset(
                tr("Backing vocals: locked"),
                &[("speed", 10.0), ("humanize", 0.0), ("amount", 100.0)],
            ),
            preset(
                tr("FX: octave down"),
                &[("speed", 20.0), ("humanize", 50.0), ("transpose", -12.0)],
            ),
        ],
        "deesser" => vec![
            preset(
                tr("Vocal: female, standard"),
                &[
                    ("frequency", 7500.0),
                    ("range", 8.0),
                    ("relative", 1.0),
                    ("threshold", -6.0),
                ],
            ),
            preset(
                tr("Vocal: female, bright and harsh"),
                &[
                    ("frequency", 8500.0),
                    ("range", 10.0),
                    ("relative", 1.0),
                    ("lookahead", 1.0),
                    ("threshold", -10.0),
                ],
            ),
            preset(
                tr("Vocal: male, standard"),
                &[
                    ("frequency", 5500.0),
                    ("range", 8.0),
                    ("relative", 1.0),
                    ("threshold", -6.0),
                ],
            ),
            preset(
                tr("Vocal: male, gentle"),
                &[
                    ("frequency", 6000.0),
                    ("range", 4.0),
                    ("relative", 1.0),
                    ("threshold", -2.0),
                ],
            ),
            preset(
                tr("Vocal: wide-band, natural"),
                &[
                    ("frequency", 6500.0),
                    ("mode", 1.0),
                    ("range", 6.0),
                    ("relative", 1.0),
                    ("threshold", -3.0),
                ],
            ),
            preset(
                tr("Vocal: backing, strong"),
                &[
                    ("frequency", 7000.0),
                    ("range", 12.0),
                    ("relative", 1.0),
                    ("lookahead", 1.0),
                    ("threshold", -11.0),
                ],
            ),
            preset(
                tr("Voice-over: broadcast"),
                &[
                    ("frequency", 6000.0),
                    ("range", 8.0),
                    ("relative", 1.0),
                    ("threshold", -7.0),
                ],
            ),
            preset(
                tr("Voice-over: podcast, close mic"),
                &[
                    ("frequency", 5500.0),
                    ("range", 10.0),
                    ("relative", 1.0),
                    ("detector", 1.0),
                    ("lookahead", 1.0),
                    ("threshold", -14.0),
                ],
            ),
            preset(
                tr("Voice-over: audiobook, gentle"),
                &[
                    ("frequency", 6500.0),
                    ("range", 5.0),
                    ("relative", 1.0),
                    ("release", 90.0),
                    ("threshold", -3.0),
                ],
            ),
            preset(
                tr("Instrument: cymbals, harsh"),
                &[("frequency", 9000.0), ("range", 6.0), ("threshold", -46.0)],
            ),
            preset(
                tr("Instrument: hi-hat bleed"),
                &[
                    ("frequency", 8000.0),
                    ("range", 9.0),
                    ("attack", 0.2),
                    ("release", 40.0),
                    ("threshold", -43.0),
                ],
            ),
            preset(
                tr("Instrument: acoustic guitar, string squeaks"),
                &[
                    ("frequency", 4000.0),
                    ("range", 8.0),
                    ("detector", 1.0),
                    ("threshold", -25.0),
                ],
            ),
        ],
        // The defaults boost lows and highs a little: each preset sets every boost and cut.
        "vintage_eq" => vec![
            preset(
                tr("Drums: kick, low end trick"),
                &[
                    ("low_freq", 2.0),
                    ("low_boost", 6.0),
                    ("low_cut", 3.0),
                    ("high_boost", 0.0),
                    ("output", 1.0),
                ],
            ),
            preset(
                tr("Drums: snare snap"),
                &[
                    ("low_freq", 3.0),
                    ("low_boost", 3.0),
                    ("high_freq", 2.0),
                    ("high_boost", 5.0),
                    ("bandwidth", 3.0),
                    ("low_cut", 0.0),
                    ("output", 0.5),
                ],
            ),
            preset(
                tr("Drums: bus, weight and silk"),
                &[
                    ("low_freq", 2.0),
                    ("low_boost", 3.0),
                    ("low_cut", 1.0),
                    ("high_freq", 4.0),
                    ("high_boost", 3.0),
                    ("bandwidth", 7.0),
                    ("output", 0.0),
                ],
            ),
            preset(
                tr("Bass: low end trick 60 Hz"),
                &[
                    ("low_freq", 2.0),
                    ("low_boost", 5.0),
                    ("low_cut", 4.0),
                    ("warmth", 30.0),
                    ("high_boost", 0.0),
                    ("output", 2.0),
                ],
            ),
            preset(
                tr("Bass: round and warm"),
                &[
                    ("low_freq", 3.0),
                    ("low_boost", 4.0),
                    ("cut_freq", 1.0),
                    ("high_cut", 3.0),
                    ("warmth", 50.0),
                    ("low_cut", 0.0),
                    ("high_boost", 0.0),
                    ("output", -2.5),
                ],
            ),
            preset(
                tr("Bass: tight low end"),
                &[
                    ("low_freq", 1.0),
                    ("low_boost", 4.0),
                    ("low_cut", 6.0),
                    ("warmth", 20.0),
                    ("high_boost", 0.0),
                    ("output", 1.5),
                ],
            ),
            preset(
                tr("Vocal: presence 5 kHz"),
                &[
                    ("high_freq", 2.0),
                    ("high_boost", 3.0),
                    ("bandwidth", 4.0),
                    ("low_boost", 0.0),
                    ("low_cut", 0.0),
                    ("output", -1.5),
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
                    ("output", -2.0),
                ],
            ),
            preset(
                tr("Vocal: body and air"),
                &[
                    ("low_freq", 3.0),
                    ("low_boost", 2.0),
                    ("high_freq", 5.0),
                    ("high_boost", 3.5),
                    ("bandwidth", 7.0),
                    ("low_cut", 0.0),
                    ("output", -1.5),
                ],
            ),
            preset(
                tr("Vocal: smooth top"),
                &[
                    ("high_freq", 0.0),
                    ("high_boost", 2.0),
                    ("bandwidth", 6.0),
                    ("cut_freq", 1.0),
                    ("high_cut", 3.0),
                    ("warmth", 40.0),
                    ("low_boost", 0.0),
                    ("low_cut", 0.0),
                    ("output", -1.0),
                ],
            ),
            preset(
                tr("Guitar: acoustic, sparkle"),
                &[
                    ("low_freq", 3.0),
                    ("low_cut", 3.0),
                    ("high_freq", 3.0),
                    ("high_boost", 4.0),
                    ("bandwidth", 6.0),
                    ("low_boost", 0.0),
                    ("output", 0.5),
                ],
            ),
            preset(
                tr("Guitar: electric, bite 3 kHz"),
                &[
                    ("high_freq", 0.0),
                    ("high_boost", 4.0),
                    ("bandwidth", 3.0),
                    ("low_boost", 0.0),
                    ("low_cut", 0.0),
                    ("output", -0.5),
                ],
            ),
            preset(
                tr("Guitar: electric, dark and fat"),
                &[
                    ("low_freq", 3.0),
                    ("low_boost", 3.0),
                    ("cut_freq", 0.0),
                    ("high_cut", 4.0),
                    ("warmth", 45.0),
                    ("low_cut", 0.0),
                    ("high_boost", 0.0),
                    ("output", 0.0),
                ],
            ),
            preset(
                tr("Master: sheen"),
                &[
                    ("low_freq", 1.0),
                    ("low_boost", 2.0),
                    ("low_cut", 0.0),
                    ("high_freq", 6.0),
                    ("high_boost", 2.5),
                    ("warmth", 30.0),
                    ("output", 0.0),
                ],
            ),
            preset(
                tr("Master: warm and dark"),
                &[
                    ("low_boost", 3.0),
                    ("low_cut", 2.0),
                    ("cut_freq", 1.0),
                    ("high_cut", 4.0),
                    ("warmth", 50.0),
                    ("high_boost", 0.0),
                    ("output", 1.0),
                ],
            ),
            preset(
                tr("Master: tube glue"),
                &[
                    ("warmth", 45.0),
                    ("low_boost", 0.0),
                    ("low_cut", 0.0),
                    ("high_boost", 0.0),
                    ("output", 0.5),
                ],
            ),
            preset(
                tr("Master: low end trick and air"),
                &[
                    ("low_freq", 2.0),
                    ("low_boost", 3.0),
                    ("low_cut", 1.5),
                    ("high_freq", 6.0),
                    ("high_boost", 2.5),
                    ("bandwidth", 8.0),
                    ("warmth", 20.0),
                    ("output", 0.5),
                ],
            ),
        ],
        "filter" => vec![
            preset(
                tr("Sweep: slow low-pass, 2 bars"),
                &[
                    ("type", 0.0),
                    ("cutoff", 800.0),
                    ("resonance", 50.0),
                    ("lfo_depth", 70.0),
                    ("sync", 1.0),
                    ("note", 5.0),
                    ("output", 1.0),
                ],
            ),
            preset(
                tr("Sweep: high-pass, 1 bar"),
                &[
                    ("type", 1.0),
                    ("cutoff", 500.0),
                    ("resonance", 40.0),
                    ("lfo_depth", 65.0),
                    ("sync", 1.0),
                    ("note", 4.0),
                    ("lfo_shape", 1.0),
                    ("output", 4.0),
                ],
            ),
            preset(
                tr("Sweep: stereo band-pass drift"),
                &[
                    ("type", 2.0),
                    ("cutoff", 1200.0),
                    ("resonance", 35.0),
                    ("lfo_rate", 0.15),
                    ("lfo_depth", 55.0),
                    ("lfo_phase", 90.0),
                    ("mix", 70.0),
                    ("output", 4.5),
                ],
            ),
            preset(
                tr("Sweep: random steps 1/16"),
                &[
                    ("type", 0.0),
                    ("cutoff", 1200.0),
                    ("resonance", 50.0),
                    ("lfo_depth", 45.0),
                    ("sync", 1.0),
                    ("note", 0.0),
                    ("lfo_shape", 5.0),
                    ("slope", 1.0),
                    ("output", 0.5),
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
                tr("Transition: narrow band, far away"),
                &[("type", 2.0), ("cutoff", 900.0), ("resonance", 45.0)],
            ),
            preset(
                tr("Rhythm: 1/16 filter gate"),
                &[
                    ("type", 0.0),
                    ("cutoff", 600.0),
                    ("resonance", 40.0),
                    ("lfo_depth", 80.0),
                    ("sync", 1.0),
                    ("note", 0.0),
                    ("lfo_shape", 4.0),
                    ("output", 1.0),
                ],
            ),
            preset(
                tr("Rhythm: wobble 1/8"),
                &[
                    ("type", 0.0),
                    ("cutoff", 300.0),
                    ("resonance", 60.0),
                    ("drive", 6.0),
                    ("lfo_depth", 80.0),
                    ("sync", 1.0),
                    ("note", 1.0),
                    ("slope", 1.0),
                    ("output", 0.0),
                ],
            ),
            preset(
                tr("Rhythm: pump 1/4"),
                &[
                    ("type", 0.0),
                    ("cutoff", 700.0),
                    ("resonance", 30.0),
                    ("lfo_depth", 75.0),
                    ("sync", 1.0),
                    ("note", 2.0),
                    ("lfo_shape", 2.0),
                    ("output", 1.5),
                ],
            ),
            preset(
                tr("Rhythm: high-pass chop 1/8"),
                &[
                    ("type", 1.0),
                    ("cutoff", 400.0),
                    ("resonance", 35.0),
                    ("lfo_depth", 70.0),
                    ("sync", 1.0),
                    ("note", 1.0),
                    ("lfo_shape", 4.0),
                    ("output", 3.0),
                ],
            ),
            preset(
                tr("Envelope: guitar auto-wah"),
                &[
                    ("type", 2.0),
                    ("cutoff", 600.0),
                    ("resonance", 55.0),
                    ("envelope", 80.0),
                    ("output", 5.0),
                ],
            ),
            preset(
                tr("Envelope: bass funk"),
                &[
                    ("type", 0.0),
                    ("cutoff", 300.0),
                    ("resonance", 60.0),
                    ("drive", 4.0),
                    ("envelope", 70.0),
                    ("output", -2.0),
                ],
            ),
            preset(
                tr("Envelope: keys quack"),
                &[
                    ("type", 0.0),
                    ("cutoff", 500.0),
                    ("resonance", 65.0),
                    ("envelope", 75.0),
                    ("env_attack", 2.0),
                    ("env_release", 120.0),
                    ("slope", 1.0),
                    ("output", 0.0),
                ],
            ),
            preset(
                tr("Envelope: drums, ducking highs"),
                &[
                    ("type", 0.0),
                    ("cutoff", 9000.0),
                    ("resonance", 15.0),
                    ("envelope", -70.0),
                    ("env_attack", 1.0),
                    ("env_release", 200.0),
                    ("output", 0.5),
                ],
            ),
            preset(
                tr("Tone: radio voice"),
                &[
                    ("type", 2.0),
                    ("cutoff", 1800.0),
                    ("resonance", 20.0),
                    ("drive", 8.0),
                    ("output", 0.5),
                ],
            ),
            preset(
                tr("Tone: next room"),
                &[
                    ("type", 0.0),
                    ("cutoff", 700.0),
                    ("resonance", 10.0),
                    ("output", 0.5),
                ],
            ),
            preset(
                tr("Tone: warm low-pass"),
                &[
                    ("type", 0.0),
                    ("cutoff", 7000.0),
                    ("resonance", 5.0),
                    ("output", 0.0),
                ],
            ),
            preset(
                tr("Tone: thin and distant"),
                &[
                    ("type", 1.0),
                    ("cutoff", 900.0),
                    ("resonance", 15.0),
                    ("slope", 1.0),
                    ("output", 1.5),
                ],
            ),
            preset(
                tr("DJ: low-pass with drive"),
                &[
                    ("type", 0.0),
                    ("cutoff", 2000.0),
                    ("resonance", 35.0),
                    ("drive", 6.0),
                    ("slope", 1.0),
                    ("output", -2.0),
                ],
            ),
            preset(
                tr("DJ: high-pass with drive"),
                &[
                    ("type", 1.0),
                    ("cutoff", 150.0),
                    ("resonance", 35.0),
                    ("drive", 6.0),
                    ("slope", 1.0),
                    ("output", -0.5),
                ],
            ),
            preset(
                tr("DJ: mids only"),
                &[
                    ("type", 2.0),
                    ("cutoff", 1000.0),
                    ("resonance", 10.0),
                    ("drive", 3.0),
                    ("output", 3.5),
                ],
            ),
        ],
        "utility" => vec![
            preset(
                tr("Fix: flip polarity"),
                &[("invert_left", 1.0), ("invert_right", 1.0)],
            ),
            preset(tr("Fix: flip left polarity"), &[("invert_left", 1.0)]),
            preset(tr("Fix: flip right polarity"), &[("invert_right", 1.0)]),
            preset(tr("Fix: swap channels"), &[("swap", 1.0)]),
            preset(tr("Fix: remove DC offset"), &[("dc", 1.0)]),
            preset(tr("Fix: mono bass below 120 Hz"), &[("bass_mono", 120.0)]),
            preset(tr("Gain: -6 dB headroom"), &[("gain", -6.0)]),
            preset(tr("Gain: -12 dB before a hot plugin"), &[("gain", -12.0)]),
            preset(tr("Gain: +6 dB"), &[("gain", 6.0)]),
            preset(tr("Width: mono check"), &[("mono", 1.0)]),
            preset(tr("Width: narrow 50%"), &[("width", 50.0)]),
            preset(tr("Width: wide 130%"), &[("width", 130.0)]),
            preset(tr("Width: Haas widener 12 ms"), &[("delay_right", 12.0)]),
        ],
        "amp_rig" => vec![
            preset(
                tr("Clean: sparkle"),
                &[
                    ("amp", 1.0),
                    ("cab", 3.0),
                    ("gain", 3.0),
                    ("treble", 6.0),
                    ("p1_type", 5.0),
                    ("p1_k1", 4.0),
                    ("p2_type", 6.0),
                    ("p2_k1", 3.0),
                    ("p2_k3", 4.0),
                    ("p3_type", 11.0),
                    ("p3_k3", 3.0),
                    ("master", -3.5),
                ],
            ),
            preset(
                tr("Clean: edge of breakup"),
                &[
                    ("amp", 2.0),
                    ("cab", 4.0),
                    ("gain", 6.0),
                    ("p1_type", 13.0),
                    ("p1_k1", 3.0),
                    ("p1_k3", 6.0),
                    ("p2_type", 11.0),
                    ("p2_k3", 2.5),
                    ("master", -3.0),
                ],
            ),
            preset(
                tr("Clean: jangle"),
                &[
                    ("amp", 6.0),
                    ("cab", 2.0),
                    ("gain", 4.0),
                    ("treble", 6.5),
                    ("p1_type", 5.0),
                    ("p1_k1", 5.0),
                    ("p2_type", 9.0),
                    ("p2_k1", 4.0),
                    ("p2_k2", 3.0),
                    ("p3_type", 11.0),
                    ("p3_k3", 3.0),
                    ("master", -2.0),
                ],
            ),
            preset(
                tr("Clean: warm jazz"),
                &[
                    ("amp", 3.0),
                    ("cab", 5.0),
                    ("gain", 3.0),
                    ("bass", 6.0),
                    ("treble", 3.5),
                    ("presence", 3.0),
                    ("p1_type", 11.0),
                    ("p1_k1", 4.0),
                    ("p1_k2", 3.0),
                    ("p1_k3", 2.0),
                    ("master", 4.5),
                ],
            ),
            preset(
                tr("Crunch: blues"),
                &[
                    ("amp", 7.0),
                    ("cab", 1.0),
                    ("gain", 5.0),
                    ("p1_type", 14.0),
                    ("p1_k1", 4.0),
                    ("p1_k3", 6.0),
                    ("p2_type", 20.0),
                    ("p2_k1", 3.5),
                    ("p2_k3", 2.5),
                    ("master", -5.0),
                ],
            ),
            preset(
                tr("Crunch: rock"),
                &[
                    ("amp", 8.0),
                    ("cab", 1.0),
                    ("gain", 7.0),
                    ("mid", 6.0),
                    ("p1_type", 2.0),
                    ("p1_k1", 3.0),
                    ("p1_k3", 7.0),
                    ("p2_type", 10.0),
                    ("p2_k2", 3.0),
                    ("p2_k3", 2.5),
                    ("master", -1.0),
                ],
            ),
            preset(
                tr("Crunch: tweed bite"),
                &[
                    ("amp", 4.0),
                    ("cab", 5.0),
                    ("gain", 7.5),
                    ("bass", 5.5),
                    ("treble", 6.0),
                    ("master", -3.5),
                ],
            ),
            preset(
                tr("Crunch: boutique edge"),
                &[
                    ("amp", 9.0),
                    ("cab", 2.0),
                    ("gain", 5.5),
                    ("mid", 5.5),
                    ("treble", 7.0),
                    ("presence", 7.0),
                    ("p1_type", 21.0),
                    ("p1_k1", 3.0),
                    ("p1_k2", 5.0),
                    ("p1_k3", 6.0),
                    ("p2_type", 11.0),
                    ("p2_k3", 2.5),
                    ("master", -4.5),
                ],
            ),
            preset(
                tr("Lead: singing"),
                &[
                    ("amp", 8.0),
                    ("cab", 0.0),
                    ("gain", 7.0),
                    ("mid", 6.5),
                    ("presence", 6.0),
                    ("gate", -60.0),
                    ("p1_type", 2.0),
                    ("p1_k1", 2.0),
                    ("p1_k3", 7.0),
                    ("p2_type", 20.0),
                    ("p2_k1", 6.0),
                    ("p2_k3", 3.0),
                    ("p3_type", 11.0),
                    ("p3_k1", 6.0),
                    ("p3_k3", 2.5),
                    ("master", 1.0),
                ],
            ),
            preset(
                tr("Lead: liquid"),
                &[
                    ("amp", 10.0),
                    ("cab", 0.0),
                    ("gain", 7.0),
                    ("mid", 6.0),
                    ("treble", 5.5),
                    ("gate", -60.0),
                    ("p1_type", 5.0),
                    ("p1_k1", 5.0),
                    ("p2_type", 20.0),
                    ("p2_k1", 5.5),
                    ("p2_k2", 4.0),
                    ("p2_k3", 3.5),
                    ("p3_type", 11.0),
                    ("p3_k1", 6.5),
                    ("p3_k2", 4.0),
                    ("p3_k3", 3.0),
                    ("master", -1.0),
                ],
            ),
            preset(
                tr("Lead: fuzz rock"),
                &[
                    ("amp", 8.0),
                    ("cab", 1.0),
                    ("gain", 4.0),
                    ("p1_type", 16.0),
                    ("p1_k1", 7.0),
                    ("p1_k2", 4.0),
                    ("p2_type", 11.0),
                    ("p2_k3", 2.0),
                    ("master", -3.5),
                ],
            ),
            preset(
                tr("High gain: metal"),
                &[
                    ("amp", 11.0),
                    ("cab", 0.0),
                    ("gain", 6.0),
                    ("bass", 6.0),
                    ("mid", 4.0),
                    ("treble", 6.0),
                    ("gate", -50.0),
                    ("p1_type", 2.0),
                    ("p1_k1", 0.0),
                    ("p1_k2", 6.0),
                    ("p1_k3", 8.0),
                    ("master", -3.0),
                ],
            ),
            preset(
                tr("High gain: modern rhythm"),
                &[
                    ("amp", 10.0),
                    ("cab", 0.0),
                    ("gain", 7.5),
                    ("bass", 5.5),
                    ("mid", 4.5),
                    ("treble", 6.0),
                    ("presence", 6.0),
                    ("gate", -50.0),
                    ("p1_type", 2.0),
                    ("p1_k1", 0.0),
                    ("p1_k2", 5.0),
                    ("p1_k3", 8.0),
                    ("master", -3.0),
                ],
            ),
            preset(
                tr("High gain: thrash scoop"),
                &[
                    ("amp", 11.0),
                    ("cab", 0.0),
                    ("gain", 8.0),
                    ("bass", 6.5),
                    ("mid", 3.0),
                    ("treble", 6.5),
                    ("presence", 6.5),
                    ("gate", -45.0),
                    ("p1_type", 18.0),
                    ("p1_k1", 3.0),
                    ("p1_k2", 6.0),
                    ("p1_k3", 6.0),
                    ("master", -5.5),
                ],
            ),
            preset(
                tr("Bass: classic"),
                &[
                    ("amp", 12.0),
                    ("cab", 7.0),
                    ("gain", 4.0),
                    ("bass", 6.5),
                    ("treble", 4.0),
                    ("p1_type", 5.0),
                    ("p1_k1", 6.0),
                    ("master", -14.5),
                ],
            ),
            preset(
                tr("Bass: flip-top, round"),
                &[
                    ("amp", 13.0),
                    ("cab", 8.0),
                    ("gain", 4.0),
                    ("bass", 6.0),
                    ("mid", 5.0),
                    ("treble", 3.5),
                    ("master", -8.5),
                ],
            ),
            preset(
                tr("Bass: driven rock"),
                &[
                    ("amp", 12.0),
                    ("cab", 6.0),
                    ("gain", 7.0),
                    ("bass", 5.5),
                    ("mid", 6.5),
                    ("treble", 5.5),
                    ("p1_type", 5.0),
                    ("p1_k1", 5.0),
                    ("p2_type", 2.0),
                    ("p2_k1", 4.0),
                    ("p2_k2", 6.0),
                    ("p2_k3", 6.0),
                    ("master", -7.5),
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
        let root = audiostrada_core::scratch::dir("presets");
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

    /// Each role of an effect offers a real choice without drowning it: three to six presets,
    /// under names that tell them apart.
    #[test]
    fn every_category_has_three_to_six_presets() {
        for info in audiostrada_dsp::EFFECTS {
            // The instruments keep banks of their own; the last two are reworked apart.
            if matches!(info.id, "synth" | "drums" | "pitch_correct" | "modulator") {
                continue;
            }
            let mut counts: BTreeMap<String, usize> = BTreeMap::new();
            let mut names = std::collections::BTreeSet::new();
            for preset in builtin_or_instrument(info.id) {
                let (category, _) = crate::effect_editors::preset_category(&preset.name)
                    .unwrap_or_else(|| panic!("{}: {} has no category", info.id, preset.name));
                *counts.entry(category).or_default() += 1;
                assert!(
                    names.insert(preset.name.clone()),
                    "{}: two presets named {}",
                    info.id,
                    preset.name
                );
            }
            for (category, count) in counts {
                assert!(
                    (3..=6).contains(&count),
                    "{}: {count} presets under {category}",
                    info.id
                );
            }
        }
    }

    /// A song-like signal through every built-in effect preset: none clips, runs away or
    /// falls silent.
    #[test]
    fn factory_presets_neither_clip_nor_run_away() {
        let rate = 48_000u32;
        let n = rate as usize * 2;
        let mut seed = 1u32;
        let mut noise = move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed as f32 / u32::MAX as f32 * 2.0 - 1.0
        };
        let tau = std::f32::consts::TAU;
        let input: Vec<[f32; 2]> = (0..n)
            .map(|i| {
                let t = i as f32 / rate as f32;
                let beat = t % 0.5;
                let kick =
                    (tau * (50.0 + 80.0 * (-beat * 30.0).exp()) * beat).sin() * (-beat * 8.0).exp();
                let bass = ((110.0 * t) % 1.0 * 2.0 - 1.0) * 0.25;
                let voice = (tau * 440.0 * t).sin() * 0.15;
                let hat = noise() * 0.08 * (-(t % 0.25) * 40.0).exp();
                let m = kick * 0.5 + bass + voice;
                [(m + hat) * 0.5, (m - hat * 0.5) * 0.5]
            })
            .collect();
        let rms = |x: &[[f32; 2]]| {
            let e: f32 = x.iter().map(|s| s[0] * s[0] + s[1] * s[1]).sum();
            10.0 * (e / (2 * x.len()) as f32).max(1e-12).log10()
        };
        let skip = rate as usize / 2;
        let reference = rms(&input[skip..]);
        for info in audiostrada_dsp::EFFECTS {
            for preset in builtin(info.id) {
                let Some(mut fx) = audiostrada_dsp::create(info.id) else {
                    continue;
                };
                fx.prepare(rate, 256);
                for (i, p) in info.params.iter().enumerate() {
                    fx.set_param(i, preset.params.get(p.id).copied().unwrap_or(p.default));
                }
                let mut out = input.clone();
                let (mut l, mut r) = (vec![0.0; 256], vec![0.0; 256]);
                for block in out.chunks_mut(256) {
                    let k = block.len();
                    for (j, s) in block.iter().enumerate() {
                        (l[j], r[j]) = (s[0], s[1]);
                    }
                    fx.process(&mut l[..k], &mut r[..k]);
                    for (j, s) in block.iter_mut().enumerate() {
                        *s = [l[j], r[j]];
                    }
                }
                let out = &out[skip..];
                let peak = out
                    .iter()
                    .map(|s| s[0].abs().max(s[1].abs()))
                    .fold(0.0, f32::max);
                let change = rms(out) - reference;
                assert!(
                    peak < 1.0 && (-20.0..10.0).contains(&change),
                    "{}: {} peaks at {peak}, level {change:+.1} dB",
                    info.id,
                    preset.name
                );
            }
        }
    }
}
