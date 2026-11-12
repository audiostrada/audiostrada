//! AudioStrada project model, undo/redo and versioned serialization.

pub mod automation;
pub mod autosave;
pub mod base64;
pub mod console;
pub mod edit;
pub mod file;
pub mod groups;
pub mod history;
pub mod legacy;
pub mod model;
pub mod prepare;
pub mod routing;
pub mod scratch;
pub mod section;
pub mod snapshot;
pub mod tempo;

pub use file::{ProjectError, ProjectPaths};
pub use history::History;
pub use model::{
    Branch, ClickSound, Clip, ClipId, ControlPoint, FadeShape, Frozen, Insert, InsertId, Marker,
    MarkerId, Master, MetronomeSettings, MidiMapping, Monitoring, Note, Playlist, Prepared,
    Project, RecordSource, ReferenceTrack, Source, SourceId, TakeInfo, Track, TrackId, TrackKind,
    TrackSend, parse_variant, variant_path,
};
