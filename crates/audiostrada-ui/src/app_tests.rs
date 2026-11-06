//! The whole application, headless, on the dummy audio backend: a project with every kind of
//! track and effect, then every command and window, frame after frame, as on the three systems.

use std::path::PathBuf;

use audiostrada_core::model::Note;
use audiostrada_core::routing::{self, Channel};
use audiostrada_io::{BackendKind, StreamConfig};
use audiostrada_media::{AudioData, write_wav};
use eframe::egui::{Pos2, RawInput, Rect, vec2};

use super::*;

pub(crate) struct Harness {
    pub app: AudioStradaApp,
    pub ctx: egui::Context,
    frame: eframe::Frame,
    dir: PathBuf,
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl Harness {
    /// The application on an empty project, its files in a folder of its own.
    pub fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("audiostrada-app-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let dirs = AppDirs {
            recovery: Some(dir.join("recovery")),
            peaks: Some(dir.join("peaks")),
            recordings: Some(dir.join("recordings")),
        };
        let doc = Rc::new(RefCell::new(Document::new(dirs.clone())));
        let mut host = AudioHost::new({
            let doc = Rc::clone(&doc);
            Box::new(move |info| {
                Box::new(
                    doc.borrow_mut()
                        .processor(info.sample_rate, info.buffer_size as usize),
                )
            })
        });
        host.start(Some(BackendKind::Dummy), StreamConfig::default());
        let hooks = Hooks {
            audio_changed: Box::new(|_| {}),
            recent_changed: Box::new(|_| {}),
            ui_changed: Box::new(|_| {}),
            plugin_scan: Box::new(|_, _| std::thread::spawn(Database::default)),
        };
        // English, as the other tests of this process expect (the language is global).
        let prefs = UiPrefs {
            check_updates: Some(false),
            language: crate::i18n::Language::English,
            ..UiPrefs::default()
        };
        let app = AudioStradaApp::new(host, doc, dirs, prefs, Vec::new(), None, hooks);
        Self {
            app,
            ctx: egui::Context::default(),
            frame: eframe::Frame::_new_kittest(),
            dir,
        }
    }

    /// With a song: an audio track, a synth, a drum machine, every effect, a folder and a VCA.
    pub fn with_song(name: &str) -> Self {
        let h = Self::new(name);
        let wav = h.dir.join("tone.wav");
        let tone: Vec<f32> = (0..96_000)
            .map(|i| (i as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin() * 0.3)
            .collect();
        write_wav(&wav, &AudioData::new(vec![tone.clone(), tone], 48_000)).unwrap();
        {
            let mut doc = h.app.doc.borrow_mut();
            doc.import(&[wav]);
            doc.history_mut().edit("Setup", |p| {
                let note = |start, key| Note {
                    start,
                    length: 6_000,
                    key,
                    velocity: 100,
                    channel: 0,
                    probability: 100,
                    muted: false,
                };
                let keys = p.add_track("Keys");
                routing::add_insert(p, Channel::Track(keys), "synth", Default::default());
                p.add_midi_clip(
                    keys,
                    0,
                    96_000,
                    vec![note(0, 60), note(24_000, 64), note(48_000, 67)],
                );
                let drums = p.add_track("Drums");
                routing::add_insert(p, Channel::Track(drums), "drums", Default::default());
                p.add_midi_clip(
                    drums,
                    0,
                    96_000,
                    vec![note(0, 36), note(12_000, 38), note(24_000, 42)],
                );
                let fx = p.add_track("Effects");
                for info in audiostrada_dsp::EFFECTS.iter().filter(|e| !e.instrument) {
                    routing::add_insert(p, Channel::Track(fx), info.id, Default::default());
                }
                let vca = routing::add_vca(p, "VCA");
                let _ = routing::set_vca(p, keys, Some(vca));
                let _ = routing::set_vca(p, drums, Some(vca));
            });
        }
        h
    }

    /// One frame, with `events`, at a desktop size.
    pub fn frame(&mut self, events: Vec<Event>) {
        self.frame_with(events, |_, _| {});
    }

    /// One frame, then `after` with the application inside it.
    pub fn frame_with(
        &mut self,
        events: Vec<Event>,
        after: impl FnOnce(&mut AudioStradaApp, &egui::Context),
    ) {
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1600.0, 1000.0))),
            events,
            ..Default::default()
        };
        let (app, frame) = (&mut self.app, &mut self.frame);
        let mut after = Some(after);
        self.ctx
            .run_ui(input, |ui| {
                eframe::App::logic(app, ui.ctx(), frame);
                eframe::App::ui(app, ui, frame);
                if let Some(after) = after.take() {
                    after(app, ui.ctx());
                }
            })
            .drop_without_applying_deltas();
    }

    /// Runs `action` as a menu or a shortcut would, and the frames that follow.
    pub fn act(&mut self, action: Action) {
        self.frame_with(Vec::new(), |app, ctx| app.run(ctx, action));
        self.frame(Vec::new());
        self.frame(Vec::new());
    }
}

#[test]
fn the_app_starts_and_draws_a_song() {
    let mut h = Harness::with_song("song");
    for _ in 0..5 {
        h.frame(Vec::new());
    }
    let doc = h.app.doc.borrow();
    let project = doc.project();
    assert!(project.tracks.len() >= 5, "{} tracks", project.tracks.len());
    assert!(project.tracks.iter().any(|t| t.name == "tone"));
}

#[test]
fn every_command_runs_without_a_panic() {
    let mut h = Harness::with_song("commands");
    h.frame(Vec::new());
    for &action in Action::ALL {
        // Quitting ends the application; the rest must keep it standing.
        if action == Action::Quit {
            continue;
        }
        h.act(action);
    }
    // Undo everything that changed the song, then redo it.
    for _ in 0..40 {
        h.act(Action::Undo);
    }
    for _ in 0..40 {
        h.act(Action::Redo);
    }
}

impl Harness {
    /// Every clip selected and a range over them, as before a processing command.
    fn select_everything(&mut self) {
        let clips: Vec<_> = self
            .app
            .doc
            .borrow()
            .project()
            .tracks
            .iter()
            .flat_map(|t| t.clips.iter().map(|c| c.id))
            .collect();
        self.app.views.borrow_mut().timeline.selection = clips;
        self.act(Action::RangeFromClips);
    }
}

#[test]
fn every_command_runs_on_a_selection() {
    let mut h = Harness::with_song("selection");
    h.frame(Vec::new());
    for &action in Action::ALL {
        if matches!(
            action,
            Action::Quit | Action::Deselect | Action::NewProject | Action::CloseTab
        ) {
            continue;
        }
        h.select_everything();
        h.act(action);
    }
}

#[test]
fn the_inspector_and_the_track_menu_draw_for_every_kind_of_track() {
    let mut h = Harness::with_song("inspector");
    h.frame(Vec::new());
    let doc = Rc::clone(&h.app.doc);
    let tracks: Vec<_> = doc.borrow().project().tracks.iter().map(|t| t.id).collect();
    let clips: Vec<_> = doc
        .borrow()
        .project()
        .tracks
        .iter()
        .flat_map(|t| t.clips.iter().map(|c| c.id))
        .collect();
    let input = || RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 900.0))),
        ..Default::default()
    };
    let ctx = egui::Context::default();
    for selection in [Vec::new(), clips.clone()]
        .into_iter()
        .chain(clips.iter().map(|&c| vec![c]))
    {
        ctx.run_ui(input(), |ui| {
            crate::inspector::show(ui, &mut doc.borrow_mut(), &selection);
        })
        .drop_without_applying_deltas();
    }
    for &track in &tracks {
        ctx.run_ui(input(), |ui| {
            let requests = crate::track_menu::show(ui, &mut doc.borrow_mut(), track);
            assert!(requests.is_empty());
        })
        .drop_without_applying_deltas();
    }
}
