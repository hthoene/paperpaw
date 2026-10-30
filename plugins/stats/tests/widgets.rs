//! The statistics' widgets on the home screen, with the real kernel, the
//! home screen and the default fonts, over a log of reading: each shows
//! its numbers, in every frame and its styles, and draws something.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_core::Kernel;
use paperpaw_platform::{Rotation, SystemStatus, WallClock};
use paperpaw_sdk::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, MemoryStorage, PixelFormat, Storage,
};

/// An X4: black and white, buttons.
const DEVICE: DeviceInfo = DeviceInfo::new(
    "test-x4",
    "Test X4",
    DisplayInfo::new(480, 800, 220, PixelFormat::Mono1, Rotation::Deg0, None),
    InputCaps::new(
        true,
        false,
        false,
        &["back", "confirm", "left", "right", "up", "down", "power"],
    ),
    MemoryClass::Small,
);

/// 14 October 2026, 10:41 in Berlin (UTC+2): local day 20 740.
const NOW_MILLIS: i64 = 1_791_967_260_000;
const TODAY: i64 = 20_740;
const DIR: &str = ".paperpaw/plugins/stats";

/// A card with reading: `minutes` on each day back from today (0), a
/// page a minute from 19:00, and books, two finished this year.
fn card(minutes: &[(i64, u32)]) -> MemoryStorage {
    let storage = MemoryStorage::new();
    let mut log = Vec::new();
    // In the order they were read, as the log has them.
    let mut minutes = minutes.to_vec();
    minutes.sort_by_key(|(back, _)| -back);
    for (back, minutes) in minutes {
        let start = (TODAY - back) * 86_400 + 17 * 3600;
        for m in 0..minutes {
            log.extend_from_slice(&1u16.to_le_bytes());
            log.extend_from_slice(&(m + 1).to_le_bytes());
            log.extend_from_slice(&((start + i64::from(m) * 60) as u32).to_le_bytes());
            log.extend_from_slice(&60u16.to_le_bytes());
            log.extend_from_slice(&300u32.to_le_bytes());
        }
    }
    storage.write(&format!("{DIR}/pages.bin"), &log).unwrap();
    let this_year = (TODAY - 100) * 86_400;
    let books = format!(
        "[book.1]\ntitle = Anna Karenina\nlast-open = {this_year}\npath = books/a.epub\nfinished = {this_year}\n\n\
         [book.2]\ntitle = Effi Briest\nlast-open = {}\npath = books/b.epub\nfinished = {}\n\n\
         [book.3]\ntitle = Old\nlast-open = 1\npath = books/c.epub\nfinished = 1\n",
        this_year + 86_400,
        this_year + 86_400,
    );
    storage
        .write(&format!("{DIR}/books.ini"), books.as_bytes())
        .unwrap();
    storage
}

/// The home screen with `page` as its first page and the settings of
/// `config` (lines of the `[stats]` section): what it describes, and
/// how many pixels are black.
fn home(storage: MemoryStorage, page: &str, config: &str) -> (String, usize) {
    let (text, rows) = screen(storage, page, config);
    (text, rows.iter().sum())
}

/// [`home`], with the black pixels of each row of the screen.
fn screen(storage: MemoryStorage, page: &str, config: &str) -> (String, Vec<usize>) {
    let user = format!(
        "[core]\nlanguage = en\n[help]\nshow = false\n[home]\ncolumns = 4\npage-1 = \"{page}\"\n[stats]\n{config}\n"
    );
    let mut kernel = Kernel::builder(DEVICE)
        .storage(storage)
        .user_config(user)
        .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
        .plugin(paperpaw_plugin_home::Home)
        .plugin(paperpaw_plugin_stats::Stats)
        .build()
        .unwrap();
    let clock = WallClock::new(Instant(0), NOW_MILLIS, Some(120));
    let status = SystemStatus::new(Some(clock), None);
    kernel.set_status(Instant(0), status);
    let mut effects = kernel.start(Instant(0));
    let mut now = 0;
    for _ in 0..1000 {
        match effects.wake_at() {
            Some(at) if at.0 <= now + 1000 => {
                now = now.max(at.0);
                effects = kernel.tick(Instant(now));
            }
            _ => break,
        }
    }
    let mut rows = vec![0; usize::from(DEVICE.display.height)];
    kernel.render(&mut |top, view| {
        for y in 0..view.height {
            let black = (0..view.width)
                .filter(|&x| view.pixel(x, y).luma() < 128)
                .count();
            if let Some(row) = rows.get_mut(usize::from(top + y)) {
                *row += black;
            }
        }
    });
    let mut text = String::new();
    for part in kernel.describe() {
        text.push_str(&part.label);
        text.push('\n');
    }
    (text, rows)
}

/// Black pixels in the rows from `from` to `to` of a screen's `rows`.
fn ink(rows: &[usize], from: usize, to: usize) -> usize {
    rows[from..to].iter().sum()
}

/// A week of reading: 23 minutes today, 41 yesterday, then 18, 65, 12,
/// none, 30.
fn week() -> MemoryStorage {
    card(&[(0, 23), (1, 41), (2, 18), (3, 65), (4, 12), (6, 30)])
}

const FRAMES: [&str; 4] = [
    "core.frame.none",
    "core.frame.line",
    "core.frame.card",
    "core.frame.dark",
];

#[test]
fn read_today_shows_the_time_and_the_goal() {
    let (shown, _) = home(week(), "stats.widget.today 0/0 4x2", "goal = stats.goal.30");
    assert!(shown.contains("Today"), "{shown}");
    assert!(shown.contains("23 min · 23 pages"), "{shown}");
    // The time kept on one line, should the note be broken.
    assert!(shown.contains("7\u{a0}min to your daily goal"), "{shown}");
    // Without the pages, without a goal: the week.
    let (shown, _) = home(
        week(),
        "stats.widget.today 0/0 4x2",
        "widget.today.pages = false",
    );
    assert!(shown.contains("23 min\n"), "{shown}");
    assert!(shown.contains("This week"), "{shown}");
}

#[test]
fn every_widget_draws_in_every_frame() {
    let widgets = [
        "stats.widget.today",
        "stats.widget.week",
        "stats.widget.goal",
        "stats.widget.calendar",
        "stats.widget.year",
        "stats.widget.streak",
    ];
    let (_, empty) = home(week(), "", "");
    for widget in widgets {
        for frame in FRAMES {
            for size in ["4x2", "2x2", "4x1"] {
                let page = format!("{widget} 0/0 {size}");
                let config = format!(
                    "goal = stats.goal.30\n{}.frame = {frame}",
                    widget.trim_start_matches("stats.")
                );
                let (_, black) = home(week(), &page, &config);
                assert!(
                    black > empty + 50,
                    "{widget} {size} in {frame} draws nothing"
                );
            }
        }
    }
}

#[test]
fn the_days_show_as_bars_with_their_weekdays() {
    let (shown, _) = home(week(), "stats.widget.week 0/0 4x2", "");
    // 23 + 41 + 18 + 65 + 12 + 30 minutes.
    assert!(shown.contains("Last 7 days: 3 h 09 min"), "{shown}");
    for day in ["Th", "Fr", "Sa", "Su", "Mo", "Tu", "We"] {
        assert!(shown.contains(&format!("{day}\n")), "{day}: {shown}");
    }
    let (shown, _) = home(
        week(),
        "stats.widget.week 0/0 4x2",
        "widget.week.period = stats.period.week",
    );
    // From Monday, the device's first day of the week (by default as
    // the language has it): the last three days.
    assert!(shown.contains("This week: 1 h 22 min"), "{shown}");
}

#[test]
fn the_goal_ring_counts_minutes_and_days() {
    let (shown, _) = home(week(), "stats.widget.goal 0/0 2x3", "goal = stats.goal.20");
    assert!(shown.contains("Daily goal"), "{shown}");
    assert!(shown.contains("23\n"), "{shown}");
    // Today and yesterday reached 20 minutes, the day before not.
    assert!(shown.contains("2 days in a row"), "{shown}");
    // Without a goal: the way to set one.
    let (shown, _) = home(week(), "stats.widget.goal 0/0 2x3", "");
    assert!(shown.contains("Set a daily goal"), "{shown}");
}

#[test]
fn the_calendar_shows_the_month_and_the_weeks() {
    let (shown, _) = home(week(), "stats.widget.calendar 0/0 4x4", "");
    assert!(shown.contains("October 2026"), "{shown}");
    assert!(shown.contains("31\n"), "{shown}");
    assert!(shown.contains("5 days in a row"), "{shown}");
    assert!(shown.contains("3 h 09 min in October"), "{shown}");
    let (shown, _) = home(
        week(),
        "stats.widget.calendar 0/0 4x4",
        "widget.calendar.style = stats.calendar.weeks",
    );
    assert!(shown.contains("Last 12 weeks"), "{shown}");
    assert!(shown.contains("3 h 09 min in 12 weeks"), "{shown}");
}

#[test]
fn books_this_year_go_towards_the_goal() {
    let (shown, _) = home(week(), "stats.widget.year 0/0 4x3", "");
    assert!(shown.contains("Books in 2026"), "{shown}");
    assert!(shown.contains("2 of 12 books"), "{shown}");
    // Nine were planned by mid-October.
    assert!(shown.contains("7 books to catch up"), "{shown}");
    assert!(shown.contains("Last: Effi Briest"), "{shown}");
    let (shown, _) = home(
        week(),
        "stats.widget.year 0/0 4x3",
        "widget.year.goal = stats.books.goal.off",
    );
    assert!(shown.contains("2 books read"), "{shown}");
}

#[test]
fn the_streak_counts_days_in_a_row() {
    let (shown, _) = home(week(), "stats.widget.streak 0/0 4x3", "");
    assert!(shown.contains("5 days"), "{shown}");
    assert!(shown.contains("in a row, longest 5"), "{shown}");
    let (shown, _) = home(
        week(),
        "stats.widget.streak 0/0 4x3",
        "goal = stats.goal.20\nwidget.streak.counts = stats.streak.goal",
    );
    assert!(shown.contains("2 days"), "{shown}");
    assert!(shown.contains("longest 2"), "{shown}");
}

/// In the low cells of the top row, nothing is drawn below them (their
/// notes are left out rather than drawn past their place).
#[test]
fn low_cells_keep_their_content_inside() {
    let widgets = [
        "stats.widget.today",
        "stats.widget.week",
        "stats.widget.goal",
        "stats.widget.calendar",
        "stats.widget.year",
        "stats.widget.streak",
    ];
    // Below the top row of cells (it ends above 140 on the X4).
    let (_, empty) = screen(week(), "", "");
    let below = ink(&empty, 140, 600);
    for widget in widgets {
        for size in ["1x1", "2x1", "3x1", "4x1"] {
            let page = format!("{widget} 0/0 {size}");
            let (_, rows) = screen(week(), &page, "goal = stats.goal.30");
            assert!(ink(&rows, 52, 140) > 0, "{widget} {size} draws nothing");
            assert_eq!(
                ink(&rows, 140, 600),
                below,
                "{widget} {size} draws below its cell"
            );
        }
    }
}
