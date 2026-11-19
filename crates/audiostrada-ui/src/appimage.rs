//! The AppImage installed for the user, as a package would: a copy among their programs
//! (`~/.local/bin/audiostrada`), an entry in the applications menu, the icon and the .strada
//! files. Updates replace that copy, then AudioStrada starts again from it.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::i18n::tr;

const ID: &str = "com.audiostrada.AudioStrada";
const DESKTOP: &str = include_str!("../../../packaging/linux/audiostrada.desktop");
const MIME: &str = include_str!("../../../packaging/linux/audiostrada.xml");
const ICON: &[u8] = include_bytes!("../../../packaging/icons/audiostrada-256.png");

/// Where this AppImage stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// Installed by AudioStrada, whichever copy runs.
    Installed,
    /// Another tool keeps it (AppImageLauncher, Gear Lever...): left where it is.
    Managed,
    /// Started from where it was downloaded.
    Loose,
}

/// The user's folders the installation writes in.
#[derive(Clone, Debug)]
struct Places {
    home: PathBuf,
    bin: PathBuf,
    data: PathBuf,
}

impl Places {
    fn user() -> Option<Self> {
        if cfg!(test) {
            let root = audiostrada_core::scratch::dir("appimage-home");
            return Some(Self {
                bin: root.join(".local/bin"),
                data: root.join(".local/share"),
                home: root,
            });
        }
        Some(Self {
            home: dirs::home_dir()?,
            bin: dirs::executable_dir()?,
            data: dirs::data_dir()?,
        })
    }

    fn program(&self) -> PathBuf {
        self.bin.join("audiostrada")
    }

    fn desktop(&self) -> PathBuf {
        self.data.join("applications").join(format!("{ID}.desktop"))
    }

    fn mime(&self) -> PathBuf {
        self.data.join("mime/packages/audiostrada.xml")
    }

    fn icon(&self) -> PathBuf {
        self.data.join("icons/hicolor/256x256/apps/audiostrada.png")
    }
}

/// The AppImage file this copy runs from.
pub fn running() -> Option<PathBuf> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    std::env::var_os("APPIMAGE").map(PathBuf::from)
}

static STATE: Mutex<Option<Option<State>>> = Mutex::new(None);

/// Where this AppImage stands; `None` when AudioStrada does not run from one.
pub fn state() -> Option<State> {
    let mut cached = STATE.lock().unwrap_or_else(|e| e.into_inner());
    *cached.get_or_insert_with(|| Some(state_of(&running()?, &Places::user()?)))
}

fn forget_state() {
    *STATE.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

fn state_of(appimage: &Path, places: &Places) -> State {
    let same = |a: &Path, b: &Path| match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    };
    if same(appimage, &places.program()) || places.program().exists() && places.desktop().exists() {
        return State::Installed;
    }
    let kept = ["Applications", "AppImages", ".local/share/AppImage"]
        .iter()
        .any(|dir| appimage.parent() == Some(places.home.join(dir).as_path()));
    let named = || {
        let text = appimage.to_string_lossy();
        std::fs::read_dir(places.data.join("applications"))
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path() != places.desktop())
            .any(|e| std::fs::read_to_string(e.path()).is_ok_and(|d| d.contains(&*text)))
    };
    if kept || named() {
        State::Managed
    } else {
        State::Loose
    }
}

/// Installs `appimage` for the user; the installed program's path.
pub fn install(appimage: &Path) -> Result<PathBuf, String> {
    let places = Places::user().ok_or_else(|| tr("No home folder").to_owned())?;
    let program = install_in(appimage, &places).map_err(|e| e.to_string())?;
    refresh_menus(&places);
    forget_state();
    Ok(program)
}

fn install_in(appimage: &Path, places: &Places) -> std::io::Result<PathBuf> {
    let program = places.program();
    replace(&program, appimage)?;
    let exec = format!("Exec=\"{}\" %F", program.display());
    let desktop = DESKTOP
        .lines()
        .map(|l| {
            if l.starts_with("Exec=") {
                exec.as_str()
            } else {
                l
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    for (path, bytes) in [
        (places.desktop(), desktop.as_bytes()),
        (places.mime(), MIME.as_bytes()),
        (places.icon(), ICON),
    ] {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, bytes)?;
    }
    Ok(program)
}

/// Takes out what `install` put in, the program included.
pub fn uninstall() -> Result<(), String> {
    let places = Places::user().ok_or_else(|| tr("No home folder").to_owned())?;
    uninstall_in(&places).map_err(|e| e.to_string())?;
    refresh_menus(&places);
    forget_state();
    Ok(())
}

fn uninstall_in(places: &Places) -> std::io::Result<()> {
    for path in [
        places.desktop(),
        places.mime(),
        places.icon(),
        places.program(),
    ] {
        match std::fs::remove_file(path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
    }
    Ok(())
}

/// Puts `new` in place of `target`, runnable; the running program can be replaced.
fn replace(target: &Path, new: &Path) -> std::io::Result<()> {
    let dir = target.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let part = dir.join(".audiostrada-update.part");
    std::fs::copy(new, &part)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&part, std::fs::Permissions::from_mode(0o755))?;
    }
    std::fs::rename(&part, target)
}

/// Brings the downloaded `new` version in: in place of the installed program, or of this
/// AppImage when another tool keeps it, else installed. The program to start again.
pub fn update(new: &Path) -> Result<PathBuf, String> {
    let current =
        running().ok_or_else(|| tr("AudioStrada does not run from an AppImage").to_owned())?;
    let target = match state() {
        Some(State::Installed) => Places::user().map(|p| p.program()),
        Some(State::Managed) => Some(current),
        _ => None,
    };
    match target {
        Some(target) => {
            replace(&target, new).map_err(|e| e.to_string())?;
            Ok(target)
        }
        None => install(new),
    }
}

/// The desktops read the new entry and the .strada type; nothing breaks without the tools.
fn refresh_menus(places: &Places) {
    if cfg!(test) {
        return;
    }
    for (tool, dir) in [
        ("update-desktop-database", places.data.join("applications")),
        ("update-mime-database", places.data.join("mime")),
    ] {
        let _ = std::process::Command::new(tool)
            .arg(dir)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
}

static RESTART: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Starts `program` once this window is closed.
pub fn restart_with(program: PathBuf) {
    *RESTART.lock().unwrap_or_else(|e| e.into_inner()) = Some(program);
}

/// The program to start once AudioStrada has closed, after an update.
pub fn take_restart() -> Option<PathBuf> {
    RESTART.lock().unwrap_or_else(|e| e.into_inner()).take()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn places(name: &str) -> Places {
        let root = audiostrada_core::scratch::dir(name);
        Places {
            bin: root.join(".local/bin"),
            data: root.join(".local/share"),
            home: root,
        }
    }

    fn appimage(places: &Places, at: &str) -> PathBuf {
        let path = places.home.join(at);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"version 1").unwrap();
        path
    }

    #[test]
    fn an_installed_appimage_has_its_program_menu_entry_and_file_type() {
        let places = places("appimage-install");
        let downloaded = appimage(&places, "Downloads/AudioStrada-1.0.0-x86_64.AppImage");
        assert_eq!(state_of(&downloaded, &places), State::Loose);
        let program = install_in(&downloaded, &places).unwrap();
        assert_eq!(std::fs::read(&program).unwrap(), b"version 1");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&program).unwrap().permissions().mode();
            assert_eq!(mode & 0o111, 0o111);
        }
        let desktop = std::fs::read_to_string(places.desktop()).unwrap();
        assert!(desktop.contains(&format!("Exec=\"{}\" %F", program.display())));
        assert!(desktop.contains("MimeType=application/x-audiostrada-project;"));
        assert!(places.mime().exists() && places.icon().exists());
        assert_eq!(state_of(&program, &places), State::Installed);
        assert_eq!(state_of(&downloaded, &places), State::Installed);

        uninstall_in(&places).unwrap();
        assert!(!program.exists() && !places.desktop().exists() && !places.icon().exists());
        assert!(downloaded.exists());
        assert_eq!(state_of(&downloaded, &places), State::Loose);
    }

    #[test]
    fn an_appimage_kept_by_another_tool_is_left_to_it() {
        let places = places("appimage-managed");
        let kept = appimage(&places, "Applications/AudioStrada.AppImage");
        assert_eq!(state_of(&kept, &places), State::Managed);
        let elsewhere = appimage(&places, "Music/AudioStrada.AppImage");
        assert_eq!(state_of(&elsewhere, &places), State::Loose);
        let entries = places.data.join("applications");
        std::fs::create_dir_all(&entries).unwrap();
        let entry = format!("[Desktop Entry]\nExec={} %F\n", elsewhere.display());
        std::fs::write(entries.join("appimagekit_1-AudioStrada.desktop"), entry).unwrap();
        assert_eq!(state_of(&elsewhere, &places), State::Managed);
    }

    #[test]
    fn an_update_replaces_the_program_in_place() {
        let places = places("appimage-replace");
        let program = install_in(&appimage(&places, "AudioStrada.AppImage"), &places).unwrap();
        let new = places.home.join("new.AppImage");
        std::fs::write(&new, b"version 2").unwrap();
        replace(&program, &new).unwrap();
        assert_eq!(std::fs::read(&program).unwrap(), b"version 2");
        assert!(!program.with_file_name(".audiostrada-update.part").exists());
    }
}
