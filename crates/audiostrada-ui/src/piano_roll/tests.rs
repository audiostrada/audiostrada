use super::*;
use crate::document::AppDirs;

fn note(start: u64, key: u8, channel: u8) -> Note {
    Note {
        start,
        length: 1000,
        key,
        velocity: 100,
        channel,
        ..Default::default()
    }
}

fn roll(notes: Vec<Note>) -> (PianoRoll, Rc<RefCell<Document>>, ClipId) {
    let mut p = Project::new("t", 48_000);
    let t = p.add_track("Synth");
    let clip = p.add_midi_clip(t, 0, 96_000, notes).unwrap();
    let mut doc = Document::new(AppDirs::default());
    doc.history_mut().replace(p, false);
    let doc = Rc::new(RefCell::new(doc));
    let mut roll = PianoRoll::new(Rc::clone(&doc));
    roll.clips = vec![clip];
    roll.active = Some(clip);
    (roll, doc, clip)
}

fn notes_of(doc: &Rc<RefCell<Document>>, clip: ClipId) -> Vec<Note> {
    let doc = doc.borrow();
    (*edit::clip(doc.project(), clip)
        .unwrap()
        .notes
        .clone()
        .unwrap())
    .clone()
}

fn keys(doc: &Rc<RefCell<Document>>, clip: ClipId) -> Vec<(u64, u8)> {
    notes_of(doc, clip)
        .iter()
        .map(|n| (n.start, n.key))
        .collect()
}

#[test]
fn selected_notes_are_deleted_and_quantized() {
    let (mut roll, doc, clip) = roll(vec![note(1000, 60, 0), note(23_000, 64, 1)]);
    assert!(!roll.delete_selected(), "nothing selected");
    roll.select_all();
    assert_eq!(roll.selected, [0, 1]);
    let pair = [note(0, 60, 0), note(0, 64, 1)];
    assert_eq!(roll.lane_channel(&pair), 0, "mixed channels");
    roll.selected = vec![1];
    assert_eq!(roll.lane_channel(&pair), 1);

    roll.quantize(&EditSettings::default());
    assert_eq!(
        keys(&doc, clip),
        [(1000, 60), (24_000, 64)],
        "only the selection"
    );
    assert_eq!(roll.selected, [1], "the selection follows the note");
    roll.selected.clear();
    roll.quantize(&EditSettings::default());
    assert_eq!(keys(&doc, clip), [(0, 60), (24_000, 64)]);
    assert_eq!(
        doc.borrow().history().undo_label(),
        Some(tr("Quantize notes"))
    );

    roll.selected = vec![0];
    assert!(roll.delete_selected());
    assert_eq!(keys(&doc, clip), [(24_000, 64)]);
    assert!(roll.selected.is_empty());
    roll.reset();
    assert!(roll.scene().is_none());
}

/// Draws the roll for one frame with input events, in the root Ui.
fn frame(roll: &mut PianoRoll, ctx: &egui::Context, events: Vec<egui::Event>) {
    let input = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1400.0, 900.0))),
        events,
        ..Default::default()
    };
    let clips = roll.clips.clone();
    ctx.run_ui(input, |ui| roll.show(ui, &EditSettings::default(), &clips))
        .drop_without_applying_deltas();
}

fn button(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

/// Presses at `from`, moves to `to` in a few steps and releases there.
fn drag(roll: &mut PianoRoll, ctx: &egui::Context, from: egui::Pos2, to: egui::Pos2) {
    frame(roll, ctx, vec![egui::Event::PointerMoved(from)]);
    frame(roll, ctx, vec![button(from, true)]);
    for t in [0.1, 0.4, 1.0] {
        frame(roll, ctx, vec![egui::Event::PointerMoved(from.lerp(to, t))]);
    }
    frame(roll, ctx, vec![button(to, false)]);
    frame(roll, ctx, vec![]);
}

/// A frame drawn, then the view set: 100 samples a pixel from the clip start, key 60 shown.
fn framed(roll: &mut PianoRoll, ctx: &egui::Context) -> view::Frame {
    frame(roll, ctx, vec![]);
    roll.view.zoom = 100.0;
    roll.view.scroll = 0.0;
    roll.view.key_height = 20.0;
    roll.view.scroll_y = 60.0 * 20.0;
    frame(roll, ctx, vec![]);
    roll.frame(roll.area)
}

#[test]
fn dragging_a_notes_right_edge_resizes_it() {
    let (mut roll, doc, clip) = roll(vec![Note {
        length: 24_000,
        ..note(0, 60, 0)
    }]);
    let ctx = egui::Context::default();
    let f = framed(&mut roll, &ctx);
    let r = f.note_rect(0, 24_000, 60).unwrap();
    let edge = pos2(r.right() - 2.0, r.center().y);
    drag(&mut roll, &ctx, edge, edge + vec2(107.0, 0.0));
    let notes = notes_of(&doc, clip);
    assert_eq!(notes.len(), 1, "no rubber band, no new note");
    assert_eq!(notes[0].length, 36_000, "to the next sixteenth");
    assert_eq!(
        doc.borrow().history().undo_label(),
        Some(tr("Resize notes"))
    );
}

#[test]
fn the_pencil_adds_notes_and_drags_their_length_and_the_eraser_removes_them() {
    let (mut roll, doc, clip) = roll(vec![note(0, 60, 0)]);
    let ctx = egui::Context::default();
    roll.tool = Tool::Draw;
    let f = framed(&mut roll, &ctx);
    let y = f.y(64).unwrap() + 10.0;
    let x = f.x(48_000) + 2.0;
    drag(&mut roll, &ctx, pos2(x, y), pos2(x + 240.0, y));
    let added = notes_of(&doc, clip)
        .into_iter()
        .find(|n| n.key == 64)
        .expect("a note on E");
    assert_eq!((added.start, added.length), (48_000, 24_000));
    roll.stamp.on = true;
    roll.scale = Some(music::Scale { root: 0, mode: 0 });
    let y = f.y(62).unwrap() + 10.0;
    let x = f.x(96_000) + 2.0;
    frame(&mut roll, &ctx, vec![egui::Event::PointerMoved(pos2(x, y))]);
    frame(&mut roll, &ctx, vec![button(pos2(x, y), true)]);
    frame(&mut roll, &ctx, vec![button(pos2(x, y), false)]);
    let mut chord: Vec<u8> = notes_of(&doc, clip)
        .iter()
        .filter(|n| n.start == 96_000)
        .map(|n| n.key)
        .collect();
    chord.sort_unstable();
    assert_eq!(chord, [62, 65, 69], "the scale's chord on D: D minor");
    roll.tool = Tool::Erase;
    let r = f.note_rect(0, 1000, 60).unwrap();
    drag(&mut roll, &ctx, r.center(), r.center() + vec2(30.0, 0.0));
    assert!(!keys(&doc, clip).contains(&(0, 60)));
    assert_eq!(doc.borrow().history().undo_label(), Some(tr("Erase notes")));
}

#[test]
fn a_transform_is_previewed_then_applied_as_one_step() {
    let (mut roll, doc, clip) = roll(vec![
        note(1000, 60, 0),
        note(1000, 64, 0),
        note(1000, 67, 0),
    ]);
    let ctx = egui::Context::default();
    frame(&mut roll, &ctx, vec![]);
    let scene = roll.scene().unwrap();
    roll.open_transform(
        panel::Transform::Strum {
            ms: 10.0,
            direction: ops::Strum::Up,
        },
        &scene,
    );
    frame(&mut roll, &ctx, vec![]);
    assert_eq!(
        keys(&doc, clip),
        [(1000, 60), (1000, 64), (1000, 67)],
        "not applied yet"
    );
    let enter = egui::Event::Key {
        key: egui::Key::Enter,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    frame(&mut roll, &ctx, vec![enter]);
    assert_eq!(keys(&doc, clip), [(1000, 60), (1480, 64), (1960, 67)]);
    assert_eq!(doc.borrow().history().undo_label(), Some(tr("Strum")));
}

#[test]
fn copy_paste_duplicate_nudge_and_transpose() {
    let (mut roll, doc, clip) = roll(vec![note(0, 60, 0), note(12_000, 64, 0)]);
    assert!(!roll.copy(), "nothing selected");
    roll.selected = vec![0, 1];
    assert!(roll.copy());
    // 120 BPM: a beat is 24 000 samples; pasted at the second bar, the clip grows.
    assert!(roll.paste(&EditSettings::default(), 96_000));
    assert_eq!(
        keys(&doc, clip),
        [(0, 60), (12_000, 64), (96_000, 60), (108_000, 64)]
    );
    assert_eq!(roll.selected, [2, 3], "the pasted notes are selected");
    assert_eq!(
        edit::clip(doc.borrow().project(), clip).unwrap().length,
        192_000
    );
    roll.selected = vec![0, 1];
    assert!(roll.duplicate());
    assert!(keys(&doc, clip).contains(&(24_000, 60)), "a beat later");
    roll.selected = vec![0];
    assert!(roll.nudge(&EditSettings::default(), true));
    assert!(!keys(&doc, clip).contains(&(0, 60)));
    roll.transpose(12);
    assert!(keys(&doc, clip).iter().any(|&(_, k)| k == 72));
    roll.selected = vec![0];
    assert!(roll.cut());
    assert_eq!(roll.clipboard.len(), 1);
}

#[test]
#[ignore = "timing, run in release"]
fn thousands_of_notes_draw_quickly() {
    let notes: Vec<Note> = (0..5_000u64)
        .map(|i| Note {
            start: i * 2_400,
            length: 4_800,
            key: 36 + (i * 7 % 48) as u8,
            velocity: 40 + (i % 80) as u8,
            ..Default::default()
        })
        .collect();
    let (mut roll, _doc, _clip) = roll(notes);
    let ctx = egui::Context::default();
    frame(&mut roll, &ctx, vec![]);
    roll.select_all();
    // Zoomed out: every note in view.
    roll.view.zoom = 20_000.0;
    roll.view.key_height = 6.0;
    let started = std::time::Instant::now();
    for _ in 0..20 {
        frame(
            &mut roll,
            &ctx,
            vec![egui::Event::PointerMoved(pos2(600.0, 400.0))],
        );
    }
    let per_frame = started.elapsed().as_secs_f64() * 1000.0 / 20.0;
    println!("{per_frame:.2} ms a frame");
    assert!(per_frame < 30.0, "{per_frame:.2} ms a frame");
}

#[test]
fn a_rubber_band_past_the_edge_scrolls_and_keeps_its_corner() {
    let (mut roll, _doc, _clip) = roll(vec![note(2_400, 60, 0), note(0, 60, 0)]);
    let ctx = egui::Context::default();
    let f = framed(&mut roll, &ctx);
    // The second note just past the right edge.
    let far = f.t(f.area.right() + 150.0);
    roll.commit(
        "Setup",
        vec![
            note(2_400, 60, 0),
            Note {
                start: far,
                ..note(0, 60, 0)
            },
        ],
        vec![0, 0],
    );
    let f = framed(&mut roll, &ctx);
    let y = f.y(60).unwrap();
    let from = pos2(f.x(1_000), y - 30.0);
    let out = pos2(f.area.right() + 60.0, y + 30.0);
    frame(&mut roll, &ctx, vec![egui::Event::PointerMoved(from)]);
    frame(&mut roll, &ctx, vec![button(from, true)]);
    for _ in 0..40 {
        frame(&mut roll, &ctx, vec![egui::Event::PointerMoved(out)]);
    }
    assert!(roll.view.scroll > 0.0, "the view followed");
    assert_eq!(roll.selected.len(), 2, "both notes, the first one kept");
    frame(&mut roll, &ctx, vec![button(out, false)]);
    roll.view.scroll = -5_000.0;
    roll.view.clamp(400.0);
    assert_eq!(roll.view.scroll, 0.0, "nothing before the start");
}

#[test]
fn every_transform_panel_and_lane_draws_and_applies() {
    let notes: Vec<Note> = (0..16u64)
        .map(|i| {
            let mut n = note(i * 6_000 + 300, 60 + (i % 7) as u8, 0);
            n.velocity = 40 + (i * 5) as u8;
            n
        })
        .collect();
    let (mut roll, doc, clip) = roll(notes);
    let ctx = egui::Context::default();
    framed(&mut roll, &ctx);
    roll.select_all();
    for transform in panel::Transform::all(6_000.0) {
        let base = notes_of(&doc, clip);
        roll.pending = Some(Pending {
            transform,
            chosen: (0..base.len()).collect(),
            owner: vec![0; base.len()],
            base,
        });
        frame(&mut roll, &ctx, vec![]);
        frame(&mut roll, &ctx, vec![]);
        roll.pending = None;
    }
    for (lane, _) in Lane::all() {
        roll.lane = lane;
        roll.selected = vec![0];
        frame(&mut roll, &ctx, vec![]);
        roll.select_all();
        frame(&mut roll, &ctx, vec![]);
    }
    // Each tool over the notes, then over the lane.
    let area = roll.area;
    for tool in Tool::ALL {
        roll.tool = tool;
        roll.lane = Lane::Velocity;
        let from = area.left_top() + vec2(area.width() * 0.3, area.height() * 0.3);
        drag(&mut roll, &ctx, from, from + vec2(120.0, 40.0));
        let lane = egui::pos2(area.left() + area.width() * 0.4, area.bottom() - 20.0);
        drag(&mut roll, &ctx, lane, lane + vec2(200.0, -10.0));
    }
    assert!(!notes_of(&doc, clip).is_empty());
}

#[test]
fn the_piano_roll_answers_a_user_trying_everything() {
    let notes: Vec<Note> = (0..24u64)
        .map(|i| note(i * 4_000, 55 + (i % 12) as u8, 0))
        .collect();
    let (mut roll, _doc, _clip) = roll(notes);
    let ctx = egui::Context::default();
    framed(&mut roll, &ctx);
    let area = Rect::from_min_size(egui::Pos2::ZERO, vec2(1400.0, 900.0));
    for (lane, _) in Lane::all().into_iter().step_by(3) {
        roll.lane = lane;
        for events in crate::test_sweep::script(area, 40.0, true) {
            frame(&mut roll, &ctx, events);
        }
    }
}

/// Two tracks, each with a MIDI clip: Keys from 0, Bass from the second bar (96 000).
fn two_tracks(keys: Vec<Note>, bass: Vec<Note>) -> (PianoRoll, Rc<RefCell<Document>>, [ClipId; 2]) {
    let mut p = Project::new("t", 48_000);
    let k = p.add_track("Keys");
    let b = p.add_track("Bass");
    let a = p.add_midi_clip(k, 0, 192_000, keys).unwrap();
    let c = p.add_midi_clip(b, 96_000, 192_000, bass).unwrap();
    let mut doc = Document::new(AppDirs::default());
    doc.history_mut().replace(p, false);
    let doc = Rc::new(RefCell::new(doc));
    let mut roll = PianoRoll::editor(Rc::clone(&doc));
    roll.clips = vec![a, c];
    roll.active = Some(a);
    (roll, doc, [a, c])
}

#[test]
fn notes_of_two_clips_merged_then_split_come_back_the_same() {
    let keys = vec![note(0, 60, 0), note(24_000, 64, 0)];
    let bass = vec![note(0, 36, 0)];
    let (notes, owner) = merge(&[(0, &keys), (96_000, &bass)]);
    assert_eq!(owner, [0, 0, 1]);
    assert_eq!(notes[2].start, 96_000, "in project time");
    assert_eq!(split(&notes, &owner, &[0, 96_000]), [keys, bass]);
    // Moved before its clip's start, a note stays at it.
    let early = [Note {
        start: 50_000,
        ..note(0, 36, 0)
    }];
    assert_eq!(split(&early, &[1], &[0, 96_000])[1][0].start, 0);
}

#[test]
fn a_change_run_on_each_part_keeps_every_note_in_its_clip() {
    let notes = [note(0, 60, 0), note(0, 36, 0), note(1000, 64, 0)];
    let owner = [0, 1, 0];
    // A change that adds a note above each chosen one.
    let changed = per_part(&notes, &owner, &[0, 1], |part, chosen| {
        let added: Vec<Note> = chosen
            .iter()
            .map(|&i| Note {
                key: part[i].key + 12,
                ..part[i]
            })
            .collect();
        let first = part.len();
        part.extend(added);
        Some((first..part.len()).collect())
    });
    assert_eq!(changed.owner, [0, 0, 0, 1, 1]);
    let keys: Vec<u8> = changed.notes.iter().map(|n| n.key).collect();
    assert_eq!(keys, [60, 64, 72, 36, 48]);
    assert_eq!(changed.picked, Some(vec![2, 4]));
    assert_eq!(changed.moved, [Some(0), Some(3), Some(1)]);
}

#[test]
fn editing_the_notes_of_two_clips_is_one_undo_step() {
    let (mut roll, doc, [a, b]) = two_tracks(vec![note(0, 60, 0)], vec![note(0, 36, 0)]);
    roll.select_all();
    roll.transpose(2);
    assert_eq!(keys(&doc, a), [(0, 62)]);
    assert_eq!(
        keys(&doc, b),
        [(0, 38)],
        "each note in its own clip, at its own time"
    );
    assert_eq!(roll.selected.len(), 2, "the selection follows");
    doc.borrow_mut().history_mut().undo();
    assert_eq!(keys(&doc, a), [(0, 60)]);
    assert_eq!(keys(&doc, b), [(0, 36)]);
}

#[test]
fn a_note_added_goes_to_the_active_tracks_clip_there_else_the_active_clip() {
    let (roll, _doc, [a, b]) = two_tracks(vec![], vec![]);
    let scene = roll.scene().unwrap();
    assert_eq!(scene.parts[roll.target(&scene, 10)].clip.id, a);
    let mut roll = roll;
    roll.active = Some(b);
    let scene = roll.scene().unwrap();
    assert_eq!(scene.parts[roll.target(&scene, 100_000)].clip.id, b);
    // Before the bass clip, the bass is still the active track: its clip takes it.
    assert_eq!(scene.parts[roll.target(&scene, 10)].clip.id, b);
}

#[test]
fn a_transform_keeps_each_note_in_its_track() {
    let (mut roll, doc, [a, b]) = two_tracks(
        vec![note(1000, 60, 0), note(1000, 64, 0)],
        vec![note(1000, 36, 0)],
    );
    let ctx = egui::Context::default();
    frame(&mut roll, &ctx, vec![]);
    roll.select_all();
    roll.instant(tr("Legato"), |n, c| ops::legato(n, c, 0));
    assert_eq!(notes_of(&doc, a).len(), 2);
    assert_eq!(notes_of(&doc, b).len(), 1);
    // Pasted at the bass clip, in the active track's clip there: the keys'.
    roll.selected = vec![2];
    assert!(roll.copy());
    assert!(roll.paste(&EditSettings::default(), 120_000));
    assert_eq!(notes_of(&doc, a).len(), 3);
    assert_eq!(notes_of(&doc, b).len(), 1);
}
