//! Optional modules built into some versions of AudioStrada. A module draws its own windows,
//! adds a menu, and keeps its data in `Project::extensions`.

use eframe::egui;

use audiostrada_engine::tap::InputTap;

use crate::document::Document;

pub trait Extension {
    /// Its key in `Project::extensions`.
    fn name(&self) -> &'static str;

    /// Every frame, after the panels: its own windows go here.
    fn tick(&mut self, _ctx: &egui::Context, _doc: &mut Document) {}

    /// The title of its menu in the menu bar, if it has one.
    fn menu_title(&self) -> Option<String> {
        None
    }

    fn menu(&mut self, _ui: &mut egui::Ui, _doc: &mut Document) {}
}

impl Document {
    /// The data a module keeps in the project.
    pub fn extension_data(&self, name: &str) -> Option<&serde_json::Value> {
        self.project().extensions.get(name)
    }

    /// Hardware input channels of the armed audio tracks, in order.
    pub fn armed_inputs(&self) -> Vec<usize> {
        let mut channels: Vec<usize> = self
            .project()
            .tracks
            .iter()
            .filter(|t| t.armed && !self.is_instrument_track(t.id))
            .flat_map(|t| {
                let first = usize::from(t.input);
                [Some(first), t.stereo.then_some(first + 1)]
            })
            .flatten()
            .collect();
        channels.sort_unstable();
        channels.dedup();
        channels
    }

    /// Copies the inputs of the armed tracks for a module to read, recording or not.
    pub fn open_armed_input_tap(&mut self, seconds: f32) -> Result<InputTap, String> {
        let channels = self.armed_inputs();
        self.player_mut()
            .engine_mut()
            .open_input_tap(channels, seconds)
    }

    /// Replaces a module's data in the project, as an undo step named `label`.
    pub fn set_extension_data(&mut self, name: &str, label: &str, value: serde_json::Value) {
        self.history_mut().edit(label, |p| {
            p.extensions.insert(name.to_string(), value);
        });
    }
}
