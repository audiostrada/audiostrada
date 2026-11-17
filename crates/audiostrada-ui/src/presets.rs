//! Presets: the built-in effects' factory presets and the user's files come from
//! `audiostrada_editors::presets`; AudioStrada adds its instruments' banks and its folder.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[cfg(test)]
use audiostrada_editors::presets::builtin;
pub use audiostrada_editors::presets::{Preset, delete, preset, save};

pub fn root() -> Option<PathBuf> {
    crate::config_dir().map(|d| audiostrada_editors::presets::root_in(&d))
}

pub fn list(root: Option<&Path>, effect: &str) -> Vec<Preset> {
    audiostrada_editors::presets::list_with(root, effect, factory(effect))
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
        _ => audiostrada_editors::presets::builtin(effect),
    }
}

fn factory(effect: &str) -> Vec<Preset> {
    match effect {
        "synth" => crate::synth_presets::factory(),
        "drums" => crate::drums_editor::factory(),
        "trigger" => crate::trigger_editor::factory(),
        _ => audiostrada_editors::presets::factory(effect),
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
        for effect in audiostrada_dsp::EFFECTS.iter() {
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
        for info in audiostrada_dsp::EFFECTS.iter() {
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
    /// under names that tell them apart, each name with a capital.
    #[test]
    fn every_category_has_three_to_six_presets() {
        for info in audiostrada_dsp::EFFECTS.iter() {
            // The instruments keep banks of their own; the last two are reworked apart.
            if matches!(info.id, "synth" | "drums" | "pitch_correct" | "modulator") {
                continue;
            }
            let mut counts: BTreeMap<String, usize> = BTreeMap::new();
            let mut names = std::collections::BTreeSet::new();
            for preset in builtin_or_instrument(info.id) {
                let (category, name) = crate::effect_editors::preset_category(&preset.name)
                    .unwrap_or_else(|| panic!("{}: {} has no category", info.id, preset.name));
                assert!(
                    !name.starts_with(char::is_lowercase),
                    "{}: {} starts without a capital",
                    info.id,
                    preset.name
                );
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
        for info in audiostrada_dsp::EFFECTS.iter() {
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
