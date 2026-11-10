use super::*;
use crate::fail_alloc::sweep;
use alloc::string::ToString;
use alloc::vec::Vec;
use core::cell::Cell;

const HEADINGS: Headings<'static> = Headings::new("Highlights", "Bookmarks");

fn all() -> Vec<Note<'static>> {
    alloc::vec![
        Note::new(Kind::Bookmark, 12, "Chapter two begins", ""),
        Note::new(Kind::Highlight, 40, "To be or not", "famous"),
        Note::new(Kind::Highlight, 41, "That is the question", ""),
        Note::new(Kind::Bookmark, 99, "The end", ""),
    ]
}

#[test]
fn the_notes_are_markdown_as_interface_6_wrote_them() {
    // The form the notes apps read: highlights as quotes (with their note), then the bookmarks.
    assert_eq!(
        markdown("Hamlet", "Shakespeare", &HEADINGS, &all()).unwrap(),
        "# Hamlet\n\nShakespeare\n\n\
         ## Highlights\n\n> To be or not\n>\n> 40 %\n\nfamous\n\n> That is the question\n>\n> 41 %\n\n\
         ## Bookmarks\n\n- 12 %: Chapter two begins\n- 99 %: The end\n\n"
    );
    assert_eq!(
        markdown("T", "", &HEADINGS, &[]).unwrap(),
        "# T\n\n",
        "no author line, no headings where there is nothing"
    );
    assert_eq!(
        markdown("T", "", &HEADINGS, &all()[..1]).unwrap(),
        "# T\n\n## Bookmarks\n\n- 12 %: Chapter two begins\n\n"
    );
}

#[test]
fn the_notes_folder_is_beside_the_library() {
    assert_eq!(notes_dir("storage/Books").unwrap(), "storage/Notes");
    assert_eq!(notes_dir("Books").unwrap(), "Notes");
    assert_eq!(notes_dir("").unwrap(), "Notes");
    assert_eq!(notes_dir("a/b/Books").unwrap(), "a/b/Notes");
}

#[test]
fn the_file_name_is_author_and_title_fit_for_a_card() {
    assert_eq!(
        file_name("Books/h.epub", "Hamlet", "Shakespeare").unwrap(),
        "Shakespeare - Hamlet.md"
    );
    assert_eq!(file_name("Books/h.epub", "", "").unwrap(), "h.epub.md");
    assert_eq!(file_name("x", "A/B: C?", "").unwrap(), "A B C.md");
    assert_eq!(file_name("x", "...", "").unwrap(), "notes.md");
    assert_eq!(title_of("Books/h.epub", "").unwrap(), "h.epub");
    assert_eq!(title_of("Books/h.epub", "Hamlet").unwrap(), "Hamlet");
    let long = "é".repeat(300);
    assert!(file_name("x", &long, "").unwrap().chars().count() <= 100 + 3);
}

#[test]
fn a_row_says_what_it_is_and_where() {
    let quote = |t: &str| format(format_args!("“{t}”"));
    let notes = all();
    assert_eq!(
        row(&notes[0], "Bookmark", "Highlight", quote).unwrap(),
        (
            "Chapter two begins".to_string(),
            "Bookmark · 12 %".to_string()
        )
    );
    assert_eq!(
        row(&notes[1], "Bookmark", "Highlight", quote).unwrap(),
        ("“To be or not”".to_string(), "famous · 40 %".to_string())
    );
    assert_eq!(
        row(&notes[2], "Bookmark", "Highlight", quote).unwrap(),
        (
            "“That is the question”".to_string(),
            "Highlight · 41 %".to_string()
        )
    );
}

#[test]
fn the_stage_runs_after_a_write_that_worked_only() {
    let announced = Cell::new(0);
    let wrote = export(
        |file, bytes| {
            assert_eq!((file, bytes), ("Notes/a.md", b"x".as_slice()));
            Ok::<(), ()>(())
        },
        "Notes/a.md",
        "x",
        || {
            announced.set(announced.get() + 1);
            Ok::<(), ()>(())
        },
    );
    assert!(wrote);
    assert_eq!(announced.get(), 1);
    let wrote = export(
        |_, _| Err::<(), _>(()),
        "Notes/a.md",
        "x",
        || {
            announced.set(announced.get() + 1);
            Ok::<(), ()>(())
        },
    );
    assert!(!wrote);
    assert_eq!(announced.get(), 1, "a failed write announces nothing");
    // A failed run changes nothing: the notes are written.
    assert!(export(
        |_, _| Ok::<(), ()>(()),
        "f",
        "x",
        || Err::<(), ()>(())
    ));
}

#[test]
fn every_allocation_may_be_refused() {
    let quote = |t: &str| format(format_args!("“{t}”"));
    // Built outside: the sweep refuses allocations, and a test vector is not the code under test.
    let notes = all();
    sweep(|| {
        markdown("Hamlet", "Shakespeare", &HEADINGS, &notes)?;
        notes_dir("storage/Books")?;
        file_name("Books/h.epub", "Hamlet", "Shakespeare")?;
        file_name("x", "", "")?;
        title_of("Books/h.epub", "")?;
        for note in &notes {
            row(note, "Bookmark", "Highlight", quote)?;
        }
        Ok(())
    });
}
