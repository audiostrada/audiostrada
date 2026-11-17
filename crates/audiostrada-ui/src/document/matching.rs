//! The global bypass heard at the same loudness: the whole mix is rendered in the background
//! with its effects and without them, and the main output makes up the difference while
//! every effect is bypassed.

use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use audiostrada_core::Project;
use audiostrada_engine::export::OfflineRender;
use audiostrada_engine::project::ProjectPlayer;

use super::Document;

/// Quiet time after a change before measuring again.
const SETTLE: Duration = Duration::from_millis(1500);
/// The difference made up at most (dB).
const RANGE: f32 = 24.0;

/// The integrated loudness of a mix rendered in the background.
#[derive(Debug)]
struct Loudness {
    /// The player holding the render's plugins, updated meanwhile.
    player: ProjectPlayer,
    handle: Option<JoinHandle<Option<f64>>>,
}

impl Loudness {
    /// The loudness once rendered (`None` for silence).
    fn poll(&mut self) -> Option<Option<f64>> {
        self.player.update();
        if !self.handle.as_ref()?.is_finished() {
            return None;
        }
        Some(self.handle.take()?.join().ok().flatten())
    }
}

#[derive(Debug, Default)]
pub(super) struct Matching {
    /// With its effects, without, and the revision rendered.
    measuring: Option<(Loudness, Loudness, u64)>,
    /// What the mix loses without its effects (dB), and for which revision.
    gap: Option<(f32, u64)>,
    /// The revision last seen, and since when.
    seen: Option<(u64, Instant)>,
}

impl Document {
    /// While the global bypass is heard at the same loudness: how much the mix without its
    /// effects is raised (dB), and whether it is being measured again.
    pub fn matched_bypass(&self) -> (Option<f32>, bool) {
        let m = &self.matching;
        (m.gap.map(|g| g.0), m.measuring.is_some())
    }

    fn start_loudness(&self, project: &Project) -> Loudness {
        let render = OfflineRender::in_background(
            project,
            self.paths.as_ref(),
            &self.media,
            self.player.plugins().clone(),
            0,
            project.length(),
        );
        let (player, mut render) = render.split();
        let handle = std::thread::spawn(move || {
            audiostrada_rt::thread::lower_current_thread();
            while !render.render(1 << 16) {}
            render.finish().loudness
        });
        Loudness {
            player,
            handle: Some(handle),
        }
    }

    /// Measures again once the project rests after a change, and sets the gain heard.
    pub(super) fn poll_matching(&mut self) {
        let project = self.history.project();
        if !project.all_fx_matched || project.length() == 0 {
            self.matching = Matching::default();
            self.player.set_matched_gain(0.0);
            return;
        }
        let revision = self.history.revision();
        let m = &mut self.matching;
        if m.seen.is_none_or(|s| s.0 != revision) {
            m.seen = Some((revision, Instant::now()));
        }
        if let Some((with, without, measured)) = &mut m.measuring
            && let (Some(with), Some(without)) = (with.poll(), without.poll())
        {
            let gap = match (with, without) {
                (Some(a), Some(b)) => ((a - b) as f32).clamp(-RANGE, RANGE),
                _ => 0.0,
            };
            m.gap = Some((gap, *measured));
            m.measuring = None;
        }
        let stale = m.gap.is_none_or(|g| g.1 != revision);
        let rested = m.seen.is_some_and(|s| s.1.elapsed() >= SETTLE);
        if m.measuring.is_none() && stale && rested {
            let mut with = project.clone();
            with.all_fx_off = false;
            let mut without = project.clone();
            without.all_fx_off = true;
            let measuring = (
                self.start_loudness(&with),
                self.start_loudness(&without),
                revision,
            );
            self.matching.measuring = Some(measuring);
        }
        let gap = self.matching.gap.map_or(0.0, |g| g.0);
        let heard = if self.history.project().all_fx_off {
            gap
        } else {
            0.0
        };
        self.player.set_matched_gain(heard);
    }
}
