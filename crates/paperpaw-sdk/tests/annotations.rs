//! The annotations file (`.paperpaw/annotations/<hash>.txt`): what is
//! saved comes back (any text and note, tabs and line breaks folded into
//! spaces), and hostile files (wrong field counts, bad numbers, invalid
//! UTF-8, huge lines, duplicates) are read without panics. Annotations
//! made in one conversion of a book are found again in another by their
//! XPointers, and none is lost on the way.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_sdk::annotations::{self, Annotation, Kind, Mark};
use paperpaw_sdk::document::{Block, Document, DocumentError, Locator, Metadata};
use paperpaw_sdk::{MemoryStorage, Storage};
use proptest::prelude::*;

const BOOK: &str = "books/a.epub";

fn annotation(
    kind: Kind,
    start: (u32, u32),
    end: (u32, u32),
    text: &str,
    note: &str,
) -> Annotation {
    Annotation::new(
        Mark::new(
            kind,
            Locator::new(start.0, start.1),
            Locator::new(end.0, end.1),
            !note.is_empty(),
        ),
        50,
        1_700_000_000,
        1_700_000_001,
        text,
        note,
    )
}

fn with_file(text: &[u8]) -> MemoryStorage {
    MemoryStorage::new().with(&annotations::file(BOOK), text.to_vec())
}

/// What a saved annotation reads back as: text and note on one line.
fn as_read(mut a: Annotation) -> Annotation {
    a.text = annotations::one_line(&a.text);
    a.note = annotations::one_line(&a.note);
    a.mark.noted = !a.note.is_empty();
    a
}

fn any_annotation() -> impl Strategy<Value = Annotation> {
    (
        any::<bool>(),
        (any::<u32>(), any::<u32>(), any::<u32>(), any::<u32>()),
        0u8..=100,
        (any::<i64>(), any::<i64>()),
        ".{0,40}",
        ".{0,40}",
    )
        .prop_map(
            |(bookmark, (s1, o1, s2, o2), percent, (created, updated), text, note)| {
                Annotation::new(
                    Mark::new(
                        if bookmark {
                            Kind::Bookmark
                        } else {
                            Kind::Highlight
                        },
                        Locator::new(s1, o1),
                        Locator::new(s2, o2),
                        false,
                    ),
                    percent,
                    created,
                    updated,
                    text,
                    note,
                )
            },
        )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(128)))]

    /// Any annotations come back sorted by position, with their numbers
    /// exact and their texts as one line.
    #[test]
    fn saved_annotations_come_back(list in proptest::collection::vec(any_annotation(), 0..8)) {
        let storage = MemoryStorage::new();
        annotations::save(&storage, BOOK, &list).unwrap();
        let mut expected: Vec<Annotation> = list.into_iter().map(as_read).collect();
        expected.sort_by_key(|a| (a.mark.start, a.mark.end));
        let read = annotations::load(&storage, BOOK);
        prop_assert_eq!(read.len(), expected.len());
        // Equal positions may come in either order: compare as sets.
        for a in &expected {
            prop_assert!(read.contains(a), "{a:?} missing from {read:?}");
        }
        let positions: Vec<_> = read.iter().map(|a| (a.mark.start, a.mark.end)).collect();
        let mut sorted = positions.clone();
        sorted.sort();
        prop_assert_eq!(positions, sorted);
    }

    /// Notes with tabs, line breaks and any Unicode survive as one line
    /// and never split the file's fields.
    #[test]
    fn notes_with_tabs_and_breaks_stay_one_field(text in "[^\r]{0,30}", note in "[^\r]{0,30}") {
        let storage = MemoryStorage::new();
        let start = Locator::new(1, 10);
        annotations::add(&storage, BOOK, annotation(Kind::Highlight, (1, 10), (1, 20), &text, "")).unwrap();
        prop_assert!(annotations::set_note(&storage, BOOK, start, &note, 5).unwrap());
        let all = annotations::load(&storage, BOOK);
        prop_assert_eq!(all.len(), 1);
        prop_assert_eq!(&all[0].text, &annotations::one_line(&text));
        prop_assert_eq!(&all[0].note, &annotations::one_line(&note));
        prop_assert_eq!(all[0].mark.noted, !annotations::one_line(&note).is_empty());
        prop_assert_eq!(all[0].updated, 5);
        prop_assert!(!all[0].note.contains(['\t', '\n']));
    }

    /// Whatever bytes the file holds, reading it does not panic, and
    /// every annotation read is in range.
    #[test]
    fn any_file_is_read_without_panic(bytes in proptest::collection::vec(any::<u8>(), 0..400)) {
        let storage = with_file(&bytes);
        for a in annotations::load(&storage, BOOK) {
            prop_assert!(a.percent <= 100);
        }
    }

    /// Lines made of tab-separated numbers and words, of any field count,
    /// are read or skipped, never misread into a panic.
    #[test]
    fn lines_of_any_field_count_are_read_or_skipped(
        fields in proptest::collection::vec(
            prop_oneof!["[bh]", "-?[0-9]{1,20}", "[a-z ]{0,5}"],
            0..14,
        )
    ) {
        let line = fields.join("\t");
        let storage = with_file(line.as_bytes());
        let read = annotations::load(&storage, BOOK);
        prop_assert!(read.len() <= 1);
    }

    /// `snippet` keeps at most 240 bytes (and an ellipsis), on one line,
    /// cut at a character boundary.
    #[test]
    fn snippets_are_short_and_on_one_line(text in ".{0,300}") {
        let snippet = annotations::snippet(&text);
        let body = snippet.strip_suffix('…').unwrap_or(&snippet);
        prop_assert!(body.len() <= 240);
        prop_assert!(!snippet.contains(['\n', '\t', '\r']));
        if text.len() <= 240 {
            prop_assert_eq!(snippet.chars().count(), text.chars().count());
        }
    }
}

#[test]
fn the_two_field_counts_are_told_apart() {
    // 8 fields: written before times were kept; the 7th is the text.
    let old = "h\t1\t2\t1\t9\t40\t1700000000\tnote";
    // 10 fields: the 7th and 8th are the times.
    let new = "b\t3\t0\t3\t0\t60\t1700000000\t1700000100\ttext\tnote";
    let storage = with_file(format!("{old}\n{new}\n").as_bytes());
    let all = annotations::load(&storage, BOOK);
    assert_eq!(all.len(), 2);
    // A number in the text of an old line stays text.
    assert_eq!(all[0].text, "1700000000");
    assert_eq!((all[0].created, all[0].updated), (0, 0));
    assert_eq!(all[0].note, "note");
    assert_eq!(
        (all[1].created, all[1].updated),
        (1_700_000_000, 1_700_000_100)
    );
    assert_eq!(
        (all[1].text.as_str(), all[1].note.as_str()),
        ("text", "note")
    );
}

#[test]
fn lines_with_too_few_fields_or_bad_numbers_are_skipped() {
    let text = [
        "",
        "h",
        "h\t1\t2\t3",
        "h\t1\t2\t3\t4",
        // A kind that is neither.
        "x\t1\t2\t3\t4\t5\ttext\tnote",
        // Negative and too large positions.
        "h\t-1\t2\t3\t4\t5\ttext\tnote",
        "h\t1\t4294967296\t3\t4\t5\ttext\tnote",
        // Times that are no numbers, with 10 fields.
        "h\t1\t2\t3\t4\t5\tsoon\tlater\ttext\tnote",
        // The progress is no number.
        "h\t1\t2\t3\t4\thalf\ttext\tnote",
        // Read: only the progress, no text.
        "b\t1\t2\t3\t4\t5",
    ]
    .join("\n");
    let all = annotations::load(&with_file(text.as_bytes()), BOOK);
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].mark.kind, Kind::Bookmark);
    assert_eq!((all[0].text.as_str(), all[0].note.as_str()), ("", ""));
    assert!(!all[0].mark.noted);
}

#[test]
fn progress_out_of_range_is_clamped() {
    let text = "h\t1\t0\t1\t5\t250\tt\t\nh\t1\t5\t1\t9\t-3\tt\t\n";
    let all = annotations::load(&with_file(text.as_bytes()), BOOK);
    assert_eq!(all.iter().map(|a| a.percent).collect::<Vec<_>>(), [100, 0]);
}

#[test]
fn more_than_ten_fields_keep_the_rest_in_the_note() {
    let text = "h\t1\t0\t1\t5\t3\t1\t2\ttext\tnote\twith\textra";
    let all = annotations::load(&with_file(text.as_bytes()), BOOK);
    assert_eq!(all[0].note, "note\twith\textra");
    // Saved again, the note is one field.
    let storage = MemoryStorage::new();
    annotations::save(&storage, BOOK, &all).unwrap();
    let again = annotations::load(&storage, BOOK);
    assert_eq!(again[0].note, "note with extra");
    assert_eq!(again[0].text, "text");
}

#[test]
fn invalid_utf8_and_windows_line_ends_are_tolerated() {
    let mut bytes = b"h\t1\t0\t1\t5\t3\t1\t2\tbad \xff\xfe text\tnote\r\n".to_vec();
    bytes.extend_from_slice(b"b\t2\t0\t2\t0\t9\t1\t2\tchapter\t\r\n");
    let all = annotations::load(&with_file(&bytes), BOOK);
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].text, "bad \u{fffd}\u{fffd} text");
    // The carriage return is no part of the last field.
    assert_eq!(all[0].note, "note");
    assert_eq!(all[1].note, "");
}

/// A huge note is cut (ending in `…`) so its line stays within what the
/// file is read by, and the annotations after it are still read.
#[test]
fn a_huge_note_is_cut_and_the_next_annotation_still_read() {
    let note = "n".repeat(100_000);
    let storage = MemoryStorage::new();
    assert!(
        annotations::add(
            &storage,
            BOOK,
            annotation(Kind::Highlight, (0, 0), (0, 5), "t", &note),
        )
        .is_ok()
    );
    assert!(
        annotations::add(
            &storage,
            BOOK,
            annotation(Kind::Highlight, (1, 0), (1, 5), "u", "short"),
        )
        .is_ok()
    );
    let all = annotations::load(&storage, BOOK);
    assert_eq!(all.len(), 2);
    assert!(all[0].note.len() <= 3 * 1024, "{}", all[0].note.len());
    assert!(all[0].note.starts_with("nnn") && all[0].note.ends_with('…'));
    assert_eq!((all[1].text.as_str(), all[1].note.as_str()), ("u", "short"));
}

/// A line far longer than any PaperPaw writes is read (cut) without
/// panics, and the line after it is read whole.
#[test]
fn a_huge_line_is_read_and_the_next_line_still_read() {
    let text = format!(
        "h\t1\t0\t1\t5\t3\t1\t2\ttext\t{}\nb\t2\t0\t2\t0\t9\t1\t2\tafter\t\n",
        "n".repeat(100_000)
    );
    let all = annotations::load(&with_file(text.as_bytes()), BOOK);
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].text, "text");
    assert!(all[0].note.starts_with("nnn") && all[0].note.len() < 100_000);
    assert_eq!(
        (all[1].mark.kind, all[1].text.as_str()),
        (Kind::Bookmark, "after")
    );
}

#[test]
fn duplicate_annotations_are_kept_and_the_first_gets_the_note() {
    let a = annotation(Kind::Highlight, (1, 10), (1, 20), "same", "");
    let storage = MemoryStorage::new();
    annotations::save(&storage, BOOK, &[a.clone(), a]).unwrap();
    assert!(annotations::set_note(&storage, BOOK, Locator::new(1, 10), "first", 9).unwrap());
    let all = annotations::load(&storage, BOOK);
    assert_eq!(all.len(), 2);
    assert_eq!(all.iter().filter(|a| a.note == "first").count(), 1);
    assert_eq!(
        annotations::note_of(&storage, BOOK, Locator::new(1, 10)),
        "first"
    );
    // Removing by position takes both.
    assert!(annotations::remove(&storage, BOOK, |m| m.start == Locator::new(1, 10)).unwrap());
    assert!(annotations::load(&storage, BOOK).is_empty());
    assert!(!storage.exists(&annotations::file(BOOK)));
}

#[test]
fn notes_go_on_highlights_only() {
    let storage = MemoryStorage::new();
    annotations::add(
        &storage,
        BOOK,
        annotation(Kind::Bookmark, (2, 0), (2, 0), "page", ""),
    )
    .unwrap();
    assert!(!annotations::set_note(&storage, BOOK, Locator::new(2, 0), "x", 1).unwrap());
    assert!(!annotations::set_note(&storage, BOOK, Locator::new(9, 0), "x", 1).unwrap());
    assert_eq!(annotations::note_of(&storage, BOOK, Locator::new(2, 0)), "");
    // An empty note takes the mark away.
    annotations::add(
        &storage,
        BOOK,
        annotation(Kind::Highlight, (3, 0), (3, 9), "h", "n"),
    )
    .unwrap();
    assert!(annotations::set_note(&storage, BOOK, Locator::new(3, 0), "  \t ", 2).unwrap());
    let h = annotations::load(&storage, BOOK).pop().unwrap();
    assert!(!h.mark.noted && h.note.is_empty());
}

#[test]
fn nothing_removed_leaves_the_file_as_it_is() {
    let text = b"h\t1\t0\t1\t5\t3\tunusual   spacing\t\n";
    let storage = with_file(text);
    assert!(!annotations::remove(&storage, BOOK, |_| false).unwrap());
    assert_eq!(storage.get(&annotations::file(BOOK)).unwrap(), text);
}

/// Lines this version does not read (a kind a newer PaperPaw added, or
/// damaged numbers) are written back as they are, whatever is changed,
/// and the file is not removed while they are in it.
#[test]
fn lines_it_cannot_read_survive_every_change() {
    let unknown = "u\t2\t10\t2\t20\t9\t0\t0\tunderlined by a newer version\tmy note";
    let damaged = "h\t1\tsoon\t1\t20\t5\t0\t0\ttext\tother note";
    let text = format!("h\t1\t10\t1\t20\t5\t0\t0\tsome text\t\n{unknown}\n{damaged}\n");
    let storage = with_file(text.as_bytes());
    let path = annotations::file(BOOK);
    let kept = |storage: &MemoryStorage| {
        let now = String::from_utf8(storage.get(&path).unwrap_or_default()).unwrap();
        now.lines().any(|l| l == unknown) && now.lines().any(|l| l == damaged)
    };
    assert!(annotations::set_note(&storage, BOOK, Locator::new(1, 10), "noted", 5).unwrap());
    assert!(kept(&storage));
    assert!(
        annotations::add(
            &storage,
            BOOK,
            annotation(Kind::Bookmark, (3, 0), (3, 0), "", ""),
        )
        .is_ok()
    );
    assert!(kept(&storage));
    assert!(annotations::remove(&storage, BOOK, |_| true).unwrap());
    assert!(kept(&storage), "the file was removed with lines in it");
    assert!(annotations::load(&storage, BOOK).is_empty());
}

#[test]
fn marks_overlap_only_where_they_share_characters() {
    let m = annotation(Kind::Highlight, (1, 10), (1, 20), "", "").mark;
    let at = |s, e| m.overlaps(Locator::new(1, s), Locator::new(1, e));
    assert!(at(15, 30) && at(0, 11) && at(12, 13) && at(0, 99));
    // Touching is no overlap: the end is just after the last character.
    assert!(!at(20, 30) && !at(0, 10));
    assert!(!m.overlaps(Locator::new(2, 0), Locator::new(2, 99)));
}

#[test]
fn files_are_per_book() {
    assert_ne!(annotations::file("a.epub"), annotations::file("b.epub"));
    assert!(annotations::file("a.epub").starts_with(".paperpaw/annotations/"));
    let storage = MemoryStorage::new();
    annotations::add(
        &storage,
        "a.epub",
        annotation(Kind::Bookmark, (1, 0), (1, 0), "", ""),
    )
    .unwrap();
    assert_eq!(annotations::marks(&storage, "a.epub").len(), 1);
    assert!(annotations::marks(&storage, "b.epub").is_empty());
}

#[test]
fn the_change_event_names_the_book() {
    use paperpaw_sdk::builtin::events;
    let paperpaw_sdk::Command::Emit(id, params) = annotations::changed("books/x.epub") else {
        panic!("an event");
    };
    let read = events::ANNOTATIONS_CHANGED.read(&id, &params).unwrap();
    assert_eq!(read.path, "books/x.epub");
}

/// Annotations kept beyond what is changed whole (1 MiB, e.g. a
/// reference book with thousands of noted highlights) are not written
/// over by the next one added: the addition is refused and the file left
/// as it is.
#[test]
fn an_oversized_file_is_not_overwritten_by_add() {
    let line = format!("h\t1\t0\t1\t5\t3\t1\t2\ttext\t{}\n", "n".repeat(1000));
    let big = line.repeat(1100);
    assert!(big.len() > 1 << 20);
    let storage = with_file(big.as_bytes());
    assert!(
        !annotations::add(
            &storage,
            BOOK,
            annotation(Kind::Bookmark, (9, 0), (9, 0), "new", ""),
        )
        .is_ok()
    );
    let kept = storage.get(&annotations::file(BOOK)).unwrap();
    assert!(
        kept == big.as_bytes(),
        "the annotations kept before are gone"
    );
}

/// A book in one conversion: each character `scale` bytes from `shift`
/// on (style runs in between, say), its XPointers `/p/text().<char>`
/// alike in all. Characters from `lost` on cannot be found by them.
struct Converted {
    scale: u32,
    shift: u32,
    lost: u32,
    metadata: Metadata,
}

impl Converted {
    fn new(scale: u32, shift: u32) -> Self {
        Self {
            scale,
            shift,
            lost: u32::MAX,
            metadata: Metadata::default(),
        }
    }

    fn at(&self, section: u32, char: u32) -> Locator {
        Locator::new(section, self.shift + char * self.scale)
    }
}

impl Document for Converted {
    fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    fn sections(&self) -> u32 {
        4
    }

    fn section_len(&self, _: u32) -> u32 {
        10_000
    }

    fn block_at(&self, _: &dyn Storage, _: Locator) -> Result<Option<Block>, DocumentError> {
        Ok(None)
    }

    /// Before the first character, the chapter's: found nowhere.
    fn xpointer(&self, _: &dyn Storage, at: Locator) -> Option<String> {
        let chapter = format!("/body/DocFragment[{}]/body", at.section + 1);
        Some(match at.offset.checked_sub(self.shift) {
            Some(bytes) => format!("{chapter}/p/text().{}", bytes / self.scale),
            None => chapter,
        })
    }

    fn locate_xpointer(&self, _: &dyn Storage, xpointer: &str) -> Option<Locator> {
        let rest = xpointer.strip_prefix("/body/DocFragment[")?;
        let (section, rest) = rest.split_once(']')?;
        let char: u32 = rest.strip_prefix("/body/p/text().")?.parse().ok()?;
        let section = section.parse::<u32>().ok()?.checked_sub(1)?;
        (char < self.lost).then(|| self.at(section, char))
    }
}

/// The two conversions of a book: with its own styles (conversion 1)
/// and without them (conversion 2).
fn conversions() -> (Converted, Converted) {
    (Converted::new(2, 10), Converted::new(1, 0))
}

/// A highlight of characters `from..to` in conversion `doc`.
fn made_in(doc: &Converted, from: u32, to: u32, note: &str) -> Annotation {
    let mut a = annotation(Kind::Highlight, (1, 0), (1, 0), "words", note);
    (a.mark.start, a.mark.end) = (doc.at(1, from), doc.at(1, to));
    a
}

/// A highlight made in one conversion of the book covers the same text
/// in another: found by its XPointer when the book is opened so, and
/// again when it is back in the first. Changes by those who know nothing
/// of conversions (a note, another annotation) keep where it was made.
#[test]
fn a_highlight_is_found_again_in_another_conversion() {
    let (styled, plain) = conversions();
    let storage = MemoryStorage::new();
    let first = made_in(&styled, 5, 9, "");
    assert!(annotations::add(&storage, BOOK, first.clone()).is_ok());
    assert_eq!(annotations::relocate(&storage, BOOK, 1, &styled), Ok(true));
    assert!(annotations::set_note(&storage, BOOK, first.mark.start, "mine", 7).unwrap());
    assert!(annotations::add(&storage, BOOK, made_in(&styled, 20, 30, "")).is_ok());
    assert_eq!(annotations::relocate(&storage, BOOK, 1, &styled), Ok(true));
    // Nothing to do in the same conversion: the file is not written.
    assert_eq!(annotations::relocate(&storage, BOOK, 1, &styled), Ok(false));
    assert_eq!(annotations::relocate(&storage, BOOK, 2, &plain), Ok(true));
    let all = annotations::load(&storage, BOOK);
    let places: Vec<_> = all.iter().map(|a| (a.mark.start, a.mark.end)).collect();
    assert_eq!(
        places,
        [
            (plain.at(1, 5), plain.at(1, 9)),
            (plain.at(1, 20), plain.at(1, 30))
        ]
    );
    assert_eq!((all[0].note.as_str(), all[0].updated), ("mine", 7));
    // Back with the styles: where they were made.
    assert_eq!(annotations::relocate(&storage, BOOK, 1, &styled), Ok(true));
    let all = annotations::load(&storage, BOOK);
    assert_eq!(
        (all[0].mark.start, all[0].mark.end),
        (first.mark.start, first.mark.end)
    );
    assert_eq!(all[1].mark.start, styled.at(1, 20));
}

/// Annotations kept before conversions were (version 1, 8 and 10
/// fields) may be made in any conversion: the one the book is opened in
/// first is a guess. So they are never taken as of one, and their places
/// never changed: each stays where it was made, with its text, note and
/// times, as before conversions were. Those added since are found again
/// in another conversion, and lines not understood stay as they are.
#[test]
fn annotations_of_before_are_never_moved_by_a_guess() {
    let (styled, plain) = conversions();
    let unknown = "u\t1\t12\t1\t20\t9\t0\t0\tunderlined by a newer version\tmy note";
    // Made with the book's own styles.
    let text = format!(
        "h\t1\t20\t1\t40\t12\tA note of old\n\
         b\t2\t10\t2\t10\t20\t1760000000\t1760000500\tChapter 2\t\n\
         {unknown}\n"
    );
    let storage = with_file(text.as_bytes());
    let before = annotations::load(&storage, BOOK);
    assert_eq!(before.len(), 2);
    // Opened without them first: nothing to do, the file stays as it is.
    assert_eq!(annotations::relocate(&storage, BOOK, 2, &plain), Ok(false));
    assert_eq!(
        storage.get(&annotations::file(BOOK)).unwrap(),
        text.as_bytes()
    );
    // One added there is of that conversion; those of before stay.
    let new = made_in(&plain, 50, 60, "new");
    assert!(annotations::add(&storage, BOOK, new).is_ok());
    assert_eq!(annotations::relocate(&storage, BOOK, 2, &plain), Ok(true));
    let file = String::from_utf8(storage.get(&annotations::file(BOOK)).unwrap()).unwrap();
    assert!(file.starts_with("# format 2\n"), "{file}");
    assert!(file.lines().any(|l| l == unknown), "{file}");
    assert_eq!(annotations::relocate(&storage, BOOK, 2, &plain), Ok(false));
    // Back with the styles: the new one is found again, those of before
    // are where they were made.
    assert_eq!(annotations::relocate(&storage, BOOK, 1, &styled), Ok(true));
    let all = annotations::load(&storage, BOOK);
    assert_eq!(all.len(), 3);
    assert_eq!(all[0], before[0]);
    assert_eq!(
        (all[1].mark.start, all[1].mark.end, all[1].note.as_str()),
        (styled.at(1, 50), styled.at(1, 60), "new")
    );
    assert_eq!(all[2], before[1]);
    // Changed by those who know nothing of conversions, they stay so.
    assert!(annotations::set_note(&storage, BOOK, before[0].mark.start, "changed", 9).unwrap());
    assert_eq!(annotations::relocate(&storage, BOOK, 2, &plain), Ok(true));
    let all = annotations::load(&storage, BOOK);
    assert_eq!(
        (all[0].mark.start, all[0].mark.end, all[0].note.as_str()),
        (before[0].mark.start, before[0].mark.end, "changed")
    );
    assert_eq!(all[1].mark.start, plain.at(1, 50));
    assert_eq!(all[2], before[1]);
}

/// An annotation that cannot be found in the conversion the book is
/// opened in is kept where it was, never removed, and found again in its
/// own.
#[test]
fn an_annotation_that_cannot_be_found_is_kept() {
    let (styled, mut plain) = conversions();
    plain.lost = 100;
    let storage = MemoryStorage::new();
    // Before the first character: its XPointer is found nowhere.
    let early = annotation(Kind::Bookmark, (1, 2), (1, 2), "first", "");
    let lost = made_in(&styled, 200, 210, "kept");
    annotations::save(
        &storage,
        BOOK,
        &[made_in(&styled, 5, 9, ""), lost.clone(), early.clone()],
    )
    .unwrap();
    assert_eq!(annotations::relocate(&storage, BOOK, 1, &styled), Ok(true));
    assert_eq!(annotations::relocate(&storage, BOOK, 2, &plain), Ok(true));
    let all = annotations::load(&storage, BOOK);
    assert_eq!(all.len(), 3);
    assert!(all.iter().any(|a| a.mark == early.mark));
    assert!(all.iter().any(|a| a.mark.start == plain.at(1, 5)));
    assert!(all.contains(&lost), "{all:?}");
    // Removing the others leaves it, and in its own conversion it is
    // where it was made.
    assert!(annotations::remove(&storage, BOOK, |m| m.start != lost.mark.start).unwrap());
    assert_eq!(annotations::relocate(&storage, BOOK, 1, &styled), Ok(false));
    assert_eq!(annotations::load(&storage, BOOK), [lost]);
}
