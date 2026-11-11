//! The automation editor: a large window over the timeline for the selected tracks, each with
//! its waveform behind the curve it shows, the tools above, and room to draw.

use audiostrada_core::automation::{AutomationLane, AutomationMode, CurveShape};

use super::automation::{AutoTool, LaneCommand, LaneKey, LaneView};
use super::toolbar::{Glyph, button, separator};
use super::*;

/// The least height of a track's band: past it, the bands scroll.
const BAND: f32 = 200.0;
/// Width of the column naming each track and its curve.
const NAMES: f32 = 176.0;
const RULER: f32 = 34.0;
const TOOLS: f32 = 36.0;

/// The editor's own view of time, apart from the timeline's.
#[derive(Debug)]
pub(super) struct AutoEditor {
    zoom: f64,
    scroll: f64,
    /// How far down the bands are scrolled, when they do not all fit.
    scroll_y: f32,
    /// The height asked for each band, by Alt+wheel or by dragging a band's lower edge.
    band: f32,
    /// Points snap as the timeline's positions do; its own switch, set as the timeline's
    /// when it opens.
    snap: Option<bool>,
    /// Where the zoom tool's drag began.
    zoom_from: Option<f32>,
}

impl AutoEditor {
    /// An editor showing what the timeline shows.
    pub(super) fn at(zoom: f64, scroll: f64) -> Self {
        Self {
            zoom,
            scroll,
            scroll_y: 0.0,
            band: BAND,
            snap: None,
            zoom_from: None,
        }
    }
}

impl TimelineView {
    /// While the editor is open, the plain keys that change its tool, taken before the
    /// shortcuts.
    pub fn take_automation_keys(&mut self, ctx: &egui::Context) {
        if self.auto_editor.is_none() || ctx.text_edit_focused() {
            return;
        }
        use super::automation::AutoTool;
        for (key, tool) in [
            (egui::Key::P, AutoTool::Pencil),
            (egui::Key::S, AutoTool::Select),
            (egui::Key::E, AutoTool::Erase),
            (egui::Key::Z, AutoTool::Zoom),
        ] {
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, key)) {
                self.auto_tool = tool;
            }
        }
    }

    /// Opens the editor on `target` of `track`, selecting the track if it is not.
    pub(super) fn open_auto_editor(
        &mut self,
        doc: &mut Document,
        track: TrackId,
        target: AutomationTarget,
    ) {
        self.lane_choice.insert(track, target);
        if !doc.is_track_selected(track) {
            doc.select_track(track, SelectMode::Only);
        }
        if self.auto_editor.is_none() {
            self.auto_editor = Some(AutoEditor::at(self.zoom, self.scroll));
        }
    }

    /// The editor's window, while it is open.
    pub(super) fn automation_window(
        &mut self,
        ctx: &egui::Context,
        doc: &mut Document,
        project: &Project,
        settings: &EditSettings,
    ) {
        if self.auto_editor.is_none() {
            return;
        }
        let screen = ctx.content_rect();
        let mut open = true;
        // Named after what it edits: "Automation · Lead vocal: Volume".
        let mut title = tr("Automation").to_owned();
        let shown: Vec<&Track> = doc
            .selected_tracks()
            .iter()
            .filter_map(|id| project.track(*id))
            .collect();
        let names: Vec<String> = shown
            .iter()
            .map(|track| {
                let target = self.lane_choice(track);
                let what = self.target_info(doc, track, &target).name;
                format!("{}: {what}", track.name)
            })
            .collect();
        if !names.is_empty() {
            title = format!("{title} · {}", names.join(", "));
        }
        egui::Window::new(title)
            .id(egui::Id::unique("automation-editor"))
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_size(vec2(screen.width() * 0.82, screen.height() * 0.62))
            .default_pos(pos2(
                screen.left() + screen.width() * 0.09,
                screen.top() + screen.height() * 0.2,
            ))
            .min_size(vec2(560.0, 260.0))
            .show(ctx, |ui| self.automation_editor(ui, doc, project, settings));
        if !open || ctx.input(|i| i.key_pressed(egui::Key::Escape)) && self.lane_drag.is_none() {
            self.auto_editor = None;
        }
    }

    fn automation_editor(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        project: &Project,
        settings: &EditSettings,
    ) {
        let visuals = ui.visuals().clone();
        let tracks: Vec<&Track> = doc
            .selected_tracks()
            .iter()
            .filter_map(|id| project.track(*id))
            .collect();
        self.editor_tools(ui, project, &tracks);
        let all = ui.available_rect_before_wrap();
        ui.allocate_rect(all, Sense::hover());
        if tracks.is_empty() {
            ui.painter().text(
                all.center(),
                Align2::CENTER_CENTER,
                tr("Select a track to draw its automation here"),
                FontId::proportional(13.0),
                visuals.weak_text_color(),
            );
            return;
        }
        let ruler = Rect::from_min_max(
            pos2(all.left() + NAMES, all.top()),
            pos2(all.right(), all.top() + RULER),
        );
        let lanes = Rect::from_min_max(pos2(ruler.left(), ruler.bottom()), all.max);
        // Each track keeps a comfortable band; past the window's height, they scroll.
        let asked = self.auto_editor.as_ref().map_or(BAND, |e| e.band);
        let height = (lanes.height() / tracks.len() as f32).max(asked);
        let overflow = (height * tracks.len() as f32 - lanes.height()).max(0.0);
        let Some(editor) = &mut self.auto_editor else {
            return;
        };
        // The wheel over the lanes: Ctrl zooms around the pointer, else it moves in time.
        if let Some(pointer) = ui
            .input(|i| i.pointer.hover_pos())
            .filter(|p| all.contains(*p))
        {
            let (zoom, wheel) = ui.input(|i| (i.zoom_delta(), i.smooth_scroll_delta));
            if (zoom - 1.0).abs() > 1e-3 {
                let at = editor.scroll + f64::from(pointer.x - lanes.left()) * editor.zoom;
                editor.zoom = (editor.zoom / f64::from(zoom)).clamp(MIN_ZOOM, MAX_ZOOM);
                editor.scroll = at - f64::from(pointer.x - lanes.left()) * editor.zoom;
            }
            // Bands that do not all fit scroll with the wheel; Shift (or a sideways wheel)
            // then moves in time.
            if ui.input(|i| i.modifiers.alt) {
                // Alt+wheel: taller or shorter bands, as the timeline's tracks.
                editor.band = (height + (wheel.x + wheel.y) * 0.5).clamp(90.0, 900.0);
            } else if overflow > 0.0 {
                editor.scroll_y -= wheel.y;
                editor.scroll -= f64::from(wheel.x) * editor.zoom;
            } else {
                editor.scroll -= f64::from(wheel.x + wheel.y) * editor.zoom;
            }
        }
        editor.scroll = editor.scroll.max(0.0);
        editor.scroll_y = editor.scroll_y.clamp(0.0, overflow);
        let scroll_y = editor.scroll_y;
        let scale = Scale {
            left: lanes.left(),
            scroll: editor.scroll,
            zoom: editor.zoom,
        };
        // The clips and their waveforms are drawn at the editor's zoom.
        let timeline_zoom = std::mem::replace(&mut self.zoom, scale.zoom);
        let painter = ui.painter().with_clip_rect(all);
        self.draw_ruler(
            &painter.with_clip_rect(ruler),
            ruler,
            project,
            scale,
            &visuals,
        );
        let ruler_response =
            ui.interact(ruler, ui.scope_id().with("ruler"), Sense::click_and_drag());
        if let Some(pos) = ruler_response.interact_pointer_pos()
            && (ruler_response.clicked() || ruler_response.dragged())
        {
            doc.player_mut().locate(scale.sample(pos.x));
        }
        // The editor's own snap switch over the timeline's settings.
        let mut own = *settings;
        let snap = self
            .auto_editor
            .as_mut()
            .map(|e| *e.snap.get_or_insert(settings.snapping()))
            .unwrap_or(false);
        own.snap = snap;
        if !snap && own.mode == EditMode::Grid {
            own.mode = EditMode::Slip;
        }
        let magnet = magnet::Magnet::new(&own, project, scale.zoom, doc.player().position(), &[]);
        self.automation = true;
        let ruler_painter = painter.clone();
        // The bands and their names stay under the ruler when scrolled.
        let bands = Rect::from_min_max(pos2(all.left(), lanes.top()), all.max);
        let painter = painter.with_clip_rect(bands);
        for (k, track) in tracks.iter().enumerate() {
            let rect = Rect::from_min_size(
                pos2(lanes.left(), lanes.top() + k as f32 * height - scroll_y),
                vec2(lanes.width(), height),
            )
            .shrink2(vec2(0.0, 1.0));
            if rect.bottom() < lanes.top() || rect.top() > lanes.bottom() {
                continue;
            }
            let color = crate::theme::track_color(track.color);
            let lane_painter = painter.with_clip_rect(rect);
            lane_painter.rect_filled(rect, 0.0, visuals.extreme_bg_color);
            self.draw_grid(&lane_painter, rect, project, scale, &visuals);
            // The track's clips behind the curve: where the sound is, to draw against it.
            let ghost = color.gamma_multiply(0.42);
            for clip in &track.clips {
                let body = Rect::from_min_max(
                    pos2(scale.x(clip.start), rect.top() + 6.0),
                    pos2(scale.x(clip.end()), rect.bottom() - 6.0),
                );
                if body.right() < rect.left() || body.left() > rect.right() {
                    continue;
                }
                lane_painter.rect_filled(body, 4.0, color.gamma_multiply(0.07));
                match &clip.notes {
                    Some(notes) => draw_notes(&lane_painter, body, clip, notes, scale.zoom, ghost),
                    None => self.draw_waveform(&lane_painter, body, doc, project, clip, ghost),
                }
            }
            let target = self.lane_choice(track);
            let info = self.target_info(doc, track, &target);
            let key = LaneKey::Track(track.id, target.clone());
            let lane = key
                .lane(doc.project())
                .cloned()
                .unwrap_or_else(|| AutomationLane::new(target));
            let reading = track.automation_mode != AutomationMode::Off;
            let ink = if reading && lane.enabled {
                draw::mix(
                    color,
                    Color32::WHITE,
                    if visuals.dark_mode { 0.3 } else { 0.0 },
                )
            } else {
                color.gamma_multiply(0.4)
            };
            draw_scale(&lane_painter, rect, &info, &visuals);
            if lane.points.is_empty() {
                lane_painter.text(
                    pos2(rect.center().x, rect.top() + 14.0),
                    Align2::CENTER_CENTER,
                    tr("Click to place points, drag to draw"),
                    FontId::proportional(11.5),
                    visuals.weak_text_color(),
                );
            } else {
                draw_curve(&lane_painter, rect, &lane, &info, scale, ink);
            }
            let view = LaneView {
                key,
                rect,
                clip: rect.intersect(lanes),
                info: &info,
                scale,
                magnet: &magnet,
                origin: 0,
                overlay: false,
            };
            if self.auto_tool != AutoTool::Zoom {
                self.lane_editor(ui, doc, &view);
            }
            // At the left: the track and the curve it shows, to choose.
            let side = Rect::from_min_max(
                pos2(all.left(), rect.top()),
                pos2(lanes.left() - 1.0, rect.bottom()),
            );
            painter.rect_filled(side, 0.0, visuals.panel_fill);
            painter.rect_filled(
                Rect::from_min_max(
                    side.min + vec2(1.0, 4.0),
                    pos2(side.left() + 5.0, side.bottom() - 4.0),
                ),
                2.0,
                color,
            );
            let mut job = egui::text::LayoutJob::simple_singleline(
                track.name.clone(),
                crate::theme::strong(13.0),
                visuals.strong_text_color(),
            );
            job.wrap = egui::text::TextWrapping::truncate_at_width(side.width() - 26.0);
            let galley = ui.fonts_mut(|f| f.layout_job(job));
            painter.galley(
                pos2(side.left() + 14.0, side.top() + 12.0),
                galley,
                visuals.strong_text_color(),
            );
            painter.text(
                pos2(side.left() + 14.0, side.top() + 38.0),
                Align2::LEFT_TOP,
                tr("Automates"),
                FontId::proportional(10.0),
                visuals.weak_text_color(),
            );
            let chooser = Rect::from_min_size(
                pos2(side.left() + 14.0, side.top() + 54.0),
                vec2(side.width() - 26.0, 26.0),
            );
            // Its lower edge, dragged, makes every band taller or shorter.
            let edge = Rect::from_min_max(
                pos2(side.left(), side.bottom() - 4.0),
                pos2(side.right(), side.bottom() + 2.0),
            )
            .intersect(bands);
            let grip = ui.interact(
                edge,
                ui.scope_id().with(("band-edge", track.id.0)),
                Sense::drag(),
            );
            if grip.hovered() || grip.dragged() {
                ui.ctx().set_cursor_icon(CursorIcon::ResizeVertical);
                painter.hline(
                    side.x_range(),
                    side.bottom(),
                    Stroke::new(2.0, visuals.selection.bg_fill),
                );
            }
            if grip.dragged()
                && let Some(editor) = &mut self.auto_editor
            {
                editor.band = (height + grip.drag_delta().y).clamp(90.0, 900.0);
            }
            if chooser.bottom() < side.bottom() && bands.contains_rect(chooser) {
                self.lane_chooser(ui, doc, track, chooser);
            }
        }
        self.automation = false;
        self.zoom = timeline_zoom;
        // How far down the bands are, along the right edge.
        if overflow > 0.0 {
            let total = lanes.height() + overflow;
            let thumb = Rect::from_min_size(
                pos2(
                    lanes.right() - 5.0,
                    lanes.top() + lanes.height() * scroll_y / total,
                ),
                vec2(4.0, lanes.height() * lanes.height() / total),
            );
            painter.rect_filled(thumb, 2.0, visuals.weak_text_color().gamma_multiply(0.6));
        }
        let painter = ruler_painter;
        // The zoom tool: a span dragged across fills the view, a click zooms in (Alt: out).
        if self.auto_tool == AutoTool::Zoom {
            let response = ui.interact(lanes, ui.scope_id().with("zoom"), Sense::click_and_drag());
            if response.hovered() {
                ui.ctx().set_cursor_icon(if ui.input(|i| i.modifiers.alt) {
                    CursorIcon::ZoomOut
                } else {
                    CursorIcon::ZoomIn
                });
            }
            // The press is kept: the pointer has let go of it by the time the drag stops.
            if response.drag_started()
                && let Some(editor) = &mut self.auto_editor
            {
                editor.zoom_from = ui
                    .input(|i| i.pointer.press_origin())
                    .or(response.interact_pointer_pos())
                    .map(|p| p.x);
            }
            let from = self.auto_editor.as_ref().and_then(|e| e.zoom_from);
            if let (Some(a), Some(b)) = (from, response.interact_pointer_pos())
                && (response.dragged() || response.drag_stopped())
            {
                let span = Rect::from_x_y_ranges(a.min(b.x)..=a.max(b.x), lanes.y_range());
                let accent = visuals.selection.bg_fill;
                painter.rect_filled(span, 0.0, accent.gamma_multiply(0.18));
                painter.rect_stroke(span, 0.0, Stroke::new(1.0, accent), StrokeKind::Inside);
                if response.drag_stopped()
                    && let Some(editor) = &mut self.auto_editor
                {
                    editor.zoom_from = None;
                    if span.width() > 6.0 {
                        let (from, to) = (scale.sample(span.left()), scale.sample(span.right()));
                        editor.zoom = ((to - from) as f64 / f64::from(lanes.width()))
                            .clamp(MIN_ZOOM, MAX_ZOOM);
                        editor.scroll = from as f64;
                    }
                }
            }
            if (response.clicked() || response.secondary_clicked())
                && let Some(pos) = response.interact_pointer_pos()
                && let Some(editor) = &mut self.auto_editor
            {
                let out = ui.input(|i| i.modifiers.alt) || response.secondary_clicked();
                let at = editor.scroll + f64::from(pos.x - lanes.left()) * editor.zoom;
                editor.zoom = (editor.zoom * if out { 2.0 } else { 0.5 }).clamp(MIN_ZOOM, MAX_ZOOM);
                editor.scroll = (at - f64::from(pos.x - lanes.left()) * editor.zoom).max(0.0);
            }
        }
        // What the tool in hand does, over the top of the lanes, on as many lines as it takes.
        let help = match self.auto_tool {
            AutoTool::Pencil => tr(
                "Click: place a point · Drag: draw · Drag a point: move it (Shift: finely) · Alt+drag a segment: move it · Double-click a point: type its value",
            ),
            AutoTool::Select => tr(
                "Click a point: select it · Drag: select in a rectangle (Shift adds) · Drag the selection or a segment: move it · Alt+drag a segment: bend it · Delete: remove it",
            ),
            AutoTool::Erase => tr("Click a point: remove it · Drag: remove the points passed over"),
            AutoTool::Zoom => tr(
                "Drag across a span: fill the view with it · Click: zoom in · Alt+click or right-click: zoom out · Wheel: move in time",
            ),
        };
        let p = crate::theme::current(ui);
        let mut job = egui::text::LayoutJob::simple(
            help.to_owned(),
            FontId::proportional(11.5),
            p.text,
            (lanes.width() - 60.0).max(120.0),
        );
        job.halign = egui::Align::Center;
        let galley = ui.fonts_mut(|f| f.layout_job(job));
        let size = galley.rect.size();
        let pill = Rect::from_center_size(
            pos2(lanes.center().x, lanes.top() + 26.0 + size.y / 2.0),
            size + vec2(20.0, 8.0),
        );
        painter.rect_filled(
            pill,
            6.0,
            draw::mix(visuals.extreme_bg_color, p.accent, 0.22),
        );
        painter.rect_stroke(
            pill,
            6.0,
            Stroke::new(1.0, p.accent.gamma_multiply(0.5)),
            StrokeKind::Inside,
        );
        painter.galley(pos2(pill.center().x, pill.top() + 4.0), galley, p.text);
        // The playhead across the lanes.
        let x = scale.x(doc.player().position());
        if x >= lanes.left() && x <= lanes.right() {
            painter.vline(
                x,
                ruler.bottom()..=lanes.bottom(),
                Stroke::new(1.5, Color32::from_rgb(255, 78, 66)),
            );
        }
    }

    /// The editor's tools: pointer and pencil, the shapes of the selected points, and the view.
    fn editor_tools(&mut self, ui: &mut Ui, project: &Project, tracks: &[&Track]) {
        let p = crate::theme::current(ui);
        let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), TOOLS), Sense::hover());
        ui.scope_builder(egui::UiBuilder::new().max_rect(bar), |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.horizontal_centered(|ui| {
                for (tool, glyph, text) in [
                    (
                        AutoTool::Pencil,
                        Glyph::Pencil,
                        tr(
                            "Pencil: click places a point, drag draws; drag a point to move it, \
                            Alt+drag a segment to bend it",
                        ),
                    ),
                    (
                        AutoTool::Select,
                        Glyph::Smart,
                        tr(
                            "Selection: click a point or drag a rectangle (Shift adds); drag \
                            what is selected to move it",
                        ),
                    ),
                    (
                        AutoTool::Erase,
                        Glyph::Eraser,
                        tr("Eraser: click a point or drag over points to remove them"),
                    ),
                ] {
                    if button(ui, glyph, self.auto_tool == tool, text).clicked() {
                        self.auto_tool = tool;
                    }
                }
                separator(ui);
                let some = self.chosen.as_ref().is_some_and(|c| !c.times.is_empty());
                for (glyph, shape, text) in [
                    (
                        Glyph::Linear,
                        CurveShape::Linear,
                        tr("Selected points: straight lines"),
                    ),
                    (
                        Glyph::Smooth,
                        CurveShape::Smooth,
                        tr("Selected points: smooth curves"),
                    ),
                    (Glyph::Step, CurveShape::Step, tr("Selected points: steps")),
                ] {
                    if ui
                        .add_enabled_ui(some, |ui| button(ui, glyph, false, text))
                        .inner
                        .clicked()
                    {
                        self.lane_command = Some(LaneCommand::Shape(shape));
                    }
                }
                if ui
                    .add_enabled_ui(some, |ui| {
                        button(ui, Glyph::Delete, false, tr("Delete the selected points"))
                    })
                    .inner
                    .clicked()
                {
                    self.lane_command = Some(LaneCommand::Delete);
                }
                separator(ui);
                // The snap, the editor's own.
                let snap = self
                    .auto_editor
                    .as_ref()
                    .and_then(|e| e.snap)
                    .unwrap_or(false);
                if crate::theme::labeled_toggle(
                    ui,
                    crate::theme::Icon::Snap,
                    tr("Snap"),
                    snap,
                    p.accent,
                    tr("Points snap to the grid, the clips' edges and the markers, as in the timeline"),
                )
                .clicked()
                    && let Some(editor) = &mut self.auto_editor
                {
                    editor.snap = Some(!snap);
                }
                separator(ui);
                if button(
                    ui,
                    Glyph::ZoomIn,
                    self.auto_tool == AutoTool::Zoom,
                    tr("Zoom: drag across a span to fill the view with it, click to zoom in, Alt+click to zoom out"),
                )
                .clicked()
                {
                    self.auto_tool = AutoTool::Zoom;
                }
                if button(ui, Glyph::ZoomOut, false, tr("Zoom out")).clicked()
                    && let Some(editor) = &mut self.auto_editor
                {
                    // Around the middle of the view.
                    let half = f64::from((bar.width() - NAMES).max(100.0)) / 2.0;
                    let middle = editor.scroll + half * editor.zoom;
                    editor.zoom = (editor.zoom * 2.0).clamp(MIN_ZOOM, MAX_ZOOM);
                    editor.scroll = (middle - half * editor.zoom).max(0.0);
                }
                // The whole of the tracks' clips in view.
                if button(ui, Glyph::Fit, false, tr("Show the whole of these tracks")).clicked()
                    && let Some(editor) = &mut self.auto_editor
                {
                    let end = tracks
                        .iter()
                        .flat_map(|t| &t.clips)
                        .map(|c| c.end())
                        .max()
                        .unwrap_or_else(|| project.length())
                        .max(1);
                    let width = f64::from((bar.width() - NAMES - 24.0).max(100.0));
                    editor.zoom = (end as f64 / width).clamp(MIN_ZOOM, MAX_ZOOM);
                    editor.scroll = 0.0;
                }
                // The keys that change the tool, in the room left.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new(tr(
                            "P: pencil · S: selection · E: eraser · Z: zoom · Esc: close",
                        ))
                        .size(11.0)
                        .color(p.weak),
                    );
                });
            });
        });
    }
}
