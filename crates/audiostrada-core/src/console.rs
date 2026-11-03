//! Console color: the character of an analog console, on every channel (its input stage) and
//! on the buses and the master (their summing). Set for the project, changed per track.

use serde::{Deserialize, Serialize};

use crate::{Project, TrackId};

/// The consoles, by evocative names; `Clean` colors nothing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ConsoleModel {
    #[default]
    Clean,
    /// Transformers: round lows, silky highs.
    Iron,
    /// Tight and precise, a touch of air.
    Imperial,
    /// Discrete amplifiers: punch and attack.
    Discrete,
    /// Smooth and warm, very low distortion.
    Velvet,
}

impl ConsoleModel {
    pub const ALL: [Self; 5] = [
        Self::Clean,
        Self::Iron,
        Self::Imperial,
        Self::Discrete,
        Self::Velvet,
    ];

    /// Its place in [`Self::ALL`], as the DSP numbers them.
    pub fn index(self) -> usize {
        Self::ALL.iter().position(|&m| m == self).unwrap_or(0)
    }

    /// Its name as shown; the variants keep their first names in project files.
    pub fn name(self) -> &'static str {
        match self {
            Self::Clean => "Clean",
            Self::Iron => "British Warm",
            Self::Imperial => "British Bright",
            Self::Discrete => "American Punch",
            Self::Velvet => "Nashville",
        }
    }
}

/// A console and how much of it is heard.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Console {
    pub model: ConsoleModel,
    /// 0 to 100 %.
    pub amount: f32,
}

impl Default for Console {
    fn default() -> Self {
        Self {
            model: ConsoleModel::Clean,
            amount: 50.0,
        }
    }
}

impl Console {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// Heard at all.
    pub fn colors(&self) -> bool {
        self.model != ConsoleModel::Clean && self.amount > 0.0
    }
}

/// The console a track plays through: its own, else the project's.
pub fn of_track(project: &Project, track: TrackId) -> Console {
    project
        .track(track)
        .and_then(|t| t.console)
        .unwrap_or(project.console)
}
