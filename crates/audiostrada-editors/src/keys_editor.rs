//! The electric pianos' editor, drawn as the instrument: the leather lid, the brushed metal
//! plate of its controls over the name rail, the felt and the keys, played with the mouse.
//! Every piano has the same build in its own colors, rail and emblem. The simple view keeps what is turned most (the character, the tremolo, the effects,
//! the volume); the advanced view lays out every setting.

use egui::{
    self, Align2, Color32, FontId, Painter, Pos2, Rect, Sense, Stroke, Ui, epaint::RectShape, pos2,
    vec2,
};

use crate::controls::{EditorParams, animate, format_value, from_unit, to_unit};
use crate::gear::{self, KnobStyle, Texture};
use crate::i18n::tr;
use crate::{theme, trf};

/// What the editor remembers between frames.
#[derive(Debug)]
pub struct KeysState {
    /// MIDI note of the keyboard's first C.
    low: i32,
    /// The key held with the mouse.
    mouse: Option<u8>,
    /// Every setting shown, not only the most used.
    pub advanced: bool,
    /// MIDI note of the bass machine's first pad.
    pads: i32,
    /// Until when a key clicked too briefly to be seen held keeps sounding.
    tap_until: f64,
}

impl Default for KeysState {
    fn default() -> Self {
        Self {
            low: 36,
            mouse: None,
            // Development aid: the screenshots of the manual show either view.
            advanced: std::env::var("AUDIOSTRADA_SCREENSHOT_VIEW").is_ok_and(|v| v == "advanced"),
            pads: 24,
            tap_until: 0.0,
        }
    }
}

const OCTAVES: i32 = 5;
const LID: f32 = 128.0;
const THIN_LID: f32 = 34.0;
const RAIL: f32 = 156.0;
const PLATE_ROWS: f32 = 262.0;
const FELT: f32 = 6.0;
const KEYS: f32 = 150.0;
const FRONT: f32 = 16.0;
const CHEEK: f32 = 30.0;

const ENGRAVED: Color32 = Color32::from_rgb(34, 34, 38);
const ALUMINIUM: Color32 = Color32::from_rgb(196, 198, 202);
const BLACK: Color32 = Color32::from_rgb(58, 58, 60);
const BEIGE: Color32 = Color32::from_rgb(196, 180, 146);
/// Mahogany.
const WOOD: Color32 = Color32::from_rgb(152, 66, 42);
/// Screen points per texture pixel of the leather: a fine pebble.
const LEATHER_GRAIN: f32 = 0.7;

/// A group of controls on the plate: its title and its controls (parameter, label).
type Group = (&'static str, &'static [(&'static str, &'static str)]);

/// What an instrument's face is made of.
struct Face {
    /// The name on the badge, after "Strada".
    word: &'static str,
    emblem: Emblem,
    covering: Color32,
    /// Leather or wood.
    material: Texture,
    /// How much the covering's grain shows, from 0 to 1: less on a light covering.
    grain: f32,
    /// The plate's metal, and the ink engraved in it.
    plate: Color32,
    ink: Color32,
    /// The ink of what is printed on the covering.
    print: Color32,
    rail: Rail,
    /// The felt along the keys, top and bottom.
    felt: (Color32, Color32),
    /// A parameter chosen with push buttons on the lid (the sound), and their look.
    selector: Option<(&'static str, Selector)>,
    /// Thin colored lines along the top of the rail.
    stripes: &'static [Color32],
    simple: &'static [Group],
    top: &'static [Group],
    bottom: &'static [Group],
}

/// How the lid's push buttons look.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Selector {
    /// Dark membrane buttons with a lamp that lights.
    Membrane,
    /// Cream keys lit from inside.
    Ivory,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Emblem {
    /// Three tines hanging from their tone bar.
    Tines,
    /// Three reeds clamped side by side, the solder on their tips.
    Reeds,
    /// A tone wheel and its pickup.
    Wheel,
    /// Two stacks of operators, modulators over carriers.
    Operators,
    /// Bars of falling length on their rails.
    Bars,
    /// A sine dropping onto its note and dying away.
    Drop,
    /// A cloud of grains.
    Cloud,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Rail {
    Ribbed,
    /// Covered, with the grilles of the built-in speakers at its ends.
    Grilles,
    /// Covered, the plate across.
    Plain,
}

fn face(id: &str) -> Face {
    match id {
        "grain" => Face {
            word: "GRAIN",
            emblem: Emblem::Cloud,
            covering: Color32::from_rgb(214, 212, 206),
            material: Texture::Plastic,
            grain: 0.6,
            plate: Color32::from_rgb(42, 44, 50),
            ink: Color32::from_rgb(226, 230, 236),
            print: Color32::from_rgb(90, 92, 98),
            rail: Rail::Plain,
            felt: (Color32::from_rgb(40, 42, 48), Color32::from_rgb(20, 22, 26)),
            selector: Some(("source", Selector::Membrane)),
            stripes: &[],
            simple: &GRAIN_SIMPLE,
            top: &GRAIN_TOP,
            bottom: &GRAIN_BOTTOM,
        },
        "bass808" => Face {
            word: "808",
            emblem: Emblem::Drop,
            covering: Color32::from_rgb(70, 72, 76),
            material: Texture::Plastic,
            grain: 1.0,
            plate: Color32::from_rgb(40, 41, 44),
            ink: Color32::from_rgb(232, 226, 210),
            print: Color32::from_rgb(200, 196, 186),
            rail: Rail::Plain,
            felt: (Color32::from_rgb(30, 30, 32), Color32::from_rgb(14, 14, 16)),
            selector: None,
            stripes: &[],
            simple: &BASS_SIMPLE,
            top: &BASS_TOP,
            bottom: &BASS_BOTTOM,
        },
        "fmkeys" => Face {
            word: "FM KEYS",
            emblem: Emblem::Operators,
            covering: Color32::from_rgb(40, 40, 42),
            material: Texture::Plastic,
            grain: 1.0,
            plate: Color32::from_rgb(52, 50, 48),
            ink: Color32::from_rgb(222, 218, 208),
            print: Color32::from_rgb(170, 168, 160),
            rail: Rail::Plain,
            felt: (
                Color32::from_rgb(130, 34, 30),
                Color32::from_rgb(70, 16, 14),
            ),
            selector: Some(("character", Selector::Membrane)),
            stripes: &FM_STRIPES,
            simple: &FM_SIMPLE,
            top: &FM_TOP,
            bottom: &ADVANCED_BOTTOM,
        },
        "mallets" => Face {
            word: "MALLETS",
            emblem: Emblem::Bars,
            covering: Color32::from_rgb(214, 170, 116),
            material: Texture::Wood,
            grain: 0.8,
            plate: ALUMINIUM,
            ink: ENGRAVED,
            print: Color32::from_rgb(90, 60, 30),
            rail: Rail::Plain,
            felt: (Color32::from_rgb(52, 58, 48), Color32::from_rgb(28, 32, 26)),
            selector: Some(("instrument", Selector::Ivory)),
            stripes: &[],
            simple: &MALLETS_SIMPLE,
            top: &MALLETS_TOP,
            bottom: &ADVANCED_BOTTOM,
        },
        "tonewheel" => Face {
            word: "TONEWHEEL",
            emblem: Emblem::Wheel,
            covering: WOOD,
            material: Texture::Wood,
            grain: 1.0,
            plate: Color32::from_rgb(46, 46, 50),
            ink: Color32::from_rgb(226, 222, 212),
            print: Color32::from_rgb(70, 40, 22),
            rail: Rail::Plain,
            felt: (Color32::from_rgb(34, 30, 28), Color32::from_rgb(14, 13, 12)),
            selector: None,
            stripes: &[],
            simple: &ORGAN_SIMPLE,
            top: &ORGAN_TOP,
            bottom: &ORGAN_BOTTOM,
        },
        "reed" => Face {
            word: "REED",
            emblem: Emblem::Reeds,
            covering: BEIGE,
            material: Texture::Leather,
            grain: 0.5,
            plate: ALUMINIUM,
            ink: ENGRAVED,
            print: Color32::from_rgb(70, 60, 44),
            rail: Rail::Grilles,
            felt: (Color32::from_rgb(58, 52, 48), Color32::from_rgb(30, 28, 26)),
            selector: None,
            stripes: &[],
            simple: &REED_SIMPLE,
            top: &REED_TOP,
            bottom: &ADVANCED_BOTTOM,
        },
        _ => Face {
            word: "TINES",
            emblem: Emblem::Tines,
            covering: BLACK,
            material: Texture::Leather,
            grain: 1.0,
            plate: ALUMINIUM,
            ink: ENGRAVED,
            print: Color32::from_rgb(150, 152, 158),
            rail: Rail::Ribbed,
            felt: (
                Color32::from_rgb(150, 26, 30),
                Color32::from_rgb(92, 12, 16),
            ),
            selector: None,
            stripes: &[],
            simple: &SIMPLE,
            top: &ADVANCED_TOP,
            bottom: &ADVANCED_BOTTOM,
        },
    }
}

/// The colored lines of the eighties' digital keyboards.
const FM_STRIPES: [Color32; 2] = [
    Color32::from_rgb(62, 170, 160),
    Color32::from_rgb(214, 132, 56),
];

const BASS_SIMPLE: [Group; 3] = [
    (
        "Sound",
        &[
            ("tune", "Tune"),
            ("punch", "Punch"),
            ("length", "Length"),
            ("click", "Click"),
        ],
    ),
    (
        "Drive",
        &[
            ("drive", "Drive"),
            ("shape", "Distortion"),
            ("clean", "Clean sub"),
        ],
    ),
    ("Playing", &[("glide", "Glide"), ("volume", "Volume")]),
];

const BASS_TOP: [Group; 2] = [
    (
        "Sound",
        &[
            ("tune", "Tune"),
            ("punch", "Punch"),
            ("punch_time", "Punch time"),
            ("length", "Length"),
            ("gate", "Note length"),
            ("release", "Release"),
        ],
    ),
    (
        "Tone",
        &[("body", "Body"), ("click", "Click"), ("cutoff", "Filter")],
    ),
];

const BASS_BOTTOM: [Group; 3] = [
    (
        "Drive",
        &[
            ("drive", "Drive"),
            ("shape", "Distortion"),
            ("clean", "Clean sub"),
        ],
    ),
    (
        "Playing",
        &[
            ("mode", "Mode"),
            ("glide", "Glide"),
            ("dynamics", "Velocity"),
        ],
    ),
    ("Output", &[("volume", "Volume")]),
];

/// The simple view: a few controls that each move several settings of the advanced one.
const GRAIN_SIMPLE: [Group; 4] = [
    (
        "Grains",
        &[("position", "Position"), ("macro:texture", "Texture")],
    ),
    (
        "Motion",
        &[("macro:movement", "Drift"), ("freeze", "Freeze")],
    ),
    (
        "Ambience",
        &[("macro:space", "Space"), ("macro:fade", "Fade")],
    ),
    ("Output", &[("trigger", "Mode"), ("volume", "Volume")]),
];

const GRAIN_TOP: [Group; 3] = [
    (
        "Grains",
        &[
            ("position", "Position"),
            ("spray", "Spray"),
            ("size", "Size"),
            ("density", "Density"),
            ("shape", "Shape"),
            ("reverse", "Reverse"),
        ],
    ),
    (
        "Motion",
        &[
            ("trigger", "Trigger"),
            ("scan", "Speed"),
            ("freeze", "Freeze"),
        ],
    ),
    ("Pitch", &[("tune", "Tune"), ("detune", "Detune")]),
];

const GRAIN_BOTTOM: [Group; 4] = [
    (
        "Sound",
        &[
            ("width", "Width"),
            ("low_cut", "Low cut"),
            ("high_cut", "High cut"),
        ],
    ),
    (
        "Envelope",
        &[
            ("attack", "Attack"),
            ("release", "Release"),
            ("dynamics", "Velocity"),
        ],
    ),
    (
        "Effects",
        &[
            ("chorus", "Chorus"),
            ("delay", "Delay"),
            ("delay_note", "Time"),
            ("delay_feedback", "Feedback"),
            ("reverb", "Reverb"),
            ("reverb_decay", "Decay"),
        ],
    ),
    ("Output", &[("volume", "Volume")]),
];

/// The four colors of a rhythm machine's rows of buttons.
const MACHINE: [Color32; 4] = [
    Color32::from_rgb(206, 58, 44),
    Color32::from_rgb(232, 128, 40),
    Color32::from_rgb(236, 196, 70),
    Color32::from_rgb(232, 226, 204),
];

const FM_SIMPLE: [Group; 4] = [
    (
        "Piano",
        &[
            ("tone", "Tone"),
            ("tine", "Tine"),
            ("decay", "Decay"),
            ("detune", "Detune"),
        ],
    ),
    (
        "Tremolo",
        &[("tremolo_rate", "Speed"), ("tremolo_depth", "Depth")],
    ),
    (
        "Effects",
        &[
            ("phaser", "Phaser"),
            ("chorus", "Chorus"),
            ("delay", "Delay"),
            ("reverb", "Reverb"),
        ],
    ),
    ("Output", &[("volume", "Volume")]),
];

const FM_TOP: [Group; 3] = [
    (
        "Operators",
        &[
            ("tone", "Tone"),
            ("tine", "Tine"),
            ("decay", "Decay"),
            ("damper", "Release"),
            ("detune", "Detune"),
        ],
    ),
    ("Touch", &[("dynamics", "Velocity"), ("curve", "Curve")]),
    ("Tuning", &[("tune", "A4"), ("stretch", "Octave stretch")]),
];

const MALLETS_SIMPLE: [Group; 4] = [
    (
        "Mallets",
        &[
            ("hardness", "Hardness"),
            ("decay", "Decay"),
            ("resonance", "Resonance"),
            ("treble", "Treble"),
        ],
    ),
    (
        "Tremolo",
        &[("tremolo_rate", "Speed"), ("tremolo_depth", "Depth")],
    ),
    (
        "Effects",
        &[
            ("chorus", "Chorus"),
            ("delay", "Delay"),
            ("reverb", "Reverb"),
        ],
    ),
    ("Output", &[("volume", "Volume")]),
];

const MALLETS_TOP: [Group; 3] = [
    (
        "Mallets",
        &[
            ("hardness", "Hardness"),
            ("decay", "Decay"),
            ("damper", "Release"),
            ("resonance", "Resonance"),
            ("noise", "Mechanics"),
        ],
    ),
    ("Touch", &[("dynamics", "Velocity"), ("curve", "Curve")]),
    ("Tuning", &[("tune", "A4"), ("stretch", "Octave stretch")]),
];

const BARS: [(&str, &str); 9] = [
    ("bar_16", "16'"),
    ("bar_5_1_3", "5 1/3'"),
    ("bar_8", "8'"),
    ("bar_4", "4'"),
    ("bar_2_2_3", "2 2/3'"),
    ("bar_2", "2'"),
    ("bar_1_3_5", "1 3/5'"),
    ("bar_1_1_3", "1 1/3'"),
    ("bar_1", "1'"),
];

const ORGAN_SIMPLE: [Group; 5] = [
    ("Drawbars", &BARS),
    (
        "Percussion",
        &[("percussion", "Type"), ("perc_decay", "Decay")],
    ),
    ("Sound", &[("vibrato", "Vibrato"), ("drive", "Drive")]),
    ("Rotary", &[("rotary", "Speed")]),
    ("Output", &[("reverb", "Reverb"), ("volume", "Volume")]),
];

const ORGAN_TOP: [Group; 3] = [
    ("Drawbars", &BARS),
    (
        "Percussion",
        &[
            ("percussion", "Type"),
            ("perc_decay", "Decay"),
            ("perc_soft", "Volume"),
        ],
    ),
    ("Rotary", &[("rotary", "Speed"), ("rotary_width", "Width")]),
];

const ORGAN_BOTTOM: [Group; 5] = [
    (
        "Organ",
        &[
            ("vibrato", "Vibrato"),
            ("click", "Click"),
            ("leakage", "Leak"),
            ("dynamics", "Velocity"),
        ],
    ),
    (
        "Amplifier",
        &[
            ("bass", "Bass"),
            ("treble", "Treble"),
            ("drive", "Drive"),
            ("volume", "Volume"),
        ],
    ),
    (
        "Delay",
        &[
            ("delay", "Level"),
            ("delay_note", "Time"),
            ("delay_feedback", "Feedback"),
        ],
    ),
    ("Reverb", &[("reverb", "Level"), ("reverb_decay", "Decay")]),
    ("Tuning", &[("tune", "A4")]),
];

const REED_SIMPLE: [Group; 4] = [
    (
        "Piano",
        &[
            ("voicing", "Bark"),
            ("hammer", "Hammer"),
            ("treble", "Treble"),
            ("drive", "Drive"),
        ],
    ),
    (
        "Tremolo",
        &[("tremolo_rate", "Speed"), ("tremolo_depth", "Depth")],
    ),
    (
        "Effects",
        &[
            ("phaser", "Phaser"),
            ("chorus", "Chorus"),
            ("delay", "Delay"),
            ("reverb", "Reverb"),
        ],
    ),
    ("Output", &[("speaker", "Speaker"), ("volume", "Volume")]),
];

const REED_TOP: [Group; 4] = [
    (
        "Reeds",
        &[
            ("voicing", "Bark"),
            ("hammer", "Hammer"),
            ("decay", "Decay"),
            ("damper", "Release"),
            ("noise", "Mechanics"),
        ],
    ),
    ("Touch", &[("dynamics", "Velocity"), ("curve", "Curve")]),
    ("Tuning", &[("tune", "A4"), ("stretch", "Octave stretch")]),
    ("Cabinet", &[("speaker", "Speaker")]),
];

const SIMPLE: [Group; 4] = [
    (
        "Piano",
        &[
            ("voicing", "Bark"),
            ("bell", "Bell"),
            ("bass", "Bass"),
            ("treble", "Treble"),
            ("drive", "Drive"),
        ],
    ),
    (
        "Tremolo",
        &[("tremolo_rate", "Speed"), ("tremolo_depth", "Depth")],
    ),
    (
        "Effects",
        &[
            ("phaser", "Phaser"),
            ("chorus", "Chorus"),
            ("delay", "Delay"),
            ("reverb", "Reverb"),
        ],
    ),
    ("Output", &[("volume", "Volume")]),
];

const ADVANCED_TOP: [Group; 3] = [
    (
        "Tines",
        &[
            ("voicing", "Bark"),
            ("bell", "Bell"),
            ("decay", "Decay"),
            ("damper", "Release"),
            ("noise", "Mechanics"),
        ],
    ),
    ("Touch", &[("dynamics", "Velocity"), ("curve", "Curve")]),
    ("Tuning", &[("tune", "A4"), ("stretch", "Octave stretch")]),
];

const ADVANCED_BOTTOM: [Group; 5] = [
    (
        "Amplifier",
        &[
            ("bass", "Bass"),
            ("treble", "Treble"),
            ("drive", "Drive"),
            ("volume", "Volume"),
        ],
    ),
    (
        "Tremolo",
        &[
            ("tremolo", "Type"),
            ("tremolo_rate", "Speed"),
            ("tremolo_depth", "Depth"),
        ],
    ),
    ("Modulation", &[("phaser", "Phaser"), ("chorus", "Chorus")]),
    (
        "Delay",
        &[
            ("delay", "Level"),
            ("delay_note", "Time"),
            ("delay_feedback", "Feedback"),
        ],
    ),
    ("Reverb", &[("reverb", "Level"), ("reverb_decay", "Decay")]),
];

pub fn show(ui: &mut Ui, params: &mut dyn EditorParams, meters: &[f32], state: &mut KeysState) {
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            if ui
                .selectable_label(state.advanced, tr("Advanced"))
                .clicked()
            {
                state.advanced = true;
            }
            if ui.selectable_label(!state.advanced, tr("Simple")).clicked() {
                state.advanced = false;
            }
            ui.add_space(12.0);
            let ink = theme::text_on(theme::MUTE);
            let panic = egui::RichText::new(tr("Panic")).strong().color(ink);
            if ui
                .add(egui::Button::new(panic).fill(theme::MUTE))
                .on_hover_text(tr("Every note off, for a note left stuck"))
                .clicked()
            {
                if let Some(key) = state.mouse.take() {
                    params.play_key([0x80, key, 0]);
                }
                params.panic();
            }
        });
    });
    ui.add_space(4.0);
    if params.info().id == "bass808" {
        machine(ui, params, state);
        return;
    }
    if params.info().id == "grain" {
        grain(ui, params, meters, state);
        return;
    }
    let top = if state.advanced {
        THIN_LID + PLATE_ROWS
    } else {
        LID + RAIL
    };
    let look = face(params.info().id);
    let width = ui.available_width().max(760.0);
    let (face, _) = ui.allocate_exact_size(vec2(width, top + FELT + KEYS + FRONT), Sense::hover());
    let painter = ui.painter_at(face.expand(12.0));
    gear::shadow(&painter, face);
    if state.advanced {
        let lid = Rect::from_min_size(face.min, vec2(width, THIN_LID));
        paint_lid(&painter, lid, &look, false);
        select(ui, &painter, params, lid, &look);
        let rail = Rect::from_min_size(lid.left_bottom(), vec2(width, PLATE_ROWS));
        let plate = paint_rail(&painter, rail, &look, 12.0, 40.0);
        stripes(&painter, rail, &look);
        paint_plate(&painter, plate, look.plate);
        let half = plate.height() / 2.0;
        let rows = [
            (
                Rect::from_min_size(plate.min, vec2(plate.width(), half)),
                look.top,
            ),
            (
                Rect::from_min_size(plate.min + vec2(0.0, half), vec2(plate.width(), half)),
                look.bottom,
            ),
        ];
        painter.line_segment(
            [
                pos2(plate.left() + 14.0, plate.top() + half),
                pos2(plate.right() - 14.0, plate.top() + half),
            ],
            Stroke::new(1.0, Color32::from_black_alpha(70)),
        );
        for (rect, groups) in rows {
            controls(ui, &painter, params, rect, groups, 40.0, look.ink);
        }
    } else {
        let lid = Rect::from_min_size(face.min, vec2(width, LID));
        paint_lid(&painter, lid, &look, true);
        select(ui, &painter, params, lid, &look);
        let rail = Rect::from_min_size(lid.left_bottom(), vec2(width, RAIL));
        let plate = paint_rail(&painter, rail, &look, 14.0, 96.0);
        stripes(&painter, rail, &look);
        paint_plate(&painter, plate, look.plate);
        controls(ui, &painter, params, plate, look.simple, 46.0, look.ink);
    }
    let felt = Rect::from_min_size(face.min + vec2(0.0, top), vec2(width, FELT));
    gear::sheen(&painter, felt, look.felt.0, look.felt.1);
    let bed = Rect::from_min_size(felt.left_bottom(), vec2(width, KEYS));
    keyboard(ui, &painter, params, state, bed, &look);
    let front = Rect::from_min_size(bed.left_bottom(), vec2(width, FRONT));
    leather(&painter, front, 0.0, &look);
    gear::bevel(&painter, front, 0.8);
    light(&painter, face);
}

/// A soft light from above the left of the instrument: brighter there, the far edges and
/// the bottom falling into shade.
fn light(painter: &Painter, face: Rect) {
    let lit = painter.with_clip_rect(face);
    gear::glow(
        &lit,
        pos2(
            face.left() + face.width() * 0.38,
            face.top() - face.height() * 0.15,
        ),
        face.width() * 0.62,
        Color32::from_white_alpha(20),
        Color32::from_white_alpha(0),
    );
    let side = face.width() * 0.16;
    for (edge, dir) in [(face.left(), 1.0), (face.right(), -1.0)] {
        let mut mesh = egui::Mesh::default();
        let inner = edge + side * dir;
        let shade = Color32::from_black_alpha(if dir > 0.0 { 46 } else { 70 });
        mesh.colored_vertex(pos2(edge, face.top()), shade);
        mesh.colored_vertex(pos2(inner, face.top()), Color32::TRANSPARENT);
        mesh.colored_vertex(pos2(inner, face.bottom()), Color32::TRANSPARENT);
        mesh.colored_vertex(pos2(edge, face.bottom()), shade);
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(0, 2, 3);
        lit.add(mesh);
    }
    let low = Rect::from_min_max(
        pos2(face.left(), face.bottom() - face.height() * 0.3),
        face.max,
    );
    gear::sheen(
        &lit,
        low,
        Color32::TRANSPARENT,
        Color32::from_black_alpha(60),
    );
}

/// `rect` covered with the instrument's leather.
fn leather(painter: &Painter, rect: Rect, radius: impl Into<egui::CornerRadius>, look: &Face) {
    let radius = radius.into();
    painter.rect_filled(rect, radius, look.covering);
    textured(
        painter,
        rect,
        radius,
        look.material,
        look.covering.gamma_multiply(look.grain),
    );
}

/// `rect` covered with a texture, its corners rounded by `radius`.
fn textured(
    painter: &Painter,
    rect: Rect,
    radius: impl Into<egui::CornerRadius>,
    which: Texture,
    tint: Color32,
) {
    let handle = gear::texture(painter.ctx(), which);
    let span = 256.0
        * match which {
            Texture::Leather => LEATHER_GRAIN,
            _ => 1.0,
        };
    let uv = Rect::from_min_max(
        pos2(rect.left() / span, rect.top() / span),
        pos2(rect.right() / span, rect.bottom() / span),
    );
    painter.add(RectShape::filled(rect, radius, tint).with_texture(handle.id(), uv));
}

/// Text cut into the metal: dark, with a light edge under it.
fn engrave(painter: &Painter, at: Pos2, text: &str, size: f32, ink: Color32) {
    let font = theme::strong(size);
    // Dark ink on light metal has a lit edge under it; light ink on dark metal a shadow.
    let edge = if is_light(ink) {
        Color32::from_black_alpha(170)
    } else {
        Color32::from_white_alpha(120)
    };
    painter.text(
        at + vec2(0.0, 1.0),
        Align2::CENTER_CENTER,
        text,
        font.clone(),
        edge,
    );
    painter.text(at, Align2::CENTER_CENTER, text, font, ink);
}

fn is_light(color: Color32) -> bool {
    let [r, g, b, _] = color.to_array();
    0.299 * f32::from(r) + 0.587 * f32::from(g) + 0.114 * f32::from(b) > 128.0
}

/// The lid in leather under a spotlight, with the logo.
fn paint_lid(painter: &Painter, lid: Rect, look: &Face, tall: bool) {
    let radius = egui::CornerRadius {
        nw: 18,
        ne: 18,
        sw: 0,
        se: 0,
    };
    leather(painter, lid, radius, look);
    let lit = painter.with_clip_rect(lid.shrink(2.0));
    gear::glow(
        &lit,
        pos2(lid.center().x, lid.top() + lid.height() * 0.25),
        lid.width() * 0.32,
        Color32::from_white_alpha(46),
        Color32::from_white_alpha(0),
    );
    // The lid's front edge, rounded toward the light.
    let edge = Rect::from_min_max(pos2(lid.left(), lid.bottom() - 10.0), lid.max);
    gear::sheen(
        painter,
        edge,
        Color32::from_white_alpha(18),
        Color32::from_black_alpha(110),
    );
    if tall {
        logo(
            painter,
            pos2(lid.left() + 34.0, lid.top() + lid.height() * 0.5),
            40.0,
            look,
            true,
        );
    } else {
        logo(
            painter,
            pos2(lid.left() + 24.0, lid.top() + 13.0),
            18.0,
            look,
            true,
        );
    }
}

/// The face's colored lines along the top of the rail, above the plate.
fn stripes(painter: &Painter, rail: Rect, look: &Face) {
    for (k, color) in look.stripes.iter().enumerate() {
        let y = rail.top() + 3.0 + k as f32 * 4.0;
        painter.line_segment(
            [pos2(rail.left() + 18.0, y), pos2(rail.right() - 18.0, y)],
            Stroke::new(2.0, *color),
        );
    }
}

/// The push buttons on the lid that choose the sound, from the right edge, the chosen one
/// lit.
/// The selector, its choice applied as it is.
fn select(ui: &mut Ui, painter: &Painter, params: &mut dyn EditorParams, lid: Rect, look: &Face) {
    if let (Some((i, response)), Some((id, _))) =
        (selector(ui, painter, params, lid, look), look.selector)
    {
        params.set(id, Some(i as f32), &response);
        params.end_gesture();
    }
}

/// The selector's buttons; the one clicked, if it is not the one lit already, with its
/// response, for the caller to apply.
fn selector(
    ui: &mut Ui,
    painter: &Painter,
    params: &mut dyn EditorParams,
    lid: Rect,
    look: &Face,
) -> Option<(usize, egui::Response)> {
    let (id, style) = look.selector?;
    let p = params.param(id)?;
    let mut picked = None;
    params.shown(id);
    let chosen = params.get(id).round() as usize;
    let tall = lid.height() > 60.0;
    let height = if tall { 30.0 } else { 20.0 };
    let font = theme::strong(if tall { 11.0 } else { 9.5 });
    let widths: Vec<f32> = p
        .choices
        .iter()
        .map(|c| {
            painter
                .layout_no_wrap(tr(c).to_owned(), font.clone(), Color32::WHITE)
                .size()
                .x
                + if tall { 26.0 } else { 18.0 }
        })
        .collect();
    let gap = 6.0;
    let total: f32 = widths.iter().sum::<f32>() + gap * (widths.len() - 1) as f32;
    let y = if tall {
        lid.center().y
    } else {
        lid.top() + 13.0
    };
    let mut x = lid.right() - 30.0 - total;
    for (i, name) in p.choices.iter().enumerate() {
        let rect = Rect::from_min_size(pos2(x, y - height / 2.0), vec2(widths[i], height));
        x += widths[i] + gap;
        let response = ui
            .interact(
                rect,
                ui.scope_id().with(("keys-selector", i)),
                Sense::click(),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        if response.clicked() && i != chosen {
            picked = Some((i, response.clone()));
        }
        let lit = i == chosen;
        painter.rect_filled(
            rect.translate(vec2(0.0, 2.0)),
            4.0,
            Color32::from_black_alpha(130),
        );
        let (cap, ink) = match style {
            Selector::Membrane => (
                if lit {
                    Color32::from_rgb(70, 70, 74)
                } else {
                    Color32::from_rgb(50, 50, 54)
                },
                Color32::from_rgb(224, 222, 214),
            ),
            Selector::Ivory => (
                if lit {
                    Color32::from_rgb(252, 238, 196)
                } else {
                    Color32::from_rgb(214, 206, 186)
                },
                Color32::from_rgb(60, 48, 32),
            ),
        };
        let pressed = rect.translate(vec2(0.0, if lit { 1.0 } else { 0.0 }));
        painter.rect_filled(pressed, 4.0, cap);
        gear::sheen(
            painter,
            Rect::from_min_max(pressed.min, pos2(pressed.right(), pressed.center().y)).shrink(1.5),
            Color32::from_white_alpha(if response.hovered() { 50 } else { 30 }),
            Color32::from_white_alpha(0),
        );
        painter.rect_stroke(
            pressed,
            4.0,
            Stroke::new(1.0, Color32::from_black_alpha(150)),
            egui::StrokeKind::Inside,
        );
        if lit && style == Selector::Ivory {
            gear::glow(
                painter,
                pressed.center(),
                pressed.width() * 0.6,
                Color32::from_rgba_unmultiplied(255, 200, 90, 70),
                Color32::TRANSPARENT,
            );
        }
        let text_at = if style == Selector::Membrane {
            // The lamp at the left, the name after it.
            let lamp = pos2(pressed.left() + 8.0, pressed.center().y);
            let color = if lit {
                Color32::from_rgb(80, 230, 200)
            } else {
                Color32::from_rgb(30, 50, 48)
            };
            if lit {
                gear::glow(
                    painter,
                    lamp,
                    8.0,
                    Color32::from_rgba_unmultiplied(80, 230, 200, 110),
                    Color32::TRANSPARENT,
                );
            }
            painter.circle_filled(lamp, 2.5, color);
            pressed.center() + vec2(4.0, 0.0)
        } else {
            pressed.center()
        };
        painter.text(text_at, Align2::CENTER_CENTER, tr(name), font.clone(), ink);
    }
    picked
}

/// Polished lettering: a shadow, a darker lower half and a bright upper half, as polished
/// metal mirrors a dark floor and a lit ceiling; `(lower, upper)` its two tones.
fn chrome(
    painter: &Painter,
    at: Pos2,
    galley: std::sync::Arc<egui::Galley>,
    (lower, upper_tone): (Color32, Color32),
) {
    let rect = Rect::from_min_size(at, galley.size());
    painter.galley_with_override_text_color(
        at + vec2(1.0, 2.0),
        galley.clone(),
        Color32::from_black_alpha(210),
    );
    painter.galley_with_override_text_color(at, galley.clone(), lower);
    let upper = Rect::from_min_max(rect.min, pos2(rect.right(), rect.center().y + 1.0));
    painter
        .with_clip_rect(upper.intersect(painter.clip_rect()))
        .galley_with_override_text_color(at, galley, upper_tone);
}

/// The instrument's logo from `left`, `height` tall: its emblem, then the name, "Strada" in
/// red; on a black enamel badge in a chrome rim when `badge`. Returns where it was drawn.
fn logo(painter: &Painter, left: Pos2, height: f32, look: &Face, badge: bool) -> Rect {
    let h = height;
    let galley = |text: &str, size: f32, italics: bool, spacing: f32| {
        let mut job = egui::text::LayoutJob::default();
        job.append(
            text,
            0.0,
            egui::TextFormat {
                font_id: if italics {
                    FontId::proportional(size)
                } else {
                    theme::strong(size)
                },
                italics,
                extra_letter_spacing: spacing,
                color: Color32::WHITE,
                ..Default::default()
            },
        );
        painter.layout_job(job)
    };
    let first = galley("Strada", h * 0.46, true, 0.0);
    let second = galley(look.word, h * 0.74, false, h * 0.14);
    let pad = if badge { h * 0.32 } else { 0.0 };
    let emblem = h * 0.82;
    let width = pad + emblem + h * 0.3 + first.size().x + h * 0.16 + second.size().x + pad;
    let area = Rect::from_min_size(left - vec2(0.0, h / 2.0 + pad * 0.5), vec2(width, h + pad));
    if badge {
        painter.rect_filled(
            area.translate(vec2(1.0, 3.0)),
            h * 0.24,
            Color32::from_black_alpha(140),
        );
        painter.rect_filled(area, h * 0.24, Color32::from_rgb(178, 180, 186));
        painter.rect_stroke(
            area.shrink(0.5),
            h * 0.24,
            Stroke::new(1.0, Color32::from_white_alpha(170)),
            egui::StrokeKind::Inside,
        );
        let enamel = area.shrink(3.0);
        painter.rect_filled(enamel, h * 0.2, Color32::from_rgb(14, 14, 16));
        gear::sheen(
            painter,
            Rect::from_min_max(enamel.min, pos2(enamel.right(), enamel.center().y)).shrink(2.0),
            Color32::from_white_alpha(26),
            Color32::from_white_alpha(0),
        );
    }
    let e = Rect::from_center_size(
        pos2(area.left() + pad + emblem / 2.0, area.center().y),
        vec2(emblem, emblem),
    );
    paint_emblem(painter, e, look.emblem);
    let mut x = e.right() + h * 0.3;
    let baseline = |g: &egui::Galley| area.center().y - g.size().y / 2.0;
    chrome(
        painter,
        pos2(x, baseline(&second) + h * 0.05),
        first.clone(),
        (
            Color32::from_rgb(170, 28, 32),
            Color32::from_rgb(232, 66, 60),
        ),
    );
    x += first.size().x + h * 0.16;
    chrome(
        painter,
        pos2(x, baseline(&second)),
        second,
        (
            Color32::from_rgb(132, 134, 140),
            Color32::from_rgb(238, 240, 245),
        ),
    );
    area
}

/// The emblem in `e`, in polished metal.
fn paint_emblem(painter: &Painter, e: Rect, emblem: Emblem) {
    let metal = Color32::from_rgb(214, 216, 222);
    let dark = Color32::from_rgb(120, 122, 128);
    let h = e.height();
    match emblem {
        Emblem::Tines => {
            // A tone bar and three tines of falling length.
            let bar = Rect::from_min_size(e.min, vec2(e.width(), h * 0.16));
            gear::sheen(painter, bar, metal, dark);
            for (k, length) in [1.0f32, 0.8, 0.6].into_iter().enumerate() {
                let x = e.left() + e.width() * (0.2 + 0.3 * k as f32);
                let bottom = bar.bottom() + h * 0.78 * length;
                painter.line_segment(
                    [pos2(x, bar.bottom()), pos2(x, bottom)],
                    Stroke::new((h * 0.085).max(1.5), metal),
                );
                painter.circle_filled(pos2(x, bottom), h * 0.07, metal);
            }
        }
        Emblem::Wheel => {
            // A tone wheel's wavy rim around its hub, its pickup beside it.
            let c = pos2(e.left() + h * 0.42, e.center().y);
            let r = h * 0.4;
            let rim: Vec<Pos2> = (0..64)
                .map(|k| {
                    let a = k as f32 / 64.0 * std::f32::consts::TAU;
                    c + vec2(a.cos(), a.sin()) * r * (1.0 + 0.09 * (8.0 * a).sin())
                })
                .collect();
            painter.add(egui::Shape::convex_polygon(rim, metal, Stroke::NONE));
            painter.circle_filled(c, r * 0.32, dark);
            let pickup = Rect::from_min_size(
                pos2(c.x + r * 1.15, c.y - h * 0.08),
                vec2(e.right() - c.x - r * 1.15, h * 0.16),
            );
            gear::sheen(painter, pickup, metal, dark);
        }
        Emblem::Operators => {
            // Two columns of operators, each modulator feeding the carrier under it.
            let size = h * 0.3;
            for col in [0.22f32, 0.72] {
                let x = e.left() + e.width() * col;
                let top = Rect::from_center_size(pos2(x, e.top() + h * 0.22), vec2(size, size));
                let low = Rect::from_center_size(pos2(x, e.bottom() - h * 0.22), vec2(size, size));
                painter.line_segment(
                    [top.center_bottom(), low.center_top()],
                    Stroke::new((h * 0.06).max(1.0), metal),
                );
                painter.rect_stroke(
                    top,
                    1.5,
                    Stroke::new((h * 0.06).max(1.0), metal),
                    egui::StrokeKind::Inside,
                );
                painter.rect_filled(low, 1.5, metal);
            }
            painter.line_segment(
                [
                    pos2(e.left() + e.width() * 0.22, e.bottom()),
                    pos2(e.left() + e.width() * 0.72, e.bottom()),
                ],
                Stroke::new((h * 0.06).max(1.0), dark),
            );
        }
        Emblem::Drop => {
            // A sine starting fast and wide, slowing onto its note and dying away.
            let mut points = Vec::with_capacity(64);
            let mut phase = 0.0f32;
            for i in 0..64 {
                let t = i as f32 / 63.0;
                phase += 0.05 + 0.35 * (-t * 6.0).exp();
                let level = (-t * 2.2).exp();
                points.push(pos2(
                    e.left() + t * e.width(),
                    e.center().y - (phase * std::f32::consts::TAU).sin() * level * h * 0.45,
                ));
            }
            painter.add(egui::Shape::line(
                points,
                Stroke::new((h * 0.08).max(1.5), metal),
            ));
        }
        Emblem::Cloud => {
            // Grains scattered in a cloud, bigger toward its middle.
            let mut seed = 7u32;
            for _ in 0..22 {
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                let a = (seed >> 8) as f32 / (1 << 24) as f32 * std::f32::consts::TAU;
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                let d = ((seed >> 8) as f32 / (1 << 24) as f32).sqrt();
                let c = e.center() + vec2(a.cos() * e.width() * 0.48, a.sin() * h * 0.42) * d;
                painter.circle_filled(c, h * (0.09 - 0.05 * d), metal);
            }
        }
        Emblem::Bars => {
            // Four bars of falling length.
            for k in 0..4 {
                let x = e.left() + e.width() * (0.12 + 0.25 * k as f32);
                let length = h * (0.95 - 0.16 * k as f32);
                let bar =
                    Rect::from_center_size(pos2(x, e.center().y), vec2(e.width() * 0.17, length));
                gear::sheen(painter, bar, metal, dark);
            }
        }
        Emblem::Reeds => {
            // A clamp and three reeds of falling length, tapering, the solder on their tips.
            let clamp = Rect::from_min_size(e.min, vec2(e.width() * 0.2, h));
            gear::sheen(painter, clamp, metal, dark);
            for (k, length) in [1.0f32, 0.82, 0.64].into_iter().enumerate() {
                let y = e.top() + h * (0.2 + 0.3 * k as f32);
                let start = clamp.right();
                let end = start + (e.right() - start) * length;
                let thick = (h * 0.1).max(1.5);
                painter.add(egui::Shape::convex_polygon(
                    vec![
                        pos2(start, y - thick / 2.0),
                        pos2(end, y - thick / 4.0),
                        pos2(end, y + thick / 4.0),
                        pos2(start, y + thick / 2.0),
                    ],
                    metal,
                    Stroke::NONE,
                ));
                painter.circle_filled(pos2(end - h * 0.06, y), h * 0.075, metal);
            }
        }
    }
}

/// The name rail and what is on it; returns where its plate goes, `inset` from its top and
/// bottom, `grilles` wide at each end when it has speaker grilles.
fn paint_rail(painter: &Painter, rail: Rect, look: &Face, inset: f32, grilles: f32) -> Rect {
    match look.rail {
        Rail::Ribbed => {
            paint_ribs(painter, rail);
            rail.shrink2(vec2(18.0, inset))
        }
        Rail::Plain => {
            leather(painter, rail, 0.0, look);
            gear::sheen(
                painter,
                rail,
                Color32::from_white_alpha(14),
                Color32::from_black_alpha(60),
            );
            rail.shrink2(vec2(18.0, inset))
        }
        Rail::Grilles => {
            leather(painter, rail, 0.0, look);
            gear::sheen(
                painter,
                rail,
                Color32::from_white_alpha(14),
                Color32::from_black_alpha(60),
            );
            for x in [rail.left() + 16.0, rail.right() - 16.0 - grilles] {
                let grille = Rect::from_min_size(
                    pos2(x, rail.top() + inset),
                    vec2(grilles, rail.height() - 2.0 * inset),
                );
                painter.rect_filled(grille.expand(2.0), 6.0, Color32::from_black_alpha(70));
                textured(
                    painter,
                    grille,
                    5.0,
                    Texture::Perforated,
                    Color32::from_gray(96),
                );
                gear::sheen(
                    painter,
                    grille,
                    Color32::from_white_alpha(20),
                    Color32::from_black_alpha(60),
                );
                for c in [
                    grille.center_top() + vec2(0.0, 7.0),
                    grille.center_bottom() - vec2(0.0, 7.0),
                ] {
                    painter.circle_filled(c, 3.0, Color32::from_rgb(40, 38, 36));
                }
            }
            rail.shrink2(vec2(16.0 + grilles + 14.0, inset))
        }
    }
}

/// Ribbed metal across the rail.
fn paint_ribs(painter: &Painter, rail: Rect) {
    painter.rect_filled(rail, 0.0, Color32::from_rgb(198, 200, 204));
    let mut y = rail.top() + 1.0;
    while y < rail.bottom() {
        painter.line_segment(
            [pos2(rail.left(), y), pos2(rail.right(), y)],
            Stroke::new(1.2, Color32::from_white_alpha(150)),
        );
        painter.line_segment(
            [pos2(rail.left(), y + 1.8), pos2(rail.right(), y + 1.8)],
            Stroke::new(1.0, Color32::from_black_alpha(80)),
        );
        y += 3.6;
    }
    gear::sheen(
        painter,
        rail,
        Color32::from_white_alpha(10),
        Color32::from_black_alpha(70),
    );
    gear::bevel(painter, rail, 0.7);
}

/// The brushed aluminium plate, screwed on the rail.
fn paint_plate(painter: &Painter, plate: Rect, metal: Color32) {
    painter.rect_filled(
        plate.translate(vec2(1.0, 3.0)),
        7.0,
        Color32::from_black_alpha(110),
    );
    textured(painter, plate, 7.0, Texture::Satin, metal);
    gear::sheen(
        painter,
        plate,
        Color32::from_white_alpha(40),
        Color32::from_black_alpha(30),
    );
    painter.rect_stroke(
        plate,
        7.0,
        Stroke::new(1.0, Color32::from_black_alpha(130)),
        egui::StrokeKind::Inside,
    );
    for c in [
        plate.left_top() + vec2(9.0, 9.0),
        plate.right_top() + vec2(-9.0, 9.0),
        plate.left_bottom() + vec2(9.0, -9.0),
        plate.right_bottom() + vec2(-9.0, -9.0),
    ] {
        painter.circle_filled(c, 3.5, Color32::from_rgb(150, 152, 156));
        painter.circle_stroke(c, 3.5, Stroke::new(0.8, Color32::from_black_alpha(140)));
        painter.line_segment(
            [c + vec2(-2.5, -1.0), c + vec2(2.5, 1.0)],
            Stroke::new(1.0, Color32::from_black_alpha(160)),
        );
    }
}

/// The groups of controls across `rect`, each titled, apart by engraved lines.
fn controls(
    ui: &mut Ui,
    painter: &Painter,
    params: &mut dyn EditorParams,
    rect: Rect,
    groups: &[Group],
    diameter: f32,
    ink: Color32,
) {
    let gap = 18.0;
    let inner = rect.shrink2(vec2(22.0, 0.0));
    let count: usize = groups.iter().map(|g| g.1.len()).sum();
    let cell = (inner.width() - gap * (groups.len() - 1) as f32) / count.max(1) as f32;
    let mut x = inner.left();
    let title_y = rect.top() + 15.0;
    let knob_y = rect.top() + 52.0 + diameter / 2.0;
    for (k, (title, list)) in groups.iter().enumerate() {
        let span = cell * list.len() as f32;
        engrave(
            painter,
            pos2(x + span / 2.0, title_y),
            &tr(title).to_uppercase(),
            11.0,
            ink,
        );
        for (i, (id, label)) in list.iter().enumerate() {
            let center = pos2(x + cell * (i as f32 + 0.5), knob_y);
            if id.starts_with("bar_") {
                let span = (rect.top() + 30.0, rect.bottom() - 8.0);
                drawbar(ui, painter, params, (id, label), center.x, span, ink);
            } else if let Some(name) = id.strip_prefix("macro:") {
                macro_control(
                    ui,
                    painter,
                    params,
                    (name, tr(label)),
                    center,
                    diameter,
                    ink,
                );
            } else {
                control(ui, painter, params, (id, tr(label)), center, diameter, ink);
            }
        }
        x += span;
        if k + 1 < groups.len() {
            let line = x + gap / 2.0;
            for (dx, color) in [
                (0.0, Color32::from_black_alpha(70)),
                (1.0, Color32::from_white_alpha(90)),
            ] {
                painter.line_segment(
                    [
                        pos2(line + dx, rect.top() + 12.0),
                        pos2(line + dx, rect.bottom() - 12.0),
                    ],
                    Stroke::new(1.0, color),
                );
            }
            x += gap;
        }
    }
}

/// A skirted knob for a setting (stepping through the names of a list), or a toggle switch
/// for a choice of two; its label above, its value below.
fn control(
    ui: &mut Ui,
    painter: &Painter,
    params: &mut dyn EditorParams,
    (id, label): (&str, &str),
    center: Pos2,
    diameter: f32,
    ink: Color32,
) {
    let Some(p) = params.param(id) else {
        return;
    };
    params.shown(id);
    let r = diameter / 2.0;
    engrave(painter, center - vec2(0.0, r + 15.0), label, 10.0, ink);
    let value = params.get(id);
    let rect = Rect::from_center_size(center, vec2(diameter + 14.0, diameter + 14.0));
    let response = ui.interact(
        rect,
        ui.scope_id().with(("keys-control", id)),
        Sense::click_and_drag(),
    );
    let count = p.choices.len();
    let changed = if count == 2 {
        response.clicked().then_some(if value >= p.min + 0.5 {
            p.min
        } else {
            p.min + 1.0
        })
    } else {
        let mut t = to_unit(p, value);
        let fine = ui.input(|i| i.modifiers.shift);
        // A list moves a step every 18 points of drag, a knob over 150.
        let steps = count.saturating_sub(1) as f32;
        let travel = if count > 0 { 18.0 * steps } else { 150.0 };
        // The drag is added up between frames: a list's small moves reach the next step.
        let held = ui.scope_id().with(("keys-drag", id));
        if response.drag_started() {
            ui.data_mut(|d| d.insert_temp(held, t));
        }
        if response.dragged() {
            let mut at = ui.data(|d| d.get_temp::<f32>(held)).unwrap_or(t);
            at -= response.drag_delta().y / travel * if fine { 0.15 } else { 1.0 };
            at = at.clamp(0.0, 1.0);
            ui.data_mut(|d| d.insert_temp(held, at));
            t = at;
        }
        if response.drag_stopped() {
            ui.data_mut(|d| d.remove::<f32>(held));
        }
        if response.hovered() {
            let wheel = ui.input(|i| i.smooth_scroll_delta.y);
            if wheel != 0.0 {
                let step = if count > 0 {
                    1.0 / steps.max(1.0)
                } else if fine {
                    0.002
                } else {
                    0.02
                };
                t += wheel.signum() * step;
            }
        }
        if response.double_clicked() {
            t = to_unit(p, p.default);
        }
        let mut v = from_unit(p, t.clamp(0.0, 1.0));
        if count > 0 {
            v = v.round();
        }
        (v != value).then_some(v)
    };
    let shown = changed.unwrap_or(value);
    let text = format_value(p, shown);
    let response = response.on_hover_text(trf!(
        "{name}: {value}. Drag, Shift for fine, double-click to reset",
        name = tr(p.name),
        value = text.clone()
    ));
    params.set(id, changed, &response);
    if count == 2 && changed.is_some() {
        params.end_gesture();
    }
    if count == 2 {
        toggle(painter, center, shown >= p.min + 0.5);
    } else {
        let style = if params.info().id == "bass808" {
            KnobStyle::Knurled
        } else {
            KnobStyle::Skirted
        };
        gear::knob(painter, center, diameter, to_unit(p, shown), style, ink);
    }
    let below = if count > 0 {
        tr(p.choices[(shown - p.min).round().clamp(0.0, count as f32 - 1.0) as usize]).to_owned()
    } else {
        text
    };
    painter.text(
        center + vec2(0.0, r + 14.0),
        Align2::CENTER_CENTER,
        below,
        FontId::monospace(10.0),
        ink.gamma_multiply(0.8),
    );
}

/// A drawbar: its footage above its slot, the bar pulled out from 0 to 8 with the numbers
/// on its shaft, the tab at its end (brown for the subharmonics, black for the odd ones).
/// Click or drag where the tab should go.
fn drawbar(
    ui: &mut Ui,
    painter: &Painter,
    params: &mut dyn EditorParams,
    (id, label): (&str, &str),
    x: f32,
    (top, bottom): (f32, f32),
    ink: Color32,
) {
    let Some(p) = params.param(id) else {
        return;
    };
    params.shown(id);
    engrave(painter, pos2(x, top + 7.0), label, 9.5, ink);
    let tab = vec2(22.0, 20.0);
    let slot = top + 18.0;
    let step = ((bottom - slot - tab.y) / 8.0).max(4.0);
    let value = params.get(id).round().clamp(0.0, 8.0);
    let area = Rect::from_min_max(pos2(x - 14.0, slot - 6.0), pos2(x + 14.0, bottom));
    let response = ui.interact(
        area,
        ui.scope_id().with(("keys-drawbar", id)),
        Sense::click_and_drag(),
    );
    let mut wanted = value;
    if (response.dragged() || response.clicked())
        && let Some(pos) = response.interact_pointer_pos()
    {
        wanted = ((pos.y - slot - tab.y / 2.0) / step)
            .round()
            .clamp(0.0, 8.0);
    }
    if response.hovered() {
        let wheel = ui.input(|i| i.smooth_scroll_delta.y);
        if wheel != 0.0 {
            wanted = (value - wheel.signum()).clamp(0.0, 8.0);
        }
    }
    if response.double_clicked() {
        wanted = p.default;
    }
    let changed = (wanted != value).then_some(wanted);
    let response = response.on_hover_text(trf!(
        "{name}: {value}. Drag, Shift for fine, double-click to reset",
        name = tr(p.name),
        value = format!("{}", wanted as i32)
    ));
    params.set(id, changed, &response);
    if response.clicked() && !response.dragged() {
        params.end_gesture();
    }
    let shown = wanted;
    // The slot, then the shaft out of it with its numbers.
    let mouth = Rect::from_center_size(pos2(x, slot), vec2(20.0, 6.0));
    painter.rect_filled(mouth, 2.0, Color32::from_rgb(8, 8, 10));
    let end = slot + shown * step;
    if shown > 0.0 {
        let shaft = Rect::from_min_max(pos2(x - 6.0, slot), pos2(x + 6.0, end));
        painter.rect_filled(shaft, 1.0, Color32::from_rgb(24, 24, 26));
        painter.line_segment(
            [shaft.left_top(), shaft.left_bottom()],
            Stroke::new(1.0, Color32::from_white_alpha(40)),
        );
        // The numbers where there is room for them, else a mark per step.
        for n in 1..=shown as i32 {
            let y = slot + (n as f32 - 0.5) * step;
            if step >= 8.5 {
                painter.text(
                    pos2(x, y),
                    Align2::CENTER_CENTER,
                    n.to_string(),
                    FontId::monospace((step * 0.9).min(10.0)),
                    Color32::from_gray(225),
                );
            } else {
                painter.line_segment(
                    [pos2(x - 3.0, y), pos2(x + 3.0, y)],
                    Stroke::new(1.0, Color32::from_gray(200)),
                );
            }
        }
    }
    let index = DRAWBAR_IDS.iter().position(|b| *b == id).unwrap_or(2);
    let color = match index {
        0 | 1 => Color32::from_rgb(112, 58, 32),
        4 | 6 | 7 => Color32::from_rgb(28, 28, 30),
        _ => Color32::from_rgb(236, 230, 212),
    };
    let cap = Rect::from_min_size(pos2(x - tab.x / 2.0, end), tab);
    painter.rect_filled(
        cap.translate(vec2(1.0, 2.0)),
        3.0,
        Color32::from_black_alpha(120),
    );
    painter.rect_filled(cap, 3.0, color);
    gear::sheen(
        painter,
        Rect::from_min_max(cap.min, pos2(cap.right(), cap.center().y)).shrink(1.5),
        Color32::from_white_alpha(60),
        Color32::from_white_alpha(0),
    );
    painter.rect_stroke(
        cap,
        3.0,
        Stroke::new(1.0, Color32::from_black_alpha(150)),
        egui::StrokeKind::Inside,
    );
}

const DRAWBAR_IDS: [&str; 9] = [
    "bar_16",
    "bar_5_1_3",
    "bar_8",
    "bar_4",
    "bar_2_2_3",
    "bar_2",
    "bar_1_3_5",
    "bar_1_1_3",
    "bar_1",
];

/// What a simple control moves: each setting between its value at the control's left end
/// and at its right end, and the hint telling what it does.
fn macro_targets(name: &str) -> (&'static [(&'static str, f32, f32)], &'static str) {
    match name {
        "texture" => (
            &[
                ("size", 400.0, 30.0),
                ("density", 18.0, 80.0),
                ("spray", 3.0, 70.0),
            ],
            "Smooth and held to the left, scattered and grainy to the right",
        ),
        "movement" => (
            &[("scan", 0.0, 60.0), ("detune", 4.0, 40.0)],
            "Still to the left; to the right the sound drifts through and the grains detune",
        ),
        "space" => (
            &[("reverb", 0.0, 70.0), ("reverb_decay", 1.0, 6.0)],
            "Dry to the left, a vast space to the right",
        ),
        _ => (
            &[("attack", 5.0, 3000.0), ("release", 200.0, 6000.0)],
            "Notes start and end at once to the left, swell and fade slowly to the right",
        ),
    }
}

/// A knob moving several settings together (the simple view's), its place read back from
/// the first of them. The settings stay as fine as ever in the advanced view.
fn macro_control(
    ui: &mut Ui,
    painter: &Painter,
    params: &mut dyn EditorParams,
    (name, label): (&str, &str),
    center: Pos2,
    diameter: f32,
    ink: Color32,
) {
    let (targets, hint) = macro_targets(name);
    // Where a value sits between its two ends, as a knob turns: by ratio for wide ranges.
    let along = |v: f32, a: f32, b: f32| -> f32 {
        if a > 0.0 && b > 0.0 && (b / a > 8.0 || a / b > 8.0) {
            ((v / a).ln() / (b / a).ln()).clamp(0.0, 1.0)
        } else {
            ((v - a) / (b - a)).clamp(0.0, 1.0)
        }
    };
    let at = |t: f32, a: f32, b: f32| -> f32 {
        if a > 0.0 && b > 0.0 && (b / a > 8.0 || a / b > 8.0) {
            a * (b / a).powf(t)
        } else {
            a + (b - a) * t
        }
    };
    let (first, a, b) = targets[0];
    let t = along(params.get(first), a, b);
    let r = diameter / 2.0;
    engrave(painter, center - vec2(0.0, r + 15.0), label, 10.0, ink);
    let rect = Rect::from_center_size(center, vec2(diameter + 14.0, diameter + 14.0));
    let response = ui
        .interact(
            rect,
            ui.scope_id().with(("keys-macro", name)),
            Sense::click_and_drag(),
        )
        .on_hover_text(tr(hint));
    let held = ui.scope_id().with(("keys-macro-drag", name));
    let mut wanted = t;
    if response.drag_started() {
        ui.data_mut(|d| d.insert_temp(held, t));
    }
    if response.dragged() {
        let fine = ui.input(|i| i.modifiers.shift);
        let mut now = ui.data(|d| d.get_temp::<f32>(held)).unwrap_or(t);
        now =
            (now - response.drag_delta().y / 150.0 * if fine { 0.15 } else { 1.0 }).clamp(0.0, 1.0);
        ui.data_mut(|d| d.insert_temp(held, now));
        wanted = now;
    }
    if response.hovered() {
        let wheel = ui.input(|i| i.smooth_scroll_delta.y);
        if wheel != 0.0 {
            wanted = (wanted + wheel.signum() * 0.02).clamp(0.0, 1.0);
        }
    }
    let moved = (wanted - t).abs() > 1e-4;
    for &(id, a, b) in targets {
        params.shown(id);
        params.change(id, moved.then(|| at(wanted, a, b)), &response);
    }
    if response.drag_stopped() {
        ui.data_mut(|d| d.remove::<f32>(held));
    }
    let shown = if moved { wanted } else { t };
    gear::knob(painter, center, diameter, shown, KnobStyle::Skirted, ink);
    painter.text(
        center + vec2(0.0, r + 14.0),
        Align2::CENTER_CENTER,
        format!("{:.0} %", shown * 100.0),
        FontId::monospace(10.0),
        ink.gamma_multiply(0.8),
    );
}

/// A chrome toggle switch, its bat up or down.
fn toggle(painter: &Painter, c: Pos2, down: bool) {
    painter.circle_filled(c + vec2(1.0, 3.0), 12.0, Color32::from_black_alpha(80));
    painter.circle_filled(c, 11.0, Color32::from_rgb(170, 172, 178));
    painter.circle_stroke(c, 11.0, Stroke::new(1.0, Color32::from_black_alpha(140)));
    painter.circle_filled(c, 6.0, Color32::from_rgb(60, 60, 64));
    let tip = c + vec2(0.0, if down { 18.0 } else { -18.0 });
    painter.line_segment([c, tip], Stroke::new(5.0, Color32::from_rgb(120, 122, 128)));
    painter.line_segment(
        [c + vec2(-1.0, 0.0), tip + vec2(-1.0, 0.0)],
        Stroke::new(1.5, Color32::from_white_alpha(170)),
    );
    painter.circle_filled(tip, 4.0, Color32::from_rgb(214, 216, 222));
}

/// The granular instrument's face: the badge, the screen of the sound with its grains, the
/// sources, the controls, the keys.
fn grain(ui: &mut Ui, params: &mut dyn EditorParams, meters: &[f32], state: &mut KeysState) {
    let look = face("grain");
    let width = ui.available_width().max(760.0);
    let header = 76.0;
    let screen_h = 150.0;
    let sources = 36.0;
    let panel = if state.advanced { 262.0 } else { 150.0 };
    let height = header + screen_h + sources + panel + FELT + KEYS + FRONT + 12.0;
    let (case, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let painter = ui.painter_at(case.expand(12.0));
    gear::shadow(&painter, case);
    let radius = egui::CornerRadius {
        nw: 18,
        ne: 18,
        sw: 0,
        se: 0,
    };
    leather(&painter, case, radius, &look);
    gear::bevel(&painter, case, 0.6);
    logo(
        &painter,
        pos2(case.left() + 30.0, case.top() + header / 2.0 + 4.0),
        34.0,
        &look,
        true,
    );
    let screen = Rect::from_min_size(
        pos2(case.left() + 18.0, case.top() + header),
        vec2(width - 36.0, screen_h - 10.0),
    );
    grain_screen(ui, &painter, params, meters, screen);
    let row = Rect::from_min_size(
        pos2(case.left(), screen.bottom() + 4.0),
        vec2(width, sources),
    );
    if let Some((i, _)) = selector(ui, &painter, params, row, &look) {
        choose_source(params, i, &[]);
    }
    let plate = Rect::from_min_size(
        pos2(case.left() + 18.0, row.bottom() + 2.0),
        vec2(width - 36.0, panel - 12.0),
    );
    paint_plate(&painter, plate, look.plate);
    if state.advanced {
        let half = plate.height() / 2.0;
        controls(
            ui,
            &painter,
            params,
            Rect::from_min_size(plate.min, vec2(plate.width(), half)),
            look.top,
            40.0,
            look.ink,
        );
        controls(
            ui,
            &painter,
            params,
            Rect::from_min_size(plate.min + vec2(0.0, half), vec2(plate.width(), half)),
            look.bottom,
            40.0,
            look.ink,
        );
    } else {
        controls(ui, &painter, params, plate, look.simple, 54.0, look.ink);
    }
    let felt = Rect::from_min_size(pos2(case.left(), plate.bottom() + 10.0), vec2(width, FELT));
    gear::sheen(&painter, felt, look.felt.0, look.felt.1);
    let bed = Rect::from_min_size(felt.left_bottom(), vec2(width, KEYS));
    keyboard(ui, &painter, params, state, bed, &look);
    let front = Rect::from_min_size(bed.left_bottom(), vec2(width, FRONT));
    leather(&painter, front, 0.0, &look);
    light(&painter, case);
}

/// The sound's outline, the region the grains are taken from (dragged to move it) and the
/// grains playing; live, the newest sound is on the right.
fn grain_screen(
    ui: &mut Ui,
    painter: &Painter,
    params: &mut dyn EditorParams,
    meters: &[f32],
    screen: Rect,
) {
    use audiostrada_dsp::keys::grain::{FILE, LIVE, meter};
    painter.rect_filled(screen.expand(3.0), 8.0, Color32::from_rgb(24, 26, 30));
    painter.rect_filled(screen, 6.0, Color32::from_rgb(12, 16, 20));
    let teal = Color32::from_rgb(70, 210, 200);
    let mid = screen.center().y;
    let inner = screen.shrink2(vec2(10.0, 10.0));
    let live = params.get("source").round() as usize == LIVE;
    let get = |i: usize| meters.get(i).copied().unwrap_or(0.0);
    let peak = (0..meter::BINS).map(get).fold(0.0f32, f32::max).max(1e-3);
    let bar = inner.width() / meter::BINS as f32;
    for b in 0..meter::BINS {
        let v = (get(meter::OUTLINE + b) / peak).min(1.0);
        let x = inner.left() + (b as f32 + 0.5) * bar;
        let h = v * inner.height() / 2.0;
        painter.line_segment(
            [pos2(x, mid - h.max(0.5)), pos2(x, mid + h.max(0.5))],
            Stroke::new((bar - 1.0).max(1.0), teal.gamma_multiply(0.45)),
        );
    }
    let source = params.get("source").round() as usize;
    let missing = source == FILE && params.file_name(SLOT).is_none();
    if !missing {
        draw_region(painter, meters, inner, mid);
    }
    let response = ui
        .interact(
            screen,
            ui.scope_id().with("grain-screen"),
            Sense::click_and_drag(),
        )
        .on_hover_cursor(egui::CursorIcon::Crosshair);
    params.shown("position");
    let silent = live && peak <= 1e-3;
    let playing = (0..meter::SHOWN).any(|k| get(meter::GRAINS + k) >= 0.0);
    let keyboard = params.get("trigger") < 0.5;
    // What to do next, said where the eye already is.
    let (hint, centered) = if missing {
        (tr("Drop an audio file here, or click to choose one"), true)
    } else if silent && params.sidechain_name().is_some() {
        (
            tr("Nothing heard yet: choose the track to listen to, top right, and play the song"),
            true,
        )
    } else if silent {
        (
            tr("Nothing heard yet: send a track to the sidechain and play the song"),
            true,
        )
    } else if params.get("freeze") >= 0.5 {
        (tr("Frozen: the grains keep playing this moment"), false)
    } else if keyboard && !playing {
        (tr("Play notes on the keyboard to hear the grains"), true)
    } else if live {
        (tr("Live: the newest sound on the right"), false)
    } else {
        (tr("Drag to move where the grains play"), false)
    };
    if centered {
        let galley = painter.layout_no_wrap(hint.to_owned(), FontId::proportional(14.0), teal);
        let back = Rect::from_center_size(screen.center(), galley.size() + vec2(28.0, 14.0));
        painter.rect_filled(back, 6.0, Color32::from_black_alpha(200));
        painter.galley(back.min + vec2(14.0, 7.0), galley, teal);
    } else {
        painter.text(
            inner.left_top() + vec2(2.0, 2.0),
            Align2::LEFT_TOP,
            hint,
            FontId::proportional(10.0),
            teal.gamma_multiply(0.7),
        );
    }
    // The file or the track the grains are taken from, top right, to change it.
    let mut ask = false;
    if source == FILE && !missing {
        let name = params.file_name(SLOT).unwrap_or_default();
        let corner = inner.right_top() - vec2(0.0, 4.0);
        ask = screen_button(ui, painter, corner, &trf!("{name} · Change", name = name)).clicked();
    } else if live && let Some(track) = params.sidechain_name() {
        let button = screen_button(
            ui,
            painter,
            inner.right_top() - vec2(0.0, 4.0),
            &trf!("Listens to: {track} ▾", track = track),
        );
        egui::Popup::menu(&button).show(|ui| params.sidechain_menu(ui));
    }
    // Audio files dropped on the window load and play.
    let (dropped, hovering) = ui.input(|i| {
        (
            i.raw
                .dropped_files
                .first()
                .map(|f| f.path().to_path_buf())
                .filter(|p| !p.as_os_str().is_empty()),
            !i.raw.hovered_files.is_empty(),
        )
    });
    if hovering {
        painter.rect_filled(screen, 6.0, Color32::from_black_alpha(170));
        painter.rect_stroke(
            screen,
            6.0,
            Stroke::new(2.0, teal),
            egui::StrokeKind::Inside,
        );
        painter.text(
            screen.center(),
            Align2::CENTER_CENTER,
            tr("Drop to play this sound"),
            FontId::proportional(15.0),
            teal,
        );
    }
    // A frozen moment of the live memory saved as a sound, which then plays from the keys.
    let mut keep = false;
    if live && !silent && params.get("freeze") >= 0.5 && params.can_keep_recording() {
        let corner = inner.right_bottom() - vec2(0.0, 18.0);
        keep = screen_button(ui, painter, corner, tr("Save as a sound…")).clicked();
    }
    if params.keep_recording(ui, SLOT, keep) {
        choose_source(params, FILE, &[("freeze", 0.0)]);
    }
    ask |= missing && response.clicked();
    if params.load_file(ui, SLOT, ask, dropped) && source != FILE {
        choose_source(params, FILE, &[]);
    }
    if missing {
        return;
    }
    if (response.dragged() || response.clicked())
        && let Some(pos) = response.interact_pointer_pos()
    {
        let x = ((pos.x - inner.left()) / inner.width()).clamp(0.0, 1.0);
        let spray = params.get("spray") / 100.0;
        let place = if live {
            1.0 - x - spray * 0.5
        } else {
            x + spray * 0.5
        };
        params.set(
            "position",
            Some((place * 100.0).clamp(0.0, 100.0)),
            &response,
        );
    } else {
        params.set("position", None, &response);
    }
    animate(ui.ctx());
}

/// The region the grains are taken from, from its start across its width (wrapping at the
/// end), and the grains playing, as sparks.
fn draw_region(painter: &Painter, meters: &[f32], inner: Rect, mid: f32) {
    use audiostrada_dsp::keys::grain::meter;
    let teal = Color32::from_rgb(70, 210, 200);
    let get = |i: usize| meters.get(i).copied().unwrap_or(0.0);
    let start = get(meter::REGION).rem_euclid(1.0);
    let span = get(meter::REGION + 1).max(0.004);
    for (a, b) in [
        (start, (start + span).min(1.0)),
        (0.0, (start + span - 1.0).max(0.0)),
    ] {
        if b > a {
            let r = Rect::from_min_max(
                pos2(inner.left() + a * inner.width(), inner.top()),
                pos2(inner.left() + b * inner.width(), inner.bottom()),
            );
            painter.rect_filled(r, 2.0, teal.gamma_multiply(0.14));
        }
    }
    let marker = inner.left() + start * inner.width();
    painter.line_segment(
        [pos2(marker, inner.top()), pos2(marker, inner.bottom())],
        Stroke::new(1.5, teal),
    );
    // The grains, as sparks.
    for k in 0..meter::SHOWN {
        let at = get(meter::GRAINS + k);
        if at < 0.0 {
            continue;
        }
        let x = inner.left() + at * inner.width();
        let y = mid + ((k as f32 * 2.399).sin()) * inner.height() * 0.35;
        gear::glow(
            painter,
            pos2(x, y),
            7.0,
            Color32::from_rgba_unmultiplied(255, 210, 120, 120),
            Color32::TRANSPARENT,
        );
        painter.circle_filled(pos2(x, y), 2.0, Color32::from_rgb(255, 230, 170));
    }
}

/// Strada-Grain's file slot.
const SLOT: &str = "sample";

/// Chooses Strada-Grain's source: the live input plays on its own, as an effect would, a
/// sound plays from the keyboard; either can be changed after.
fn choose_source(params: &mut dyn EditorParams, source: usize, more: &[(&str, f32)]) {
    use audiostrada_dsp::keys::grain::LIVE;
    let was = params.get("source").round() as usize == LIVE;
    let live = source == LIVE;
    let mut changes = vec![("source".to_owned(), source as f32)];
    if live != was {
        changes.push(("trigger".to_owned(), if live { 1.0 } else { 0.0 }));
    }
    changes.extend(more.iter().map(|(id, v)| ((*id).to_owned(), *v)));
    params.edit_many(tr("Source"), &changes);
}

/// A small button on a screen, its top right corner at `corner`.
fn screen_button(ui: &mut Ui, painter: &Painter, corner: Pos2, text: &str) -> egui::Response {
    let font = FontId::proportional(11.0);
    let galley = painter.layout_no_wrap(text.to_owned(), font, Color32::from_rgb(200, 240, 236));
    let size = galley.size() + vec2(16.0, 8.0);
    let rect = Rect::from_min_size(pos2(corner.x - size.x, corner.y), size);
    let response = ui
        .interact(
            rect,
            ui.scope_id().with(("screen-button", text)),
            Sense::click(),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let fill = if response.hovered() { 60 } else { 40 };
    painter.rect_filled(
        rect,
        4.0,
        Color32::from_rgba_unmultiplied(70, 210, 200, fill),
    );
    painter.rect_stroke(
        rect,
        4.0,
        Stroke::new(1.0, Color32::from_rgb(70, 210, 200).gamma_multiply(0.6)),
        egui::StrokeKind::Inside,
    );
    painter.galley(rect.min + vec2(8.0, 4.0), galley, Color32::WHITE);
    response
}

/// The bass machine's controls: a column of large knobs at the left, then the sound around
/// its screen, then the tone, each item an id and its label.
type Items = &'static [(&'static str, &'static str)];
const MACHINE_LEFT: Items = &[("glide", "Glide"), ("tune", "Tune")];
const MACHINE_LEFT_ALL: Items = &[("glide", "Glide"), ("tune", "Tune"), ("mode", "Mode")];
const MACHINE_SOUND: [Items; 1] = [&[("punch", "Punch"), ("length", "Length"), ("click", "Click")]];
const MACHINE_SOUND_ALL: [Items; 2] = [
    &[
        ("punch", "Punch"),
        ("punch_time", "Punch time"),
        ("length", "Length"),
    ],
    &[
        ("gate", "Note length"),
        ("release", "Release"),
        ("click", "Click"),
    ],
];
const MACHINE_TONE: [Items; 1] = [&[
    ("drive", "Drive"),
    ("shape", "Distortion"),
    ("clean", "Clean sub"),
]];
const MACHINE_TONE_ALL: [Items; 2] = [
    &[
        ("drive", "Drive"),
        ("shape", "Distortion"),
        ("clean", "Clean sub"),
    ],
    &[
        ("body", "Body"),
        ("cutoff", "Filter"),
        ("dynamics", "Velocity"),
    ],
];
const WALNUT: Color32 = Color32::from_rgb(148, 90, 52);
const PANEL: Color32 = Color32::from_rgb(28, 27, 26);
const CREAM: Color32 = Color32::from_rgb(226, 218, 196);
const AMBER: Color32 = Color32::from_rgb(232, 160, 52);

/// The bass machine: a black panel in a walnut case, the name printed in amber at the top
/// right, its controls in boxes drawn in cream lines (the sound around its screen, the tone),
/// large knobs at the left, and sixteen pads in its four colors that play the notes from the
/// pads' first one.
fn machine(ui: &mut Ui, params: &mut dyn EditorParams, state: &mut KeysState) {
    let width = ui.available_width().max(760.0);
    let frame = 20.0;
    let header = 62.0;
    let body = if state.advanced { 300.0 } else { 214.0 };
    let pads = 100.0;
    let height = frame * 2.0 + header + body + pads;
    let (case, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let painter = ui.painter_at(case.expand(12.0));
    gear::shadow(&painter, case);
    painter.rect_filled(case, 12.0, WALNUT);
    textured(&painter, case, 12.0, Texture::Wood, WALNUT);
    gear::bevel(&painter, case, 0.7);
    let panel = case.shrink(frame);
    painter.rect_filled(panel.expand(2.0), 6.0, Color32::from_black_alpha(170));
    painter.rect_filled(panel, 4.0, PANEL);
    textured(&painter, panel, 4.0, Texture::Plastic, PANEL);
    gear::sheen(
        &painter,
        Rect::from_min_size(panel.min, vec2(panel.width(), 40.0)),
        Color32::from_white_alpha(10),
        Color32::from_white_alpha(0),
    );
    // The name, and an amber rule running to it.
    let y = panel.top() + header / 2.0 + 2.0;
    let name = printed_logo(&painter, pos2(panel.right() - 26.0, y), 40.0);
    painter.line_segment(
        [
            pos2(panel.left() + 26.0, name.bottom() - 4.0),
            pos2(name.left() - 14.0, name.bottom() - 4.0),
        ],
        Stroke::new(1.5, AMBER),
    );
    let top = panel.top() + header;
    let left = Rect::from_min_size(pos2(panel.left() + 10.0, top), vec2(140.0, body));
    let items = if state.advanced {
        MACHINE_LEFT_ALL
    } else {
        MACHINE_LEFT
    };
    let step = (body - 10.0) / items.len() as f32;
    for (k, &(id, label)) in items.iter().enumerate() {
        let center = pos2(left.center().x, top + 8.0 + step * (k as f32 + 0.5));
        let diameter = if k == 0 { 54.0 } else { 46.0 };
        control(
            ui,
            &painter,
            params,
            (id, tr(label)),
            center,
            diameter,
            CREAM,
        );
    }
    let rest = Rect::from_min_max(
        pos2(left.right() + 6.0, top + 8.0),
        pos2(panel.right() - 16.0, top + body - 6.0),
    );
    let split = rest.left() + rest.width() * 0.46;
    let sound = Rect::from_min_max(rest.min, pos2(split - 9.0, rest.bottom()));
    let tone = Rect::from_min_max(pos2(split + 9.0, rest.top()), rest.max);
    print_box(&painter, sound, tr("Sound"));
    print_box(&painter, tone, tr("Tone"));
    let screen = Rect::from_min_size(
        sound.min + vec2(18.0, 20.0),
        vec2(sound.width() - 36.0, 58.0),
    );
    note_screen(&painter, screen, params);
    let rows: &[Items] = if state.advanced {
        &MACHINE_SOUND_ALL
    } else {
        &MACHINE_SOUND
    };
    knob_rows(
        ui,
        &painter,
        params,
        Rect::from_min_max(pos2(sound.left(), screen.bottom() + 4.0), sound.max),
        rows,
    );
    // The tone's knobs, the volume large at the right.
    let volume = Rect::from_min_max(pos2(tone.right() - 110.0, tone.top()), tone.max);
    control(
        ui,
        &painter,
        params,
        ("volume", tr("Volume")),
        pos2(volume.center().x - 6.0, tone.top() + 74.0),
        64.0,
        CREAM,
    );
    let rows: &[Items] = if state.advanced {
        &MACHINE_TONE_ALL
    } else {
        &MACHINE_TONE
    };
    let knobs = Rect::from_min_max(
        tone.min + vec2(0.0, 8.0),
        pos2(volume.left(), tone.top() + 8.0 + 104.0 * rows.len() as f32),
    );
    knob_rows(ui, &painter, params, knobs, rows);
    if !state.advanced {
        printed_waves(
            &painter,
            Rect::from_min_max(
                pos2(tone.left() + 22.0, tone.top() + 134.0),
                pos2(tone.right() - 22.0, tone.bottom() - 12.0),
            ),
        );
    }
    let row = Rect::from_min_size(
        pos2(panel.left() + 12.0, top + body + 2.0),
        vec2(panel.width() - 24.0, pads - 8.0),
    );
    machine_pads(ui, &painter, params, state, row, CREAM);
}

/// Strada-808's name as printed on its panel: "Strada" in red italics, "808" in amber; its
/// right end at `right`, centered on its y.
fn printed_logo(painter: &Painter, right: Pos2, h: f32) -> Rect {
    let galley = |text: &str, size: f32, italics: bool, color: Color32| {
        let mut job = egui::text::LayoutJob::default();
        job.append(
            text,
            0.0,
            egui::TextFormat {
                font_id: if italics {
                    FontId::proportional(size)
                } else {
                    theme::strong(size)
                },
                italics,
                extra_letter_spacing: if italics { 0.0 } else { size * 0.12 },
                color,
                ..Default::default()
            },
        );
        painter.layout_job(job)
    };
    let first = galley("Strada", h * 0.5, true, Color32::from_rgb(220, 52, 44));
    let second = galley("808", h * 0.9, false, AMBER);
    let width = first.size().x + h * 0.2 + second.size().x;
    let area = Rect::from_min_size(
        pos2(right.x - width, right.y - second.size().y / 2.0),
        vec2(width, second.size().y),
    );
    let base = area.bottom();
    painter.galley(
        pos2(area.left(), base - first.size().y - h * 0.06),
        first.clone(),
        Color32::WHITE,
    );
    painter.galley(
        pos2(area.left() + first.size().x + h * 0.2, area.top()),
        second,
        Color32::WHITE,
    );
    area
}

/// A box drawn in cream lines with its name set in the top line.
fn print_box(painter: &Painter, rect: Rect, title: &str) {
    painter.rect_stroke(
        rect,
        8.0,
        Stroke::new(1.5, CREAM.gamma_multiply(0.85)),
        egui::StrokeKind::Inside,
    );
    let galley = painter.layout_no_wrap(title.to_uppercase(), theme::strong(11.0), AMBER);
    let at = pos2(rect.left() + 22.0, rect.top() - galley.size().y / 2.0);
    painter.rect_filled(
        Rect::from_min_size(at - vec2(8.0, 0.0), galley.size() + vec2(16.0, 0.0)),
        0.0,
        PANEL,
    );
    painter.galley(at, galley, AMBER);
}

/// Rows of knobs spread evenly across `rect`, each row 104 points high.
fn knob_rows(
    ui: &mut Ui,
    painter: &Painter,
    params: &mut dyn EditorParams,
    rect: Rect,
    rows: &[Items],
) {
    for (r, items) in rows.iter().enumerate() {
        let cell = rect.width() / items.len() as f32;
        let y = rect.top() + 104.0 * r as f32 + 54.0;
        for (k, &(id, label)) in items.iter().enumerate() {
            let center = pos2(rect.left() + cell * (k as f32 + 0.5), y);
            control(ui, painter, params, (id, tr(label)), center, 44.0, CREAM);
        }
    }
}

/// Three decaying waves printed on the panel, from a short note to a long one.
fn printed_waves(painter: &Painter, rect: Rect) {
    if rect.height() < 20.0 {
        return;
    }
    let row = rect.height() / 3.0;
    for k in 0..3 {
        let mid = rect.top() + row * (k as f32 + 0.5);
        let length = 0.25 + 0.3 * k as f32;
        let points: Vec<Pos2> = (0..160)
            .map(|i| {
                let t = i as f32 / 159.0;
                let level = 10f32.powf(-2.0 * t / length) * (t * 40.0).min(1.0);
                let wave = (t * 70.0 * (1.0 + 0.8 * (-t * 9.0).exp())).sin();
                pos2(
                    rect.left() + t * rect.width(),
                    mid - wave * level * row * 0.36,
                )
            })
            .collect();
        painter.add(egui::Shape::line(
            points,
            Stroke::new(1.0, CREAM.gamma_multiply(0.55)),
        ));
    }
}

/// The note as the screen draws it: its wave dropping onto the pitch and dying away, the
/// distortion squaring it.
fn note_screen(painter: &Painter, screen: Rect, params: &mut dyn EditorParams) {
    // A liquid crystal screen in its bezel.
    painter.rect_filled(screen.expand(4.0), 5.0, Color32::from_rgb(12, 12, 12));
    painter.rect_filled(screen, 3.0, Color32::from_rgb(38, 44, 22));
    let glow = Color32::from_rgb(196, 222, 96);
    let mid = screen.center().y;
    painter.line_segment(
        [
            pos2(screen.left() + 6.0, mid),
            pos2(screen.right() - 6.0, mid),
        ],
        Stroke::new(1.0, glow.gamma_multiply(0.15)),
    );
    let punch = params.get("punch");
    let punch_time = params.get("punch_time").max(1.0) / 1000.0;
    let length = params.get("length").max(0.05);
    let drive = params.get("drive") / 100.0;
    let body = params.get("body") / 100.0;
    // The note's length across, the wave slowed so its cycles show.
    let span = (length * 1.1).clamp(0.3, 3.0);
    let n = 360;
    let mut phase = 0.0f32;
    let gain = 1.0 + 6.0 * drive + 2.0 * body;
    let points: Vec<Pos2> = (0..n)
        .map(|i| {
            let t = i as f32 / (n - 1) as f32 * span;
            let shift = punch * (-t / punch_time).exp();
            phase += 0.045 * 2f32.powf(shift / 12.0);
            let level = 10f32.powf(-3.0 * t / length);
            let wave = ((phase * std::f32::consts::TAU).sin() * gain).tanh() / gain.tanh();
            pos2(
                screen.left() + 6.0 + t / span * (screen.width() - 12.0),
                mid - wave * level * (screen.height() / 2.0 - 8.0),
            )
        })
        .collect();
    painter.add(egui::Shape::line(
        points.clone(),
        Stroke::new(3.0, glow.gamma_multiply(0.25)),
    ));
    painter.add(egui::Shape::line(points, Stroke::new(1.4, glow)));
    gear::sheen(
        painter,
        Rect::from_min_max(screen.min, pos2(screen.right(), mid)).shrink(2.0),
        Color32::from_white_alpha(14),
        Color32::from_white_alpha(0),
    );
}

/// Sixteen pads in four colors, each a note from the first one, with the octave arrows.
fn machine_pads(
    ui: &mut Ui,
    painter: &Painter,
    params: &mut dyn EditorParams,
    state: &mut KeysState,
    row: Rect,
    ink: Color32,
) {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    let arrows = 34.0;
    for (down, x) in [(true, row.left()), (false, row.right() - arrows)] {
        let rect = Rect::from_min_size(pos2(x, row.top()), vec2(arrows, row.height()));
        let response = ui
            .interact(
                rect,
                ui.scope_id().with(("pads-octave", down)),
                Sense::click(),
            )
            .on_hover_text(tr(if down { "Octave down" } else { "Octave up" }));
        if response.clicked() {
            state.pads = (state.pads + if down { -12 } else { 12 }).clamp(0, 111);
        }
        let c = rect.center();
        let dir = if down { -1.0 } else { 1.0 };
        let color = if response.hovered() {
            Color32::WHITE
        } else {
            ink
        };
        painter.add(egui::Shape::convex_polygon(
            vec![
                c + vec2(6.0 * dir, 0.0),
                c + vec2(-5.0 * dir, -7.0),
                c + vec2(-5.0 * dir, 7.0),
            ],
            color,
            Stroke::NONE,
        ));
    }
    let inner = row.shrink2(vec2(arrows + 6.0, 0.0));
    let gap = 8.0;
    let pad = (inner.width() - gap * 15.0) / 16.0;
    let mut wanted = None;
    let mut down = false;
    for i in 0..16 {
        let key = state.pads + i;
        let rect = Rect::from_min_size(
            pos2(inner.left() + i as f32 * (pad + gap), inner.top() + 32.0),
            vec2(pad, inner.height() - 36.0),
        );
        let response = ui.interact(
            rect,
            ui.scope_id().with(("bass-pad", i)),
            Sense::click_and_drag(),
        );
        if response.is_pointer_button_down_on() {
            wanted = u8::try_from(key).ok();
            down = true;
        } else if response.clicked() && wanted.is_none() {
            wanted = u8::try_from(key).ok();
        }
        let lit = state.mouse == u8::try_from(key).ok();
        let color = MACHINE[(i / 4) as usize];
        painter.text(
            pos2(rect.center().x, inner.top() + 9.0),
            Align2::CENTER_CENTER,
            format!("{}{}", NAMES[key.rem_euclid(12) as usize], key / 12 - 1),
            FontId::monospace(10.0),
            ink,
        );
        painter.rect_filled(
            rect.translate(vec2(0.0, 3.0)),
            5.0,
            Color32::from_black_alpha(140),
        );
        let cap = rect.translate(vec2(0.0, if lit { 2.0 } else { 0.0 }));
        painter.rect_filled(
            cap,
            5.0,
            if lit {
                color
            } else {
                color.gamma_multiply(0.82)
            },
        );
        gear::sheen(
            painter,
            Rect::from_min_max(cap.min, pos2(cap.right(), cap.center().y)).shrink(2.0),
            Color32::from_white_alpha(if response.hovered() { 60 } else { 36 }),
            Color32::from_white_alpha(0),
        );
        // The pad's lamp above it, lit while it plays.
        let lamp = pos2(rect.center().x, inner.top() + 22.0);
        if lit {
            gear::glow(
                painter,
                lamp,
                10.0,
                Color32::from_rgba_unmultiplied(255, 60, 40, 140),
                Color32::TRANSPARENT,
            );
        }
        painter.circle_filled(
            lamp,
            3.0,
            if lit {
                Color32::from_rgb(255, 70, 50)
            } else {
                Color32::from_rgb(70, 24, 20)
            },
        );
    }
    let wanted = tap(ui, state, down, wanted);
    if wanted != state.mouse {
        if let Some(old) = state.mouse.take() {
            params.play_key([0x80, old, 0]);
        }
        if let Some(key) = wanted {
            params.play_key([0x90, key, 100]);
            state.mouse = Some(key);
        }
    }
    if state.mouse.is_some() {
        animate(ui.ctx());
    }
}

/// The key the mouse plays: `key` while held down (`down`). A click too brief for a frame to
/// see it held still plays its key, for a moment.
fn tap(ui: &Ui, state: &mut KeysState, down: bool, key: Option<u8>) -> Option<u8> {
    if down {
        return key;
    }
    let now = ui.input(|i| i.time);
    if let Some(k) = key
        && state.mouse != Some(k)
    {
        state.tap_until = now + 0.15;
        return Some(k);
    }
    if state.mouse.is_some() && now < state.tap_until {
        return state.mouse;
    }
    None
}

fn is_black(key: i32) -> bool {
    matches!(key.rem_euclid(12), 1 | 3 | 6 | 8 | 10)
}

/// The keys between the cheek blocks, which carry the octave arrows.
fn keyboard(
    ui: &mut Ui,
    painter: &Painter,
    params: &mut dyn EditorParams,
    state: &mut KeysState,
    bed: Rect,
    look: &Face,
) {
    painter.rect_filled(bed, 0.0, Color32::from_rgb(20, 18, 18));
    for (side, down) in [(bed.left(), true), (bed.right() - CHEEK, false)] {
        let cheek = Rect::from_min_size(pos2(side, bed.top()), vec2(CHEEK, bed.height()));
        leather(painter, cheek, 0.0, look);
        gear::bevel(painter, cheek, 0.7);
        let arrow = Rect::from_center_size(cheek.center(), vec2(22.0, 30.0));
        let response = ui
            .interact(
                arrow,
                ui.scope_id().with(("keys-octave", down)),
                Sense::click(),
            )
            .on_hover_text(tr(if down { "Octave down" } else { "Octave up" }));
        let shift = if down { -12 } else { 12 };
        if response.clicked() {
            state.low = (state.low + shift).clamp(0, 127 - 12 * OCTAVES);
        }
        let c = arrow.center();
        let dir = if down { -1.0 } else { 1.0 };
        let color = if response.hovered() {
            look.print.gamma_multiply(1.6)
        } else {
            look.print
        };
        painter.add(egui::Shape::convex_polygon(
            vec![
                c + vec2(6.0 * dir, 0.0),
                c + vec2(-5.0 * dir, -7.0),
                c + vec2(-5.0 * dir, 7.0),
            ],
            color,
            Stroke::NONE,
        ));
    }
    let keys = Rect::from_min_max(
        pos2(bed.left() + CHEEK, bed.top()),
        pos2(bed.right() - CHEEK, bed.bottom()),
    );
    let whites: Vec<i32> = (state.low..=state.low + 12 * OCTAVES)
        .filter(|k| !is_black(*k))
        .collect();
    let white_width = keys.width() / whites.len() as f32;
    let black_size = vec2(white_width * 0.58, keys.height() * 0.62);
    let blacks: Vec<(i32, Rect)> = whites
        .iter()
        .enumerate()
        .filter(|(_, k)| is_black(**k + 1) && **k < state.low + 12 * OCTAVES)
        .map(|(i, k)| {
            let x = keys.left() + (i + 1) as f32 * white_width - black_size.x / 2.0;
            (k + 1, Rect::from_min_size(pos2(x, keys.top()), black_size))
        })
        .collect();
    let response = ui.interact(
        keys,
        ui.scope_id().with("keys-bed"),
        Sense::click_and_drag(),
    );
    let under = |pos: Pos2| -> Option<(i32, f32)> {
        if let Some((key, r)) = blacks.iter().find(|(_, r)| r.contains(pos)) {
            return Some((*key, (pos.y - r.top()) / r.height()));
        }
        let i = ((pos.x - keys.left()) / white_width) as usize;
        whites
            .get(i)
            .map(|k| (*k, (pos.y - keys.top()) / keys.height()))
    };
    let at = response.interact_pointer_pos().and_then(under);
    let pressed = if response.is_pointer_button_down_on() {
        at
    } else {
        at.filter(|_| response.clicked())
    };
    let wanted = tap(
        ui,
        state,
        response.is_pointer_button_down_on(),
        pressed.and_then(|(k, _)| u8::try_from(k).ok()),
    );
    if wanted != state.mouse {
        if let Some(old) = state.mouse.take() {
            params.play_key([0x80, old, 0]);
        }
        if let Some(key) = wanted {
            let depth = pressed.map_or(0.7, |(_, d)| d);
            // Lower on the key plays louder, as a finger pressing harder.
            let velocity = (30.0 + 97.0 * depth.clamp(0.0, 1.0)) as u8;
            params.play_key([0x90, key, velocity]);
            state.mouse = Some(key);
        }
    }
    if state.mouse.is_some() {
        animate(ui.ctx());
    }
    let held = |k: i32| state.mouse == u8::try_from(k).ok();
    for (i, k) in whites.iter().enumerate() {
        let r = Rect::from_min_size(
            pos2(keys.left() + i as f32 * white_width, keys.top()),
            vec2(white_width, keys.height()),
        )
        .shrink2(vec2(0.6, 0.0));
        let down = held(*k);
        let lip = if down { 4.0 } else { 9.0 };
        let top = Rect::from_min_max(r.min, pos2(r.right(), r.bottom() - lip));
        let (light, dark) = if down {
            (
                Color32::from_rgb(214, 212, 204),
                Color32::from_rgb(196, 194, 186),
            )
        } else {
            (
                Color32::from_rgb(246, 244, 236),
                Color32::from_rgb(228, 226, 218),
            )
        };
        theme::gradient_rect(painter, top, dark, light);
        let front = Rect::from_min_max(pos2(r.left(), r.bottom() - lip), r.max);
        theme::gradient_rect(
            painter,
            front,
            Color32::from_rgb(150, 148, 140),
            Color32::from_rgb(206, 204, 196),
        );
        painter.line_segment(
            [r.right_top(), r.right_bottom()],
            Stroke::new(1.0, Color32::from_black_alpha(90)),
        );
        if k.rem_euclid(12) == 0 {
            painter.text(
                top.center_bottom() - vec2(0.0, 6.0),
                Align2::CENTER_BOTTOM,
                format!("C{}", k / 12 - 1),
                FontId::proportional(9.0),
                Color32::from_gray(150),
            );
        }
    }
    for (k, r) in &blacks {
        let down = held(*k);
        painter.rect_filled(
            r.translate(vec2(2.0, 3.0)),
            2.0,
            Color32::from_black_alpha(90),
        );
        painter.rect_filled(*r, 2.0, Color32::from_rgb(16, 16, 18));
        let cap = Rect::from_min_max(
            r.min + vec2(2.5, 0.0),
            pos2(r.right() - 2.5, r.bottom() - if down { 5.0 } else { 9.0 }),
        );
        theme::gradient_rect(
            painter,
            cap,
            Color32::from_rgb(52, 52, 56),
            Color32::from_rgb(28, 28, 30),
        );
        painter.line_segment(
            [cap.left_top() + vec2(1.0, 2.0), cap.left_bottom()],
            Stroke::new(1.0, Color32::from_white_alpha(36)),
        );
    }
    // The felt's shadow over the keys.
    gear::sheen(
        painter,
        Rect::from_min_size(keys.min, vec2(keys.width(), 10.0)),
        Color32::from_black_alpha(130),
        Color32::from_black_alpha(0),
    );
}
