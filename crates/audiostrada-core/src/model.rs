//! Project data: tracks, clips, media sources and tempo.
//!
//! Timeline positions are in samples at the project sample rate. Tracks are shared (`Arc`) so
//! that undo snapshots only copy what an edit touches.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::automation::{AutomationLane, AutomationMode, AutomationTarget};
use crate::tempo::TempoMap;

macro_rules! id_type {
    ($name:ident) => {
        #[derive(
            Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(pub u64);
    };
}

id_type!(TrackId);
id_type!(ClipId);
id_type!(SourceId);
id_type!(MarkerId);
id_type!(InsertId);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum FadeShape {
    /// Constant power: no level dip in crossfades between uncorrelated material.
    #[default]
    EqualPower,
    Linear,
    /// Rises quickly, then levels off.
    Fast,
    /// Starts slowly (exponential feel).
    Slow,
    SCurve,
}

impl FadeShape {
    pub const ALL: [FadeShape; 5] = [
        FadeShape::EqualPower,
        FadeShape::Linear,
        FadeShape::Fast,
        FadeShape::Slow,
        FadeShape::SCurve,
    ];

    pub fn name(self) -> &'static str {
        match self {
            FadeShape::EqualPower => "Equal power",
            FadeShape::Linear => "Linear",
            FadeShape::Fast => "Fast",
            FadeShape::Slow => "Slow",
            FadeShape::SCurve => "S-curve",
        }
    }

    /// Gain for a fade-in at position `x` in 0..=1.
    pub fn gain(self, x: f32) -> f32 {
        let x = x.clamp(0.0, 1.0);
        match self {
            FadeShape::EqualPower => (x * std::f32::consts::FRAC_PI_2).sin(),
            FadeShape::Linear => x,
            FadeShape::Fast => 1.0 - (1.0 - x).powi(3),
            FadeShape::Slow => x.powi(3),
            FadeShape::SCurve => x * x * (3.0 - 2.0 * x),
        }
    }
}

/// When a track hears its hardware input.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Monitoring {
    /// While armed, as the preferences say: always, or only while stopped or recording.
    #[default]
    Auto,
    /// Always, armed or not (playing through an amp, for instance).
    Input,
    /// Never: the input is heard elsewhere (the interface's direct monitoring).
    Off,
}

impl Monitoring {
    pub fn is_auto(&self) -> bool {
        *self == Monitoring::Auto
    }
}

/// What a track records: its input as it comes, as its effects make it heard, or both. What
/// the effects make goes on the track's printed twin.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecordSource {
    #[default]
    Raw,
    Processed,
    Both,
}

impl RecordSource {
    pub fn is_raw(&self) -> bool {
        *self == RecordSource::Raw
    }

    pub fn raw(self) -> bool {
        self != RecordSource::Processed
    }

    pub fn processed(self) -> bool {
        self != RecordSource::Raw
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Marker {
    pub id: MarkerId,
    pub position: u64,
    pub name: String,
}

/// An alternate set of clips for a track (takes, versions).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Playlist {
    pub name: String,
    pub clips: Vec<Clip>,
}

/// An audio file used by the project.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Source {
    /// Relative to the project folder when inside it, absolute otherwise.
    pub path: PathBuf,
    pub channels: u16,
    pub sample_rate: u32,
    pub frames: u64,
}

impl Source {
    /// Length in samples at `rate`.
    pub fn frames_at(&self, rate: u32) -> u64 {
        if self.sample_rate == rate || self.sample_rate == 0 {
            self.frames
        } else {
            (self.frames as f64 * f64::from(rate) / f64::from(self.sample_rate)).round() as u64
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    pub id: ClipId,
    pub source: SourceId,
    pub name: String,
    /// Timeline position of the first sample.
    pub start: u64,
    /// First sample of the source played (in project-rate samples).
    pub offset: u64,
    pub length: u64,
    pub gain_db: f32,
    #[serde(default)]
    pub fade_in: u64,
    #[serde(default)]
    pub fade_out: u64,
    /// Shape of the fade-in (and of the fade-out unless `fade_out_shape` is set).
    #[serde(default)]
    pub fade_shape: FadeShape,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fade_out_shape: Option<FadeShape>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub muted: bool,
    /// Time stretch of the source: 2.0 plays it twice as long at the same pitch. `offset` and
    /// `length` count stretched samples.
    #[serde(default = "unit", skip_serializing_if = "is_unit")]
    pub stretch: f64,
    /// Transposition in semitones.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub pitch: f32,
    /// Controller changes of a MIDI clip, timed like its notes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub controls: Option<Arc<Vec<ControlPoint>>>,
    /// Notes of a MIDI clip (then `source` is unused); times are in project samples from the
    /// start of the content, so `offset` and `length` window them like audio.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<Arc<Vec<Note>>>,
    /// The drum sequencer's variation of each bar of a MIDI clip: 0 for A, 1 for B, 2 for
    /// the fill, 255 for a bar left alone.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variations: Vec<u8>,
    /// What is known of a recorded take.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub take: Option<Box<TakeInfo>>,
    /// Effects of the clip alone, between it and its track.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inserts: Vec<Insert>,
    /// Automation of those effects, timed from the start of the content (as the notes): it
    /// follows the clip when moved, and its audio when trimmed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub automation: Vec<AutomationLane>,
}

/// A recorded take: its number on its track, the section it was played in, how it was rated
/// and what went wrong while recording it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TakeInfo {
    pub number: u32,
    /// The marker where it starts, such as "Chorus".
    pub section: Option<String>,
    /// Stars, 0 (not rated) to 3.
    pub rating: u8,
    /// Samples at full scale or over.
    pub clipped: u64,
    /// Where the audio dropped out, in project samples from the start of its file.
    pub dropouts: Vec<u64>,
}

fn unit() -> f64 {
    1.0
}

fn is_unit(v: &f64) -> bool {
    (*v - 1.0).abs() < 1e-12
}

pub(crate) fn is_zero(v: &f32) -> bool {
    *v == 0.0
}

/// Media path standing for `path` stretched and transposed (decoded and rendered by the
/// media pool, never a file on disk).
pub fn variant_path(path: &std::path::Path, (stretch, pitch): (f64, f32)) -> std::path::PathBuf {
    format!(
        "{}#audiostrada-stretch={stretch:.6},{pitch:.3}",
        path.display()
    )
    .into()
}

/// The file and the alteration a media path stands for.
pub fn parse_variant(path: &std::path::Path) -> (std::path::PathBuf, Option<(f64, f32)>) {
    let text = path.to_string_lossy();
    if let Some((file, params)) = text.rsplit_once("#audiostrada-stretch=")
        && let Some((s, p)) = params.split_once(',')
        && let (Ok(s), Ok(p)) = (s.parse(), p.parse())
    {
        return (file.into(), Some((s, p)));
    }
    (path.to_path_buf(), None)
}

/// A controller value in a MIDI clip.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ControlPoint {
    /// Project samples from the start of the clip's content.
    pub time: u64,
    /// 0-127: control change; [`PITCH_BEND`]; [`CHANNEL_PRESSURE`].
    pub controller: u16,
    /// 0 to 1 (pitch bend: 0.5 is the center).
    pub value: f32,
    #[serde(default)]
    pub channel: u8,
}

pub const PITCH_BEND: u16 = 128;
pub const CHANNEL_PRESSURE: u16 = 129;

impl ControlPoint {
    /// The MIDI message for this value.
    pub fn message(&self) -> [u8; 3] {
        let ch = self.channel & 15;
        let v = self.value.clamp(0.0, 1.0);
        match self.controller {
            PITCH_BEND => {
                let bend = (v * 16383.0).round() as u16;
                [0xE0 | ch, (bend & 0x7F) as u8, (bend >> 7) as u8]
            }
            CHANNEL_PRESSURE => [0xD0 | ch, (v * 127.0).round() as u8, 0],
            cc => [0xB0 | ch, (cc & 0x7F) as u8, (v * 127.0).round() as u8],
        }
    }

    /// The point a MIDI message stands for, if it is a controller message.
    pub fn from_message(time: u64, data: [u8; 3]) -> Option<Self> {
        let channel = data[0] & 15;
        let (controller, value) = match data[0] & 0xF0 {
            0xB0 => (u16::from(data[1] & 0x7F), f32::from(data[2] & 0x7F) / 127.0),
            0xE0 => {
                let bend = u16::from(data[2] & 0x7F) << 7 | u16::from(data[1] & 0x7F);
                (PITCH_BEND, f32::from(bend) / 16383.0)
            }
            0xD0 => (CHANNEL_PRESSURE, f32::from(data[1] & 0x7F) / 127.0),
            _ => return None,
        };
        Some(Self {
            time,
            controller,
            value,
            channel,
        })
    }
}

/// A MIDI note.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    pub start: u64,
    pub length: u64,
    pub key: u8,
    pub velocity: u8,
    #[serde(default)]
    pub channel: u8,
    /// Chance of playing at each pass, in percent.
    #[serde(default = "always", skip_serializing_if = "is_always")]
    pub probability: u8,
    /// Kept but silent.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub muted: bool,
}

fn always() -> u8 {
    100
}

fn is_always(p: &u8) -> bool {
    *p >= 100
}

impl Default for Note {
    fn default() -> Self {
        Self {
            start: 0,
            length: 0,
            key: 60,
            velocity: 100,
            channel: 0,
            probability: 100,
            muted: false,
        }
    }
}

impl Clip {
    pub fn end(&self) -> u64 {
        self.start + self.length
    }

    /// Stretch and transposition when the clip plays its source altered.
    pub fn variant(&self) -> Option<(f64, f32)> {
        ((self.stretch - 1.0).abs() > 1e-9 || self.pitch.abs() > 1e-6)
            .then_some((self.stretch, self.pitch))
    }

    pub fn out_shape(&self) -> FadeShape {
        self.fade_out_shape.unwrap_or(self.fade_shape)
    }

    pub fn is_midi(&self) -> bool {
        self.notes.is_some()
    }

    /// Whether the content of a MIDI clip would begin before the project's start (the clip moved
    /// further left than it was trimmed), which positions counted from it cannot express.
    pub fn needs_rebase(&self) -> bool {
        self.is_midi() && self.offset > self.start
    }

    /// Moves the notes and controls of such a clip so its content begins at 0: what plays stays
    /// the same, and what is dropped could never be shown again.
    pub fn rebase(&mut self) {
        if !self.needs_rebase() {
            return;
        }
        let shift = self.offset - self.start;
        if let Some(notes) = &mut self.notes {
            let notes = Arc::make_mut(notes);
            notes.retain(|n| n.start >= shift);
            for n in notes.iter_mut() {
                n.start -= shift;
            }
        }
        if let Some(controls) = &mut self.controls {
            let controls = Arc::make_mut(controls);
            controls.retain(|p| p.time >= shift);
            for p in controls.iter_mut() {
                p.time -= shift;
            }
        }
        self.offset = self.start;
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub id: TrackId,
    pub name: String,
    pub color: [u8; 3],
    pub gain_db: f32,
    /// -1 (left) to 1 (right).
    pub pan: f32,
    pub mute: bool,
    pub solo: bool,
    pub clips: Vec<Clip>,
    /// First hardware input recorded on this track.
    #[serde(default)]
    pub input: u16,
    /// Records two inputs (`input` and the next one) instead of one.
    #[serde(default)]
    pub stereo: bool,
    #[serde(default)]
    pub armed: bool,
    /// Name of the playlist holding `clips`.
    #[serde(default = "default_playlist")]
    pub playlist: String,
    /// Inactive playlists.
    #[serde(default)]
    pub alternates: Vec<Playlist>,
    /// Take lanes: each take recorded on the track kept whole in a lane of its own, which the
    /// track's clips (the comp) are assembled from (see `edit::record_take`,
    /// `edit::comp_take`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub takes: Vec<Playlist>,
    #[serde(default)]
    pub kind: TrackKind,
    /// Bus this track is routed to, and shown under; without one it goes to the master.
    #[serde(default)]
    pub parent: Option<TrackId>,
    /// Tracks routed to this bus hidden in the editor.
    #[serde(default)]
    pub collapsed: bool,
    /// Feeds its bus, or the master.
    #[serde(default = "yes")]
    pub master_send: bool,
    #[serde(default)]
    pub sends: Vec<TrackSend>,
    #[serde(default)]
    pub inserts: Vec<Insert>,
    /// VCA track controlling this one.
    #[serde(default)]
    pub vca: Option<TrackId>,
    /// Input gain before the inserts.
    #[serde(default)]
    pub trim_db: f32,
    /// Inverts the polarity of the input.
    #[serde(default)]
    pub invert: bool,
    #[serde(default)]
    pub automation: Vec<AutomationLane>,
    #[serde(default)]
    pub automation_mode: AutomationMode,
    /// Automation lanes shown under the track in the editor.
    #[serde(default)]
    pub show_automation: bool,
    /// Alternate playlists shown as take lanes under the track, for comping.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub show_takes: bool,
    /// When the track hears its input.
    #[serde(default, skip_serializing_if = "Monitoring::is_auto")]
    pub monitor: Monitoring,
    /// Rendered audio played instead of the clips and inserts (freeze).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frozen: Option<Frozen>,
    /// MIDI output port the track's MIDI is sent to (external instruments).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub midi_out: Option<String>,
    /// Every insert of the track bypassed, without touching their own bypass.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub fx_off: bool,
    /// Its own console color instead of the project's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub console: Option<crate::console::Console>,
    /// Icon chosen for the track (else found from its name and content).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// Height in the timeline, relative to the others' (1 for the same).
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub height: f32,
    #[serde(default, skip_serializing_if = "RecordSource::is_raw")]
    pub record_source: RecordSource,
    /// The track whose processed takes this one receives.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub printed_from: Option<TrackId>,
}

pub(crate) fn one() -> f32 {
    1.0
}

pub(crate) fn is_one(v: &f32) -> bool {
    *v == 1.0
}

/// A finished song to compare the mix with (heard instead of it, never exported).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReferenceTrack {
    /// Relative to the project folder when inside it.
    pub path: std::path::PathBuf,
    /// Brought to the mix's loudness, to compare the sound and not the level.
    #[serde(default = "yes")]
    pub matched: bool,
}

/// A track rendered with its inserts, to save processing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Frozen {
    pub source: SourceId,
    pub start: u64,
}

fn default_playlist() -> String {
    "Main".to_owned()
}

pub(crate) fn yes() -> bool {
    true
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TrackKind {
    /// Plays clips and records.
    #[default]
    Audio,
    /// Carries no audio: its fader and mute act on the member tracks.
    Vca,
    /// Sums the tracks routed to it through its inserts, sends and fader; holds no clips.
    Bus,
}

impl TrackKind {
    /// Audio goes through it: a track or a bus.
    pub fn carries_audio(self) -> bool {
        matches!(self, TrackKind::Audio | TrackKind::Bus)
    }
}

/// Extra feed from a track into another track's input.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrackSend {
    pub target: TrackId,
    pub gain_db: f32,
    /// Taken before the fader and pan instead of after.
    pub pre_fader: bool,
    pub mute: bool,
}

/// An effect in a channel's insert chain.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Insert {
    pub id: InsertId,
    /// Built-in effect identifier, or `format:id` for plugins (`clap:org.example.reverb`).
    pub effect: String,
    pub bypass: bool,
    /// Parameter values by id; plugin parameters use their numeric id.
    pub params: BTreeMap<String, f32>,
    /// Opaque plugin state, base64.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    /// Files loaded by a built-in effect (amp model, impulse response), by slot; relative to
    /// the project folder when inside it.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub files: BTreeMap<String, std::path::PathBuf>,
    /// Track whose output drives the effect's detection (compressor, gate).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sidechain: Option<TrackId>,
    /// Master insert heard on the outputs only, after the meters, never exported (headphone or
    /// room correction, reference checks).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub monitor: bool,
    /// Gain after the effect (dB), to match the loudness of the dry signal.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub gain_db: f32,
    /// Branches of a parallel chain (effect `parallel`): each plays the signal through its own
    /// inserts, then they are mixed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub branches: Vec<Branch>,
    /// Plugin parameters shown on the insert's face in the rack.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub macros: Vec<String>,
}

/// A branch of a parallel chain: its inserts and its level in the mix.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Branch {
    pub name: String,
    #[serde(default)]
    pub inserts: Vec<Insert>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub gain_db: f32,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub mute: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub solo: bool,
}

impl Branch {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            inserts: Vec::new(),
            gain_db: 0.0,
            mute: false,
            solo: false,
        }
    }
}

impl Track {
    pub fn end(&self) -> u64 {
        self.clips.iter().map(Clip::end).max().unwrap_or(0)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Master {
    pub gain_db: f32,
    #[serde(default)]
    pub inserts: Vec<Insert>,
    #[serde(default)]
    pub automation: Vec<AutomationLane>,
    /// The master's own inserts bypassed (not the monitoring ones).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub fx_off: bool,
    /// How its automation plays and records; it read it always before it had one.
    #[serde(default)]
    pub automation_mode: AutomationMode,
}

const PALETTE: [[u8; 3]; 8] = [
    [86, 156, 214],
    [214, 157, 86],
    [120, 190, 110],
    [200, 100, 120],
    [160, 120, 210],
    [90, 190, 190],
    [210, 200, 90],
    [150, 150, 150],
];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    pub sample_rate: u32,
    pub tempo: TempoMap,
    pub sources: BTreeMap<SourceId, Source>,
    pub tracks: Vec<Arc<Track>>,
    pub master: Master,
    pub loop_range: Option<(u64, u64)>,
    /// Recording only happens inside this range when set.
    #[serde(default)]
    pub punch_range: Option<(u64, u64)>,
    #[serde(default)]
    pub markers: Vec<Marker>,
    /// MIDI controllers mapped to controls (MIDI learn).
    #[serde(default)]
    pub midi_map: Vec<MidiMapping>,
    #[serde(default)]
    pub metronome: MetronomeSettings,
    /// Track groups.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<crate::groups::TrackGroup>,
    /// Every group suspended: members behave on their own.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub groups_off: bool,
    /// Mix snapshots, recalled from the console.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub snapshots: Vec<crate::snapshot::MixSnapshot>,
    /// Every insert of every track and of the master bypassed, for comparisons.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub all_fx_off: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<ReferenceTrack>,
    /// The loop range is kept, but playback does not loop.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub loop_off: bool,
    /// Read from older projects only: the loop is the time selection now, always.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub loop_fixed: bool,
    /// Console color of every channel, the buses and the master.
    #[serde(default, skip_serializing_if = "crate::console::Console::is_default")]
    pub console: crate::console::Console,
    /// The consoles' hiss and crosstalk are heard too.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub console_noise: bool,
    /// Every console color switched off (the settings are kept).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub console_off: bool,
    /// Data of optional modules, by module name, kept as is by builds without them.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extensions: BTreeMap<String, serde_json::Value>,
    next_id: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MetronomeSettings {
    pub enabled: bool,
    pub gain_db: f32,
    /// Bars of clicks before recording starts.
    pub count_in: u32,
    pub sound: ClickSound,
    /// Clicks per beat: 1, 2 (eighths), 3 (triplets) or 4 (sixteenths).
    pub subdivision: u32,
    /// Accent the first beat of the bar.
    pub accent: bool,
    /// In 6/8, 9/8 and 12/8, click the dotted quarters rather than every eighth.
    pub compound: bool,
    /// Accented groups of beats for odd meters, such as "2+2+3" in 7/8 (empty: none).
    pub groups: String,
    /// Click only while recording (and during the count-in).
    pub recording_only: bool,
    /// Bars played before the count-in, to hear the song before recording.
    pub pre_roll: u32,
}

impl Default for MetronomeSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            gain_db: -6.0,
            count_in: 0,
            sound: ClickSound::default(),
            subdivision: 1,
            accent: true,
            compound: true,
            groups: String::new(),
            recording_only: false,
            pre_roll: 0,
        }
    }
}

impl MetronomeSettings {
    /// Beats of a `numerator`-beat bar that start an accented group (bit per beat, from 0),
    /// or 0 when the groups do not add up to the bar.
    pub fn group_starts(&self, numerator: u32) -> u32 {
        let mut mask = 0u32;
        let mut at = 0u32;
        for part in self
            .groups
            .split('+')
            .map(str::trim)
            .filter(|p| !p.is_empty())
        {
            let Ok(n) = part.parse::<u32>() else {
                return 0;
            };
            if n == 0 || at >= 32 {
                return 0;
            }
            mask |= 1 << at;
            at += n;
        }
        if at == numerator { mask } else { 0 }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClickSound {
    #[default]
    Beep,
    Woodblock,
    Clave,
    Cowbell,
    HiHat,
}

impl ClickSound {
    pub const ALL: [ClickSound; 5] = [
        ClickSound::Beep,
        ClickSound::Woodblock,
        ClickSound::Clave,
        ClickSound::Cowbell,
        ClickSound::HiHat,
    ];

    pub fn name(self) -> &'static str {
        match self {
            ClickSound::Beep => "Beep",
            ClickSound::Woodblock => "Wood block",
            ClickSound::Clave => "Clave",
            ClickSound::Cowbell => "Cowbell",
            ClickSound::HiHat => "Hi-hat",
        }
    }
}

/// Latest position a project can use: about 260 days at 48 kHz, far from overflows.
pub const MAX_POSITION: u64 = 1 << 40;

/// Gain floor, heard as silence.
pub const MIN_GAIN_DB: f32 = -144.0;
/// Highest track and master gain, the top of the fader.
pub const MAX_TRACK_GAIN_DB: f32 = 12.0;
/// Highest clip gain.
pub const MAX_CLIP_GAIN_DB: f32 = 24.0;

/// A MIDI controller driving a control: values 0-127 map to `min..max`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MidiMapping {
    pub channel: u8,
    pub controller: u8,
    /// `None` for the master.
    pub track: Option<TrackId>,
    pub target: AutomationTarget,
    pub min: f32,
    pub max: f32,
}

/// Gives `insert` (and those of its branches) a new id if `seen` has it, then marks it seen;
/// `renewed` gets the old and new ids.
fn renew_repeated(
    insert: &mut Insert,
    seen: &mut std::collections::HashSet<InsertId>,
    next: &mut u64,
    renewed: &mut Vec<(InsertId, InsertId)>,
) {
    if !seen.insert(insert.id) {
        let id = InsertId(*next);
        *next = next.saturating_add(1);
        renewed.push((insert.id, id));
        insert.id = id;
        seen.insert(id);
    }
    for branch in &mut insert.branches {
        for inner in &mut branch.inserts {
            renew_repeated(inner, seen, next, renewed);
        }
    }
}

/// Points in range, finite, sorted and one per time.
fn repair_lanes(lanes: &mut [AutomationLane]) {
    for lane in lanes {
        for p in &mut lane.points {
            p.time = p.time.min(MAX_POSITION);
            if !p.value.is_finite() {
                p.value = 0.0;
            }
            p.tension = if p.tension.is_finite() {
                p.tension.clamp(-1.0, 1.0)
            } else {
                0.0
            };
        }
        lane.points.sort_by_key(|p| p.time);
        lane.points.dedup_by_key(|p| p.time);
    }
}

impl Default for Project {
    fn default() -> Self {
        Self::new("Untitled", 48_000)
    }
}

impl Project {
    pub fn new(name: &str, sample_rate: u32) -> Self {
        Self {
            name: name.to_owned(),
            sample_rate,
            tempo: TempoMap::default(),
            sources: BTreeMap::new(),
            tracks: Vec::new(),
            master: Master {
                gain_db: 0.0,
                inserts: Vec::new(),
                automation: Vec::new(),
                fx_off: false,
                automation_mode: AutomationMode::Read,
            },
            loop_range: None,
            punch_range: None,
            markers: Vec::new(),
            midi_map: Vec::new(),
            metronome: MetronomeSettings::default(),
            snapshots: Vec::new(),
            groups: Vec::new(),
            groups_off: false,
            all_fx_off: false,
            loop_off: false,
            loop_fixed: false,
            console: Default::default(),
            console_noise: false,
            console_off: false,
            extensions: BTreeMap::new(),
            reference: None,
            next_id: 1,
        }
    }

    /// Largest id in use.
    pub fn highest_id(&self) -> u64 {
        let mut highest = self.sources.keys().map(|s| s.0).max().unwrap_or(0);
        for t in &self.tracks {
            highest = highest.max(t.id.0);
            for c in t
                .clips
                .iter()
                .chain(t.alternates.iter().flat_map(|p| p.clips.iter()))
                .chain(t.takes.iter().flat_map(|p| p.clips.iter()))
            {
                highest = highest.max(c.id.0);
                for i in crate::routing::flatten(&c.inserts) {
                    highest = highest.max(i.id.0);
                }
            }
            for i in &t.inserts {
                highest = highest.max(i.id.0);
            }
        }
        for i in &self.master.inserts {
            highest = highest.max(i.id.0);
        }
        for m in &self.markers {
            highest = highest.max(m.id.0);
        }
        highest
    }

    /// Gives new ids to the effects of a clip that share theirs with another chain: a clip
    /// split, duplicated or pasted copies its effects, which must stay apart.
    pub fn separate_clip_inserts(&mut self) {
        let has_clip_inserts = self.tracks.iter().any(|t| {
            t.clips
                .iter()
                .chain(t.alternates.iter().flat_map(|p| p.clips.iter()))
                .chain(t.takes.iter().flat_map(|p| p.clips.iter()))
                .any(|c| !c.inserts.is_empty())
        });
        if !has_clip_inserts {
            return;
        }
        let mut seen: std::collections::HashSet<InsertId> = self
            .master
            .inserts
            .iter()
            .chain(self.tracks.iter().flat_map(|t| t.inserts.iter()))
            .flat_map(|i| crate::routing::flatten(std::slice::from_ref(i)))
            .map(|i| i.id)
            .collect();
        let mut next = self.next_id;
        for index in 0..self.tracks.len() {
            let track = &self.tracks[index];
            let clash = track
                .clips
                .iter()
                .chain(track.alternates.iter().flat_map(|p| p.clips.iter()))
                .chain(track.takes.iter().flat_map(|p| p.clips.iter()))
                .flat_map(|c| crate::routing::flatten(&c.inserts))
                .any(|i| seen.contains(&i.id));
            let all_new = || {
                track
                    .clips
                    .iter()
                    .chain(track.alternates.iter().flat_map(|p| p.clips.iter()))
                    .chain(track.takes.iter().flat_map(|p| p.clips.iter()))
                    .flat_map(|c| crate::routing::flatten(&c.inserts))
                    .map(|i| i.id)
                    .collect::<Vec<_>>()
            };
            let ids = all_new();
            let repeated = ids.len() != ids.iter().collect::<std::collections::HashSet<_>>().len();
            if !clash && !repeated {
                seen.extend(ids);
                continue;
            }
            let t = Arc::make_mut(&mut self.tracks[index]);
            for clip in t
                .clips
                .iter_mut()
                .chain(t.alternates.iter_mut().flat_map(|p| p.clips.iter_mut()))
                .chain(t.takes.iter_mut().flat_map(|p| p.clips.iter_mut()))
            {
                let mut renewed = Vec::new();
                for insert in &mut clip.inserts {
                    renew_repeated(insert, &mut seen, &mut next, &mut renewed);
                }
                // Its automation follows its effects to their new ids.
                for lane in &mut clip.automation {
                    if let crate::automation::AutomationTarget::Param { insert, .. } =
                        &mut lane.target
                        && let Some(&(_, id)) = renewed.iter().find(|(old, _)| *old == *insert)
                    {
                        *insert = id;
                    }
                }
            }
        }
        self.next_id = next;
    }

    /// Rebases the MIDI clips whose content would begin before the project's start.
    pub fn rebase_midi(&mut self) {
        for track in &mut self.tracks {
            let clips = || {
                track
                    .clips
                    .iter()
                    .chain(track.alternates.iter().flat_map(|p| p.clips.iter()))
                    .chain(track.takes.iter().flat_map(|p| p.clips.iter()))
            };
            if !clips().any(Clip::needs_rebase) {
                continue;
            }
            let t = Arc::make_mut(track);
            for clip in t
                .clips
                .iter_mut()
                .chain(t.alternates.iter_mut().flat_map(|p| p.clips.iter_mut()))
                .chain(t.takes.iter_mut().flat_map(|p| p.clips.iter_mut()))
            {
                clip.rebase();
            }
        }
    }

    /// Brings values from a damaged or hand-edited file back into range, so that editing and
    /// playback can rely on them.
    pub fn repair(&mut self) {
        // Fresh ids must not collide with ids already in the file.
        self.next_id = self.next_id.max(self.highest_id().saturating_add(1));
        for source in self.sources.values_mut() {
            source.sample_rate = source.sample_rate.clamp(1, 768_000);
            source.channels = source.channels.clamp(1, 64);
        }
        let at = |t: u64| t.min(MAX_POSITION);
        for range in [&mut self.loop_range, &mut self.punch_range] {
            *range = range.map(|(a, b)| (at(a), at(b))).filter(|(a, b)| a < b);
        }
        for m in &mut self.markers {
            m.position = at(m.position);
        }
        self.markers.sort_by_key(|m| (m.position, m.id));
        for track in &mut self.tracks {
            let t = Arc::make_mut(track);
            if !t.gain_db.is_finite() {
                t.gain_db = 0.0;
            }
            t.gain_db = t.gain_db.clamp(MIN_GAIN_DB, MAX_TRACK_GAIN_DB);
            t.pan = if t.pan.is_finite() {
                t.pan.clamp(-1.0, 1.0)
            } else {
                0.0
            };
            t.trim_db = if t.trim_db.is_finite() {
                t.trim_db.clamp(-48.0, 48.0)
            } else {
                0.0
            };
            repair_lanes(&mut t.automation);
            for clip in t
                .clips
                .iter_mut()
                .chain(t.alternates.iter_mut().flat_map(|p| p.clips.iter_mut()))
                .chain(t.takes.iter_mut().flat_map(|p| p.clips.iter_mut()))
            {
                clip.start = at(clip.start);
                clip.offset = at(clip.offset);
                clip.length = clip.length.clamp(1, MAX_POSITION);
                repair_lanes(&mut clip.automation);
                if let Some(notes) = &mut clip.notes {
                    for n in Arc::make_mut(notes) {
                        n.start = at(n.start);
                        n.length = n.length.clamp(1, MAX_POSITION);
                        n.key = n.key.min(127);
                        n.velocity = n.velocity.clamp(1, 127);
                        n.channel = n.channel.min(15);
                    }
                }
                if let Some(controls) = &mut clip.controls {
                    let controls = Arc::make_mut(controls);
                    for p in controls.iter_mut() {
                        p.time = at(p.time);
                        p.channel = p.channel.min(15);
                        if !p.value.is_finite() {
                            p.value = 0.0;
                        }
                    }
                    controls.sort_by_key(|p| (p.time, p.controller, p.channel));
                }
                if !clip.stretch.is_finite() {
                    clip.stretch = 1.0;
                }
                clip.stretch = clip.stretch.clamp(0.1, 10.0);
                clip.pitch = if clip.pitch.is_finite() {
                    clip.pitch.clamp(-24.0, 24.0)
                } else {
                    0.0
                };
                clip.gain_db = if clip.gain_db.is_finite() {
                    clip.gain_db.clamp(MIN_GAIN_DB, MAX_CLIP_GAIN_DB)
                } else {
                    0.0
                };
                clip.fade_in = clip.fade_in.min(clip.length);
                clip.fade_out = clip.fade_out.min(clip.length - clip.fade_in);
            }
        }
        if !self.master.gain_db.is_finite() {
            self.master.gain_db = 0.0;
        }
        self.master.gain_db = self.master.gain_db.clamp(MIN_GAIN_DB, MAX_TRACK_GAIN_DB);
        repair_lanes(&mut self.master.automation);
        let gain = &mut self.metronome.gain_db;
        *gain = if gain.is_finite() {
            gain.clamp(-60.0, 12.0)
        } else {
            -6.0
        };
        self.metronome.count_in = self.metronome.count_in.min(8);
        let ids: Vec<TrackId> = self.tracks.iter().map(|t| t.id).collect();
        for g in &mut self.groups {
            let mut kept = Vec::with_capacity(g.tracks.len());
            for t in g.tracks.drain(..) {
                if ids.contains(&t) && !kept.contains(&t) {
                    kept.push(t);
                }
            }
            g.tracks = kept;
        }
    }

    pub fn new_track_id(&mut self) -> TrackId {
        TrackId(self.next())
    }

    pub fn new_clip_id(&mut self) -> ClipId {
        ClipId(self.next())
    }

    pub fn new_marker_id(&mut self) -> MarkerId {
        MarkerId(self.next())
    }

    /// Next id that will be handed out.
    pub fn peek_id(&self) -> u64 {
        self.next_id
    }

    /// Ensures ids below `next` are never handed out again.
    pub(crate) fn reserve_ids(&mut self, next: u64) {
        self.next_id = self.next_id.max(next);
    }

    fn next(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    /// End of the last clip, in samples.
    pub fn length(&self) -> u64 {
        self.tracks.iter().map(|t| t.end()).max().unwrap_or(0)
    }

    /// The range playback loops over, when looping is on.
    pub fn looping(&self) -> Option<(u64, u64)> {
        self.loop_range.filter(|_| !self.loop_off)
    }

    pub fn add_source(&mut self, source: Source) -> SourceId {
        let id = SourceId(self.next());
        self.sources.insert(id, source);
        id
    }

    pub fn add_track(&mut self, name: &str) -> TrackId {
        let id = TrackId(self.next());
        let color = PALETTE[self.tracks.len() % PALETTE.len()];
        self.tracks.push(Arc::new(Track {
            id,
            name: name.to_owned(),
            color,
            gain_db: 0.0,
            pan: 0.0,
            mute: false,
            solo: false,
            clips: Vec::new(),
            input: 0,
            stereo: false,
            armed: false,
            playlist: default_playlist(),
            alternates: Vec::new(),
            takes: Vec::new(),
            kind: TrackKind::Audio,
            parent: None,
            collapsed: false,
            master_send: true,
            sends: Vec::new(),
            inserts: Vec::new(),
            vca: None,
            trim_db: 0.0,
            invert: false,
            automation: Vec::new(),
            automation_mode: AutomationMode::default(),
            show_automation: false,
            show_takes: false,
            monitor: Monitoring::Auto,
            frozen: None,
            midi_out: None,
            fx_off: false,
            console: None,
            icon: None,
            height: 1.0,
            record_source: RecordSource::Raw,
            printed_from: None,
        }));
        id
    }

    pub fn track(&self, id: TrackId) -> Option<&Track> {
        self.tracks.iter().find(|t| t.id == id).map(|t| t.as_ref())
    }

    /// Mutable access, copying the track only if an undo snapshot still shares it.
    pub fn track_mut(&mut self, id: TrackId) -> Option<&mut Track> {
        self.tracks
            .iter_mut()
            .find(|t| t.id == id)
            .map(Arc::make_mut)
    }

    /// Removes a track and every reference to it (sends, folder, VCA); children move up.
    pub fn remove_track(&mut self, id: TrackId) -> bool {
        let Some(removed) = self.track(id).map(|t| t.parent) else {
            return false;
        };
        self.tracks.retain(|t| t.id != id);
        self.midi_map
            .retain(|m| m.track != Some(id) && m.target != AutomationTarget::Send(id));
        for g in &mut self.groups {
            g.tracks.retain(|t| *t != id);
        }
        // Down the branches of parallel chains too.
        fn unkey(list: &mut [Insert], id: TrackId) {
            for insert in list {
                if insert.sidechain == Some(id) {
                    insert.sidechain = None;
                }
                for branch in &mut insert.branches {
                    unkey(&mut branch.inserts, id);
                }
            }
        }
        unkey(&mut self.master.inserts, id);
        for t in &mut self.tracks {
            let keyed = crate::routing::flatten(&t.inserts)
                .iter()
                .any(|i| i.sidechain == Some(id));
            if keyed {
                unkey(&mut Arc::make_mut(t).inserts, id);
            }
        }
        for t in &mut self.tracks {
            let refers =
                t.parent == Some(id) || t.vca == Some(id) || t.sends.iter().any(|s| s.target == id);
            if refers {
                let t = Arc::make_mut(t);
                if t.parent == Some(id) {
                    t.parent = removed;
                }
                if t.vca == Some(id) {
                    t.vca = None;
                }
                t.sends.retain(|s| s.target != id);
            }
            if t.automation
                .iter()
                .any(|l| l.target == AutomationTarget::Send(id))
            {
                Arc::make_mut(t)
                    .automation
                    .retain(|l| l.target != AutomationTarget::Send(id));
            }
        }
        true
    }

    pub fn new_insert_id(&mut self) -> InsertId {
        InsertId(self.next())
    }

    /// Places the whole of `source` on `track` at `start`.
    pub fn add_clip(&mut self, track: TrackId, source: SourceId, start: u64) -> Option<ClipId> {
        let rate = self.sample_rate;
        let src = self.sources.get(&source)?;
        let length = src.frames_at(rate);
        let name = src
            .path
            .file_stem()
            .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
        let id = ClipId(self.next());
        let track = self.track_mut(track)?;
        track.clips.push(Clip {
            id,
            source,
            name,
            start,
            offset: 0,
            length,
            gain_db: 0.0,
            fade_in: 0,
            fade_out: 0,
            fade_shape: FadeShape::default(),
            fade_out_shape: None,
            muted: false,
            stretch: 1.0,
            pitch: 0.0,
            controls: None,
            notes: None,
            variations: Vec::new(),
            take: None,
            inserts: Vec::new(),
            automation: Vec::new(),
        });
        Some(id)
    }

    /// Adds an empty MIDI clip to `track`.
    pub fn add_midi_clip(
        &mut self,
        track: TrackId,
        start: u64,
        length: u64,
        notes: Vec<Note>,
    ) -> Option<ClipId> {
        let id = ClipId(self.next());
        let name = self.track(track)?.name.clone();
        let track = self.track_mut(track)?;
        track.clips.push(Clip {
            id,
            source: SourceId(0),
            name,
            start,
            offset: 0,
            length: length.max(1),
            gain_db: 0.0,
            fade_in: 0,
            fade_out: 0,
            fade_shape: FadeShape::default(),
            fade_out_shape: None,
            muted: false,
            stretch: 1.0,
            pitch: 0.0,
            controls: None,
            notes: Some(Arc::new(notes)),
            variations: Vec::new(),
            take: None,
            inserts: Vec::new(),
            automation: Vec::new(),
        });
        track.clips.sort_by_key(|c| (c.start, c.id));
        Some(id)
    }

    /// Removes sources no clip uses anymore.
    pub fn prune_sources(&mut self) {
        let used: Vec<SourceId> = self
            .tracks
            .iter()
            .flat_map(|t| {
                t.clips
                    .iter()
                    .chain(t.alternates.iter().flat_map(|p| &p.clips))
                    .chain(t.takes.iter().flat_map(|p| &p.clips))
                    .filter(|c| !c.is_midi())
                    .map(|c| c.source)
                    .chain(t.frozen.map(|f| f.source))
            })
            .collect();
        self.sources.retain(|id, _| used.contains(id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metronome_groups_must_fill_the_bar() {
        let mut click = MetronomeSettings {
            groups: "2+2+3".into(),
            ..Default::default()
        };
        assert_eq!(click.group_starts(7), 0b10101);
        assert_eq!(
            click.group_starts(8),
            0,
            "groups that do not fill the bar are ignored"
        );
        click.groups = "3 + 2".into();
        assert_eq!(click.group_starts(5), 0b1001);
        click.groups = "x+2".into();
        assert_eq!(click.group_starts(3), 0);
    }

    fn source(frames: u64, rate: u32) -> Source {
        Source {
            path: PathBuf::from("Audio Files/kick.wav"),
            channels: 2,
            sample_rate: rate,
            frames,
        }
    }

    #[test]
    fn clips_use_project_rate_lengths() {
        let mut p = Project::new("Song", 48_000);
        let s = p.add_source(source(44_100, 44_100));
        let t = p.add_track("Drums");
        let c = p.add_clip(t, s, 1_000).unwrap();
        let clip = &p.track(t).unwrap().clips[0];
        assert_eq!(clip.id, c);
        assert_eq!(clip.length, 48_000);
        assert_eq!(clip.name, "kick");
        assert_eq!(p.length(), 49_000);
    }

    #[test]
    fn ids_are_unique_and_colors_rotate() {
        let mut p = Project::default();
        let a = p.add_track("a");
        let b = p.add_track("b");
        assert_ne!(a, b);
        assert_ne!(p.track(a).unwrap().color, p.track(b).unwrap().color);
    }

    #[test]
    fn editing_a_shared_track_copies_it() {
        let mut p = Project::default();
        let t = p.add_track("a");
        let snapshot = p.clone();
        p.track_mut(t).unwrap().gain_db = -6.0;
        assert_eq!(snapshot.track(t).unwrap().gain_db, 0.0);
        assert_eq!(p.track(t).unwrap().gain_db, -6.0);
    }

    #[test]
    fn repair_brings_damaged_values_back_into_range() {
        use crate::automation::{AutomationPoint, CurveShape};
        let mut p = Project::default();
        let s = p.add_source(source(10, 0));
        let t = p.add_track("a");
        let c = p.add_clip(t, s, 0).unwrap();
        p.add_midi_clip(t, 0, 100, Vec::new()).unwrap();
        p.groups.push(crate::groups::TrackGroup::new("g", [0; 3]));
        p.groups[0].tracks = vec![t, TrackId(77), t];
        let u = p.add_track("b");
        p.groups[0].tracks.extend([u, t]);
        p.loop_range = Some((10, 5));
        p.punch_range = Some((0, u64::MAX));
        p.next_id = 1;
        p.master.gain_db = f32::NAN;
        let point = |time, value| AutomationPoint::new(time, value, CurveShape::Linear);
        let mut lane = AutomationLane::new(AutomationTarget::Gain);
        lane.points = vec![point(50, 1.0), point(10, f32::INFINITY), point(50, 2.0)];
        p.master.automation.push(lane.clone());
        let track = p.track_mut(t).unwrap();
        (track.gain_db, track.pan, track.trim_db) = (f32::NAN, 3.0, f32::NEG_INFINITY);
        track.automation.push(lane);
        let clip = &mut track.clips[0];
        (clip.stretch, clip.pitch, clip.gain_db) = (f64::NAN, 99.0, f32::NAN);
        (clip.length, clip.fade_in, clip.fade_out) = (0, 5, 5);
        track.clips[1].controls = Some(Arc::new(vec![
            ControlPoint {
                time: u64::MAX,
                controller: 1,
                value: f32::NAN,
                channel: 200,
            },
            ControlPoint {
                time: 3,
                controller: 1,
                value: 0.5,
                channel: 0,
            },
        ]));
        p.repair();
        assert!(p.peek_id() > c.0, "fresh ids skip the ones in use");
        assert_eq!(p.sources[&s].sample_rate, 1);
        assert_eq!(p.loop_range, None);
        assert_eq!(p.punch_range, Some((0, MAX_POSITION)));
        assert_eq!(p.groups[0].tracks, [t, u]);
        assert_eq!(p.master.gain_db, 0.0);
        let times = |l: &AutomationLane| {
            l.points
                .iter()
                .map(|p| (p.time, p.value))
                .collect::<Vec<_>>()
        };
        assert_eq!(times(&p.master.automation[0]), [(10, 0.0), (50, 1.0)]);
        let track = p.track(t).unwrap();
        assert_eq!((track.gain_db, track.pan, track.trim_db), (0.0, 1.0, 0.0));
        assert_eq!(times(&track.automation[0]), [(10, 0.0), (50, 1.0)]);
        let clip = track.clips.iter().find(|x| x.id == c).unwrap();
        assert_eq!((clip.stretch, clip.pitch, clip.gain_db), (1.0, 24.0, 0.0));
        assert_eq!((clip.length, clip.fade_in, clip.fade_out), (1, 1, 0));
        let midi = track.clips.iter().find(|x| x.is_midi()).unwrap();
        let controls = midi.controls.as_deref().unwrap();
        assert_eq!(controls[0].time, 3);
        assert_eq!(
            (controls[1].time, controls[1].value, controls[1].channel),
            (MAX_POSITION, 0.0, 15)
        );
    }

    #[test]
    fn removing_a_track_clears_sidechains_groups_and_send_lanes() {
        let mut p = Project::default();
        let (a, b) = (p.add_track("a"), p.add_track("b"));
        let insert = Insert {
            id: p.new_insert_id(),
            effect: "compressor".into(),
            bypass: false,
            params: BTreeMap::new(),
            state: None,
            files: BTreeMap::new(),
            sidechain: Some(b),
            monitor: false,
            gain_db: 0.0,
            branches: Vec::new(),
            macros: Vec::new(),
        };
        p.master.inserts.push(insert.clone());
        let track = p.track_mut(a).unwrap();
        track.inserts.push(insert);
        track
            .automation
            .push(AutomationLane::new(AutomationTarget::Send(b)));
        p.groups.push(crate::groups::TrackGroup::new("g", [0; 3]));
        p.groups[0].tracks = vec![a, b];
        assert!(p.remove_track(b));
        assert!(!p.remove_track(b));
        assert_eq!(p.master.inserts[0].sidechain, None);
        let track = p.track(a).unwrap();
        assert_eq!(track.inserts[0].sidechain, None);
        assert!(track.automation.is_empty());
        assert_eq!(p.groups[0].tracks, [a]);
    }

    #[test]
    fn controller_messages_round_trip() {
        for (controller, value, channel) in [
            (7, 1.0, 3),
            (PITCH_BEND, 0.5, 0),
            (CHANNEL_PRESSURE, 0.0, 15),
        ] {
            let point = ControlPoint {
                time: 9,
                controller,
                value,
                channel,
            };
            let back = ControlPoint::from_message(9, point.message()).unwrap();
            assert_eq!((back.controller, back.channel), (controller, channel));
            assert!((back.value - value).abs() < 1e-3, "{controller}");
        }
        assert_eq!(ControlPoint::from_message(0, [0x90, 60, 100]), None);
    }

    #[test]
    fn fades_start_silent_and_end_at_full_level() {
        for shape in FadeShape::ALL {
            assert_eq!(shape.gain(-1.0), 0.0, "{}", shape.name());
            assert!((shape.gain(2.0) - 1.0).abs() < 1e-6, "{}", shape.name());
            let mid = shape.gain(0.5);
            assert!(mid > 0.0 && mid < 1.0, "{}", shape.name());
        }
        assert!(FadeShape::Fast.gain(0.5) > FadeShape::Slow.gain(0.5));
    }

    #[test]
    fn unused_sources_are_pruned() {
        let mut p = Project::default();
        let used = p.add_source(source(10, 48_000));
        let unused = p.add_source(source(10, 48_000));
        let t = p.add_track("a");
        p.add_clip(t, used, 0);
        p.prune_sources();
        assert!(p.sources.contains_key(&used));
        assert!(!p.sources.contains_key(&unused));
    }

    #[test]
    fn copied_clips_get_effects_of_their_own() {
        let mut p = Project::new("t", 48_000);
        let t = p.add_track("a");
        let s = p.add_source(Source {
            path: "a.wav".into(),
            channels: 1,
            sample_rate: 48_000,
            frames: 1000,
        });
        let c = p.add_clip(t, s, 0).unwrap();
        let channel = crate::routing::Channel::Clip(c);
        let eq = crate::routing::add_insert(&mut p, channel, "eq", BTreeMap::new()).unwrap();
        assert_eq!(crate::routing::inserts(&p, channel)[0].id, eq);
        let target = AutomationTarget::Param {
            insert: eq,
            param: "b1_gain".into(),
        };
        p.track_mut(t).unwrap().clips[0]
            .automation
            .push(AutomationLane::new(target));
        let mut copy = p.track(t).unwrap().clips[0].clone();
        copy.id = ClipId(p.highest_id() + 100);
        copy.start = 2000;
        p.track_mut(t).unwrap().clips.push(copy);
        p.separate_clip_inserts();
        let clips = &p.track(t).unwrap().clips;
        let (first, second) = (clips[0].inserts[0].id, clips[1].inserts[0].id);
        assert_eq!(first, eq);
        assert_ne!(second, eq);
        // Its automation follows its own effect.
        let follows = |c: &Clip, id: InsertId| matches!(&c.automation[0].target, AutomationTarget::Param { insert, .. } if *insert == id);
        assert!(follows(&clips[0], first) && follows(&clips[1], second));
        assert!(p.new_insert_id().0 > second.0);
        // A clip that is not there takes none.
        let midi = crate::routing::add_insert(
            &mut p,
            crate::routing::Channel::Clip(ClipId(424_242)),
            "eq",
            BTreeMap::new(),
        );
        assert_eq!(midi, None);
    }
}
