//! Dockable panels: timeline, mixer, browser, inspector and markers, rearranged by dragging
//! their tabs. The layout is saved in the preferences.

use eframe::egui::{self, WidgetText};
use egui_tiles::{Behavior, Container, SimplificationOptions, Tile, TileId, Tree, UiResponse};
use serde::{Deserialize, Serialize};

use crate::actions::Action;
use crate::i18n::tr;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Pane {
    Timeline,
    Mixer,
    Browser,
    Inspector,
    Markers,
    PianoRoll,
    Rack,
    /// The armed tracks' inputs, for the recording mode.
    Inputs,
    Takes,
}

impl Pane {
    pub const ALL: [Pane; 9] = [
        Pane::Timeline,
        Pane::Mixer,
        Pane::PianoRoll,
        Pane::Rack,
        Pane::Browser,
        Pane::Inspector,
        Pane::Markers,
        Pane::Inputs,
        Pane::Takes,
    ];

    /// The name shown on its tab and window, in the interface's language.
    pub fn title(self) -> &'static str {
        match self {
            Pane::Timeline => tr("Timeline"),
            Pane::Mixer => tr("Mixer"),
            Pane::Browser => tr("Browser"),
            Pane::Inspector => tr("Inspector"),
            Pane::Markers => tr("Markers"),
            Pane::PianoRoll => tr("Piano roll"),
            Pane::Rack => tr("Rack"),
            Pane::Inputs => tr("Armed inputs"),
            Pane::Takes => tr("Takes"),
        }
    }

    pub fn from_action(action: Action) -> Option<Self> {
        Some(match action {
            Action::ShowTimeline => Pane::Timeline,
            Action::ShowMixer => Pane::Mixer,
            Action::ShowBrowser => Pane::Browser,
            Action::ShowInspector => Pane::Inspector,
            Action::ShowMarkers => Pane::Markers,
            Action::ShowPianoRoll => Pane::PianoRoll,
            Action::ShowRack => Pane::Rack,
            Action::ShowInputs => Pane::Inputs,
            Action::ShowTakes => Pane::Takes,
            _ => return None,
        })
    }
}

pub fn default_tree() -> Tree<Pane> {
    let mut tiles = egui_tiles::Tiles::default();
    let browser = tiles.insert_pane(Pane::Browser);
    let timeline = tiles.insert_pane(Pane::Timeline);
    let mixer = tiles.insert_pane(Pane::Mixer);
    let inspector = tiles.insert_pane(Pane::Inspector);
    let markers = tiles.insert_pane(Pane::Markers);
    let piano = tiles.insert_pane(Pane::PianoRoll);
    let rack = tiles.insert_pane(Pane::Rack);
    let inputs = tiles.insert_pane(Pane::Inputs);
    let takes = tiles.insert_pane(Pane::Takes);
    let bottom = tiles.insert_tab_tile(vec![mixer, piano, rack]);
    let center = tiles.insert_vertical_tile(vec![timeline, bottom]);
    let left = tiles.insert_tab_tile(vec![browser, inputs]);
    let right = tiles.insert_tab_tile(vec![inspector, markers, takes]);
    let root = tiles.insert_horizontal_tile(vec![left, center, right]);
    // Side panels are one View menu click away.
    for id in [browser, inspector, markers, inputs, takes] {
        tiles.set_visible(id, false);
    }
    set_shares(&mut tiles, center, &[(timeline, 1.15), (bottom, 1.0)]);
    set_shares(
        &mut tiles,
        root,
        &[(left, 1.0), (center, 4.5), (right, 1.2)],
    );
    Tree::new(egui::Id::unique("audiostrada-layout"), root, tiles)
}

/// The recording mode's layout: the armed inputs, the timeline and the takes.
pub fn recording_tree() -> Tree<Pane> {
    let mut tree = default_tree();
    for pane in Pane::ALL {
        set_visible(&mut tree, pane, false);
    }
    for pane in [Pane::Inputs, Pane::Timeline, Pane::Takes] {
        set_visible(&mut tree, pane, true);
        activate(&mut tree, pane);
    }
    let root = tree.root();
    let sides: Vec<TileId> = root
        .and_then(|r| match tree.tiles.get(r) {
            Some(Tile::Container(c)) => Some(c.children().copied().collect()),
            _ => None,
        })
        .unwrap_or_default();
    if let (Some(root), [left, center, right]) = (root, sides.as_slice()) {
        set_shares(
            &mut tree.tiles,
            root,
            &[(*left, 1.7), (*center, 4.5), (*right, 1.5)],
        );
    }
    tree
}

/// Restores the recording mode's saved layout, falling back to its default one.
pub fn load_recording(saved: Option<&str>) -> Tree<Pane> {
    saved
        .and_then(|v| serde_json::from_str::<Tree<Pane>>(v).ok())
        .filter(|tree| Pane::ALL.iter().all(|p| tree.tiles.find_pane(p).is_some()))
        .unwrap_or_else(recording_tree)
}

fn set_shares(tiles: &mut egui_tiles::Tiles<Pane>, container: TileId, shares: &[(TileId, f32)]) {
    if let Some(Tile::Container(Container::Linear(linear))) = tiles.get_mut(container) {
        for &(id, share) in shares {
            linear.shares.set_share(id, share);
        }
    }
}

/// Bumped when the default layout changes, so older saved layouts are replaced once.
pub const VERSION: u32 = 3;

/// Restores a saved layout, falling back to the default one.
pub fn load(saved: Option<&str>) -> Tree<Pane> {
    let mut tree: Option<Tree<Pane>> = saved.and_then(|v| serde_json::from_str(v).ok());
    // Saved before the rack: it joins the mixer's tabs.
    if let Some(tree) = &mut tree
        && tree.tiles.find_pane(&Pane::Rack).is_none()
        && let Some(mixer) = tree.tiles.find_pane(&Pane::Mixer)
        && let Some(parent) = tree.tiles.parent_of(mixer)
    {
        let rack = tree.tiles.insert_pane(Pane::Rack);
        if let Some(egui_tiles::Tile::Container(container)) = tree.tiles.get_mut(parent) {
            container.add_child(rack);
        }
    }
    // Saved before the recording mode's panels: they join the side panels, hidden.
    if let Some(tree) = &mut tree
        && let Some(root) = tree.root()
    {
        for pane in [Pane::Inputs, Pane::Takes] {
            if tree.tiles.find_pane(&pane).is_none() {
                let id = tree.tiles.insert_pane(pane);
                tree.tiles.set_visible(id, false);
                if let Some(egui_tiles::Tile::Container(container)) = tree.tiles.get_mut(root) {
                    container.add_child(id);
                }
            }
        }
    }
    match tree {
        Some(tree) if Pane::ALL.iter().all(|p| tree.tiles.find_pane(p).is_some()) => tree,
        _ => default_tree(),
    }
}

pub fn save(tree: &Tree<Pane>) -> Option<String> {
    serde_json::to_string(tree).ok()
}

pub fn is_visible(tree: &Tree<Pane>, pane: Pane) -> bool {
    tree.tiles
        .find_pane(&pane)
        .is_some_and(|id| tree.tiles.is_visible(id))
}

pub fn set_visible(tree: &mut Tree<Pane>, pane: Pane, visible: bool) {
    if let Some(id) = tree.tiles.find_pane(&pane) {
        tree.tiles.set_visible(id, visible);
    }
}

/// Brings the pane's tab to the front of its tab group.
pub fn activate(tree: &mut Tree<Pane>, pane: Pane) {
    tree.make_active(|_, tile| matches!(tile, Tile::Pane(p) if *p == pane));
}

pub fn toggle(tree: &mut Tree<Pane>, pane: Pane) {
    if let Some(id) = tree.tiles.find_pane(&pane) {
        tree.tiles.toggle_visibility(id);
    }
}

/// Draws one panel's content.
pub trait PaneContent {
    fn pane_ui(&mut self, ui: &mut egui::Ui, pane: Pane);
}

pub struct Workspace<'a> {
    pub content: &'a mut dyn PaneContent,
    /// Panel the user asked to move into its own window.
    pub detach: Option<Pane>,
}

impl Behavior<Pane> for Workspace<'_> {
    fn tab_title_for_pane(&mut self, pane: &Pane) -> WidgetText {
        pane.title().into()
    }

    fn pane_ui(&mut self, ui: &mut egui::Ui, _tile_id: TileId, pane: &mut Pane) -> UiResponse {
        egui::Frame::new().inner_margin(6.0).show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            self.content.pane_ui(ui, *pane);
        });
        UiResponse::None
    }

    fn on_tab_button(
        &mut self,
        tiles: &mut egui_tiles::Tiles<Pane>,
        tile_id: TileId,
        response: egui::Response,
    ) -> egui::Response {
        if let Some(Tile::Pane(pane)) = tiles.get(tile_id) {
            let pane = *pane;
            response.context_menu(|ui| {
                if ui.button(tr("Open in its own window")).clicked() {
                    self.detach = Some(pane);
                    ui.close();
                }
            });
        }
        response
    }

    fn simplification_options(&self) -> SimplificationOptions {
        SimplificationOptions {
            all_panes_must_have_tabs: true,
            ..SimplificationOptions::default()
        }
    }

    fn tab_bar_height(&self, _style: &egui::Style) -> f32 {
        28.0
    }

    fn gap_width(&self, _style: &egui::Style) -> f32 {
        3.0
    }

    fn tab_title_spacing(&self, _visuals: &egui::Visuals) -> f32 {
        12.0
    }

    fn tab_text_color(
        &self,
        visuals: &egui::Visuals,
        _tiles: &egui_tiles::Tiles<Pane>,
        _tile_id: TileId,
        state: &egui_tiles::TabState,
    ) -> egui::Color32 {
        if state.active {
            visuals.strong_text_color()
        } else {
            visuals.weak_text_color()
        }
    }

    // The tab shown is outlined in the accent.
    fn tab_outline_stroke(
        &self,
        visuals: &egui::Visuals,
        _tiles: &egui_tiles::Tiles<Pane>,
        _tile_id: TileId,
        state: &egui_tiles::TabState,
    ) -> egui::Stroke {
        if state.active {
            egui::Stroke::new(1.0, visuals.selection.bg_fill.gamma_multiply(0.7))
        } else {
            egui::Stroke::NONE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_round_trips_and_toggles() {
        let mut tree = default_tree();
        assert!(!is_visible(&tree, Pane::Browser) && !is_visible(&tree, Pane::Inspector));
        assert!(is_visible(&tree, Pane::Timeline) && is_visible(&tree, Pane::Mixer));
        toggle(&mut tree, Pane::Browser);
        assert!(is_visible(&tree, Pane::Browser));
        toggle(&mut tree, Pane::Browser);
        assert!(!is_visible(&tree, Pane::Browser));
        let restored = load(save(&tree).as_deref());
        assert!(!is_visible(&restored, Pane::Browser));
        assert!(is_visible(&restored, Pane::Timeline));
        assert!(
            Pane::ALL
                .iter()
                .all(|p| load(None).tiles.find_pane(p).is_some())
        );
        assert!(is_visible(
            &load(Some(r#"{"not": "a layout"}"#)),
            Pane::Timeline
        ));
    }
}
