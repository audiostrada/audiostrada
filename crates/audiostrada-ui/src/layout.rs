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
}

impl Pane {
    pub const ALL: [Pane; 7] = [
        Pane::Timeline,
        Pane::Mixer,
        Pane::PianoRoll,
        Pane::Rack,
        Pane::Browser,
        Pane::Inspector,
        Pane::Markers,
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
    let bottom = tiles.insert_tab_tile(vec![mixer, piano, rack]);
    let center = tiles.insert_vertical_tile(vec![timeline, bottom]);
    let right = tiles.insert_tab_tile(vec![inspector, markers]);
    let root = tiles.insert_horizontal_tile(vec![browser, center, right]);
    // Side panels are one View menu click away.
    for id in [browser, inspector, markers] {
        tiles.set_visible(id, false);
    }
    set_shares(&mut tiles, center, &[(timeline, 0.75), (bottom, 1.0)]);
    set_shares(
        &mut tiles,
        root,
        &[(browser, 1.0), (center, 4.5), (right, 1.2)],
    );
    Tree::new(egui::Id::unique("audiostrada-layout"), root, tiles)
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
    let mut tree: Option<Tree<Pane>> = saved.and_then(|v| {
        serde_json::from_str(v)
            .ok()
            .or_else(|| serde_json::from_str(&without_unknown_panes(v)?).ok())
    });
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
    match tree {
        Some(tree) if Pane::ALL.iter().all(|p| tree.tiles.find_pane(p).is_some()) => tree,
        _ => default_tree(),
    }
}

/// A saved layout without the panels of older versions that this one no longer has.
fn without_unknown_panes(saved: &str) -> Option<String> {
    use serde_json::Value;
    let mut value: Value = serde_json::from_str(saved).ok()?;
    let tiles = value.get_mut("tiles")?;
    let unknown: Vec<String> = tiles
        .get("tiles")?
        .as_object()?
        .iter()
        .filter(|(_, tile)| {
            tile.get("Pane")
                .is_some_and(|p| serde_json::from_value::<Pane>(p.clone()).is_err())
        })
        .map(|(id, _)| id.clone())
        .collect();
    let gone = |v: &Value| v.as_u64().is_some_and(|n| unknown.contains(&n.to_string()));
    if let Some(invisible) = tiles.get_mut("invisible").and_then(Value::as_array_mut) {
        invisible.retain(|v| !gone(v));
    }
    let map = tiles.get_mut("tiles")?.as_object_mut()?;
    map.retain(|id, _| !unknown.contains(id));
    for tile in map.values_mut() {
        let Some(container) = tile
            .get_mut("Container")
            .and_then(Value::as_object_mut)
            .and_then(|c| c.values_mut().next())
        else {
            continue;
        };
        if let Some(children) = container.get_mut("children").and_then(Value::as_array_mut) {
            children.retain(|v| !gone(v));
        }
        if let Some(shares) = container
            .pointer_mut("/shares/shares")
            .and_then(Value::as_object_mut)
        {
            shares.retain(|id, _| !unknown.contains(id));
        }
        if container.get("active").is_some_and(gone) {
            let first = container
                .pointer("/children/0")
                .cloned()
                .unwrap_or(Value::Null);
            container["active"] = first;
        }
    }
    serde_json::to_string(&value).ok()
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
    fn a_layout_with_panels_since_removed_keeps_the_others() {
        // Saved with the recording mode's Inputs (8) and Takes (9) panels, Takes in front.
        let saved = r#"{"id":1,"root":14,"tiles":{"next_tile_id":16,"tiles":{"2":{"Pane":"Timeline"},"11":{"Container":{"Linear":{"children":[15,10],"dir":"Vertical","shares":{"shares":{"10":1.0,"15":1.15}}}}},"9":{"Pane":"Takes"},"8":{"Pane":"Inputs"},"15":{"Container":{"Tabs":{"children":[2],"active":2}}},"6":{"Pane":"PianoRoll"},"4":{"Pane":"Inspector"},"14":{"Container":{"Linear":{"children":[12,11,13],"dir":"Horizontal","shares":{"shares":{"13":1.5,"11":4.5,"12":1.7}}}}},"5":{"Pane":"Markers"},"13":{"Container":{"Tabs":{"children":[4,5,9],"active":9}}},"7":{"Pane":"Rack"},"10":{"Container":{"Tabs":{"children":[3,6,7],"active":3}}},"12":{"Container":{"Tabs":{"children":[1,8],"active":8}}},"3":{"Pane":"Mixer"},"1":{"Pane":"Browser"}},"invisible":[7,3,1,6,4,5,8]},"height":null,"width":null}"#;
        let tree = load(Some(saved));
        assert!(Pane::ALL.iter().all(|p| tree.tiles.find_pane(p).is_some()));
        assert!(is_visible(&tree, Pane::Timeline), "not the default layout");
        assert!(!is_visible(&tree, Pane::Mixer));
    }

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
