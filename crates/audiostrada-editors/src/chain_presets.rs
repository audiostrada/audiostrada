//! Strada-Master's and Strada-Vocals' starting points: styles, voices and spaces made of the
//! stages' own factory presets, and the chains' presets made of those.

use std::collections::BTreeMap;

use crate::i18n::tr;
use crate::presets::{Preset, factory};

/// Every parameter of `stage` under its prefix: its defaults, then its factory preset `name`
/// (in English; none when empty).
pub fn stage(stage: &str, name: &'static str) -> Vec<(String, f32)> {
    let Some(info) = audiostrada_dsp::info(stage) else {
        return Vec::new();
    };
    let mut values: BTreeMap<String, f32> = info
        .params
        .iter()
        .map(|p| (p.id.to_owned(), p.default))
        .collect();
    if !name.is_empty() {
        let found = factory(stage).into_iter().find(|p| p.name == tr(name));
        debug_assert!(found.is_some(), "{stage} has no preset {name}");
        if let Some(preset) = found {
            values.extend(preset.params);
        }
    }
    values
        .into_iter()
        .map(|(k, v)| (format!("{stage}.{k}"), v))
        .collect()
}

/// Every parameter of `stage` under its prefix: its defaults, then `preset`'s values.
pub fn stage_from(stage: &str, preset: &Preset) -> Vec<(String, f32)> {
    let Some(info) = audiostrada_dsp::info(stage) else {
        return Vec::new();
    };
    let mut values: BTreeMap<String, f32> = info
        .params
        .iter()
        .map(|p| (p.id.to_owned(), p.default))
        .collect();
    values.extend(preset.params.iter().map(|(k, v)| (k.clone(), *v)));
    values
        .into_iter()
        .map(|(k, v)| (format!("{stage}.{k}"), v))
        .collect()
}

/// Strada-Master's styles: the EQ, MB-Comp, Tape, Wide and Max each one starts from.
const STYLES: [[(&str, &str); 5]; 4] = [
    [
        ("eq", ""),
        ("mb_comp", "Master: Gentle glue"),
        ("tape", "Mastering: Transparent 30 ips"),
        ("imager", "Master: Mono bass 150 Hz"),
        ("maximizer", "Master: Transparent -1 dBTP"),
    ],
    [
        ("eq", "Master: Tilt warmer"),
        ("mb_comp", "Master: Gentle glue"),
        ("tape", "Mastering: Round 15 ips"),
        ("imager", "Master: Mono bass 150 Hz"),
        ("maximizer", "Master: Acoustic, gentle"),
    ],
    [
        ("eq", "Master: Tilt brighter"),
        ("mb_comp", "Master: Gentle glue"),
        ("tape", "Mastering: Analog sheen"),
        ("imager", "Master: Wide top"),
        ("maximizer", "Master: Streaming -1 dBTP"),
    ],
    [
        ("eq", "Master: Clear the low mids"),
        ("mb_comp", "Master: Tight low end"),
        ("tape", "Mix bus: Glue 30 ips"),
        ("imager", "Master: Mono bass 150 Hz"),
        ("maximizer", "Loud: Punchy pop"),
    ],
];

/// Each loudness target's safety limiter and ceiling (dB), and what else it sets.
type Target = (&'static str, f32, &'static [(&'static str, f32)]);

const TARGETS: [Target; 7] = [
    ("Master: Streaming -1 dBTP", -1.0, &[]),
    ("Master: Streaming -1 dBTP", -1.0, &[]),
    ("Master: Loud -0.3 dB", -0.3, &[]),
    ("Master: Loud -0.3 dB", -0.3, &[]),
    // A cutting lathe wants the lows in mono and some room under the peaks.
    (
        "Master: Safety -1 dBTP",
        -1.5,
        &[("imager.mono_bass", 150.0)],
    ),
    ("Master: Broadcast -2 dBTP", -2.0, &[]),
    // For a mastering engineer: nothing maximized, the peaks held at -6 dBFS.
    (
        "Master: Safety -1 dBTP",
        -6.0,
        &[
            ("limiter.ceiling", -6.0),
            ("loudness", 0.0),
            ("follow", 0.0),
        ],
    ),
];

/// Strada-Vocals' voices: the tuning, EQ, compressor, de-esser and doubler each starts from,
/// and the low cut under them (Hz).
const VOICES: [([(&str, &str); 5], f32); 3] = [
    (
        [
            ("pitch_correct", "Vocal: Natural, keeps the vibrato"),
            ("eq", "Vocal: Presence"),
            ("compressor", "Vocal: Smooth opto"),
            ("deesser", "Vocal: Wide-band, natural"),
            ("doubler", "Vocal: Natural double"),
        ],
        90.0,
    ),
    (
        [
            ("pitch_correct", "Vocal: Pop, tight"),
            ("eq", "Vocal: Less mud"),
            ("compressor", "Vocal: Rap, upfront"),
            ("deesser", "Vocal: Male, standard"),
            ("doubler", "Vocal: Tight widen"),
        ],
        80.0,
    ),
    (
        [
            ("pitch_correct", "Vocal: Light touch"),
            ("eq", "Vocal: Voice-over, podcast"),
            ("compressor", "Vocal: Voice-over, steady"),
            ("deesser", "Voice-over: Podcast, close mic"),
            ("doubler", "Vocal: Tight widen"),
        ],
        70.0,
    ),
];

/// Strada-Vocals' spaces: the delay and the reverb each starts from.
const ROOMS: [[(&str, &str); 2]; 5] = [
    [
        ("delay", "Vocal: Slapback 90 ms"),
        ("reverb", "Vocal: Close ambience"),
    ],
    [
        ("delay", "Vocal: Dark 1/8 echo"),
        ("reverb", "Vocal: Plate"),
    ],
    [
        ("delay", "Vocal: 1/4 throw, ducked"),
        ("reverb", "Vocal: Ducked hall"),
    ],
    [
        ("delay", "Vocal: Analog slap 110 ms"),
        ("reverb", "Vocal: Close ambience"),
    ],
    [
        ("delay", "Vocal: 1/4 throw, ducked"),
        ("reverb", "Vocal: Plate, 1/16 pre-delay"),
    ],
];

fn stages(list: &[(&str, &'static str)]) -> Vec<(String, f32)> {
    list.iter().flat_map(|&(s, name)| stage(s, name)).collect()
}

/// The EQ's last bands, the knobs' own (Tone's shelves, Master's sub), which the stages' EQ
/// presets leave alone: bands 13 to 15 for Tone, 16 for the sub.
fn knob_bands(master: bool) -> Vec<(String, f32)> {
    let (low, high) = if master {
        (200.0, 4000.0)
    } else {
        (250.0, 3500.0)
    };
    let mut bands = vec![
        (13, 2.0, 12_000.0, 0.7),
        (14, 1.0, low, 0.6),
        (15, 2.0, high, 0.6),
    ];
    if master {
        bands.push((16, 1.0, 70.0, 0.7));
    }
    bands
        .into_iter()
        .flat_map(|(n, kind, freq, q)| {
            [
                ("used", 1.0),
                ("type", kind),
                ("freq", freq),
                ("gain", 0.0),
                ("q", q),
            ]
            .map(|(field, v)| (format!("eq.b{n}_{field}"), v))
        })
        .collect()
}

/// Strada-Master compresses gently and never lifts the quiet parts: the MB-Comp presets' lift
/// goes, their thresholds come up a little.
fn gentler(settings: &mut [(String, f32)]) {
    for (id, v) in settings.iter_mut() {
        if let Some(field) = id.strip_prefix("mb_comp.b") {
            if field.ends_with("_thresh") {
                *v = (*v + 4.0).min(0.0);
            } else if field.ends_with("_up") {
                *v = 1.0;
            }
        }
    }
}

/// The settings of Strada-Master's style `style`.
pub fn master_style(style: usize) -> Vec<(String, f32)> {
    let mut out = stages(&STYLES[style.min(STYLES.len() - 1)]);
    gentler(&mut out);
    out.extend(knob_bands(true));
    // The Loudness knob is the maximizer's gain.
    out.push(("maximizer.gain".into(), 0.0));
    out.push(("style".into(), style as f32));
    out
}

/// The settings of Strada-Master's loudness target `target`.
pub fn master_target(target: usize) -> Vec<(String, f32)> {
    let (limiter, ceiling, more) = TARGETS[target.min(TARGETS.len() - 1)];
    let mut out = stage("limiter", limiter);
    out.push(("maximizer.ceiling".into(), ceiling));
    out.extend(more.iter().map(|&(k, v)| (k.to_owned(), v)));
    out.push(("target".into(), target as f32));
    out
}

/// The settings of Strada-Vocals' voice `voice`.
pub fn vocal_voice(voice: usize) -> Vec<(String, f32)> {
    let (list, low_cut) = VOICES[voice.min(VOICES.len() - 1)];
    let mut out = stages(&list);
    out.extend(knob_bands(false));
    // The last band is the low cut every voice keeps; the presets use the first ones.
    out.extend([
        ("eq.b16_used".into(), 1.0),
        ("eq.b16_type".into(), 3.0),
        ("eq.b16_freq".into(), low_cut),
        ("voice".into(), voice as f32),
    ]);
    out
}

/// The settings of Strada-Vocals' space `room`.
pub fn vocal_room(room: usize) -> Vec<(String, f32)> {
    let mut out = stages(&ROOMS[room.min(ROOMS.len() - 1)]);
    out.push(("room".into(), room as f32));
    out
}

fn assemble(name: &str, parts: Vec<Vec<(String, f32)>>) -> Preset {
    let mut params = BTreeMap::new();
    for part in parts {
        params.extend(part);
    }
    Preset {
        name: name.to_owned(),
        params,
        state: None,
        factory: true,
    }
}

fn own(ids: &[&str], values: &[f32]) -> Vec<(String, f32)> {
    ids.iter()
        .zip(values)
        .map(|(id, v)| ((*id).to_owned(), *v))
        .collect()
}

/// A Strada-Master preset: its style, its target, its six knobs (tone, glue, color, width,
/// sub, loudness), then stage presets put over the style's.
fn master(
    name: &str,
    style: usize,
    target: usize,
    knobs: [f32; 6],
    more: &[(&str, &'static str)],
) -> Preset {
    assemble(
        name,
        vec![
            master_style(style),
            master_target(target),
            {
                let mut more = stages(more);
                gentler(&mut more);
                more
            },
            knob_bands(true),
            vec![("maximizer.gain".into(), 0.0)],
            own(
                &["tone", "glue", "color", "width", "sub", "loudness"],
                &knobs,
            ),
        ],
    )
}

/// A Strada-Vocals preset: its voice, its space, its knobs (tune, tone, saturation, comp,
/// de-ess, double, space), the compressor's makeup that gives the voice back at its loudness, then
/// stage presets put over the voice's and the space's.
fn vocal(
    name: &str,
    voice: usize,
    room: usize,
    knobs: [f32; 7],
    makeup: f32,
    more: &[(&str, &'static str)],
) -> Preset {
    assemble(
        name,
        vec![
            vocal_voice(voice),
            vocal_room(room),
            stages(more),
            knob_bands(false),
            // The voice's low cut, which an EQ preset put over it would take out.
            vocal_voice(voice)
                .into_iter()
                .filter(|(k, _)| k.starts_with("eq.b16_"))
                .collect(),
            vec![("compressor.makeup".into(), makeup)],
            own(
                &["tune", "tone", "drive", "comp", "deess", "double", "space"],
                &knobs,
            ),
        ],
    )
}

const CLEAN: usize = 0;
const WARM: usize = 1;
const OPEN: usize = 2;
const PUNCHY: usize = 3;
const STREAMING: usize = 0;
const CD: usize = 2;
const CLUB: usize = 3;
const BROADCAST: usize = 5;

pub fn master_presets() -> Vec<Preset> {
    vec![
        master(
            tr("Pop: Polished for streaming"),
            OPEN,
            STREAMING,
            [10.0, 55.0, 40.0, 110.0, 0.0, -1.5],
            &[],
        ),
        master(
            tr("Pop: Bright and loud"),
            OPEN,
            CD,
            [25.0, 65.0, 45.0, 115.0, 10.0, 1.5],
            &[],
        ),
        master(
            tr("Pop: Warm ballad"),
            WARM,
            STREAMING,
            [-10.0, 45.0, 55.0, 105.0, 0.0, -0.5],
            &[],
        ),
        master(
            tr("Rock: Punchy"),
            PUNCHY,
            CD,
            [5.0, 60.0, 60.0, 105.0, 10.0, 3.0],
            &[],
        ),
        master(
            tr("Rock: Dense and loud"),
            PUNCHY,
            CD,
            [0.0, 75.0, 70.0, 100.0, 10.0, 3.2],
            &[("maximizer", "Loud: Dense rock")],
        ),
        master(
            tr("Rock: Live and open"),
            OPEN,
            STREAMING,
            [10.0, 45.0, 50.0, 115.0, 0.0, -0.5],
            &[],
        ),
        master(
            tr("Hip-hop: Heavy low end"),
            PUNCHY,
            CD,
            [-15.0, 60.0, 50.0, 100.0, 40.0, 1.7],
            &[],
        ),
        master(
            tr("Hip-hop: Crisp and loud"),
            PUNCHY,
            CD,
            [15.0, 65.0, 45.0, 105.0, 20.0, 1.9],
            &[],
        ),
        master(
            tr("Hip-hop: Lo-fi and warm"),
            WARM,
            STREAMING,
            [-35.0, 55.0, 80.0, 90.0, 10.0, 1.0],
            &[("tape", "Mix bus: Vintage")],
        ),
        master(
            tr("Electronic: Club loud"),
            PUNCHY,
            CLUB,
            [10.0, 70.0, 45.0, 110.0, 30.0, 1.8],
            &[("maximizer", "Loud: EDM maximum")],
        ),
        master(
            tr("Electronic: Wide and bright"),
            OPEN,
            CD,
            [20.0, 55.0, 35.0, 130.0, 15.0, 1.0],
            &[],
        ),
        master(
            tr("Electronic: Deep and warm"),
            WARM,
            STREAMING,
            [-15.0, 50.0, 55.0, 115.0, 25.0, 0.0],
            &[],
        ),
        master(
            tr("Acoustic: Natural"),
            CLEAN,
            STREAMING,
            [0.0, 35.0, 30.0, 100.0, 0.0, -2.0],
            &[],
        ),
        master(
            tr("Acoustic: Folk warmth"),
            WARM,
            STREAMING,
            [-10.0, 40.0, 50.0, 105.0, 0.0, -1.5],
            &[],
        ),
        master(
            tr("Acoustic: Jazz, keeps the dynamics"),
            CLEAN,
            STREAMING,
            [5.0, 20.0, 25.0, 105.0, 0.0, -2.5],
            &[],
        ),
        master(
            tr("Orchestral: Transparent"),
            CLEAN,
            BROADCAST,
            [0.0, 15.0, 15.0, 100.0, 0.0, -9.0],
            &[],
        ),
        master(
            tr("Orchestral: Film, wide"),
            OPEN,
            STREAMING,
            [5.0, 25.0, 20.0, 125.0, 0.0, -2.0],
            &[],
        ),
        master(
            tr("Orchestral: Gentle warmth"),
            WARM,
            STREAMING,
            [-5.0, 25.0, 35.0, 105.0, 0.0, -1.5],
            &[],
        ),
        master(
            tr("Podcast: Clear voice"),
            CLEAN,
            STREAMING,
            [10.0, 60.0, 20.0, 80.0, -30.0, -2.5],
            &[],
        ),
        master(
            tr("Podcast: Broadcast"),
            CLEAN,
            BROADCAST,
            [5.0, 55.0, 15.0, 90.0, -30.0, -10.5],
            &[],
        ),
        master(
            tr("Podcast: Warm audiobook"),
            WARM,
            STREAMING,
            [-5.0, 50.0, 30.0, 85.0, -20.0, -2.5],
            &[],
        ),
        master(
            tr("Lo-fi: Dusty tape"),
            WARM,
            STREAMING,
            [-40.0, 55.0, 95.0, 90.0, -10.0, 3.0],
            &[("tape", "Lo-fi: Worn cassette")],
        ),
        master(
            tr("Lo-fi: Cassette demo"),
            WARM,
            STREAMING,
            [-25.0, 50.0, 90.0, 95.0, 0.0, 2.5],
            &[("tape", "Lo-fi: Cassette demo")],
        ),
        master(
            tr("Lo-fi: Warbly and soft"),
            WARM,
            STREAMING,
            [-30.0, 45.0, 85.0, 100.0, 0.0, 1.5],
            &[("tape", "Lo-fi: Warbly tape")],
        ),
    ]
}

const SINGING: usize = 0;
const RAP: usize = 1;
const SPOKEN: usize = 2;
const ROOM: usize = 0;
const PLATE: usize = 1;
const HALL: usize = 2;
const SLAP: usize = 3;
const ECHO: usize = 4;

pub fn vocal_presets() -> Vec<Preset> {
    vec![
        vocal(
            tr("Pop: Modern lead"),
            SINGING,
            PLATE,
            [45.0, 25.0, 5.0, 60.0, 55.0, 25.0, 50.0],
            10.0,
            &[],
        ),
        vocal(
            tr("Pop: Airy ballad"),
            SINGING,
            HALL,
            [30.0, 35.0, 0.0, 50.0, 55.0, 20.0, 65.0],
            14.5,
            &[],
        ),
        vocal(
            tr("Pop: Tuned and tight"),
            SINGING,
            PLATE,
            [80.0, 20.0, 0.0, 65.0, 55.0, 30.0, 45.0],
            9.5,
            &[("pitch_correct", "Vocal: Pop, tight")],
        ),
        vocal(
            tr("Pop: Backing stack"),
            SINGING,
            PLATE,
            [50.0, 10.0, 0.0, 70.0, 65.0, 70.0, 55.0],
            11.5,
            &[
                ("eq", "Vocal: Backing, sit behind"),
                ("compressor", "Vocal: Backing, tucked in"),
                ("deesser", "Vocal: Backing, strong"),
                ("doubler", "Vocal: Stack of takes"),
            ],
        ),
        vocal(
            tr("R&B: Smooth lead"),
            SINGING,
            PLATE,
            [40.0, 15.0, 0.0, 55.0, 50.0, 30.0, 55.0],
            10.0,
            &[],
        ),
        vocal(
            tr("R&B: Silky doubles"),
            SINGING,
            HALL,
            [40.0, 25.0, 0.0, 55.0, 55.0, 60.0, 60.0],
            13.0,
            &[],
        ),
        vocal(
            tr("R&B: Late night"),
            SINGING,
            HALL,
            [30.0, -15.0, 0.0, 50.0, 45.0, 20.0, 70.0],
            15.0,
            &[],
        ),
        vocal(
            tr("Rap: Upfront"),
            RAP,
            SLAP,
            [0.0, 15.0, 20.0, 70.0, 55.0, 20.0, 30.0],
            14.0,
            &[],
        ),
        vocal(
            tr("Rap: Hard-tuned melodic"),
            RAP,
            ECHO,
            [100.0, 20.0, 15.0, 65.0, 55.0, 30.0, 50.0],
            15.5,
            &[("pitch_correct", "Vocal: Hard tune")],
        ),
        vocal(
            tr("Rap: Ad-libs, wide"),
            RAP,
            ECHO,
            [0.0, 30.0, 0.0, 60.0, 55.0, 80.0, 70.0],
            16.0,
            &[("eq", "Vocal: Backing, sit behind")],
        ),
        vocal(
            tr("Rock: Gritty lead"),
            SINGING,
            SLAP,
            [20.0, 20.0, 45.0, 75.0, 50.0, 20.0, 40.0],
            11.5,
            &[("compressor", "Vocal: Radio-ready")],
        ),
        vocal(
            tr("Rock: Big chorus"),
            SINGING,
            HALL,
            [25.0, 15.0, 20.0, 65.0, 55.0, 60.0, 60.0],
            12.5,
            &[],
        ),
        vocal(
            tr("Rock: Raw and close"),
            SINGING,
            ROOM,
            [0.0, 5.0, 0.0, 55.0, 45.0, 0.0, 30.0],
            8.5,
            &[],
        ),
        vocal(
            tr("Singer-songwriter: Natural"),
            SINGING,
            ROOM,
            [20.0, 5.0, 0.0, 40.0, 45.0, 0.0, 40.0],
            10.0,
            &[],
        ),
        vocal(
            tr("Singer-songwriter: Intimate"),
            SINGING,
            PLATE,
            [15.0, -5.0, 0.0, 45.0, 50.0, 0.0, 35.0],
            9.0,
            &[("reverb", "Vocal: Close ambience")],
        ),
        vocal(
            tr("Singer-songwriter: Warm room"),
            SINGING,
            ROOM,
            [15.0, -15.0, 0.0, 45.0, 45.0, 10.0, 50.0],
            10.0,
            &[],
        ),
        vocal(
            tr("Podcast: Clear voice"),
            SPOKEN,
            ROOM,
            [0.0, 15.0, 0.0, 60.0, 55.0, 0.0, 0.0],
            8.5,
            &[],
        ),
        vocal(
            tr("Podcast: Radio voice"),
            SPOKEN,
            ROOM,
            [0.0, 25.0, 0.0, 75.0, 60.0, 0.0, 0.0],
            8.5,
            &[
                ("compressor", "Vocal: Radio-ready"),
                ("deesser", "Voice-over: Broadcast"),
            ],
        ),
        vocal(
            tr("Podcast: Audiobook"),
            SPOKEN,
            ROOM,
            [0.0, 0.0, 0.0, 50.0, 45.0, 0.0, 0.0],
            8.0,
            &[("deesser", "Voice-over: Audiobook, gentle")],
        ),
        vocal(
            tr("FX: Telephone"),
            SINGING,
            SLAP,
            [0.0, 0.0, 30.0, 60.0, 30.0, 0.0, 30.0],
            6.0,
            &[("eq", "FX: Telephone")],
        ),
        vocal(
            tr("FX: Old radio"),
            SINGING,
            ROOM,
            [0.0, -10.0, 50.0, 65.0, 30.0, 0.0, 35.0],
            6.5,
            &[("eq", "FX: Old radio")],
        ),
        vocal(
            tr("FX: Robot"),
            SINGING,
            PLATE,
            [100.0, 10.0, 25.0, 60.0, 50.0, 40.0, 40.0],
            9.0,
            &[
                ("pitch_correct", "Vocal: Hard tune"),
                ("doubler", "FX: Out-of-tune choir"),
            ],
        ),
        vocal(
            tr("FX: Huge echo"),
            SINGING,
            ECHO,
            [30.0, 15.0, 0.0, 55.0, 55.0, 30.0, 100.0],
            18.5,
            &[("reverb", "Vocal: Ducked hall")],
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_style_voice_and_space_is_made_of_existing_presets() {
        for i in 0..STYLES.len() {
            assert!(!master_style(i).is_empty());
        }
        for i in 0..TARGETS.len() {
            assert!(!master_target(i).is_empty());
        }
        for i in 0..VOICES.len() {
            assert!(!vocal_voice(i).is_empty());
        }
        for i in 0..ROOMS.len() {
            assert!(!vocal_room(i).is_empty());
        }
        master_presets();
        vocal_presets();
    }

    #[test]
    fn the_chain_presets_only_set_parameters_of_their_chain() {
        for (info, list) in [
            (&*audiostrada_dsp::chain::MASTER, master_presets()),
            (&*audiostrada_dsp::chain::VOCALS, vocal_presets()),
        ] {
            for preset in list {
                for id in preset.params.keys() {
                    assert!(
                        info.params.iter().any(|p| p.id == id),
                        "{}: {} sets {id}",
                        info.id,
                        preset.name
                    );
                }
            }
        }
    }
}
