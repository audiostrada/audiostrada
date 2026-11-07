//! Every user command, so menus, shortcuts and the command palette share one registry.

use serde::{Deserialize, Serialize};

macro_rules! actions {
    ($($id:ident => ($category:literal, $name:literal)),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        pub enum Action {
            $($id),+
        }

        impl Action {
            pub const ALL: &'static [Action] = &[$(Action::$id),+];

            pub fn name(self) -> &'static str {
                match self {
                    $(Action::$id => $name),+
                }
            }

            pub fn category(self) -> &'static str {
                match self {
                    $(Action::$id => $category),+
                }
            }

            /// The name shown in menus and the command palette, in the interface's language;
            /// `name` stays the English identifier (keymaps, agents).
            pub fn label(self) -> &'static str {
                crate::i18n::tr(self.name())
            }
        }
    };
}

actions! {
    NewProject => ("File", "New project"),
    OpenProject => ("File", "Open project…"),
    NewProjectTab => ("File", "New project tab"),
    OpenProjectInTab => ("File", "Open project in a new tab…"),
    CloseTab => ("File", "Close project tab"),
    NextTab => ("File", "Next project tab"),
    PreviousTab => ("File", "Previous project tab"),
    Save => ("File", "Save"),
    SaveAs => ("File", "Save as…"),
    ProjectVersions => ("File", "Project versions…"),
    CollectMedia => ("File", "Collect media into the project folder"),
    ArchiveProject => ("File", "Archive project…"),
    ImportAudio => ("File", "Import audio…"),
    Export => ("File", "Export…"),
    ImportMidi => ("File", "Import MIDI file…"),
    ExportMidi => ("File", "Export MIDI file…"),
    Quit => ("File", "Quit"),
    Undo => ("Edit", "Undo"),
    Redo => ("Edit", "Redo"),
    SelectAll => ("Edit", "Select all clips"),
    Deselect => ("Edit", "Deselect"),
    Delete => ("Edit", "Delete"),
    Copy => ("Edit", "Copy"),
    Cut => ("Edit", "Cut"),
    Paste => ("Edit", "Paste at playhead"),
    Split => ("Edit", "Split at playhead"),
    Duplicate => ("Edit", "Duplicate"),
    NudgeLeft => ("Edit", "Nudge left"),
    NudgeRight => ("Edit", "Nudge right"),
    ModeSlip => ("Edit", "Edit mode: Slip"),
    ModeShuffle => ("Edit", "Edit mode: Shuffle"),
    ModeGrid => ("Edit", "Edit mode: Grid"),
    Fades => ("Edit", "Fades on the range, or short ones on the selected clips"),
    NextTransient => ("Transport", "Go to the next transient"),
    PreviousTransient => ("Transport", "Go to the previous transient"),
    ToolSmart => ("Edit", "Smart tool"),
    ToolRange => ("Edit", "Range tool"),
    ToolSplit => ("Edit", "Scissors"),
    ToolErase => ("Edit", "Eraser"),
    ToolMute => ("Edit", "Mute tool"),
    ToggleSnap => ("Edit", "Toggle snap to grid"),
    GridFiner => ("Edit", "Finer grid"),
    GridCoarser => ("Edit", "Coarser grid"),
    PlayPause => ("Transport", "Play / pause"),
    Stop => ("Transport", "Stop"),
    Panic => ("Transport", "Panic: every note off"),
    Record => ("Transport", "Record"),
    GoToStart => ("Transport", "Go to start"),
    GoToEnd => ("Transport", "Go to end"),
    ToggleLoop => ("Transport", "Toggle loop"),
    KeyboardShortcuts => ("Help", "Keyboard shortcuts…"),
    About => ("Help", "About AudioStrada"),
    License => ("Help", "License…"),
    Assistants => ("Help", "AI assistants…"),
    ToggleMetronome => ("Transport", "Toggle metronome"),
    MetronomeSettings => ("Transport", "Metronome settings…"),
    ListenToReference => ("Transport", "Listen to the reference / the mix"),
    LowLatency => ("Transport", "Low latency mode"),
    RecordingMode => ("Transport", "Recording mode"),
    HeadphoneMixes => ("Transport", "Headphone mixes"),
    Talkback => ("Transport", "Talkback"),
    ReferenceTrack => ("Transport", "Reference track…"),
    TogglePunch => ("Transport", "Toggle punch range"),
    LoopSelection => ("Transport", "Loop selected clips"),
    AddMarker => ("Markers", "Add marker at playhead"),
    NextMarker => ("Markers", "Go to next marker"),
    PreviousMarker => ("Markers", "Go to previous marker"),
    ZoomIn => ("View", "Zoom in"),
    ZoomOut => ("View", "Zoom out"),
    ZoomToFit => ("View", "Zoom to fit session"),
    ZoomToSelection => ("View", "Zoom to the selection (again: back)"),
    TallerTracks => ("View", "Taller tracks"),
    ShorterTracks => ("View", "Shorter tracks"),
    Heal => ("Edit", "Heal separation"),
    Consolidate => ("Edit", "Consolidate clips"),
    StripSilence => ("Edit", "Strip silence…"),
    ProcessClips => ("Edit", "Process clips…"),
    BeatDetective => ("Edit", "Beat Detective…"),
    Groove => ("Edit", "Groove…"),
    RangeFromClips => ("Edit", "Range over the selected clips"),
    RangeFromLoop => ("Edit", "Range from the loop"),
    RangeAllTracks => ("Edit", "Range on every track"),
    CropToSelection => ("Edit", "Crop project to selection"),
    RemoveSection => ("Edit", "Remove section (close the gap)"),
    ClearSection => ("Edit", "Clear section (keep the gap)"),
    WaveformZoomIn => ("View", "Waveform zoom in"),
    WaveformZoomOut => ("View", "Waveform zoom out"),
    WaveformZoomReset => ("View", "Waveform zoom reset"),
    ShowTimeline => ("View", "Show timeline"),
    ShowMixer => ("View", "Show mixer"),
    ShowBrowser => ("View", "Show browser"),
    ShowInspector => ("View", "Show inspector"),
    ShowMarkers => ("View", "Show markers"),
    ShowPianoRoll => ("View", "Show piano roll"),
    ShowRack => ("View", "Show rack"),
    RevealRack => ("Track", "Effects rack of the selected track"),
    ResetLayout => ("View", "Reset layout"),
    ToggleFullscreen => ("View", "Full screen"),
    DetachMixer => ("View", "Mixer in its own window"),
    CompactMixer => ("View", "Compact mixer"),
    TransportAtBottom => ("View", "Transport at the bottom"),
    LargeTransport => ("View", "Large transport"),
    ShowInputs => ("View", "Armed inputs"),
    ShowTakes => ("View", "Takes"),
    DetachPianoRoll => ("View", "Piano roll in its own window"),
    CommandPalette => ("View", "Command palette…"),
    Preferences => ("View", "Preferences…"),
    AddTrack => ("Track", "Add audio track"),
    AddStereoTrack => ("Track", "Add stereo track"),
    AddTracks => ("Track", "Add audio tracks…"),
    AddInstrumentTrack => ("Track", "Add instrument track"),
    AddDrumTrack => ("Track", "Add drum machine track"),
    NewMidiClip => ("Track", "New MIDI clip"),
    Quantize => ("Edit", "Quantize notes"),
    AddVca => ("Track", "Add VCA"),
    MoveTrackUp => ("Track", "Move track up"),
    MoveTrackDown => ("Track", "Move track down"),
    DuplicateTracks => ("Track", "Duplicate selected tracks"),
    DeleteTracks => ("Track", "Delete selected tracks"),
    SelectPreviousTrack => ("Track", "Select previous track"),
    SelectNextTrack => ("Track", "Select next track"),
    MuteTracks => ("Track", "Mute selected tracks"),
    SoloTracks => ("Track", "Solo selected tracks"),
    ArmTracks => ("Track", "Arm selected tracks"),
    GroupTracks => ("Track", "Group selected tracks"),
    ToggleGroups => ("Track", "Groups on or off"),
    NewPlaylist => ("Track", "New playlist on selected track"),
    NextPlaylist => ("Track", "Next playlist on selected track"),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_unique() {
        let mut names: Vec<&str> = Action::ALL.iter().map(|a| a.name()).collect();
        names.sort_unstable();
        let count = names.len();
        names.dedup();
        assert_eq!(names.len(), count);
    }
}
