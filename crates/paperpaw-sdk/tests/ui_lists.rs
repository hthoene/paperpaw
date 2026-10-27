//! Lists as every screen uses them, by buttons and by touch alike:
//! [`ListView`] (pages of equal rows, or a grid), [`Rows`] (rows of
//! different heights with section headings) and [`Sections`] (the same
//! for lists of any length). Focus moves wrap or stop as documented and
//! never land on a heading, every row is hit where it is drawn and only
//! there, pages split the list without losing or repeating an entry,
//! and nothing is drawn outside the list's area.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod support;

use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::ui::sections::{Item, Section, Sections};
use paperpaw_sdk::ui::{ListChange, ListView, Paging, Rows, UiStyle, list_step, step_focus};
use paperpaw_sdk::{Cx, Id, Output, Point, Rect, RefreshMode};
use proptest::prelude::*;

use support::{Device, Harness, drawn_outside, frame, middle};

/// Sizes for tests: no fonts needed.
fn style() -> UiStyle {
    UiStyle::PLAIN.with_corner(6)
}

/// Every point of `rect` worth trying: its corners and its middle.
fn probes(rect: Rect) -> [Point; 5] {
    let (x0, y0) = (i32::from(rect.x), i32::from(rect.y));
    let (x1, y1) = (rect.right() as i32 - 1, rect.bottom() as i32 - 1);
    [
        Point::new(x0, y0),
        Point::new(x1, y0),
        Point::new(x0, y1),
        Point::new(x1, y1),
        middle(rect),
    ]
}

fn overlap(a: Rect, b: Rect) -> bool {
    a.intersection(&b).is_some_and(|i| !i.is_empty())
}

fn inside(inner: Rect, outer: Rect) -> bool {
    inner.x >= outer.x
        && inner.y >= outer.y
        && inner.right() <= outer.right()
        && inner.bottom() <= outer.bottom()
}

// ---- step_focus and list_step -----------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(256)))]

    #[test]
    fn single_steps_wrap_and_larger_ones_stop_at_the_ends(
        len in 0usize..50,
        focus in 0usize..60,
        delta in -60isize..60,
    ) {
        if len == 0 {
            prop_assert_eq!(step_focus(focus, delta, len), 0);
            return Ok(());
        }
        let focus = focus % len;
        let to = step_focus(focus, delta, len);
        prop_assert!(to < len);
        let target = focus as isize + delta;
        let expected = match delta {
            1 | -1 => target.rem_euclid(len as isize) as usize,
            _ => target.clamp(0, len as isize - 1) as usize,
        };
        prop_assert_eq!(to, expected);
    }
}

#[test]
fn every_direction_button_moves_a_list_one_step_and_page_buttons_a_page() {
    for back in [actions::UP, actions::LEFT, actions::PREVIOUS] {
        assert_eq!(list_step(&back, 7), Some(-1), "{back}");
    }
    for on in [actions::DOWN, actions::RIGHT, actions::NEXT] {
        assert_eq!(list_step(&on, 7), Some(1), "{on}");
    }
    assert_eq!(list_step(&actions::NEXT_PAGE, 7), Some(7));
    assert_eq!(list_step(&actions::PREV_PAGE, 7), Some(-7));
    assert_eq!(list_step(&actions::CONFIRM, 7), None);
    assert_eq!(list_step(&Id::new_static("my.other"), 7), None);
}

// ---- ListView ---------------------------------------------------------

fn list_strategy() -> impl Strategy<Value = (ListView, usize)> {
    (
        0u16..40,
        0u16..300,
        100u16..480,
        40u16..700,
        20u32..120,
        1u32..5,
        0usize..80,
    )
        .prop_map(|(x, y, width, height, row, columns, len)| {
            // At least a row high: a list in less shows one row all the
            // same, past its area.
            let height = height.max(row as u16);
            let list = ListView::new(Rect::new(x, y, width, height), row).columns(columns);
            (list, len)
        })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(128)))]

    /// Each visible item is hit at its corners and middle, the items of
    /// a page never overlap and lie in the list's area, and points below
    /// the last item or outside the area hit nothing.
    #[test]
    fn every_visible_item_is_hit_where_it_is_drawn(
        (mut list, len) in list_strategy(),
        focus in 0usize..80,
    ) {
        list.set_focus(focus.min(len.saturating_sub(1)));
        let first = list.page() * list.rows();
        let shown: Vec<(usize, Rect)> = (first..len)
            .take(list.rows())
            .enumerate()
            .map(|(slot, index)| (index, list.row_rect(slot)))
            .collect();
        for (n, &(index, rect)) in shown.iter().enumerate() {
            prop_assert!(inside(rect, list.area()), "{rect:?} in {:?}", list.area());
            for point in probes(rect) {
                prop_assert_eq!(list.hit(point, len), Some(index), "{:?}", point);
            }
            for &(_, other) in &shown[n + 1..] {
                prop_assert!(!overlap(rect, other), "{rect:?} and {other:?}");
            }
        }
        // Outside the area: nothing.
        let a = list.area();
        for point in [
            Point::new(i32::from(a.x) - 1, i32::from(a.y)),
            Point::new(i32::from(a.x), i32::from(a.y) - 1),
            Point::new(a.right() as i32, i32::from(a.y)),
            Point::new(i32::from(a.x), a.bottom() as i32),
        ] {
            prop_assert_eq!(list.hit(point, len), None);
        }
        // Below the last item of a page that is not full: nothing.
        if shown.len() < list.rows() && list.column_count() == 1 {
            let below = list.row_rect(shown.len());
            if inside(below, list.area()) {
                prop_assert_eq!(list.hit(middle(below), len), None);
            }
        }
    }

    /// Buttons: one step wraps round, a page stops at the end, the change
    /// says whether the page turned, and the focus always stays in the
    /// list.
    #[test]
    fn focus_moves_by_buttons_keep_to_the_list(
        (mut list, len) in list_strategy(),
        moves in proptest::collection::vec(0usize..6, 1..40),
    ) {
        let buttons = [
            actions::UP,
            actions::DOWN,
            actions::NEXT,
            actions::PREVIOUS,
            actions::NEXT_PAGE,
            actions::PREV_PAGE,
        ];
        for m in moves {
            let before = (list.focus(), list.page());
            let change = list.navigate(&buttons[m], len).unwrap();
            if len == 0 {
                prop_assert_eq!(change, ListChange::None);
                continue;
            }
            prop_assert!(list.focus() < len);
            let delta = list_step(&buttons[m], list.rows() as isize).unwrap();
            prop_assert_eq!(list.focus(), step_focus(before.0, delta, len));
            let expected = if list.focus() == before.0 {
                Kind::None
            } else if list.page() == before.1 {
                Kind::Focus
            } else {
                Kind::Page
            };
            prop_assert_eq!(kind(change), expected);
            prop_assert_eq!(list.page(), list.focus() / list.rows());
            prop_assert!(list.page() < list.pages(len));
        }
    }

    /// The focus bar, the lines between rows and what the rows draw stay
    /// in the list's area; the focused row alone is drawn inverted.
    #[test]
    fn a_list_draws_inside_its_area(
        (mut list, len) in list_strategy(),
        focus in 0usize..80,
    ) {
        list.set_focus(focus.min(len.saturating_sub(1)));
        let mut frame = frame(560, 1000);
        let mut focused_rows = Vec::new();
        let harness = support::Harness::new(support::Device::BUTTONS);
        let env = harness.env();
        list.draw(&mut paperpaw_sdk::ui::Ui::new(&mut frame.canvas(), &env, &style()), len, |_, index, rect, focused| {
            assert!(inside(rect, list.area()));
            if focused {
                focused_rows.push((index, rect));
            }
        });
        prop_assert_eq!(drawn_outside(&mut frame, list.area()), None);
        if len > 0 {
            prop_assert_eq!(focused_rows.len(), 1);
            prop_assert_eq!(focused_rows[0].0, list.focus());
        } else {
            prop_assert!(focused_rows.is_empty());
        }
    }
}

#[test]
fn an_empty_list_has_one_page_and_nothing_to_focus_or_hit() {
    let mut list = ListView::new(Rect::new(0, 0, 400, 300), 50);
    assert_eq!(list.pages(0), 1);
    assert_eq!(list.paging(0), Some(Paging::new(0, 1)));
    for delta in [-1, 1, -6, 6] {
        assert_eq!(list.move_focus(delta, 0), ListChange::None);
    }
    assert_eq!(list.focus(), 0);
    assert_eq!(list.hit(Point::new(10, 10), 0), None);
    let mut rows = 0;
    let harness = support::Harness::new(support::Device::BUTTONS);
    let env = harness.env();
    list.draw(
        &mut paperpaw_sdk::ui::Ui::new(&mut frame(400, 300).canvas(), &env, &style()),
        0,
        |_, _, _, _| rows += 1,
    );
    assert_eq!(rows, 0);
}

#[test]
fn a_hidden_focus_shows_on_the_first_step_and_stays_hidden_while_paging() {
    let h = Harness::new(Device::TOUCH);
    let env = h.env();
    let mut list = ListView::new(Rect::new(0, 0, 400, 300), 100).focus_on_keys(&env);
    assert!(!list.shown(), "touch screens hide it at first");
    // Paging (a swipe, a tap on an arrow) keeps it hidden.
    assert_eq!(list.move_focus(3, 10), ListChange::Page);
    assert_eq!(list.focus(), 3);
    assert!(!list.shown());
    // The first step shows where it is, without moving it.
    assert!(list.move_focus(1, 10).is_focus());
    assert!(list.shown());
    assert_eq!(list.focus(), 3);
    assert!(list.move_focus(1, 10).is_focus());
    assert_eq!(list.focus(), 4);
    // A hidden focus is not drawn.
    let mut hidden = ListView::new(Rect::new(0, 0, 400, 300), 100).focus_on_keys(&env);
    hidden.set_focus(1);
    let mut focused = 0;
    hidden.draw(
        &mut paperpaw_sdk::ui::Ui::new(&mut frame(400, 300).canvas(), &env, &style()),
        3,
        |_, _, _, f| focused += usize::from(f),
    );
    assert_eq!(focused, 0);

    // Without touch it shows from the start.
    let x4 = Harness::new(Device::BUTTONS);
    let list = ListView::new(Rect::new(0, 0, 400, 300), 100).focus_on_keys(&x4.env());
    assert!(list.shown());
}

#[test]
fn a_grid_hits_the_column_under_the_finger_and_its_last_column_takes_the_rest() {
    // 3 columns of 33 in 100: the last pixel column belongs to the last.
    let list = ListView::new(Rect::new(0, 0, 100, 100), 50).columns(3);
    assert_eq!(list.rows(), 6);
    assert_eq!(list.hit(Point::new(0, 0), 6), Some(0));
    assert_eq!(list.hit(Point::new(33, 0), 6), Some(1));
    assert_eq!(list.hit(Point::new(99, 0), 6), Some(2));
    assert_eq!(list.hit(Point::new(99, 99), 6), Some(5));
    // Only five items: the sixth place hits nothing.
    assert_eq!(list.hit(Point::new(99, 99), 5), None);
}

// ---- Rows -------------------------------------------------------------

/// Sections, each with an optional heading (5 to 15 high) and 1 to 5
/// entries (10 to 40 high), in an area 100 high.
fn rows_strategy() -> impl Strategy<Value = (Vec<u16>, Vec<bool>)> {
    proptest::collection::vec(
        (
            proptest::option::of(5u16..15),
            proptest::collection::vec(10u16..40, 1..5),
        ),
        0..8,
    )
    .prop_map(|sections| {
        let mut heights = Vec::new();
        let mut headings = Vec::new();
        for (heading, entries) in sections {
            if let Some(h) = heading {
                heights.push(h);
                headings.push(true);
            }
            for e in entries {
                heights.push(e);
                headings.push(false);
            }
        }
        (heights, headings)
    })
}

const ROWS_AREA: Rect = Rect::new(10, 50, 300, 100);

/// The pages of `rows`, each as what it shows, turned through from the
/// first.
fn rows_pages(rows: &mut Rows) -> Vec<Vec<(usize, Rect)>> {
    while rows.turn(false) != ListChange::None {}
    let mut pages = vec![rows.visible()];
    while rows.turn(true) != ListChange::None {
        pages.push(rows.visible());
    }
    pages
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(192)))]

    /// Turned through, the pages show every row once (headings of
    /// sections going on again at the top), each within the area, rows
    /// stacked without gaps, no page ending on a heading.
    #[test]
    fn rows_are_split_into_pages_without_losing_any(
        (heights, headings) in rows_strategy(),
    ) {
        let mut rows = Rows::new(ROWS_AREA, heights.clone(), headings.clone());
        let entries = headings.iter().filter(|h| !**h).count();
        if entries == 0 {
            prop_assert!(rows.visible().iter().all(|(i, _)| headings[*i]));
            return Ok(());
        }
        let pages = rows_pages(&mut rows);
        prop_assert_eq!(pages.len(), rows.paging().pages);
        let mut seen = Vec::new();
        for page in &pages {
            let mut y = ROWS_AREA.y;
            for (n, &(index, rect)) in page.iter().enumerate() {
                prop_assert_eq!(rect, Rect::new(ROWS_AREA.x, y, ROWS_AREA.width, heights[index]));
                prop_assert!(inside(rect, ROWS_AREA), "{rect:?}");
                y += rect.height;
                // A repeated heading comes first only.
                if n == 0 && headings[index] && seen.contains(&index) {
                    continue;
                }
                seen.push(index);
            }
            let last = page.last().unwrap().0;
            prop_assert!(!headings[last], "a page ends on heading {last}");
        }
        prop_assert_eq!(seen, (0..heights.len()).collect::<Vec<_>>());
    }

    /// Stepping forward goes through every entry once, in order, never
    /// onto a heading, and back to the first; backward the other way;
    /// the change says whether the page turned.
    #[test]
    fn steps_visit_every_entry_and_never_a_heading(
        (heights, headings) in rows_strategy(),
        forward in any::<bool>(),
    ) {
        let mut rows = Rows::new(ROWS_AREA, heights, headings.clone());
        let entries: Vec<usize> = (0..headings.len()).filter(|&i| !headings[i]).collect();
        if entries.is_empty() {
            prop_assert_eq!(rows.step(forward), ListChange::None);
            return Ok(());
        }
        prop_assert_eq!(rows.focus(), entries[0]);
        let mut visited = vec![rows.focus()];
        for _ in 0..entries.len() {
            let page = rows.paging().page;
            let change = rows.step(forward);
            prop_assert!(!headings[rows.focus()]);
            let expected = if entries.len() == 1 {
                Kind::None
            } else if rows.paging().page == page {
                Kind::Focus
            } else {
                Kind::Page
            };
            prop_assert_eq!(kind(change), expected);
            visited.push(rows.focus());
        }
        prop_assert_eq!(visited.first(), visited.last(), "back where it began");
        visited.pop();
        let mut expected = entries.clone();
        if !forward {
            expected[1..].reverse();
        }
        prop_assert_eq!(visited, expected);
    }

    /// A tap on an entry's row hits that entry; a tap on a heading hits
    /// nothing; the focused entry is drawn on the visible page.
    #[test]
    fn taps_hit_the_entry_drawn_there_and_headings_none(
        (heights, headings) in rows_strategy(),
        steps in 0usize..30,
    ) {
        let mut rows = Rows::new(ROWS_AREA, heights, headings.clone());
        for _ in 0..steps {
            rows.step(true);
        }
        let visible = rows.visible();
        if headings.iter().any(|h| !h) {
            prop_assert!(visible.iter().any(|(i, _)| *i == rows.focus()), "the focus shows");
        }
        for (index, rect) in visible {
            let expected = (!headings[index]).then_some(index);
            for point in probes(rect) {
                prop_assert_eq!(rows.hit(point), expected);
            }
        }
        let below = Point::new(i32::from(ROWS_AREA.x) + 5, ROWS_AREA.bottom() as i32);
        prop_assert_eq!(rows.hit(below), None);
    }

    /// Drawn, rows keep to the area, and only the focused entry (never a
    /// heading) is drawn focused.
    #[test]
    fn rows_draw_inside_their_area(
        (heights, headings) in rows_strategy(),
        steps in 0usize..10,
    ) {
        let mut rows = Rows::new(ROWS_AREA, heights, headings.clone());
        for _ in 0..steps {
            rows.step(true);
        }
        let mut frame = frame(400, 300);
        let mut focused = Vec::new();
        let harness = support::Harness::new(support::Device::BUTTONS);
        let env = harness.env();
        rows.draw(&mut paperpaw_sdk::ui::Ui::new(&mut frame.canvas(), &env, &style()), true, |_, index, _, f| {
            if f {
                focused.push(index);
            }
        });
        prop_assert_eq!(drawn_outside(&mut frame, ROWS_AREA), None);
        if headings.iter().any(|h| !h) {
            prop_assert_eq!(focused, vec![rows.focus()]);
        }
    }
}

#[test]
fn rows_without_any_go_nowhere() {
    let mut rows = Rows::new(ROWS_AREA, Vec::new(), Vec::new());
    assert!(rows.is_empty());
    assert_eq!(rows.paging(), Paging::new(0, 1));
    assert!(rows.visible().is_empty());
    assert_eq!(rows.step(true), ListChange::None);
    assert_eq!(rows.turn(true), ListChange::None);
    assert_eq!(rows.turn(false), ListChange::None);
    assert_eq!(rows.hit(Point::new(20, 60)), None);
}

// ---- Sections ---------------------------------------------------------

fn sections_strategy() -> impl Strategy<Value = (Vec<Section>, u16, u16, u16)> {
    (
        proptest::collection::vec((any::<bool>(), 0usize..7), 0..8),
        5u16..20,
        10u16..40,
        60u16..200,
    )
        .prop_map(|(sections, heading, row, height)| {
            let sections = sections
                .into_iter()
                .map(|(heading, count)| Section { heading, count })
                .collect();
            (sections, heading, row, height.max(heading + row))
        })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(192)))]

    /// Turned through, the pages hold every entry once and in order, each
    /// page within the area; a section going on over a page break shows
    /// its heading again at the top; entries are hit where drawn.
    #[test]
    fn sections_page_every_entry_once_and_repeat_headings(
        (sections, heading, row, height) in sections_strategy(),
    ) {
        let area = Rect::new(0, 40, 320, height);
        let mut list = Sections::new(area, heading, row, sections.clone());
        let len = list.len();
        prop_assert_eq!(len, sections.iter().map(|s| s.count).sum::<usize>());
        if len == 0 {
            prop_assert!(list.visible().is_empty());
            prop_assert_eq!(list.turn(true), ListChange::None);
            return Ok(());
        }
        let mut next = 0;
        let mut pages = 0;
        loop {
            pages += 1;
            let entries = list.page_entries();
            prop_assert_eq!(entries.start, next, "pages follow each other");
            prop_assert!(entries.end > entries.start, "every page has an entry");
            next = entries.end;
            let visible = list.visible();
            let mut y = area.y;
            for (n, &(item, rect)) in visible.iter().enumerate() {
                prop_assert_eq!(rect.y, y, "stacked");
                prop_assert!(inside(rect, area), "{rect:?} in {area:?}");
                y += rect.height;
                match item {
                    Item::Entry(e) => {
                        prop_assert!(entries.contains(&e));
                        prop_assert_eq!(list.hit(middle(rect)), Some(e));
                        prop_assert_eq!(list.rect_of(e), Some(rect));
                    }
                    Item::Heading(s) => {
                        prop_assert!(sections[s].heading);
                        prop_assert_eq!(list.hit(middle(rect)), None);
                        // Followed by an entry of its own section.
                        let after = visible.get(n + 1).map(|(item, _)| *item);
                        let Some(Item::Entry(e)) = after else {
                            return Err(TestCaseError::fail("a heading without an entry"));
                        };
                        prop_assert_eq!(list.section_of(e).map(|(s, _)| s), Some(s));
                    }
                }
            }
            // The first entry of a page in the middle of a headed section:
            // its heading above it.
            let (section, within) = list.section_of(entries.start).unwrap();
            if sections[section].heading && within > 0 {
                prop_assert_eq!(visible[0].0, Item::Heading(section));
            }
            if list.turn(true) == ListChange::None {
                break;
            }
            prop_assert_eq!(list.focus(), entries.end, "turned to the first entry");
        }
        prop_assert_eq!(next, len, "the last page ends the list");
        prop_assert_eq!(list.paging(), Paging::new(pages - 1, pages));
        // And back again, page by page.
        for page in (0..pages - 1).rev() {
            prop_assert_ne!(list.turn(false), ListChange::None);
            prop_assert_eq!(list.paging().page, page);
        }
        prop_assert_eq!(list.turn(false), ListChange::None);
    }

    /// Steps go through every entry and round; a focus asked past the
    /// end goes to the last.
    #[test]
    fn section_steps_wrap_and_far_focus_clamps(
        (sections, heading, row, height) in sections_strategy(),
        far in 0usize..100,
    ) {
        let area = Rect::new(0, 0, 320, height);
        let mut list = Sections::new(area, heading, row, sections);
        let len = list.len();
        if len == 0 {
            prop_assert_eq!(list.step(true), ListChange::None);
            return Ok(());
        }
        for n in 1..=len {
            list.step(true);
            prop_assert_eq!(list.focus(), n % len);
            prop_assert!(list.page_entries().contains(&list.focus()), "the focus shows");
        }
        list.step(false);
        prop_assert_eq!(list.focus(), len - 1);
        list.focus_on(len + far);
        prop_assert_eq!(list.focus(), len - 1);
    }

    /// The same list as [`Rows`] and as [`Sections`] (both documented to
    /// page alike) is split into the same pages. Rows know sections by
    /// their headings only: every section but the first has one here.
    #[test]
    fn sections_and_rows_page_alike(
        (mut sections, heading, row, height) in sections_strategy(),
    ) {
        for s in sections.iter_mut().skip(1) {
            s.heading = true;
        }
        let area = Rect::new(0, 0, 320, height);
        let mut list = Sections::new(area, heading, row, sections.clone());
        if list.is_empty() {
            return Ok(());
        }
        // The same as rows: shown sections only (those with entries).
        let mut heights = Vec::new();
        let mut headings = Vec::new();
        for s in sections.iter().filter(|s| s.count > 0) {
            if s.heading {
                heights.push(heading);
                headings.push(true);
            }
            for _ in 0..s.count {
                heights.push(row);
                headings.push(false);
            }
        }
        let mut rows = Rows::new(area, heights, headings.clone());
        let row_pages: Vec<Vec<Rect>> = rows_pages(&mut rows)
            .into_iter()
            .map(|page| page.into_iter().map(|(_, r)| r).collect())
            .collect();
        let mut section_pages = Vec::new();
        loop {
            section_pages.push(list.visible().into_iter().map(|(_, r)| r).collect::<Vec<_>>());
            if list.turn(true) == ListChange::None {
                break;
            }
        }
        prop_assert_eq!(section_pages, row_pages);
    }

    /// Drawn, sections keep to their area; only the focused entry is
    /// drawn focused.
    #[test]
    fn sections_draw_inside_their_area(
        (sections, heading, row, height) in sections_strategy(),
        steps in 0usize..20,
    ) {
        let area = Rect::new(20, 30, 320, height);
        let mut list = Sections::new(area, heading, row, sections);
        for _ in 0..steps {
            list.step(true);
        }
        let mut frame = frame(400, 300);
        let mut focused = Vec::new();
        let harness = support::Harness::new(support::Device::BUTTONS);
        let env = harness.env();
        list.draw(&mut paperpaw_sdk::ui::Ui::new(&mut frame.canvas(), &env, &style()), true, |_, item, _, f| {
            if f {
                focused.push(item);
            }
        });
        prop_assert_eq!(drawn_outside(&mut frame, area), None);
        if !list.is_empty() {
            prop_assert_eq!(focused, vec![Item::Entry(list.focus())]);
        }
    }
}

#[test]
fn a_long_list_turns_to_its_last_page_and_back_to_its_first() {
    // 10 000 entries in sections of 7 under headings.
    let sections = vec![
        Section {
            heading: true,
            count: 7
        };
        1_000
    ];
    let mut list = Sections::new(Rect::new(0, 0, 300, 400), 20, 50, sections);
    list.focus_on(usize::MAX);
    assert_eq!(list.focus(), 6_999);
    assert!(list.paging().is_last());
    assert_eq!(list.page_entries().end, 7_000);
    assert_eq!(list.turn(true), ListChange::None);
    list.step(true);
    assert_eq!(list.focus(), 0, "on from the last: the first");
    assert_eq!(list.paging().page, 0);
}

// ---- What a move asks to be drawn ---------------------------------------

/// What kind of change a move made, whatever rows it concerns.
#[derive(Debug, PartialEq, Eq)]
enum Kind {
    None,
    Focus,
    Page,
}

fn kind(change: ListChange) -> Kind {
    match change {
        ListChange::Focus { .. } => Kind::Focus,
        ListChange::Page => Kind::Page,
        _ => Kind::None,
    }
}

/// What `request` asked for: the area drawn again (`None` for all of the
/// screen) and the refresh.
fn asked(
    h: &Harness,
    request: impl FnOnce(&mut Cx<'_, '_>),
) -> (Option<Rect>, bool, Option<RefreshMode>) {
    let env = h.env();
    let mut output = Output::default();
    request(&mut Cx::new(&env, &mut output));
    (output.redraw_area, output.redraw, output.refresh)
}

/// A step within a page draws the row left and the row come to again,
/// nothing else, refreshed fast (on the X4 a third of the drawing); a
/// new page draws the screen; no change draws nothing.
#[test]
fn a_step_redraws_only_the_two_rows_it_concerns() {
    let h = Harness::new(Device::BUTTONS);
    // ListView: rows 100 high.
    let mut list = ListView::new(Rect::new(0, 100, 400, 500), 100);
    let change = list.move_focus(2, 12);
    assert!(change.is_focus());
    let (area, all, refresh) = asked(&h, |cx| change.request(cx));
    assert_eq!(area, Some(list.row_rect(0).union(&list.row_rect(2))));
    assert!(!all);
    assert_eq!(refresh, Some(RefreshMode::Fast));
    let change = list.move_focus(5, 12);
    assert_eq!(change, ListChange::Page);
    let (_, all, refresh) = asked(&h, |cx| change.request(cx));
    assert!(all);
    assert_eq!(refresh, Some(RefreshMode::Partial));
    assert_eq!(
        asked(&h, |cx| ListChange::None.request(cx)),
        (None, false, None)
    );

    // Rows: the entries around a heading.
    let mut rows = Rows::new(
        ROWS_AREA,
        vec![10, 20, 20, 10, 20],
        vec![true, false, false, true, false],
    );
    let change = rows.step(true);
    assert!(change.is_focus());
    let (area, all, _) = asked(&h, |cx| change.request(cx));
    assert_eq!(
        area,
        Some(rows.rect_of(1).unwrap().union(&rows.rect_of(2).unwrap()))
    );
    assert!(!all);

    // Sections alike.
    let mut list = Sections::new(
        Rect::new(0, 0, 300, 200),
        10,
        20,
        vec![Section {
            heading: true,
            count: 3,
        }],
    );
    let change = list.step(true);
    let (area, all, _) = asked(&h, |cx| change.request(cx));
    assert_eq!(
        area,
        Some(list.rect_of(0).unwrap().union(&list.rect_of(1).unwrap()))
    );
    assert!(!all);
}

// ---- drawing in bands --------------------------------------------------

/// A frame drawn in bands, as the X4's is (five of 192 rows for 800): the
/// screen's `draw` runs once for each, and a row outside a band is not
/// worked on for it, yet the bands come out as the frame drawn whole.
mod bands {
    use super::*;
    use paperpaw_sdk::ui::Ui;
    use paperpaw_graphics::Framebuffer;
    use paperpaw_sdk::Color;

    const WIDTH: u16 = 400;
    const HEIGHT: u16 = 840;
    const BAND: u16 = 192;

    fn pixels(frame: &Framebuffer) -> Vec<bool> {
        let view = frame.view();
        (0..HEIGHT)
            .flat_map(|y| (0..WIDTH).map(move |x| (x, y)))
            .map(|(x, y)| view.pixel(x, y) == Color::BLACK)
            .collect()
    }

    /// Draws with `draw` once whole and once in bands; returns the two
    /// frames' pixels and, for each band, the rows the band worked on.
    fn both(
        draw: &dyn Fn(&mut Ui<'_>, &mut dyn FnMut(usize, Rect)),
    ) -> (Vec<bool>, Vec<bool>, Vec<(Rect, Vec<(usize, Rect)>)>) {
        let harness = Harness::new(Device::BUTTONS);
        let env = harness.env();
        let style = style();
        let mut whole = frame(WIDTH, HEIGHT);
        draw(
            &mut Ui::new(&mut whole.canvas(), &env, &style),
            &mut |_, _| {},
        );
        let mut banded = frame(WIDTH, HEIGHT);
        let mut worked = Vec::new();
        let mut top = 0;
        while top < HEIGHT {
            let band = Rect::new(0, top, WIDTH, BAND.min(HEIGHT - top));
            let mut rows = Vec::new();
            let mut canvas = banded.canvas();
            let mut part = canvas.clipped(band);
            draw(
                &mut Ui::new(&mut part, &env, &style),
                &mut |index, rect| rows.push((index, rect)),
            );
            worked.push((band, rows));
            top += BAND;
        }
        (pixels(&whole), pixels(&banded), worked)
    }

    fn row_ink(ui: &mut Ui<'_>, rect: Rect) {
        ui.canvas().fill(rect.inset(12, 12), Color::BLACK);
    }

    #[test]
    fn a_list_works_only_on_the_rows_in_the_band_and_draws_as_a_whole() {
        let mut list = ListView::new(Rect::new(0, 0, WIDTH, HEIGHT), 70);
        list.set_focus(5);
        let len = 12;
        let (whole, banded, worked) = both(&|ui, seen| {
            list.draw(ui, len, |ui, index, rect, _| {
                seen(index, rect);
                row_ink(ui, rect);
            });
        });
        assert_eq!(whole, banded, "the bands make the frame drawn whole");
        for (band, rows) in &worked {
            let lying_in: Vec<usize> = (0..len)
                .filter(|&i| {
                    let rect = list.row_rect(i);
                    rect.intersection(band).is_some()
                })
                .collect();
            let seen: Vec<usize> = rows.iter().map(|&(i, _)| i).collect();
            assert_eq!(seen, lying_in, "band {band:?}");
        }
        let calls: usize = worked.iter().map(|(_, rows)| rows.len()).sum();
        assert!(
            calls < 2 * len,
            "{calls} calls for {len} rows in {} bands",
            worked.len()
        );
    }

    #[test]
    fn rows_work_only_on_the_rows_in_the_band_and_draw_as_a_whole() {
        let heights = vec![40, 90, 90, 60, 120, 90, 90, 90, 100, 90];
        let headings = vec![
            true, false, false, true, false, false, false, false, true, false,
        ];
        let mut rows = Rows::new(Rect::new(0, 0, WIDTH, HEIGHT), heights, headings);
        rows.step(true);
        rows.step(true);
        let visible = rows.visible();
        let (whole, banded, worked) = both(&|ui, seen| {
            rows.draw(ui, true, |ui, index, rect, _| {
                seen(index, rect);
                row_ink(ui, rect);
            });
        });
        assert_eq!(whole, banded, "the bands make the frame drawn whole");
        for (band, drawn) in &worked {
            let lying_in: Vec<usize> = visible
                .iter()
                .filter(|(_, rect)| rect.intersection(band).is_some())
                .map(|&(i, _)| i)
                .collect();
            let seen: Vec<usize> = drawn.iter().map(|&(i, _)| i).collect();
            assert_eq!(seen, lying_in, "band {band:?}");
        }
    }
}
