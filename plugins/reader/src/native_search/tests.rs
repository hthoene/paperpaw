//! The search model: finding text and the snippet round it for any text (the
//! properties of interface 6's `matching_tests`), the scan over a book double,
//! and every allocation refused in turn.
extern crate std;

use super::*;
use crate::fail_alloc::{exempt, sweep};
use alloc::format;
use alloc::string::ToString;
use proptest::prelude::*;

fn needle(text: &str) -> Needle {
    Needle::new(text).unwrap()
}

fn find(text: &str, needle: &Needle) -> Vec<(usize, usize)> {
    needle.matches(text).collect()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(256)))]

    #[test]
    fn hits_are_whole_characters_of_the_text_looked_for(
        text in "[aAbBäÄß€😀İ .,\n]{0,60}",
        query in "[abäß€😀İ ]{1,4}",
    ) {
        let n = needle(&query);
        let hits = find(&text, &n);
        let mut end = 0;
        for &(start, stop) in &hits {
            prop_assert!(start >= end, "overlap in {hits:?}");
            prop_assert!(text.is_char_boundary(start) && text.is_char_boundary(stop));
            let found: Vec<char> = text[start..stop].chars().map(lower).collect();
            prop_assert_eq!(&found, &n.0);
            end = stop;
        }
    }

    #[test]
    fn every_place_of_it_is_found(
        parts in proptest::collection::vec("[xyz ]{0,6}", 1..6),
        word in "[a-dä]{1,4}",
    ) {
        // The word between the parts, in upper case every other time.
        let mut text = String::new();
        for (i, part) in parts.iter().enumerate() {
            text.push_str(part);
            if i + 1 < parts.len() {
                if i % 2 == 0 {
                    text.push_str(&word.to_uppercase());
                } else {
                    text.push_str(&word);
                }
            }
        }
        let hits = find(&text, &needle(&word));
        prop_assert_eq!(hits.len(), parts.len() - 1, "{} in {}", word, text);
    }

    #[test]
    fn a_snippet_holds_its_hit(
        before in "[a-zäö€😀 \n\t]{0,80}",
        hit in "[A-Z]{1,8}",
        after in "[a-zäö€😀 \n\t]{0,120}",
    ) {
        let text = format!("{before}{hit}{after}");
        let start = before.len();
        let end = start + hit.len();
        let shown = snippet(&text, start, end).unwrap();
        prop_assert!(shown.contains(&hit), "{shown}");
        prop_assert!(!shown.contains(['\n', '\t']));
        // At most the context on either side, and the marks for more.
        prop_assert!(shown.chars().count() <= BEFORE + hit.chars().count() + AFTER + 2);
        prop_assert_eq!(shown.starts_with('…'), before.chars().count() > BEFORE);
        prop_assert_eq!(shown.ends_with('…'), after.chars().count() > AFTER);
        // Where the match is in the snippet, in characters.
        let (from, to) = snippet_at(&text, start, end);
        let found: String = shown.chars().skip(from).take(to - from).collect();
        prop_assert_eq!(found, hit);
    }
}

#[test]
fn finds_ignoring_case() {
    let n = needle("äpfel");
    let text = "Äpfel und Birnen; zwei ÄPFEL.";
    let found = find(text, &n);
    assert_eq!(found.len(), 2);
    assert_eq!(&text[found[0].0..found[0].1], "Äpfel");
    assert_eq!(&text[found[1].0..found[1].1], "ÄPFEL");
    assert!(find(text, &needle("")).is_empty());
    // Hits do not overlap, and one at the very end is found.
    assert_eq!(find("aaa", &needle("aa")), [(0, 2)]);
    assert_eq!(find("xaA", &needle("aa")), [(1, 3)]);
    assert!(find("a", &needle("aa")).is_empty());
    assert_eq!(find("aaaa", &needle("aa")), [(0, 2), (2, 4)]);
    assert!(find("", &needle("a")).is_empty());
    assert!(find("abc", &needle("abcd")).is_empty());
}

#[test]
fn a_hit_at_either_end_is_found_whole() {
    let text = "€uro and €uro";
    assert_eq!(find(text, &needle("€URO")), [(0, 6), (11, 17)]);
    assert_eq!(snippet(text, 11, 17).unwrap(), "€uro and €uro");
}

#[test]
fn a_long_query_is_cut_to_the_most() {
    let long = "a".repeat(MAX_NEEDLE + 20);
    let found = find(&long, &needle(&long));
    // Only its first 32 characters count; the 20 left over are not a second match.
    assert_eq!(found, [(0, MAX_NEEDLE)]);
}

#[test]
fn the_query_is_trimmed() {
    assert_eq!(needle("  Katze \n"), needle("katze"));
    assert!(needle("   ").is_empty());
}

/// Every character one pixel wide: a row of `width` holds that many.
struct Ruler;

impl Measure for Ruler {
    fn advance(&mut self, _: char) -> f32 {
        1.0
    }
    fn ellipsis(&mut self) -> f32 {
        1.0
    }
}

#[test]
fn a_snippet_that_fits_is_left_as_it_is() {
    let shown = "a short line with word in it";
    let at = (18, 22);
    assert_eq!(with_hit(&mut Ruler, shown, at, 60.0).unwrap(), shown);
}

#[test]
fn a_snippet_that_does_not_fit_begins_later_so_the_word_is_in_the_row() {
    let shown = "…the long road to the quiet word in the valley below the hills…";
    let start = shown.find("word").unwrap();
    let at = (start, start + 4);
    let cut = with_hit(&mut Ruler, shown, at, 30.0).unwrap();
    assert!(cut.starts_with('…') && cut.contains("word"), "{cut}");
    assert!(cut.len() < shown.len());
    // A match wider than the row: from its start.
    let wide = with_hit(&mut Ruler, "…abc defghij klm…", (5, 11), 6.0).unwrap();
    assert!(wide.contains("defghi"), "{wide}");
}

/// A book of sections of blocks (one block a line), each block its own text.
struct Book {
    sections: Vec<Vec<String>>,
    /// The block read at this place fails with this error.
    broken: Option<(Place, Error)>,
}

struct Line {
    text: String,
    end: u32,
}

impl Book {
    fn new(sections: &[&[&str]]) -> Self {
        Self {
            sections: sections
                .iter()
                .map(|s| s.iter().map(|t| t.to_string()).collect())
                .collect(),
            broken: None,
        }
    }
}

impl Blocks for Book {
    type Block = Line;
    fn sections(&self) -> u32 {
        self.sections.len() as u32
    }
    fn block_at(&mut self, at: Place) -> Result<Option<Line>, Error> {
        if let Some((place, error)) = &self.broken
            && *place == at
        {
            return Err(*error);
        }
        // A block begins every 100 offsets.
        let index = (at.offset / 100) as usize;
        let Some(text) = self
            .sections
            .get(at.section as usize)
            .and_then(|s| s.get(index))
        else {
            return Ok(None);
        };
        // The double's own allocation is not the code's.
        Ok(Some(Line {
            text: exempt(|| text.clone()),
            end: (index as u32 + 1) * 100,
        }))
    }
    fn text<'b>(&self, block: &'b Line) -> &'b str {
        &block.text
    }
    fn end(&self, block: &Line) -> u32 {
        block.end
    }
    fn source_offset(&self, block: &Line, byte: usize) -> u32 {
        block.end - 100 + byte as u32
    }
    fn percent(&self, at: Place) -> u8 {
        (at.section * 10) as u8 + (at.offset / 100) as u8
    }
}

fn run(search: &mut Search, book: &mut Book) -> usize {
    let mut steps = 0;
    while search.searching() {
        search.step(book, &mut Ruler, 400.0).unwrap();
        steps += 1;
        assert!(steps < 100, "the scan does not end");
    }
    steps
}

#[test]
fn the_scan_reads_the_book_and_keeps_the_hits_in_order() {
    let mut book = Book::new(&[
        &["One Cat sat.", "No hit here."],
        &[],
        &["a cat, another CAT", "cat"],
    ]);
    let mut search = Search::new(" cat ", true).unwrap();
    assert_eq!(search.note(), Note::Searching(0));
    run(&mut search, &mut book);
    let hits = search.hits();
    assert_eq!(hits.len(), 4);
    assert_eq!(
        hits[0].start,
        Place {
            section: 0,
            offset: 4
        }
    );
    assert_eq!(
        hits[0].end,
        Place {
            section: 0,
            offset: 7
        }
    );
    assert_eq!(hits[0].snippet, "One Cat sat.");
    assert_eq!(
        hits[1].start,
        Place {
            section: 2,
            offset: 2
        }
    );
    assert_eq!(
        hits[2].start,
        Place {
            section: 2,
            offset: 15
        }
    );
    assert_eq!(
        hits[3].start,
        Place {
            section: 2,
            offset: 100
        }
    );
    assert_eq!(hits[3].percent, 21);
    assert_eq!(search.note(), Note::Hits(4));
    assert_eq!(
        search.target(1),
        Some((
            Place {
                section: 2,
                offset: 2
            },
            Place {
                section: 2,
                offset: 5
            }
        ))
    );
    assert_eq!(search.target(4), None);
}

#[test]
fn nothing_is_looked_for_without_a_query_or_a_book() {
    assert!(!Search::new("  ", true).unwrap().searching());
    let none = Search::new("cat", false).unwrap();
    assert!(!none.searching());
    assert_eq!(none.note(), Note::Hits(0));
}

#[test]
fn more_than_the_most_are_counted_as_more() {
    let line = "cat ".repeat(30);
    let blocks = [line.as_str(); 6];
    let mut book = Book::new(&[&blocks]);
    let mut search = Search::new("cat", true).unwrap();
    run(&mut search, &mut book);
    assert_eq!(search.hits().len(), MAX_HITS);
    assert_eq!(search.note(), Note::MoreHits(MAX_HITS));
    assert!(!search.searching());
}

#[test]
fn a_step_reads_a_bounded_number_of_blocks() {
    let blocks = ["no"; 3 * BLOCKS_PER_STEP];
    let mut book = Book::new(&[&blocks]);
    let mut search = Search::new("cat", true).unwrap();
    search.step(&mut book, &mut Ruler, 100.0).unwrap();
    assert!(search.searching());
    assert_eq!(run(&mut search, &mut book), 3);
}

#[test]
fn a_block_that_cannot_be_read_ends_its_section() {
    let mut book = Book::new(&[&["cat", "cat"], &["cat"]]);
    book.broken = Some((
        Place {
            section: 0,
            offset: 100,
        },
        Error::Invalid,
    ));
    let mut search = Search::new("cat", true).unwrap();
    run(&mut search, &mut book);
    // The second block of the first section is lost; the next section is read.
    assert_eq!(search.hits().len(), 2);
    assert_eq!(search.hits()[1].start.section, 1);
}

#[test]
fn the_header_changes_in_tens_and_when_done() {
    let mut search = Search::new("cat", true).unwrap();
    assert!(!search.progressed(5));
    assert!(search.progressed(37));
    assert_eq!(search.note(), Note::Searching(30));
    assert!(!search.progressed(39));
    assert!(search.progressed(40));
}

#[test]
fn a_refused_allocation_keeps_the_scan_where_it_was_and_no_hit_twice() {
    let book = core::cell::RefCell::new(exempt(|| Book::new(&[&["cat and cat", "another cat"]])));
    sweep(|| {
        let mut search = Search::new("cat", true)?;
        let mut refused = 0;
        while search.searching() {
            match search.step(&mut *book.borrow_mut(), &mut Ruler, 400.0) {
                Ok(_) => {}
                // The refusal is once: the step is made again where it was.
                Err(Error::NoMemory) => {
                    refused += 1;
                    assert!(refused < 3, "the scan does not go on");
                }
                Err(error) => return Err(error),
            }
        }
        let starts: Vec<u32> = exempt(|| search.hits().iter().map(|h| h.start.offset).collect());
        assert_eq!(starts, [0, 8, 108]);
        Ok(())
    });
}

#[test]
fn every_allocation_may_be_refused() {
    let text = "Ein Katzentext mit Katze und noch einer katze am Ende dieser langen Zeile.";
    sweep(|| {
        let n = Needle::new("Katze")?;
        for (start, end) in n.matches(text) {
            let shown = snippet(text, start, end)?;
            with_hit(&mut Ruler, &shown, snippet_at(text, start, end), 20.0)?;
        }
        Ok(())
    });
    let book = exempt(|| Book::new(&[&["cat and cat", "another cat"], &["cat"]]));
    let book = core::cell::RefCell::new(book);
    sweep(|| {
        let mut search = Search::new("cat", true)?;
        while search.searching() {
            search.step(&mut *book.borrow_mut(), &mut Ruler, 400.0)?;
        }
        Ok(())
    });
}
