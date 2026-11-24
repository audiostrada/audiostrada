//! The frame of an effect's window, drawn by `audiostrada_editors::frame` on an insert of the
//! project: the song row, the sidechain and MIDI learn are AudioStrada's own.

use std::collections::BTreeSet;

use audiostrada_core::automation::AutomationTarget;
use audiostrada_core::routing::{self, Channel};
use audiostrada_core::{Insert, InsertId};
pub use audiostrada_editors::frame::*;
use audiostrada_editors::frame::{self as shared, PresetMove};
use eframe::egui::{
    self, Align2, FontId, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, pos2, vec2,
};

use crate::document::{Document, PresetStep};
use crate::i18n::tr;
use crate::presets::Preset;
use crate::theme;
use crate::trf;

/// An insert of the project, as the frame sees it.
struct DocFrame<'a> {
    doc: &'a mut Document,
    channel: Channel,
    insert: &'a Insert,
}

impl FrameHost for DocFrame<'_> {
    fn effect(&self) -> &str {
        &self.insert.effect
    }

    fn salt(&self) -> u64 {
        self.insert.id.0
    }

    fn bypassed(&self) -> bool {
        self.insert.bypass
    }

    fn params(&self) -> std::collections::BTreeMap<String, f32> {
        self.insert.params.clone()
    }

    fn settings(&mut self) -> Settings {
        let state = self
            .doc
            .player_mut()
            .plugin(self.channel, self.insert.id)
            .and_then(|p| p.save_state());
        Settings {
            params: self.insert.params.clone(),
            state,
        }
    }

    fn put(&mut self, settings: &Settings, label: Option<&str>) {
        let (channel, insert) = (self.channel, self.insert.id);
        let encoded = settings
            .state
            .as_deref()
            .map(audiostrada_core::base64::encode);
        let change = |p: &mut audiostrada_core::Project| {
            let inserts = match channel {
                Channel::Master => Some(&mut p.master.inserts),
                Channel::Track(t) => p.track_mut(t).map(|t| &mut t.inserts),
                Channel::Clip(c) => audiostrada_core::edit::clip_mut(p, c).map(|c| &mut c.inserts),
            };
            if let Some(i) = inserts.and_then(|list| list.iter_mut().find(|i| i.id == insert)) {
                i.params = settings.params.clone();
                if encoded.is_some() {
                    i.state = encoded.clone();
                }
            }
        };
        match label {
            Some(label) => {
                self.doc.history_mut().edit(label, change);
            }
            None => self.doc.history_mut().modify_unrecorded(change),
        }
        if let (Some(state), Some(plugin)) = (
            &settings.state,
            self.doc.player_mut().plugin(channel, insert),
        ) {
            plugin.load_state(state);
        }
    }

    fn presets(&mut self) -> Vec<Preset> {
        crate::presets::list(crate::presets::root().as_deref(), &self.insert.effect)
    }

    fn save_preset(&mut self, preset: &Preset) -> Result<(), String> {
        let root = crate::presets::root().ok_or(tr("No configuration folder for presets"))?;
        crate::presets::save(&root, &self.insert.effect, preset)
    }

    fn delete_preset(&mut self, name: &str) -> Result<(), String> {
        let root = crate::presets::root().ok_or(tr("No configuration folder for presets"))?;
        crate::presets::delete(&root, &self.insert.effect, name)
    }

    fn favorites(&self) -> BTreeSet<String> {
        crate::favorites::with(|f| {
            f.presets
                .get(&self.insert.effect)
                .cloned()
                .unwrap_or_default()
        })
    }

    fn toggle_favorite(&mut self, name: &str) {
        crate::favorites::change(|f| f.toggle_preset(&self.insert.effect, name));
    }

    fn notify(&mut self, message: String) {
        self.doc.notify(message);
    }

    fn arrow_menu(&mut self, response: &Response, forward: bool) {
        // A button of a MIDI controller steps the presets as this arrow does.
        let target = AutomationTarget::Param {
            insert: self.insert.id,
            param: if forward {
                PresetStep::NEXT
            } else {
                PresetStep::PREVIOUS
            }
            .to_owned(),
        };
        crate::mixer::learn_menu(response, self.doc, self.channel, &target, (0.0, 1.0));
    }

    fn levels(&mut self, matching: bool) -> [f32; 7] {
        let (channel, id) = (self.channel, self.insert.id);
        let player = self.doc.player_mut();
        player.set_insert_watched(channel, id, true);
        player.set_insert_match(channel, id, matching);
        player
            .insert_levels(channel, id)
            .unwrap_or([-120.0, -120.0, -120.0, -120.0, -120.0, -120.0, 0.0])
    }

    fn gain_db(&self) -> f32 {
        self.insert.gain_db
    }

    fn set_gain(&mut self, gain_db: f32) {
        let (channel, id) = (self.channel, self.insert.id);
        self.doc.history_mut().edit_merged(
            tr("Insert gain"),
            &format!("insert-gain:{}", id.0),
            |p| routing::set_insert_gain(p, channel, id, gain_db),
        );
    }

    fn end_gain(&mut self) {
        self.doc.history_mut().end_gesture();
    }

    fn keep_gain(&mut self, gain_db: f32) {
        let (channel, id) = (self.channel, self.insert.id);
        self.doc.history_mut().edit(tr("Keep matched gain"), |p| {
            routing::set_insert_gain(p, channel, id, gain_db)
        });
    }

    fn sidechain(&mut self, ui: &mut Ui, at: Pos2) -> f32 {
        sidechain_button(ui, self.doc, self.channel, self.insert, at)
    }
}

/// The top bar of an effect's window; `keys` are those the window took for it.
pub fn top(
    ui: &mut Ui,
    keys: &[egui::Key],
    doc: &mut Document,
    channel: Channel,
    insert: &Insert,
    state: &mut FrameState,
    who: &Identity,
) -> TopActions {
    let mut host = DocFrame {
        doc,
        channel,
        insert,
    };
    shared::top(ui, keys, &mut host, state, who)
}

/// The bottom bar: the sidechain when the effect has one, levels in and out, the output gain
/// and Match.
pub fn bottom(
    ui: &mut Ui,
    doc: &mut Document,
    channel: Channel,
    insert: &Insert,
    matching: &mut bool,
    sidechain: bool,
) {
    let mut host = DocFrame {
        doc,
        channel,
        insert,
    };
    shared::bottom(ui, &mut host, matching, sidechain);
}

/// Loads the preset a MIDI controller asked for, the window open or not: the next or the
/// previous one in the menu's order, or the one at a place in it.
pub fn step_preset(
    doc: &mut Document,
    channel: Channel,
    insert: &Insert,
    state: &mut FrameState,
    step: PresetStep,
) {
    let step = match step {
        PresetStep::Next => PresetMove::Next,
        PresetStep::Previous => PresetMove::Previous,
        PresetStep::Number(k) => PresetMove::Number(k),
    };
    let mut host = DocFrame {
        doc,
        channel,
        insert,
    };
    shared::step_preset(&mut host, state, step);
}

/// The window closes: a preset only heard goes back to the settings before it.
pub fn close(state: &mut FrameState, doc: &mut Document, channel: Channel, insert: InsertId) {
    let Some(insert) = routing::find_insert(doc.project(), channel, insert).cloned() else {
        return;
    };
    let mut host = DocFrame {
        doc,
        channel,
        insert: &insert,
    };
    state.close(&mut host);
}

/// Height of the song row under an effect's editor.
const SONG: f32 = 52.0;

/// The song under an effect's editor: play and pause, solo of the track the effect is on, the
/// loop, and the song's bar over the outline of what the effect hears.
pub fn song_row(ui: &mut Ui, doc: &mut Document, channel: Channel, insert: InsertId) {
    egui::Panel::bottom(("effect-song", insert.0))
        .frame(egui::Frame::NONE)
        .exact_size(SONG)
        .show_separator_line(false)
        .show(ui, |ui| {
            let p = theme::current(ui);
            let full = ui.max_rect();
            ui.painter().rect_filled(full, 0.0, bar_fill(&p));
            ui.painter()
                .hline(full.x_range(), full.top(), Stroke::new(1.0, p.border));
            ui.allocate_rect(full, Sense::hover());
            let cy = crate::song_bar::middle(full);
            let playing = doc.player().engine().playing();
            let play = Rect::from_center_size(pos2(full.left() + 29.0, cy), vec2(30.0, 26.0));
            let (icon, hint) = if playing {
                (theme::Icon::Pause, tr("Pause"))
            } else {
                (theme::Icon::Play, tr("Play"))
            };
            let clicked = ui
                .put(play, |ui: &mut Ui| {
                    theme::icon_button(ui, icon, playing, theme::PLAY, hint, 1.0)
                })
                .clicked();
            if clicked {
                crate::send_action(ui.ctx(), crate::actions::Action::PlayPause);
            }
            let mut left = play.right() + 6.0;
            let track = match channel {
                Channel::Master => None,
                _ => channel.track(doc.project()),
            };
            if let Some(id) = track
                && let Some(t) = doc.project().track(id)
            {
                let (solo, name) = (t.solo, t.name.clone());
                let rect = Rect::from_min_size(pos2(left, cy - 13.0), vec2(26.0, 26.0));
                let response = ui.put(rect, |ui: &mut Ui| {
                    theme::letter_toggle(ui, vec2(26.0, 26.0), "S", solo, theme::SOLO)
                });
                if response.on_hover_text(tr("Solo")).clicked() {
                    doc.history_mut()
                        .edit(&trf!("Solo {name}", name = name), |p| {
                            audiostrada_core::groups::set_switch(
                                p,
                                id,
                                audiostrada_core::groups::Link::Solo,
                                !solo,
                            )
                        });
                }
                left = rect.right() + 6.0;
            }
            let looping = doc.project().looping().is_some();
            let loop_rect = Rect::from_min_size(pos2(left, cy - 13.0), vec2(30.0, 26.0));
            let action = crate::actions::Action::ToggleLoop;
            let toggled = ui
                .put(loop_rect, |ui: &mut Ui| {
                    theme::icon_button(
                        ui,
                        theme::Icon::Loop,
                        looping,
                        p.accent,
                        action.label(),
                        1.0,
                    )
                })
                .clicked();
            if toggled {
                crate::send_action(ui.ctx(), action);
            }
            left = loop_rect.right() + 6.0;
            let bar = Rect::from_min_max(pos2(left, full.top()), full.max);
            crate::song_bar::show(ui, bar, 8.0, doc, |doc, heights, per_column| {
                crate::song_bar::outline(doc, channel, heights, per_column)
            });
        });
}

/// Which track drives the detection of a compressor or gate; returns where the button ends.
fn sidechain_button(
    ui: &mut Ui,
    doc: &mut Document,
    channel: Channel,
    insert: &Insert,
    at: Pos2,
) -> f32 {
    let p = theme::current(ui);
    let current = insert.sidechain;
    let tracks = key_tracks(doc, channel);
    let source = current
        .and_then(|id| tracks.iter().find(|t| t.0 == id).map(|t| t.1.clone()))
        .unwrap_or_else(|| tr("Off").to_owned());
    let painter = ui.painter().clone();
    let label = tr("Sidechain");
    let font = FontId::proportional(12.0);
    let w1 = painter
        .layout_no_wrap(label.to_owned(), font.clone(), p.weak)
        .size()
        .x;
    let w2 = painter
        .layout_no_wrap(source.clone(), font.clone(), p.text)
        .size()
        .x
        .min(140.0);
    let rect = Rect::from_min_size(
        pos2(at.x, at.y - 14.0),
        vec2(10.0 + 13.0 + 7.0 + w1 + 5.0 + w2 + 8.0 + 10.0 + 10.0, 28.0),
    );
    let response = button(ui, rect, ui.scope_id().with("sidechain"), true);
    paint_face(ui, rect, radius(&p), &response, Face::Plain);
    let key = star_color(&p);
    if current.is_some() {
        painter.rect_stroke(rect, radius(&p), Stroke::new(1.0, key), StrokeKind::Inside);
    }
    let mut x = rect.left() + 10.0;
    paint_glyph(
        &painter,
        Rect::from_min_size(pos2(x, at.y - 6.5), vec2(13.0, 13.0)),
        Glyph::Key,
        key,
    );
    x += 20.0;
    painter.text(
        pos2(x, at.y),
        Align2::LEFT_CENTER,
        label,
        font.clone(),
        p.weak,
    );
    x += w1 + 5.0;
    painter
        .with_clip_rect(Rect::from_min_size(pos2(x, rect.top()), vec2(w2, 28.0)))
        .text(pos2(x, at.y), Align2::LEFT_CENTER, &source, font, p.text);
    paint_glyph(
        &painter,
        Rect::from_center_size(pos2(rect.right() - 14.0, at.y), vec2(10.0, 10.0)),
        Glyph::Down,
        p.weak,
    );
    let response = response.on_hover_text(tr(
        "Detect on another track's output, e.g. duck the bass with the kick",
    ));
    egui::Popup::menu(&response).show(|ui| sidechain_menu(ui, doc, channel, insert));
    rect.right()
}

/// The tracks that can key an insert of `channel`: those carrying audio, but its own.
pub(crate) fn key_tracks(
    doc: &Document,
    channel: Channel,
) -> Vec<(audiostrada_core::TrackId, String)> {
    let own = channel.track(doc.project());
    doc.project()
        .tracks
        .iter()
        .filter(|t| Some(t.id) != own && t.kind.carries_audio())
        .map(|t| (t.id, t.name.clone()))
        .collect()
}

/// The choices of an insert's sidechain, off or a track: in the bar's menu and the insert's
/// right-click menu.
pub(crate) fn sidechain_menu(ui: &mut Ui, doc: &mut Document, channel: Channel, insert: &Insert) {
    let current = insert.sidechain;
    let mut chosen = None;
    if ui
        .selectable_label(current.is_none(), tr("Off (own signal)"))
        .clicked()
    {
        chosen = Some(None);
        ui.close();
    }
    for (id, track) in key_tracks(doc, channel) {
        if ui.selectable_label(current == Some(id), track).clicked() {
            chosen = Some(Some(id));
            ui.close();
        }
    }
    if let Some(source) = chosen {
        let id = insert.id;
        doc.history_mut().edit(tr("Sidechain"), |p| {
            let inserts = match channel {
                Channel::Master => Some(&mut p.master.inserts),
                Channel::Track(t) => p.track_mut(t).map(|t| &mut t.inserts),
                Channel::Clip(c) => audiostrada_core::edit::clip_mut(p, c).map(|c| &mut c.inserts),
            };
            if let Some(i) = inserts.and_then(|l| l.iter_mut().find(|i| i.id == id)) {
                i.sidechain = source;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::AppDirs;
    use audiostrada_editors::presets::menu_order;

    #[test]
    fn midi_steps_the_presets_in_the_menus_order_without_the_window() {
        let mut doc = Document::new(AppDirs::default());
        let mut id = None;
        doc.history_mut().edit("Keys", |p| {
            let t = p.add_track("Keys");
            id = routing::add_insert(p, Channel::Track(t), "synth", Default::default());
        });
        let channel = Channel::Track(doc.project().tracks[0].id);
        let insert = |doc: &Document| {
            routing::find_insert(doc.project(), channel, id.unwrap())
                .unwrap()
                .clone()
        };
        let list = crate::presets::list(None, "synth");
        let order = menu_order(&list);
        let mut state = FrameState::default();
        let mut step = |doc: &mut Document, step: PresetStep| {
            let i = insert(doc);
            step_preset(doc, channel, &i, &mut state, step);
            state.preset().unwrap().to_owned()
        };
        assert_eq!(step(&mut doc, PresetStep::Next), list[order[0]].name);
        assert_eq!(step(&mut doc, PresetStep::Next), list[order[1]].name);
        assert_eq!(step(&mut doc, PresetStep::Previous), list[order[0]].name);
        assert_eq!(
            step(&mut doc, PresetStep::Previous),
            list[order[order.len() - 1]].name
        );
        assert_eq!(step(&mut doc, PresetStep::Number(3)), list[order[3]].name);
        assert_eq!(insert(&doc).params, list[order[3]].params);
    }
}
