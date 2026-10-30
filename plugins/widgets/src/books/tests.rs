//! Tests of the book widgets: what they choose and count, and that each
//! draws, in every frame and at several sizes.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

extern crate std;

use alloc::borrow::Cow;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use paperpaw_graphics::Framebuffer;
use paperpaw_sdk::document::ImageRef;
use paperpaw_sdk::document::Locator;
use paperpaw_sdk::library::{self, catalog};
use paperpaw_sdk::widgets::categories;
use paperpaw_sdk::{
    Color, Id, MemoryStorage, PixelFormat, Rect, SettingKind, SettingSpec, SettingValue,
};

use super::highlight::{self, Found};
use super::next;
use super::numbers::Numbers;
use super::reading::{Position, chapter_left_text, left_text};
use super::series::{self, Member};
use super::wrap_words;
use crate::harness::{DEVICE, Harness, pixels};

/// A cover: a decoded picture (`PPGI`, its width and height, rows of
/// gray), dark at the top, light at the bottom.
const COVER: &str = ".paperpaw/covers/tales.gray";
const COVER_SIZE: (u32, u32) = (300, 450);

fn cover() -> Vec<u8> {
    let (w, h) = COVER_SIZE;
    let mut data = b"PPGI".to_vec();
    data.extend_from_slice(&w.to_le_bytes());
    data.extend_from_slice(&h.to_le_bytes());
    for y in 0..h {
        data.extend((0..w).map(|x| ((y * 255 / h + x) % 256) as u8));
    }
    data
}

/// A library: a series of four, the third being read, the first read,
/// each with a cover; two other books, favourites, a highlight.
pub(crate) fn library() -> MemoryStorage {
    let mut storage = MemoryStorage::new().with(COVER, cover());
    let mut records = Vec::new();
    let mut add = |storage: MemoryStorage,
                   path: &str,
                   title: &str,
                   series: &str,
                   n: &str,
                   progress: Option<u8>| {
        let storage = storage.with(path, "a book");
        let book = library::Book::default()
            .size(6)
            .indexed(true)
            .title(title.to_string())
            .author("Ann Author".to_string())
            .series((!series.is_empty()).then(|| series.to_string()))
            .series_index((!n.is_empty()).then(|| n.to_string()))
            .cover((!series.is_empty()).then(|| ImageRef {
                path: COVER.to_string(),
                width: COVER_SIZE.0,
                height: COVER_SIZE.1,
            }))
            .progress(progress);
        library::save(&storage, path, &book).unwrap();
        records.push(catalog::Record::of(path, &book));
        storage
    };
    storage = add(
        storage,
        "books/s1.epub",
        "The First",
        "Tales",
        "1",
        Some(100),
    );
    storage = add(storage, "books/s2.epub", "The Second", "Tales", "2", None);
    storage = add(
        storage,
        "books/s3.epub",
        "The Third",
        "Tales",
        "3",
        Some(40),
    );
    storage = add(storage, "books/s4.epub", "The Fourth", "Tales", "4", None);
    storage = add(storage, "books/a.epub", "Alone", "", "", None);
    storage = add(storage, "books/b.epub", "Begun", "", "", Some(10));
    catalog::write(&storage, &records).unwrap();
    let _ = paperpaw_sdk::Storage::write(
        &storage,
        ".paperpaw/library/opened.txt",
        b"books/s3.epub\nbooks/b.epub\nbooks/s1.epub",
    );
    library::set_favourite(&storage, "books/a.epub", true);
    library::set_favourite(&storage, "books/s1.epub", true);
    paperpaw_sdk::annotations::add(
        &storage,
        "books/s3.epub",
        paperpaw_sdk::annotations::Annotation::new(
            paperpaw_sdk::annotations::Mark::new(
                paperpaw_sdk::annotations::Kind::Highlight,
                Locator::new(2, 10),
                Locator::new(2, 60),
                false,
            ),
            30,
            0,
            0,
            "A sentence worth keeping, of some length.".to_string(),
            String::new(),
        ),
    )
    .unwrap();
    Position {
        path: "books/s3.epub".to_string(),
        page: Some(40),
        pages: Some(100),
        chapter: "Chapter Three, in Which Something Happens at Last".to_string(),
        chapter_left: Some(6),
        seconds: 600,
        turned: 20,
    }
    .save(&storage);
    storage
}

const IDS: [&str; 8] = [
    "widgets.progress",
    "widgets.reading",
    "widgets.next",
    "widgets.series",
    "widgets.favourites",
    "widgets.highlight",
    "widgets.library",
    "widgets.battery",
];

/// Each way the widgets with covers or long texts can be drawn.
const STYLES: [(&str, &str, &str); 7] = [
    ("widgets.progress", "style", "widgets.progress.beside"),
    ("widgets.progress", "style", "widgets.progress.round"),
    ("widgets.progress", "style", "widgets.progress.bar"),
    ("widgets.favourites", "layout", "widgets.books.layout.grid"),
    ("widgets.reading", "frame", "core.frame.line"),
    ("widgets.next", "frame", "core.frame.line"),
    ("widgets.highlight", "size", "core.size.fit"),
];

/// Placed, a widget has made what it draws (its covers small, its text
/// in lines) and draws without reading anything: the same with the
/// storage gone.
#[test]
fn placed_widgets_draw_from_memory() {
    let mut h = Harness::new(library());
    let mut bare = Harness::new(MemoryStorage::new());
    let styles = STYLES
        .iter()
        .map(|&(id, key, value)| (id, Some((key, value))));
    for (id, style) in IDS.iter().map(|&id| (id, None)).chain(styles) {
        if let Some((key, value)) = style {
            let value = SettingValue::Text(Cow::Borrowed(value));
            h.set(&alloc::format!("{id}.{key}"), value.clone());
            bare.set(&alloc::format!("{id}.{key}"), value);
        }
        for area in [
            Rect::new(10, 10, 230, 200),
            Rect::new(10, 10, 460, 200),
            Rect::new(10, 10, 460, 600),
        ] {
            let env = h.env();
            let mut widget = env.widgets().make(id, &env).unwrap();
            widget.place(&env, area);
            let drawn = pixels(&env, &*widget, area);
            assert!(drawn.iter().any(|&p| p < 128), "{id} {style:?} {area:?}");
            let without = pixels(&bare.env(), &*widget, area);
            assert!(
                drawn == without,
                "{id} {style:?} {area:?} reads as it draws"
            );
        }
    }
}

/// The logo widget with a picture for the logo: the picture is read and
/// made small where the widget is placed, and drawn from memory.
#[test]
fn a_logo_picture_draws_from_memory() {
    let logo = ".paperpaw/plugins/mine/logo.png";
    let storage = library();
    // Larger than it is drawn: made small, not larger.
    let mut png = paperpaw_sdk::image::PngWriter::new(&storage, logo, 256, 256).unwrap();
    for y in 0..256u32 {
        let row: Vec<u8> = (0..256u32)
            .map(|x| if (x / 32 + y / 32) % 2 == 0 { 0 } else { 255 })
            .collect();
        png.row(&storage, logo, &row).unwrap();
    }
    let mut h = Harness::new(storage);
    let mut bare = Harness::new(MemoryStorage::new());
    for h in [&mut h, &mut bare] {
        h.settings
            .define(SettingSpec::new(
                Id::new_static("core.logo"),
                "x",
                SettingKind::Text,
                SettingValue::Text(Cow::Borrowed(logo)),
                paperpaw_sdk::builtin::groups::OTHER,
            ))
            .unwrap();
        h.set(
            "widgets.logo.size",
            SettingValue::Text(Cow::Borrowed("core.size.large")),
        );
    }
    let env = h.env();
    let area = Rect::new(10, 10, 460, 150);
    let mut widget = env.widgets().make("widgets.logo", &env).unwrap();
    widget.place(&env, area);
    let drawn = pixels(&env, &*widget, area);
    let drawn_logo = paperpaw_sdk::logo::Logo::read(&env, 10, 10);
    assert!(matches!(drawn_logo, paperpaw_sdk::logo::Logo::Picture(_)));
    assert!(drawn.iter().any(|&p| p < 128));
    assert!(
        pixels(&bare.env(), &*widget, area) == drawn,
        "reads as it draws"
    );
}

/// Texts laid out when their widget is placed (a highlight, a note, the
/// quote of the day) show as those laid out as they are drawn unplaced.
#[test]
fn texts_draw_as_laid_out() {
    let mut h = Harness::new(library());
    let note = "If found, please return to Ann Author, who will be very glad to have it \
                back and will gladly lend you another book in return.";
    h.set(
        "widgets.note.text",
        SettingValue::Text(Cow::Owned(note.to_string())),
    );
    h.set(
        "widgets.note.heading",
        SettingValue::Text(Cow::Borrowed("Mine")),
    );
    let env = h.env();
    for id in ["widgets.highlight", "widgets.note", "widgets.quote"] {
        for area in [Rect::new(10, 10, 230, 200), Rect::new(10, 10, 460, 200)] {
            let unplaced = env.widgets().make(id, &env).unwrap();
            let mut placed = env.widgets().make(id, &env).unwrap();
            placed.place(&env, area);
            let drawn = pixels(&env, &*placed, area);
            assert!(drawn.iter().any(|&p| p < 128), "{id} {area:?}");
            assert!(drawn == pixels(&env, &*unplaced, area), "{id} {area:?}");
        }
    }
}

/// Draws widget `id` into `area` of a page; how many pixels are dark.
fn draw(h: &Harness, id: &str, area: Rect) -> usize {
    let env = h.env();
    let widget = env.widgets().make(id, &env).unwrap();
    let mut fb = Framebuffer::new(
        DEVICE.display.width,
        DEVICE.display.height,
        PixelFormat::Gray4,
    )
    .unwrap();
    {
        let mut canvas = fb.canvas();
        canvas.clear(Color::WHITE);
        widget.draw(&mut canvas, &env, area);
    }
    let view = fb.view();
    let mut dark = 0;
    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            dark += usize::from(view.pixel(x, y).luma() < 128);
        }
    }
    dark
}

#[test]
fn every_widget_draws_in_every_frame_and_size() {
    let mut h = Harness::new(library());
    for id in IDS {
        assert!(h.widgets.has(id), "{id}");
        let kind = h.widgets.category(id).unwrap();
        assert!(categories::ALL.contains(&kind), "{id}: {kind}");
        for frame in paperpaw_sdk::widgets::FRAMES {
            h.set(
                &alloc::format!("{id}.frame"),
                SettingValue::Text(Cow::Borrowed(frame)),
            );
            for area in [
                Rect::new(10, 10, 110, 95),
                Rect::new(10, 10, 230, 200),
                Rect::new(10, 10, 460, 200),
                Rect::new(10, 10, 460, 600),
            ] {
                let dark = draw(&h, id, area);
                assert!(dark > 50, "{id} {frame} {area:?}: {dark}");
            }
        }
    }
}

#[test]
fn every_style_draws() {
    let mut h = Harness::new(library());
    let area = Rect::new(0, 0, 460, 220);
    for (id, key, values) in [
        (
            "widgets.progress",
            "style",
            &[
                "widgets.progress.beside",
                "widgets.progress.round",
                "widgets.progress.ring",
                "widgets.progress.bar",
            ][..],
        ),
        (
            "widgets.battery",
            "style",
            &[
                "widgets.battery.lying",
                "widgets.battery.standing",
                "widgets.battery.ring",
            ][..],
        ),
        (
            "widgets.favourites",
            "layout",
            &["widgets.books.layout.row", "widgets.books.layout.grid"][..],
        ),
    ] {
        for value in values {
            h.set(
                &alloc::format!("{id}.{key}"),
                SettingValue::Text(Cow::Borrowed(value)),
            );
            assert!(draw(&h, id, area) > 50, "{id} {value}");
        }
    }
}

#[test]
fn widgets_show_what_the_library_knows() {
    let h = Harness::new(library());
    let env = h.env();
    // Tapped, the progress opens the book being read.
    let widget = env.widgets().make("widgets.progress", &env).unwrap();
    let targets = widget.targets(&env, Rect::new(0, 0, 200, 100));
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].command, super::open("books/s3.epub"));
    // The series: each book a target, in order.
    let widget = env.widgets().make("widgets.series", &env).unwrap();
    let targets = widget.targets(&env, Rect::new(0, 0, 460, 300));
    let opened: Vec<_> = targets.iter().map(|t| t.command.clone()).collect();
    assert_eq!(
        opened,
        [
            "books/s1.epub",
            "books/s2.epub",
            "books/s3.epub",
            "books/s4.epub"
        ]
        .map(super::open)
    );
    // Next up: the next in the series not read.
    let widget = env.widgets().make("widgets.next", &env).unwrap();
    let targets = widget.targets(&env, Rect::new(0, 0, 200, 100));
    assert_eq!(targets[0].command, super::open("books/s4.epub"));
    // The favourites, the newest first.
    let widget = env.widgets().make("widgets.favourites", &env).unwrap();
    let targets = widget.targets(&env, Rect::new(0, 0, 460, 300));
    let opened: Vec<_> = targets.iter().map(|t| t.command.clone()).collect();
    assert_eq!(opened, ["books/s1.epub", "books/a.epub"].map(super::open));
    // The highlight opens its book at its place.
    let widget = env.widgets().make("widgets.highlight", &env).unwrap();
    let targets = widget.targets(&env, Rect::new(0, 0, 200, 100));
    assert_eq!(targets.len(), 1);
}

#[test]
fn series_go_by_their_numbers() {
    assert!(super::series_rank("2") < super::series_rank("2.5"));
    assert!(super::series_rank("2.5") < super::series_rank("10"));
    assert_eq!(super::series_rank("3,25"), 3250);
    assert_eq!(super::series_rank(""), u64::MAX);
    let member = |path: &str, title: &str, number: &str, read: bool| Member {
        path: path.to_string(),
        title: title.to_string(),
        number: number.to_string(),
        read,
    };
    let mut members = vec![
        member("c", "Cee", "10", false),
        member("x", "Extra", "", false),
        member("a", "Ay", "2", true),
        member("b", "Bee", "2.5", false),
    ];
    series::order(&mut members);
    let paths: Vec<&str> = members.iter().map(|m| m.path.as_str()).collect();
    assert_eq!(paths, ["a", "b", "c", "x"]);
    // Next: the first after the current one not read.
    members[1].read = true;
    assert_eq!(
        next::next_in_series(&members, "a").map(|m| m.path.as_str()),
        Some("c")
    );
    assert_eq!(next::next_in_series(&members, "x"), None);
    assert_eq!(next::next_in_series(&members, "elsewhere"), None);
    // Which show of a long series: round the book being read.
    assert_eq!(series::window(10, 4, Some(6), true), 5..9);
    assert_eq!(series::window(10, 4, Some(9), true), 6..10);
    assert_eq!(series::window(10, 4, Some(6), false), 0..4);
    assert_eq!(series::window(3, 6, None, true), 0..3);
}

/// Of a series longer than is kept, the books round the one being read
/// are kept, and it is counted whole.
#[test]
fn a_long_series_is_kept_round_the_book_being_read() {
    let records: Vec<catalog::Record> = (1..=150)
        .rev()
        .map(|n| {
            let book = library::Book::default()
                .series("Long".to_string())
                .series_index(n.to_string());
            catalog::Record::of(&alloc::format!("books/long/{n:03}.epub"), &book)
        })
        .collect();
    let storage = MemoryStorage::new();
    catalog::write(&storage, &records).unwrap();
    let h = Harness::new(storage);
    let env = h.env();
    let read = |path: &str| path < "books/long/010.epub";
    let near = series::members(&env, "long", Some("120"), &read);
    assert_eq!((near.total, near.read), (150, 9));
    let numbers: Vec<&str> = near.books.iter().map(|m| m.number.as_str()).collect();
    assert_eq!(numbers.len(), 60);
    assert!(
        numbers.contains(&"120") && numbers.contains(&"121"),
        "{numbers:?}"
    );
    // In order, by number.
    assert_eq!(numbers.first(), Some(&"90"));
    assert_eq!(numbers.last(), Some(&"149"));
    let first = series::members(&env, "long", None, &read);
    assert_eq!(first.books.first().map(|m| m.number.as_str()), Some("1"));
    assert_eq!(first.books.last().map(|m| m.number.as_str()), Some("60"));
}

#[test]
fn chance_picks_another_book_each_day() {
    let books: Vec<String> = (0..50).map(|i| alloc::format!("books/{i}.epub")).collect();
    let first = |day: i64| {
        let mut kept = Vec::new();
        for path in &books {
            next::keep_lowest(&mut kept, 3, next::chance(path, day), path);
        }
        assert_eq!(kept.len(), 3);
        assert!(kept.windows(2).all(|w| w[0].0 <= w[1].0));
        kept[0].1.clone()
    };
    // The same all day; not the same every day.
    assert_eq!(first(20_000), first(20_000));
    let days: std::collections::BTreeSet<String> = (20_000..20_010).map(first).collect();
    assert!(days.len() > 3, "{days:?}");
}

#[test]
fn new_books_are_those_not_seen() {
    let mut seen = next::Seen::new();
    for hash in [1, 2, 3] {
        seen.add(hash);
    }
    assert!(seen.has(2) && !seen.has(5));
    let newest = next::new_books(vec!["five".to_string()], vec!["older".to_string()]);
    assert_eq!(newest, ["five", "older"]);
    // Ten thousand seen: few others count as seen.
    let mut seen = next::Seen::new();
    for n in 0..10_000u64 {
        seen.add(super::mix(n));
    }
    assert!((0..10_000u64).all(|n| seen.has(super::mix(n))));
    let wrong = (10_000..20_000u64)
        .filter(|&n| seen.has(super::mix(n)))
        .count();
    assert!(wrong < 1000, "{wrong}");
}

/// In a large library, a book added after thousands of others is found
/// new, and what is kept stays small.
#[test]
fn new_books_in_a_large_library() {
    let books = |count: usize| -> Vec<catalog::Record> {
        (0..count)
            .map(|n| {
                let path = alloc::format!("books/{n:05}.epub");
                catalog::Record::of(&path, &library::Book::default())
            })
            .collect()
    };
    let storage = MemoryStorage::new();
    catalog::write(&storage, &books(5000)).unwrap();
    let mut h = Harness::new(storage);
    h.set(
        "widgets.next.pick",
        SettingValue::Text(Cow::Borrowed("widgets.next.newest")),
    );
    let pick = |h: &Harness| {
        let env = h.env();
        let widget = env.widgets().make("widgets.next", &env).unwrap();
        let mut described = Vec::new();
        let mut fb = Framebuffer::new(480, 800, PixelFormat::Gray4).unwrap();
        widget.draw(
            &mut fb.canvas().describing(&mut described),
            &env,
            Rect::new(0, 0, 460, 300),
        );
        described
            .into_iter()
            .map(|d| d.label)
            .collect::<Vec<_>>()
            .join(" ")
    };
    // The first look: all seen.
    pick(&h);
    catalog::write(&h.storage, &books(5001)).unwrap();
    paperpaw_sdk::Storage::write(&h.storage, "books/05000.epub", b"a book").unwrap();
    let shown = pick(&h);
    assert!(shown.contains("05000"), "{shown}");
    let kept = paperpaw_sdk::Storage::size(&h.storage, ".paperpaw/plugins/widgets/seen.bits");
    assert_eq!(kept, Ok(8 * 1024));
}

#[test]
fn a_highlight_a_day() {
    let books: Vec<String> = ["a", "b", "c"].iter().map(|s| s.to_string()).collect();
    let found = |path: &str| -> Vec<Found> {
        if path == "b" {
            return Vec::new();
        }
        (0..5)
            .map(|i| {
                let at = Locator::new(i, 0);
                (at, at, alloc::format!("{path}{i}"), String::new())
            })
            .collect()
    };
    let pick = |seed: u64| highlight::pick(&books, seed, &mut |p| found(p)).unwrap();
    // The same for the same day, never from a book without any.
    assert_eq!(pick(7), pick(7));
    let picks: std::collections::BTreeSet<String> = (0..40).map(|d| pick(d).1.2).collect();
    assert!(picks.len() > 4);
    assert!(picks.iter().all(|t| !t.starts_with('b')));
    assert!(highlight::pick(&[], 1, &mut |p| found(p)).is_none());
}

/// Set to change each time it shows, a highlight read anew while it
/// shows (after events, each time a book closes) stays the one shown.
#[test]
fn a_highlight_each_time_it_shows_stays_while_it_shows() {
    let storage = library();
    // Highlights in three books, to have others to change to.
    for (path, n) in [
        ("books/a.epub", 1),
        ("books/b.epub", 2),
        ("books/s1.epub", 3),
    ] {
        paperpaw_sdk::annotations::add(
            &storage,
            path,
            paperpaw_sdk::annotations::Annotation::new(
                paperpaw_sdk::annotations::Mark::new(
                    paperpaw_sdk::annotations::Kind::Highlight,
                    Locator::new(n, 0),
                    Locator::new(n, 20),
                    false,
                ),
                10,
                0,
                0,
                alloc::format!("Highlight number {n}"),
                String::new(),
            ),
        )
        .unwrap();
    }
    let mut h = Harness::new(storage);
    h.set(
        "widgets.highlight.change",
        SettingValue::Text(Cow::Borrowed("widgets.highlight.shown")),
    );
    h.set(
        "widgets.highlight.source",
        SettingValue::Text(Cow::Borrowed("widgets.highlight.all")),
    );
    let env = h.env();
    let mut widget = env.widgets().make("widgets.highlight", &env).unwrap();
    let area = Rect::new(0, 0, 400, 200);
    widget.place(&env, area);
    let first = pixels(&env, &*widget, area);
    for _ in 0..12 {
        assert!(!widget.update(&env), "changed as it was read anew");
    }
    assert!(pixels(&env, &*widget, area) == first);
}

#[test]
fn the_library_counted() {
    let numbers = Numbers::count(
        10,
        [Some(100), Some(40), None, Some(0), Some(100)].into_iter(),
    );
    assert_eq!(
        numbers,
        Numbers {
            books: 10,
            read: 2,
            reading: 1,
            unread: 7
        }
    );
    // Books read outside the library count too.
    assert_eq!(Numbers::count(1, [Some(100), Some(5)].into_iter()).books, 2);
    let h = Harness::new(library());
    let env = h.env();
    let mut widget = env.widgets().make("widgets.library", &env).unwrap();
    assert!(!widget.update(&env));
    assert!(!widget.live());
}

#[test]
fn time_left_from_the_pages_read() {
    let mut position = Position {
        pages: Some(200),
        page: Some(50),
        ..Position::default()
    };
    // Leafing and pauses do not count.
    position.turned(500);
    position.turned(3_600_000);
    assert_eq!(position.turned, 0);
    for _ in 0..9 {
        position.turned(30_000);
    }
    assert_eq!(position.left(), None);
    position.turned(30_000);
    // 150 pages at 30 seconds.
    assert_eq!(position.left(), Some(4500));
    let h = Harness::new(MemoryStorage::new());
    let env = h.env();
    assert_eq!(left_text(&env, 4500), "About 1 h 15 min left");
    assert_eq!(left_text(&env, 600), "About 10 min left");
    // Kept and read again, the chapter too.
    let storage = MemoryStorage::new();
    position.path = "books/a.epub".to_string();
    position.chapter = "Down the Rabbit-Hole".to_string();
    position.chapter_left = Some(12);
    position.save(&storage);
    assert_eq!(Position::load(&storage), position);
    // 12 pages at 30 seconds.
    assert_eq!(position.chapter_seconds(), Some(360));
}

#[test]
fn reading_ini_of_every_version_is_read() {
    // Version 1, as written before the chapter was kept.
    let storage = MemoryStorage::new().with(
        ".paperpaw/plugins/widgets/reading.ini",
        include_str!("../../testdata/reading.ini.v1"),
    );
    let position = Position::load(&storage);
    assert_eq!(position.path, "books/a.epub");
    assert_eq!((position.page, position.pages), (Some(34), Some(210)));
    assert_eq!((position.seconds, position.turned), (600, 20));
    assert_eq!(position.chapter, "");
    assert_eq!(position.chapter_left, None);
    assert_eq!(position.chapter_seconds(), None);
}

#[test]
fn whats_left_of_the_chapter_as_room_allows() {
    let mut h = Harness::new(MemoryStorage::new());
    let mut position = Position {
        chapter: "Down the Rabbit-Hole".to_string(),
        chapter_left: Some(12),
        ..Position::default()
    };
    let text = |h: &Harness, position: &Position, timed, titled, width| {
        chapter_left_text(&h.env(), position, timed, titled, 12.0, width).unwrap()
    };
    let wide = 1000;
    assert_eq!(
        text(&h, &position, true, true, wide),
        "12 pages left in the chapter"
    );
    // With a speed, the time too; less where there is less room.
    position.seconds = 300;
    position.turned = 10;
    assert_eq!(
        text(&h, &position, true, true, wide),
        "12 pages left in the chapter · about 6 min"
    );
    assert_eq!(
        text(&h, &position, false, true, wide),
        "12 pages left in the chapter"
    );
    let env = h.env();
    let font = paperpaw_sdk::ui::UiStyle::new(&env).font(&env).unwrap();
    let width = |text: &str| font.measure(12.0, text).ceil() as u16;
    let narrow = width("12 pages left · about 6 min");
    assert_eq!(
        text(&h, &position, true, true, narrow),
        "12 pages left · about 6 min"
    );
    assert_eq!(text(&h, &position, true, true, narrow - 1), "12 pages left");
    assert_eq!(text(&h, &position, true, true, 10), "12 pages left");
    // Without the chapter's title above it, it says which pages are meant.
    assert_eq!(
        text(&h, &position, true, false, 10),
        "12 pages left in the chapter"
    );
    position.chapter_left = Some(1);
    assert_eq!(
        text(&h, &position, false, true, wide),
        "1 page left in the chapter"
    );
    position.chapter_left = Some(0);
    assert_eq!(
        text(&h, &position, true, true, wide),
        "Last page of the chapter"
    );
    position.chapter_left = None;
    let env = h.env();
    assert_eq!(
        chapter_left_text(&env, &position, true, true, 12.0, wide),
        None
    );
    h.i18n.set_language("de");
    position.chapter_left = Some(12);
    assert_eq!(
        text(&h, &position, true, true, wide),
        "Noch 12 Seiten im Kapitel · etwa 6 Min."
    );
    assert_eq!(text(&h, &position, false, true, 10), "Noch 12 Seiten");
}

#[test]
fn long_titles_in_lines_as_even_as_can_be() {
    let h = Harness::new(MemoryStorage::new());
    let env = h.env();
    let font = paperpaw_sdk::ui::UiStyle::new(&env).font(&env).unwrap();
    let width = |text: &str| font.measure(12.0, text).ceil() as u16;
    let title = "Alice's Adventures in Wonderland";
    let wrap = |most, room| wrap_words(&font, 12.0, title, room, most);
    // Where it fits, or one line is all there is room for: as it is.
    assert_eq!(wrap(3, width(title)), [title]);
    assert_eq!(wrap(1, width("Alice's")), [title]);
    // In two, the longer line as short as can be.
    assert_eq!(
        wrap(3, width("Alice's Adventures in")),
        ["Alice's Adventures", "in Wonderland"]
    );
    // Narrower: as many words as fit, the rest last.
    assert_eq!(
        wrap(3, width("Adventures in")),
        ["Alice's", "Adventures in", "Wonderland"]
    );
    // Where it needs more lines than there are, or a word is wider than
    // a line: on one line, shortened where it is drawn, never a line too
    // wide nor a word broken.
    assert_eq!(wrap(2, width("Adventures in")), [title]);
    assert_eq!(wrap(3, width("Adventures") - 1), [title]);
}

#[test]
fn every_setting_has_a_name_in_both_languages() {
    let mut h = Harness::new(MemoryStorage::new());
    // Every widget of the pack's, not only the books': values such as
    // `widgets.ring.one` and `.two` look like plural forms to the
    // comparison of the catalogs, which would not miss one.
    let groups: Vec<String> = h
        .widgets
        .iter()
        .map(|(id, _)| id.as_str().to_string())
        .collect();
    for language in ["en", "de"] {
        h.i18n.set_language(language);
        for spec in h.settings.specs() {
            if !groups
                .iter()
                .any(|g| g == h.settings.group_of(spec).as_str())
            {
                continue;
            }
            let mut keys = vec![spec.title.to_string()];
            if let SettingKind::Choice(values) = &spec.kind {
                keys.extend(values.iter().map(|v| v.to_string()));
            }
            // The shared look's names come from the kernel.
            for key in keys.iter().filter(|k| !k.starts_with("core.")) {
                assert!(h.i18n.get(key).is_some(), "{language}: {key}");
            }
        }
        for id in IDS {
            assert!(h.i18n.get(h.widgets.title(id).unwrap()).is_some(), "{id}");
        }
    }
}

/// The settings whose names and values do not say all they do are
/// explained, in both languages.
#[test]
fn settings_that_need_it_are_explained() {
    let mut h = Harness::new(MemoryStorage::new());
    for language in ["en", "de"] {
        h.i18n.set_language(language);
        for id in [
            "widgets.quote.change",
            "widgets.picture.change",
            "widgets.highlight.change",
            "widgets.series.start",
            "widgets.series.name",
            "widgets.next.pick",
            "widgets.countdown.date",
            "widgets.picture.folder",
            "widgets.quote.source",
        ] {
            let spec = h.settings.spec(id).unwrap();
            let help = spec.description.as_deref().unwrap_or_default();
            assert!(h.i18n.get(help).is_some(), "{language}: {id}");
        }
    }
}

/// In small places as on a page of small cells, each book widget draws
/// inside its place only, in every style.
#[test]
fn widgets_keep_inside_small_places() {
    let mut h = Harness::new(library());
    let styles = STYLES
        .iter()
        .map(|&(id, key, value)| (id, Some((key, value))));
    for (id, style) in IDS.iter().map(|&id| (id, None)).chain(styles) {
        if let Some((key, value)) = style {
            let value = SettingValue::Text(Cow::Borrowed(value));
            h.set(&alloc::format!("{id}.{key}"), value);
        }
        for (w, h_) in [
            (90, 67),
            (197, 67),
            (411, 67),
            (90, 153),
            (197, 153),
            (74, 300),
        ] {
            let area = Rect::new(30, 30, w, h_);
            let env = h.env();
            let widget = env.widgets().make(id, &env).unwrap();
            let mut fb = Framebuffer::new(
                DEVICE.display.width,
                DEVICE.display.height,
                PixelFormat::Gray4,
            )
            .unwrap();
            {
                let mut canvas = fb.canvas();
                canvas.clear(Color::WHITE);
                widget.draw(&mut canvas, &env, area);
            }
            let view = fb.view();
            for y in 0..DEVICE.display.height {
                for x in 0..DEVICE.display.width {
                    let inside = x >= area.x
                        && x < area.x + area.width
                        && y >= area.y
                        && y < area.y + area.height;
                    assert!(
                        inside || view.pixel(x, y).luma() >= 128,
                        "{id} {style:?} {area:?}: ink at {x},{y}"
                    );
                }
            }
        }
    }
}

/// Lines break a title between words only: each line is whole words of
/// it, or ends shortened.
#[test]
fn titles_break_between_words_only() {
    use super::{Lines, Role};
    let h = Harness::new(MemoryStorage::new());
    let env = h.env();
    let style = paperpaw_sdk::ui::UiStyle::new(&env);
    let title = "Alice's Adventures in Wonderland, an Unexpectedly Long Title";
    let words: Vec<&str> = title.split_whitespace().collect();
    for width in [40u16, 60, 90, 120, 180, 260, 400] {
        for height in [30u16, 60, 120, 240] {
            let mut lines = Lines::new();
            lines.add(Role::Caption, style.small(), "Next up", Color::BLACK);
            lines.add(Role::Title, style.title(), title, Color::BLACK);
            lines.add(Role::Note, style.small(), "Ann Author", Color::BLACK);
            lines.fit(&env, width, height);
            let font = style.font(&env).unwrap();
            for line in &lines.lines {
                let text = line.text.as_ref();
                assert!(
                    font.measure(line.size, text) <= f32::from(width) + 1.0,
                    "{width}x{height}: {text:?} too wide"
                );
                if line.role == Role::Title && !text.ends_with('…') {
                    for word in text.split_whitespace() {
                        assert!(words.contains(&word), "{width}x{height}: {text:?}");
                    }
                }
            }
            // The title stays, and the caption goes before it.
            assert!(lines.lines.iter().any(|l| l.role == Role::Title));
        }
    }
}

/// Lines made to fit by way of a memo come out as made to fit without one,
/// the first time (worked out) and the next (recalled), in each room, and
/// the memo keeps no more than a few rooms.
#[test]
fn a_memo_gives_the_lines_made_to_fit_without_one() {
    use super::{FitMemo, Lines, Role};
    let h = Harness::new(MemoryStorage::new());
    let env = h.env();
    let style = paperpaw_sdk::ui::UiStyle::new(&env);
    let title = "Alice's Adventures in Wonderland, an Unexpectedly Long Title";
    let add = |lines: &mut Lines<'_>| {
        lines.add(Role::Caption, style.small(), "Next up", Color::BLACK);
        lines.add(Role::Title, style.title(), title, Color::BLACK);
        lines.add(Role::Note, style.small(), "Ann Author", Color::BLACK);
    };
    let said = |lines: &Lines<'_>| {
        lines
            .lines
            .iter()
            .map(|l| (l.role, l.size.to_bits(), l.text.to_string()))
            .collect::<Vec<_>>()
    };
    let memo = FitMemo::default();
    for round in 0..2 {
        for width in [40u16, 90, 180, 400] {
            for height in [30u16, 60, 120, 240] {
                let mut plain = Lines::new();
                add(&mut plain);
                plain.fit(&env, width, height);
                let mut memoed = Lines::remembering(&memo);
                add(&mut memoed);
                memoed.fit(&env, width, height);
                assert_eq!(said(&plain), said(&memoed), "{round}: {width}x{height}");
                assert_eq!(plain.height(&env), memoed.height(&env));
            }
        }
    }
}

/// A library of one's own gives the widgets its books, none of them on
/// the card.
#[test]
fn a_library_of_ones_own_gives_the_widgets_its_books() {
    struct Remote;

    impl library::Library for Remote {
        fn catalog_state(&self, _: &paperpaw_sdk::Env<'_>) -> catalog::State {
            catalog::State::Ready { stamp: 1 }
        }

        fn each(&self, _: &paperpaw_sdk::Env<'_>, f: &mut dyn FnMut(u64, catalog::Record)) {
            for (key, path) in ["remote/a.epub", "remote/b.epub"].iter().enumerate() {
                f(
                    key as u64,
                    catalog::Record::of(path, &library::Book::default()),
                );
            }
        }
    }

    let mut h = Harness::new(MemoryStorage::new());
    let remote: alloc::boxed::Box<dyn library::Library> = alloc::boxed::Box::new(Remote);
    h.services.insert(
        Id::new_static(library::service::SERVICE),
        paperpaw_sdk::services::Service::new(
            library::service::LibraryService(remote),
            library::service::VERSION,
        ),
    );
    let env = h.env();
    let mut paths = Vec::new();
    super::each_book(&env, &mut |record| paths.push(record.path));
    assert_eq!(paths, ["remote/a.epub", "remote/b.epub"]);
}
