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
    /// The selected track's strip and the master's, in a column.
    Strip,
}

impl Pane {
    pub const ALL: [Pane; 8] = [
        Pane::Timeline,
        Pane::Mixer,
        Pane::PianoRoll,
        Pane::Rack,
        Pane::Strip,
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
            Pane::Strip => tr("Track strip"),
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
            Action::ShowStrip => Pane::Strip,
            _ => return None,
        })
    }
}

pub fn default_tree() -> Tree<Pane> {
    start_tree(Start::Classic)
}

/// The layouts offered at the first start, for one screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Start {
    /// The timeline over the mixer.
    Classic,
    /// The selected track's strip in a column left of the timeline; the mixer a key away.
    Strip,
    /// The timeline alone; every other panel a key away.
    Focus,
}

impl Start {
    pub const ALL: [Start; 3] = [Start::Classic, Start::Strip, Start::Focus];
}

pub fn start_tree(start: Start) -> Tree<Pane> {
    let mut tiles = egui_tiles::Tiles::default();
    let browser = tiles.insert_pane(Pane::Browser);
    let timeline = tiles.insert_pane(Pane::Timeline);
    let mixer = tiles.insert_pane(Pane::Mixer);
    let inspector = tiles.insert_pane(Pane::Inspector);
    let markers = tiles.insert_pane(Pane::Markers);
    let piano = tiles.insert_pane(Pane::PianoRoll);
    let rack = tiles.insert_pane(Pane::Rack);
    let strip = tiles.insert_pane(Pane::Strip);
    let bottom = tiles.insert_tab_tile(vec![mixer, piano, rack]);
    let center = tiles.insert_vertical_tile(vec![timeline, bottom]);
    let (root, hidden) = match start {
        Start::Classic | Start::Focus => {
            let left = tiles.insert_tab_tile(vec![strip]);
            let right = tiles.insert_tab_tile(vec![inspector, markers]);
            let root = tiles.insert_horizontal_tile(vec![browser, left, center, right]);
            set_shares(
                &mut tiles,
                root,
                &[(browser, 1.0), (left, 1.3), (center, 4.5), (right, 1.2)],
            );
            // Side panels are one View menu click away.
            let mut hidden = vec![browser, inspector, markers, strip];
            if start == Start::Focus {
                hidden.extend([mixer, piano, rack]);
            }
            (root, hidden)
        }
        Start::Strip => {
            let left = tiles.insert_tab_tile(vec![strip]);
            let right = tiles.insert_tab_tile(vec![browser, inspector, markers]);
            let root = tiles.insert_horizontal_tile(vec![left, center, right]);
            set_shares(
                &mut tiles,
                root,
                &[(left, 1.3), (center, 4.5), (right, 1.2)],
            );
            (root, vec![browser, inspector, markers, mixer, piano, rack])
        }
    };
    for id in hidden {
        tiles.set_visible(id, false);
    }
    set_shares(&mut tiles, center, &[(timeline, 0.75), (bottom, 1.0)]);
    Tree::new(egui::Id::unique("audiostrada-layout"), root, tiles)
}

/// Moves `strip` into a column of its own, at the left of the one holding the timeline.
fn strip_column(tiles: &mut egui_tiles::Tiles<Pane>, strip: TileId) -> bool {
    let Some(mut column) = tiles.find_pane(&Pane::Timeline) else {
        return false;
    };
    let row = loop {
        let Some(parent) = tiles.parent_of(column) else {
            return false;
        };
        if let Some(Tile::Container(Container::Linear(linear))) = tiles.get(parent)
            && linear.dir == egui_tiles::LinearDir::Horizontal
        {
            break parent;
        }
        column = parent;
    };
    if let Some(Tile::Container(old)) = tiles.parent_of(strip).and_then(|p| tiles.get_mut(p)) {
        old.remove_child(strip);
    }
    let own = tiles.insert_tab_tile(vec![strip]);
    if let Some(Tile::Container(Container::Linear(linear))) = tiles.get_mut(row) {
        let at = linear
            .children
            .iter()
            .position(|&c| c == column)
            .unwrap_or(0);
        linear.children.insert(at, own);
        linear.shares.set_share(own, 1.3);
    }
    true
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
    // Saved before the track strip, or with it among the inspector's tabs: a hidden column of
    // its own at the left of the timeline.
    if let Some(tree) = &mut tree {
        let tiles = &mut tree.tiles;
        let strip = tiles.find_pane(&Pane::Strip);
        let beside_inspector = strip.is_some_and(|s| {
            !tiles.is_visible(s)
                && tiles.parent_of(s).is_some_and(|p| {
                    tiles
                        .find_pane(&Pane::Inspector)
                        .and_then(|i| tiles.parent_of(i))
                        == Some(p)
                })
        });
        if strip.is_none() {
            let strip = tiles.insert_pane(Pane::Strip);
            tiles.set_visible(strip, false);
            strip_column(tiles, strip);
        } else if let Some(strip) = strip
            && beside_inspector
        {
            strip_column(tiles, strip);
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

/// Space between panels.
const GAP: f32 = 3.0;

/// Gives the column holding `pane` `width` points in its row, as laid out last frame; false
/// while the row was not laid out yet.
pub fn fit_width(tree: &mut Tree<Pane>, pane: Pane, width: f32) -> bool {
    let tiles = &mut tree.tiles;
    let Some(column) = tiles.find_pane(&pane).and_then(|p| tiles.parent_of(p)) else {
        return false;
    };
    let Some(row) = tiles.parent_of(column) else {
        return false;
    };
    let visible: Vec<TileId> = match tiles.get(row) {
        Some(Tile::Container(Container::Linear(linear)))
            if linear.dir == egui_tiles::LinearDir::Horizontal =>
        {
            linear
                .children
                .iter()
                .copied()
                .filter(|&c| tiles.is_visible_in_layout(c))
                .collect()
        }
        _ => return false,
    };
    let Some(row_rect) = tiles.rect(row) else {
        return false;
    };
    let room = row_rect.width() - GAP * (visible.len() as f32 - 1.0);
    if visible.len() < 2 || room <= width || !visible.contains(&column) {
        return false;
    }
    if let Some(Tile::Container(Container::Linear(linear))) = tiles.get_mut(row) {
        let others: f32 = visible
            .iter()
            .filter(|&&c| c != column)
            .map(|&c| linear.shares[c])
            .sum();
        linear
            .shares
            .set_share(column, others * width / (room - width));
    }
    true
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
        GAP
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
    fn every_starting_layout_holds_every_panel_and_shows_its_own() {
        for start in Start::ALL {
            let tree = start_tree(start);
            assert!(
                Pane::ALL.iter().all(|p| tree.tiles.find_pane(p).is_some()),
                "{start:?}"
            );
            assert!(is_visible(&tree, Pane::Timeline), "{start:?}");
            let shown = |pane| is_visible(&tree, pane);
            match start {
                Start::Classic => assert!(shown(Pane::Mixer) && !shown(Pane::Strip)),
                Start::Strip => {
                    assert!(shown(Pane::Strip) && !shown(Pane::Mixer) && !shown(Pane::Inspector))
                }
                Start::Focus => assert!(!shown(Pane::Mixer) && !shown(Pane::Strip)),
            }
        }
        // Saved and read back, each stays as it was.
        let tree = start_tree(Start::Strip);
        let again = load(save(&tree).as_deref());
        assert!(is_visible(&again, Pane::Strip) && !is_visible(&again, Pane::Mixer));
    }

    #[test]
    fn an_older_layout_gets_the_track_strip_in_a_column_of_its_own() {
        let older = |with_strip: bool| {
            let mut tiles = egui_tiles::Tiles::default();
            let timeline = tiles.insert_pane(Pane::Timeline);
            let mut right = vec![
                tiles.insert_pane(Pane::Inspector),
                tiles.insert_pane(Pane::Markers),
            ];
            if with_strip {
                let strip = tiles.insert_pane(Pane::Strip);
                tiles.set_visible(strip, false);
                right.push(strip);
            }
            let bottom = [Pane::Mixer, Pane::PianoRoll, Pane::Rack].map(|p| tiles.insert_pane(p));
            let bottom = tiles.insert_tab_tile(bottom.to_vec());
            let center = tiles.insert_vertical_tile(vec![timeline, bottom]);
            let browser = tiles.insert_pane(Pane::Browser);
            let right = tiles.insert_tab_tile(right);
            let root = tiles.insert_horizontal_tile(vec![browser, center, right]);
            save(&Tree::new(egui::Id::unique("older"), root, tiles))
        };
        for with_strip in [false, true] {
            let tree = load(older(with_strip).as_deref());
            let tiles = &tree.tiles;
            let strip = tiles.find_pane(&Pane::Strip).unwrap();
            let column = tiles.parent_of(strip).unwrap();
            assert_eq!(tiles.get_container(column).unwrap().num_children(), 1);
            let Some(Tile::Container(Container::Linear(row))) = tiles.get(tree.root.unwrap())
            else {
                panic!("no row");
            };
            assert_eq!(
                row.children[1], column,
                "between the browser and the timeline"
            );
            assert!(!is_visible(&tree, Pane::Strip) && is_visible(&tree, Pane::Inspector));
        }
    }

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
