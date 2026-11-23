//! Offline rendering of a project, faster than real time: a separate player renders block by
//! block when asked, measuring the peak and the integrated loudness on the way.

use audiostrada_core::{Project, ProjectPaths};
use audiostrada_io::ChannelBuffers;
use audiostrada_media::AudioData;
use audiostrada_plugins::scan::Database;

use crate::evaluation;
use crate::loudness::LoudnessAnalyzer;
use crate::media::MediaPool;
use crate::project::ProjectPlayer;
use crate::{EngineConfig, EngineProcessor};

/// Offline blocks are long: the workers then have enough to do to be worth waking.
const BLOCK: usize = 4096;

/// A render in progress.
#[derive(Debug)]
pub struct OfflineRender {
    player: ProjectPlayer,
    worker: RenderWorker,
    /// First project sample rendered.
    start: u64,
}

/// The computing part of a render, which can run on any thread (see [`OfflineRender::split`]).
#[derive(Debug)]
pub struct RenderWorker {
    processor: EngineProcessor,
    storage: ChannelBuffers,
    total: u64,
    rendered: u64,
    channels: [Vec<f32>; 2],
    loudness: LoudnessAnalyzer,
    peak: f32,
    rate: u32,
    /// Off for a render only measured, never heard nor written.
    evaluated: bool,
    /// Only these spans are rendered and kept (engine samples from the start, sorted), when
    /// set: the transport jumps over what lies between.
    keep: Option<Vec<(u64, u64)>>,
    /// The transport's position at the start, engine samples.
    origin: u64,
}

/// The rendered audio and its measurements.
#[derive(Debug)]
pub struct Rendered {
    pub audio: AudioData,
    pub peak: f32,
    /// Integrated loudness in LUFS, `None` for silence.
    pub loudness: Option<f64>,
}

impl OfflineRender {
    /// Renders `project` from `start` to `end` (project samples) plus `tail` seconds, at the
    /// rate of `media`.
    pub fn new(
        project: &Project,
        paths: Option<&ProjectPaths>,
        media: &MediaPool,
        plugins: Database,
        start: u64,
        end: u64,
        tail: f64,
    ) -> Self {
        // Every core, whatever the load: a render is not bound to real time.
        let workers = std::thread::available_parallelism()
            .map_or(0, |n| n.get().saturating_sub(1))
            .min(7);
        Self::with_workers(
            project,
            (paths, media, plugins),
            (start, end, tail),
            workers,
        )
    }

    /// A render measured while the music plays: one worker thread, behind the audio (the
    /// thread rendering it is lowered by its caller, see `audiostrada_rt::thread`).
    pub fn in_background(
        project: &Project,
        paths: Option<&ProjectPaths>,
        media: &MediaPool,
        plugins: Database,
        start: u64,
        end: u64,
    ) -> Self {
        let render = Self::with_workers(project, (paths, media, plugins), (start, end, 0.0), 1);
        render.worker.processor.lower_workers();
        render.measured_only()
    }

    fn with_workers(
        project: &Project,
        (paths, media, plugins): (Option<&ProjectPaths>, &MediaPool, Database),
        (start, end, tail): (u64, u64, f64),
        workers: usize,
    ) -> Self {
        let rate = media.rate();
        let mut player = ProjectPlayer::new(EngineConfig {
            workers: Some(workers),
        });
        player.set_offline(true);
        player.set_plugins(plugins);
        let processor = player.processor(rate, BLOCK);
        let mut project = project.clone();
        project.loop_range = None;
        player.sync(&project, paths, media);
        player.locate(start);
        player.engine_mut().play();
        let total =
            player.to_engine(end.saturating_sub(start)) + (tail.max(0.0) * f64::from(rate)) as u64;
        let origin = player.to_engine(start);
        Self {
            player,
            worker: RenderWorker {
                processor,
                storage: ChannelBuffers::new(0, 2, BLOCK),
                total,
                rendered: 0,
                channels: [Vec::new(), Vec::new()],
                loudness: LoudnessAnalyzer::new(rate),
                peak: 0.0,
                rate,
                evaluated: true,
                keep: None,
                origin,
            },
            start,
        }
    }

    /// A render to measure only: its audio is never heard nor written, so the evaluation's
    /// dropouts would only falsify the figures.
    pub fn measured_only(mut self) -> Self {
        self.worker.evaluated = false;
        self
    }

    /// Keeps only what plays within `spans` (project samples, sorted, apart): a project's long
    /// gaps are rendered but never held in memory, and the render stops after the last span.
    pub fn keeping(mut self, spans: &[(u64, u64)]) -> Self {
        let w = &mut self.worker;
        let keep: Vec<(u64, u64)> = spans
            .iter()
            .map(|&(a, b)| {
                let a = self.player.to_engine(a.saturating_sub(self.start));
                let b = self.player.to_engine(b.saturating_sub(self.start));
                (a.min(w.total), b.min(w.total))
            })
            .filter(|(a, b)| b > a)
            .collect();
        w.total = keep.last().map_or(0, |s| s.1);
        w.keep = Some(keep);
        self
    }

    /// Renders up to `frames` more frames; true once everything is rendered.
    pub fn render(&mut self, frames: u64) -> bool {
        let done = self.worker.render(frames);
        self.player.update();
        done
    }

    pub fn finish(self) -> Rendered {
        self.worker.finish()
    }

    /// Splits the render: the worker computes on another thread while the player, which
    /// holds the plugins, stays on this one and is updated (`ProjectPlayer::update`) until
    /// the worker is done.
    pub fn split(self) -> (ProjectPlayer, RenderWorker) {
        (self.player, self.worker)
    }
}

impl RenderWorker {
    /// Renders up to `frames` more frames; true once everything is rendered.
    pub fn render(&mut self, frames: u64) -> bool {
        if self.rendered == 0 && self.channels[0].capacity() == 0 {
            let kept = self
                .keep
                .as_ref()
                .map_or(self.total, |k| k.iter().map(|(a, b)| b - a).sum());
            for c in &mut self.channels {
                c.reserve_exact(kept as usize);
            }
        }
        let mut target = (self.rendered + frames).min(self.total);
        while self.rendered < target {
            // Past a span kept: on to the next one, the silence between left unrendered.
            if let Some(next) = self
                .keep
                .as_ref()
                .and_then(|k| k.iter().find(|s| s.1 > self.rendered))
                && next.0 > self.rendered
            {
                let skipped = next.0 - self.rendered;
                self.processor.locate(self.origin + next.0);
                self.rendered = next.0;
                target = (target + skipped).min(self.total);
                continue;
            }
            let n = ((target - self.rendered) as usize).min(BLOCK);
            let mut buffers = self.storage.buffers(n);
            self.processor.render(&mut buffers);
            if self.evaluated
                && let Some(t) =
                    evaluation::export_clock(self.rendered as f64 / f64::from(self.rate))
            {
                evaluation::apply(&mut buffers, n, t, self.rate);
            }
            let (l, r) = (&self.storage.output(0)[..n], &self.storage.output(1)[..n]);
            let block = (self.rendered, self.rendered + n as u64);
            let kept: Vec<(usize, usize)> = match &self.keep {
                None => vec![(0, n)],
                Some(spans) => spans
                    .iter()
                    .filter(|s| s.0 < block.1 && s.1 > block.0)
                    .map(|s| {
                        let a = s.0.max(block.0) - block.0;
                        let b = s.1.min(block.1) - block.0;
                        (a as usize, b as usize)
                    })
                    .collect(),
            };
            for (a, b) in kept {
                let (l, r) = (&l[a..b], &r[a..b]);
                self.peak = l.iter().chain(r).fold(self.peak, |p, s| p.max(s.abs()));
                self.loudness.push(l, r);
                self.channels[0].extend_from_slice(l);
                self.channels[1].extend_from_slice(r);
            }
            self.rendered += n as u64;
        }
        self.rendered >= self.total
    }

    /// Fraction rendered, 0 to 1.
    pub fn progress(&self) -> f32 {
        if self.total == 0 {
            1.0
        } else {
            self.rendered as f32 / self.total as f32
        }
    }

    pub fn finish(self) -> Rendered {
        let [left, right] = self.channels;
        Rendered {
            audio: AudioData::new(vec![left, right], self.rate),
            peak: self.peak,
            loudness: self.loudness.integrated(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use audiostrada_core::Source;
    use audiostrada_media::write_wav;

    use super::*;
    use crate::project::media_paths;

    #[test]
    #[ignore = "times AUDIOSTRADA_BENCH_PROJECT's render"]
    fn bench_render() {
        let path = std::path::PathBuf::from(std::env::var("AUDIOSTRADA_BENCH_PROJECT").unwrap());
        let project = audiostrada_core::file::load(&path).unwrap();
        let paths = audiostrada_core::file::ProjectPaths::new(path);
        let mut media = MediaPool::new(48_000, None);
        media.retain(&media_paths(&project, Some(&paths)));
        assert!(media.wait(Duration::from_secs(60)));
        let end = project.length();
        let started = std::time::Instant::now();
        let mut render = OfflineRender::new(
            &project,
            Some(&paths),
            &media,
            Database::default(),
            0,
            end,
            0.0,
        );
        while !render.render(1 << 20) {}
        eprintln!(
            "render: {:?} for {:.0} s",
            started.elapsed(),
            end as f64 / 48_000.0
        );
    }

    #[test]
    fn renders_the_project_with_tail_and_measures_it() {
        let dir = audiostrada_core::scratch::dir("export");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.wav");
        write_wav(&file, &AudioData::new(vec![vec![0.5; 48_000]], 48_000)).unwrap();
        let mut project = Project::new("Test", 48_000);
        let s = project.add_source(Source {
            path: file.clone(),
            channels: 1,
            sample_rate: 48_000,
            frames: 48_000,
        });
        let t = project.add_track("a");
        project.add_clip(t, s, 0);
        let mut media = MediaPool::new(48_000, None);
        media.retain(&media_paths(&project, None));
        assert!(media.wait(Duration::from_secs(10)));
        let mut render =
            OfflineRender::new(&project, None, &media, Database::default(), 0, 48_000, 0.5);
        let mut steps = 0;
        while !render.render(10_000) {
            steps += 1;
        }
        assert!(steps >= 6);
        let result = render.finish();
        assert_eq!(result.audio.frames(), 72_000);
        assert!((result.peak - 0.5).abs() < 1e-6);
        assert!(
            result.audio.channel(0)[60_000].abs() < 1e-6,
            "tail is silent here"
        );
        // A constant signal is mostly removed by the K-weighting high-pass.
        assert!(result.loudness.is_some_and(|l| l < -20.0));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
