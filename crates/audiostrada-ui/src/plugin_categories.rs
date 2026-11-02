//! Families of third-party plugins: guessed from what each plugin says it is (CLAP features,
//! VST3 subcategories, LV2 class), or chosen by the user, kept in `plugin_categories.json`.

use std::collections::BTreeMap;
use std::path::PathBuf;

/// Family of plugins whose kind is unknown.
pub const OTHER: &str = "Other";

/// Words of plugin tags, and the family they mean.
const WORDS: &[(&str, &[&str])] = &[
    (
        "Amp & Cab",
        &["amp", "amps", "amplifier", "cab", "cabinet", "guitar"],
    ),
    (
        "Dynamics",
        &[
            "compressor",
            "compression",
            "limiter",
            "gate",
            "expander",
            "dynamics",
            "transient",
            "esser",
            "deesser",
            "leveler",
        ],
    ),
    (
        "EQ & Filter",
        &[
            "eq",
            "equalizer",
            "equaliser",
            "equalization",
            "filter",
            "highpass",
            "lowpass",
            "bandpass",
            "parametric",
        ],
    ),
    (
        "Space",
        &[
            "reverb",
            "delay",
            "echo",
            "room",
            "hall",
            "plate",
            "convolution",
        ],
    ),
    (
        "Modulation",
        &[
            "chorus",
            "flanger",
            "phaser",
            "tremolo",
            "vibrato",
            "modulation",
            "modulator",
            "rotary",
            "pitch",
            "shifter",
            "harmonizer",
        ],
    ),
    (
        "Color",
        &[
            "distortion",
            "saturation",
            "saturator",
            "overdrive",
            "waveshaper",
            "fuzz",
            "bitcrusher",
            "crusher",
            "tape",
            "exciter",
        ],
    ),
    (
        "Stereo",
        &[
            "spatial", "imager", "widener", "panner", "panning", "surround",
        ],
    ),
    (
        "Utility",
        &[
            "utility",
            "utilities",
            "tools",
            "analyzer",
            "analyser",
            "analysis",
            "meter",
            "tuner",
            "restoration",
            "mastering",
        ],
    ),
];

/// The family a plugin's tags point to, the first tag that names one winning.
pub fn guess(tags: &[String]) -> &'static str {
    for tag in tags {
        let tag = tag.to_lowercase();
        let words: Vec<&str> = tag
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .collect();
        for (family, known) in WORDS {
            if known.iter().any(|k| words.contains(k) || tag == *k) {
                return family;
            }
        }
    }
    OTHER
}

fn file() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("audiostrada").join("plugin_categories.json"))
}

/// Families chosen by the user, by plugin key.
pub fn load() -> BTreeMap<String, String> {
    file()
        .and_then(|f| std::fs::read_to_string(f).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn save(chosen: &BTreeMap<String, String>) -> std::io::Result<()> {
    let Some(file) = file() else {
        return Ok(());
    };
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_string_pretty(chosen).map_err(std::io::Error::other)?;
    std::fs::write(file, json)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(t: &[&str]) -> Vec<String> {
        t.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn families_come_from_the_formats_tags() {
        assert_eq!(guess(&tags(&["Fx", "EQ"])), "EQ & Filter");
        assert_eq!(guess(&tags(&["audio-effect", "reverb", "stereo"])), "Space");
        assert_eq!(
            guess(&tags(&["audio-effect", "stereo", "compressor"])),
            "Dynamics"
        );
        assert_eq!(guess(&tags(&["Fx", "Spatial"])), "Stereo");
        assert_eq!(guess(&tags(&["De-esser"])), "Dynamics");
        assert_eq!(guess(&tags(&["Fx", "Pitch Shift"])), "Modulation");
        assert_eq!(guess(&tags(&["Distortion"])), "Color");
        assert_eq!(guess(&tags(&["Fx"])), OTHER);
        assert_eq!(guess(&[]), OTHER);
        // Every family is one of the built-in ones.
        for (family, _) in WORDS {
            assert!(audiostrada_dsp::CATEGORIES.contains(family), "{family}");
        }
    }
}
