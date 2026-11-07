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
fn the_transport_moves_to_the_bottom_and_grows() {
    let mut h = Harness::with_song("transport");
    h.frame(Vec::new());
    h.act(Action::TransportAtBottom);
    h.act(Action::LargeTransport);
    assert!(h.app.prefs.transport_bottom && h.app.prefs.transport_large);
    for _ in 0..3 {
        h.frame(Vec::new());
    }
    h.act(Action::TransportAtBottom);
    h.act(Action::LargeTransport);
    assert!(!h.app.prefs.transport_bottom && !h.app.prefs.transport_large);
}

#[test]
fn the_recording_mode_has_its_layout_and_puts_everything_back() {
    let mut h = Harness::with_song("recording-mode");
    h.frame(Vec::new());
    let shown = |tree: &egui_tiles::Tree<Pane>| Pane::ALL.map(|p| layout::is_visible(tree, p));
    let before = shown(&h.app.tree);
    let buffer = h.app.host.config().buffer_size;
    assert_ne!(
        buffer, 64,
        "the dummy device's buffer differs from the mode's"
    );
    h.app.doc.borrow_mut().history_mut().edit("Arm", |p| {
        let t = p.add_track("Vox");
        p.track_mut(t).unwrap().armed = true;
    });
    h.frame(Vec::new());
    assert_eq!(
        h.app.recording_offer,
        RecordingOffer::Shown,
        "arming offers the mode"
    );
    let project = h.app.doc.borrow().project().clone();
    h.act(Action::RecordingMode);
    assert!(h.app.recording_mode.is_some());
    assert_eq!(h.app.recording_offer, RecordingOffer::Declined);
    for pane in [Pane::Inputs, Pane::Timeline, Pane::Takes] {
        assert!(layout::is_visible(&h.app.tree, pane), "{pane:?}");
    }
    assert!(!layout::is_visible(&h.app.tree, Pane::Mixer));
    assert_eq!(h.app.host.config().buffer_size, 64);
    assert!(h.app.doc.borrow().player().low_latency());
    for _ in 0..3 {
        h.frame(Vec::new());
    }
    assert!(h.app.recording_health().is_some());
    h.act(Action::ShowMarkers);
    let recording_layout = h.app.prefs.recording_layout.clone();
    assert!(
        recording_layout
            .as_deref()
            .is_some_and(|l| l.contains("Markers"))
    );
    h.act(Action::RecordingMode);
    assert!(h.app.recording_mode.is_none());
    assert_eq!(shown(&h.app.tree), before);
    assert_eq!(shown(&layout::load(h.app.prefs.layout.as_deref())), before);
    assert_eq!(h.app.host.config().buffer_size, buffer);
    assert!(!h.app.doc.borrow().player().low_latency());
    assert_eq!(
        *h.app.doc.borrow().project(),
        project,
        "the project is not touched"
    );
    h.act(Action::RecordingMode);
    assert!(
        layout::is_visible(&h.app.tree, Pane::Markers),
        "its own layout comes back"
    );
}

#[test]
fn a_module_draws_every_frame_and_keeps_its_data_in_the_project() {
    use std::cell::Cell;
    use std::rc::Rc;
    struct Module(Rc<Cell<u32>>, Option<audiostrada_engine::tap::InputTap>);
    impl extension::Extension for Module {
        fn name(&self) -> &'static str {
            "test"
        }
        fn tick(&mut self, _ctx: &egui::Context, doc: &mut document::Document) {
            self.0.set(self.0.get() + 1);
            if self.0.get() == 2 {
                doc.set_extension_data("test", "Module edit", serde_json::json!({"n": 2}));
                self.1 = doc.open_armed_input_tap(1.0).ok();
            }
        }
        fn menu_title(&self) -> Option<String> {
            Some("Module".into())
        }
    }
    let ticks = Rc::new(Cell::new(0));
    let mut h = Harness::with_song("module");
    h.app
        .add_extension(Box::new(Module(Rc::clone(&ticks), None)));
    for _ in 0..3 {
        h.frame(Vec::new());
    }
    assert_eq!(ticks.get(), 3);
    let data = h.app.doc.borrow().extension_data("test").cloned();
    assert_eq!(data, Some(serde_json::json!({"n": 2})));
    h.act(Action::Undo);
    assert!(h.app.doc.borrow().extension_data("test").is_none());
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

/// The application swept by a user trying everything, `part` of four bands of the window.
fn sweep_app(part: usize) {
    let mut h = Harness::with_song(&format!("sweep-{part}"));
    h.frame(Vec::new());
    let band = Rect::from_min_size(Pos2::new(0.0, part as f32 * 250.0), vec2(1600.0, 250.0));
    for events in crate::test_sweep::script(band, 45.0, true) {
        h.frame(events);
    }
    // Then with every panel shown, in their tabs.
    for action in [
        Action::ShowMixer,
        Action::ShowPianoRoll,
        Action::ShowRack,
        Action::ShowBrowser,
        Action::ShowInspector,
        Action::ShowMarkers,
    ] {
        h.act(action);
        for events in crate::test_sweep::script(band, 90.0, false) {
            h.frame(events);
        }
    }
}

#[test]
fn the_app_answers_a_user_trying_everything_0() {
    sweep_app(0);
}

#[test]
fn the_app_answers_a_user_trying_everything_1() {
    sweep_app(1);
}

#[test]
fn the_app_answers_a_user_trying_everything_2() {
    sweep_app(2);
}

#[test]
fn the_app_answers_a_user_trying_everything_3() {
    sweep_app(3);
}

#[test]
fn the_inspector_and_the_audio_settings_answer_a_user_trying_everything() {
    let h = Harness::with_song("inspector-sweep");
    let doc = Rc::clone(&h.app.doc);
    let tracks: Vec<_> = doc.borrow().project().tracks.iter().map(|t| t.id).collect();
    let ctx = egui::Context::default();
    let area = Rect::from_min_size(Pos2::ZERO, vec2(500.0, 900.0));
    for &track in tracks.iter().take(3) {
        doc.borrow_mut()
            .select_track(track, crate::document::SelectMode::Only);
        let selection: Vec<_> = doc
            .borrow()
            .project()
            .track(track)
            .map(|t| t.clips.iter().map(|c| c.id).collect())
            .unwrap_or_default();
        for events in crate::test_sweep::script(area, 30.0, true) {
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(500.0, 900.0))),
                events,
                ..Default::default()
            };
            ctx.run_ui(input, |ui| {
                crate::inspector::show(ui, &mut doc.borrow_mut(), &selection)
            })
            .drop_without_applying_deltas();
        }
    }
    let mut panel = crate::settings::AudioSettingsPanel::default();
    let ctx = egui::Context::default();
    for events in crate::test_sweep::script(
        Rect::from_min_size(Pos2::ZERO, vec2(700.0, 700.0)),
        25.0,
        false,
    ) {
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(700.0, 700.0))),
            events,
            ..Default::default()
        };
        ctx.run_ui(input, |ui| {
            let _ = panel.show(ui, &h.app.host);
        })
        .drop_without_applying_deltas();
    }
}

#[test]
fn an_assistant_drives_the_running_application() {
    let mut h = Harness::with_song("live");
    for _ in 0..20 {
        h.frame(Vec::new());
    }
    let (server, requests) = crate::control::Server::for_test();
    h.app.control = Some(server);
    let ctx = {
        let doc = h.app.doc.borrow();
        let project = doc.project();
        crate::control::tests::Guess {
            track: project.tracks[0].name.clone(),
            clips: project
                .tracks
                .iter()
                .flat_map(|t| t.clips.iter().map(|c| c.id.0))
                .collect(),
            file: h.dir.join("tone.wav").to_string_lossy().into_owned(),
        }
    };
    let mut asked = vec![
        ("list_actions".to_owned(), serde_json::json!({})),
        (
            "run_action".to_owned(),
            serde_json::json!({"name": "Toggle metronome"}),
        ),
        (
            "run_action".to_owned(),
            serde_json::json!({"name": "no such action"}),
        ),
        (
            "show".to_owned(),
            serde_json::json!({"track": ctx.track, "start": 0.0, "end": 1.0}),
        ),
        ("show".to_owned(), serde_json::json!({"clips": ctx.clips})),
    ];
    for definition in crate::control::tool_definitions(true) {
        let name = definition["name"].as_str().unwrap_or_default().to_owned();
        let mut args = serde_json::Map::new();
        if let Some(props) = definition["inputSchema"]["properties"].as_object() {
            for (key, schema) in props {
                args.insert(key.clone(), crate::control::tests::guess(key, schema, &ctx));
            }
        }
        for key in ["path", "file", "output"] {
            if args.contains_key(key) && (name.starts_with("export") || name.starts_with("save")) {
                args.insert(
                    key.into(),
                    serde_json::json!(h.dir.join(format!("{name}.out")).to_string_lossy()),
                );
            }
        }
        asked.push((name.clone(), serde_json::json!({})));
        asked.push((name, serde_json::Value::Object(args)));
    }
    let mut answered = 0;
    for (tool, args) in asked {
        let (reply, answer) = std::sync::mpsc::channel();
        requests
            .send(crate::control::Request { tool, args, reply })
            .expect("the application listens");
        h.frame(Vec::new());
        if answer.try_recv().is_ok() {
            answered += 1;
        }
    }
    assert!(answered > 100, "{answered} answers");
    assert!(!h.app.control_log.is_empty());
}

#[test]
fn every_menu_item_is_clicked() {
    use eframe::egui::{PointerButton, pos2};
    let mut h = Harness::with_song("menus");
    h.frame(Vec::new());
    let click = |h: &mut Harness, at: Pos2| {
        h.frame(vec![Event::PointerMoved(at)]);
        for pressed in [true, false] {
            h.frame(vec![Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            }]);
        }
        h.frame(Vec::new());
    };
    let escape = Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    // The menu titles sit at the left of the top bar.
    let mut x = 12.0;
    while x < 330.0 {
        let mut y = 34.0;
        while y < 760.0 {
            click(&mut h, pos2(x, 10.0));
            click(&mut h, pos2(x + 40.0, y));
            h.frame(vec![escape.clone()]);
            h.frame(vec![escape.clone()]);
            y += 21.0;
        }
        x += 38.0;
    }
}

impl Harness {
    /// Clicks over the middle of the window, where dialogs and modals stand.
    fn answer_dialogs(&mut self) {
        let area = Rect::from_min_size(Pos2::new(450.0, 300.0), vec2(700.0, 400.0));
        for events in crate::test_sweep::script(area, 30.0, false) {
            self.frame(events);
        }
    }
}

#[test]
fn projects_save_open_in_tabs_and_close() {
    let mut h = Harness::with_song("projects");
    for _ in 0..10 {
        h.frame(Vec::new());
    }
    let file = h.dir.join("Song").join("Song.strada");
    h.app
        .doc
        .borrow_mut()
        .save_as(audiostrada_core::ProjectPaths::new(&file))
        .expect("saved");
    h.act(Action::Save);
    assert!(file.exists());
    // Opened again, in a tab of its own, then in place.
    let path = file.clone();
    h.frame_with(Vec::new(), |app, ctx| app.open(ctx, path, true));
    h.act(Action::NextTab);
    h.act(Action::PreviousTab);
    let path = file.clone();
    h.frame_with(Vec::new(), |app, ctx| app.open(ctx, path, false));
    // A file that is not there.
    let missing = h.dir.join("missing.strada");
    h.frame_with(Vec::new(), |app, ctx| app.open(ctx, missing, false));
    h.answer_dialogs();
    // Unsaved changes ask before a new project, closing a tab and quitting.
    for pending in [Pending::New, Pending::CloseTab, Pending::Quit] {
        h.app.doc.borrow_mut().history_mut().edit("Change", |p| {
            p.add_track("More");
        });
        h.frame_with(Vec::new(), |app, ctx| app.request(ctx, pending));
        h.frame(Vec::new());
        h.answer_dialogs();
    }
    h.act(Action::CloseTab);
    h.answer_dialogs();
}
