//! AudioStrada's look (theme, fonts, textures, widgets), its translations, and the editors of
//! its built-in effects, which AudioStrada and the AudioStrada plugins both draw.

pub mod controls;
pub mod delay_editor;
pub mod drive_editor;
pub mod effects;
pub mod eq_editor;
pub mod frame;
pub mod gear;
pub mod i18n;
pub mod mb_editor;
pub mod mod_editor;
pub mod pitch_editor;
pub mod presets;
pub mod tape_deck;
pub mod tape_editor;
pub mod theme;
pub mod verb_editor;
pub mod widgets;

/// Repaint rate of a window that moves on its own.
pub const REFRESH: std::time::Duration = std::time::Duration::from_millis(33);

/// Size of the editor window of effect `id`, in points, its bars included.
pub fn window_size(id: &str) -> [f32; 2] {
    match id {
        "eq" => [1040.0, 800.0],
        "amp_rig" => [1000.0, 960.0],
        "channel" => [1060.0, 720.0],
        "synth" => [1180.0, 720.0],
        "drums" => [1000.0, 720.0],
        "trigger" => [960.0, 760.0],
        "tape" => [980.0, 900.0],
        "mb_comp" => [1000.0, 860.0],
        "modulator" => [940.0, 640.0],
        "pitch" => [940.0, 720.0],
        "doubler" => [900.0, 600.0],
        "compressor" => [980.0, 720.0],
        "gate" => [980.0, 700.0],
        "deesser" => [980.0, 680.0],
        "transient" => [900.0, 600.0],
        "limiter" => [900.0, 650.0],
        "delay" => [980.0, 620.0],
        "filter" => [1000.0, 600.0],
        "imager" => [900.0, 620.0],
        "utility" => [940.0, 480.0],
        "drive" => [1000.0, 620.0],
        "pitch_correct" => [860.0, 560.0],
        "reverb" => [980.0, 800.0],
        "maximizer" => [900.0, 730.0],
        "vintage_eq" => [820.0, 660.0],
        _ => [760.0, 560.0],
    }
}

/// Effects that only measure: they have no presets nor Match.
pub fn is_meter(id: &str) -> bool {
    matches!(id, "analyzer" | "tuner")
}
