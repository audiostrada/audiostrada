//! The audio editor's stack applied to a decoded source (see `audiostrada_core::audio_edit`).

use audiostrada_core::audio_edit::{AudioEdits, Kind, Level, Region};
use audiostrada_dsp::retouch::{self, Range, Target};
use audiostrada_media::AudioData;

fn ranges(regions: &[Region]) -> Vec<Range> {
    regions
        .iter()
        .map(|r| (r.start as usize, r.end as usize))
        .collect()
}

fn dosed(regions: &[Region], db: f32) -> Vec<(Range, f32)> {
    regions
        .iter()
        .map(|r| ((r.start as usize, r.end as usize), r.db.unwrap_or(db)))
        .collect()
}

/// `data` with the edits switched on, in their order; then its hits moved, then its pauses
/// shortened.
pub fn apply(data: &AudioData, edits: &AudioEdits) -> AudioData {
    let rate = data.sample_rate();
    let mut channels: Vec<Vec<f32>> = (0..data.channels())
        .map(|c| data.channel(c).to_vec())
        .collect();
    for entry in edits.entries.iter().filter(|e| e.on) {
        match &entry.kind {
            Kind::Background { regions, db, .. } => {
                retouch::apply_gains(&mut channels, rate, &dosed(regions, *db), 25.0);
            }
            Kind::Phrases { regions } => {
                retouch::apply_gains(&mut channels, rate, &dosed(regions, 0.0), 20.0);
            }
            Kind::Breaths { regions, db } => {
                retouch::apply_gains(&mut channels, rate, &dosed(regions, *db), 8.0);
            }
            Kind::Gain { regions, db } => {
                retouch::apply_gains(&mut channels, rate, &dosed(regions, *db), 5.0);
            }
            Kind::Normalize { span, level } => {
                let target = match *level {
                    Level::Peak(db) => Target::Peak(db),
                    Level::Loudness(lufs) => Target::Loudness(lufs),
                };
                let range = (span.start as usize, span.end as usize);
                let views: Vec<&[f32]> = channels.iter().map(Vec::as_slice).collect();
                if let Some(db) = retouch::gain_to(&views, rate, range, target) {
                    retouch::apply_gains(&mut channels, rate, &[(range, db)], 5.0);
                }
            }
            Kind::Clicks { regions } => retouch::repair_clicks(&mut channels, &ranges(regions)),
            Kind::Clipping { regions } => {
                retouch::repair_clipping(&mut channels, &ranges(regions));
            }
            Kind::Sibilants { regions, db } => {
                retouch::lower_highs(&mut channels, rate, &dosed(regions, *db));
            }
            Kind::Shorten { .. } | Kind::Moves { .. } => {}
        }
    }
    let timing = edits.timing();
    let anchors: Vec<Range> = timing
        .anchors()
        .iter()
        .map(|&(at, to)| (at as usize, to as usize))
        .collect();
    if !anchors.is_empty() {
        retouch::warp(&mut channels, rate, &anchors);
    }
    let cuts: Vec<Range> = timing
        .cuts()
        .iter()
        .map(|c| (c.start as usize, c.end as usize))
        .collect();
    if !cuts.is_empty() {
        retouch::cut(&mut channels, rate, &cuts, 10.0);
    }
    AudioData::new(channels, rate)
}
