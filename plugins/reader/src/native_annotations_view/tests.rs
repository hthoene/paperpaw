use super::*;
use alloc::{string::String, vec::Vec};

fn notes() -> Vec<(Kind, u8, &'static str, &'static str)> {
    alloc::vec![
        (Kind::Bookmark, 3, "Chapter one starts here", ""),
        (Kind::Highlight, 12, "A quoted line", "my note"),
        (Kind::Highlight, 40, "Another", ""),
        (Kind::Bookmark, 90, "The end is near", ""),
    ]
}

/// The pieces the streaming writer makes, joined, and how many there were.
fn streamed(title: &str, author: &str, all: &[(Kind, u8, &str, &str)]) -> (String, usize) {
    let headings = Headings::new("Highlights", "Bookmarks");
    let mut pieces = 0;
    let mut text = String::new();
    {
        let mut put = |piece: &str| {
            pieces += 1;
            text.push_str(piece);
            Ok(())
        };
        let mut markdown = Markdown::new(title, author, &headings, &mut put);
        markdown.begin().unwrap();
        // The highlights from one pass, the bookmarks from a second.
        for kind in [Kind::Highlight, Kind::Bookmark] {
            for (k, percent, snippet, note) in all {
                if *k == kind {
                    markdown
                        .note(&Note::new(*k, *percent, snippet, note))
                        .unwrap();
                }
            }
        }
        markdown.end().unwrap();
    }
    (text, pieces)
}

#[test]
fn the_streamed_markdown_is_what_the_model_makes_in_one_string() {
    let headings = Headings::new("Highlights", "Bookmarks");
    for (title, author) in [("A Book", "An Author"), ("A Book", ""), ("", "")] {
        let all = notes();
        let notes: Vec<Note<'_>> = all
            .iter()
            .map(|(kind, percent, text, note)| Note::new(*kind, *percent, text, note))
            .collect();
        let whole = model::markdown(title, author, &headings, &notes).unwrap();
        assert_eq!(streamed(title, author, &all).0, whole, "{title}/{author}");
    }
}

#[test]
fn the_markdown_of_no_notes_and_of_one_kind_has_no_empty_heading() {
    let headings = Headings::new("Highlights", "Bookmarks");
    assert_eq!(streamed("T", "", &[]).0, "# T\n\n");
    let only_bookmarks = [(Kind::Bookmark, 5, "x", "")];
    let notes = [Note::new(Kind::Bookmark, 5, "x", "")];
    let (text, _) = streamed("T", "", &only_bookmarks);
    assert_eq!(text, model::markdown("T", "", &headings, &notes).unwrap());
    assert!(!text.contains("Highlights"));
}

#[test]
fn a_refusing_sink_stops_the_markdown_at_once() {
    let headings = Headings::new("H", "B");
    let mut calls = 0;
    let mut put = |_: &str| {
        calls += 1;
        Err(Error::Io)
    };
    let mut markdown = Markdown::new("T", "A", &headings, &mut put);
    assert_eq!(markdown.begin(), Err(Error::Io));
    assert_eq!(calls, 1, "the author is not written after the refusal");
}

#[test]
fn a_text_is_cut_at_a_character_and_a_percent_stays_in_range() {
    assert_eq!(cut("abcdef", 3), "abc");
    assert_eq!(cut("abc", 10), "abc");
    // "é" is two bytes: a cut inside it goes back to the character before.
    assert_eq!(cut("aéb", 2), "a");
    assert_eq!(cut("é", 0), "");
    assert_eq!(percent_of(0), 0);
    assert_eq!(percent_of(100), 100);
    assert_eq!(percent_of(250), 100);
}

#[test]
fn a_kind_the_host_adds_is_shown_as_a_highlight() {
    assert_eq!(kind_of(AnnotationKind::Bookmark), Kind::Bookmark);
    assert_eq!(kind_of(AnnotationKind::Highlight), Kind::Highlight);
    assert_eq!(kind_of(AnnotationKind::Other(3)), Kind::Highlight);
}

#[test]
fn the_screen_without_a_book_says_so_and_has_nothing_to_choose() {
    let view = AnnotationsView::none("reader.no-book");
    assert_eq!(view.message, Some("reader.no-book"));
    assert!(view.places.is_empty());
    assert!(view.list.items().is_empty());
    assert!(!view.more);
}

#[test]
fn building_the_markdown_survives_every_refused_allocation() {
    let all = notes();
    crate::fail_alloc::sweep(|| {
        let headings = Headings::new("Highlights", "Bookmarks");
        let mut size = 0;
        let mut put = |piece: &str| {
            size += piece.len();
            Ok(())
        };
        let mut markdown = Markdown::new("Title", "Author", &headings, &mut put);
        markdown.begin()?;
        for (kind, percent, snippet, note) in all.iter().copied() {
            markdown.note(&Note::new(kind, percent, snippet, note))?;
        }
        markdown.end()?;
        Ok(size)
    });
}
