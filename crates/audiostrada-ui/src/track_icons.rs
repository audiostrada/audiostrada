//! Track icons: an instrument for each track, chosen or found from its name (English and
//! French words and abbreviations of studio track sheets), its folder and what it plays.
//! Glyphs from Lucide (ISC), in assets/fonts/Lucide-TrackIcons.ttf.

use audiostrada_core::{Project, Track, TrackKind};
use eframe::egui::{self, Color32, FontId, Rect, Sense, Ui, vec2};

use crate::i18n::tr;
use crate::theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TrackIcon {
    Waveform,
    Vocals,
    Choir,
    Speech,
    Drums,
    Bass,
    Sub,
    Guitar,
    Electric,
    Piano,
    Synth,
    Pad,
    Bells,
    Instrument,
    Loop,
    Fx,
    Bus,
    Folder,
    Click,
    Cue,
}

impl TrackIcon {
    pub const ALL: [Self; 20] = [
        Self::Waveform,
        Self::Vocals,
        Self::Choir,
        Self::Speech,
        Self::Drums,
        Self::Bass,
        Self::Sub,
        Self::Guitar,
        Self::Electric,
        Self::Piano,
        Self::Synth,
        Self::Pad,
        Self::Bells,
        Self::Instrument,
        Self::Loop,
        Self::Fx,
        Self::Bus,
        Self::Folder,
        Self::Click,
        Self::Cue,
    ];

    /// Its name in project files.
    pub fn id(self) -> &'static str {
        match self {
            Self::Waveform => "waveform",
            Self::Vocals => "vocals",
            Self::Choir => "choir",
            Self::Speech => "speech",
            Self::Drums => "drums",
            Self::Bass => "bass",
            Self::Sub => "sub",
            Self::Guitar => "guitar",
            Self::Electric => "electric",
            Self::Piano => "piano",
            Self::Synth => "synth",
            Self::Pad => "pad",
            Self::Bells => "bells",
            Self::Instrument => "instrument",
            Self::Loop => "loop",
            Self::Fx => "fx",
            Self::Bus => "bus",
            Self::Folder => "folder",
            Self::Click => "click",
            Self::Cue => "cue",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|i| i.id() == id)
    }

    /// Its Lucide glyph.
    pub fn glyph(self) -> char {
        match self {
            Self::Waveform => '\u{e55b}',
            Self::Vocals => '\u{e349}',
            Self::Choir => '\u{e1a4}',
            Self::Speech => '\u{e118}',
            Self::Drums => '\u{e55d}',
            Self::Bass => '\u{e77a}',
            Self::Sub => '\u{e166}',
            Self::Guitar => '\u{e55f}',
            Self::Electric => '\u{e7ad}',
            Self::Piano => '\u{e561}',
            Self::Synth => '\u{e560}',
            Self::Pad => '\u{e283}',
            Self::Bells => '\u{e059}',
            Self::Instrument => '\u{e122}',
            Self::Loop => '\u{e494}',
            Self::Fx => '\u{e412}',
            Self::Bus => '\u{e162}',
            Self::Folder => '\u{e0d7}',
            Self::Click => '\u{e6bc}',
            Self::Cue => '\u{e0f1}',
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Waveform => tr("Audio"),
            Self::Vocals => tr("Vocals"),
            Self::Choir => tr("Backing vocals"),
            Self::Speech => tr("Speech"),
            Self::Drums => tr("Drums and percussion"),
            Self::Bass => tr("Bass guitar"),
            Self::Sub => tr("Sub and 808"),
            Self::Guitar => tr("Acoustic guitar"),
            Self::Electric => tr("Electric guitar"),
            Self::Piano => tr("Piano and keys"),
            Self::Synth => tr("Synth"),
            Self::Pad => tr("Pad and texture"),
            Self::Bells => tr("Bells and mallets"),
            Self::Instrument => tr("Strings, brass and winds"),
            Self::Loop => tr("Loop and sample"),
            Self::Fx => tr("Effects and ambience"),
            Self::Bus => tr("Bus"),
            Self::Folder => tr("Folder"),
            Self::Click => tr("Click"),
            Self::Cue => tr("Cue mix"),
        }
    }
}

/// Words of track names, by the icon they call for, in the order they are tried: the first
/// that matches wins ("bass drum" is a drum, "lead vox" a voice).
const WORDS: &[(TrackIcon, &[&str])] = &[
    (TrackIcon::Click, &["click", "clic", "metronome", "metro"]),
    (
        TrackIcon::Cue,
        &["cue", "cues", "phones", "headphones", "casque"],
    ),
    (
        TrackIcon::Speech,
        &[
            "speech",
            "talk",
            "dialog",
            "dialogue",
            "dialogues",
            "podcast",
            "interview",
            "host",
            "guest",
            "narration",
            "narrator",
            "vo",
            "voiceover",
        ],
    ),
    (
        TrackIcon::Choir,
        &[
            "bv",
            "bvs",
            "bgv",
            "bgvs",
            "backing",
            "backings",
            "backs",
            "choir",
            "choirs",
            "choeur",
            "choeurs",
            "harmony",
            "harmonies",
            "harmo",
            "harmos",
        ],
    ),
    (
        TrackIcon::Vocals,
        &[
            "vox",
            "vocal",
            "vocals",
            "voc",
            "vocs",
            "voice",
            "voices",
            "voix",
            "chant",
            "chanteur",
            "chanteuse",
            "singer",
            "leadvox",
            "ldvox",
            "lv",
            "adlib",
            "adlibs",
        ],
    ),
    (
        TrackIcon::Drums,
        &[
            "kick",
            "kik",
            "kck",
            "bd",
            "bassdrum",
            "snare",
            "snr",
            "sn",
            "sd",
            "tom",
            "toms",
            "floor",
            "oh",
            "ohl",
            "ohr",
            "overhead",
            "overheads",
            "hh",
            "hat",
            "hats",
            "hihat",
            "ride",
            "crash",
            "china",
            "splash",
            "cymbal",
            "cymbals",
            "cym",
            "cymbale",
            "cymbales",
            "room",
            "rooms",
            "drum",
            "drums",
            "drumkit",
            "kit",
            "batterie",
            "perc",
            "percs",
            "percussion",
            "percussions",
            "conga",
            "congas",
            "bongo",
            "bongos",
            "shaker",
            "tamb",
            "tambourine",
            "tambourin",
            "cajon",
            "timba",
            "timbale",
            "timbales",
            "clap",
            "claps",
            "rim",
            "caisse",
            "charley",
            "breakbeat",
        ],
    ),
    (TrackIcon::Sub, &["sub", "subs", "808", "subbass"]),
    (
        TrackIcon::Bass,
        &[
            "bass",
            "basse",
            "basses",
            "bassgtr",
            "contrebasse",
            "doublebass",
            "bassline",
        ],
    ),
    (
        TrackIcon::Electric,
        &[
            "eg",
            "egtr",
            "elec",
            "electric",
            "electrique",
            "dist",
            "distortion",
            "crunch",
            "amp",
            "lp",
            "strat",
            "tele",
            "riff",
            "riffs",
        ],
    ),
    (
        TrackIcon::Guitar,
        &[
            "gtr",
            "gtrs",
            "guitar",
            "guitars",
            "guit",
            "guitare",
            "guitares",
            "acou",
            "acoustic",
            "acgtr",
            "ag",
            "folk",
            "nylon",
            "classical",
            "ukulele",
            "uke",
            "banjo",
            "mandolin",
            "mandoline",
        ],
    ),
    (
        TrackIcon::Piano,
        &[
            "piano",
            "pianos",
            "pno",
            "grand",
            "keys",
            "rhodes",
            "wurli",
            "wurlitzer",
            "ep",
            "epiano",
            "organ",
            "organs",
            "orgue",
            "hammond",
            "b3",
            "clav",
            "clavinet",
            "clavier",
            "mellotron",
            "harpsichord",
            "clavecin",
        ],
    ),
    (
        TrackIcon::Synth,
        &[
            "synth", "synths", "synthe", "synthes", "moog", "juno", "prophet", "lead", "leads",
            "arp", "arps", "arpeggio", "pluck", "plucks", "seq", "sequence",
        ],
    ),
    (
        TrackIcon::Pad,
        &[
            "pad", "pads", "nappe", "nappes", "drone", "drones", "texture", "textures", "ambient",
            "atmo", "atmos",
        ],
    ),
    (
        TrackIcon::Bells,
        &[
            "bell",
            "bells",
            "cloche",
            "cloches",
            "glock",
            "glockenspiel",
            "vibes",
            "vibraphone",
            "marimba",
            "xylo",
            "xylophone",
            "chimes",
            "celesta",
        ],
    ),
    (
        TrackIcon::Instrument,
        &[
            "strings",
            "string",
            "str",
            "violin",
            "violins",
            "violon",
            "violons",
            "viola",
            "cello",
            "cellos",
            "celli",
            "violoncelle",
            "orch",
            "orchestra",
            "orchestre",
            "cordes",
            "quartet",
            "brass",
            "horn",
            "horns",
            "trumpet",
            "trumpets",
            "tpt",
            "trombone",
            "tbn",
            "sax",
            "saxes",
            "saxophone",
            "tenor",
            "flute",
            "flutes",
            "clarinet",
            "clarinette",
            "cuivres",
            "trompette",
            "trompettes",
            "winds",
            "woodwinds",
            "harp",
            "harpe",
            "accordion",
            "accordeon",
        ],
    ),
    (
        TrackIcon::Loop,
        &[
            "loop", "loops", "sample", "samples", "smp", "break", "breaks", "boucle",
        ],
    ),
    (
        TrackIcon::Fx,
        &[
            "fx", "sfx", "verb", "reverb", "rev", "delay", "dly", "echo", "amb", "ambience",
            "ambiance", "riser", "risers", "impact", "impacts", "whoosh", "noise", "plate",
        ],
    ),
];

/// The words of a name: lower case, accents dropped, split at anything else than a letter or
/// a digit.
fn words(name: &str) -> Vec<String> {
    let plain: String = name
        .to_lowercase()
        .chars()
        .map(|c| match c {
            'à' | 'â' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'î' | 'ï' => 'i',
            'ô' | 'ö' => 'o',
            'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            c if c.is_alphanumeric() => c,
            _ => ' ',
        })
        .collect();
    plain.split_whitespace().map(str::to_owned).collect()
}

/// The icon a name calls for, if any.
pub fn from_name(name: &str) -> Option<TrackIcon> {
    let words = words(name);
    // "T1", "T2": toms of a drum sheet; "Gtr2", "Vox3": numbered takes.
    let stems: Vec<&str> = words
        .iter()
        .map(|w| w.trim_end_matches(|c: char| c.is_ascii_digit()))
        .filter(|w| !w.is_empty())
        .collect();
    if words
        .iter()
        .any(|w| w.len() > 1 && w.starts_with('t') && w[1..].chars().all(|c| c.is_ascii_digit()))
    {
        return Some(TrackIcon::Drums);
    }
    let has = |list: &[&str]| {
        words.iter().any(|w| list.contains(&w.as_str())) || stems.iter().any(|w| list.contains(w))
    };
    WORDS
        .iter()
        .find(|(_, list)| has(list))
        .map(|&(icon, _)| icon)
}

/// The icon found for a track: its name, else what leads it, else its folder's, else what it
/// is (a folder, a bus, a VCA).
pub fn detect(project: &Project, track: &Track) -> TrackIcon {
    detect_at(project, track, 0)
}

fn detect_at(project: &Project, track: &Track, depth: usize) -> TrackIcon {
    let amp = track.inserts.iter().any(|i| i.effect == "amp_rig");
    if let Some(icon) = from_name(&track.name) {
        // A guitar through an amp is an electric one.
        return if icon == TrackIcon::Guitar && amp {
            TrackIcon::Electric
        } else {
            icon
        };
    }
    if track.kind == TrackKind::Vca {
        return TrackIcon::Bus;
    }
    if track.inserts.first().is_some_and(|i| i.effect == "drums") {
        return TrackIcon::Drums;
    }
    if track
        .inserts
        .first()
        .and_then(|i| audiostrada_dsp::info(&i.effect))
        .is_some_and(|i| i.instrument)
        || track.clips.iter().any(|c| c.notes.is_some())
    {
        return TrackIcon::Synth;
    }
    if amp {
        return TrackIcon::Electric;
    }
    if depth < 4
        && let Some(parent) = track.parent.and_then(|p| project.track(p))
    {
        let icon = detect_at(project, parent, depth + 1);
        if !matches!(
            icon,
            TrackIcon::Folder | TrackIcon::Bus | TrackIcon::Waveform
        ) {
            return icon;
        }
    }
    if audiostrada_core::routing::has_children(project, track.id) {
        return TrackIcon::Folder;
    }
    if track.clips.is_empty()
        && project
            .tracks
            .iter()
            .any(|t| t.sends.iter().any(|s| s.target == track.id))
    {
        return TrackIcon::Bus;
    }
    TrackIcon::Waveform
}

/// A track's icon: the one chosen, else the one found.
pub fn of(project: &Project, track: &Track) -> TrackIcon {
    track
        .icon
        .as_deref()
        .and_then(TrackIcon::from_id)
        .unwrap_or_else(|| detect(project, track))
}

/// Paints an icon centered in `rect`.
pub fn paint(ui: &Ui, rect: Rect, icon: TrackIcon, color: Color32) {
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        icon.glyph(),
        FontId::proportional(rect.height().min(rect.width()) * 0.9),
        color,
    );
}

/// The icons to pick from, the automatic one first; true once one is picked.
pub fn picker(
    ui: &mut Ui,
    doc: &mut crate::document::Document,
    id: audiostrada_core::TrackId,
) -> bool {
    let project = doc.project();
    let Some(track) = project.track(id) else {
        return false;
    };
    let found = detect(project, track);
    let chosen = track.icon.as_deref().and_then(TrackIcon::from_id);
    let p = theme::current(ui);
    let mut picked = None;
    ui.set_width(6.0 * 34.0);
    let auto = ui.selectable_label(
        chosen.is_none(),
        format!(
            "{}  {}",
            found.glyph(),
            crate::trf!("Automatic: {name}", name = found.label())
        ),
    );
    if auto.clicked() {
        picked = Some(None);
    }
    ui.add_space(4.0);
    egui::Grid::new(ui.scope_id().with("icons"))
        .spacing(vec2(4.0, 4.0))
        .show(ui, |ui| {
            for (k, icon) in TrackIcon::ALL.into_iter().enumerate() {
                let (rect, response) = ui.allocate_exact_size(vec2(30.0, 30.0), Sense::click());
                let on = chosen == Some(icon);
                let fill = if on {
                    p.accent.gamma_multiply(0.5)
                } else if response.hovered() {
                    p.hover
                } else {
                    p.surface
                };
                ui.painter().rect_filled(rect, 5.0, fill);
                paint(ui, rect.shrink(6.0), icon, p.text);
                if response.on_hover_text(icon.label()).clicked() {
                    picked = Some(Some(icon));
                }
                if k % 6 == 5 {
                    ui.end_row();
                }
            }
        });
    let Some(icon) = picked else {
        return false;
    };
    doc.history_mut().edit(tr("Track icon"), |p| {
        if let Some(t) = p.track_mut(id) {
            t.icon = icon.map(|i| i.id().to_owned());
        }
    });
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_of_track_sheets_find_their_instrument() {
        for (name, icon) in [
            ("01-KICK-consolidated", TrackIcon::Drums),
            ("04-SN DO-consolidated", TrackIcon::Drums),
            ("05-T3 -consolidated", TrackIcon::Drums),
            ("07-OH L-consolidated", TrackIcon::Drums),
            ("10-TIMBA", TrackIcon::Drums),
            ("Bass drum", TrackIcon::Drums),
            ("BASS DI", TrackIcon::Bass),
            ("Lead Vox", TrackIcon::Vocals),
            ("Chant principal", TrackIcon::Vocals),
            ("Choeurs 2", TrackIcon::Choir),
            ("BV 1", TrackIcon::Choir),
            ("Gtr L crunch", TrackIcon::Electric),
            ("Guitare acoustique", TrackIcon::Guitar),
            ("Rhodes", TrackIcon::Piano),
            ("Synth lead", TrackIcon::Synth),
            ("Lead", TrackIcon::Synth),
            ("Nappe", TrackIcon::Pad),
            ("Violons", TrackIcon::Instrument),
            ("Reverb plate", TrackIcon::Fx),
            ("808", TrackIcon::Sub),
            ("Click", TrackIcon::Click),
            ("Vox3", TrackIcon::Vocals),
        ] {
            assert_eq!(from_name(name), Some(icon), "{name}");
        }
        assert_eq!(from_name("Audio 1"), None);
        assert_eq!(from_name("AEA"), None);
    }

    #[test]
    fn a_track_without_words_takes_its_folder_or_what_it_is() {
        let mut project = Project::new("Test", 48_000);
        let drums = project.add_track("DRUMS");
        let room = project.add_track("AEA");
        let other = project.add_track("Audio 1");
        audiostrada_core::routing::set_parent(&mut project, room, Some(drums)).unwrap();
        let t = |id| project.track(id).unwrap();
        assert_eq!(detect(&project, t(room)), TrackIcon::Drums);
        assert_eq!(detect(&project, t(other)), TrackIcon::Waveform);
        let mut project = project.clone();
        let folder = project.add_track("Group");
        audiostrada_core::routing::set_parent(&mut project, other, Some(folder)).unwrap();
        assert_eq!(
            detect(&project, project.track(folder).unwrap()),
            TrackIcon::Folder
        );
    }

    #[test]
    fn ids_round_trip_and_glyphs_are_in_the_font() {
        let font = include_bytes!("../assets/fonts/Lucide-TrackIcons.ttf");
        for icon in TrackIcon::ALL {
            assert_eq!(TrackIcon::from_id(icon.id()), Some(icon));
            assert!(!icon.label().is_empty());
            assert!(font.len() > 1000);
        }
    }
}
