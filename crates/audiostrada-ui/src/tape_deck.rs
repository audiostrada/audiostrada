//! Strada-Tape's reel-to-reel machines, drawn: spun-metal reels whose windows turn under a
//! fixed light, the tape threaded round the guides, the heads under their cover, and each
//! machine's own controls for the tape and the speed.

use std::f32::consts::{FRAC_PI_2, TAU};

use eframe::egui::{
    self, Align2, Color32, FontId, Mesh, Painter, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2, pos2,
    vec2,
};

use crate::effect_editors::{Params, VuFace};
use crate::gear::{self, Texture};
use crate::theme;

/// Where a machine wants its two meters, and their face.
pub struct Meters {
    pub places: [Rect; 2],
    pub face: VuFace,
}

/// How a reel looks: its metal, its hub and its windows (count, span in degrees, inner and
/// outer radius as a share of the reel's).
#[derive(Clone, Copy)]
struct ReelLook {
    metal: Color32,
    hub: Color32,
    windows: (usize, f32, f32, f32),
}

const TAPE: Color32 = Color32::from_rgb(58, 38, 27);
const PACK: Color32 = Color32::from_rgb(78, 52, 36);
/// How flat an upright roller's round top looks from the slanted view.
const SQUASH: f32 = 0.38;
const SCREW: Color32 = Color32::from_rgb(176, 178, 182);

/// How far each reel has turned: the tape travelled over each pack's radius.
pub fn turns(travel: f32, radius: f32, packs: [f32; 2]) -> [f32; 2] {
    packs.map(|p| travel / (radius * p))
}

pub fn studio(ui: &mut Ui, rect: Rect, params: &mut Params, travel: f32) -> Meters {
    let painter = ui.painter_at(rect);
    let w = rect.width();
    let deck = Rect::from_min_size(rect.min, vec2(w, 300.0));
    let lower = Rect::from_min_max(pos2(rect.left(), deck.bottom()), rect.max);
    let aluminium = Color32::from_rgb(202, 204, 207);
    panel(&painter, deck, aluminium, 1.0);
    panel(&painter, lower, Color32::from_rgb(192, 194, 197), 1.0);
    let path = deck.top() + 236.0;
    seam(&painter, path, rect.x_range());
    seam(&painter, deck.bottom(), rect.x_range());
    let at = |x: f32, y: f32| pos2(rect.left() + w * x, rect.top() + y);
    let radius = 128.0;
    let packs = [0.82, 0.5];
    let turn = turns(travel, radius, packs);
    let reels = [at(0.25, 112.0), at(0.75, 112.0)];
    // The spindle motors' cups, between the reels and the heads.
    for (k, c) in reels.into_iter().enumerate() {
        let inward = if k == 0 { 1.0 } else { -1.0 };
        recess(
            &painter,
            c + vec2(inward * radius * 0.5, radius * 0.92),
            46.0,
            aluminium,
        );
    }
    let cover = Rect::from_center_size(at(0.5, 266.0), vec2(w * 0.38, 58.0));
    let arm_left = at(0.07, 222.0);
    let arm_right = at(0.93, 222.0);
    let guide_left = pos2(cover.left() - 26.0, cover.bottom() - 4.0);
    let pinch = pos2(cover.right() + 14.0, cover.bottom() - 2.0);
    let capstan = pos2(cover.right() + 62.0, cover.center().y + 2.0);
    let guide_right = pos2(capstan.x + 58.0, cover.bottom() - 4.0);
    thread(
        &painter,
        &[
            (reels[0], radius * packs[0], false),
            (arm_left, 11.0, false),
            (guide_left, 9.0, true),
            (pinch, 11.0, true),
            (guide_right, 9.0, true),
            (arm_right, 11.0, false),
            (reels[1], radius * packs[1], false),
        ],
    );
    let silver = ReelLook {
        metal: Color32::from_rgb(172, 174, 178),
        hub: Color32::from_rgb(40, 40, 42),
        windows: (3, 62.0, 0.37, 0.86),
    };
    let gold = ReelLook {
        metal: Color32::from_rgb(214, 184, 106),
        ..silver
    };
    reel(&painter, reels[0], radius, packs[0], turn[0], silver);
    reel(&painter, reels[1], radius, packs[1], turn[1], gold);
    bracket(&painter, at(0.125, 272.0), arm_left, aluminium);
    bracket(&painter, at(0.875, 272.0), arm_right, aluminium);
    roller(&painter, arm_left, 11.0, aluminium);
    roller(&painter, arm_right, 11.0, aluminium);
    roller(&painter, arm_left + vec2(24.0, 14.0), 6.0, aluminium);
    roller(&painter, arm_right + vec2(-24.0, 14.0), 6.0, aluminium);
    roller(&painter, guide_left, 9.0, aluminium);
    roller(&painter, guide_right, 9.0, aluminium);
    pill_cover(&painter, cover);
    roller(
        &painter,
        pos2(cover.center().x - 58.0, cover.bottom() + 14.0),
        8.0,
        aluminium,
    );
    roller(
        &painter,
        pos2(cover.center().x + 74.0, cover.bottom() + 14.0),
        8.0,
        Color32::from_rgb(196, 160, 92),
    );
    roller(&painter, pinch, 11.0, Color32::from_rgb(62, 62, 64));
    machined(&painter, capstan, 33.0);
    for y in [12.0, 224.0, 248.0, 288.0] {
        for x in [0.015, 0.985] {
            screw(&painter, at(x, y), 4.0, x * 7.0 + y);
        }
    }
    for x in [0.33, 0.67] {
        screw(&painter, at(x, 248.0), 3.5, x * 11.0);
    }
    // The lower panel: the door's latch, the plate, the selectors and the meters in their slot.
    for y in [312.0, rect.height() - 12.0] {
        for x in [0.015, 0.985] {
            screw(&painter, at(x, y), 4.0, x + y);
        }
    }
    let plate = Rect::from_min_size(lower.min + vec2(26.0, 56.0), vec2(180.0, 80.0));
    let latch = pos2(plate.center().x, lower.top() + 22.0);
    painter.add(Shape::line(
        vec![latch + vec2(-7.0, 4.0), latch, latch + vec2(7.0, 4.0)],
        Stroke::new(1.6, Color32::from_gray(70)),
    ));
    painter.text(
        latch + vec2(0.0, 14.0),
        Align2::CENTER_CENTER,
        "OPEN",
        theme::strong(9.0),
        Color32::from_gray(70),
    );
    gear::shadow(&painter, plate);
    painter.rect_filled(plate, 3.0, Color32::from_rgb(240, 240, 238));
    let band = Rect::from_min_size(plate.min, vec2(plate.width(), 28.0)).shrink(3.0);
    painter.rect_filled(band, 2.0, Color32::from_rgb(26, 26, 28));
    painter.text(
        band.center(),
        Align2::CENTER_CENTER,
        "S T R A D A",
        theme::strong(15.0),
        Color32::from_gray(238),
    );
    painter.text(
        pos2(plate.center().x, band.bottom() + 9.0),
        Align2::CENTER_CENTER,
        "MULTICHANNEL TAPE RECORDER",
        FontId::proportional(7.0),
        Color32::from_gray(96),
    );
    painter.text(
        pos2(plate.center().x, plate.bottom() - 18.0),
        Align2::CENTER_CENTER,
        "A80",
        theme::strong(24.0),
        Color32::from_rgb(30, 30, 32),
    );
    gear::bevel(&painter, plate, 0.6);
    let tape_knob = pos2(plate.right() + 78.0, lower.top() + 92.0);
    choice_knob(ui, &painter, tape_knob, params, "tape", "TAPE");
    let speed_knob = pos2(lower.right() - 80.0, lower.top() + 92.0);
    choice_knob(ui, &painter, speed_knob, params, "speed", "SPEED");
    let span = (speed_knob.x - 62.0) - (tape_knob.x + 62.0);
    let size = vec2(((span - 30.0) / 2.0).min(206.0), 126.0);
    let left = tape_knob.x + 62.0 + (span - 2.0 * size.x - 14.0) / 2.0;
    let places = [
        Rect::from_min_size(pos2(left, lower.top() + 18.0), size),
        Rect::from_min_size(pos2(left + size.x + 14.0, lower.top() + 18.0), size),
    ];
    let slot = places[0].union(places[1]).expand(7.0);
    painter.rect_filled(slot, 3.0, Color32::from_rgb(150, 152, 155));
    painter.rect_stroke(
        slot,
        3.0,
        Stroke::new(1.5, Color32::from_black_alpha(110)),
        egui::StrokeKind::Inside,
    );
    painter.line_segment(
        [
            slot.left_bottom() + vec2(3.0, 0.5),
            slot.right_bottom() + vec2(-3.0, 0.5),
        ],
        Stroke::new(1.0, Color32::from_white_alpha(120)),
    );
    Meters {
        places,
        face: VuFace::Bright,
    }
}

pub fn mastering(ui: &mut Ui, rect: Rect, params: &mut Params, travel: f32) -> Meters {
    let w = rect.width();
    let bridge = Rect::from_min_size(rect.min, vec2(w, 150.0));
    let deck = Rect::from_min_max(pos2(rect.left(), bridge.bottom()), rect.max);
    let painter = ui.painter_at(rect);
    // Wood cheeks, then the black deck between them.
    for side in [
        Rect::from_min_size(deck.min, vec2(14.0, deck.height())),
        Rect::from_min_size(
            pos2(deck.right() - 14.0, deck.top()),
            vec2(14.0, deck.height()),
        ),
    ] {
        wood(&painter, side);
    }
    let plate = deck.shrink2(vec2(14.0, 0.0));
    panel(&painter, plate, Color32::from_rgb(40, 40, 43), 0.8);
    scuffs(&painter, plate);
    let at = |x: f32, y: f32| pos2(rect.left() + w * x, rect.top() + y);
    let radius = 124.0;
    let packs = [0.78, 0.55];
    let turn = turns(travel, radius, packs);
    let reels = [at(0.27, 248.0), at(0.73, 248.0)];
    let cover = Rect::from_center_size(at(0.5, 410.0), vec2(w * 0.36, 74.0));
    let guides = [
        pos2(cover.left() + 26.0, cover.top() - 16.0),
        pos2(cover.right() - 26.0, cover.top() - 16.0),
    ];
    let deck_painter = painter.with_clip_rect(plate);
    thread(
        &deck_painter,
        &[
            (reels[0], radius * packs[0], true),
            (guides[0], 12.0, true),
            (guides[1], 12.0, true),
            (reels[1], radius * packs[1], true),
        ],
    );
    let grey = ReelLook {
        metal: Color32::from_rgb(132, 132, 134),
        hub: Color32::from_rgb(34, 34, 36),
        windows: (3, 50.0, 0.42, 0.84),
    };
    let beige = ReelLook {
        metal: Color32::from_rgb(196, 182, 156),
        ..grey
    };
    reel(&deck_painter, reels[0], radius, packs[0], turn[0], grey);
    reel(&deck_painter, reels[1], radius, packs[1], turn[1], beige);
    head_cover(&painter, cover, Color32::from_rgb(34, 34, 36), 30.0);
    roller(
        &painter,
        pos2(cover.right() - 46.0, cover.center().y + 4.0),
        24.0,
        Color32::from_rgb(212, 214, 218),
    );
    for g in guides {
        roller(&painter, g, 12.0, Color32::from_rgb(228, 216, 186));
    }
    // The power switch and its lamp.
    let switch = Rect::from_center_size(
        pos2(cover.left() + 70.0, cover.top() - 18.0),
        vec2(14.0, 22.0),
    );
    painter.rect_filled(switch, 2.0, Color32::from_rgb(20, 20, 22));
    painter.rect_filled(
        Rect::from_min_size(switch.min + vec2(2.0, 2.0), vec2(10.0, 9.0)),
        1.0,
        Color32::from_gray(70),
    );
    gear::lamp(
        &painter,
        switch.right_center() + vec2(14.0, 0.0),
        2.5,
        theme::RECORD,
    );
    // The side panels: the name, and the tape and speed keys.
    let left_panel = Rect::from_min_size(
        pos2(plate.left() + 14.0, deck.top() + 188.0),
        vec2(150.0, 112.0),
    );
    let right_panel = Rect::from_min_size(
        pos2(plate.right() - 14.0 - 176.0, deck.top() + 188.0),
        vec2(176.0, 112.0),
    );
    for p in [left_panel, right_panel] {
        gear::shadow(&painter, p);
        painter.rect_filled(p, 3.0, Color32::from_rgb(232, 230, 224));
        gear::bevel(&painter, p, 0.7);
        let badge = Rect::from_center_size(pos2(p.center().x, p.top() + 15.0), vec2(96.0, 18.0));
        painter.rect_stroke(
            badge,
            4.0,
            Stroke::new(1.5, Color32::from_gray(30)),
            egui::StrokeKind::Inside,
        );
        painter.text(
            badge.center(),
            Align2::CENTER_CENTER,
            "STRADA",
            theme::strong(12.0),
            Color32::from_gray(30),
        );
    }
    painter.text(
        pos2(left_panel.center().x, left_panel.top() + 48.0),
        Align2::CENTER_CENTER,
        "MASTERING",
        theme::strong(13.0),
        Color32::from_gray(40),
    );
    painter.text(
        pos2(left_panel.center().x, left_panel.top() + 74.0),
        Align2::CENTER_CENTER,
        "102",
        theme::strong(26.0),
        Color32::from_gray(40),
    );
    let grid = right_panel.shrink2(vec2(10.0, 0.0));
    let cell = vec2((grid.width() - 4.0) / 2.0, 22.0);
    for (column, id, labels) in [
        (0, "tape", ["GP9", "456", "250"]),
        (1, "speed", ["30", "15", "7.5"]),
    ] {
        for (row, label) in labels.into_iter().enumerate() {
            let value = if id == "tape" {
                row as f32
            } else {
                2.0 - row as f32
            };
            let place = Rect::from_min_size(
                pos2(
                    grid.left() + column as f32 * (cell.x + 4.0),
                    grid.top() + 32.0 + row as f32 * (cell.y + 3.0),
                ),
                cell,
            );
            let lit = params.get(id).round() == value;
            painter.rect_filled(place, 2.0, Color32::from_rgb(44, 44, 46));
            painter.text(
                place.center(),
                Align2::CENTER_CENTER,
                if id == "speed" {
                    format!("{label} ips")
                } else {
                    label.to_owned()
                },
                theme::strong(11.0),
                if lit {
                    Color32::from_rgb(250, 208, 70)
                } else {
                    Color32::from_gray(150)
                },
            );
            if lit {
                painter.rect_stroke(
                    place,
                    2.0,
                    Stroke::new(1.0, Color32::from_rgba_unmultiplied(250, 208, 70, 90)),
                    egui::StrokeKind::Inside,
                );
            }
            click(ui, place, params, id, value);
        }
    }
    // The meter bridge, over the reels.
    gear::sheen(
        &painter,
        Rect::from_min_size(deck.min, vec2(w, 18.0)),
        Color32::from_black_alpha(150),
        Color32::from_black_alpha(0),
    );
    painter.rect_filled(bridge, 0.0, Color32::from_rgb(214, 210, 200));
    let half = (w - 18.0) / 2.0;
    let mut places = [Rect::NOTHING; 2];
    for (k, (title, id, label)) in [
        ("RECORD", "drive", "INPUT"),
        ("REPRODUCE", "output", "OUTPUT"),
    ]
    .into_iter()
    .enumerate()
    {
        let unit = Rect::from_min_size(
            pos2(
                rect.left() + 6.0 + k as f32 * (half + 6.0),
                rect.top() + 6.0,
            ),
            vec2(half, bridge.height() - 12.0),
        );
        panel(&painter, unit, Color32::from_rgb(46, 46, 48), 0.5);
        gear::bevel(&painter, unit, 1.0);
        places[k] = Rect::from_min_size(
            unit.min + vec2(10.0, 6.0),
            vec2(236.0, unit.height() - 12.0),
        );
        let knob_at = pos2(places[k].right() + 96.0, unit.center().y - 4.0);
        let t = params
            .param(id)
            .map_or(0.5, |p| (params.get(id) - p.min) / (p.max - p.min));
        painter.text(
            knob_at - vec2(0.0, 40.0),
            Align2::CENTER_CENTER,
            title,
            theme::strong(9.5),
            Color32::from_gray(220),
        );
        pointer_knob(&painter, knob_at, 17.0, t);
        painter.text(
            knob_at + vec2(0.0, 34.0),
            Align2::CENTER_CENTER,
            label,
            theme::strong(8.5),
            Color32::from_gray(200),
        );
        for (i, mark) in ["0", "2", "4", "6", "8", "10"].into_iter().enumerate() {
            let a = sweep(i as f32 / 5.0);
            painter.text(
                knob_at + vec2(a.cos(), a.sin()) * 28.0,
                Align2::CENTER_CENTER,
                mark,
                FontId::proportional(7.5),
                Color32::from_gray(170),
            );
        }
        let screws = [
            unit.right_top() + vec2(-10.0, 10.0),
            unit.right_bottom() + vec2(-10.0, -10.0),
        ];
        for s in screws {
            screw(&painter, s, 3.5, s.x);
        }
    }
    Meters {
        places,
        face: VuFace::Amber,
    }
}

pub fn vintage(ui: &mut Ui, rect: Rect, params: &mut Params, travel: f32) -> Meters {
    let painter = ui.painter_at(rect);
    let w = rect.width();
    let deck = Rect::from_min_size(rect.min, vec2(w, 300.0));
    let lower = Rect::from_min_max(pos2(rect.left(), deck.bottom()), rect.max);
    let paint = Color32::from_rgb(116, 134, 140);
    panel(&painter, deck, paint, 0.5);
    panel(&painter, lower, Color32::from_rgb(70, 88, 96), 0.5);
    seam(&painter, deck.bottom(), rect.x_range());
    let at = |x: f32, y: f32| pos2(rect.left() + w * x, rect.top() + y);
    let radius = 122.0;
    let packs = [0.8, 0.6];
    let turn = turns(travel, radius, packs);
    let reels = [at(0.27, 104.0), at(0.73, 104.0)];
    let block = Rect::from_center_size(at(0.5, 182.0), vec2(170.0, 76.0));
    let tension = [at(0.085, 240.0), at(0.915, 240.0)];
    let guides = [at(0.29, 250.0), at(0.71, 250.0)];
    // The tape runs round the rollers' bodies, halfway up.
    let waist = |c: Pos2, height: f32| c + vec2(0.0, height / 2.0);
    thread_round(
        &painter,
        &[
            (reels[0], radius * packs[0], false, 1.0),
            (waist(tension[0], 30.0), 34.0, false, SQUASH),
            (waist(guides[0], 34.0), 20.0, true, SQUASH),
            (waist(guides[1], 34.0), 20.0, true, SQUASH),
            (waist(tension[1], 30.0), 34.0, false, SQUASH),
            (reels[1], radius * packs[1], false, 1.0),
        ],
    );
    let tape_y = waist(guides[0], 34.0).y - 20.0 * SQUASH;
    let spokes = ReelLook {
        metal: Color32::from_rgb(196, 198, 200),
        hub: Color32::from_rgb(176, 178, 182),
        windows: (3, 84.0, 0.3, 0.9),
    };
    reel(&painter, reels[0], radius, packs[0], turn[0], spokes);
    reel(&painter, reels[1], radius, packs[1], turn[1], spokes);
    for c in tension {
        cylinder(&painter, c, 34.0, 30.0);
    }
    for c in guides {
        cylinder(&painter, c, 20.0, 34.0);
    }
    // The head block: its top, its front and its badge; the tape runs under it.
    let top = [
        block.left_top() + vec2(12.0, -16.0),
        block.right_top() + vec2(-12.0, -16.0),
        block.right_top(),
        block.left_top(),
    ];
    painter.add(Shape::convex_polygon(
        top.to_vec(),
        Color32::from_rgb(214, 216, 218),
        Stroke::NONE,
    ));
    gear::shadow(&painter, block);
    let mut front = Mesh::default();
    for (x, k) in [(0.0f32, 0.82f32), (0.5, 1.0), (1.0, 0.7)] {
        let c = shade(Color32::from_rgb(186, 188, 190), k);
        front.colored_vertex(pos2(block.left() + block.width() * x, block.top()), c);
        front.colored_vertex(
            pos2(block.left() + block.width() * x, block.bottom()),
            shade(c, 0.85),
        );
    }
    for i in 0..2u32 {
        let j = 2 * i;
        front.add_triangle(j, j + 1, j + 2);
        front.add_triangle(j + 1, j + 3, j + 2);
    }
    painter.add(front);
    gear::bevel(&painter, block, 0.8);
    let badge = Rect::from_center_size(block.center() - vec2(0.0, 10.0), vec2(64.0, 26.0));
    painter.rect_filled(badge, 13.0, Color32::from_rgb(22, 22, 24));
    painter.rect_stroke(
        badge,
        13.0,
        Stroke::new(2.0, Color32::from_gray(210)),
        egui::StrokeKind::Outside,
    );
    painter.text(
        badge.center(),
        Align2::CENTER_CENTER,
        "J37",
        theme::strong(15.0),
        Color32::from_gray(236),
    );
    for s in [
        block.left_bottom() + vec2(14.0, -12.0),
        block.right_bottom() + vec2(-14.0, -12.0),
    ] {
        screw(&painter, s, 4.0, s.x);
    }
    // The head stack down to the tape, which runs against it.
    let heads = Rect::from_min_max(
        pos2(block.left() + 18.0, block.bottom()),
        pos2(block.right() - 18.0, tape_y + 1.0),
    );
    painter.rect_filled(heads, 2.0, Color32::from_rgb(44, 40, 38));
    for k in 0..3 {
        let x = heads.left() + heads.width() * (0.2 + 0.3 * k as f32);
        painter.rect_filled(
            Rect::from_center_size(pos2(x, heads.bottom() - 6.0), vec2(16.0, 10.0)),
            2.0,
            Color32::from_rgb(150, 150, 154),
        );
    }
    painter.hline(heads.x_range().expand(4.0), tape_y, Stroke::new(3.2, TAPE));
    // The lower panel: tape and speed keys, the plaque, the meters.
    let ink = Color32::from_gray(225);
    let mut x = lower.left() + 24.0;
    for (title, id, labels, red) in [
        ("FORMULA", "tape", ["GP9", "456", "250"], false),
        ("SPEED", "speed", ["7.5", "15", "30"], true),
    ] {
        let keys = Rect::from_min_size(pos2(x, lower.top() + 62.0), vec2(3.0 * 44.0 + 12.0, 44.0));
        painter.text(
            pos2(keys.center().x, lower.top() + 34.0),
            Align2::CENTER_CENTER,
            title,
            theme::strong(10.5),
            ink,
        );
        painter.line_segment(
            [
                pos2(keys.left(), lower.top() + 46.0),
                pos2(keys.right(), lower.top() + 46.0),
            ],
            Stroke::new(1.0, ink.gamma_multiply(0.6)),
        );
        for (i, label) in labels.into_iter().enumerate() {
            let place = Rect::from_min_size(
                pos2(keys.left() + i as f32 * 50.0, keys.top()),
                vec2(44.0, 44.0),
            );
            let lit = params.get(id).round() == i as f32;
            key(&painter, place, label, lit, red);
            click(ui, place, params, id, i as f32);
        }
        x = keys.right() + 28.0;
    }
    let plaque = Rect::from_min_size(pos2(x, lower.top() + 44.0), vec2(118.0, 64.0));
    painter.rect_filled(plaque, 3.0, Color32::from_rgb(40, 42, 44));
    painter.rect_stroke(
        plaque,
        3.0,
        Stroke::new(2.0, Color32::from_rgb(150, 154, 156)),
        egui::StrokeKind::Inside,
    );
    for (y, text, size) in [
        (16.0, "STRADA", 9.0),
        (30.0, "RECORDING STUDIOS", 7.0),
        (48.0, "J37  No 1", 12.0),
    ] {
        painter.text(
            pos2(plaque.center().x, plaque.top() + y),
            Align2::CENTER_CENTER,
            text,
            theme::strong(size),
            Color32::from_gray(200),
        );
    }
    let size = vec2(192.0, 128.0);
    let right = lower.right() - 24.0;
    Meters {
        places: [
            Rect::from_min_size(pos2(right - 2.0 * size.x - 14.0, lower.top() + 11.0), size),
            Rect::from_min_size(pos2(right - size.x, lower.top() + 11.0), size),
        ],
        face: VuFace::Cream,
    }
}

// ---------------------------------------------------------------- Parts

/// A color made lighter or darker.
fn shade(c: Color32, k: f32) -> Color32 {
    let f = |x: u8| (f32::from(x) * k).clamp(0.0, 255.0) as u8;
    Color32::from_rgba_premultiplied(f(c.r()), f(c.g()), f(c.b()), c.a())
}

/// Spun metal under a light from the top left: two bright lobes facing each other.
fn spun(a: f32) -> f32 {
    0.9 + 0.2 * (2.0 * a + 4.7).cos() + 0.04 * (6.0 * a + 1.0).cos()
}

/// A ring of spun metal from `r0` to `r1`, lit by the fixed light.
fn ring(painter: &Painter, c: Pos2, r0: f32, r1: f32, metal: Color32, shine: f32) {
    let n = 96;
    let mut mesh = Mesh::default();
    for i in 0..=n {
        let a = i as f32 / n as f32 * TAU;
        let d = vec2(a.cos(), a.sin());
        let k = 1.0 + shine * (spun(a) - 1.0);
        mesh.colored_vertex(c + d * r0, shade(metal, k * 0.96));
        mesh.colored_vertex(c + d * r1, shade(metal, k));
        if i > 0 {
            let j = 2 * i as u32;
            mesh.add_triangle(j - 2, j - 1, j);
            mesh.add_triangle(j - 1, j + 1, j);
        }
    }
    painter.add(mesh);
}

/// The brushed panel of a machine, its light falling from the top.
fn panel(painter: &Painter, rect: Rect, color: Color32, grain: f32) {
    painter.rect_filled(rect, 0.0, color);
    let [r, g, b, _] = color.to_array();
    gear::tile(
        painter,
        rect,
        Texture::Brushed,
        1.0,
        Color32::from_rgba_unmultiplied(r, g, b, (grain * 255.0) as u8),
    );
    gear::sheen(
        painter,
        rect,
        Color32::from_white_alpha(28),
        Color32::from_black_alpha(40),
    );
}

/// A joint between two plates.
fn seam(painter: &Painter, y: f32, x: egui::Rangef) {
    painter.hline(x, y, Stroke::new(1.5, Color32::from_black_alpha(140)));
    painter.hline(x, y + 1.5, Stroke::new(1.0, Color32::from_white_alpha(60)));
}

fn wood(painter: &Painter, rect: Rect) {
    painter.rect_filled(rect, 0.0, Color32::from_rgb(112, 64, 36));
    for k in 0..5 {
        let x = rect.left() + 2.0 + k as f32 * 2.6;
        painter.vline(
            x,
            rect.y_range(),
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(60, 30, 14, 90)),
        );
    }
    gear::bevel(painter, rect, 0.8);
}

/// The wear of a working machine: fine light scratches here and there.
fn scuffs(painter: &Painter, rect: Rect) {
    let mut seed = 0x2545_f491u32;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed as f32 / u32::MAX as f32
    };
    for _ in 0..40 {
        let p = pos2(
            rect.left() + next() * rect.width(),
            rect.top() + next() * rect.height(),
        );
        let a = next() * TAU;
        let l = 4.0 + next() * 14.0;
        painter.line_segment(
            [p, p + vec2(a.cos(), a.sin()) * l],
            Stroke::new(0.8, Color32::from_white_alpha((8.0 + next() * 16.0) as u8)),
        );
    }
}

fn screw(painter: &Painter, c: Pos2, r: f32, turn: f32) {
    painter.circle_filled(c + vec2(0.5, 1.0), r + 0.5, Color32::from_black_alpha(90));
    ring(painter, c, 0.0, r, SCREW, 1.0);
    let d = vec2(turn.cos(), turn.sin()) * r * 0.8;
    painter.line_segment(
        [c - d, c + d],
        Stroke::new(1.2, Color32::from_black_alpha(150)),
    );
}

/// A reel: its spun front flange, the brown pack and the back flange seen through its
/// windows, the hub with its lock.
fn reel(painter: &Painter, c: Pos2, r: f32, pack: f32, angle: f32, look: ReelLook) {
    gear::glow(
        painter,
        c + vec2(4.0, 9.0),
        r * 1.1,
        Color32::from_black_alpha(130),
        Color32::from_black_alpha(0),
    );
    ring(painter, c, 0.0, r, look.metal, 1.0);
    for k in 0..6 {
        let rr = r * (0.3 + 0.12 * k as f32);
        painter.circle_stroke(c, rr, Stroke::new(0.7, Color32::from_white_alpha(10)));
    }
    let (count, span, r1, r2) = look.windows;
    for k in 0..count {
        let middle = angle + k as f32 * TAU / count as f32;
        window(
            painter,
            c,
            r,
            (middle, span.to_radians()),
            (r1 * r, r2 * r),
            (pack * r, look.metal),
        );
    }
    painter.circle_stroke(c, r - 1.0, Stroke::new(1.5, Color32::from_black_alpha(110)));
    painter.circle_stroke(c, r - 2.5, Stroke::new(1.0, Color32::from_white_alpha(50)));
    // The hub, its lock bar across and its screws.
    let hub = r * 0.27;
    painter.circle_filled(c + vec2(1.0, 2.0), hub + 1.5, Color32::from_black_alpha(90));
    ring(painter, c, 0.0, hub, look.hub, 1.0);
    painter.circle_stroke(c, hub, Stroke::new(1.0, Color32::from_black_alpha(120)));
    let cap = r * 0.15;
    ring(painter, c, 0.0, cap, shade(look.metal, 1.06), 1.0);
    let d = vec2(angle.cos(), angle.sin());
    let side = vec2(-d.y, d.x);
    let bar = [
        c - d * hub * 0.92 - side * 5.0,
        c + d * hub * 0.92 - side * 5.0,
        c + d * hub * 0.92 + side * 5.0,
        c - d * hub * 0.92 + side * 5.0,
    ];
    painter.add(Shape::convex_polygon(
        bar.iter().map(|p| *p + vec2(1.0, 2.0)).collect(),
        Color32::from_black_alpha(90),
        Stroke::NONE,
    ));
    painter.add(Shape::convex_polygon(
        bar.to_vec(),
        shade(
            Color32::from_rgb(196, 198, 202),
            0.9 + 0.2 * (2.0 * angle + 4.7).cos(),
        ),
        Stroke::new(1.0, Color32::from_black_alpha(110)),
    ));
    painter.circle_filled(c, 3.0, Color32::from_gray(60));
    for k in 0..3 {
        let a = angle + FRAC_PI_2 + k as f32 * TAU / 3.0;
        screw(painter, c + vec2(a.cos(), a.sin()) * hub * 0.72, 2.2, a);
    }
}

/// A window of a reel's flange, a rounded sector: the pack inside the tape's radius, the back
/// flange beyond, and the cut edge.
fn window(
    painter: &Painter,
    c: Pos2,
    r: f32,
    (middle, span): (f32, f32),
    (r1, r2): (f32, f32),
    (pack, metal): (f32, Color32),
) {
    let corner = r * 0.1;
    let half = span / 2.0;
    let n = 36;
    // Each column's radial extent, its corners rounded.
    let extent = |t: f32| {
        let edge = half - t.abs();
        let round = |d: f32| {
            if d >= corner {
                0.0
            } else {
                corner - (corner * corner - (corner - d).powi(2)).max(0.0).sqrt()
            }
        };
        (r1 + round(edge * r1), r2 - round(edge * r2))
    };
    let column = |i: usize| -half + span * i as f32 / n as f32;
    let back = shade(metal, 0.42);
    let mut mesh = Mesh::default();
    let mut strip = |from: &dyn Fn(f32) -> f32,
                     to: &dyn Fn(f32) -> f32,
                     color: &dyn Fn(f32, f32) -> Color32| {
        let base = mesh.vertices.len() as u32;
        for i in 0..=n {
            let t = column(i);
            let a = middle + t;
            let d = vec2(a.cos(), a.sin());
            let (lo, hi) = (from(t), to(t).max(from(t)));
            mesh.colored_vertex(c + d * lo, color(lo, a));
            mesh.colored_vertex(c + d * hi, color(hi, a));
            if i > 0 {
                let j = base + 2 * i as u32;
                mesh.add_triangle(j - 2, j - 1, j);
                mesh.add_triangle(j - 1, j + 1, j);
            }
        }
    };
    let rings = 5;
    for s in 0..rings {
        let lo = move |t: f32| {
            let (a, b) = extent(t);
            let top = b.min(pack).max(a);
            a + (top - a) * s as f32 / rings as f32
        };
        let hi = move |t: f32| {
            let (a, b) = extent(t);
            let top = b.min(pack).max(a);
            a + (top - a) * (s + 1) as f32 / rings as f32
        };
        let tint = |rr: f32, _a: f32| {
            let k = 0.86 + 0.1 * (rr * 0.7).sin() + 0.12 * (rr / pack).powi(6);
            shade(PACK, k)
        };
        strip(&lo, &hi, &tint);
    }
    let beyond_lo = |t: f32| extent(t).1.min(pack).max(extent(t).0);
    let beyond_hi = |t: f32| extent(t).1;
    let dark = |_rr: f32, a: f32| shade(back, spun(a));
    strip(&beyond_lo, &beyond_hi, &dark);
    painter.add(mesh);
    // The cut edge: dark on the side away from the light, a thin bright lip on the other.
    let mut outline = Vec::with_capacity(2 * n + 2);
    for i in 0..=n {
        let t = column(i);
        let a = middle + t;
        outline.push(c + vec2(a.cos(), a.sin()) * extent(t).1);
    }
    for i in (0..=n).rev() {
        let t = column(i);
        let a = middle + t;
        outline.push(c + vec2(a.cos(), a.sin()) * extent(t).0);
    }
    painter.add(Shape::closed_line(
        outline.iter().map(|p| *p + vec2(0.6, 0.9)).collect(),
        Stroke::new(1.0, Color32::from_white_alpha(60)),
    ));
    painter.add(Shape::closed_line(
        outline,
        Stroke::new(1.6, Color32::from_black_alpha(140)),
    ));
}

/// The tape threaded round circles, each passed on its left (true) or right side of the
/// direction of travel.
fn thread(painter: &Painter, path: &[(Pos2, f32, bool)]) {
    let flat: Vec<_> = path.iter().map(|&(c, r, l)| (c, r, l, 1.0)).collect();
    thread_round(painter, &flat);
}

/// The tape threaded as `thread`, some circles seen at a slant: upright rollers whose round
/// section is squashed in height by the given factor.
fn thread_round(painter: &Painter, path: &[(Pos2, f32, bool, f32)]) {
    for pair in path.windows(2) {
        let [(c1, r1, l1, k1), (c2, r2, l2, k2)] = [pair[0], pair[1]];
        let (a, b) = belt((c1, r1, l1), (c2, r2, l2));
        let a = c1 + vec2(a.x - c1.x, (a.y - c1.y) * k1);
        let b = c2 + vec2(b.x - c2.x, (b.y - c2.y) * k2);
        painter.line_segment([a, b], Stroke::new(3.2, TAPE));
        painter.line_segment(
            [a - vec2(0.0, 0.8), b - vec2(0.0, 0.8)],
            Stroke::new(0.8, Color32::from_rgb(120, 90, 62)),
        );
    }
}

/// The straight run of a belt from one circle to the next, touching each on the given side.
fn belt((c1, r1, l1): (Pos2, f32, bool), (c2, r2, l2): (Pos2, f32, bool)) -> (Pos2, Pos2) {
    let d = c2 - c1;
    let length = d.length().max(1e-3);
    let u = d / length;
    let left = vec2(u.y, -u.x);
    let side = |l: bool| if l { 1.0 } else { -1.0 };
    let (s1, s2) = (side(l1), side(l2));
    // The run's left normal m: perpendicular to the run, so m.u = (s1 r1 - s2 r2) / length.
    let sin = ((s1 * r1 - s2 * r2) / length).clamp(-1.0, 1.0);
    let m = left * (1.0 - sin * sin).sqrt() + u * sin;
    (c1 + m * s1 * r1, c2 + m * s2 * r2)
}

/// A shallow round cup in the panel, in shadow at its top.
fn recess(painter: &Painter, c: Pos2, r: f32, metal: Color32) {
    ring(painter, c, 0.0, r, shade(metal, 0.94), 0.5);
    painter.circle_stroke(c, r, Stroke::new(1.5, Color32::from_black_alpha(80)));
    let top = |a: f32| c + vec2(a.cos(), a.sin()) * (r - 2.0);
    let arc: Vec<Pos2> = (0..=24)
        .map(|i| top(-2.9 + 2.65 * i as f32 / 24.0))
        .collect();
    painter.add(Shape::line(
        arc,
        Stroke::new(3.0, Color32::from_black_alpha(36)),
    ));
    let low: Vec<Pos2> = (0..=24).map(|i| top(0.3 + 2.5 * i as f32 / 24.0)).collect();
    painter.add(Shape::line(
        low,
        Stroke::new(1.2, Color32::from_white_alpha(110)),
    ));
    for k in 0..3 {
        let a = 0.6 + k as f32 * TAU / 3.0;
        screw(painter, c + vec2(a.cos(), a.sin()) * r * 0.7, 3.0, a);
    }
    painter.circle_filled(c, r * 0.2, shade(metal, 0.8));
    painter.circle_stroke(c, r * 0.2, Stroke::new(1.0, Color32::from_black_alpha(90)));
}

/// A tension arm as a bent bracket: its thickness in shade, its top face lit.
fn bracket(painter: &Painter, pivot: Pos2, end: Pos2, metal: Color32) {
    let d = (end - pivot).normalized();
    let elbow = pivot + (end - pivot) * 0.45 + vec2(d.y, -d.x) * 6.0;
    for (from, to) in [(pivot, elbow), (elbow, end)] {
        painter.line_segment(
            [from + vec2(2.0, 4.0), to + vec2(2.0, 4.0)],
            Stroke::new(15.0, Color32::from_black_alpha(70)),
        );
    }
    for (from, to) in [(pivot, elbow), (elbow, end)] {
        painter.line_segment([from, to], Stroke::new(14.0, shade(metal, 0.72)));
        painter.circle_filled(from, 7.0, shade(metal, 0.72));
    }
    for (from, to) in [(pivot, elbow), (elbow, end)] {
        painter.line_segment(
            [from - vec2(0.0, 2.0), to - vec2(0.0, 2.0)],
            Stroke::new(9.0, shade(metal, 1.02)),
        );
        painter.line_segment(
            [from - vec2(0.0, 4.5), to - vec2(0.0, 4.5)],
            Stroke::new(1.5, Color32::from_white_alpha(130)),
        );
    }
    roller(painter, pivot, 8.0, metal);
}

/// The heads' cover, a polished pill with its notch and a slot for the tape.
fn pill_cover(painter: &Painter, rect: Rect) {
    let round = rect.height() / 2.0;
    gear::shadow(painter, rect);
    painter.rect_filled(rect, round, Color32::from_rgb(150, 152, 156));
    let body = rect.shrink(1.5);
    let mut mesh = Mesh::default();
    let stops = [
        (0.0f32, 236.0f32),
        (0.18, 248.0),
        (0.42, 214.0),
        (0.7, 186.0),
        (1.0, 160.0),
    ];
    for (t, g) in stops {
        let y = body.top() + body.height() * t;
        let inset = round
            - (round * round - (y - body.center().y).powi(2))
                .max(0.0)
                .sqrt();
        let c = Color32::from_rgb(g as u8, (g + 2.0) as u8, (g + 5.0).min(255.0) as u8);
        mesh.colored_vertex(pos2(body.left() + inset, y), c);
        mesh.colored_vertex(pos2(body.right() - inset, y), c);
    }
    for i in 0..stops.len() as u32 - 1 {
        let j = 2 * i;
        mesh.add_triangle(j, j + 1, j + 2);
        mesh.add_triangle(j + 1, j + 3, j + 2);
    }
    painter.add(mesh);
    painter.rect_stroke(
        rect,
        round,
        Stroke::new(1.2, Color32::from_black_alpha(140)),
        egui::StrokeKind::Inside,
    );
    // The notch at the bottom, where the tape is seen on the heads.
    let notch =
        Rect::from_center_size(pos2(rect.center().x, rect.bottom() - 9.0), vec2(54.0, 18.0));
    painter.rect_filled(notch, 6.0, Color32::from_rgb(58, 58, 62));
    painter.rect_filled(
        Rect::from_min_max(
            pos2(notch.left() + 6.0, notch.top() + 4.0),
            pos2(notch.right() - 6.0, notch.top() + 9.0),
        ),
        2.0,
        Color32::from_rgb(120, 122, 126),
    );
    painter.hline(
        notch.x_range().shrink(4.0),
        notch.top() + 11.0,
        Stroke::new(2.5, TAPE),
    );
    for x in [rect.left() + round * 0.9, rect.right() - round * 0.9] {
        screw(painter, pos2(x, rect.center().y - 4.0), 3.0, x);
    }
}

/// A machined cap, its rings turned on the lathe.
fn machined(painter: &Painter, c: Pos2, r: f32) {
    gear::glow(
        painter,
        c + vec2(3.0, 6.0),
        r * 1.35,
        Color32::from_black_alpha(130),
        Color32::from_black_alpha(0),
    );
    ring(painter, c, 0.0, r, Color32::from_rgb(206, 208, 212), 1.4);
    let mut rr = r - 2.0;
    let mut k = 0;
    while rr > r * 0.3 {
        let stroke = if k % 2 == 0 {
            Stroke::new(0.8, Color32::from_white_alpha(46))
        } else {
            Stroke::new(0.8, Color32::from_black_alpha(26))
        };
        painter.circle_stroke(c, rr, stroke);
        rr -= 1.7;
        k += 1;
    }
    painter.circle_stroke(c, r, Stroke::new(1.5, Color32::from_black_alpha(150)));
    painter.circle_stroke(c, r - 1.5, Stroke::new(1.0, Color32::from_white_alpha(90)));
    ring(
        painter,
        c,
        0.0,
        r * 0.28,
        Color32::from_rgb(186, 188, 192),
        1.4,
    );
    painter.circle_stroke(
        c,
        r * 0.28,
        Stroke::new(1.0, Color32::from_black_alpha(110)),
    );
    painter.circle_filled(c, 3.0, Color32::from_gray(70));
}

/// A guide roller of spun metal on its post.
fn roller(painter: &Painter, c: Pos2, r: f32, metal: Color32) {
    gear::glow(
        painter,
        c + vec2(2.0, 4.0),
        r * 1.5,
        Color32::from_black_alpha(120),
        Color32::from_black_alpha(0),
    );
    ring(painter, c, 0.0, r, metal, 1.0);
    painter.circle_stroke(c, r, Stroke::new(1.0, Color32::from_black_alpha(130)));
    ring(painter, c, 0.0, r * 0.42, shade(metal, 0.85), 1.0);
    painter.circle_stroke(c, r * 0.42, Stroke::new(0.8, Color32::from_black_alpha(90)));
    painter.circle_filled(c, r * 0.12, Color32::from_gray(50));
}

/// The cover over the heads, a rounded block lit from above.
fn head_cover(painter: &Painter, rect: Rect, color: Color32, round: f32) {
    gear::shadow(painter, rect);
    painter.rect_filled(rect, round, shade(color, 0.86));
    let top = Rect::from_min_max(rect.min, pos2(rect.right(), rect.center().y + 6.0))
        .shrink2(vec2(3.0, 2.0));
    painter.rect_filled(top, round - 2.0, color);
    painter.rect_filled(
        Rect::from_min_max(
            top.min + vec2(round * 0.5, 2.0),
            pos2(top.right() - round * 0.5, top.top() + 8.0),
        ),
        4.0,
        Color32::from_white_alpha(46),
    );
    painter.rect_stroke(
        rect,
        round,
        Stroke::new(1.2, Color32::from_black_alpha(150)),
        egui::StrokeKind::Inside,
    );
    for x in [rect.left() + round * 0.9, rect.right() - round * 0.9] {
        screw(painter, pos2(x, rect.center().y), 3.0, x);
    }
}

/// An upright cylinder seen from above and in front: its body, then its top.
fn cylinder(painter: &Painter, c: Pos2, r: f32, height: f32) {
    gear::glow(
        painter,
        c + vec2(4.0, height * 0.7),
        r * 1.5,
        Color32::from_black_alpha(110),
        Color32::from_black_alpha(0),
    );
    let metal = Color32::from_rgb(196, 198, 202);
    let squash = SQUASH;
    let mut body = Mesh::default();
    let n = 24;
    for i in 0..=n {
        let a = std::f32::consts::PI * i as f32 / n as f32;
        let x = c.x - r * a.cos();
        let k = 0.5 + 0.62 * (a - 1.1).cos().max(0.0).powf(1.5);
        let y_top = c.y + r * squash * a.sin();
        body.colored_vertex(pos2(x, y_top), shade(metal, k));
        body.colored_vertex(pos2(x, y_top + height), shade(metal, k * 0.8));
        if i > 0 {
            let j = 2 * i as u32;
            body.add_triangle(j - 2, j - 1, j);
            body.add_triangle(j - 1, j + 1, j);
        }
    }
    painter.add(body);
    ellipse(painter, c, vec2(r, r * squash), shade(metal, 1.08));
    ellipse(
        painter,
        c,
        vec2(r * 0.45, r * 0.45 * squash),
        shade(metal, 0.8),
    );
    ellipse(
        painter,
        c,
        vec2(r * 0.15, r * 0.15 * squash),
        Color32::from_gray(60),
    );
}

fn ellipse(painter: &Painter, c: Pos2, radius: Vec2, color: Color32) {
    let n = 40;
    let points: Vec<Pos2> = (0..n)
        .map(|i| {
            let a = i as f32 / n as f32 * TAU;
            c + vec2(a.cos() * radius.x, a.sin() * radius.y)
        })
        .collect();
    painter.add(Shape::convex_polygon(
        points,
        color,
        Stroke::new(0.8, Color32::from_black_alpha(90)),
    ));
}

/// The angle of a knob's pointer at `t` of its travel, over 270 degrees.
fn sweep(t: f32) -> f32 {
    (135.0 + 270.0 * t.clamp(0.0, 1.0)).to_radians()
}

/// A black knob with its white pointer.
fn pointer_knob(painter: &Painter, c: Pos2, r: f32, t: f32) {
    painter.circle_filled(c + vec2(1.0, 3.0), r + 1.0, Color32::from_black_alpha(120));
    ring(painter, c, 0.0, r, Color32::from_rgb(40, 40, 42), 1.0);
    ring(
        painter,
        c,
        0.0,
        r * 0.62,
        Color32::from_rgb(62, 62, 64),
        1.0,
    );
    let a = sweep(t);
    let d = vec2(a.cos(), a.sin());
    painter.line_segment(
        [c + d * r * 0.2, c + d * r * 0.95],
        Stroke::new(2.5, Color32::from_gray(236)),
    );
}

/// A chrome selector knob pointing at the chosen label: a label or the knob picks.
fn choice_knob(
    ui: &mut Ui,
    painter: &Painter,
    c: Pos2,
    params: &mut Params,
    id: &str,
    title: &str,
) {
    let Some(param) = params.param(id) else {
        return;
    };
    let count = param.choices.len().max(1);
    let value = params.get(id).round().clamp(0.0, count as f32 - 1.0) as usize;
    let angle_of = |i: usize| (-90.0 + (i as f32 - (count as f32 - 1.0) / 2.0) * 48.0).to_radians();
    let r = 28.0;
    for (i, name) in param.choices.iter().enumerate() {
        let a = angle_of(i);
        let at = c + vec2(a.cos(), a.sin()) * (r + 18.0);
        let text = name.trim_end_matches(" ips");
        let lit = i == value;
        painter.text(
            at,
            Align2::CENTER_CENTER,
            text,
            theme::strong(11.0),
            if lit {
                Color32::from_gray(20)
            } else {
                Color32::from_gray(80)
            },
        );
        painter.line_segment(
            [
                c + vec2(a.cos(), a.sin()) * (r + 3.0),
                c + vec2(a.cos(), a.sin()) * (r + 7.0),
            ],
            Stroke::new(1.2, Color32::from_gray(60)),
        );
        click(
            ui,
            Rect::from_center_size(at, vec2(30.0, 18.0)),
            params,
            id,
            i as f32,
        );
    }
    painter.text(
        c + vec2(0.0, r + 16.0),
        Align2::CENTER_CENTER,
        title,
        theme::strong(11.0),
        Color32::from_gray(40),
    );
    gear::glow(
        painter,
        c + vec2(3.0, 7.0),
        r * 1.5,
        Color32::from_black_alpha(120),
        Color32::from_black_alpha(0),
    );
    ring(painter, c, 0.0, r, Color32::from_rgb(214, 216, 220), 1.0);
    for k in 0..48 {
        let a = k as f32 / 48.0 * TAU;
        let d = vec2(a.cos(), a.sin());
        painter.line_segment(
            [c + d * (r - 3.0), c + d * r],
            Stroke::new(
                1.0,
                Color32::from_black_alpha(if k % 2 == 0 { 70 } else { 20 }),
            ),
        );
    }
    painter.circle_stroke(c, r, Stroke::new(1.0, Color32::from_black_alpha(140)));
    ring(
        painter,
        c,
        0.0,
        r * 0.8,
        Color32::from_rgb(200, 202, 206),
        1.2,
    );
    let a = angle_of(value);
    let d = vec2(a.cos(), a.sin());
    let side = vec2(-d.y, d.x);
    painter.add(Shape::convex_polygon(
        vec![
            c - d * r * 0.9 - side * 4.0,
            c + d * r * 0.9 - side * 2.0,
            c + d * r * 0.9 + side * 2.0,
            c - d * r * 0.9 + side * 4.0,
        ],
        Color32::from_rgb(58, 58, 60),
        Stroke::NONE,
    ));
    let knob = ui.interact(
        Rect::from_center_size(c, Vec2::splat(2.0 * r)),
        ui.scope_id().with(("choice-knob", id)),
        Sense::click(),
    );
    if knob.clicked() {
        params.set(id, Some(((value + 1) % count) as f32), &knob);
        params.doc.history_mut().end_gesture();
    }
}

/// A square push key, lit when chosen.
fn key(painter: &Painter, rect: Rect, label: &str, lit: bool, red: bool) {
    painter.rect_filled(rect.expand(3.0), 4.0, Color32::from_rgb(28, 30, 32));
    let cap = rect.shrink(if lit { 2.0 } else { 0.0 });
    let color = match (lit, red) {
        (true, true) => Color32::from_rgb(214, 70, 52),
        (true, false) => Color32::from_rgb(250, 246, 232),
        (false, true) => Color32::from_rgb(140, 60, 50),
        (false, false) => Color32::from_rgb(196, 192, 182),
    };
    if lit {
        gear::glow(
            painter,
            cap.center(),
            cap.width() * 0.9,
            shade(color, 1.0).gamma_multiply(0.35),
            Color32::TRANSPARENT,
        );
    }
    theme::gradient_rect(painter, cap, shade(color, 0.82), color);
    gear::bevel(painter, cap, 0.8);
    painter.text(
        cap.center(),
        Align2::CENTER_CENTER,
        label,
        theme::strong(12.0),
        if red {
            Color32::from_gray(245)
        } else {
            Color32::from_gray(40)
        },
    );
}

/// A click in `rect` chooses `value` for `id`.
fn click(ui: &mut Ui, rect: Rect, params: &mut Params, id: &str, value: f32) {
    let response = ui
        .interact(
            rect,
            ui.scope_id().with(("tape-key", id, value.to_bits())),
            Sense::click(),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    if response.clicked() {
        params.set(id, Some(value), &response);
        params.doc.history_mut().end_gesture();
    }
}
