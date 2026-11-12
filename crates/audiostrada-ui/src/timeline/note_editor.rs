//! The note editor: the piano roll in a large window over the timeline, laid out as the
//! automation editor (the same bar, keys, zoom and help), for the selected MIDI clip. Only one
//! of the two editors is open at a time.

use super::*;
use crate::editor_frame;
use crate::piano_roll::{EDITOR_TOOLS, PianoRoll};

#[derive(Debug)]
pub(super) struct NoteEditor {
    /// Kept while closed: its zoom, tool and grid come back with it.
    pub(super) piano: PianoRoll,
    pub(super) open: bool,
}

impl NoteEditor {
    pub(super) fn new(doc: Rc<RefCell<Document>>) -> Self {
        Self {
            piano: PianoRoll::editor(doc),
            open: false,
        }
    }
}

impl TimelineView {
    /// Opens the note editor on `clip`, selecting it; the automation editor closes.
    pub(super) fn open_note_editor(&mut self, clip: ClipId) {
        self.selection = vec![clip];
        self.notes.open = true;
        self.auto_editor = None;
    }

    /// Opens the note editor on the selected MIDI clip, or closes it.
    pub(super) fn toggle_note_editor(&mut self) {
        self.notes.open = !self.notes.open;
        if self.notes.open {
            self.auto_editor = None;
        } else {
            self.notes.piano.release_preview();
        }
    }

    /// The piano roll of the note editor, while it is open and has the keyboard.
    pub fn note_piano(&mut self) -> Option<&mut PianoRoll> {
        (self.notes.open && self.notes.piano.focused).then_some(&mut self.notes.piano)
    }

    /// The note editor's piano roll, open or not: a press elsewhere, a hidden frame.
    pub fn note_piano_any(&mut self) -> &mut PianoRoll {
        &mut self.notes.piano
    }

    /// Whether the note editor is open.
    pub fn note_editor_open(&self) -> bool {
        self.notes.open
    }

    /// The keys of the note editor's tools, while it is open.
    pub(super) fn take_note_keys(&mut self, ctx: &egui::Context) {
        if !self.notes.open {
            return;
        }
        editor_frame::keep_escape(ctx, editor_frame::NOTES);
        if let Some(tool) = editor_frame::take_tool_key(ctx, &EDITOR_TOOLS) {
            self.notes.piano.set_tool(tool);
        }
        if editor_frame::take_view_keys(ctx, editor_frame::NOTES) {
            self.notes.piano.frame_notes();
        }
    }

    /// The note editor's window, while it is open. Drawn once the timeline has let go of the
    /// document: the piano roll borrows it itself.
    pub(super) fn note_window(&mut self, ctx: &egui::Context, settings: &EditSettings) {
        if !self.notes.open {
            return;
        }
        let clips = self.midi_clips();
        // Named after what it edits: "Notes · Keys: Verse", or the tracks: "Notes · Keys, Bass".
        let mut title = tr("Notes").to_owned();
        {
            let doc = self.doc.borrow();
            let project = doc.project();
            let mut tracks: Vec<&str> = Vec::new();
            for &c in &clips {
                if let Some(t) = edit::track_of_clip(project, c).and_then(|t| project.track(t))
                    && !tracks.contains(&t.name.as_str())
                {
                    tracks.push(&t.name);
                }
            }
            match (clips.as_slice(), tracks.as_slice()) {
                ([one], [track]) => {
                    if let Some(c) = edit::clip(project, *one) {
                        title = format!("{title} · {track}: {}", c.name);
                    }
                }
                (_, []) => {}
                (_, tracks) => title = format!("{title} · {}", tracks.join(", ")),
            }
        }
        // Escape first cancels a transform waiting; not in the middle of a drag.
        let escape = editor_frame::take_escape(ctx, editor_frame::NOTES)
            && !self.notes.piano.cancel_transform()
            && self.notes.piano.lets_escape();
        let piano = &mut self.notes.piano;
        let open = editor_frame::window(ctx, editor_frame::NOTES, title, escape, |ui| {
            piano.show(ui, settings, &clips);
        });
        // The list asked for other clips: the timeline's selection follows.
        if let Some(clips) = self.notes.piano.take_clip_request() {
            self.selection = clips;
        }
        if !open {
            self.notes.open = false;
            self.notes.piano.release_preview();
        }
    }
}
