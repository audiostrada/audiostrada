//! Effect chains: a channel's inserts saved under a name (settings, plugin states, files) and
//! added to any channel, plus a few factory chains of built-in effects.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use audiostrada_core::{Insert, InsertId};
use serde::{Deserialize, Serialize};

use crate::i18n::tr;

const FORMAT: &str = "audiostrada-chain";
const EXTENSION: &str = "stradachain";

/// A named list of inserts; `factory` ones come with AudioStrada and cannot be deleted.
#[derive(Clone, Debug, PartialEq)]
pub struct Chain {
    pub name: String,
    pub inserts: Vec<Insert>,
    pub factory: bool,
}

impl Chain {
    /// "EQ → Compressor → Plate Reverb", for lists.
    pub fn summary(&self, name_of: impl Fn(&str) -> String) -> String {
        self.inserts
            .iter()
            .map(|i| name_of(&i.effect))
            .collect::<Vec<_>>()
            .join(" → ")
    }

    /// The name shown: a factory chain's in the chosen language, `name` staying its key.
    pub fn label(&self) -> String {
        if !self.factory {
            return self.name.clone();
        }
        match self.name.as_str() {
            "Vocal" => tr("Vocal"),
            "Drum bus" => tr("Drum bus"),
            "Acoustic guitar" => tr("Acoustic guitar"),
            "Master for streaming" => tr("Master for streaming"),
            other => other,
        }
        .to_owned()
    }
}

#[derive(Serialize, Deserialize)]
struct File {
    format: String,
    version: u32,
    name: String,
    inserts: Vec<Insert>,
}

pub fn root() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("audiostrada").join("chains"))
}

fn path(root: &Path, name: &str) -> PathBuf {
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
    root.join(format!("{}.{EXTENSION}", clean.trim()))
}

/// Factory chains first, then the user's, by name.
pub fn list(root: Option<&Path>) -> Vec<Chain> {
    let mut user: Vec<Chain> = root
        .and_then(|r| std::fs::read_dir(r).ok())
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == EXTENSION))
        .filter_map(|e| std::fs::read(e.path()).ok())
        .filter_map(|bytes| serde_json::from_slice::<File>(&bytes).ok())
        .filter(|f| f.format == FORMAT)
        .map(|f| Chain {
            name: f.name,
            inserts: f
                .inserts
                .into_iter()
                .map(|mut i| {
                    if i.effect == "eq" {
                        audiostrada_core::legacy::upgrade_eq(&mut i.params);
                    }
                    i
                })
                .collect(),
            factory: false,
        })
        .collect();
    user.sort_by_key(|c| c.name.to_lowercase());
    factory().into_iter().chain(user).collect()
}

/// Saves `inserts` as the chain `name`, replacing one of that name. Their files must be
/// absolute paths, so that the chain works in any project.
pub fn save(root: &Path, name: &str, inserts: &[Insert]) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(tr("give the chain a name").into());
    }
    if inserts.is_empty() {
        return Err(tr("there are no effects to save").into());
    }
    let file = File {
        format: FORMAT.to_owned(),
        version: 1,
        name: name.to_owned(),
        inserts: inserts.to_vec(),
    };
    let bytes = serde_json::to_vec_pretty(&file).map_err(|e| e.to_string())?;
    audiostrada_core::file::write_atomic(&path(root, name), &bytes).map_err(|e| e.to_string())
}

pub fn remove(root: &Path, name: &str) -> Result<(), String> {
    std::fs::remove_file(path(root, name)).map_err(|e| e.to_string())
}

fn insert(effect: &str, params: &[(&str, f32)]) -> Insert {
    Insert {
        id: InsertId(0),
        effect: effect.to_owned(),
        bypass: false,
        params: params
            .iter()
            .map(|&(k, v)| (k.to_owned(), v))
            .collect::<BTreeMap<_, _>>(),
        state: None,
        files: BTreeMap::new(),
        sidechain: None,
        monitor: false,
        gain_db: 0.0,
    }
}

fn factory() -> Vec<Chain> {
    let chain = |name: &str, inserts: Vec<Insert>| Chain {
        name: name.to_owned(),
        inserts,
        factory: true,
    };
    vec![
        chain(
            "Vocal",
            vec![
                insert(
                    "eq",
                    &[
                        ("b1_used", 1.0),
                        ("b1_type", 3.0),
                        ("b1_freq", 90.0),
                        ("b2_used", 1.0),
                        ("b2_type", 2.0),
                        ("b2_freq", 8000.0),
                        ("b2_gain", 2.0),
                    ],
                ),
                insert(
                    "compressor",
                    &[("threshold", -20.0), ("ratio", 3.0), ("attack", 8.0)],
                ),
                insert("deesser", &[]),
                insert("reverb", &[("type", 3.0), ("mix", 15.0)]),
            ],
        ),
        chain(
            "Drum bus",
            vec![
                insert(
                    "compressor",
                    &[
                        ("type", 1.0),
                        ("ratio", 4.0),
                        ("attack", 30.0),
                        ("release", 300.0),
                        ("makeup", 3.0),
                        ("sc_hpf", 60.0),
                        ("mix", 60.0),
                    ],
                ),
                insert("tape", &[("drive", 2.0), ("wow", 0.0)]),
            ],
        ),
        chain(
            "Acoustic guitar",
            vec![
                insert(
                    "eq",
                    &[
                        ("b1_used", 1.0),
                        ("b1_type", 3.0),
                        ("b1_freq", 100.0),
                        ("b2_used", 1.0),
                        ("b2_freq", 250.0),
                        ("b2_gain", -2.0),
                    ],
                ),
                insert("compressor", &[("threshold", -22.0), ("ratio", 2.5)]),
            ],
        ),
        chain(
            "Master for streaming",
            vec![
                insert(
                    "eq",
                    &[("b1_used", 1.0), ("b1_type", 3.0), ("b1_freq", 30.0)],
                ),
                insert(
                    "compressor",
                    &[
                        ("type", 1.0),
                        ("ratio", 2.0),
                        ("threshold", -10.0),
                        ("release", 300.0),
                        ("makeup", 3.0),
                        ("sc_hpf", 60.0),
                    ],
                ),
                insert("maximizer", &[("gain", 4.0), ("ceiling", -1.0)]),
            ],
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chains_save_list_and_remove() {
        let dir = std::env::temp_dir().join(format!("audiostrada-chains-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut amp = insert("amp_rig", &[("gain", 6.0)]);
        amp.files
            .insert("amp".into(), PathBuf::from("/captures/plexi.nam"));
        save(&dir, "My amp", &[amp.clone(), insert("reverb", &[])]).unwrap();
        let all = list(Some(&dir));
        let mine = all.iter().find(|c| !c.factory).unwrap();
        assert_eq!(mine.name, "My amp");
        assert_eq!(mine.inserts[0], amp);
        assert!(all.iter().any(|c| c.factory && c.name == "Vocal"));
        remove(&dir, "My amp").unwrap();
        assert!(list(Some(&dir)).iter().all(|c| c.factory));
        assert!(save(&dir, " ", &[amp]).is_err());
    }

    #[test]
    fn factory_chains_use_known_parameters() {
        for chain in factory() {
            for insert in &chain.inserts {
                let info = audiostrada_dsp::info(&insert.effect).unwrap();
                for key in insert.params.keys() {
                    assert!(
                        info.params.iter().any(|p| p.id == key),
                        "{}: {} has no {key}",
                        chain.name,
                        insert.effect
                    );
                }
            }
        }
    }
}
