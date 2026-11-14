//! Turning pages with the real kernel: N pages forward and N back is the
//! first page again (in the session, and after a new start where the
//! page index leads back), the last page stays and the book is finished
//! once, and the edges: an empty book, a book of one page, a book that is
//! missing or vanishes while open, a kept position that is broken or
//! points past the end, the last book reopened at start-up or not.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::*;
use paperpaw_sdk::Storage;
use paperpaw_sdk::builtin::actions;

const BOOK: &str = "books/numbered.txt";

fn numbered(device: paperpaw_sdk::DeviceInfo, storage: Shared) -> Reading {
    let text = numbered_book(120, 12);
    Reading::new(device, storage, BOOK, text.as_bytes(), "")
}

#[test]
fn n_pages_forward_and_n_back_is_the_first_page_again() {
    let mut reading = numbered(BUTTONS, Shared::default());
    reading.open();
    let first = reading.shown().at();
    for n in [1, 2, 5, 9, 17] {
        let mut starts = vec![first];
        for _ in 0..n {
            reading.press(actions::NEXT_PAGE);
            starts.push(reading.shown().at());
        }
        // Forward: every page further on.
        assert!(starts.windows(2).all(|p| p[0] < p[1]), "{n}: {starts:?}");
        // Back: the same pages in reverse.
        for back in (0..n).rev() {
            reading.press(actions::PREV_PAGE);
            assert_eq!(reading.shown().at(), starts[back], "{n} back to {back}");
        }
        assert_eq!(reading.shown().at(), first);
    }
}

#[test]
fn going_back_after_a_new_start_follows_the_page_index() {
    let storage = Shared::default();
    let mut reading = numbered(BUTTONS, storage.clone());
    reading.open();
    let mut starts = vec![reading.shown().at()];
    for _ in 0..7 {
        reading.press(actions::NEXT_PAGE);
        starts.push(reading.shown().at());
    }
    reading.kept();
    // Another start: no pages remembered from before, only the index.
    let mut again = numbered(BUTTONS, storage);
    again.open();
    assert_eq!(again.shown().at(), *starts.last().unwrap());
    for back in (0..7).rev() {
        again.press(actions::PREV_PAGE);
        assert_eq!(again.shown().at(), starts[back], "back to {back}");
    }
    // Not before the first page.
    let turned = again.turned();
    again.press(actions::PREV_PAGE);
    assert_eq!(again.turned(), turned);
}

#[test]
fn the_last_page_stays_and_the_book_is_finished_once() {
    let text = numbered_book(8, 12);
    let mut reading = Reading::new(BUTTONS, Shared::default(), BOOK, text.as_bytes(), "");
    reading.open();
    let mut starts = vec![reading.shown().at()];
    for _ in 0..30 {
        reading.press(actions::NEXT_PAGE);
        starts.push(reading.shown().at());
    }
    starts.dedup();
    let pages = starts.len();
    assert!((2..30).contains(&pages), "{pages} pages");
    // A page turned for each page after the first, none past the last.
    assert_eq!(reading.turned(), pages - 1);
    let finished = reading.events();
    let finished = finished.iter().filter(|e| **e == "finished").count();
    assert_eq!(finished, 1);
    let last = reading
        .last_event("finished")
        .map(|p| reading_of(&p))
        .unwrap();
    assert_eq!(last.at(), *starts.last().unwrap());
    assert!(last.progress > 50.0, "{}", last.progress);
    // Back from the last page goes back one.
    reading.press(actions::PREV_PAGE);
    assert_eq!(reading.shown().at(), starts[pages - 2]);
}

#[test]
fn an_empty_book_opens_and_turns_nowhere() {
    let mut reading = Reading::new(BUTTONS, Shared::default(), BOOK, b"", "");
    reading.open();
    for action in [
        actions::NEXT_PAGE,
        actions::PREV_PAGE,
        paperpaw_plugin_reader::actions::SELECT,
        actions::CONFIRM,
        actions::BACK,
        paperpaw_plugin_reader::actions::LINKS,
        actions::NEXT_PAGE,
        actions::BACK,
        paperpaw_plugin_reader::actions::TOGGLE_BOOKMARK,
        paperpaw_plugin_reader::actions::FONT_LARGER,
    ] {
        reading.press(action);
    }
    assert_eq!(reading.turned(), 0);
    assert_eq!(reading.screen(), "reader.screen");
    reading.render();
}

#[test]
fn a_book_of_one_page_does_not_turn() {
    let mut reading = Reading::new(
        BUTTONS,
        Shared::default(),
        BOOK,
        b"Just one short page.\n",
        "",
    );
    reading.open();
    let first = reading.shown();
    for _ in 0..3 {
        reading.press(actions::NEXT_PAGE);
        reading.press(actions::PREV_PAGE);
    }
    assert_eq!(reading.turned(), 0);
    assert_eq!(reading.shown().at(), first.at());
    // The only page is the whole book.
    assert_eq!(first.progress, 0.0);
}

#[test]
fn a_missing_book_says_so_and_keeps_nothing() {
    let storage = Shared::default();
    let mut reading = Reading::on(BUTTONS, storage.clone(), "books/gone.epub", "", false);
    reading.open();
    for action in [
        actions::NEXT_PAGE,
        actions::PREV_PAGE,
        paperpaw_plugin_reader::actions::SELECT,
        paperpaw_plugin_reader::actions::TOC,
        paperpaw_plugin_reader::actions::SEARCH,
        paperpaw_plugin_reader::actions::SKIM_BOOK,
        paperpaw_plugin_reader::actions::AUTO_TURN,
    ] {
        reading.press(action);
    }
    reading.render();
    // Nothing heard of a book that is not there, nothing kept of it.
    assert!(reading.events().iter().all(|e| *e != "opened"));
    assert!(!storage.exists(&reading.progress_path()));
    assert!(!storage.exists(".paperpaw/plugins/reader/state.ini"));
    // The message drawn has ink.
    let ink = reading.image.iter().flatten().filter(|&&l| l < 128).count();
    assert!(ink > 100, "{ink}");
}

#[test]
fn a_book_that_vanishes_while_open_does_not_break_the_reader() {
    let storage = Shared::default();
    let mut reading = numbered(BUTTONS, storage.clone());
    reading.open();
    reading.press(actions::NEXT_PAGE);
    storage.remove(BOOK).unwrap();
    for action in [
        actions::NEXT_PAGE,
        actions::NEXT_PAGE,
        actions::PREV_PAGE,
        paperpaw_plugin_reader::actions::FONT_LARGER,
        paperpaw_plugin_reader::actions::SELECT,
        actions::CONFIRM,
        actions::BACK,
    ] {
        reading.press(action);
    }
    reading.render();
    reading.kept();
}

/// Kept positions that cannot be used: the book opens at its start.
#[test]
fn a_broken_position_opens_the_book_at_its_start() {
    let broken: [&[u8]; 9] = [
        b"",
        b"garbage \xff\xfe without any keys",
        b"[progress]\nsection = 0\n",
        b"[progress]\noffset = 10\n",
        b"[progress]\nsection = 999\noffset = 0\n",
        b"[progress]\nsection = 0\noffset = 4000000000\n",
        b"[progress]\nsection = -1\noffset = -5\n",
        b"[progress]\nsection = 0\noffset = 99999999999999999999\n",
        b"[progress\nsection = 0\noffset = 1x\n",
    ];
    // Where the book starts without any position.
    let mut fresh = numbered(BUTTONS, Shared::default());
    fresh.open();
    let start = fresh.shown().at();
    for data in broken {
        let storage = Shared::default();
        let mut reading = numbered(BUTTONS, storage.clone());
        storage
            .create_dir(&paperpaw_plugin_reader_dir(BOOK))
            .unwrap();
        storage.write(&reading.progress_path(), data).unwrap();
        reading.open();
        let text = String::from_utf8_lossy(data);
        assert_eq!(reading.shown().at(), start, "{text}");
        // And it is read on as usual.
        reading.press(actions::NEXT_PAGE);
        assert!(reading.shown().at() > start, "{text}");
        assert_eq!(
            reading.kept(),
            (reading.shown().section, reading.shown().offset)
        );
    }
}

/// A position inside a line (kept with another layout) continues at the
/// start of that line, never after it.
#[test]
fn a_position_inside_a_line_continues_at_its_start() {
    let storage = Shared::default();
    let mut reading = numbered(BUTTONS, storage.clone());
    reading.open();
    for _ in 0..3 {
        reading.press(actions::NEXT_PAGE);
    }
    let third = reading.shown().at();
    reading.press(actions::NEXT_PAGE);
    let fourth = reading.shown().at();
    assert_eq!(third.section, fourth.section);
    reading.kept();
    let inside = third.offset + (fourth.offset - third.offset) / 2 + 3;
    let progress = format!(
        "[progress]\nsection = {}\noffset = {inside}\n",
        third.section
    );
    storage
        .write(&reading.progress_path(), progress.as_bytes())
        .unwrap();
    let mut again = numbered(BUTTONS, storage);
    again.open();
    let at = again.shown().at();
    assert!(
        at.offset <= inside && at.offset > third.offset,
        "{at:?} for {inside}"
    );
}

/// A page index that is damaged (out of order, past the end of the book,
/// cut, or garbage marked complete): pages still turn both ways and stay
/// in the book.
#[test]
fn a_damaged_page_index_does_not_lose_the_book() {
    let storage = Shared::default();
    let mut reading = numbered(BUTTONS, storage.clone());
    reading.open();
    reading.press(actions::NEXT_PAGE);
    reading.press(actions::NEXT_PAGE);
    reading.kept();
    let dir = paperpaw_plugin_reader_dir(BOOK);
    let mut indexes = Vec::new();
    storage
        .list(&dir, &mut |e: paperpaw_sdk::DirEntry<'_>| {
            if e.name.starts_with("pages-") {
                indexes.push(format!("{dir}/{}", e.name));
            }
        })
        .unwrap();
    assert_eq!(indexes.len(), 1, "{indexes:?}");
    let entry = |section: u32, offset: u32| {
        let mut bytes = section.to_le_bytes().to_vec();
        bytes.extend_from_slice(&offset.to_le_bytes());
        bytes
    };
    let damaged: [Vec<u8>; 4] = [
        // Out of order, then complete.
        [
            entry(0, 900),
            entry(0, 0),
            entry(0, 400),
            entry(u32::MAX, u32::MAX),
        ]
        .concat(),
        // Far past the end of the book.
        [
            entry(0, 0),
            entry(7, 5),
            entry(0, 4_000_000_000),
            entry(u32::MAX, u32::MAX),
        ]
        .concat(),
        // Cut inside an entry.
        [entry(0, 0), entry(0, 300)].concat()[..13].to_vec(),
        // Noise.
        (0..203u32).map(|i| (i * 37 % 251) as u8).collect(),
    ];
    for data in damaged {
        storage.write(&indexes[0], &data).unwrap();
        let mut again = numbered(BUTTONS, storage.clone());
        again.open();
        let at = again.shown().at();
        for _ in 0..4 {
            again.press(actions::PREV_PAGE);
        }
        for _ in 0..3 {
            again.press(actions::NEXT_PAGE);
        }
        // Wherever it went, a page of the book.
        let shown = again.shown();
        assert!(
            shown.section == 0 && shown.progress < 100.0,
            "{shown:?} from {at:?}"
        );
        again.press(actions::NEXT_PAGE);
        assert!(again.shown().at() > shown.at());
    }
}

#[test]
fn the_last_book_is_opened_again_at_start_up() {
    let storage = Shared::default();
    let mut reading = numbered(BUTTONS, storage.clone());
    reading.open();
    reading.press(actions::NEXT_PAGE);
    let left = reading.shown().at();
    reading.kept();
    let again = numbered(BUTTONS, storage);
    assert_eq!(again.screen(), "reader.screen");
    assert_eq!(again.shown().at(), left);
}

#[test]
fn a_last_book_that_is_gone_is_not_opened_at_start_up() {
    let storage = Shared::default();
    storage.create_dir(".paperpaw/plugins/reader").unwrap();
    storage
        .write(
            ".paperpaw/plugins/reader/state.ini",
            b"[reader]\nlast = \"books/gone.txt\"\n",
        )
        .unwrap();
    let reading = Reading::on(BUTTONS, storage, "books/gone.txt", "", false);
    assert_ne!(reading.screen(), "reader.screen");
    assert!(reading.events().is_empty());
}

#[test]
fn the_last_book_is_not_opened_again_when_switched_off() {
    let storage = Shared::default();
    let mut reading = numbered(BUTTONS, storage.clone());
    reading.open();
    reading.kept();
    let again = Reading::on(
        BUTTONS,
        storage,
        BOOK,
        "[reader]\nopen-last = false\n",
        false,
    );
    assert_ne!(again.screen(), "reader.screen");
}

/// A state file that is no ini, or names no book: nothing opens, nothing
/// breaks.
#[test]
fn a_broken_state_file_opens_nothing() {
    for data in [
        &b"\xff\xfe\x00garbage"[..],
        b"[reader]\nlast =\n",
        b"[reader]\nlast = \"\"\n",
        b"[reader]\nlast = \"../../etc/passwd\"\n",
        b"[other]\nlast = \"books/numbered.txt\"\n",
    ] {
        let storage = Shared::default();
        storage.create_dir(".paperpaw/plugins/reader").unwrap();
        storage
            .write(".paperpaw/plugins/reader/state.ini", data)
            .unwrap();
        let reading = Reading::on(BUTTONS, storage, BOOK, "", false);
        assert_ne!(
            reading.screen(),
            "reader.screen",
            "{}",
            String::from_utf8_lossy(data)
        );
    }
}

#[test]
fn larger_then_smaller_text_comes_back_to_the_same_place() {
    let mut reading = numbered(BUTTONS, Shared::default());
    reading.open();
    for _ in 0..6 {
        reading.press(actions::NEXT_PAGE);
    }
    let before = reading.shown().at();
    for _ in 0..3 {
        reading.run(paperpaw_plugin_reader::actions::FONT_LARGER);
    }
    for _ in 0..3 {
        reading.run(paperpaw_plugin_reader::actions::FONT_SMALLER);
    }
    let after = reading.kept();
    // Each step starts the page at the start of the line the place is
    // in: at most a few lines before it.
    assert_eq!(after.0, before.section);
    assert!(
        after.1 <= before.offset && after.1 + 600 > before.offset,
        "{after:?} for {before:?}"
    );
    // And the pages go on from there.
    reading.press(actions::NEXT_PAGE);
    assert!(reading.shown().at() > before);
}

#[test]
#[ignore = "needs native activation (format removal)"]
fn pages_that_go_right_to_left_turn_on_with_a_tap_on_the_left() {
    let book = epub_with(
        &[("c1.xhtml", filler("manga", 6))],
        &[],
        r#" page-progression-direction="rtl""#,
    );
    let mut reading = Reading::new(TOUCH, Shared::default(), "books/manga.epub", &book, "");
    reading.open();
    let first = reading.shown().offset;
    // The left side turns on, the right side back.
    reading.tap(paperpaw_sdk::Point::new(30, 400));
    assert_eq!(reading.turned(), 1);
    assert!(reading.shown().offset > first);
    reading.tap(paperpaw_sdk::Point::new(450, 400));
    assert_eq!(reading.shown().offset, first);
}

#[test]
#[ignore = "needs native activation (format removal)"]
fn pages_that_go_left_to_right_turn_on_with_a_tap_on_the_right() {
    let book = epub(&[("c1.xhtml", filler("novel", 6))], &[]);
    let mut reading = Reading::new(TOUCH, Shared::default(), "books/novel.epub", &book, "");
    reading.open();
    let first = reading.shown().offset;
    reading.tap(paperpaw_sdk::Point::new(450, 400));
    assert!(reading.shown().offset > first);
}

#[test]
#[ignore = "needs native activation (format removal)"]
fn a_swipe_in_from_the_left_edge_turns_on_where_pages_go_right_to_left() {
    use paperpaw_platform::PointerPhase;
    let book = epub_with(
        &[("c1.xhtml", filler("manga", 6))],
        &[],
        r#" page-progression-direction="rtl""#,
    );
    let mut reading = Reading::new(TOUCH, Shared::default(), "books/manga.epub", &book, "");
    reading.open();
    let first = reading.shown().offset;
    // From the left edge in, as a swipe further in would: on.
    reading.now += 1000;
    reading.finger(PointerPhase::Down, paperpaw_sdk::Point::new(2, 400));
    reading.finger(PointerPhase::Move, paperpaw_sdk::Point::new(120, 400));
    reading.finger(PointerPhase::Move, paperpaw_sdk::Point::new(240, 400));
    reading.finger(PointerPhase::Up, paperpaw_sdk::Point::new(240, 400));
    assert!(reading.shown().offset > first, "{}", reading.shown().offset);
}

/// A page index that counts a page too many (made when a letter measured
/// otherwise, before a font pack came): the turn past the last real page
/// finds nothing, the book stays on its last page, and the pages are
/// counted again, the count right after.
#[test]
fn a_turn_past_the_last_page_counts_the_pages_again_when_the_index_says_more() {
    let storage = Shared::default();
    let mut reading = numbered(BUTTONS, storage.clone());
    reading.open();
    reading.kept();
    let dir = paperpaw_plugin_reader_dir(BOOK);
    let mut indexes = Vec::new();
    storage
        .list(&dir, &mut |e: paperpaw_sdk::DirEntry<'_>| {
            if e.name.starts_with("pages-") {
                indexes.push(format!("{dir}/{}", e.name));
            }
        })
        .unwrap();
    assert_eq!(indexes.len(), 1, "{indexes:?}");
    let right = paperpaw_sdk::read_file(&storage, &indexes[0], 1 << 20).unwrap();
    // The last page of the book, as the turns lead there.
    let mut last = reading.shown().at();
    loop {
        reading.press(actions::NEXT_PAGE);
        let now = reading.shown().at();
        if now == last {
            break;
        }
        last = now;
    }
    // One page more in a complete index: after the last real start.
    let mut wrong = right[..right.len() - 8].to_vec();
    wrong.extend_from_slice(&last.section.to_le_bytes());
    wrong.extend_from_slice(&(last.offset + 1).to_le_bytes());
    wrong.extend_from_slice(&right[right.len() - 8..]);
    assert_eq!(wrong.len(), right.len() + 8);
    storage.write(&indexes[0], &wrong).unwrap();
    let mut again = numbered(BUTTONS, storage.clone());
    again.open();
    let mut at = again.shown().at();
    loop {
        again.press(actions::NEXT_PAGE);
        let now = again.shown().at();
        if now == at {
            break;
        }
        at = now;
    }
    assert_eq!(at, last);
    // The wrong count is gone, the right one is made again; the turn that
    // found no page says nothing: no page turned, no book finished again.
    let heard = again.events();
    again.press(actions::NEXT_PAGE);
    assert_eq!(again.shown().at(), last);
    assert_eq!(again.events(), heard);
    assert_eq!(
        heard.iter().filter(|e| **e == "finished").count(),
        1,
        "{heard:?}"
    );
    let mended = paperpaw_sdk::read_file(&storage, &indexes[0], 1 << 20).unwrap();
    assert_eq!(mended, right);
}
