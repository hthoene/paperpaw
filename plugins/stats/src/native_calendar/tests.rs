//! The calendar's contents and layout over the plugin's catalogs and files.

use alloc::string::ToString;
use core::cell::RefCell;

use paperpaw_sdk::{Color, MemoryStorage, Point, Rect};
use paperpaw_ui_kit::Style;

use super::{CalendarView, Cells, Plan};
use crate::screens_tests::{QuietWords, sweep, words};
use crate::store::{self, Book, PageStat};
use crate::testing::Quiet;
use crate::testing::heap::fixture;

const DAY: u32 = 86_400;
/// Day 20,000 (4 October 2024), UTC.
const TODAY: i32 = 20_000;

fn read(storage: &MemoryStorage, day: i32, book: u16, page: u32, seconds: u16) {
    store::append(
        storage,
        PageStat {
            book,
            page,
            start: day as u32 * DAY + 8 * 3_600,
            duration: seconds,
            pages: 100,
        },
    )
    .unwrap();
}

/// Two books; 23 minutes today on pages 1 and 2, 41 yesterday, and a month
/// before 5 minutes.
fn card() -> Quiet {
    let storage = MemoryStorage::new();
    let book = |id: u16, title: &str| Book {
        id,
        title: title.to_string(),
        authors: "N/A".to_string(),
        pages: 100,
        ..Book::default()
    };
    store::save_books(&storage, &[book(1, "Anna Karenina"), book(2, "B")]).unwrap();
    read(&storage, TODAY - 31, 2, 1, 300);
    read(&storage, TODAY - 1, 2, 5, 41 * 60);
    read(&storage, TODAY, 1, 1, 600);
    read(&storage, TODAY, 1, 2, 780);
    Quiet(storage)
}

#[test]
fn the_chosen_day_is_today_with_its_time_pages_and_books_in_english() {
    let data = card();
    let view = CalendarView::build(&mut words("en"), &data, Some(TODAY), 1).unwrap();
    assert_eq!(view.day, TODAY);
    assert_eq!(view.title[0], "October 2024");
    assert_eq!(view.title[1], "Oct. 2024");
    assert_eq!(view.said.long, "Friday, October 4");
    assert_eq!(view.said.summary, "23 min · 2 pages");
    assert_eq!(
        view.said.books,
        [("Anna Karenina".to_string(), "23 min".to_string())]
    );
    assert_eq!(view.heading, "Calendar");
    assert_eq!(view.names, ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"]);
    // Two days in a row, today and yesterday.
    assert_eq!(view.note.as_deref(), Some("2 days in a row"));
}

#[test]
fn in_german_the_week_may_start_on_sunday() {
    let data = card();
    let view = CalendarView::build(&mut words("de"), &data, Some(TODAY), 0).unwrap();
    assert_eq!(view.heading, "Kalender");
    assert_eq!(view.names[0], "So");
    assert_eq!(view.title[0], "Oktober 2024");
    assert_eq!(view.said.summary, "23 min · 2 Seiten");
}

#[test]
fn without_the_clock_the_last_day_read_is_chosen_and_without_either_nothing() {
    let data = card();
    let view = CalendarView::build(&mut words("en"), &data, None, 1).unwrap();
    assert_eq!(view.day, TODAY);
    assert!(!view.empty());
    assert!(view.note.is_none());
    let none = Quiet(MemoryStorage::new());
    let view = CalendarView::build(&mut words("en"), &none, None, 1).unwrap();
    assert!(view.empty());
}

#[test]
fn choosing_a_day_reads_it_and_a_failure_changes_nothing() {
    let data = card();
    let mut view = CalendarView::build(&mut words("en"), &data, Some(TODAY), 1).unwrap();
    view.show(&mut words("en"), &data, TODAY - 1).unwrap();
    assert_eq!(view.day, TODAY - 1);
    assert_eq!(view.said.summary, "41 min · 1 page");
    // A day of another month turns the month's name with it.
    view.show(&mut words("en"), &data, TODAY - 31).unwrap();
    assert_eq!(view.title[0], "September 2024");
    assert_eq!(view.said.summary, "5 min · 1 page");
    view.show(&mut words("en"), &data, TODAY - 5).unwrap();
    assert_eq!(view.said.summary, "Not read");
    assert!(view.said.books.is_empty());
}

#[test]
fn the_calendar_refuses_cleanly() {
    let data = card();
    let catalog = RefCell::new(fixture(|| words("en")));
    for today in [Some(TODAY), None] {
        sweep(&|| {
            CalendarView::build(&mut QuietWords(&catalog), &data, today, 1)
                .map(|view| view.names.len())
        });
    }
    // A refused read of another day leaves the chosen one as it was.
    let view = RefCell::new(fixture(|| {
        CalendarView::build(&mut words("en"), &data, Some(TODAY), 1).unwrap()
    }));
    let before = fixture(|| {
        let v = view.borrow();
        (v.day, v.said.clone(), v.title.clone())
    });
    sweep(&|| {
        let result = view
            .borrow_mut()
            .show(&mut QuietWords(&catalog), &data, TODAY - 1);
        // Compared and put back with nothing refused.
        fixture(|| {
            let mut v = view.borrow_mut();
            if result.is_err() {
                assert_eq!((v.day, &v.said, &v.title), (before.0, &before.1, &before.2));
            } else {
                v.show(&mut words("en"), &data, TODAY).unwrap();
            }
        });
        result.map(|()| 0)
    });
}

fn style() -> Style {
    Style {
        corner: 4,
        pill: false,
        ring: false,
        soft: false,
        shadow: 0,
        frame: None,
        button: paperpaw_ui_kit::style::ButtonLook::Outlined,
        mono: false,
        roles: paperpaw_ui_kit::style::Roles::default(),
        separator_dither: false,
        separator_ink: Color::BLACK,
        text: 22.0,
        small: 17.0,
        title: 26.0,
        head: 28.0,
        row: 64,
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
fn the_days_are_square_and_everything_stays_on_the_screen() {
    for (width, height) in [(480, 800), (800, 480), (240, 320), (1404, 1872)] {
        let plan = Plan::of(&style(), width, height, 22);
        assert!(plan.cell > 0, "{width}x{height}");
        assert!(plan.grid_x + plan.cell * 7 <= width, "{width}x{height}");
        assert!(plan.cells_y + plan.cell * 6 <= height, "{width}x{height}");
        assert!(plan.detail.y >= plan.cells_y + plan.cell * 6);
        // The arrows are inside the screen and apart.
        assert!(plan.left.x + plan.left.width < plan.right.x);
        assert!(plan.right.x + plan.right.width <= width);
    }
    // A tiny screen leaves no room and no panic.
    let plan = Plan::of(&style(), 100, 100, 22);
    assert_eq!(plan.detail.height, 0);
}

#[test]
fn a_tap_finds_its_square_and_squares_beyond_the_weeks_are_none() {
    let cells = Cells {
        x: 10,
        y: 100,
        size: 50,
    };
    assert_eq!(cells.at(Point::new(10, 100)), Some(0));
    assert_eq!(cells.at(Point::new(59, 149)), Some(0));
    assert_eq!(cells.at(Point::new(60, 100)), Some(1));
    assert_eq!(cells.at(Point::new(359, 399)), Some(41));
    assert_eq!(cells.at(Point::new(360, 100)), None);
    assert_eq!(cells.at(Point::new(9, 100)), None);
    assert_eq!(cells.at(Point::new(10, 99)), None);
    assert_eq!(cells.at(Point::new(10, 400)), None);
    assert_eq!(cells.of(8), Rect::new(60, 150, 50, 50));
    let none = Cells {
        x: 0,
        y: 0,
        size: 0,
    };
    assert_eq!(none.at(Point::new(5, 5)), None);
}

#[test]
fn days_far_out_never_overflow_the_day_math() {
    use crate::calendar_model::{self as model, MAX_DAY, Step};
    let steps = [
        Step::DayBack,
        Step::DayForward,
        Step::WeekBack,
        Step::WeekForward,
        Step::MonthBack,
        Step::MonthForward,
    ];
    for day in [
        i32::MIN,
        -1,
        0,
        1,
        MAX_DAY - 1,
        MAX_DAY,
        MAX_DAY + 1,
        i32::MAX,
    ] {
        for step in steps {
            assert!((0..=MAX_DAY).contains(&model::step(day, step)));
        }
        let first = model::first_shown(day, 1);
        assert!(first <= MAX_DAY);
        let _ = model::month_of(day);
        let _ = model::tap_day(day, first.saturating_add(41));
    }
    assert_eq!(model::step(0, Step::DayBack), 0);
    assert_eq!(model::step(MAX_DAY, Step::WeekForward), MAX_DAY);
    // The last month can be shown and read.
    let data = card();
    let mut view = CalendarView::build(&mut words("en"), &data, Some(i32::MAX), 1).unwrap();
    assert_eq!(view.day, MAX_DAY);
    view.show(
        &mut words("en"),
        &data,
        model::step(MAX_DAY, Step::MonthForward),
    )
    .unwrap();
    assert_eq!(view.title[0], "December 9999");
}
