//! Strada-Synth's ready-made sounds, by role, each with its four macros named after what they
//! do. A sound is written as a few lines of text: its settings, its modulations, its macros.
//!
//! Settings are `id=value` pairs, the ids those of the synth's parameters; a choice is written
//! by a short name (`mode=mono`), and a few pairs set several parameters at once:
//! `osc1=wave:voices:detune`, `osc2=wave:voices:detune:level` (a wavetable's name for the wave
//! chooses the table too), `adsr=` and `env2=attack:decay:sustain:release`,
//! `filter=type:cutoff:resonance:envelope`, `lfo1=shape:rate` (a note value syncs it, `:free`
//! lets every note follow the same wave). Modulations read `source>target amount`, macros
//! `Name: target amount, target amount`. What is not written keeps its default.
//!
//! A macro shows what it does: where the sound already has some of what a macro adds (a
//! detune, a vibrato, a reverb...), the macro takes it over and starts turned up by as much, so
//! that its knob at zero means none of it. `Name@45: ...` sets where a macro starts by hand.

use audiostrada_dsp::synth::{MACRO_NAMES, SLOTS, SOURCES};

use crate::i18n::tr;
use crate::presets::Preset;

/// A sound as written.
struct Sound {
    name: &'static str,
    tone: &'static str,
    mods: &'static str,
    macros: [&'static str; 4],
}

/// Short names of the modulation targets, in the order of the synth's.
const TARGETS: [&str; 28] = [
    "",
    "pitch",
    "osc1_pitch",
    "osc2_pitch",
    "osc1_level",
    "osc2_level",
    "sub",
    "noise",
    "cutoff",
    "resonance",
    "drive",
    "volume",
    "pos1",
    "pos2",
    "fm",
    "chorus",
    "delay",
    "reverb",
    "detune",
    "attack",
    "decay",
    "release",
    "env_amount",
    "lfo1_rate",
    "lfo2_rate",
    "lfo1_depth",
    "lfo2_depth",
    "env2_depth",
];
const MOD_SOURCES: [&str; 7] = ["", "lfo1", "lfo2", "env2", "velocity", "wheel", "pressure"];
const WAVES: [&str; 4] = ["saw", "square", "sine", "triangle"];
const TABLES: [&str; 8] = [
    "basic",
    "harmonics",
    "pwm",
    "vocal",
    "metallic",
    "sync",
    "fold",
    "organ",
];
const FILTERS: [&str; 6] = ["lp12", "lp24", "ladder", "hp", "bp", "notch"];
const SHAPES: [&str; 6] = ["sine", "triangle", "sawup", "sawdown", "square", "random"];

fn number(text: &str, of: &str) -> f32 {
    match text {
        "on" => 1.0,
        "off" => 0.0,
        _ => text
            .parse()
            .unwrap_or_else(|_| panic!("{of}: {text} is not a number")),
    }
}

fn place(list: &[&str], name: &str, of: &str) -> f32 {
    list.iter()
        .position(|n| *n == name)
        .unwrap_or_else(|| panic!("{of}: no {name}")) as f32
}

/// A note value (`1/8`, `1/8D`, `1bar`) as its place among the synced times.
fn note(text: &str) -> Option<f32> {
    audiostrada_dsp::delay::NOTE_NAMES
        .iter()
        .position(|n| n.replace(' ', "") == text)
        .map(|i| i as f32)
}

/// A wave: one of the four, or a wavetable by its name.
fn wave(text: &str, of: &str) -> (f32, Option<f32>) {
    match WAVES.iter().position(|w| *w == text) {
        Some(i) => (i as f32, None),
        None => (
            audiostrada_dsp::synth::WAVETABLE as f32,
            Some(place(&TABLES, text, of)),
        ),
    }
}

/// The parameters one `id=value` pair sets.
fn setting(id: &str, value: &str, of: &str) -> Vec<(String, f32)> {
    let parts: Vec<&str> = value.split(':').collect();
    let n = |i: usize| {
        number(
            parts
                .get(i)
                .unwrap_or_else(|| panic!("{of}: {id} lacks a value")),
            of,
        )
    };
    let named = |pairs: &[(&str, f32)]| -> Vec<(String, f32)> {
        pairs.iter().map(|&(k, v)| (k.to_owned(), v)).collect()
    };
    match id {
        "osc1" => {
            let (w, table) = wave(parts[0], of);
            let mut out = named(&[("wave", w), ("osc1_voices", n(1)), ("detune", n(2))]);
            out.extend(table.map(|t| ("osc1_table".to_owned(), t)));
            out
        }
        "osc2" => {
            let (w, table) = wave(parts[0], of);
            let mut out = named(&[
                ("osc2_wave", w),
                ("osc2_voices", n(1)),
                ("osc2_detune", n(2)),
                ("osc2_level", n(3)),
            ]);
            out.extend(table.map(|t| ("osc2_table".to_owned(), t)));
            out
        }
        "adsr" => named(&[
            ("attack", n(0)),
            ("decay", n(1)),
            ("sustain", n(2)),
            ("release", n(3)),
        ]),
        "env2" => named(&[
            ("env2_attack", n(0)),
            ("env2_decay", n(1)),
            ("env2_sustain", n(2)),
            ("env2_release", n(3)),
        ]),
        "filter" => named(&[
            ("filter_type", place(&FILTERS, parts[0], of)),
            ("cutoff", n(1)),
            ("resonance", n(2)),
            ("env_amount", n(3)),
        ]),
        "lfo1" | "lfo2" => {
            let mut out = vec![(format!("{id}_shape"), place(&SHAPES, parts[0], of))];
            let rate = parts
                .get(1)
                .unwrap_or_else(|| panic!("{of}: {id} lacks a rate"));
            match note(rate) {
                Some(at) => {
                    out.push((format!("{id}_sync"), 1.0));
                    out.push((format!("{id}_note"), at));
                }
                None => out.push((format!("{id}_rate"), number(rate, of))),
            }
            if parts.get(2) == Some(&"free") {
                out.push((format!("{id}_retrig"), 0.0));
            }
            out
        }
        "mode" => named(&[("mode", place(&["poly", "mono", "legato"], value, of))]),
        "sub_octave" => named(&[("sub_octave", place(&["-1", "-2", "0"], value, of))]),
        "arp_mode" => named(&[(
            "arp_mode",
            place(&["up", "down", "updown", "random", "played"], value, of),
        )]),
        "arp_note" | "fx_delay_note" => named(&[(
            id,
            note(value).unwrap_or_else(|| panic!("{of}: no note {value}")),
        )]),
        _ => named(&[(id, number(value, of))]),
    }
}

/// The preset a written sound makes: its settings, then its modulations and its macros, one
/// routing each.
fn build(sound: &Sound) -> Preset {
    let of = sound.name;
    let mut params = std::collections::BTreeMap::new();
    for pair in sound.tone.split_whitespace() {
        let (id, value) = pair
            .split_once('=')
            .unwrap_or_else(|| panic!("{of}: {pair} is not id=value"));
        params.extend(setting(id, value, of));
    }
    let mut slot = 0;
    let mut routed: Vec<(String, f32)> = Vec::new();
    let mut route = |source: usize, to: &str, amount: &str| {
        slot += 1;
        assert!(slot <= SLOTS, "{of}: more than {SLOTS} routings");
        let target = place(&TARGETS, to, of);
        assert!(
            audiostrada_dsp::synth::reaches(source, target as usize),
            "{of}: {} cannot move {to}",
            SOURCES[source]
        );
        routed.push((format!("mod{slot}_source"), source as f32));
        routed.push((format!("mod{slot}_target"), target));
        routed.push((format!("mod{slot}_amount"), number(amount, of)));
    };
    for routing in sound
        .mods
        .split(',')
        .map(str::trim)
        .filter(|r| !r.is_empty())
    {
        let (source, rest) = routing
            .split_once('>')
            .unwrap_or_else(|| panic!("{of}: {routing} is not source>target amount"));
        let (to, amount) = rest
            .split_once(' ')
            .unwrap_or_else(|| panic!("{of}: {routing} lacks an amount"));
        route(place(&MOD_SOURCES, source, of) as usize, to, amount);
    }
    let mut names = Vec::new();
    let mut carried: Vec<&str> = Vec::new();
    for (m, written) in sound.macros.iter().enumerate() {
        let (name, moves) = written
            .split_once(':')
            .unwrap_or_else(|| panic!("{of}: {written} is not Name: target amount"));
        let (name, start) = match name.split_once('@') {
            Some((name, start)) => (name, Some(number(start, of))),
            None => (name, None),
        };
        names.push((
            format!("macro{}_name", m + 1),
            place(&MACRO_NAMES, name, of),
        ));
        let mut moves: Vec<(&str, f32)> = moves
            .split(',')
            .map(|one| {
                let (to, amount) = one
                    .trim()
                    .split_once(' ')
                    .unwrap_or_else(|| panic!("{of}: {one} lacks an amount"));
                (to, number(amount, of))
            })
            .collect();
        let start = start.unwrap_or_else(|| carry(&mut params, &mut moves, &mut carried));
        if start > 0.0 {
            names.push((format!("macro{}", m + 1), start));
        }
        for (to, amount) in moves {
            route(
                audiostrada_dsp::synth::FIRST_MACRO + m,
                to,
                &amount.to_string(),
            );
        }
    }
    params.extend(routed);
    params.extend(names);
    Preset {
        name: sound.name.to_owned(),
        params,
        state: None,
        factory: true,
    }
}

/// The setting a macro's target adds to, straight: its id, and what a full amount adds.
fn added_to(target: &str) -> Option<(&'static str, f32)> {
    Some(match target {
        "sub" => ("sub_level", 100.0),
        "noise" => ("noise", 100.0),
        "resonance" => ("resonance", 95.0),
        "drive" => ("drive", 100.0),
        "osc2_level" => ("osc2_level", 100.0),
        "pos1" => ("osc1_position", 100.0),
        "pos2" => ("osc2_position", 100.0),
        "fm" => ("fm", 100.0),
        "chorus" => ("fx_chorus", 100.0),
        "delay" => ("fx_delay", 100.0),
        "reverb" => ("fx_reverb", 100.0),
        "detune" => ("detune", 50.0),
        "env_amount" => ("env_amount", 100.0),
        "lfo1_depth" => ("lfo1_depth", 100.0),
        "lfo2_depth" => ("lfo2_depth", 100.0),
        "env2_depth" => ("env2_depth", 100.0),
        _ => return None,
    })
}

/// The setting a macro's target multiplies, and by how much at a full amount.
fn multiplied(target: &str) -> Option<(&'static str, f32)> {
    Some(match target {
        "cutoff" => ("cutoff", 32.0),
        "attack" => ("attack", 100.0),
        "decay" => ("decay", 30.0),
        "release" => ("release", 30.0),
        "lfo1_rate" => ("lfo1_rate", 8.0),
        "lfo2_rate" => ("lfo2_rate", 8.0),
        _ => return None,
    })
}

/// Hands a macro what the sound already has of what it adds: the settings go to zero, the
/// macro's amounts grow to reach as far as before, and the macro starts turned up (the
/// percents returned) to where the sound was. What the macro multiplies is set back by as
/// much, so that the sound stays the one written.
fn carry<'a>(
    params: &mut std::collections::BTreeMap<String, f32>,
    moves: &mut [(&'a str, f32)],
    carried: &mut Vec<&'a str>,
) -> f32 {
    let info = audiostrada_dsp::info("synth").expect("the synth");
    let now = |params: &std::collections::BTreeMap<String, f32>, id: &str| {
        params.get(id).copied().unwrap_or_else(|| {
            info.params
                .iter()
                .find(|p| p.id == id)
                .map_or(0.0, |p| p.default)
        })
    };
    // Where the macro stands: where the first setting it adds to already is.
    let mut start = 0.0f32;
    for &(to, amount) in moves.iter() {
        let Some((id, reach)) = added_to(to) else {
            continue;
        };
        let base = now(params, id);
        if amount > 0.0 && base > 0.0 && !carried.contains(&to) {
            start = base / (base + amount / 100.0 * reach);
            break;
        }
    }
    if start <= 0.0 {
        return 0.0;
    }
    let start = (start * 100.0).round() / 100.0;
    for (to, amount) in moves.iter_mut() {
        if let Some((id, reach)) = added_to(to) {
            if *amount <= 0.0 || carried.contains(to) {
                continue;
            }
            let base = now(params, id);
            let full = base + *amount / 100.0 * reach;
            *amount = (full / reach * 100.0).min(100.0).round();
            params.insert(id.to_owned(), 0.0);
            carried.push(to);
            // The second oscillator keeps what it had more than the first.
            if id == "detune" {
                let more = (now(params, "osc2_detune") - base).max(0.0);
                params.insert("osc2_detune".to_owned(), more);
            }
        } else if let Some((id, factor)) = multiplied(to) {
            let Some(p) = info.params.iter().find(|p| p.id == id) else {
                continue;
            };
            let synced = id.starts_with("lfo") && now(params, &id.replace("rate", "sync")) >= 0.5;
            if !synced {
                let back = now(params, id) / factor.powf(start * *amount / 100.0);
                params.insert(id.to_owned(), back.clamp(p.min, p.max));
            }
        }
    }
    start * 100.0
}

/// The ready-made sounds, the synth's defaults first.
pub(crate) fn factory() -> Vec<Preset> {
    let mut presets = vec![crate::presets::preset(tr("Init"), &[])];
    presets.extend(sounds().iter().map(build));
    presets
}

fn sounds() -> Vec<Sound> {
    let mut all = Vec::new();
    all.extend(basses());
    all.extend(leads());
    all.extend(keys());
    all.extend(pads());
    all.extend(strings_and_brass());
    all.extend(plucks());
    all.extend(bells());
    all.extend(polys());
    all.extend(arps());
    all.extend(atmospheres());
    all.extend(effects());
    all
}

fn basses() -> Vec<Sound> {
    vec![
        Sound {
            name: tr("Bass: Sub"),
            tone: "osc1=sine:1:0 osc2=saw:1:0:0 osc2_octave=1 filter=lp24:500:0:0 drive=30 \
                   key_track=50 adsr=3:300:100:140 env2=1:70:0:70 env2_depth=0 mode=mono \
                   drift=4 velocity=35 spread=0 sub_octave=0 volume=-6.7",
            mods: "env2>pitch 40",
            macros: [
                "Harmonics: drive 70, cutoff 30",
                "Octave: osc2_level 60, cutoff 20",
                "Punch: env2_depth 100",
                "Length: release 55",
            ],
        },
        Sound {
            name: tr("Bass: Analog"),
            tone: "osc1=saw:1:0 osc2=square:1:0:60 osc2_octave=-1 filter=ladder:420:25:55 drive=25 key_track=30 adsr=2:450:55:150 mode=mono drift=15 velocity=50 spread=0 sub_octave=0 volume=-5.5",
            mods: "velocity>cutoff 15",
            macros: [
                "Brightness: cutoff 45",
                "Impact: env_amount 45, decay -40",
                "Thickness: osc2_level 40",
                "Dirt: drive 70",
            ],
        },
        Sound {
            name: tr("Bass: Acid"),
            tone: "osc1=saw:1:0 filter=ladder:260:70:75 drive=15 adsr=1:200:15:90 mode=legato \
                   glide=80 drift=0 velocity=60 spread=0 sub_octave=0 fx_delay_note=1/8D \
                   fx_delay_feedback=45 volume=-3.8",
            mods: "velocity>cutoff 25",
            macros: [
                "Squelch: cutoff 40",
                "Resonance: resonance 28",
                "Dirt: drive 85",
                "Echo: delay 35, reverb 10",
            ],
        },
        Sound {
            name: tr("Bass: Reese"),
            tone: "osc1=saw:2:0 osc2=saw:2:0:85 sub_level=45 sub_octave=0 filter=lp24:800:12:15 adsr=6:300:100:180 mode=mono spread=35 drift=20 velocity=30 lfo1=sine:0.15:free lfo1_depth=40 volume=-10",
            mods: "lfo1>cutoff 18",
            macros: [
                "Brightness: cutoff 50",
                "Detune@45: detune 50, osc2_pitch -1.6",
                "Growl: drive 70, resonance 35",
                "Motion: lfo1_depth 100, lfo1_rate 50",
            ],
        },
        Sound {
            name: tr("Bass: Wobble"),
            tone: "osc1=saw:2:12 osc2=square:1:0:60 osc2_octave=-1 sub_level=40 sub_octave=0 \
                   filter=ladder:400:45:0 drive=35 adsr=2:300:100:120 mode=mono spread=20 \
                   lfo1=triangle:1/8 lfo1_depth=70 velocity=30 volume=-8.7",
            mods: "lfo1>cutoff 50",
            macros: [
                "Speed: lfo1_rate 67",
                "Wobble: lfo1_depth 60, cutoff 15",
                "Grit: drive 60, resonance 25",
                "Brightness: cutoff 35",
            ],
        },
        Sound {
            name: tr("Bass: FM Growl"),
            tone: "osc1=sine:1:0 osc2=sine:1:0:0 fm=22 sub_level=30 sub_octave=0 filter=lp24:1400:10:0 drive=20 adsr=2:300:90:120 env2=1:220:20:150 mode=mono spread=0 lfo1=triangle:1/8 lfo1_depth=0 velocity=40 volume=-6.3",
            mods: "env2>fm 25, lfo1>fm 35, velocity>fm 10",
            macros: [
                "Growl: fm 45",
                "Wobble: lfo1_depth 100",
                "Bite: resonance 50, cutoff 25",
                "Dirt: drive 70",
            ],
        },
        Sound {
            name: tr("Bass: 808"),
            tone: "osc1=sine:1:0 osc2=saw:1:0:0 filter=lp12:900:0:0 drive=22 key_track=60 \
                   adsr=1:1800:0:350 env2=1:85:0:85 env2_depth=40 mode=mono drift=0 \
                   velocity=30 spread=0 sub_octave=0 volume=-7.6",
            mods: "env2>pitch 45",
            macros: [
                "Punch: env2_depth 100, env_amount 30",
                "Dirt: drive 75, cutoff 25",
                "Length: decay 40, release 40",
                "Brightness: cutoff 35, osc2_level 22",
            ],
        },
        Sound {
            name: tr("Bass: Trap sub"),
            tone: "osc1=sine:1:0 osc2=sine:1:0:0 osc2_octave=1 sub_level=40 sub_octave=0 filter=lp12:700:0:0 drive=28 key_track=60 adsr=1:3500:0:500 env2=1:55:0:55 env2_depth=45 mode=legato glide=90 drift=0 velocity=20 spread=0 volume=-10.9",
            mods: "env2>pitch 35",
            macros: [
                "Punch: env2_depth 100",
                "Dirt: drive 70, cutoff 30",
                "Length: decay 35, release 45",
                "Octave: osc2_level 60, cutoff 25",
            ],
        },
        Sound {
            name: tr("Bass: Pluck"),
            tone: "osc1=saw:1:0 osc2=square:1:0:50 osc2_octave=-1 filter=lp24:220:18:0 \
                   key_track=40 adsr=1:260:0:120 env2=1:170:0:120 env2_depth=70 mode=mono \
                   velocity=55 spread=0 sub_octave=0 volume=-7.9",
            mods: "env2>cutoff 55, velocity>cutoff 15",
            macros: [
                "Brightness: cutoff 40",
                "Impact: env2_depth 100, resonance 20",
                "Length: decay 45, release 30",
                "Dirt: drive 65",
            ],
        },
        Sound {
            name: tr("Bass: Fold"),
            tone: "osc1=fold:1:0 osc2=square:1:0:0 osc2_octave=-1 sub_level=30 sub_octave=0 filter=lp24:4500:5:0 adsr=2:300:90:120 \
                   env2=1:380:10:200 mode=mono velocity=40 spread=0 lfo1=sine:1/4 \
                   lfo1_depth=0 volume=-12.4",
            mods: "env2>pos1 70, lfo1>pos1 30",
            macros: [
                "Fold: pos1 45",
                "Motion: lfo1_depth 100",
                "Dirt: drive 65",
                "Octave: osc2_level 45",
            ],
        },
        Sound {
            name: tr("Bass: Rubber"),
            tone: "osc1=pwm:1:0 osc1_position=20 filter=lp24:350:35:0 key_track=40 \
                   adsr=1:220:35:90 env2=1:140:5:100 env2_depth=55 mode=mono velocity=60 \
                   spread=0 sub_octave=0 volume=3.3",
            mods: "env2>cutoff 50, velocity>cutoff 20",
            macros: [
                "Hollow: pos1 75",
                "Squelch: env2_depth 80, resonance 20",
                "Brightness: cutoff 40",
                "Dirt: drive 60",
            ],
        },
    ]
}

fn leads() -> Vec<Sound> {
    vec![
        Sound {
            name: tr("Lead: Supersaw"),
            tone: "osc1=saw:7:22 osc2=saw:7:26:45 osc2_octave=1 filter=lp12:5500:10:20 \
                   key_track=30 adsr=4:400:85:350 spread=100 velocity=40 lfo1=sine:5.5 \
                   lfo1_depth=0 fx_delay=15 fx_reverb=18 volume=-12",
            mods: "lfo1>pitch 3, wheel>lfo1_depth 100, velocity>cutoff 15",
            macros: [
                "Brightness: cutoff 40",
                "Detune: detune 45",
                "Vibrato: lfo1_depth 100",
                "Space: reverb 35, delay 25",
            ],
        },
        Sound {
            name: tr("Lead: Mono glide"),
            tone: "osc1=saw:1:0 osc2=saw:1:0:80 osc2_fine=4 filter=ladder:1600:30:45 drive=15 \
                   adsr=3:500:75:200 mode=legato glide=110 drift=5 velocity=45 \
                   lfo1=sine:5.5 lfo1_depth=0 fx_delay=22 volume=-3.6",
            mods: "lfo1>pitch 3.5, wheel>lfo1_depth 100, velocity>cutoff 20",
            macros: [
                "Brightness: cutoff 40",
                "Vibrato: lfo1_depth 100",
                "Bite: resonance 40, drive 40",
                "Echo: delay 35, reverb 15",
            ],
        },
        Sound {
            name: tr("Lead: Sync"),
            tone: "osc1=saw:1:0 osc1_semi=7 osc2=saw:1:0:0 sync=on filter=lp12:9000:5:0 \
                   adsr=2:400:85:250 env2=1:700:20:300 env2_depth=60 mode=legato glide=40 \
                   velocity=40 lfo1=sine:5.5 lfo1_depth=0 fx_delay=18 fx_reverb=12 volume=-4.9",
            mods: "env2>osc1_pitch 60, lfo1>pitch 3.5, wheel>lfo1_depth 100",
            macros: [
                "Sync: osc1_pitch 70",
                "Sweep: env2_depth 80",
                "Vibrato: lfo1_depth 100",
                "Echo: delay 35, reverb 15",
            ],
        },
        Sound {
            name: tr("Lead: Chip"),
            tone: "osc1=pwm:1:0 osc1_position=12 osc2=square:1:0:0 osc2_octave=1 \
                   filter=lp12:5000:0:0 adsr=1:80:85:60 mode=mono drift=0 velocity=20 \
                   spread=0 lfo1=sine:6 lfo1_depth=0 lfo2=triangle:0.6:free volume=1.8",
            mods: "lfo1>pitch 3, lfo2>pos1 12, wheel>lfo1_depth 100",
            macros: [
                "Pulse: pos1 45",
                "Vibrato: lfo1_depth 100",
                "Octave: osc2_level 25",
                "Echo: delay 40",
            ],
        },
        Sound {
            name: tr("Lead: Whistle"),
            tone: "osc1=sine:1:0 osc2=sine:1:0:0 osc2_octave=1 noise=3 filter=lp12:2000:10:0 \
                   key_track=100 adsr=40:200:90:220 env2=450:10:100:200 mode=legato glide=60 \
                   drift=15 velocity=40 lfo1=sine:5.2 lfo1_depth=0 fx_reverb=28 volume=-6",
            mods: "lfo1>pitch 3.5, env2>lfo1_depth 30",
            macros: [
                "Breath: noise 25, cutoff 20",
                "Vibrato: lfo1_depth 100",
                "Octave: osc2_level 35",
                "Space: reverb 35, delay 15",
            ],
        },
        Sound {
            name: tr("Lead: Screamer"),
            tone: "osc1=saw:2:10 osc2=saw:1:0:0 osc2_semi=7 filter=ladder:1800:50:50 drive=40 \
                   adsr=2:500:80:200 mode=mono glide=30 velocity=45 lfo1=sine:5.8 \
                   lfo1_depth=0 fx_delay=20 fx_reverb=12 volume=-1.8",
            mods: "lfo1>pitch 3, wheel>lfo1_depth 100, velocity>cutoff 20",
            macros: [
                "Bite: resonance 35, cutoff 25",
                "Dirt: drive 50",
                "Fifth: osc2_level 60",
                "Echo: delay 35, reverb 15",
            ],
        },
        Sound {
            name: tr("Lead: Fold"),
            tone: "osc1=fold:2:8 osc1_position=25 osc2=saw:1:0:0 osc2_octave=-1 \
                   filter=lp12:7000:5:0 adsr=3:400:85:220 mode=legato glide=50 velocity=40 \
                   lfo1=sine:5.5 lfo1_depth=0 lfo2=triangle:0.25:free fx_delay=18 \
                   fx_reverb=15 volume=-8.3",
            mods: "lfo1>pitch 3.5, lfo2>pos1 15, wheel>lfo1_depth 100",
            macros: [
                "Fold: pos1 60",
                "Vibrato: lfo1_depth 100",
                "Thickness: detune 40, osc2_level 50",
                "Space: reverb 35, delay 20",
            ],
        },
        Sound {
            name: tr("Lead: Vocal"),
            tone: "osc1=vocal:1:0 osc1_position=0 filter=lp12:8000:5:0 adsr=15:400:90:250 \
                   env2=350:600:60:300 mode=legato glide=70 velocity=40 lfo1=sine:5.3 \
                   lfo1_depth=0 lfo2=sine:0.3:free lfo2_depth=0 fx_delay=18 fx_reverb=18 \
                   volume=-2.6",
            mods: "env2>pos1 30, lfo1>pitch 3.5, lfo2>pos1 35, wheel>lfo1_depth 100",
            macros: [
                "Vowel: pos1 70",
                "Motion: lfo2_depth 100",
                "Vibrato: lfo1_depth 100",
                "Echo: delay 35, reverb 20",
            ],
        },
        Sound {
            name: tr("Lead: Square"),
            tone: "osc1=pwm:3:9 osc1_position=5 filter=lp12:4000:15:25 adsr=3:400:85:220 \
                   mode=mono glide=25 velocity=40 lfo1=sine:5.5 lfo1_depth=0 \
                   lfo2=triangle:0.4:free fx_delay=18 fx_reverb=12 volume=3.6",
            mods: "lfo1>pitch 3.5, lfo2>pos1 18, wheel>lfo1_depth 100",
            macros: [
                "Hollow: pos1 55",
                "Brightness: cutoff 40",
                "Vibrato: lfo1_depth 100",
                "Echo: delay 35, reverb 15",
            ],
        },
    ]
}

fn keys() -> Vec<Sound> {
    vec![
        Sound {
            name: tr("Keys: Soft"),
            tone: "osc1=saw:2:5 osc2=sine:1:0:45 osc2_octave=1 filter=lp24:700:5:45 \
                   key_track=50 adsr=3:900:25:350 velocity=75 lfo1=sine:5 lfo1_depth=0 \
                   fx_chorus=22 fx_reverb=16 volume=-10.7",
            mods: "velocity>cutoff 25, lfo1>volume 45",
            macros: [
                "Brightness: cutoff 40",
                "Tremolo: lfo1_depth 100",
                "Chorus: chorus 50, detune 20",
                "Space: reverb 40",
            ],
        },
        Sound {
            name: tr("Keys: Electric"),
            tone: "osc1=sine:1:0 osc2=sine:1:0:0 fm=8 filter=lp12:6000:0:0 \
                   adsr=2:1800:0:350 env2=1:350:0:350 velocity=80 drift=6 lfo1=sine:4.8 \
                   lfo1_depth=0 fx_chorus=30 fx_reverb=14 volume=-11.2",
            mods: "env2>fm 22, velocity>fm 15, lfo1>volume 45",
            macros: [
                "Hammer: fm 25",
                "Tremolo: lfo1_depth 100",
                "Chorus: chorus 55",
                "Space: reverb 35, delay 12",
            ],
        },
        Sound {
            name: tr("Keys: Wurly"),
            tone: "osc1=sine:1:0 osc2=sine:1:0:0 fm=14 filter=lp24:3200:5:0 drive=20 \
                   adsr=2:1100:15:200 env2=1:180:0:180 velocity=80 drift=6 lfo1=sine:5.6 \
                   lfo1_depth=25 fx_reverb=12 volume=-11.2",
            mods: "env2>fm 30, velocity>fm 12, lfo1>volume 45",
            macros: [
                "Bite: fm 30, drive 40",
                "Tremolo: lfo1_depth 75",
                "Brightness: cutoff 30",
                "Space: reverb 35, chorus 25",
            ],
        },
        Sound {
            name: tr("Keys: Clav"),
            tone: "osc1=pwm:1:0 osc1_position=55 filter=lp24:1500:30:60 key_track=70 \
                   adsr=1:450:0:90 env2=1:60:0:60 velocity=80 drift=4 lfo1=sine:1/8:free \
                   lfo1_depth=0 volume=-1.2",
            mods: "env2>cutoff 35, velocity>cutoff 20, lfo1>cutoff 55",
            macros: [
                "Brightness: cutoff 40",
                "Bite: resonance 45, env2_depth 100",
                "Sweep: lfo1_depth 100",
                "Echo: delay 30, chorus 30",
            ],
        },
        Sound {
            name: tr("Keys: Organ"),
            tone: "osc1=organ:1:0 osc1_position=45 osc2=organ:1:0:35 osc2_position=90 \
                   osc2_octave=1 filter=lp12:6000:0:0 adsr=3:50:100:60 velocity=0 drift=5 \
                   lfo1=sine:6.2:free lfo1_depth=0 fx_chorus=35 fx_reverb=12 volume=-11.5",
            mods: "lfo1>pitch 1.5",
            macros: [
                "Harmonics: pos1 50",
                "Chorus: chorus 40, lfo1_depth 100",
                "Drive: drive 65",
                "Space: reverb 40",
            ],
        },
        Sound {
            name: tr("Keys: Bright FM"),
            tone: "osc1=sine:1:0 osc2=sine:1:0:8 osc2_octave=2 fm=4 filter=lp12:9000:0:0 \
                   adsr=1:2200:0:450 env2=1:120:0:120 velocity=80 drift=4 fx_chorus=25 \
                   fx_reverb=20 volume=-10.9",
            mods: "env2>fm 20, velocity>fm 12",
            macros: [
                "Hammer: fm 18",
                "Shimmer: osc2_level 30",
                "Chorus: chorus 50, detune 15",
                "Space: reverb 40, delay 12",
            ],
        },
        Sound {
            name: tr("Keys: Lo-fi"),
            tone: "osc1=triangle:2:14 osc2=sine:1:0:40 noise=2 filter=lp24:1100:10:35 \
                   adsr=4:1200:20:400 drift=60 velocity=60 lfo2=sine:0.6:free lfo2_depth=40 \
                   fx_chorus=25 fx_reverb=22 volume=-7.2",
            mods: "lfo2>pitch 1.2, velocity>cutoff 20",
            macros: [
                "Brightness: cutoff 40",
                "Drift: lfo2_depth 100, detune 25",
                "Dirt: drive 60, noise 5",
                "Space: reverb 40, delay 15",
            ],
        },
        Sound {
            name: tr("Keys: Dream"),
            tone: "osc1=triangle:1:0 osc2=saw:5:16:0 filter=lp24:1100:8:30 key_track=40 \
                   adsr=2:2500:40:900 env2=900:1500:70:900 env2_depth=50 velocity=65 \
                   spread=90 fx_chorus=25 fx_reverb=35 fx_reverb_decay=4 volume=-8.6",
            mods: "env2>osc2_level 40, velocity>cutoff 20",
            macros: [
                "Brightness: cutoff 50",
                "Bloom: env2_depth 100",
                "Chorus: chorus 45, detune 25",
                "Space: reverb 35, delay 20",
            ],
        },
    ]
}

fn pads() -> Vec<Sound> {
    vec![
        Sound {
            name: tr("Pad: Warm"),
            tone: "osc1=saw:4:15 osc2=saw:3:12:60 osc2_octave=-1 filter=lp12:1200:15:20 \
                   adsr=600:1500:80:2500 spread=80 drift=30 velocity=30 \
                   lfo1=triangle:0.2:free fx_chorus=15 fx_reverb=25 fx_reverb_decay=4 \
                   volume=-12.3",
            mods: "lfo1>cutoff 15",
            macros: [
                "Brightness: cutoff 40",
                "Motion: lfo1_depth 100, lfo1_rate 40",
                "Detune: detune 40, chorus 30",
                "Space: reverb 40, release 25",
            ],
        },
        Sound {
            name: tr("Pad: Glass"),
            tone: "osc1=harmonics:3:10 osc1_position=25 osc2=sine:1:0:50 osc2_octave=1 filter=lp12:5000:25:15 \
                   adsr=900:2000:85:3500 spread=90 drift=20 velocity=30 lfo1=sine:0.15:free \
                   fx_reverb=30 fx_reverb_decay=5 volume=-12.6",
            mods: "lfo1>osc2_level 40",
            macros: [
                "Harmonics: pos1 50",
                "Motion: lfo1_depth 100, lfo1_rate 50",
                "Metal: fm 12",
                "Space: reverb 40, delay 20",
            ],
        },
        Sound {
            name: tr("Pad: Airy"),
            tone: "osc1=saw:5:18 osc2=triangle:1:0:30 osc2_octave=1 noise=3 \
                   filter=hp:380:15:0 adsr=1200:2000:85:4000 spread=100 drift=25 velocity=30 \
                   lfo1=sine:0.1:free lfo1_depth=30 fx_reverb=35 fx_reverb_decay=6 volume=-5.2",
            mods: "lfo1>cutoff 20",
            macros: [
                "Air: noise 14, cutoff 30",
                "Motion: lfo1_depth 100, lfo1_rate 50",
                "Shimmer: osc2_level 45, chorus 30",
                "Space: reverb 40, delay 20",
            ],
        },
        Sound {
            name: tr("Pad: Evolving"),
            tone: "osc1=saw:4:14 osc2=triangle:2:8:70 osc2_octave=1 filter=lp24:900:30:10 \
                   adsr=1500:3000:80:4000 spread=90 drift=25 velocity=30 \
                   lfo1=triangle:0.08:free lfo2=sine:0.13:free fx_reverb=30 \
                   fx_reverb_decay=5 volume=-13.5",
            mods: "lfo1>cutoff 30, lfo2>osc2_level 60",
            macros: [
                "Brightness: cutoff 40",
                "Motion: lfo1_rate 60, lfo2_rate 60, lfo1_depth 60",
                "Resonance: resonance 50",
                "Space: reverb 40, delay 20",
            ],
        },
        Sound {
            name: tr("Pad: Vowels"),
            tone: "osc1=vocal:4:12 osc1_position=50 osc2=saw:3:10:0 osc2_octave=-1 \
                   filter=lp12:6000:10:0 adsr=800:2000:85:2500 spread=80 drift=15 \
                   velocity=30 lfo1=triangle:0.12:free fx_chorus=20 fx_reverb=25 \
                   fx_reverb_decay=4 volume=-7.8",
            mods: "lfo1>pos1 50",
            macros: [
                "Vowel: pos1 45",
                "Motion: lfo1_rate 67, lfo1_depth 60",
                "Thickness: osc2_level 40, detune 30",
                "Space: reverb 40, delay 15",
            ],
        },
        Sound {
            name: tr("Pad: Space"),
            tone: "osc1=saw:5:16 osc2=triangle:1:0:50 osc2_octave=1 filter=lp24:2200:15:10 \
                   adsr=1800:3000:85:4500 spread=100 drift=20 velocity=30 fx_chorus=40 \
                   fx_reverb=45 fx_reverb_decay=7 fx_delay=15 fx_delay_note=1/4 volume=-7.4",
            mods: "",
            macros: [
                "Brightness: cutoff 40",
                "Shimmer: osc2_level 45",
                "Echo: delay 40",
                "Space: reverb 35, release 20",
            ],
        },
        Sound {
            name: tr("Pad: Dark"),
            tone: "osc1=saw:6:14 osc1_octave=-1 osc2=saw:4:8:50 filter=lp24:600:20:10 \
                   adsr=900:2000:90:3000 spread=100 drift=25 velocity=30 \
                   lfo1=triangle:0.1:free lfo1_depth=50 fx_reverb=30 fx_reverb_decay=5 \
                   volume=-10.9",
            mods: "lfo1>cutoff 14",
            macros: [
                "Brightness: cutoff 45",
                "Motion: lfo1_depth 100, lfo1_rate 50",
                "Resonance: resonance 45",
                "Space: reverb 40, delay 15",
            ],
        },
        Sound {
            name: tr("Pad: PWM"),
            tone: "osc1=pwm:4:10 osc2=pwm:4:8:70 filter=lp12:2800:10:0 \
                   adsr=500:2000:90:2500 spread=90 velocity=30 lfo1=sine:0.3:free \
                   lfo2=sine:0.21:free fx_chorus=30 fx_reverb=25 volume=-3.3",
            mods: "lfo1>pos1 60, lfo2>pos2 60",
            macros: [
                "Brightness: cutoff 35",
                "Motion: lfo1_rate 60, lfo2_rate 60",
                "Hollow: pos1 30, pos2 30",
                "Space: reverb 40, delay 15",
            ],
        },
        Sound {
            name: tr("Pad: Sweep"),
            tone: "osc1=saw:5:16 osc2=square:3:8:40 filter=lp24:900:40:0 \
                   adsr=700:2000:90:3000 spread=90 velocity=30 lfo1=triangle:0.07:free \
                   lfo1_depth=60 fx_reverb=30 fx_reverb_decay=6 volume=-12.6",
            mods: "lfo1>cutoff 45",
            macros: [
                "Sweep: lfo1_depth 80",
                "Speed: lfo1_rate 80",
                "Resonance: resonance 40",
                "Space: reverb 40, delay 20",
            ],
        },
        Sound {
            name: tr("Pad: Choir"),
            tone: "osc1=vocal:5:12 osc1_position=15 osc2=vocal:3:10:40 osc2_position=15 \
                   osc2_octave=-1 filter=lp12:7000:10:0 adsr=600:2000:90:2500 spread=90 \
                   velocity=30 lfo1=sine:0.15:free fx_chorus=35 fx_reverb=35 \
                   fx_reverb_decay=5 volume=-7.7",
            mods: "lfo1>pos1 20",
            macros: [
                "Vowel: pos1 60, pos2 60",
                "Air: noise 40",
                "Body: osc2_level 45",
                "Space: reverb 40, delay 15",
            ],
        },
    ]
}

fn strings_and_brass() -> Vec<Sound> {
    vec![
        Sound {
            name: tr("Strings: Ensemble"),
            tone: "osc1=harmonics:4:10 osc1_position=62 osc2=harmonics:3:9:45 osc2_position=50 osc2_octave=-1 noise=1 filter=lp12:2600:5:20 key_track=40 adsr=140:800:85:500 spread=85 drift=25 velocity=45 lfo1=sine:5.4:free lfo1_depth=0 fx_chorus=30 fx_reverb=24 fx_reverb_decay=3 volume=-11.1",
            mods: "lfo1>pitch 1.6, velocity>cutoff 18",
            macros: [
                "Brightness: cutoff 35",
                "Swell: attack 55",
                "Vibrato@45: lfo1_depth 100",
                "Space: reverb 40, release 20",
            ],
        },
        Sound {
            name: tr("Strings: Slow"),
            tone: "osc1=harmonics:5:11 osc1_position=50 osc2=harmonics:3:9:0 osc2_position=60 osc2_octave=1 sub_level=0 filter=lp12:1900:5:20 key_track=40 adsr=800:1500:85:1400 spread=90 drift=25 velocity=40 lfo1=sine:5:free lfo1_depth=0 fx_chorus=30 fx_reverb=30 fx_reverb_decay=4 volume=-10.4",
            mods: "lfo1>pitch 1.6, velocity>cutoff 15",
            macros: [
                "Brightness: cutoff 40",
                "Octave: osc2_level 50",
                "Vibrato@35: lfo1_depth 100",
                "Space: reverb 40, release 20",
            ],
        },
        Sound {
            name: tr("Strings: Machine"),
            tone: "osc1=saw:1:0 osc2=pwm:1:0:60 osc2_position=35 osc2_octave=1 filter=lp12:2400:0:0 adsr=60:300:100:900 spread=40 drift=4 velocity=0 lfo2=triangle:0.7:free fx_chorus=70 fx_reverb=15 volume=-12",
            mods: "lfo2>pos2 15",
            macros: [
                "Brightness: cutoff 35",
                "Chorus: chorus 30",
                "Octave: osc2_level 40",
                "Space: reverb 45, release 25",
            ],
        },
        Sound {
            name: tr("Strings: Pizzicato"),
            tone: "osc1=harmonics:2:8 osc1_position=60 osc2=triangle:1:0:40 filter=lp24:900:10:0 key_track=50 \
                   adsr=1:200:0:160 env2=1:110:0:90 env2_depth=70 velocity=70 spread=60 \
                   fx_reverb=25 fx_reverb_decay=1.6 volume=-7.5",
            mods: "env2>cutoff 45, velocity>cutoff 15",
            macros: [
                "Brightness: cutoff 40",
                "Length: decay 50, release 40",
                "Impact: env2_depth 100",
                "Space: reverb 40",
            ],
        },
        Sound {
            name: tr("Brass: Analog"),
            tone: "osc1=saw:2:9 osc2=saw:1:0:70 osc2_fine=6 filter=lp24:600:15:0 \
                   key_track=40 adsr=25:300:85:220 env2=60:350:45:200 env2_depth=70 \
                   velocity=60 drift=15 lfo1=sine:5.5 lfo1_depth=0 fx_reverb=15 volume=-13.1",
            mods: "env2>cutoff 50, velocity>cutoff 25, lfo1>pitch 3, wheel>lfo1_depth 100",
            macros: [
                "Brightness: cutoff 40",
                "Bite: env2_depth 70, resonance 20",
                "Vibrato: lfo1_depth 100",
                "Space: reverb 40",
            ],
        },
        Sound {
            name: tr("Brass: Stab"),
            tone: "osc1=saw:5:14 osc2=saw:3:12:70 filter=lp24:500:15:0 key_track=40 \
                   adsr=3:350:0:150 env2=2:220:0:150 env2_depth=75 velocity=65 spread=80 \
                   fx_reverb=20 fx_delay=12 volume=-11.8",
            mods: "env2>cutoff 60, velocity>cutoff 20",
            macros: [
                "Brightness: cutoff 40",
                "Length: decay 45, release 35",
                "Detune: detune 45",
                "Space: reverb 35, delay 20",
            ],
        },
        Sound {
            name: tr("Brass: Soft horns"),
            tone: "osc1=saw:2:7 osc2=triangle:1:0:50 osc2_octave=-1 noise=1 \
                   filter=lp12:500:5:0 key_track=40 adsr=90:400:85:300 env2=140:500:50:300 \
                   velocity=55 drift=20 fx_reverb=28 fx_reverb_decay=3 volume=-11.3",
            mods: "env2>cutoff 30, velocity>cutoff 20",
            macros: [
                "Brightness: cutoff 40",
                "Breath: noise 30",
                "Swell: attack 50",
                "Space: reverb 40",
            ],
        },
        Sound {
            name: tr("Brass: Fanfare"),
            tone: "osc1=saw:4:14 osc2=saw:4:16:90 filter=lp24:1400:10:0 key_track=40 \
                   adsr=8:500:80:300 env2=20:500:50:300 env2_depth=70 velocity=55 spread=90 \
                   fx_chorus=20 fx_reverb=20 volume=-12.3",
            mods: "env2>cutoff 35, velocity>cutoff 20",
            macros: [
                "Brightness: cutoff 35",
                "Detune: detune 45",
                "Bite: env2_depth 80, resonance 25",
                "Space: reverb 40, delay 15",
            ],
        },
    ]
}

fn plucks() -> Vec<Sound> {
    vec![
        Sound {
            name: tr("Pluck: Bright"),
            tone: "osc1=saw:3:10 osc2=square:1:0:40 osc2_octave=1 filter=lp24:500:25:0 \
                   key_track=50 adsr=1:350:0:300 env2=1:200:0:200 env2_depth=70 velocity=55 \
                   fx_delay=25 fx_reverb=15 volume=-6.6",
            mods: "env2>cutoff 60, velocity>cutoff 20",
            macros: [
                "Brightness: cutoff 40",
                "Length: decay 45, release 40",
                "Bite: resonance 35, env2_depth 60",
                "Echo: delay 35, reverb 20",
            ],
        },
        Sound {
            name: tr("Pluck: Soft"),
            tone: "osc1=saw:2:6 osc2=sine:1:0:50 filter=lp24:600:5:0 key_track=50 \
                   adsr=2:500:0:400 env2=1:250:0:250 velocity=60 fx_chorus=20 fx_reverb=25 \
                   volume=-5.8",
            mods: "env2>cutoff 35, velocity>cutoff 15",
            macros: [
                "Brightness: cutoff 40",
                "Length: decay 45, release 40",
                "Chorus: chorus 50, detune 25",
                "Space: reverb 40, delay 20",
            ],
        },
        Sound {
            name: tr("Pluck: Harp"),
            tone: "osc1=saw:1:0 osc2=sine:1:0:0 osc2_octave=1 filter=lp24:600:5:70 \
                   key_track=70 adsr=1:1600:0:900 velocity=70 drift=8 fx_reverb=30 \
                   fx_reverb_decay=3 volume=-8.5",
            mods: "velocity>cutoff 25",
            macros: [
                "Brightness: cutoff 40",
                "Length: decay 35, release 35",
                "Shimmer: osc2_level 40",
                "Space: reverb 40, delay 18",
            ],
        },
        Sound {
            name: tr("Pluck: Muted"),
            tone: "osc1=saw:1:0 osc2=square:1:0:50 filter=lp24:350:20:0 key_track=60 \
                   drive=20 adsr=1:160:0:90 env2=1:60:0:60 env2_depth=75 velocity=60 \
                   fx_delay=20 fx_delay_note=1/8D volume=0.2",
            mods: "env2>cutoff 55, velocity>cutoff 15",
            macros: [
                "Brightness: cutoff 40",
                "Length: decay 55, release 40",
                "Dirt: drive 90, resonance 30",
                "Echo: delay 35, reverb 15",
            ],
        },
        Sound {
            name: tr("Pluck: Kalimba"),
            tone: "osc1=sine:1:0 osc2=sine:1:0:0 osc2_octave=2 osc2_semi=4 \
                   filter=lp12:8000:0:0 adsr=1:700:0:500 env2=1:50:0:50 velocity=70 \
                   fx_reverb=25 volume=-10.7",
            mods: "env2>fm 30, velocity>fm 8",
            macros: [
                "Metal: fm 20",
                "Length: decay 40, release 40",
                "Body: sub 50",
                "Space: reverb 40, delay 20",
            ],
        },
        Sound {
            name: tr("Pluck: Glass"),
            tone: "osc1=harmonics:2:8 osc1_position=20 filter=lp12:12000:0:0 \
                   adsr=1:600:0:500 env2=1:300:0:300 velocity=60 fx_delay=22 fx_reverb=22 \
                   volume=-5.9",
            mods: "env2>pos1 70",
            macros: [
                "Harmonics: pos1 50",
                "Length: decay 45, release 40",
                "Chorus: chorus 45, detune 25",
                "Echo: delay 35, reverb 20",
            ],
        },
        Sound {
            name: tr("Pluck: Wide"),
            tone: "osc1=saw:7:20 osc2=saw:5:18:70 osc2_octave=1 filter=lp24:400:15:0 \
                   key_track=50 adsr=1:400:10:350 env2=1:240:5:240 env2_depth=75 velocity=50 \
                   spread=100 fx_delay=28 fx_delay_note=1/8D fx_delay_feedback=45 \
                   fx_reverb=22 volume=-9.8",
            mods: "env2>cutoff 62, velocity>cutoff 15",
            macros: [
                "Brightness: cutoff 40",
                "Length: decay 50, release 40",
                "Detune: detune 45",
                "Space: reverb 35, delay 25",
            ],
        },
    ]
}

fn bells() -> Vec<Sound> {
    vec![
        Sound {
            name: tr("Bell: Classic"),
            tone: "osc1=sine:1:0 osc2=sine:1:0:0 osc2_octave=1 osc2_semi=10 fm=10 \
                   filter=lp12:14000:0:0 adsr=1:2800:0:2500 env2=1:1200:0:1200 velocity=70 \
                   drift=5 fx_reverb=30 fx_reverb_decay=4 volume=-10.1",
            mods: "env2>fm 30, velocity>fm 10",
            macros: [
                "Metal: fm 30",
                "Length: decay 35, release 35",
                "Shimmer: osc2_level 50",
                "Space: reverb 40, delay 20",
            ],
        },
        Sound {
            name: tr("Bell: Soft"),
            tone: "osc1=sine:1:0 osc2=sine:1:0:10 osc2_octave=2 filter=lp12:6000:0:0 adsr=2:2600:0:2200 velocity=65 drift=4 lfo1=sine:4.5:free lfo1_depth=0 fx_reverb=30 fx_reverb_decay=4 volume=-10.8",
            mods: "lfo1>volume 40",
            macros: [
                "Shimmer: osc2_level 35",
                "Length: decay 35, release 35",
                "Tremolo: lfo1_depth 100",
                "Space: reverb 40, delay 18",
            ],
        },
        Sound {
            name: tr("Bell: Metal"),
            tone: "osc1=metallic:2:6 osc1_position=12 filter=lp12:4500:0:0 \
                   adsr=1:2200:0:2000 env2=1:900:0:900 velocity=65 fx_reverb=30 \
                   fx_reverb_decay=4 volume=0.1",
            mods: "env2>pos1 25",
            macros: [
                "Metal: pos1 55",
                "Length: decay 35, release 35",
                "Detune: detune 45",
                "Space: reverb 40, delay 20",
            ],
        },
        Sound {
            name: tr("Bell: Tubular"),
            tone: "osc1=sine:1:0 osc2=sine:1:0:0 osc2_octave=1 osc2_semi=7 fm=12 \
                   filter=lp12:9000:0:0 adsr=2:4000:0:3000 env2=1:1800:10:1500 velocity=65 \
                   drift=5 fx_reverb=35 fx_reverb_decay=5 volume=-10.2",
            mods: "env2>fm 18, velocity>fm 8",
            macros: [
                "Metal: fm 28",
                "Length: decay 45, release 45",
                "Fifth: osc2_level 55",
                "Space: reverb 40, delay 15",
            ],
        },
        Sound {
            name: tr("Bell: Marimba"),
            tone: "osc1=sine:1:0 osc2=sine:1:0:0 osc2_octave=2 filter=lp12:6000:0:0 \
                   adsr=1:450:0:350 env2=1:70:0:70 velocity=75 fx_reverb=18 \
                   fx_reverb_decay=1.8 volume=-10.8",
            mods: "env2>fm 25, velocity>fm 8",
            macros: [
                "Hammer: fm 20",
                "Length: decay 45, release 40",
                "Body: sub 50",
                "Space: reverb 40, delay 15",
            ],
        },
        Sound {
            name: tr("Bell: Vibraphone"),
            tone: "osc1=sine:1:0 osc2=sine:1:0:12 osc2_octave=2 filter=lp12:8000:0:0 \
                   adsr=1:3000:0:2200 env2=1:90:0:90 velocity=70 lfo1=sine:5:free \
                   lfo1_depth=30 fx_reverb=28 fx_reverb_decay=3.5 volume=-11.5",
            mods: "env2>fm 15, lfo1>volume 45",
            macros: [
                "Tremolo: lfo1_depth 100",
                "Speed: lfo1_rate 40",
                "Hammer: fm 18",
                "Space: reverb 40, delay 15",
            ],
        },
        Sound {
            name: tr("Bell: Celesta"),
            tone: "osc1=triangle:1:0 osc2=sine:1:0:30 osc2_octave=2 env2=1:60:0:60 filter=lp12:6000:0:30 \
                   key_track=50 adsr=1:1300:0:900 velocity=70 fx_reverb=28 \
                   fx_reverb_decay=3 volume=-9",
            mods: "env2>fm 6",
            macros: [
                "Hammer: fm 22",
                "Length: decay 40, release 35",
                "Shimmer: osc2_level 40",
                "Space: reverb 40, delay 18",
            ],
        },
        Sound {
            name: tr("Bell: Music box"),
            tone: "osc1=sine:2:0 osc1_octave=1 osc2=sine:1:0:0 osc2_octave=3 osc2_semi=4 \
                   filter=lp12:12000:0:0 adsr=1:900:0:700 env2=1:80:0:80 velocity=60 \
                   lfo2=sine:0.5:free lfo2_depth=0 fx_reverb=25 volume=-10.6",
            mods: "env2>fm 22, lfo2>pitch 4",
            macros: [
                "Metal: fm 18",
                "Length: decay 40, release 40",
                "Drift: lfo2_depth 100, detune 30",
                "Space: reverb 40, delay 20",
            ],
        },
    ]
}

fn polys() -> Vec<Sound> {
    vec![
        Sound {
            name: tr("Poly: Chorus"),
            tone: "osc1=saw:1:0 osc2=pwm:1:0:70 osc2_position=10 sub_level=25 \
                   filter=lp24:1800:12:35 adsr=5:600:60:450 velocity=50 \
                   lfo2=triangle:0.5:free fx_chorus=45 fx_reverb=12 volume=-10.4",
            mods: "lfo2>pos2 30, velocity>cutoff 15",
            macros: [
                "Brightness: cutoff 40",
                "Pulse: pos2 40, osc2_level 20",
                "Chorus: chorus 45, detune 25",
                "Space: reverb 40, delay 15",
            ],
        },
        Sound {
            name: tr("Poly: House stab"),
            tone: "osc1=saw:3:12 osc2=saw:2:10:15 osc2_semi=7 filter=lp24:900:25:0 \
                   key_track=40 adsr=1:380:0:200 env2=1:260:0:200 env2_depth=75 velocity=60 \
                   spread=80 fx_reverb=25 fx_delay=18 volume=-6.7",
            mods: "env2>cutoff 50, velocity>cutoff 20",
            macros: [
                "Brightness: cutoff 40",
                "Fifth: osc2_level 45",
                "Length: decay 45, release 40",
                "Space: reverb 35, delay 20",
            ],
        },
        Sound {
            name: tr("Poly: Detuned"),
            tone: "osc1=saw:5:24 osc2=saw:5:28:80 osc2_fine=5 filter=lp12:3200:10:25 \
                   adsr=8:700:75:500 velocity=50 spread=100 fx_chorus=20 fx_reverb=15 \
                   volume=-13.3",
            mods: "velocity>cutoff 15",
            macros: [
                "Brightness: cutoff 40",
                "Detune: detune 45",
                "Body: sub 45",
                "Space: reverb 40, delay 20",
            ],
        },
        Sound {
            name: tr("Poly: Sync chords"),
            tone: "osc1=sync:2:8 osc1_position=25 filter=lp12:8000:5:0 adsr=3:700:60:400 \
                   env2=1:500:15:400 env2_depth=60 velocity=50 spread=80 fx_reverb=18 \
                   volume=-7",
            mods: "env2>pos1 45",
            macros: [
                "Sync: pos1 55",
                "Sweep: env2_depth 100",
                "Chorus: chorus 50, detune 25",
                "Space: reverb 40, delay 20",
            ],
        },
        Sound {
            name: tr("Poly: Soft square"),
            tone: "osc1=pwm:2:7 osc1_position=0 osc2=triangle:1:0:25 osc2_octave=1 \
                   filter=lp12:1400:8:30 adsr=8:800:65:500 velocity=55 fx_chorus=25 \
                   fx_reverb=18 volume=-2.1",
            mods: "velocity>cutoff 15",
            macros: [
                "Brightness: cutoff 40",
                "Hollow: pos1 50",
                "Chorus: chorus 45, detune 25",
                "Space: reverb 40, delay 15",
            ],
        },
        Sound {
            name: tr("Poly: Dream"),
            tone: "osc1=harmonics:3:12 osc1_position=35 osc2=sine:1:0:30 osc2_octave=1 \
                   filter=lp12:9000:0:0 adsr=30:1500:60:1200 velocity=50 spread=90 \
                   lfo1=triangle:0.2:free lfo1_depth=60 fx_chorus=30 fx_reverb=35 \
                   fx_reverb_decay=4 fx_delay=15 volume=-6.5",
            mods: "lfo1>pos1 25",
            macros: [
                "Harmonics: pos1 45",
                "Motion: lfo1_depth 100, lfo1_rate 50",
                "Shimmer: osc2_level 45",
                "Space: reverb 35, delay 25",
            ],
        },
    ]
}

fn arps() -> Vec<Sound> {
    vec![
        Sound {
            name: tr("Arp: Bright pluck"),
            tone: "arp=on arp_octaves=2 arp_gate=50 osc1=saw:3:10 osc2=square:1:0:40 \
                   osc2_octave=1 filter=lp24:700:30:70 adsr=1:250:0:150 velocity=40 \
                   fx_delay=25 fx_reverb=15 volume=-5",
            mods: "",
            macros: [
                "Brightness: cutoff 40",
                "Length: decay 45, release 35",
                "Bite: resonance 40",
                "Echo: delay 35, reverb 20",
            ],
        },
        Sound {
            name: tr("Arp: Up and down keys"),
            tone: "arp=on arp_mode=updown arp_note=1/8 arp_octaves=2 arp_gate=80 \
                   osc1=saw:2:8 osc2=sine:1:0:50 osc2_octave=1 filter=lp24:900:10:50 \
                   adsr=3:600:40:300 fx_chorus=30 fx_reverb=20 volume=-6.2",
            mods: "",
            macros: [
                "Brightness: cutoff 40",
                "Chorus: chorus 40, detune 25",
                "Length: decay 40, release 40",
                "Space: reverb 40, delay 25",
            ],
        },
        Sound {
            name: tr("Arp: Random bells"),
            tone: "arp=on arp_mode=random arp_note=1/8 arp_octaves=3 arp_gate=40 \
                   arp_hold=on osc1=sine:1:0 osc2=sine:1:0:0 osc2_octave=2 fm=25 \
                   filter=lp12:14000:0:0 adsr=1:900:0:900 env2=1:400:0:400 spread=70 \
                   fx_delay=30 fx_delay_note=1/8D fx_reverb=30 fx_reverb_decay=5 volume=-6.3",
            mods: "env2>fm 40",
            macros: [
                "Metal: fm 25",
                "Length: decay 40, release 35",
                "Shimmer: osc2_level 25",
                "Space: reverb 35, delay 25",
            ],
        },
        Sound {
            name: tr("Arp: Down sequence"),
            tone: "arp=on arp_mode=down arp_octaves=2 arp_gate=45 osc1=saw:2:6 \
                   filter=lp24:900:25:60 adsr=1:220:0:150 velocity=40 fx_delay=25 \
                   fx_reverb=15 volume=-2.1",
            mods: "",
            macros: [
                "Brightness: cutoff 40",
                "Bite: resonance 45, drive 30",
                "Length: decay 45, release 35",
                "Echo: delay 35, reverb 20",
            ],
        },
        Sound {
            name: tr("Arp: Acid line"),
            tone: "arp=on arp_mode=played arp_octaves=2 arp_gate=55 osc1=saw:1:0 mode=mono \
                   filter=ladder:500:70:60 drive=40 adsr=1:250:30:80 lfo1=sine:0.1:free \
                   fx_delay=20 volume=-1.5",
            mods: "lfo1>cutoff 40",
            macros: [
                "Squelch: cutoff 35",
                "Resonance: resonance 22",
                "Dirt: drive 55",
                "Echo: delay 35, reverb 15",
            ],
        },
        Sound {
            name: tr("Arp: Chime"),
            tone: "arp=on arp_mode=up arp_note=1/8 arp_octaves=3 arp_gate=50 osc1=sine:1:0 \
                   osc2=sine:1:0:30 osc2_octave=2 filter=lp12:14000:0:0 adsr=1:700:0:600 \
                   fx_delay=30 fx_reverb=30 fx_reverb_decay=5 volume=-6.4",
            mods: "",
            macros: [
                "Shimmer: osc2_level 35",
                "Length: decay 40, release 35",
                "Metal: fm 15",
                "Space: reverb 35, delay 25",
            ],
        },
        Sound {
            name: tr("Arp: Pulse bass"),
            tone: "arp=on arp_mode=up arp_note=1/16 arp_octaves=2 arp_gate=60 \
                   osc1=pwm:1:0 osc1_position=25 osc1_octave=-1 sub_level=30 sub_octave=0 mode=mono \
                   filter=lp24:400:25:0 adsr=1:200:20:80 env2=1:120:0:100 env2_depth=70 \
                   velocity=40 spread=0 volume=-1.3",
            mods: "env2>cutoff 50",
            macros: [
                "Brightness: cutoff 40",
                "Bite: resonance 45, env2_depth 60",
                "Dirt: drive 60",
                "Echo: delay 35, reverb 15",
            ],
        },
    ]
}

fn atmospheres() -> Vec<Sound> {
    vec![
        Sound {
            name: tr("Atmosphere: Wind"),
            tone: "osc1=saw:1:0 osc1_level=0 noise=100 filter=bp:700:35:0 key_track=60 \
                   adsr=1500:2000:100:3500 velocity=0 lfo1=sine:0.11:free \
                   lfo2=triangle:0.07:free lfo1_depth=60 fx_reverb=35 fx_reverb_decay=6 \
                   volume=6",
            mods: "lfo1>cutoff 22, lfo2>resonance 15, lfo2>volume 25",
            macros: [
                "Brightness: cutoff 30",
                "Motion: lfo1_depth 100, lfo1_rate 60",
                "Resonance: resonance 35",
                "Space: reverb 40, delay 20",
            ],
        },
        Sound {
            name: tr("Atmosphere: Drone"),
            tone: "osc1=saw:4:10 osc1_octave=-2 osc2=saw:2:8:20 osc2_octave=-1 osc2_semi=7 \
                   filter=lp24:350:30:0 drive=30 adsr=2000:3000:100:5000 velocity=0 \
                   spread=90 drift=40 lfo1=triangle:0.06:free fx_reverb=40 \
                   fx_reverb_decay=8 volume=-3.2",
            mods: "lfo1>cutoff 25",
            macros: [
                "Brightness: cutoff 45",
                "Fifth: osc2_level 50",
                "Growl: drive 60, resonance 30",
                "Space: reverb 40, delay 20",
            ],
        },
        Sound {
            name: tr("Atmosphere: Shimmer"),
            tone: "osc1=triangle:5:18 osc1_octave=1 osc2=sine:1:0:40 osc2_octave=2 \
                   filter=hp:500:10:0 adsr=2000:3000:90:5000 velocity=20 spread=100 \
                   drift=30 lfo1=sine:0.17:free fx_delay=25 fx_delay_note=1/4 \
                   fx_delay_feedback=55 fx_reverb=50 fx_reverb_decay=9 volume=-5",
            mods: "lfo1>osc2_level 40",
            macros: [
                "Shimmer: osc2_level 40, fm 8",
                "Motion: lfo1_rate 67, lfo1_depth 60",
                "Air: noise 12",
                "Space: reverb 35, delay 25",
            ],
        },
        Sound {
            name: tr("Atmosphere: Dark matter"),
            tone: "osc1=fold:3:14 osc1_octave=-2 osc1_position=15 osc2=sine:1:0:0 \
                   osc2_semi=1 ring=30 sub_level=40 filter=lp24:500:35:0 \
                   adsr=2000:3000:100:5000 velocity=0 spread=90 drift=40 \
                   lfo1=triangle:0.05:free lfo2=sine:0.09:free fx_reverb=45 \
                   fx_reverb_decay=9 volume=-10.1",
            mods: "lfo1>pos1 30, lfo2>cutoff 20",
            macros: [
                "Fold: pos1 50",
                "Brightness: cutoff 45",
                "Motion: lfo1_rate 67, lfo2_rate 67",
                "Space: reverb 40, delay 25",
            ],
        },
        Sound {
            name: tr("Atmosphere: Ocean"),
            tone: "osc1=sine:1:0 osc1_octave=-2 osc1_level=15 noise=70 filter=lp12:900:5:0 \
                   adsr=1500:2000:100:4000 velocity=0 lfo1=sine:0.09:free fx_reverb=30 \
                   fx_reverb_decay=5 volume=1.4",
            mods: "lfo1>cutoff 40, lfo1>volume 35",
            macros: [
                "Brightness: cutoff 35",
                "Speed: lfo1_rate 60",
                "Body: osc1_level 40",
                "Space: reverb 40, delay 20",
            ],
        },
        Sound {
            name: tr("Atmosphere: Ghost choir"),
            tone: "osc1=vocal:6:20 osc1_octave=1 osc1_position=60 noise=4 \
                   filter=hp:400:10:0 adsr=1800:3000:90:5000 velocity=20 spread=100 \
                   drift=40 lfo1=triangle:0.07:free fx_delay=20 fx_delay_note=1/4 \
                   fx_reverb=55 fx_reverb_decay=10 volume=-2",
            mods: "lfo1>pos1 35",
            macros: [
                "Vowel: pos1 40",
                "Air: noise 14",
                "Motion: lfo1_rate 67, lfo1_depth 60",
                "Space: reverb 30, delay 25",
            ],
        },
    ]
}

fn effects() -> Vec<Sound> {
    vec![
        Sound {
            name: tr("FX: Noise sweep"),
            tone: "osc1=saw:5:20 osc1_level=0 noise=100 filter=lp24:300:40:0 \
                   adsr=20:100:100:800 env2=2000:10:100:1500 velocity=0 fx_reverb=25 \
                   volume=6",
            mods: "env2>cutoff 70",
            macros: [
                "Brightness: cutoff 30",
                "Resonance: resonance 40",
                "Body: osc1_level 15",
                "Space: reverb 40, delay 25",
            ],
        },
        Sound {
            name: tr("FX: Riser"),
            tone: "osc1=saw:7:25 noise=20 filter=lp24:500:20:0 adsr=30:100:100:400 \
                   env2=4000:10:100:400 velocity=0 spread=100 fx_reverb=30 volume=-9.9",
            mods: "env2>pitch 100, env2>cutoff 40",
            macros: [
                "Brightness: cutoff 40",
                "Detune: detune 50",
                "Air: noise 40",
                "Space: reverb 40, delay 25",
            ],
        },
        Sound {
            name: tr("FX: Laser"),
            tone: "osc1=saw:1:0 osc1_octave=1 osc2=sine:1:0:0 osc2_octave=-1 \
                   filter=lp12:9000:20:0 adsr=1:200:0:120 env2=1:160:0:120 mode=mono \
                   velocity=30 fx_delay=30 fx_delay_feedback=50 volume=-1.9",
            mods: "env2>pitch 100",
            macros: [
                "Length: decay 50, release 40",
                "Pitch: pitch 50",
                "Metal: fm 30",
                "Echo: delay 35, reverb 20",
            ],
        },
        Sound {
            name: tr("FX: Alarm"),
            tone: "osc1=square:1:0 filter=lp12:5000:10:0 adsr=2:100:100:80 mode=mono \
                   velocity=0 lfo1=square:2 lfo1_depth=60 fx_reverb=12 volume=-11.7",
            mods: "lfo1>pitch 40",
            macros: [
                "Speed: lfo1_rate 67",
                "Depth: lfo1_depth 100",
                "Dirt: drive 70",
                "Echo: delay 40, reverb 20",
            ],
        },
        Sound {
            name: tr("FX: Sci-fi"),
            tone: "osc1=sine:1:0 osc2=sine:1:0:0 osc2_octave=1 osc2_semi=3 \
                   filter=lp12:8000:20:0 adsr=5:200:100:400 mode=mono velocity=0 \
                   lfo1=random:8 lfo1_depth=60 lfo2=sine:0.3:free fx_delay=35 \
                   fx_delay_feedback=55 fx_reverb=30 volume=-7.1",
            mods: "lfo1>pitch 60, lfo2>cutoff 25",
            macros: [
                "Speed: lfo1_rate 60",
                "Depth: lfo1_depth 100",
                "Metal: fm 40",
                "Space: reverb 35, delay 25",
            ],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use audiostrada_dsp::MidiEvent;

    const RATE: usize = 48_000;
    /// Seconds a note is held, then heard after its release.
    const HELD: f32 = 1.5;
    const AFTER: f32 = 1.5;
    const ALL_UP: [(&str, f32); 4] = [
        ("macro1", 100.0),
        ("macro2", 100.0),
        ("macro3", 100.0),
        ("macro4", 100.0),
    ];

    /// The keys a sound is tried with: a low note for a bass, one note for a voice that plays
    /// one at a time, else a chord around the middle of the keyboard.
    fn keys_of(preset: &Preset) -> Vec<u8> {
        let low = ["Bass", "808", "bass"]
            .iter()
            .any(|w| preset.name.contains(w));
        let mono = preset.params.get("mode").is_some_and(|m| *m >= 0.5);
        match (low, mono) {
            (true, _) => vec![36],
            (false, true) => vec![60],
            (false, false) => vec![57, 60, 64],
        }
    }

    /// A sound played: held, then released, with some settings changed.
    fn play(preset: &Preset, changed: &[(&str, f32)]) -> (Vec<f32>, Vec<f32>) {
        let info = audiostrada_dsp::info("synth").unwrap();
        let mut synth = audiostrada_dsp::create("synth").unwrap();
        for (i, p) in info.params.iter().enumerate() {
            let value = changed
                .iter()
                .find(|c| c.0 == p.id)
                .map(|c| c.1)
                .or_else(|| preset.params.get(p.id).copied())
                .unwrap_or(p.default);
            synth.set_param(i, value);
        }
        synth.prepare(RATE as u32, 512);
        synth.set_tempo(120.0);
        let keys = keys_of(preset);
        for &key in &keys {
            synth.midi(&MidiEvent::note_on(0, 0, key, 100));
        }
        let (mut left, mut right) = (Vec::new(), Vec::new());
        let blocks = |seconds: f32| (seconds * RATE as f32 / 512.0) as usize;
        for block in 0..blocks(HELD + AFTER) {
            if block == blocks(HELD) {
                for &key in &keys {
                    synth.midi(&MidiEvent::note_off(0, 0, key));
                }
            }
            let (mut l, mut r) = (vec![0.0f32; 512], vec![0.0f32; 512]);
            synth.process(&mut l, &mut r);
            left.extend(l);
            right.extend(r);
        }
        (left, right)
    }

    fn db(power: f64) -> f64 {
        10.0 * power.max(1e-12).log10()
    }

    fn power(x: &[f32]) -> f64 {
        x.iter().map(|s| f64::from(s * s)).sum::<f64>() / x.len().max(1) as f64
    }

    /// The loudest 100 ms of both sides, in dB: plucks are judged on their attack.
    fn loudest(left: &[f32], right: &[f32]) -> f64 {
        left.chunks(4800)
            .zip(right.chunks(4800))
            .map(|(l, r)| db((power(l) + power(r)) / 2.0))
            .fold(f64::MIN, f64::max)
    }

    /// Power in each octave from 20 Hz (eleven of them) of a stretch of sound.
    fn bands(x: &[f32]) -> [f64; 11] {
        const N: usize = 4096;
        let mut out = [0.0f64; 11];
        let mut frames = 0;
        let mut at = 0;
        while at + N <= x.len() {
            let mut re: Vec<f64> = x[at..at + N]
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    let w = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / N as f64).cos();
                    f64::from(*s) * w
                })
                .collect();
            let mut im = vec![0.0f64; N];
            fft(&mut re, &mut im);
            for k in 1..N / 2 {
                let hz = k as f64 * RATE as f64 / N as f64;
                let band = (hz / 20.0).log2().floor();
                if (0.0..11.0).contains(&band) {
                    out[band as usize] += (re[k] * re[k] + im[k] * im[k]) / (N * N) as f64 * 8.0;
                }
            }
            frames += 1;
            at += N / 2;
        }
        out.map(|p| p / f64::from(frames.max(1)))
    }

    /// How a stretch of sound moves: the spread of its level (tremolo, wobble, beating), in dB
    /// over slices of 10 ms; how often that level swings, in Hz; and the spread of its pitch
    /// (vibrato, sweeps), in percent, from the rising zero crossings of slices of 40 ms.
    fn movement(x: &[f32]) -> [f64; 3] {
        let spread = |values: &[f64]| {
            let mean = values.iter().sum::<f64>() / values.len().max(1) as f64;
            let var = values.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>()
                / values.len().max(1) as f64;
            (mean, var.sqrt())
        };
        let levels: Vec<f64> = x
            .as_chunks::<480>()
            .0
            .iter()
            .map(|c| db(power(c)).max(-60.0))
            .collect();
        let (mean, level_spread) = spread(&levels);
        let swings = levels
            .windows(2)
            .filter(|w| w[0] < mean && w[1] >= mean)
            .count() as f64
            / (x.len().max(1) as f64 / RATE as f64);
        let pitches: Vec<f64> = x
            .as_chunks::<1920>()
            .0
            .iter()
            .filter_map(|c| {
                // Where the wave rises through zero, between its samples.
                let rises: Vec<f64> = c
                    .windows(2)
                    .enumerate()
                    .filter(|(_, w)| w[0] < 0.0 && w[1] >= 0.0)
                    .map(|(i, w)| i as f64 + f64::from(w[0] / (w[0] - w[1])))
                    .collect();
                let (first, last) = (rises.first()?, rises.last()?);
                (rises.len() > 2).then(|| (rises.len() - 1) as f64 / (last - first))
            })
            .collect();
        let (pitch, pitch_spread) = spread(&pitches);
        [
            level_spread,
            if level_spread > 1.0 { swings } else { 0.0 },
            100.0 * pitch_spread / pitch.max(1e-9),
        ]
    }

    fn fft(re: &mut [f64], im: &mut [f64]) {
        let n = re.len();
        let mut j = 0;
        for i in 1..n {
            let mut bit = n >> 1;
            while j & bit != 0 {
                j ^= bit;
                bit >>= 1;
            }
            j ^= bit;
            if i < j {
                re.swap(i, j);
                im.swap(i, j);
            }
        }
        let mut len = 2;
        while len <= n {
            let angle = -std::f64::consts::TAU / len as f64;
            for start in (0..n).step_by(len) {
                for k in 0..len / 2 {
                    let (c, s) = ((angle * k as f64).cos(), (angle * k as f64).sin());
                    let (a, b) = (start + k, start + k + len / 2);
                    let (tr, ti) = (re[b] * c - im[b] * s, re[b] * s + im[b] * c);
                    re[b] = re[a] - tr;
                    im[b] = im[a] - ti;
                    re[a] += tr;
                    im[a] += ti;
                }
            }
            len *= 2;
        }
    }

    /// The stretches of a note a sound is heard over: its start, its body, and its tail once
    /// released.
    const STRETCHES: [(f32, f32); 5] =
        [(0.0, 0.15), (0.15, 0.6), (0.6, 1.5), (1.5, 2.0), (2.0, 3.0)];

    /// What is heard of a sound over each stretch: the octaves of what both sides share and
    /// the level of what differs between them, in dB, then how the level and the pitch move.
    fn heard(left: &[f32], right: &[f32]) -> Vec<(Vec<f64>, [f64; 3])> {
        let mid: Vec<f32> = left.iter().zip(right).map(|(l, r)| 0.5 * (l + r)).collect();
        let side: Vec<f32> = left.iter().zip(right).map(|(l, r)| 0.5 * (l - r)).collect();
        let at = |seconds: f32| ((seconds * RATE as f32) as usize).min(mid.len());
        STRETCHES
            .iter()
            .map(|&(from, to)| {
                let (a, b) = (at(from), at(to));
                let mut levels: Vec<f64> = bands(&mid[a..b]).iter().map(|p| db(*p)).collect();
                levels.push(db(power(&side[a..b])));
                (levels, movement(&mid[a..b]))
            })
            .collect()
    }

    /// How far a macro turned all the way takes a sound from itself: the largest change of
    /// anything heard, in dB. What lies 45 dB under the loudest octave is not heard; a level
    /// that starts moving counts for three times its spread, a pitch for three times its percents (a
    /// vibrato of a third of a semitone reads 6).
    fn reach(preset: &Preset, m: usize) -> f64 {
        let (l0, r0) = play(preset, &[]);
        let id = format!("macro{}", m + 1);
        let (l1, r1) = play(preset, &[(id.as_str(), 100.0)]);
        let (before, after) = (heard(&l0, &r0), heard(&l1, &r1));
        let loudest = before
            .iter()
            .chain(&after)
            .flat_map(|s| s.0.iter().copied())
            .fold(f64::MIN, f64::max);
        let floor = loudest - 45.0;
        let mut reach = 0.0f64;
        for (a, b) in before.iter().zip(&after) {
            for (x, y) in a.0.iter().zip(&b.0) {
                reach = reach.max((x.max(floor) - y.max(floor)).abs());
            }
            // Only where the note sounds in full.
            if a.0.iter().chain(&b.0).any(|l| *l > floor + 30.0) {
                for ((x, y), weight) in a.1.iter().zip(&b.1).zip([3.0, 1.0, 3.0]) {
                    reach = reach.max(weight * (x - y).abs());
                }
            }
        }
        reach
    }

    /// Where the weight of the spectrum sits, in Hz.
    fn centroid(x: &[f32]) -> f64 {
        let b = bands(x);
        let total: f64 = b.iter().sum::<f64>().max(1e-12);
        let mean: f64 = b
            .iter()
            .enumerate()
            .map(|(i, p)| (i as f64 + 0.5) * p)
            .sum::<f64>()
            / total;
        20.0 * 2f64.powf(mean)
    }

    #[test]
    fn every_sound_is_written_right_and_names_its_macros() {
        let presets = factory();
        let mut names: Vec<&str> = presets.iter().map(|p| p.name.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), presets.len(), "two sounds share a name");
        for preset in presets.iter().skip(1) {
            for m in 1..=4 {
                let name = preset.params.get(&format!("macro{m}_name")).copied();
                assert!(name.is_some_and(|n| n >= 1.0), "{}: macro {m}", preset.name);
            }
            assert!(
                crate::effect_editors::preset_category(&preset.name).is_some(),
                "{} has no category",
                preset.name
            );
        }
    }

    /// Loudness of each factory sound, so that none jumps out of the list.
    #[test]
    fn factory_sounds_have_matching_levels() {
        for preset in factory().into_iter().step_by(crate::test_sweep::sample()) {
            let (l, r) = play(&preset, &[]);
            let level = loudest(&l, &r);
            assert!(
                (-24.0..=-16.0).contains(&level),
                "{}: {level:.1} dB RMS",
                preset.name
            );
        }
    }

    /// A macro that changes little is no macro: each one, turned up, is plainly heard, and
    /// with all four up the sound neither clips nor breaks.
    #[test]
    fn every_macro_is_plainly_heard() {
        for preset in factory()
            .into_iter()
            .skip(1)
            .step_by(crate::test_sweep::sample())
        {
            for m in 0..4 {
                let reach = reach(&preset, m);
                assert!(
                    reach >= 4.0,
                    "{}: macro {} moves {reach:.1} dB",
                    preset.name,
                    m + 1
                );
            }
            let (l, r) = play(&preset, &ALL_UP);
            let peak = l.iter().chain(&r).fold(0.0f32, |m, s| m.max(s.abs()));
            assert!(
                peak.is_finite() && peak < 1.0,
                "{}: peak {peak}",
                preset.name
            );
        }
    }

    /// Prints what each sound measures, to tune them by:
    /// `scripts/test.sh -p audiostrada-ui --lib synth_presets::tests::report -- --ignored
    /// --nocapture`.
    #[test]
    #[ignore = "a report to read, not a check"]
    fn report() {
        println!(
            "name | level | volume for -20 | centroid start, held | side-mid | tail | peak and \
             level, all macros | macros"
        );
        for preset in factory().into_iter().skip(1) {
            let (l, r) = play(&preset, &[]);
            let level = loudest(&l, &r);
            let volume = preset.params.get("volume").copied().unwrap_or(-12.0);
            let mid: Vec<f32> = l.iter().zip(&r).map(|(l, r)| 0.5 * (l + r)).collect();
            let side: Vec<f32> = l.iter().zip(&r).map(|(l, r)| 0.5 * (l - r)).collect();
            let held = RATE / 4..(HELD * RATE as f32) as usize;
            let width = db(power(&side[held.clone()])) - db(power(&mid[held.clone()]));
            let tail = db(power(&mid[RATE * 5 / 2..])) - db(power(&mid[held]));
            let (la, ra) = play(&preset, &ALL_UP);
            let peak = la.iter().chain(&ra).fold(0.0f32, |m, s| m.max(s.abs()));
            let reaches: Vec<String> = (0..4)
                .map(|m| format!("{:.0}", reach(&preset, m)))
                .collect();
            println!(
                "{} | {level:.1} | {:.1} | {:.0}, {:.0} | {width:.0} | {tail:.0} | {peak:.2} \
                 ({:+.1}) | {}",
                preset.name,
                f64::from(volume) - 20.0 - level,
                centroid(&mid[..RATE / 8]),
                centroid(&mid[RATE / 2..RATE * 3 / 2]),
                loudest(&la, &ra) - level,
                reaches.join(" ")
            );
        }
    }
}
