use super::*;

fn items(spec: &str) -> Vec<Item> {
    spec.chars()
        .map(|c| {
            if c == 'h' {
                Item::heading("h").unwrap()
            } else {
                Item::row("r", "").unwrap()
            }
        })
        .collect()
}

fn style() -> Style {
    Style {
        corner: 4,
        pill: false,
        ring: false,
        separator_ink: paperpaw_sdk::Color::BLACK,
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
        separators: alloc::string::ToString::to_string("core.separators.lines"),
        ink: paperpaw_sdk::Color::BLACK,
        paper: paperpaw_sdk::Color::WHITE,
        disabled: paperpaw_sdk::Color::gray(160),
        rule: paperpaw_sdk::Color::gray(190),
        muted: paperpaw_sdk::Color::gray(100),
        note: paperpaw_sdk::Color::gray(80),
        heading: 30,
        card: paperpaw_sdk::Color::gray(230),
        on_dark_muted: paperpaw_sdk::Color::gray(200),
        strong: false,
        rtl: false,
        describing: false,
    }
}

#[test]
fn a_field_row_is_a_selectable_row_that_looks_like_a_field() {
    let Item::Row(row) = Item::field("Search").unwrap() else {
        panic!("a row")
    };
    assert!(row.field && row.value.is_empty() && row.note.is_empty());
    assert_eq!(row.title, "Search");
    assert!(Item::field("Search").unwrap().selectable());
    assert!(!matches!(Item::row("Search", "").unwrap(), Item::Row(r) if r.field));
}

#[test]
fn a_whole_row_is_a_row_with_the_flag_and_no_other_change() {
    let Item::Row(whole) = Item::whole("Title", "Keep", "A long note").unwrap() else {
        panic!("a row")
    };
    let Item::Row(noted) = Item::noted("Title", "Keep", "A long note").unwrap() else {
        panic!("a row")
    };
    assert!(whole.whole && !noted.whole);
    assert_eq!(
        RowItem {
            whole: false,
            ..whole
        },
        noted
    );
    const { assert!(WHOLE_NOTE_LINES > NOTE_LINES && WHOLE_TITLE_LINES == 2) };
}

#[test]
fn a_title_that_fits_stays_on_one_line_and_a_long_one_wraps_to_two_at_most() {
    // Ten units a letter.
    let measure = |line: &str| Ok(line.chars().count() as f32 * 10.0);
    assert!(
        wrap_title(measure, "Short title", 200.0)
            .unwrap()
            .is_empty()
    );
    let long = "Spanish: cards missing from the file, kept or removed";
    let lines = wrap_title(measure, long, 200.0).unwrap();
    assert_eq!(lines.len(), WHOLE_TITLE_LINES);
    assert!(lines.iter().all(|line| measure(line).unwrap() <= 200.0));
    assert!(lines[0].starts_with("Spanish: cards"));
    // More than two lines of text: the second ends in an ellipsis.
    let longer = "one two three four five six seven eight nine ten eleven twelve thirteen";
    let cut = wrap_title(measure, longer, 200.0).unwrap();
    assert_eq!(cut.len(), 2);
    assert!(cut[1].ends_with('…'));
}

#[test]
fn a_field_is_outlined_inside_its_row_with_the_corner_of_the_style() {
    let style = style();
    let (outline, radius, line) = field_outline(&style, Rect::new(0, 100, 480, 60));
    assert_eq!(outline, Rect::new(8, 102, 464, 56));
    assert_eq!((radius, line), (4, 2));
    let pill = Style {
        pill: true,
        ..style
    };
    assert_eq!(field_outline(&pill, Rect::new(0, 100, 480, 60)).1, 28);
}

fn action(action: &str) -> Input<'_> {
    Input::Action { now_ms: 0, action }
}

#[test]
fn a_value_is_replaced_in_place_and_a_heading_has_none() {
    let mut list = ItemList::new(items("hrr"));
    list.set_focus(2);
    assert!(list.set_value(1, "42 %".into()));
    assert!(!list.set_value(0, "x".into()), "a heading has no value");
    assert!(!list.set_value(9, "x".into()));
    assert_eq!(list.focus(), 2);
    let Item::Row(row) = &list.items()[1] else {
        panic!("a row")
    };
    assert_eq!((row.title.as_str(), row.value.as_str()), ("r", "42 %"));
}

#[test]
fn a_wiped_list_is_empty_and_its_focus_is_back_at_the_start() {
    let mut list = ItemList::new(items("hrrr"));
    list.set_focus(3);
    list.wipe();
    assert!(list.items().is_empty());
    assert_eq!(list.focus(), 0);
}

#[test]
fn the_focus_starts_on_the_first_row_and_skips_headings() {
    let list = ItemList::new(items("hrrhr"));
    assert_eq!(list.focus(), 1);
    assert_eq!(list.decide(&action("core.down")), ItemStep::Focus(2));
    let mut list = list;
    list.set_focus(2);
    assert_eq!(list.decide(&action("core.down")), ItemStep::Focus(4));
    assert_eq!(list.decide(&action("core.up")), ItemStep::Focus(1));
    list.set_focus(4);
    assert_eq!(list.decide(&action("core.down")), ItemStep::Focus(4));
}

#[test]
fn faint_rows_are_passed_by_the_focus_and_do_nothing_when_tapped() {
    let mut spec = items("rrr");
    if let Item::Row(row) = &mut spec[1] {
        row.disabled = true;
    }
    let mut list = ItemList::new(spec);
    assert_eq!(list.decide(&action("core.down")), ItemStep::Focus(2));
    list.set_focus(2);
    assert_eq!(list.decide(&action("core.up")), ItemStep::Focus(0));
    // The focus never rests on one.
    list.set_focus(1);
    assert_eq!(list.focus(), 0);
    list.hits = Vec::from([(Rect::new(0, 0, 10, 10), 1)]);
    list.focus = 2;
    assert_eq!(
        list.decide(&Input::Gesture {
            now_ms: 0,
            position: Point::new(1, 1),
            kind: InputGestureKind::Tap,
        }),
        ItemStep::Focus(2)
    );
}

#[test]
fn replacing_keeps_the_focus_near_where_it_was() {
    let mut list = ItemList::new(items("hrrrr"));
    list.set_focus(4);
    list.replace(items("hrr"));
    assert_eq!(list.focus(), 2);
    list.replace(items("rrrh"));
    assert_eq!(list.focus(), 2);
    list.replace(Vec::new());
    assert_eq!(list.focus(), 0);
}

#[test]
fn a_heading_never_ends_a_page_alone() {
    let list = items("rrhrr");
    // Three rows fit; the heading would be last of the first page.
    let starts = paginate(&list, &[10, 10, 10, 10, 10], 30).unwrap();
    assert_eq!(starts, [0, 2]);
    // A page that ends before the heading is left as it is.
    let starts = paginate(&list, &[10, 10, 10, 10, 10], 20).unwrap();
    assert_eq!(starts, [0, 2, 4]);
}

#[test]
fn pagination_survives_every_refused_allocation() {
    let list = items("rrhrrrhrr");
    crate::fail_alloc::sweep(|| paginate(&list, &[10; 9], 20).map(|starts| starts.len()));
}

#[test]
fn pages_follow_the_heights() {
    let list = items("rrrr");
    assert_eq!(paginate(&list, &[10, 25, 10, 10], 30).unwrap(), [0, 1, 2]);
    assert_eq!(paginate(&items("rr"), &[40, 40], 30).unwrap(), [0, 1]);
    assert_eq!(paginate(&list, &[40, 40], 30), Err(Error::Invalid));
    assert_eq!(paginate(&[], &[], 30).unwrap(), [0]);
}

#[test]
fn page_actions_go_to_the_first_row_of_the_other_page() {
    let mut list = ItemList::new(items("hrrhrr"));
    list.starts = Vec::from([0, 3]);
    assert_eq!(list.decide(&action("core.next-page")), ItemStep::Focus(4));
    list.set_focus(5);
    assert_eq!(list.decide(&action("core.prev-page")), ItemStep::Focus(1));
    assert_eq!(list.decide(&action("core.confirm")), ItemStep::Chosen(5));
}

#[test]
fn a_long_press_holds_the_row_and_never_the_header() {
    let mut list = ItemList::new(items("hr"));
    list.hits = Vec::from([(Rect::new(0, 40, 100, 40), 1)]);
    let press = |y| Input::Gesture {
        now_ms: 0,
        position: Point::new(5, y),
        kind: InputGestureKind::LongPress,
    };
    assert_eq!(list.decide(&press(45)), ItemStep::Held(1));
    assert_eq!(list.decide(&press(5)), ItemStep::Ignored);
}

#[test]
fn taps_choose_rows_and_headings_are_not_hit() {
    let mut list = ItemList::new(items("hrr"));
    list.hits = Vec::from([
        (Rect::new(0, 40, 100, 40), 1),
        (Rect::new(0, 80, 100, 40), 2),
    ]);
    let tap = |y| Input::Gesture {
        now_ms: 0,
        position: Point::new(5, y),
        kind: InputGestureKind::Tap,
    };
    assert_eq!(list.decide(&tap(45)), ItemStep::Chosen(1));
    assert_eq!(list.decide(&tap(100)), ItemStep::Chosen(2));
    assert_eq!(list.decide(&tap(10)), ItemStep::Ignored);
}
