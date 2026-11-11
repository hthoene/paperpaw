//! Moving through a book with the real kernel: the table of contents (a
//! book's own, or one made from its headings), searching (a hit, none,
//! too many, the last search offered again), going to a page number,
//! links and footnotes (over the page or jumped to, and back), skimming,
//! the status lines switched and kept, and the list of bookmarks and
//! highlights with its export. *Back* returns from each jump.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::*;
use paperpaw_plugin_reader::actions as reader;
use paperpaw_sdk::builtin::{actions, params};
use paperpaw_sdk::document::Locator;
use paperpaw_sdk::{DirEntry, Storage};

const ALICE: &[u8] = include_bytes!("../../../samples/library/books/alice-in-wonderland.epub");

/// The focused part's label.
fn focused(reading: &mut Reading) -> String {
    reading.render();
    let parts = reading.kernel.describe();
    paperpaw_sdk::semantics::focused(&parts)
        .map(|p| p.label.clone())
        .unwrap_or_default()
}

#[test]
fn the_contents_list_the_chapters_and_lead_to_one_and_back() {
    let mut reading = Reading::new(BUTTONS, Shared::default(), "books/alice.epub", ALICE, "");
    reading.open();
    let start = reading.shown();
    reading.press(reader::TOC);
    assert_eq!(reading.screen(), "reader.toc");
    for _ in 0..4 {
        reading.press(actions::DOWN);
    }
    let chosen = focused(&mut reading);
    assert!(!chosen.is_empty());
    reading.press(actions::CONFIRM);
    assert_eq!(reading.screen(), "reader.screen");
    let there = reading.shown();
    assert!(there.at() > start.at());
    assert_eq!(there.chapter, chosen);
    // The contents again: the chapter shown has the focus.
    reading.press(reader::TOC);
    assert_eq!(focused(&mut reading), chosen);
    reading.press(actions::BACK);
    assert_eq!(reading.screen(), "reader.screen");
    // *Back* in the book: where it was before the jump.
    reading.press(actions::BACK);
    assert_eq!(reading.shown().at(), start.at());
    assert_eq!(reading.screen(), "reader.screen");
}

/// A book with one entry in its contents and headings in its text.
fn headed() -> Vec<u8> {
    let mut chapters = Vec::new();
    for (i, title) in ["Arrival", "The Garden", "Departure"].iter().enumerate() {
        chapters.push((
            format!("ch{i}.xhtml"),
            format!(
                "<h1>{title}</h1>{}<h2>Part of {title}</h2>{}",
                filler(title, 2),
                filler("more", 1)
            ),
        ));
    }
    let chapters: Vec<(&str, String)> = chapters
        .iter()
        .map(|(n, b)| (n.as_str(), b.clone()))
        .collect();
    epub(&chapters, &[("Everything", "ch0.xhtml")])
}

#[test]
fn a_book_with_a_short_contents_gets_one_from_its_headings() {
    let storage = Shared::default();
    let book = "books/headed.epub";
    let mut reading = Reading::new(BUTTONS, storage.clone(), book, &headed(), "");
    // A list made from another file of that name: made again.
    let headings = format!("{}/headings.txt", paperpaw_plugin_reader_dir(book));
    storage
        .create_dir(&paperpaw_plugin_reader_dir(book))
        .unwrap();
    storage
        .write(&headings, b"size 1\n0\t0\t0\tBogus\n0\t0\t0\tStale\n")
        .unwrap();
    reading.open();
    reading.press(reader::TOC);
    let shown = reading.described();
    for title in ["Arrival", "Part of Arrival", "The Garden", "Departure"] {
        assert!(shown.contains(title), "{title}: {shown}");
    }
    assert!(!shown.contains("Bogus"), "{shown}");
    // Kept for next time, with the book's size.
    let kept = storage.text(&headings).unwrap();
    let size = storage.size(book).unwrap();
    assert!(kept.starts_with(&format!("size {size}\n")), "{kept}");
    assert_eq!(kept.lines().count(), 7, "{kept}");
    // Choosing *The Garden* goes to its chapter.
    while focused(&mut reading) != "The Garden" {
        reading.press(actions::DOWN);
    }
    reading.press(actions::CONFIRM);
    assert_eq!(reading.screen(), "reader.screen");
    assert_eq!(reading.shown().section, 1);
    // A second look reads the list kept.
    reading.press(reader::TOC);
    assert!(reading.described().contains("Departure"));
}

#[test]
fn a_book_without_contents_or_headings_says_so() {
    let text = numbered_book(30, 12);
    let mut reading = Reading::new(
        BUTTONS,
        Shared::default(),
        "books/n.txt",
        text.as_bytes(),
        "",
    );
    reading.open();
    reading.press(reader::TOC);
    assert!(reading.described().contains("no table of contents"));
    // Nothing to choose: confirm stays.
    reading.press(actions::CONFIRM);
    assert_eq!(reading.screen(), "reader.toc");
}

/// The place of the first `needle` in the book, as its document has it.
fn place_of(reading: &Reading, needle: &str) -> Locator {
    let document = reading
        .kernel
        .formats()
        .open(&reading.storage, &reading.book)
        .unwrap();
    let mut at = Locator::START;
    while at.section < document.sections() {
        match document.block_at(&reading.storage, at).unwrap() {
            Some(block) => {
                if let Some(i) = block.text.find(needle) {
                    return Locator::new(at.section, block.source_offset(i));
                }
                at = Locator::new(at.section, block.end.max(at.offset + 1));
            }
            None => at = Locator::new(at.section + 1, 0),
        }
    }
    panic!("{needle} not in the book");
}

/// A book with `Zebra` once, far in.
fn zebra() -> Reading {
    let text = numbered_book(300, 12).replace("word250x5 ", "Zebra ");
    let mut reading = Reading::new(
        BUTTONS,
        Shared::default(),
        "books/n.txt",
        text.as_bytes(),
        "",
    );
    reading.open();
    reading
}

#[test]
fn a_search_finds_the_word_and_its_page_and_back_returns() {
    let mut reading = zebra();
    let start = reading.shown().at();
    reading.press(reader::SEARCH);
    assert_eq!(reading.screen(), "recorder.keyboard");
    reading.type_text("  zEBRA ");
    assert_eq!(reading.screen(), "reader.search-results");
    let shown = reading.described();
    assert!(shown.contains("1 hit"), "{shown}");
    // The row with the text around the hit, and how far into the book.
    assert!(shown.contains("word250x4"), "{shown}");
    assert!(shown.contains("82 %"), "{shown}");
    reading.press(actions::CONFIRM);
    assert_eq!(reading.screen(), "reader.screen");
    // The page has the word: it starts before it, the next one after.
    let hit = place_of(&reading, "Zebra");
    let page = reading.shown().at();
    assert!(page <= hit, "{page:?} for {hit:?}");
    reading.press(actions::NEXT_PAGE);
    assert!(
        reading.shown().at() > hit,
        "{:?} for {hit:?}",
        reading.shown().at()
    );
    reading.press(actions::PREV_PAGE);
    assert_eq!(reading.shown().at(), page);
    // *Back*: where the search began.
    reading.press(actions::BACK);
    assert_eq!(reading.shown().at(), start);
    // The last search is offered again.
    reading.press(reader::SEARCH);
    let asked = reading.last_slot("keyboard").unwrap();
    assert_eq!(asked.get(params::TEXT), Some("zEBRA"));
}

/// On a narrow screen (the X4's 480 pixels) the row of a hit begins
/// late enough for the word found to be in it.
#[test]
fn a_search_hit_shows_the_word_found() {
    let mut reading = zebra();
    reading.press(reader::SEARCH);
    reading.type_text("zebra");
    let shown = reading.described();
    assert!(shown.contains("Zebra"), "{shown}");
}

/// A match wider than its row shows from its start, not its middle.
#[test]
fn a_search_hit_wider_than_its_row_shows_its_start() {
    let mut reading = zebra();
    reading.press(reader::SEARCH);
    reading.type_text("p250 word250x1 word250x2 word250x3 word250x4 zebra word250x6 word250x7");
    let shown = reading.described();
    assert!(shown.contains("…p250 word250x1"), "{shown}");
}

#[test]
fn a_search_without_hits_says_so_and_stays() {
    let mut reading = zebra();
    let start = reading.shown().at();
    reading.press(reader::SEARCH);
    reading.type_text("quagga");
    let shown = reading.described();
    assert!(shown.contains("Nothing found"), "{shown}");
    reading.press(actions::CONFIRM);
    assert_eq!(reading.screen(), "reader.search-results");
    reading.press(actions::BACK);
    assert_eq!(reading.screen(), "reader.screen");
    assert_eq!(reading.shown().at(), start);
}

#[test]
fn an_empty_search_searches_nothing() {
    let mut reading = zebra();
    reading.press(reader::SEARCH);
    reading.type_text("   ");
    assert_eq!(reading.screen(), "reader.screen");
}

#[test]
fn a_search_with_many_hits_keeps_a_hundred() {
    let mut reading = zebra();
    reading.press(reader::SEARCH);
    reading.type_text("word");
    let shown = reading.described();
    assert!(shown.contains("More than 100 hits"), "{shown}");
    // The last of them is reached and leads into the book.
    for _ in 0..99 {
        reading.press(actions::DOWN);
    }
    reading.press(actions::CONFIRM);
    assert_eq!(reading.screen(), "reader.screen");
    assert!(reading.shown().at() > Locator::START);
}

#[test]
fn going_to_a_page_number() {
    let mut reading = zebra();
    let mut starts = vec![reading.shown().at()];
    for _ in 0..3 {
        reading.press(actions::NEXT_PAGE);
        starts.push(reading.shown().at());
    }
    let pages = reading.shown().pages.expect("pages counted");
    let go = |reading: &mut Reading, typed: &str| {
        reading.press(reader::GO_TO_PAGE);
        let asked = reading.last_slot("keyboard").unwrap();
        assert_eq!(asked.get(params::MODE), Some("number"));
        reading.type_text(typed);
        assert_eq!(reading.screen(), "reader.screen");
        reading.shown()
    };
    assert_eq!(go(&mut reading, "2").at(), starts[1]);
    assert_eq!(go(&mut reading, " 4 ").at(), starts[3]);
    // Below the first and past the last: the first and the last.
    assert_eq!(go(&mut reading, "0").at(), starts[0]);
    let last = go(&mut reading, "4294967295");
    assert_eq!(last.page, Some(pages));
    // No number: nothing happens.
    for typed in ["", "abc", "-3", "99999999999999999999", "1.5"] {
        assert_eq!(go(&mut reading, typed).at(), last.at(), "{typed:?}");
    }
    // *Back* returns from the jumps, one by one.
    reading.press(actions::BACK);
    assert_eq!(reading.shown().at(), starts[0]);
}

/// A book with a note mark in its first chapter, the note in the last.
fn noted() -> Vec<u8> {
    let first = format!(
        "<p>The text has a note<a href=\"notes.xhtml#n1\">1</a> and a link that goes \
         <a href=\"missing.xhtml#x\">nowhere at all</a>.</p>{}",
        filler("first", 2)
    );
    let notes = format!(
        "{}<p id=\"n1\">1. The note says hello.</p>{}",
        filler("before", 1),
        filler("after", 1)
    );
    epub(
        &[("one.xhtml", first), ("notes.xhtml", notes)],
        &[("One", "one.xhtml"), ("Notes", "notes.xhtml")],
    )
}

#[test]
fn a_note_shows_over_the_page_and_leads_to_itself_and_back() {
    let mut reading = Reading::new(BUTTONS, Shared::default(), "books/noted.epub", &noted(), "");
    reading.open();
    let start = reading.shown().at();
    reading.press(reader::LINKS);
    let shown = reading.described();
    assert!(shown.contains("In this book"), "{shown}");
    reading.press(actions::CONFIRM);
    assert_eq!(reading.screen(), "reader.footnote");
    let shown = reading.described();
    assert!(shown.contains("The note says hello."), "{shown}");
    // *Close* (focused first) closes it, staying here.
    assert_eq!(focused(&mut reading), "Close");
    reading.press(actions::CONFIRM);
    assert_eq!(reading.screen(), "reader.screen");
    assert_eq!(reading.shown().at(), start);
    // *Go to the note*: there, and *Back* returns.
    reading.press(reader::LINKS);
    reading.press(actions::CONFIRM);
    reading.press(actions::RIGHT);
    assert_eq!(focused(&mut reading), "Go to the note");
    reading.press(actions::CONFIRM);
    assert_eq!(reading.screen(), "reader.screen");
    let note = reading.shown();
    assert_eq!(note.section, 1);
    assert_eq!(note.chapter, "Notes");
    reading.press(actions::BACK);
    assert_eq!(reading.shown().at(), start);
}

#[test]
fn notes_can_be_jumped_to_at_once() {
    let mut reading = Reading::new(
        BUTTONS,
        Shared::default(),
        "books/noted.epub",
        &noted(),
        "[reader]\nfootnotes = core.footnotes.jump\n",
    );
    reading.open();
    reading.press(reader::LINKS);
    reading.press(actions::CONFIRM);
    assert_eq!(reading.screen(), "reader.screen");
    assert_eq!(reading.shown().section, 1);
}

#[test]
fn a_link_that_leads_nowhere_does_nothing() {
    let mut reading = Reading::new(BUTTONS, Shared::default(), "books/noted.epub", &noted(), "");
    reading.open();
    let start = reading.shown().at();
    reading.press(reader::LINKS);
    reading.press(actions::RIGHT);
    let turned = reading.turned();
    reading.press(actions::CONFIRM);
    assert_eq!(reading.screen(), "reader.screen");
    assert_eq!(reading.turned(), turned);
    assert_eq!(reading.shown().at(), start);
}

#[test]
fn choosing_links_goes_on_over_the_pages() {
    let mut reading = Reading::new(BUTTONS, Shared::default(), "books/noted.epub", &noted(), "");
    reading.open();
    let start = reading.shown().at();
    reading.press(reader::LINKS);
    // Past the last link of the page: the next page.
    reading.press(actions::RIGHT);
    reading.press(actions::RIGHT);
    assert_eq!(reading.turned(), 1);
    // A page without links says so; confirm does nothing there.
    let shown = reading.described();
    assert!(shown.contains("No links"), "{shown}");
    reading.press(actions::CONFIRM);
    assert_eq!(reading.screen(), "reader.screen");
    // Back from there goes back a page, to its last link.
    reading.press(reader::LINKS);
    reading.press(actions::LEFT);
    assert_eq!(reading.shown().at(), start);
    reading.press(actions::BACK);
    // Choosing has ended: the page buttons turn pages again.
    reading.press(actions::NEXT_PAGE);
    assert_eq!(reading.turned(), 3);
}

#[test]
fn skimming_moves_by_chapters_and_pages_and_back() {
    let mut reading = Reading::new(BUTTONS, Shared::default(), "books/alice.epub", ALICE, "");
    reading.open();
    for _ in 0..3 {
        reading.press(actions::NEXT_PAGE);
    }
    let start = reading.shown();
    reading.press(reader::SKIM_BOOK);
    assert_eq!(reading.screen(), "reader.skim-screen");
    let shown = reading.described();
    for label in ["Chapter", "Where I was", "Page number"] {
        assert!(shown.contains(label), "{label}: {shown}");
    }
    // A chapter on, and another.
    reading.press(actions::RIGHT);
    let next = reading.shown();
    assert_ne!(next.chapter, start.chapter);
    reading.press(actions::RIGHT);
    let after = reading.shown();
    assert!(after.at() > next.at());
    // A chapter back from a chapter's start: the one before.
    reading.press(actions::LEFT);
    assert_eq!(reading.shown().at(), next.at());
    // Ten pages on, once they are counted.
    let page = next.page.expect("pages counted");
    reading.press(actions::NEXT_PAGE);
    let on = reading.shown();
    assert_eq!(on.page, Some(page + 10));
    // A chapter back from inside one: its start.
    reading.press(actions::LEFT);
    let back = reading.shown();
    assert!(back.at() < on.at());
    assert_eq!(back.chapter, on.chapter);
    // Back returns to where skimming began, and ends it.
    reading.press(actions::BACK);
    assert_eq!(reading.screen(), "reader.screen");
    assert_eq!(reading.shown().at(), start.at());
}

#[test]
fn skimming_with_buttons_shows_where_the_focus_is() {
    let mut reading = Reading::new(BUTTONS, Shared::default(), "books/alice.epub", ALICE, "");
    reading.open();
    reading.press(reader::SKIM_BOOK);
    // At first the bar: left and right move along it, confirm stays.
    let bar = focused(&mut reading);
    assert!(!bar.is_empty() && !bar.contains("Chapter"), "{bar}");
    // Down to the buttons, up again to the bar.
    reading.press(actions::DOWN);
    let first = focused(&mut reading);
    assert!(first.contains("Chapter"), "{first}");
    reading.press(actions::UP);
    assert_eq!(focused(&mut reading), bar);
    // Up from the bar: the last button, down from it the bar.
    reading.press(actions::UP);
    let last = focused(&mut reading);
    assert!(last.contains("Page number"), "{last}");
    reading.press(actions::DOWN);
    reading.press(actions::RIGHT);
    // On the bar, confirm ends skimming where it is.
    let here = reading.shown();
    reading.press(actions::CONFIRM);
    assert_eq!(reading.screen(), "reader.screen");
    assert_eq!(reading.shown().at(), here.at());
}

#[test]
fn skimming_ended_where_it_is_keeps_the_way_back() {
    let mut reading = Reading::new(BUTTONS, Shared::default(), "books/alice.epub", ALICE, "");
    reading.open();
    let start = reading.shown();
    reading.press(reader::SKIM_BOOK);
    reading.press(actions::RIGHT);
    reading.press(actions::RIGHT);
    let there = reading.shown().at();
    // The bar has the focus at first (where one is): Confirm stays here.
    let bar = focused(&mut reading);
    assert!(bar.contains(" / ") && !bar.contains("Chapter"), "{bar}");
    reading.press(actions::CONFIRM);
    assert_eq!(reading.screen(), "reader.screen");
    assert_eq!(reading.shown().at(), there);
    reading.press(actions::BACK);
    assert_eq!(reading.shown().at(), start.at());
}

/// The ink in the strip of the status line at the bottom.
fn bottom_ink(reading: &mut Reading) -> usize {
    let image = reading.snapshot();
    let height = image.len();
    image[height * 93 / 100..]
        .iter()
        .flatten()
        .filter(|&&l| l < 160)
        .count()
}

#[test]
fn the_status_lines_switch_and_the_choice_is_kept() {
    let storage = Shared::default();
    let text = numbered_book(60, 12);
    let mut reading = Reading::new(BUTTONS, storage.clone(), "books/n.txt", text.as_bytes(), "");
    reading.open();
    let start = reading.shown().at();
    let choice = ".paperpaw/plugins/reader/status-lines.txt";
    let mut seen = Vec::new();
    for _ in 0..3 {
        reading.press(reader::NEXT_STATUS_LINE);
        let kept = storage.text(choice).unwrap();
        seen.push(kept.lines().nth(1).unwrap().to_string());
    }
    assert_eq!(
        seen,
        [
            "reader.status-line.progress",
            "reader.status-line.empty",
            "core.status-line.items"
        ]
    );
    // The page stays as it is: the lines take the same room.
    assert_eq!(reading.turned(), 0);
    assert_eq!(reading.shown().at(), start);
    // The empty line shows nothing, the others something.
    assert!(bottom_ink(&mut reading) > 0);
    reading.press(reader::NEXT_STATUS_LINE);
    reading.press(reader::NEXT_STATUS_LINE);
    assert_eq!(bottom_ink(&mut reading), 0);
    // Kept for the next start.
    let mut again = Reading::new(BUTTONS, storage, "books/n.txt", b"", "");
    again.open();
    assert_eq!(bottom_ink(&mut again), 0);
}

#[test]
fn a_broken_status_choice_shows_the_first_line() {
    let storage = Shared::default();
    storage.create_dir(".paperpaw/plugins/reader").unwrap();
    storage
        .write(
            ".paperpaw/plugins/reader/status-lines.txt",
            b"\xff\xfe nonsense\n../../etc\n\n\n",
        )
        .unwrap();
    let text = numbered_book(60, 12);
    let mut reading = Reading::new(BUTTONS, storage, "books/n.txt", text.as_bytes(), "");
    reading.open();
    assert!(bottom_ink(&mut reading) > 0);
    reading.press(reader::NEXT_STATUS_LINE);
    assert!(bottom_ink(&mut reading) > 0);
}

/// The files in folder `dir`.
fn files_in(storage: &Shared, dir: &str) -> Vec<String> {
    let mut names = Vec::new();
    let _ = storage.list(dir, &mut |entry: DirEntry<'_>| {
        names.push(entry.name.to_string());
    });
    names
}

#[test]
fn the_list_of_marks_leads_to_them_and_exports_them() {
    let storage = Shared::default();
    let mut reading = Reading::new(BUTTONS, storage.clone(), "books/noted.epub", &noted(), "");
    reading.open();
    // Nothing yet.
    reading.press(reader::ANNOTATIONS);
    let shown = reading.described();
    // (Its title is cut on a screen this narrow.)
    assert!(shown.contains("No bookmarks or highl"), "{shown}");
    assert!(shown.contains("Select text to highlight"), "{shown}");
    reading.press(actions::BACK);
    // A highlight on the first page, a bookmark two pages on.
    reading.press(reader::SELECT);
    reading.press(actions::CONFIRM);
    reading.press(reader::HIGHLIGHT);
    reading.press(actions::NEXT_PAGE);
    reading.press(actions::NEXT_PAGE);
    let marked = reading.shown().at();
    reading.press(reader::TOGGLE_BOOKMARK);
    reading.press(actions::PREV_PAGE);
    reading.press(actions::PREV_PAGE);
    reading.press(reader::ANNOTATIONS);
    assert_eq!(reading.screen(), "reader.annotations");
    let shown = reading.described();
    assert!(shown.contains("Highlight"), "{shown}");
    assert!(shown.contains("Bookmark"), "{shown}");
    assert!(shown.contains("Export as Markdown"), "{shown}");
    // The bookmark (the later one, second) leads to its page.
    reading.press(actions::DOWN);
    reading.press(actions::CONFIRM);
    assert_eq!(reading.screen(), "reader.screen");
    assert_eq!(reading.shown().at(), marked);
    // Export: a Markdown file beside the library.
    reading.press(reader::ANNOTATIONS);
    reading.press(actions::DOWN);
    reading.press(actions::DOWN);
    assert_eq!(
        focused(&mut reading).split(',').next(),
        Some("Export as Markdown")
    );
    reading.press(actions::CONFIRM);
    let files = files_in(&storage, "Notes");
    assert_eq!(files.len(), 1, "{files:?}");
    assert!(files[0].ends_with(".md"), "{files:?}");
    let text = storage.text(&format!("Notes/{}", files[0])).unwrap();
    assert!(text.starts_with("# Test Book\n\nA. Writer\n"), "{text}");
    assert!(text.contains("## Highlights"), "{text}");
    assert!(text.contains("## Bookmarks"), "{text}");
}

/// The text at `at` in the EPUB `path` on `storage`, converted with the
/// book's styles or without.
fn epub_text_at(storage: &Shared, path: &str, styles: bool, at: (u32, u32), n: usize) -> String {
    use paperpaw_sdk::document::Document;
    let book = paperpaw_epub_family::EpubDocument::open_with(storage, path, styles).unwrap();
    let block = book
        .block_at(storage, Locator::new(at.0, at.1))
        .unwrap()
        .expect("a block there");
    block.text[block.text_index(at.1)..]
        .chars()
        .take(n)
        .collect()
}

/// Its own styles give a book style runs its conversion without them
/// lacks, so the same text is at other places there: the reader keeps
/// the place on its text when they are switched, with the book open or
/// closed.
#[test]
fn the_place_stays_on_its_text_when_the_books_styles_switch() {
    let body: String = (0..300)
        .map(|n| {
            format!(
                "<p>Paragraph {n} has <span style=\"font-weight: bold\">some words in bold</span> \
                 and <span style=\"font-style: italic\">a few in italics</span> among the others.</p>"
            )
        })
        .collect();
    let epub = epub(&[("one.xhtml", body)], &[]);
    let path = "books/styled.epub";
    let storage = Shared::default();
    let mut reading = Reading::new(BUTTONS, storage.clone(), path, &epub, "");
    reading.open();
    for _ in 0..20 {
        reading.run(actions::NEXT_PAGE);
    }
    let place = reading.kept();
    let text = epub_text_at(&storage, path, true, place, 20);
    for (styles, open) in [("off", true), ("on", false)] {
        if !open {
            reading.press(actions::BACK);
        }
        reading.now += 10;
        let effects = reading
            .kernel
            .set_setting(
                paperpaw_sdk::Instant(reading.now),
                "core.publisher-styles",
                styles,
            )
            .unwrap();
        reading.settle(effects);
        if !open {
            reading.open();
        }
        let at = reading.kept();
        // From the start of the line the text is in, laid out anew.
        let shown = epub_text_at(&storage, path, styles == "on", at, 90);
        assert!(
            shown.contains(&text),
            "{text:?} at {place:?}, then {shown:?} at {at:?} with styles {styles}"
        );
    }
}
