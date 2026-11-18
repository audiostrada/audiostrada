//! Where AudioStrada keeps its settings, caches, recoveries and log: the user's folders, or the
//! folders of a profile given at start (`--profile`, `--fresh`), the user's own left untouched.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static ROOT: OnceLock<PathBuf> = OnceLock::new();

/// Uses `root` as the profile for the rest of the process; false when one was set already.
pub fn set(root: PathBuf) -> bool {
    ROOT.set(root).is_ok()
}

/// The profile folder, when one was given.
pub fn root() -> Option<&'static Path> {
    ROOT.get().map(PathBuf::as_path)
}

fn under(root: Option<&Path>, kind: &str, user: Option<PathBuf>) -> Option<PathBuf> {
    match root {
        Some(root) => Some(root.join(kind)),
        None => user,
    }
}

pub fn config_dir() -> Option<PathBuf> {
    under(root(), "config", dirs::config_dir())
}

pub fn data_dir() -> Option<PathBuf> {
    under(root(), "data", dirs::data_dir())
}

pub fn cache_dir() -> Option<PathBuf> {
    under(root(), "cache", dirs::cache_dir())
}

/// The log's folder: the state folder where the system has one, else the cache.
pub fn state_dir() -> Option<PathBuf> {
    under(root(), "state", dirs::state_dir().or_else(dirs::cache_dir))
}

/// The user's configuration folder whatever the profile: what other programs read there (the
/// control channel's token) stays where they look.
pub fn user_config_dir() -> Option<PathBuf> {
    dirs::config_dir()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_profile_takes_every_folder_in() {
        let root = Path::new("/tmp/p");
        let user = Some(PathBuf::from("/home/u/.config"));
        assert_eq!(
            under(Some(root), "config", user.clone()),
            Some(root.join("config"))
        );
        assert_eq!(under(None, "config", user.clone()), user);
        assert_eq!(under(None, "cache", None), None);
    }
}
