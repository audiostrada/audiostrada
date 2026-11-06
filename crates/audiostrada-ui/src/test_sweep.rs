//! Scripts of a user trying everything, for tests: clicks on a grid, drags from every other
//! point, right clicks followed by a click in the menu they opened, and the wheel.

use eframe::egui::{Event, Key, Modifiers, MouseWheelUnit, PointerButton, Pos2, Rect, pos2, vec2};

fn button(pos: Pos2, button: PointerButton, pressed: bool) -> Event {
    Event::PointerButton {
        pos,
        button,
        pressed,
        modifiers: Modifiers::NONE,
    }
}

fn escape() -> Event {
    Event::Key {
        key: Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    }
}

/// One list of events per frame, over `area` every `step` pixels.
pub(crate) fn script(area: Rect, step: f32, right_clicks: bool) -> Vec<Vec<Event>> {
    let mut frames: Vec<Vec<Event>> = Vec::new();
    let mut n = 0;
    let mut y = area.top() + step / 2.0;
    while y < area.bottom() {
        let mut x = area.left() + step / 2.0;
        while x < area.right() {
            let at = pos2(x, y);
            frames.push(vec![Event::PointerMoved(at)]);
            frames.push(vec![button(at, PointerButton::Primary, true)]);
            if n % 2 == 1 {
                for d in [vec2(4.0, -20.0), vec2(30.0, 25.0)] {
                    frames.push(vec![Event::PointerMoved(at + d)]);
                }
            }
            frames.push(vec![button(at, PointerButton::Primary, false)]);
            if right_clicks && n % 3 == 0 {
                frames.push(vec![button(at, PointerButton::Secondary, true)]);
                frames.push(vec![button(at, PointerButton::Secondary, false)]);
                frames.push(Vec::new());
                // An item of the menu, just below and right of the click.
                let item = at + vec2(30.0, 14.0 + (n % 5) as f32 * 22.0);
                frames.push(vec![Event::PointerMoved(item)]);
                frames.push(vec![button(item, PointerButton::Primary, true)]);
                frames.push(vec![button(item, PointerButton::Primary, false)]);
                frames.push(vec![escape()]);
            }
            if n % 7 == 0 {
                frames.push(vec![Event::MouseWheel {
                    unit: MouseWheelUnit::Line,
                    delta: vec2(0.0, if n % 2 == 0 { 1.0 } else { -1.0 }),
                    phase: eframe::egui::TouchPhase::Move,
                    source: eframe::egui::MouseWheelSource::Wheel,
                    modifiers: Modifiers::NONE,
                }]);
            }
            n += 1;
            x += step;
        }
        y += step;
    }
    frames.push(vec![escape()]);
    frames
}
