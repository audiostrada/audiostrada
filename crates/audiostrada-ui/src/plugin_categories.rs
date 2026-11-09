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
        ],
    ),
];

/// Words of plugin names besides those of [`WORDS`], short as names have them.
const NAME_WORDS: &[(&str, &[&str])] = &[
    (
        "Dynamics",
        &["comp", "limit", "deess", "clip", "clipper", "leveller"],
    ),
    ("Space", &["verb", "delays"]),
    ("Color", &["sat", "drive", "dist", "crush", "lofi"]),
    ("Utility", &["vu", "scope", "spectrum", "loudness", "lufs"]),
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

/// The family of a plugin, from what is known of it in order: AudioStrada's own list, the
/// tags the plugin declares, PluginDB's table, the words of its name.
pub fn family(vendor: &str, name: &str, tags: &[String]) -> &'static str {
    let (extra, plugindb) = tables();
    extra
        .find(vendor, name)
        .or_else(|| Some(guess(tags)).filter(|f| *f != OTHER))
        .or_else(|| plugindb.find(vendor, name))
        .unwrap_or_else(|| from_name(name))
}

/// The family the words of a plugin's name point to: "RF-EQ1-A", "ValhallaVintageVerb".
fn from_name(name: &str) -> &'static str {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut last: Option<char> = None;
    for c in name.chars() {
        // A new word at a separator, between letters and digits, and at "aB".
        let split = !c.is_alphanumeric()
            || last.is_some_and(|l| {
                l.is_alphabetic() != c.is_alphabetic() || (l.is_lowercase() && c.is_uppercase())
            });
        if split && !word.is_empty() {
            words.push(std::mem::take(&mut word));
        }
        if c.is_alphanumeric() {
            word.extend(c.to_lowercase());
        }
        last = Some(c);
    }
    words.push(word);
    for (family, known) in WORDS.iter().chain(NAME_WORDS) {
        if words.iter().any(|w| known.contains(&w.as_str())) {
            return family;
        }
    }
    OTHER
}

/// Letters and digits in lower case: "Pro-Q 3" is "proq3".
fn norm(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// A name without its version nor channels, to know the next versions: "Pro-Q 4 (Mono)" is
/// "proq", as "Pro-Q 3".
fn stem(name: &str) -> String {
    let name = name.split('(').next().unwrap_or(name);
    let mut stem = norm(name);
    while stem.ends_with(|c: char| c.is_ascii_digit()) {
        stem.pop();
    }
    for suffix in ["mk", "v"] {
        if stem.len() > suffix.len() + 3
            && stem.ends_with(suffix)
            && name.trim_end().ends_with(|c: char| c.is_ascii_digit())
        {
            stem.truncate(stem.len() - suffix.len());
        }
    }
    stem
}

/// The same vendor, written alike: "Tokyo Dawn Labs" and "Tokyo Dawn Records", "u-he".
fn same_vendor(a: &str, b: &str) -> bool {
    let shorter = a.len().min(b.len());
    let common = a.bytes().zip(b.bytes()).take_while(|(x, y)| x == y).count();
    shorter > 0 && common >= shorter.min(4) && common as f32 >= 0.6 * shorter as f32
}

/// Plugins of a list by name and by stem, with their vendor and family.
#[derive(Default)]
struct Table {
    names: std::collections::HashMap<String, Vec<(String, &'static str)>>,
    stems: std::collections::HashMap<String, Vec<(String, &'static str)>>,
}

impl Table {
    /// Reads lines of `vendor<TAB>family<TAB>name|alias|...`.
    fn parse(text: &str) -> Self {
        let mut table = Table::default();
        for line in text
            .lines()
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
        {
            let mut fields = line.split('\t');
            let (Some(vendor), Some(family), Some(names)) =
                (fields.next(), fields.next(), fields.next())
            else {
                continue;
            };
            let Some(family) = audiostrada_dsp::CATEGORIES
                .iter()
                .copied()
                .find(|f| *f == family)
            else {
                continue;
            };
            let vendor = norm(vendor);
            for name in names.split('|') {
                for (map, key) in [
                    (&mut table.names, norm(name)),
                    (&mut table.stems, stem(name)),
                ] {
                    let short = key.strip_prefix(vendor.as_str()).map(str::to_owned);
                    for key in [Some(key), short].into_iter().flatten() {
                        let list = map.entry(key).or_default();
                        if !list.iter().any(|(v, f)| *v == vendor && *f == family) {
                            list.push((vendor.clone(), family));
                        }
                    }
                }
            }
        }
        table
    }

    /// The family of a plugin of this vendor by its name, then by its stem; none when the
    /// vendor's plugins of that name are of several families.
    fn find(&self, vendor: &str, name: &str) -> Option<&'static str> {
        let vendor = norm(vendor);
        let short = |key: String| {
            let stripped = key.strip_prefix(vendor.as_str()).map(str::to_owned);
            [Some(key), stripped]
        };
        let lookups = short(norm(name))
            .map(|k| (&self.names, k))
            .into_iter()
            .chain(short(stem(name)).map(|k| (&self.stems, k)));
        for (map, key) in lookups {
            let Some(key) = key.filter(|k| k.len() >= 3) else {
                continue;
            };
            let families: Vec<&'static str> = map
                .get(&key)
                .into_iter()
                .flatten()
                .filter(|(v, _)| same_vendor(v, &vendor))
                .map(|(_, f)| *f)
                .collect();
            if let Some(&first) = families.first() {
                return families.iter().all(|f| *f == first).then_some(first);
            }
        }
        None
    }
}

/// AudioStrada's own list, and PluginDB's table (see `scripts/update-plugindb.py`).
fn tables() -> &'static (Table, Table) {
    static TABLES: std::sync::OnceLock<(Table, Table)> = std::sync::OnceLock::new();
    TABLES.get_or_init(|| {
        (
            Table::parse(include_str!("../assets/plugin_families_extra.tsv")),
            Table::parse(include_str!("../assets/plugindb.tsv")),
        )
    })
}

fn file() -> Option<PathBuf> {
    crate::config_dir().map(|d| d.join("audiostrada").join("plugin_categories.json"))
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
        assert_eq!(guess(&tags(&["Fx", "Mastering"])), OTHER);
        assert_eq!(guess(&[]), OTHER);
        // Every family is one of the built-in ones.
        for (family, _) in WORDS {
            assert!(audiostrada_dsp::CATEGORIES.contains(family), "{family}");
        }
    }

    #[test]
    fn known_plugins_and_their_next_versions_find_their_family() {
        let fx = tags(&["Fx"]);
        for (vendor, name, expected) in [
            ("FabFilter", "Pro-Q 4", "EQ & Filter"),
            ("FabFilter", "Pro-Q 5", "EQ & Filter"),
            ("FabFilter", "Pro-Q 4 (Mono)", "EQ & Filter"),
            ("FabFilter", "FabFilter Pro-C 3", "Dynamics"),
            ("FabFilter", "Saturn 3", "Color"),
            ("Tokyo Dawn Labs", "TDR Nova", "EQ & Filter"),
            // An alias PluginDB gives to another vendor's plugin is not taken.
            ("Valhalla DSP, LLC", "ValhallaSupermassive", "Space"),
            ("Native Instruments", "Guitar Rig 7", "Amp & Cab"),
            ("TONE3000", "TONE3000", "Amp & Cab"),
            ("Riverflux Audio", "RF-EQ1-A", "EQ & Filter"),
            ("Riverflux Audio", "RF-16VU", "Utility"),
            ("Someone", "BigVerb", "Space"),
            ("Someone", "Thing", OTHER),
        ] {
            assert_eq!(family(vendor, name, &fx), expected, "{vendor} {name}");
        }
        // What the plugin declares wins over the table.
        assert_eq!(
            family("FabFilter", "Pro-Q 4", &tags(&["Fx", "Reverb"])),
            "Space"
        );
        assert_eq!(
            family("Tokyo Dawn Labs", "TDR Nova", &tags(&["Fx", "Mastering"])),
            "EQ & Filter"
        );
        assert_eq!(stem("Pro-Q 4 (Mono)"), "proq");
        assert_eq!(stem("Density MkIV"), "densitymkiv");
        assert_eq!(stem("Density Mk4"), "density");
        assert!(same_vendor("tokyodawnlabs", "tokyodawnrecords"));
        assert!(!same_vendor("uhe", "valhalladsp"));
    }
}
