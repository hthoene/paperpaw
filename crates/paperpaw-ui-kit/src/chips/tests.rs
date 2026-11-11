use super::*;
use alloc::string::ToString;

fn style() -> Style {
    Style {
        corner: 4,
        pill: false,
        ring: false,
        separator_ink: Color::BLACK,
        text: 22.0,
        small: 17.0,
        title: 26.0,
        head: 28.0,
        row: 60,
        header: 56,
        margin: 16,
        gap: 8,
        line: 2,
        dark: false,
        plain: false,
        separators: "core.separators.lines".to_string(),
        ink: Color::BLACK,
        paper: Color::WHITE,
        disabled: Color::gray(160),
        rule: Color::gray(190),
        muted: Color::gray(100),
        note: Color::gray(80),
        heading: 30,
        card: Color::gray(230),
        on_dark_muted: Color::gray(200),
        strong: false,
        rtl: false,
        describing: false,
    }
}

#[test]
fn a_chip_is_seven_tenths_of_a_row_and_a_fingertip_at_least() {
    let style = style();
    assert_eq!(Chips::height(&style, 0), 42);
    // A fingertip taller than that raises it, but never above the row.
    assert_eq!(Chips::height(&style, 50), 50);
    assert_eq!(Chips::height(&style, 90), 60);
}

#[test]
fn chips_go_from_the_left_a_gap_apart_each_its_own_width() {
    let style = style();
    let height = Chips::height(&style, 0);
    let wide = height * 3;
    let area = Rect::new(10, 20, 1000, height + 10);
    let chips = Chips::from_widths(&style, 0, area, &[wide, wide * 2]).unwrap();
    // Centred in the area's height: five pixels above and below.
    assert_eq!(chips.cell(0), Some(Rect::new(10, 25, wide, height)));
    assert_eq!(
        chips.cell(1),
        Some(Rect::new(10 + wide + style.gap, 25, wide * 2, height))
    );
    assert_eq!(chips.cell(2), None);
    assert_eq!(chips.len(), 2);
    // A narrow word's chip is as wide as high.
    let narrow = Chips::from_widths(&style, 0, area, &[1]).unwrap();
    assert_eq!(narrow.cell(0).unwrap().width, height);
}

#[test]
fn a_chip_that_does_not_fit_goes_onto_the_next_row_whole() {
    let style = style();
    let height = Chips::height(&style, 0);
    let wide = height * 2;
    let area = Rect::new(
        0,
        0,
        2 * wide + style.gap + wide / 2,
        2 * height + style.gap,
    );
    let chips = Chips::from_widths(&style, 0, area, &[wide; 4]).unwrap();
    assert_eq!(chips.len(), 4);
    assert_eq!(
        chips.cell(2),
        Some(Rect::new(0, height + style.gap, wide, height))
    );
    assert_eq!(
        chips.cell(3),
        Some(Rect::new(
            wide + style.gap,
            height + style.gap,
            wide,
            height
        ))
    );
    // A word wider than the whole area alone is cut to it.
    let cut = Chips::from_widths(&style, 0, area, &[area.width * 2]).unwrap();
    assert_eq!(cut.cell(0).unwrap().width, area.width);
}

#[test]
fn chips_hit_and_step_within_their_row() {
    let style = style();
    let height = Chips::height(&style, 0);
    let area = Rect::new(0, 0, 1000, height);
    let chips = Chips::from_widths(&style, 0, area, &[height * 2, height * 3, height]).unwrap();
    let second = chips.cell(1).unwrap();
    assert_eq!(chips.hit(Point::new(i32::from(second.x) + 1, 1)), Some(1));
    // Between chips: none.
    assert_eq!(chips.hit(Point::new(i32::from(second.x) - 1, 1)), None);
    assert_eq!(chips.step(0, "core.left"), Some(0));
    assert_eq!(chips.step(0, "core.next"), Some(1));
    assert_eq!(chips.step(2, "core.right"), Some(2));
    assert_eq!(chips.step(2, "core.previous"), Some(1));
    // A focus past the end is brought back among the chips.
    assert_eq!(chips.step(9, "core.left"), Some(2));
    assert_eq!(chips.step(1, "core.up"), None);
    let none = Chips::from_widths(&style, 0, area, &[]).unwrap();
    assert!(none.is_empty());
    assert_eq!(none.step(0, "core.left"), None);
    assert_eq!(none.hit(Point::new(1, 1)), None);
}

#[test]
fn the_focus_frame_is_inside_the_reach() {
    let style = style();
    let height = Chips::height(&style, 0);
    let area = Rect::new(20, 20, 400, height);
    let chips = Chips::from_widths(&style, 0, area, &[height * 2]).unwrap();
    let (cell, reach) = (chips.cell(0).unwrap(), chips.reach(0).unwrap());
    // Two borders out for the frame, a third for its line.
    assert_eq!(reach, cell.grow(6));
    let mut paint = Vec::new();
    chips.frames(&mut paint, 0, Some(0)).unwrap();
    let Some(Paint::Focus(frame, _, Some(_), _)) = paint.last() else {
        panic!("{paint:?}");
    };
    assert_eq!(*frame, cell.grow(4));
    assert!(frame.x >= reach.x && frame.right() <= reach.right());
}

#[test]
fn the_frames_are_the_chosen_one_filled_and_the_focused_one_framed() {
    let style = style();
    let height = Chips::height(&style, 0);
    let area = Rect::new(0, 0, 1000, height);
    let chips = Chips::from_widths(&style, 0, area, &[height * 2, height * 2]).unwrap();
    let (first, second) = (chips.cell(0).unwrap(), chips.cell(1).unwrap());
    let mut paint = Vec::new();
    chips.frames(&mut paint, 1, Some(0)).unwrap();
    let shown = alloc::format!("{paint:?}");
    let expect = [
        // The first, on paper, outlined, with the focus frame round it.
        Paint::Focus(first, 4, None, Color::WHITE),
        Paint::Focus(first, 4, Some(2), Color::BLACK),
        Paint::Focus(first.grow(4), 8, Some(2), Color::BLACK),
        // The second, the chosen one, filled.
        Paint::Focus(second, 4, None, Color::BLACK),
        Paint::Focus(second, 4, Some(2), Color::BLACK),
    ];
    assert_eq!(shown, alloc::format!("{expect:?}"));
    // Nothing chosen (an index past the chips): none is filled.
    let mut paint = Vec::new();
    chips.frames(&mut paint, 9, None).unwrap();
    assert_eq!(paint.len(), 4);
}

#[test]
fn pills_have_round_ends() {
    let mut style = style();
    style.pill = true;
    let height = Chips::height(&style, 0);
    let area = Rect::new(0, 0, 1000, height);
    let chips = Chips::from_widths(&style, 0, area, &[height * 3]).unwrap();
    let mut paint = Vec::new();
    chips.frames(&mut paint, 0, None).unwrap();
    assert!(matches!(paint[0], Paint::Focus(_, r, None, _) if r == u32::from(height) / 2));
}

#[test]
fn more_than_the_most_chips_are_refused_not_cut() {
    let style = style();
    let area = Rect::new(0, 0, 1000, 100);
    assert!(Chips::from_widths(&style, 0, area, &[10; MAX_CHIPS]).is_ok());
    assert_eq!(
        Chips::from_widths(&style, 0, area, &[10; MAX_CHIPS + 1]).unwrap_err(),
        Error::NoSpace
    );
}

#[test]
fn hostile_geometry_does_not_wrap_or_panic() {
    let style = style();
    let area = Rect::new(u16::MAX - 5, u16::MAX - 5, u16::MAX, u16::MAX);
    let chips = Chips::from_widths(&style, u16::MAX, area, &[u16::MAX, 0, 1, u16::MAX]).unwrap();
    assert_eq!(chips.len(), 4);
    let mut paint = Vec::new();
    chips.frames(&mut paint, 0, Some(3)).unwrap();
    assert!(chips.reach(3).is_some());
    let empty = Rect::new(0, 0, 0, 0);
    let chips = Chips::from_widths(&style, 0, empty, &[10, 20]).unwrap();
    assert_eq!(chips.len(), 2);
}

#[test]
fn a_refused_allocation_is_no_memory_and_nothing_else() {
    let style = style();
    let area = Rect::new(0, 0, 400, 80);
    crate::fail_alloc::sweep(|| {
        let chips = Chips::from_widths(&style, 0, area, &[80, 90, 100, 200, 300])?;
        let mut paint = Vec::new();
        chips.frames(&mut paint, 1, Some(2))?;
        Ok(paint.len() + chips.len())
    });
}

#[test]
fn only_the_chips_in_the_band_are_drawn() {
    let style = style();
    let height = Chips::height(&style, 0);
    let wide = height * 2;
    // Two rows of two chips, low enough for the frames round them to be whole.
    let area = Rect::new(0, 100, 2 * wide + style.gap, 2 * height + style.gap);
    let chips = Chips::from_widths(&style, 0, area, &[wide; 4]).unwrap();
    let second_row = chips.cell(2).unwrap();
    let mut all = Vec::new();
    chips.frames(&mut all, 0, None).unwrap();
    assert_eq!(all.len(), 8);
    // A band over the second row only: its two chips, outline and fill each.
    let mut some = Vec::new();
    let band = Rect::new(0, second_row.y + height / 2, area.width, height);
    chips.frames_in(&mut some, 0, None, Some(band)).unwrap();
    assert_eq!(some.len(), 4);
    // A band just above the second row still meets what the focus frame
    // of its chips reaches outside them.
    let mut framed = Vec::new();
    let low = Rect::new(0, second_row.y - 1, area.width, 1);
    chips.frames_in(&mut framed, 0, Some(2), Some(low)).unwrap();
    assert_eq!(framed.len(), 5, "{framed:?}");
    // A band above everything draws nothing.
    let mut none = Vec::new();
    let nowhere = Rect::new(0, area.y + area.height + 100, area.width, 10);
    chips.frames_in(&mut none, 0, None, Some(nowhere)).unwrap();
    assert!(none.is_empty());
}

fn rtl() -> Style {
    Style {
        rtl: true,
        ..style()
    }
}

#[test]
fn where_the_interface_runs_right_to_left_the_chips_run_from_the_right() {
    let style = rtl();
    let height = Chips::height(&style, 0);
    let wide = height * 3;
    let area = Rect::new(10, 20, 1000, height);
    let chips = Chips::from_widths(&style, 0, area, &[wide, wide * 2]).unwrap();
    // The first chip ends at the area's right edge, the second a gap left of it.
    let (first, second) = (chips.cell(0).unwrap(), chips.cell(1).unwrap());
    assert_eq!(first.x + first.width, 1010);
    assert_eq!(second.x + second.width + style.gap, first.x);
    assert_eq!(second.width, wide * 2);
    // Hit-testing goes by where they are drawn.
    assert_eq!(chips.hit(Point::new(i32::from(first.x) + 1, 30)), Some(0));
    // Left goes on to the next chip, right back: the order is mirrored.
    assert_eq!(chips.step(0, "core.left"), Some(1));
    assert_eq!(chips.step(1, "core.right"), Some(0));
    assert_eq!(chips.step(0, "core.next"), Some(1));
    assert_eq!(chips.step(1, "core.previous"), Some(0));
}

#[test]
fn a_row_that_wraps_wraps_from_the_right_too() {
    let style = rtl();
    let height = Chips::height(&style, 0);
    let wide = height * 2;
    let area = Rect::new(
        0,
        0,
        2 * wide + style.gap + wide / 2,
        2 * height + style.gap,
    );
    let chips = Chips::from_widths(&style, 0, area, &[wide; 3]).unwrap();
    let third = chips.cell(2).unwrap();
    assert_eq!(third.y, height + style.gap);
    assert_eq!(third.x + third.width, area.width);
}

#[test]
fn the_mirror_image_of_the_left_to_right_layout() {
    let (ltr, rtl) = (style(), rtl());
    let height = Chips::height(&ltr, 0);
    let area = Rect::new(7, 9, 640, 3 * height);
    let widths = [height * 2, height * 3, height * 2, height * 4, height];
    let (a, b) = (
        Chips::from_widths(&ltr, 0, area, &widths).unwrap(),
        Chips::from_widths(&rtl, 0, area, &widths).unwrap(),
    );
    for i in 0..widths.len() {
        let (x, y) = (a.cell(i).unwrap(), b.cell(i).unwrap());
        assert_eq!((x.y, x.width, x.height), (y.y, y.width, y.height));
        assert_eq!(rtl.flip(area, x), y);
    }
}

#[test]
fn chips_tell_nothing_unless_somebody_asked() {
    let style = style();
    assert!(!style.describing);
    let height = Chips::height(&style, 0);
    let area = Rect::new(0, 0, 400, height);
    let chips = Chips::from_widths(&style, 0, area, &[height * 2, height * 2]).unwrap();
    let mut paint = Vec::new();
    chips.frames(&mut paint, 0, Some(1)).unwrap();
    assert!(paint.iter().all(|op| !matches!(op, Paint::Describe { .. })));
}

#[test]
fn each_chip_tells_it_is_a_button_before_its_frame_where_somebody_asks() {
    let mut style = style();
    style.describing = true;
    let height = Chips::height(&style, 0);
    let area = Rect::new(0, 0, 400, height);
    let mut chips = Chips::from_widths(&style, 0, area, &[height * 2, height * 2]).unwrap();
    chips.set_describing(true);
    let mut paint = Vec::new();
    chips.frames(&mut paint, 0, Some(1)).unwrap();
    let told: Vec<_> = paint
        .iter()
        .filter_map(|op| match op {
            Paint::Describe { role, flags, rect } => Some((*role, flags.bits(), *rect)),
            _ => None,
        })
        .collect();
    assert_eq!(told.len(), 2);
    assert_eq!(told[0].1, 0, "the first has no focus");
    assert_eq!(told[1].1, 1, "the second has it");
    assert!(told.iter().all(|(role, ..)| *role == PartRole::Button));
    // Told before its frame is drawn.
    assert!(matches!(paint[0], Paint::Describe { .. }));
}
