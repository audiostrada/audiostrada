//! Track icons: an instrument for each track, chosen or found from its name (English and
//! French words and abbreviations of studio track sheets), its folder and what it plays.
//! Glyphs from Lucide (ISC) and our own in its style, in assets/fonts/Lucide-TrackIcons.ttf
//! (see assets/icons).

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
    Kick,
    Snare,
    RackTom,
    FloorTom,
    Cymbals,
    HiHat,
    DrumMachine,
    Bass,
    Sub,
    Guitar,
    Electric,
    Piano,
    Synth,
    Pad,
    Bells,
    Strings,
    Brass,
    Winds,
    Instrument,
    Loop,
    Fx,
    Bus,
    Folder,
    Click,
    Cue,
}

impl TrackIcon {
    pub const ALL: [Self; 30] = [
        Self::Waveform,
        Self::Vocals,
        Self::Choir,
        Self::Speech,
        Self::Drums,
        Self::Kick,
        Self::Snare,
        Self::RackTom,
        Self::FloorTom,
        Self::Cymbals,
        Self::HiHat,
        Self::DrumMachine,
        Self::Bass,
        Self::Sub,
        Self::Guitar,
        Self::Electric,
        Self::Piano,
        Self::Synth,
        Self::Pad,
        Self::Bells,
        Self::Strings,
        Self::Brass,
        Self::Winds,
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
            Self::Kick => "kick",
            Self::Snare => "snare",
            Self::RackTom => "rack_tom",
            Self::FloorTom => "floor_tom",
            Self::Cymbals => "cymbals",
            Self::HiHat => "hihat",
            Self::DrumMachine => "drum_machine",
            Self::Bass => "bass",
            Self::Sub => "sub",
            Self::Guitar => "guitar",
            Self::Electric => "electric",
            Self::Piano => "piano",
            Self::Synth => "synth",
            Self::Pad => "pad",
            Self::Bells => "bells",
            Self::Strings => "strings",
            Self::Brass => "brass",
            Self::Winds => "winds",
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
            Self::Vocals => '\u{e904}',
            Self::Choir => '\u{e905}',
            Self::Speech => '\u{e118}',
            Self::Drums => '\u{e907}',
            Self::Kick => '\u{e908}',
            Self::Snare => '\u{e55d}',
            Self::RackTom => '\u{e909}',
            Self::FloorTom => '\u{e90a}',
            Self::Cymbals => '\u{e90b}',
            Self::HiHat => '\u{e90c}',
            Self::DrumMachine => '\u{e906}',
            Self::Bass => '\u{e903}',
            Self::Sub => '\u{e166}',
            Self::Guitar => '\u{e55f}',
            Self::Electric => '\u{e7ad}',
            Self::Piano => '\u{e561}',
            Self::Synth => '\u{e560}',
            Self::Pad => '\u{e283}',
            Self::Bells => '\u{e059}',
            Self::Strings => '\u{e900}',
            Self::Brass => '\u{e901}',
            Self::Winds => '\u{e902}',
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
            Self::Drums => tr("Drum kit"),
            Self::Kick => tr("Kick"),
            Self::Snare => tr("Snare and percussion"),
            Self::RackTom => tr("Rack tom"),
            Self::FloorTom => tr("Floor tom"),
            Self::Cymbals => tr("Overheads and cymbals"),
            Self::HiHat => tr("Hi-hat"),
            Self::DrumMachine => tr("Drum machine"),
            Self::Bass => tr("Bass guitar"),
            Self::Sub => tr("Sub and 808"),
            Self::Guitar => tr("Acoustic guitar"),
            Self::Electric => tr("Electric guitar"),
            Self::Piano => tr("Piano and keys"),
            Self::Synth => tr("Synth"),
            Self::Pad => tr("Pad and texture"),
            Self::Bells => tr("Bells and mallets"),
            Self::Strings => tr("Strings"),
            Self::Brass => tr("Brass"),
            Self::Winds => tr("Saxophone and winds"),
            Self::Instrument => tr("Other instrument"),
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
        TrackIcon::HiHat,
        &[
            "hh",
            "hat",
            "hats",
            "hihat",
            "hihats",
            "charley",
            "charleston",
            "chh",
            "ohh",
        ],
    ),
    (
        TrackIcon::Cymbals,
        &[
            "oh",
            "ohl",
            "ohr",
            "overhead",
            "overheads",
            "ride",
            "crash",
            "china",
            "splash",
            "cymbal",
            "cymbals",
            "cym",
            "cymbale",
            "cymbales",
        ],
    ),
    (
        TrackIcon::Kick,
        &["kick", "kik", "kck", "bd", "bassdrum", "kickin", "kickout"],
    ),
    (
        TrackIcon::Snare,
        &[
            "snare",
            "snr",
            "sn",
            "sd",
            "rim",
            "caisse",
            "claire",
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
        ],
    ),
    (TrackIcon::FloorTom, &["floor", "floortom", "ft"]),
    (TrackIcon::RackTom, &["tom", "toms", "rack", "racktom"]),
    (
        TrackIcon::DrumMachine,
        &[
            "drummachine",
            "machine",
            "boite",
            "909",
            "707",
            "606",
            "linndrum",
            "beat",
            "beats",
        ],
    ),
    (
        TrackIcon::Drums,
        &[
            "room",
            "rooms",
            "drum",
            "drums",
            "drumkit",
            "kit",
            "batterie",
            "breakbeat",
            "drumbus",
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
        TrackIcon::Strings,
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
            "quatuor",
        ],
    ),
    (
        TrackIcon::Brass,
        &[
            "brass",
            "horn",
            "horns",
            "trumpet",
            "trumpets",
            "tpt",
            "trombone",
            "trombones",
            "tbn",
            "tuba",
            "cuivres",
            "trompette",
            "trompettes",
            "cor",
            "bugle",
        ],
    ),
    (
        TrackIcon::Winds,
        &[
            "sax",
            "saxes",
            "saxophone",
            "saxophones",
            "tenor",
            "flute",
            "flutes",
            "clarinet",
            "clarinette",
            "winds",
            "woodwinds",
            "bois",
            "oboe",
            "hautbois",
            "bassoon",
            "basson",
        ],
    ),
    (
        TrackIcon::Instrument,
        &[
            "harp",
            "harpe",
            "accordion",
            "accordeon",
            "harmonica",
            "kalimba",
            "sitar",
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
    let has = |list: &[&str]| {
        words.iter().any(|w| list.contains(&w.as_str())) || stems.iter().any(|w| list.contains(w))
    };
    // "Tom basse", "floor tom"; "grosse caisse".
    if has(&["tom", "toms"]) && has(&["basse", "bass", "floor"]) {
        return Some(TrackIcon::FloorTom);
    }
    if has(&["grosse"]) || (has(&["bass"]) && has(&["drum"])) {
        return Some(TrackIcon::Kick);
    }
    if words
        .iter()
        .any(|w| w.len() > 1 && w.starts_with('t') && w[1..].chars().all(|c| c.is_ascii_digit()))
    {
        return Some(TrackIcon::RackTom);
    }
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
    match track.kind {
        TrackKind::Vca => return TrackIcon::Bus,
        TrackKind::Bus => return TrackIcon::Folder,
        TrackKind::Audio => {}
    }
    if track.inserts.first().is_some_and(|i| i.effect == "drums") {
        return TrackIcon::DrumMachine;
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

/// A track's icon: the one chosen, else the instrument it was prepared as (the name's icon
/// when it tells the same more precisely), else the one found.
pub fn of(project: &Project, track: &Track) -> TrackIcon {
    if let Some(icon) = track.icon.as_deref().and_then(TrackIcon::from_id) {
        return icon;
    }
    let found = detect(project, track);
    match track
        .prepared
        .as_ref()
        .and_then(|p| prepared_as(&p.instrument))
    {
        Some(icon) if !same(icon, found) => icon,
        _ => found,
    }
}

/// The icon of an instrument a track was prepared as.
fn prepared_as(id: &str) -> Option<TrackIcon> {
    use audiostrada_dsp::prepare::Instrument::*;
    if audiostrada_core::prepare::is_space(id) {
        return Some(TrackIcon::Fx);
    }
    Some(match audiostrada_dsp::prepare::Instrument::from_id(id)? {
        VocalMale | VocalFemale => TrackIcon::Vocals,
        BackingVocals => TrackIcon::Choir,
        Whistling => TrackIcon::Vocals,
        Kick => TrackIcon::Kick,
        Snare => TrackIcon::Snare,
        Toms => TrackIcon::RackTom,
        HiHat => TrackIcon::HiHat,
        Overheads => TrackIcon::Cymbals,
        Drums | HandDrums | Cajon | Claps => TrackIcon::Drums,
        Shaker | Tambourine | Cowbell => TrackIcon::Bells,
        Bass => TrackIcon::Bass,
        ElectricGuitar => TrackIcon::Electric,
        AcousticGuitar => TrackIcon::Guitar,
        Keys => TrackIcon::Piano,
        Synth => TrackIcon::Synth,
        Strings => TrackIcon::Strings,
        Brass => TrackIcon::Brass,
        Other => return None,
    })
}

/// Two icons of the same instrument, one more precise.
fn same(a: TrackIcon, b: TrackIcon) -> bool {
    use TrackIcon::*;
    a == b
        || [
            (RackTom, FloorTom),
            (Bass, Sub),
            (Synth, Pad),
            (Vocals, Speech),
            (Drums, DrumMachine),
            (Brass, Winds),
        ]
        .iter()
        .any(|&(x, y)| (a, b) == (x, y) || (a, b) == (y, x))
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
            ("01-KICK-consolidated", TrackIcon::Kick),
            ("04-SN DO-consolidated", TrackIcon::Snare),
            ("05-T3 -consolidated", TrackIcon::RackTom),
            ("07-OH L-consolidated", TrackIcon::Cymbals),
            ("10-TIMBA", TrackIcon::Snare),
            ("Bass drum", TrackIcon::Kick),
            ("Grosse caisse", TrackIcon::Kick),
            ("Caisse claire", TrackIcon::Snare),
            ("Tom basse", TrackIcon::FloorTom),
            ("Floor tom", TrackIcon::FloorTom),
            ("Tom 1", TrackIcon::RackTom),
            ("HH", TrackIcon::HiHat),
            ("Charley", TrackIcon::HiHat),
            ("Ride", TrackIcon::Cymbals),
            ("Batterie", TrackIcon::Drums),
            ("Room", TrackIcon::Drums),
            ("Boite a rythme", TrackIcon::DrumMachine),
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
            ("Violons", TrackIcon::Strings),
            ("Tpt 1", TrackIcon::Brass),
            ("Sax tenor", TrackIcon::Winds),
            ("Harpe", TrackIcon::Instrument),
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
        let drums = audiostrada_core::routing::add_bus(&mut project, "DRUMS");
        let room = project.add_track("AEA");
        let other = project.add_track("Audio 1");
        audiostrada_core::routing::set_parent(&mut project, room, Some(drums)).unwrap();
        let t = |id| project.track(id).unwrap();
        assert_eq!(detect(&project, t(room)), TrackIcon::Drums);
        assert_eq!(detect(&project, t(other)), TrackIcon::Waveform);
        let mut project = project.clone();
        let folder = audiostrada_core::routing::add_bus(&mut project, "Group");
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
