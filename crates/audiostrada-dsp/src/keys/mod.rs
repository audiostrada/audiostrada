//! Keyboards modeled from their physics: struck tines, reeds and strings, tone wheels, their
//! pickups, amplifiers and speakers. No samples; the parts they share live here.

pub(crate) mod common;
pub(crate) mod modal;
pub(crate) mod rotary;
pub(crate) mod voice;

pub mod bass808;
pub mod ep80;
pub mod fmkeys;
pub mod grain;
pub mod mallets;
pub mod reed;
pub mod tines;
pub mod tonewheel;

/// Whether built-in effect `id` is one of the electric pianos.
pub fn is_piano(id: &str) -> bool {
    matches!(
        id,
        "tines" | "reed" | "ep80" | "tonewheel" | "fmkeys" | "mallets"
    )
}

/// Whether built-in effect `id` is one of the instruments drawn by the keyboards' editor.
pub fn has_face(id: &str) -> bool {
    is_piano(id) || matches!(id, "bass808" | "grain")
}
