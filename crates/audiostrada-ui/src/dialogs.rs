//! Native file dialogs. They run on a thread of their own: waiting for them on the interface
//! thread froze the window, and the system offered to kill AudioStrada.

use std::fmt::Debug;
use std::hash::Hash;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::sync::{Arc, Mutex};

use eframe::egui;

use audiostrada_core::ProjectPaths;
use audiostrada_core::file::EXTENSION;

use crate::i18n::tr;

pub(crate) const AUDIO_EXTENSIONS: [&str; 19] = [
    "wav", "wave", "aif", "aiff", "aifc", "flac", "mp3", "mp2", "ogg", "oga", "caf", "m4a", "m4b",
    "aac", "mp4", "mov", "mka", "mkv", "webm",
];

type Slot<T> = Arc<Mutex<Receiver<Option<T>>>>;

/// File dialogs open now.
static OPEN: AtomicUsize = AtomicUsize::new(0);

/// Whether a file dialog is open: floating windows then stay at the normal level, under it.
pub fn any_open() -> bool {
    OPEN.load(Ordering::Relaxed) > 0
}

fn key(id: impl Hash + Debug) -> egui::Id {
    egui::Id::unique(("file-dialog", id))
}

/// Opens `dialog` (a blocking rfd call) without blocking the interface, unless one with the
/// same `id` is already open.
pub fn start<T: Send + 'static>(
    ctx: &egui::Context,
    id: impl Hash + Debug,
    dialog: impl FnOnce() -> Option<T> + Send + 'static,
) {
    let key = key(id);
    // An answer nobody took (its window was closed) is dropped.
    let open = ctx
        .data(|d| d.get_temp::<Slot<T>>(key))
        .is_some_and(|slot| {
            slot.lock()
                .is_ok_and(|rx| matches!(rx.try_recv(), Err(TryRecvError::Empty)))
        });
    if open {
        return;
    }
    let (tx, rx) = channel();
    ctx.data_mut(|d| d.insert_temp::<Slot<T>>(key, Arc::new(Mutex::new(rx))));
    // Tests have no screen: every dialog is cancelled.
    if cfg!(test) {
        let _ = tx.send(None);
        return;
    }
    // macOS shows its panels from the main thread only, and keeps the window alive meanwhile.
    if cfg!(target_os = "macos") {
        let _ = tx.send(dialog());
        return;
    }
    OPEN.fetch_add(1, Ordering::Relaxed);
    for id in [
        egui::ViewportId::ROOT,
        ctx.viewport_id(),
        ctx.parent_viewport_id(),
    ] {
        ctx.request_repaint_of(id);
    }
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        // The floating windows go down first: GNOME opens a new window under an "always on
        // top" one of the application.
        std::thread::sleep(std::time::Duration::from_millis(200));
        let chosen = dialog();
        OPEN.fetch_sub(1, Ordering::Relaxed);
        let _ = tx.send(chosen);
        ctx.request_repaint_of(egui::ViewportId::ROOT);
        ctx.request_repaint();
    });
}

/// The answer of the dialog `id` once closed: `Some(None)` when cancelled.
pub fn answer<T: Send + 'static>(ctx: &egui::Context, id: impl Hash + Debug) -> Option<Option<T>> {
    let key = key(id);
    let slot = ctx.data(|d| d.get_temp::<Slot<T>>(key))?;
    let answer = slot.lock().ok()?.try_recv().ok()?;
    ctx.data_mut(|d| d.remove::<Slot<T>>(key));
    Some(answer)
}

/// For widgets drawn every frame: opens the dialog when `open`, and gives what was chosen.
pub fn ask<T: Send + 'static>(
    ctx: &egui::Context,
    id: impl Hash + Debug + Copy,
    open: bool,
    dialog: impl FnOnce() -> Option<T> + Send + 'static,
) -> Option<T> {
    if open {
        start(ctx, id, dialog);
    }
    answer(ctx, id).flatten()
}

pub fn pick_audio_file() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title(tr("Locate audio file"))
        .add_filter(tr("Audio"), &AUDIO_EXTENSIONS)
        .pick_file()
}

pub fn pick_audio_files() -> Option<Vec<PathBuf>> {
    rfd::FileDialog::new()
        .set_title(tr("Import audio"))
        .add_filter(tr("Audio"), &AUDIO_EXTENSIONS)
        .pick_files()
}

pub fn pick_project() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title(tr("Open project"))
        .add_filter(tr("AudioStrada project"), &[EXTENSION])
        .pick_file()
}

pub fn save_project(name: &str) -> Option<ProjectPaths> {
    rfd::FileDialog::new()
        .set_title(tr("Save project"))
        .add_filter(tr("AudioStrada project"), &[EXTENSION])
        .set_file_name(format!("{name}.{EXTENSION}"))
        .set_can_create_directories(true)
        .save_file()
        .map(|chosen| project_location(&chosen))
}

/// Where a project chosen as `chosen` is stored: in a folder of its own name, unless the user
/// already picked a location inside such a folder.
pub fn project_location(chosen: &Path) -> ProjectPaths {
    let file = chosen.with_extension(EXTENSION);
    let stem = file.file_stem().unwrap_or_default();
    let parent = file.parent().unwrap_or(Path::new("."));
    if parent.file_name() == Some(stem) {
        ProjectPaths::new(file)
    } else {
        ProjectPaths::in_new_folder(parent, &stem.to_string_lossy())
    }
}

pub fn pick_midi_file() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title(tr("Import MIDI file"))
        .add_filter(tr("MIDI file"), &["mid", "midi"])
        .pick_file()
}

pub fn save_midi_file(name: &str) -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title(tr("Export MIDI file"))
        .set_file_name(format!("{name}.mid"))
        .add_filter(tr("MIDI file"), &["mid"])
        .save_file()
}

pub fn save_archive(name: &str) -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title(tr("Archive project"))
        .set_file_name(format!("{name}.tar.gz"))
        .add_filter(tr("Project archive"), &["gz"])
        .save_file()
}

pub fn save_audio_file(name: &str, extension: &str) -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title(tr("Export track"))
        .set_file_name(format!("{name}.{extension}"))
        .add_filter(tr("Audio file"), &[extension])
        .save_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projects_get_their_own_folder() {
        assert_eq!(
            project_location(Path::new("/music/Song.strada")).file,
            PathBuf::from("/music/Song/Song.strada")
        );
        assert_eq!(
            project_location(Path::new("/music/Song")).file,
            PathBuf::from("/music/Song/Song.strada")
        );
        assert_eq!(
            project_location(Path::new("/music/Song/Song.strada")).file,
            PathBuf::from("/music/Song/Song.strada")
        );
    }
}
