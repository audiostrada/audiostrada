//! Strada-Channel's editor, laid out as a console channel: the EQ curve with its handles and the
//! meter bridge, the console, the track's input gain and polarity and the output, then the
//! modules in the order they play, on one grid, dragged by their grip to reorder them.

use audiostrada_core::groups::{self, Link};
use audiostrada_core::routing::{self, Channel};
use audiostrada_dsp::channel::{CHARACTERS, MODULES, module_order, order_code};
use eframe::egui::{
    self, Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, Ui, pos2, vec2,
};

use crate::effect_editors::{Params, display, module_switch, named_knob, pill_switch, segmented};
use crate::i18n::tr;
use crate::{theme, trf};

/// Columns each module takes on the grid, by [`MODULES`].
const COLUMNS: [usize; 5] = [2, 2, 3, 6, 2];
const GAP: f32 = 8.0;
const PAD: f32 = 8.0;
const HEADER: f32 = 30.0;
const SELECTOR: f32 = 28.0;
const ROW: f32 = 84.0;
const METER: f32 = 22.0;
const KNOB: f32 = 38.0;
/// The level the automatic input gain aims at, as consoles like it.
const STAGE: f32 = -18.0;

pub fn show(ui: &mut Ui, params: &mut Params, meters: &[f32]) {
    curve(ui, params, meters);
    ui.add_space(8.0);
    io_bar(ui, params);
    ui.add_space(8.0);
    modules(ui, params, meters);
}

fn family(module: &str) -> &'static str {
    match module {
        "gate" | "comp" => "Dynamics",
        "drive" => "Color",
        _ => "EQ & Filter",
    }
}

/// The response of the filters and the EQ, with a handle per band, and the meter bridge.
fn curve(ui: &mut Ui, params: &mut Params, meters: &[f32]) {
    use audiostrada_dsp::Effect;
    let p = theme::current(ui);
    let (rect, painter) = display(ui, 180.0);
    let bridge = 108.0;
    let graph = Rect::from_min_max(
        rect.min + vec2(40.0, 14.0),
        rect.max - vec2(bridge + 16.0, 20.0),
    );
    let x_of = |f: f32| graph.left() + graph.width() * ((f / 20.0).max(1e-3).log10() / 3.0);
    let f_of =
        |x: f32| 20.0 * 10f32.powf(((x - graph.left()) / graph.width()).clamp(0.0, 1.0) * 3.0);
    let y_of = |db: f32| graph.center().y - db / 18.0 * graph.height() * 0.5;
    let db_of =
        |y: f32| ((graph.center().y - y) / (graph.height() * 0.5) * 18.0).clamp(-15.0, 15.0);
    let small = FontId::proportional(9.0);
    for db in [-12.0f32, -6.0, 0.0, 6.0, 12.0] {
        let y = y_of(db);
        painter.line_segment(
            [pos2(graph.left(), y), pos2(graph.right(), y)],
            Stroke::new(
                1.0,
                p.weak.gamma_multiply(if db == 0.0 { 0.35 } else { 0.12 }),
            ),
        );
        painter.text(
            pos2(graph.left() - 6.0, y),
            Align2::RIGHT_CENTER,
            format!("{db:+.0}"),
            small.clone(),
            p.weak,
        );
    }
    for (f, label) in [
        (50.0, "50"),
        (100.0, "100"),
        (500.0, "500"),
        (1000.0, "1k"),
        (5000.0, "5k"),
        (10_000.0, "10k"),
    ] {
        let x = x_of(f);
        painter.line_segment(
            [pos2(x, graph.top()), pos2(x, graph.bottom())],
            Stroke::new(1.0, p.weak.gamma_multiply(0.1)),
        );
        painter.text(
            pos2(x, graph.bottom() + 4.0),
            Align2::CENTER_TOP,
            label,
            small.clone(),
            p.weak,
        );
    }
    spectra(&painter, graph, meters, &x_of, &p);
    let mut dsp = audiostrada_dsp::channel::Channel::default();
    dsp.prepare(48_000, 64);
    let character = crate::console_ui::character(params.doc, params.channel);
    for (i, (v, q)) in params.values.iter().zip(params.info.params).enumerate() {
        dsp.set_param(
            i,
            if q.id == "character" {
                character as f32
            } else {
                *v
            },
        );
    }
    let points: Vec<Pos2> = (0..=200)
        .map(|i| {
            let x = graph.left() + graph.width() * i as f32 / 200.0;
            let db = dsp.eq_response_db(f64::from(f_of(x))).clamp(-18.0, 18.0) as f32;
            pos2(x, y_of(db))
        })
        .collect();
    for pair in points.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let zero = y_of(0.0);
        painter.add(egui::Shape::convex_polygon(
            vec![a, b, pos2(b.x, zero), pos2(a.x, zero)],
            p.accent.gamma_multiply(0.12),
            Stroke::NONE,
        ));
    }
    painter.add(egui::Shape::line(points, Stroke::new(2.0, p.accent)));
    let eq_on = params.get("eq") >= 0.5;
    let filters_on = params.get("filters") >= 0.5;
    let handles: [(&str, Option<&str>, &str, Color32); 6] = [
        ("hpf", None, "filters", theme::family_color("EQ & Filter")),
        ("low_freq", Some("low_gain"), "eq", band_color(0)),
        ("lmf_freq", Some("lmf_gain"), "eq", band_color(1)),
        ("hmf_freq", Some("hmf_gain"), "eq", band_color(2)),
        ("high_freq", Some("high_gain"), "eq", band_color(3)),
        ("lpf", None, "filters", theme::family_color("EQ & Filter")),
    ];
    for (freq, gain, module, color) in handles {
        let on = if module == "eq" { eq_on } else { filters_on };
        let center = pos2(
            x_of(params.get(freq)),
            gain.map_or(y_of(0.0), |g| y_of(params.get(g))),
        );
        let response = ui.interact(
            Rect::from_center_size(center, vec2(18.0, 18.0)),
            ui.scope_id().with(("strip-handle", freq)),
            Sense::drag(),
        );
        let hot = response.hovered() || response.dragged();
        painter.circle_filled(
            center,
            if hot { 7.5 } else { 6.0 },
            if on { color } else { color.gamma_multiply(0.3) },
        );
        painter.circle_stroke(
            center,
            if hot { 7.5 } else { 6.0 },
            Stroke::new(1.0, Color32::BLACK.gamma_multiply(0.5)),
        );
        if hot {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        if response.drag_started() && !on {
            // Grabbing a handle switches its module on.
            params.set(module, Some(1.0), &response);
        }
        if let Some(pos) = response
            .interact_pointer_pos()
            .filter(|_| response.dragged())
        {
            let range = params
                .param(freq)
                .map_or((20.0, 20_000.0), |p| (p.min, p.max));
            params.set(freq, Some(f_of(pos.x).clamp(range.0, range.1)), &response);
            if let Some(g) = gain {
                params.set(g, Some(db_of(pos.y)), &response);
            }
        } else {
            params.set(freq, None, &response);
        }
    }
    bridge_meters(
        &painter,
        Rect::from_min_max(
            pos2(rect.right() - bridge - 6.0, graph.top()),
            pos2(rect.right() - 6.0, graph.bottom()),
        ),
        meters,
        &p,
    );
}

/// The output's spectrum filled and the input's as a line, -90 to 0 dBFS over the graph's
/// height, behind the EQ curve: what the EQ is given and what it does to it.
fn spectra(
    painter: &egui::Painter,
    graph: Rect,
    meters: &[f32],
    x_of: &dyn Fn(f32) -> f32,
    p: &theme::Palette,
) {
    use audiostrada_dsp::analyzer::{BANDS, band_frequency};
    use audiostrada_dsp::channel::{INPUT_SPECTRUM, OUTPUT_SPECTRUM};
    let points = |levels: &[f32]| -> Vec<Pos2> {
        let mut points: Vec<Pos2> = levels
            .iter()
            .enumerate()
            .map(|(i, m)| {
                let level = ((m + 90.0) / 90.0).clamp(0.0, 1.0);
                pos2(
                    x_of(band_frequency(i)).min(graph.right()),
                    graph.bottom() - level * graph.height(),
                )
            })
            .collect();
        points.dedup_by(|a, b| (a.x - b.x).abs() < 0.5);
        points
    };
    let band = |start: usize| meters.get(start..start + BANDS);
    // Nothing measured yet (the window just opened): nothing drawn.
    let heard = |levels: &[f32]| levels.iter().any(|m| *m > -119.0);
    if let Some(input) = band(INPUT_SPECTRUM).filter(|l| heard(l)) {
        painter.add(egui::Shape::line(
            points(input),
            Stroke::new(1.0, p.weak.gamma_multiply(0.45)),
        ));
    }
    if let Some(output) = band(OUTPUT_SPECTRUM).filter(|l| heard(l)) {
        let line = points(output);
        let mut mesh = egui::Mesh::default();
        let (top, bottom) = (p.accent.gamma_multiply(0.18), p.accent.gamma_multiply(0.02));
        for pair in line.windows(2) {
            let i = mesh.vertices.len() as u32;
            for (q, c) in [
                (pair[0], top),
                (pair[1], top),
                (pos2(pair[1].x, graph.bottom()), bottom),
                (pos2(pair[0].x, graph.bottom()), bottom),
            ] {
                mesh.colored_vertex(q, c);
            }
            mesh.add_triangle(i, i + 1, i + 2);
            mesh.add_triangle(i, i + 2, i + 3);
        }
        painter.add(egui::Shape::mesh(mesh));
        painter.add(egui::Shape::line(
            line,
            Stroke::new(1.0, p.accent.gamma_multiply(0.45)),
        ));
    }
}

fn band_color(band: usize) -> Color32 {
    [
        Color32::from_rgb(236, 120, 90),
        Color32::from_rgb(230, 190, 80),
        Color32::from_rgb(110, 200, 120),
        Color32::from_rgb(110, 170, 240),
    ][band.min(3)]
}

/// Input, gate, gain reduction and output, as bars on a shared scale.
fn bridge_meters(painter: &egui::Painter, area: Rect, meters: &[f32], p: &theme::Palette) {
    let value = |k: usize| meters.get(k).copied().unwrap_or(-120.0);
    let level = |db: f32| ((db + 60.0) / 60.0).clamp(0.0, 1.0);
    let bars = [
        (tr("In"), level(value(2)), false, p.accent),
        (
            tr("Gate"),
            (-value(0)).clamp(0.0, 40.0) / 40.0,
            true,
            theme::PLAY,
        ),
        (
            tr("GR"),
            value(1).abs().min(24.0) / 24.0,
            true,
            theme::family_color("Dynamics"),
        ),
        (tr("Out"), level(value(3)), false, p.accent),
    ];
    let width = 12.0;
    let step = area.width() / bars.len() as f32;
    let small = FontId::proportional(9.0);
    for (k, (label, share, from_top, color)) in bars.into_iter().enumerate() {
        let x = area.left() + step * (k as f32 + 0.5) - width / 2.0;
        let bar = Rect::from_min_max(pos2(x, area.top()), pos2(x + width, area.bottom()));
        painter.rect_filled(bar, 3.0, p.panel);
        let h = bar.height() * share;
        let lit = if from_top {
            Rect::from_min_max(bar.min, pos2(bar.right(), bar.top() + h))
        } else {
            Rect::from_min_max(pos2(bar.left(), bar.bottom() - h), bar.max)
        };
        painter.rect_filled(lit, 3.0, color);
        painter.text(
            pos2(bar.center().x, bar.bottom() + 4.0),
            Align2::CENTER_TOP,
            label,
            small.clone(),
            p.weak,
        );
    }
}

/// The console, the track's input gain and polarity with a button to set the gain, and the
/// output, along one bar.
fn io_bar(ui: &mut Ui, params: &mut Params) {
    let p = theme::current(ui);
    egui::Frame::new()
        .fill(p.base)
        .corner_radius(8.0)
        .inner_margin(egui::Margin::symmetric(12, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                console_button(ui, params);
                ui.add_space(12.0);
                if let Channel::Track(track) = params.channel {
                    input_gain(ui, params, track);
                }
                ui.add_space((ui.available_width() - KNOB - 24.0).max(0.0));
                ui.vertical(|ui| {
                    named_knob(ui, params, "output", tr("Output"), 34.0, p.accent);
                });
            });
        });
}

fn console_button(ui: &mut Ui, params: &mut Params) {
    let p = theme::current(ui);
    ui.vertical(|ui| {
        ui.label(RichText::new(tr("Console")).size(10.5).color(p.weak));
        let console = crate::console_ui::of_channel(params.doc, params.channel);
        let mut text = RichText::new(crate::console_ui::name(console.model)).strong();
        if console.colors() {
            text = text.color(crate::console_ui::color(console.model));
        }
        let channel = params.channel;
        let button = ui.button(text).on_hover_text(tr(
            "The console this channel plays through: its preamp colors the track, and the EQ \
             and the drive take its character",
        ));
        // Stays open while its settings change.
        egui::Popup::menu(&button)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| match channel {
                Channel::Track(t) => crate::console_ui::track_menu(ui, params.doc, t),
                Channel::Master => crate::console_ui::console_menu(ui, params.doc),
                Channel::Clip(_) => {}
            });
        let character = crate::console_ui::character(params.doc, params.channel);
        ui.label(
            RichText::new(trf!(
                "Character: {name}",
                name = tr(CHARACTERS[character.min(CHARACTERS.len() - 1)])
            ))
            .size(10.0)
            .color(p.weak),
        );
    });
}

/// The track's input gain as a knob, its polarity, and a button that sets the gain so the
/// track plays around the level consoles like.
fn input_gain(ui: &mut Ui, params: &mut Params, track: audiostrada_core::TrackId) {
    let p = theme::current(ui);
    let Some((trim, invert)) = params
        .doc
        .project()
        .track(track)
        .map(|t| (t.trim_db, t.invert))
    else {
        return;
    };
    ui.vertical(|ui| {
        ui.set_width(KNOB + 22.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new(tr("Input")).size(10.5).color(p.weak));
            let (changed, response) =
                crate::widgets::knob(ui, trim, (-24.0, 24.0), 0.0, 34.0, true, p.accent);
            let response = response.on_hover_text(tr(
                "The track's input gain, before everything else. Drag, double-click to reset",
            ));
            if let Some(value) = changed {
                let value = (value * 10.0).round() / 10.0;
                params.doc.history_mut().edit_merged(
                    tr("Trim"),
                    &format!("trim:{}", track.0),
                    |p| {
                        if let Some(t) = p.track_mut(track) {
                            t.trim_db = value;
                        }
                    },
                );
            }
            crate::mixer::end_gesture(params.doc.history_mut(), &response);
            ui.label(
                RichText::new(format!("{trim:+.1} dB"))
                    .monospace()
                    .size(11.0),
            );
        });
    });
    ui.vertical(|ui| {
        ui.add_space(14.0);
        let response = pill_switch(ui, invert, "Ø").on_hover_text(tr("Invert polarity"));
        if response.clicked() {
            params.doc.history_mut().edit(tr("Polarity"), |p| {
                groups::set_switch(p, track, Link::Polarity, !invert)
            });
        }
    });
    ui.vertical(|ui| {
        ui.add_space(14.0);
        let id = ui.scope_id().with("strip-stage");
        let response = ui
            .button(tr("Auto"))
            .on_hover_text(tr("Sets the input gain so the track plays around -18 dBFS"));
        if response.clicked() {
            let project = params.doc.project().clone();
            let note = match crate::console_ui::working_level(params.doc, &project, track) {
                Some(level) => {
                    let value = ((STAGE - level + trim) * 10.0).round() / 10.0;
                    let value = value.clamp(-24.0, 24.0);
                    params.doc.history_mut().edit(tr("Gain staging"), |p| {
                        if let Some(t) = p.track_mut(track) {
                            t.trim_db = value;
                        }
                    });
                    trf!("Input set to {gain} dB", gain = format!("{value:+.1}"))
                }
                None => tr("No audio on this track yet").to_owned(),
            };
            ui.data_mut(|d| d.insert_temp(id, note));
        }
        if let Some(note) = ui.data(|d| d.get_temp::<String>(id)) {
            // Small, under the button, without taking room.
            let at = response.rect.left_bottom() + vec2(0.0, 3.0);
            ui.painter().text(
                at,
                Align2::LEFT_TOP,
                note,
                FontId::proportional(10.0),
                p.weak,
            );
        }
    });
}

/// The modules on one grid, in the order they play.
fn modules(ui: &mut Ui, params: &mut Params, meters: &[f32]) {
    let p = theme::current(ui);
    let order = module_order(params.get("order"));
    let drag_id = ui.scope_id().with("strip-module-drag");
    let dragged: Option<usize> = ui.data(|d| d.get_temp(drag_id));
    let full = ui.available_width();
    let columns: usize = COLUMNS.iter().sum();
    let column = (full - GAP * (MODULES.len() - 1) as f32 - 2.0 * PAD * MODULES.len() as f32)
        / columns as f32;
    let height = HEADER + SELECTOR + 2.0 * ROW + METER + PAD;
    let top = ui.cursor().min;
    let mut x = top.x;
    let mut frames = [Rect::NOTHING; 5];
    for (slot, &m) in order.iter().enumerate() {
        let module = MODULES[m];
        let width = column * COLUMNS[m] as f32 + 2.0 * PAD;
        let card = Rect::from_min_size(pos2(x, top.y), vec2(width, height));
        frames[slot] = card;
        x += width + GAP;
        let tint = theme::family_color(family(module));
        ui.painter().rect_filled(card, 8.0, theme::screen(&p));
        if dragged == Some(m) {
            ui.painter().rect_stroke(
                card,
                8.0,
                Stroke::new(1.5, p.accent),
                egui::StrokeKind::Inside,
            );
        }
        let inner = card.shrink(PAD);
        // The grip and the module's switch.
        let header = Rect::from_min_size(inner.min, vec2(inner.width(), HEADER - 4.0));
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(header)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
            |ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let (grip, response) = ui.allocate_exact_size(vec2(10.0, 22.0), Sense::drag());
                let hot = response.hovered() || response.dragged();
                let ink = if hot {
                    p.text
                } else {
                    p.weak.gamma_multiply(0.6)
                };
                for k in 0..6 {
                    let dot = pos2(
                        grip.left() + 3.0 + (k % 2) as f32 * 4.0,
                        grip.center().y - 4.0 + (k / 2) as f32 * 4.0,
                    );
                    ui.painter().circle_filled(dot, 1.1, ink);
                }
                if hot {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                }
                if response.drag_started() {
                    ui.data_mut(|d| d.insert_temp(drag_id, m));
                }
                response.on_hover_text(tr("Drag to change the order of the modules"));
                module_switch(ui, params, module, tr(title(module)), tint);
            },
        );
        let on = params.get(module) >= 0.5;
        let tint = if on { tint } else { p.weak };
        let grid = Grid {
            left: inner.left(),
            top: inner.top() + HEADER,
            column,
            columns: COLUMNS[m],
        };
        match module {
            "filters" => {
                grid.span(ui, 0, 0, 2, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        let steep = params.get("slope") >= 0.5;
                        for (k, label) in ["12 dB", "24 dB"].into_iter().enumerate() {
                            let response = ui
                                .selectable_label(steep == (k == 1), label)
                                .on_hover_text(tr("Slope of both filters, per octave"));
                            let changed = response.clicked().then_some(k as f32);
                            params.set("slope", changed, &response);
                        }
                    });
                });
                grid.knob(ui, params, 1, 0, "hpf", tr("High pass"), tint);
                grid.knob(ui, params, 1, 1, "lpf", tr("Low pass"), tint);
            }
            "gate" => {
                grid.knob(ui, params, 1, 0, "gate_threshold", tr("Threshold"), tint);
                grid.knob(ui, params, 1, 1, "gate_range", tr("Range"), tint);
                grid.knob(ui, params, 2, 0, "gate_attack", tr("Attack"), tint);
                grid.knob(ui, params, 2, 1, "gate_release", tr("Release"), tint);
                let closed = (-meters.first().copied().unwrap_or(0.0)).clamp(0.0, 40.0) / 40.0;
                grid.meter(ui, (1.0 - closed, tr("Open")), theme::PLAY, on);
            }
            "comp" => {
                grid.span(ui, 0, 0, 3, |ui| {
                    ui.horizontal(|ui| {
                        segmented(
                            ui,
                            params,
                            "comp_type",
                            &audiostrada_dsp::compressor::TYPE_HINTS,
                        )
                    });
                });
                grid.knob(ui, params, 1, 0, "comp_threshold", tr("Threshold"), tint);
                grid.knob(ui, params, 1, 1, "comp_ratio", tr("Ratio"), tint);
                grid.knob(ui, params, 1, 2, "comp_makeup", tr("Makeup"), tint);
                grid.knob(ui, params, 2, 0, "comp_attack", tr("Attack"), tint);
                grid.knob(ui, params, 2, 1, "comp_release", tr("Release"), tint);
                grid.knob(ui, params, 2, 2, "comp_mix", tr("Mix"), tint);
                let reduction = meters.get(1).copied().unwrap_or(0.0).abs().min(24.0);
                grid.meter(
                    ui,
                    (reduction / 24.0, format!("-{reduction:.1} dB")),
                    theme::family_color("Dynamics"),
                    on,
                );
            }
            "eq" => {
                for (column, id) in [(0, "low_bell"), (5, "high_bell")] {
                    grid.span(ui, 0, column, 1, |ui| {
                        let bell = params.get(id) >= 0.5;
                        let response = pill_switch(ui, bell, tr("Bell"))
                            .on_hover_text(tr("A bell around its frequency instead of a shelf"));
                        let changed = response.clicked().then_some(if bell { 0.0 } else { 1.0 });
                        params.set(id, changed, &response);
                    });
                }
                let band = |b: usize| if on { band_color(b) } else { p.weak };
                grid.knob(ui, params, 1, 0, "low_gain", tr("Low"), band(0));
                grid.span(ui, 1, 1, 2, |ui| {
                    named_knob(ui, params, "lmf_gain", tr("Low mid"), KNOB, band(1));
                });
                grid.span(ui, 1, 3, 2, |ui| {
                    named_knob(ui, params, "hmf_gain", tr("High mid"), KNOB, band(2));
                });
                grid.knob(ui, params, 1, 5, "high_gain", tr("High"), band(3));
                grid.knob(ui, params, 2, 0, "low_freq", tr("Freq"), band(0));
                grid.knob(ui, params, 2, 1, "lmf_freq", tr("Freq"), band(1));
                grid.knob(ui, params, 2, 2, "lmf_q", "Q", band(1));
                grid.knob(ui, params, 2, 3, "hmf_freq", tr("Freq"), band(2));
                grid.knob(ui, params, 2, 4, "hmf_q", "Q", band(2));
                grid.knob(ui, params, 2, 5, "high_freq", tr("Freq"), band(3));
            }
            _ => {
                grid.span(ui, 1, 0, 2, |ui| {
                    named_knob(ui, params, "drive_amount", tr("Amount"), KNOB, tint);
                });
            }
        }
    }
    ui.allocate_rect(
        Rect::from_min_max(top, pos2(top.x + full, top.y + height)),
        Sense::hover(),
    );
    let Some(m) = dragged else {
        return;
    };
    // Where the dragged module would go: among the others, by the pointer's x.
    let others: Vec<usize> = order.iter().copied().filter(|&o| o != m).collect();
    let rect_of = |module: usize| frames[order.iter().position(|&o| o == module).unwrap_or(0)];
    let x = ui.input(|i| i.pointer.hover_pos()).map_or(0.0, |pos| pos.x);
    let target = others
        .iter()
        .filter(|&&o| rect_of(o).center().x < x)
        .count();
    let line_x = match target {
        0 => rect_of(others[0]).left() - GAP / 2.0,
        t => rect_of(others[t - 1]).right() + GAP / 2.0,
    };
    ui.painter().line_segment(
        [pos2(line_x, top.y), pos2(line_x, top.y + height)],
        Stroke::new(2.0, p.accent),
    );
    if ui.input(|i| !i.pointer.any_down()) {
        ui.data_mut(|d| d.remove::<usize>(drag_id));
        let mut new = [m; 5];
        let mut rest = others.iter();
        for (k, slot) in new.iter_mut().enumerate() {
            if k != target {
                *slot = *rest.next().unwrap_or(&m);
            }
        }
        if new != order {
            let code = order_code(new);
            if let Some(i) = params.info.params.iter().position(|q| q.id == "order") {
                params.values[i] = code;
            }
            let (channel, insert) = (params.channel, params.insert);
            params
                .doc
                .history_mut()
                .edit(tr("Reorder the channel strip"), |p| {
                    routing::set_insert_param(p, channel, insert, "order", code)
                });
        }
    }
}

fn title(module: &str) -> &'static str {
    match module {
        "filters" => "Filters",
        "gate" => "Gate",
        "comp" => "Compressor",
        "eq" => "EQ",
        _ => "Drive",
    }
}

/// A module's cells: a row of selectors, two rows of knobs and a row for its meter.
struct Grid {
    left: f32,
    top: f32,
    column: f32,
    columns: usize,
}

impl Grid {
    fn cell(&self, row: usize, column: usize, span: usize) -> Rect {
        let y = match row {
            0 => self.top,
            r => self.top + SELECTOR + (r as f32 - 1.0) * ROW,
        };
        let height = match row {
            0 => SELECTOR,
            3 => METER,
            _ => ROW,
        };
        Rect::from_min_size(
            pos2(self.left + column as f32 * self.column, y),
            vec2(span as f32 * self.column, height),
        )
    }

    fn span(&self, ui: &mut Ui, row: usize, column: usize, span: usize, add: impl FnOnce(&mut Ui)) {
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(self.cell(row, column, span))
                .layout(egui::Layout::top_down(egui::Align::Center)),
            add,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn knob(
        &self,
        ui: &mut Ui,
        params: &mut Params,
        row: usize,
        column: usize,
        id: &str,
        label: &str,
        tint: Color32,
    ) {
        self.span(ui, row, column, 1, |ui| {
            named_knob(ui, params, id, label, KNOB, tint);
        });
    }

    /// A bar across the module, `share` lit, with its reading.
    fn meter(&self, ui: &Ui, (share, text): (f32, impl Into<String>), color: Color32, on: bool) {
        let p = theme::current(ui);
        let cell = self.cell(3, 0, self.columns).shrink2(vec2(4.0, 6.0));
        let painter = ui.painter();
        painter.rect_filled(cell, 3.0, p.panel);
        if !on {
            return;
        }
        {
            let lit = Rect::from_min_max(
                cell.min,
                pos2(
                    cell.left() + cell.width() * share.clamp(0.0, 1.0),
                    cell.bottom(),
                ),
            );
            painter.rect_filled(lit, 3.0, color.gamma_multiply(0.8));
        }
        painter.text(
            cell.right_center() + vec2(-3.0, 0.0),
            Align2::RIGHT_CENTER,
            text.into(),
            FontId::monospace(9.0),
            p.text,
        );
    }
}
