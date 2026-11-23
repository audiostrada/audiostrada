//! Effect chains: a channel's inserts saved under a name (settings, plugin states, files) and
//! added to any channel.

use std::path::{Path, PathBuf};

use audiostrada_core::Insert;
use serde::{Deserialize, Serialize};

use crate::i18n::tr;

const FORMAT: &str = "audiostrada-chain";
const EXTENSION: &str = "stradachain";

/// A named list of inserts.
#[derive(Clone, Debug, PartialEq)]
pub struct Chain {
    pub name: String,
    pub inserts: Vec<Insert>,
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
}

#[derive(Serialize, Deserialize)]
struct File {
    format: String,
    version: u32,
    name: String,
    inserts: Vec<Insert>,
}

pub fn root() -> Option<PathBuf> {
    crate::config_dir().map(|d| d.join("audiostrada").join("chains"))
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

/// The user's chains, by name.
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
                    if i.effect == "amp_rig" {
                        audiostrada_core::legacy::upgrade_amp_rig(&mut i.params);
                    }
                    i
                })
                .collect(),
        })
        .collect();
    user.sort_by_key(|c| c.name.to_lowercase());
    user
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

#[cfg(test)]
mod tests {
    use super::*;
    use audiostrada_core::InsertId;

    fn insert(effect: &str, params: &[(&str, f32)]) -> Insert {
        Insert {
            id: InsertId(0),
            effect: effect.to_owned(),
            bypass: false,
            params: params.iter().map(|&(k, v)| (k.to_owned(), v)).collect(),
            state: None,
            files: Default::default(),
            sidechain: None,
            monitor: false,
            gain_db: 0.0,
            branches: Vec::new(),
            macros: Vec::new(),
            name: None,
        }
    }

    #[test]
    fn chains_save_list_and_remove() {
        let dir = audiostrada_core::scratch::dir("chains");
        let _ = std::fs::remove_dir_all(&dir);
        let mut amp = insert("amp_rig", &[("gain", 6.0)]);
        amp.files
            .insert("amp".into(), PathBuf::from("/captures/plexi.nam"));
        save(&dir, "My amp", &[amp.clone(), insert("reverb", &[])]).unwrap();
        let all = list(Some(&dir));
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].name, "My amp");
        assert_eq!(all[0].inserts[0], amp);
        remove(&dir, "My amp").unwrap();
        assert!(list(Some(&dir)).is_empty());
        assert!(save(&dir, " ", &[amp]).is_err());
    }
}
