//! What the interface-6 `tests/screen.rs` judged by pixels, judged here by the
//! rectangles the screen lays out.
use super::*;

const GRAY4: Panel = Panel {
    levels: 16,
    colour: false,
};

fn fills(ops: &[Paint]) -> Vec<(Rect, Color)> {
    ops.iter()
        .filter_map(|op| match op {
            Paint::Fill(rect, color) => Some((*rect, *color)),
            _ => None,
        })
        .collect()
}

fn setup(state: &State, pages: u32) -> (Layout, Vec<Paint>) {
    let metrics = Metrics(220);
    let layout = Layout::new(480, 800, metrics);
    let ops = shapes(&layout, metrics, GRAY4, state, pages).unwrap();
    (layout, ops)
}

#[test]
fn layout_fits_small_and_large_screens() {
    for (w, h, dpi) in [(480, 800, 220), (1264, 1680, 300), (300, 400, 167)] {
        let l = Layout::new(w, h, Metrics(dpi));
        for rect in [l.header, l.ramp, l.progress].iter().chain(&l.boxes) {
            assert!(rect.right() <= u32::from(w) && rect.bottom() <= u32::from(h));
        }
    }
}

#[test]
fn the_ramp_has_a_stripe_for_each_gray_level() {
    let (layout, ops) = setup(&State::start(), 20);
    let stripes: Vec<_> = fills(&ops)
        .into_iter()
        .filter(|(rect, _)| rect.y == layout.ramp.y && rect.height == layout.ramp.height)
        .collect();
    assert_eq!(stripes.len(), 16);
    assert_eq!(stripes[0].1, Color::gray(0));
    assert_eq!(stripes[15].1, Color::gray(255));
    // A panel with two levels shows two stripes; more than 16 stay at 16.
    let metrics = Metrics(220);
    for (levels, expected) in [(2, 2), (4, 4), (256, 16), (0, 2)] {
        let panel = Panel {
            levels,
            colour: false,
        };
        let ops = shapes(&layout, metrics, panel, &State::start(), 20).unwrap();
        let count = fills(&ops)
            .iter()
            .filter(|(rect, _)| rect.y == layout.ramp.y && rect.height == layout.ramp.height)
            .count();
        assert_eq!(count, expected, "{levels} levels");
    }
}

#[test]
fn a_colour_panel_adds_the_six_hues_under_the_grays() {
    let metrics = Metrics(220);
    let layout = Layout::new(480, 800, metrics);
    let panel = Panel {
        levels: 16,
        colour: true,
    };
    let ops = shapes(&layout, metrics, panel, &State::start(), 20).unwrap();
    let hues = fills(&ops)
        .into_iter()
        .filter(|(rect, color)| {
            rect.y > layout.ramp.y
                && rect.y < layout.ramp.y + layout.ramp.height
                && *color != Color::BLACK
                && *color != Color::WHITE
                && !(color.r == color.g && color.g == color.b)
        })
        .count();
    assert_eq!(hues, 6);
}

#[test]
fn a_checked_box_is_filled_and_the_focus_has_a_thicker_border() {
    let mut state = State::start();
    let (layout, plain) = setup(&state, 20);
    let inner = |ops: &[Paint], index: usize| {
        let b = layout.boxes[index];
        fills(ops).iter().any(|(rect, color)| {
            *color == Color::gray(64)
                && b.contains(Point::new(i32::from(rect.x), i32::from(rect.y)))
        })
    };
    assert!(!inner(&plain, 0));
    state.checked[0] = true;
    state.focus = 2;
    let (_, checked) = setup(&state, 20);
    assert!(inner(&checked, 0));
    assert!(!inner(&checked, 1));
    // Border thickness: the focused box's top edge is thicker.
    let top = |ops: &[Paint], index: usize| {
        let b = layout.boxes[index];
        fills(ops)
            .into_iter()
            .find(|(rect, color)| {
                *color == Color::BLACK && rect.x == b.x && rect.y == b.y && rect.width == b.width
            })
            .map(|(rect, _)| rect.height)
            .unwrap()
    };
    assert!(top(&checked, 2) > top(&checked, 1));
}

#[test]
fn the_progress_bar_follows_the_page_and_stops_at_the_ends() {
    let width = |state: &State, pages| {
        let (layout, ops) = setup(state, pages);
        let p = layout.progress;
        let border = Metrics(220).tenth_mm(3) as u16;
        fills(&ops)
            .into_iter()
            .filter(|(rect, color)| {
                *color == Color::BLACK
                    && rect.x == p.x + border
                    && rect.y == p.y + border
                    && rect.height == p.height - 2 * border
            })
            .map(|(rect, _)| u32::from(rect.width))
            .max()
            .unwrap_or(0)
    };
    let mut state = State::start();
    assert_eq!(width(&state, 5), 0, "first page: empty");
    state.page = 2;
    let half = width(&state, 5);
    state.page = 4;
    let full = width(&state, 5);
    assert!(half > 0 && full > half);
    assert_eq!(full / 2, half, "page 3 of 5 is half way");
    // More pages shown than there are: the last one, not beyond.
    state.page = 99;
    assert_eq!(width(&state, 5), full);
    // One page: the bar is full.
    state.page = 0;
    assert_eq!(width(&state, 1), full);
}

#[test]
fn the_page_shown_is_within_the_pages() {
    let state = State {
        page: 7,
        ..State::start()
    };
    assert_eq!(state.shown(3), 2);
    assert_eq!(state.shown(100), 7);
    assert_eq!(state.shown(1), 0);
}

#[test]
fn focus_steps_through_the_boxes() {
    assert_eq!(move_focus(0, "core.right"), Some(1));
    assert_eq!(move_focus(3, "core.right"), Some(0));
    assert_eq!(move_focus(0, "core.left"), Some(3));
    assert_eq!(move_focus(1, "core.down"), Some(3));
    assert_eq!(move_focus(3, "core.up"), Some(1));
    assert_eq!(move_focus(0, "core.back"), None);
}
