use super::*;
use crate::fail_alloc::sweep;
use paperpaw_ui_kit::Item;

/// A book of lines, one block each.
struct Lines(Vec<&'static str>);

impl Blocks for Lines {
    type Block = &'static str;
    fn sections(&self) -> u32 {
        1
    }
    fn block_at(&mut self, at: Place) -> Result<Option<&'static str>, Error> {
        Ok(self.0.get(at.offset as usize).copied())
    }
    fn text<'b>(&self, block: &'b &'static str) -> &'b str {
        block
    }
    fn end(&self, _: &&'static str) -> u32 {
        0
    }
    fn source_offset(&self, _: &&'static str, byte: usize) -> u32 {
        byte as u32
    }
    fn percent(&self, at: Place) -> u8 {
        (at.offset * 10) as u8
    }
}

/// Ten of a letter's width, whatever it is.
struct Wide;

impl Measure for Wide {
    fn advance(&mut self, _: char) -> f32 {
        10.0
    }
    fn ellipsis(&mut self) -> f32 {
        10.0
    }
}

fn found(lines: &[&'static str]) -> SearchView {
    let mut view = SearchView::none("reader.search.wait");
    view.message = None;
    let mut search = Search::new("cat", true).unwrap();
    let mut book = Lines(lines.to_vec());
    while search.searching() {
        search.step(&mut book, &mut Wide, 10_000.0).unwrap();
    }
    view.search = Some(search);
    view
}

#[test]
fn how_far_in_is_by_the_lengths_or_by_the_section() {
    let at = |section, offset| Place { section, offset };
    assert_eq!(percent_of(None, 4, at(1, 99)), 25);
    assert_eq!(percent_of(None, 0, at(3, 0)), 100);
    assert_eq!(percent_of(None, 4, at(9, 0)), 100);
    let progress = Progress::new(alloc::vec![100, 100]);
    assert_eq!(percent_of(Some(&progress), 2, at(1, 50)), 75);
    assert_eq!(percent_of(Some(&progress), 2, at(0, 0)), 0);
}

#[test]
fn a_font_that_cannot_say_leaves_half_the_size_for_a_letter() {
    let mut ruler = Ruler {
        font: None,
        size: 20.0,
    };
    assert_eq!(ruler.advance('a'), 10.0);
    assert_eq!(ruler.ellipsis(), 10.0);
    assert_eq!(ruler.width_of("abcd"), 40.0);
}

#[test]
fn a_screen_with_no_book_says_so_and_has_nothing_to_search() {
    let view = SearchView::none("reader.no-book");
    assert_eq!(view.message, Some("reader.no-book"));
    assert!(!view.searching());
    assert!(view.list.items().is_empty());
}

#[test]
fn the_list_has_a_row_for_each_hit_with_how_far_in() {
    let mut view = found(&["no", "a cat here", "none", "the cat and the cat"]);
    view.extend_list().unwrap();
    assert_eq!(view.places.len(), 3);
    let Some(Item::Row(row)) = view.list.items().first() else {
        panic!("no row");
    };
    assert_eq!(row.title, "a cat here");
    assert_eq!(row.value, "20 %");
    assert!(view.list.items().len() == 3);
    assert!(!view.searching());
}

#[test]
fn a_hit_chosen_goes_to_where_its_match_begins() {
    let view = found(&["x", "a cat here"]);
    let (start, end) = view.search.as_ref().unwrap().target(0).unwrap();
    assert_eq!((start.section, start.offset), (0, 2));
    assert_eq!(end.offset, 5);
}

#[test]
fn a_snippet_is_held_by_its_row_only_and_a_row_is_made_once() {
    let mut view = found(&["a cat", "no", "the cat and the cat"]);
    view.extend_list().unwrap();
    assert_eq!(view.places.len(), 3);
    // The hits keep their places, not their snippets.
    let hits = view.search.as_ref().unwrap().hits();
    assert!(hits.iter().all(|hit| hit.snippet.is_empty()));
    // A second call finds nothing new: the rows are not made again.
    view.extend_list().unwrap();
    assert_eq!(view.list.items().len(), 3);
    assert_eq!(view.places.len(), 3);
    // A row leads to its hit's start.
    assert_eq!(
        view.places.iter().map(|p| p.0.offset).collect::<Vec<_>>(),
        [2, 4, 16]
    );
    // And knows where the match ends, for the page to mark it.
    assert_eq!(
        view.places.iter().map(|p| p.1).collect::<Vec<_>>(),
        [5, 7, 19]
    );
}

#[test]
fn a_search_that_failed_says_so_and_keeps_its_rows() {
    let mut view = found(&["a cat"]);
    view.extend_list().unwrap();
    view.fail(Error::NoMemory);
    assert!(!view.searching());
    assert_eq!(view.failed, Some(Error::NoMemory));
    assert_eq!(view.list.items().len(), 1);
}

#[test]
fn the_list_of_hits_survives_every_refused_allocation() {
    sweep(|| {
        // The fixture is not the code's: it is made without counting.
        let mut view = crate::fail_alloc::exempt(|| found(&["a cat", "a cat and a cat", "cat"]));
        view.extend_list()?;
        let rows = view.places.len();
        crate::fail_alloc::exempt(|| drop(view));
        Ok(rows)
    });
}
