#![allow(clippy::unwrap_used, reason = "fixture failures must fail tests")]
//! The annotation file read a line at a time: ids, shapes, windows.
extern crate std;
use super::*;
use crate::MemoryStorage;
use std::format;
use std::string::{String, ToString};
use std::vec::Vec;

const BOOK: &str = "books/a.epub";

fn put(text: &str) -> MemoryStorage {
    MemoryStorage::new().with(&super::super::file(BOOK), text.as_bytes())
}

/// What the test keeps of a row.
#[derive(Debug, PartialEq)]
struct Got {
    id: u32,
    kind: Kind,
    start: (u32, u32),
    percent: u8,
    conversion: Option<u64>,
    text: (String, u32),
    note: (String, u32),
    key: (String, u32),
    xp: (u32, u32),
}

fn rows(storage: &MemoryStorage, windows: &Windows) -> Result<Vec<Got>, Error> {
    let mut scratch = std::vec![0_u8; windows.scratch()];
    let mut all = Vec::new();
    let seen = try_rows(storage, BOOK, windows, &mut scratch, &mut |index, row| {
        assert_eq!(index as usize, all.len());
        all.push(Got {
            id: row.id,
            kind: row.kind,
            start: (row.start.section, row.start.offset),
            percent: row.percent,
            conversion: row.conversion,
            text: (row.text.text.to_string(), row.text.whole),
            note: (row.note.text.to_string(), row.note.whole),
            key: (row.key.text.to_string(), row.key.whole),
            xp: (row.start_xpointer.whole, row.end_xpointer.whole),
        });
        ControlFlow::Continue(())
    })?;
    assert_eq!(seen as usize, all.len());
    Ok(all)
}

fn wide() -> Windows {
    Windows {
        text: Window::new(0, 512),
        note: Window::new(0, 512),
        key: Window::new(0, 64),
        ..Windows::default()
    }
}

#[test]
fn a_file_of_version_2_has_ids_by_place_and_no_generation() {
    let v2 = String::from(
        "# format 2\n\
         b\t1\t0\t1\t0\t10\t100\t200\t00000000000000ff\t/a\t\tfirst\tnote\n\
         garbage line\n\
         h\t2\t5\t2\t9\t20\t0\t0\t\t\t\tsecond\t\n\
         x\t3\t0\t3\t0\t30\t1\t2\t?\t/p\t/q\tfuture\tn\n\
         h\t4\tzz\t4\t0\t40\t0\t0\t\t\t\tbad number\t\n\
         # book 5 abc\n\
         h\t5\t0\t5\t0\t50\t0\t0\t\t\t\tlast\t\n",
    );
    let storage = put(&v2);
    let head = try_head(&storage, BOOK).unwrap();
    assert_eq!((head.version, head.generation), (2, 1));
    let first = rows(&storage, &wide()).unwrap();
    // Garbage and the bad number are no rows and take no id.
    let ids: Vec<u32> = first.iter().map(|r| r.id).collect();
    assert_eq!(ids, [1, 2, 3, 4]);
    assert_eq!(first[0].kind, Kind::Bookmark);
    assert_eq!(first[0].conversion, Some(255));
    assert_eq!(first[0].xp, (2, 0));
    assert_eq!(first[0].note, ("note".to_string(), 4));
    assert_eq!(first[1].kind, Kind::Highlight);
    assert_eq!(first[1].conversion, None);
    // A kind this build does not know is a row, by its letter.
    assert_eq!(first[2].kind, Kind::Other(u16::from(b'x')));
    assert_eq!(first[2].conversion, None, "the conversion is not known (?)");
    assert_eq!(first[2].xp, (2, 2));
    assert_eq!(first[3].text, ("last".to_string(), 4));
    // Read twice: the same ids.
    assert_eq!(rows(&storage, &wide()).unwrap(), first);
}

#[test]
fn a_file_of_version_1_has_no_times_or_conversions() {
    let v1 = "b\t1\t0\t1\t0\t10\t5\t6\tshown\tnoted\n\
              h\t2\t0\t2\t4\t90\tshort\tn\n";
    let got = rows(&put(v1), &wide()).unwrap();
    assert_eq!(
        got.iter().map(|r| (r.id, r.percent)).collect::<Vec<_>>(),
        [(1, 10), (2, 90)]
    );
    assert_eq!(got[0].text.0, "shown");
    assert_eq!(got[1].text.0, "short");
    assert_eq!(try_head(&put(v1), BOOK).unwrap().version, 1);
}

#[test]
fn a_file_of_version_3_has_ids_keys_and_a_generation() {
    let v3 = "# format 3\n# book 5 abc\n# next-id 9\n# generation 7\n\
              h\t1\t0\t1\t4\t10\t1\t2\t00000000000000ff\t/a\t/b\ttext\tnote\t4\tdt-1\n\
              b\t2\t0\t2\t0\t20\t0\t0\t\t\t\t\t\t8\t\n\
              h\t3\t0\t3\t4\t30\t0\t0\t\t\t\tv2 line\t\n\
              h\t4\t0\t4\t4\t30\t0\t0\t\t\t\tno id\t\t0\t\n";
    let storage = put(v3);
    let head = try_head(&storage, BOOK).unwrap();
    assert_eq!((head.version, head.generation), (3, 7));
    let got = rows(&storage, &wide()).unwrap();
    // The 13-field line has no id and the id 0 is none: not rows.
    assert_eq!(got.iter().map(|r| r.id).collect::<Vec<_>>(), [4, 8]);
    assert_eq!(got[0].key, ("dt-1".to_string(), 4));
    assert_eq!(got[0].conversion, Some(255));
    assert_eq!(got[1].key, (String::new(), 0));
}

#[test]
fn a_later_version_is_refused_and_a_missing_file_has_no_rows() {
    assert_eq!(try_head(&put("# format 4\n"), BOOK), Err(Error::Newer(4)));
    assert_eq!(rows(&put("# format 4\n"), &wide()), Err(Error::Newer(4)));
    let none = MemoryStorage::new();
    assert_eq!(rows(&none, &wide()), Ok(Vec::new()));
    let head = try_head(&none, BOOK).unwrap();
    assert_eq!((head.version, head.generation), (1, 1));
}

#[test]
fn a_long_note_is_read_in_pieces_cut_at_a_character_with_its_whole_length() {
    // 700 bytes of three-byte characters in the note.
    let note = "€".repeat(233);
    let file = format!("# format 2\nh\t1\t0\t1\t4\t10\t0\t0\t\t\t\tt\t{note}\n");
    let storage = put(&file);
    let got = rows(&storage, &wide()).unwrap();
    assert_eq!(got[0].note.1, 699);
    // 512 bytes hold 170 characters.
    assert_eq!(got[0].note.0, "€".repeat(170));
    // From byte 510 on: the rest.
    let tail = Windows {
        note: Window::new(510, 1024),
        ..Windows::default()
    };
    let got = rows(&storage, &tail).unwrap();
    assert_eq!(got[0].note.0, "€".repeat(233 - 170));
    assert_eq!(got[0].text, (String::new(), 1));
}

#[test]
fn a_visitor_may_stop_the_read_and_a_small_scratch_is_refused() {
    let file = "b\t1\t0\t1\t0\t1\ta\tb\nb\t2\t0\t2\t0\t1\ta\tb\nb\t3\t0\t3\t0\t1\ta\tb\n";
    let storage = put(file);
    let mut scratch = std::vec![0_u8; 8];
    let mut ids = Vec::new();
    let seen = try_rows(
        &storage,
        BOOK,
        &Windows::default(),
        &mut scratch,
        &mut |_, row| {
            ids.push(row.id);
            if ids.len() == 2 {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        },
    )
    .unwrap();
    assert_eq!((seen, ids), (2, std::vec![1, 2]));
    let mut small = std::vec![0_u8; 10];
    assert_eq!(
        try_rows(&storage, BOOK, &wide(), &mut small, &mut |_, _| {
            ControlFlow::Continue(())
        }),
        Err(Error::Unreadable)
    );
}

#[test]
fn a_line_too_long_or_a_key_too_long_is_no_row() {
    let long = "x".repeat(LINE_MAX);
    let file = format!(
        "# format 3\n\
         b\t1\t0\t1\t0\t1\t0\t0\t\t\t\t{long}\t\t1\t\n\
         b\t1\t0\t1\t0\t1\t0\t0\t\t\t\tt\t\t2\t{}\n\
         b\t1\t0\t1\t0\t1\t0\t0\t\t\t\tt\t\t3\tok\n",
        "k".repeat(65)
    );
    let got = rows(&put(&file), &wide()).unwrap();
    assert_eq!(got.iter().map(|r| r.id).collect::<Vec<_>>(), [3]);
}

#[test]
fn reading_makes_no_allocation_but_the_file_name() {
    use crate::native::commands::menu_alloc_tests::refuse_nth;
    let storage = put("# format 3\n# generation 2\nb\t1\t0\t1\t0\t1\t0\t0\t\t\t\tt\tn\t1\tk\n");
    let windows = wide();
    let mut scratch = std::vec![0_u8; windows.scratch()];
    let mut read = || {
        try_rows(&storage, BOOK, &windows, &mut scratch, &mut |_, _| {
            ControlFlow::Continue(())
        })
    };
    let (result, refused) = refuse_nth(1, &mut read);
    assert!(refused);
    assert_eq!(result, Err(Error::OutOfMemory));
    // The name is the only allocation.
    let (result, refused) = refuse_nth(2, &mut read);
    assert!(!refused);
    assert_eq!(result, Ok(1));
    let (head, refused) = refuse_nth(1, || try_head(&storage, BOOK));
    assert!(refused);
    assert_eq!(head, Err(Error::OutOfMemory));
}

#[test]
fn the_generation_is_never_0_or_1_after_a_change() {
    // Missing, version 1 and 2, and a header that says 0 or 1: generation 1.
    assert_eq!(generation_of(1, None), 1);
    assert_eq!(
        generation_of(2, Some(9)),
        1,
        "an older file has no header to trust"
    );
    assert_eq!(generation_of(3, None), 1);
    assert_eq!(generation_of(3, Some(0)), 1);
    assert_eq!(generation_of(3, Some(1)), 1);
    assert_eq!(generation_of(3, Some(7)), 7);
    // A change adds one; a wrap skips 0 and 1; the first write converts to 2
    // and the change makes it 3.
    assert_eq!(next_generation(1), 2);
    assert_eq!(next_generation(2), 3);
    assert_eq!(next_generation(u32::MAX - 1), u32::MAX);
    assert_eq!(next_generation(u32::MAX), 2);
    assert_eq!(next_generation(0), 2);
}

#[test]
fn is_row_is_the_definition_try_rows_reads_by() {
    // The writer of version 3 calls `is_row`; it must agree with the reader
    // on every line of a mixed file, in every version.
    let lines = [
        "b\t1\t0\t1\t0\t10\t100\t200\t00000000000000ff\t/a\t\tfirst\tnote\t7\tkey",
        "b\t1\t0\t1\t0\t10\t100\t200\t00000000000000ff\t/a\t\tfirst\tnote",
        "h\t2\t5\t2\t9\t20\t0\t0\t\t\t\tsecond\t",
        "h\t2\t5\t2\t9\t20\t1\t2\ttext\tnote",
        "h\t2\t5\t2\t9\t20\ttext\tnote",
        "x\t3\t0\t3\t0\t30\t1\t2\t?\t/p\t/q\tfuture\tn",
        "h\t4\tzz\t4\t0\t40\t0\t0\t\t\t\tbad number\t",
        "# book 5 abc",
        "garbage line",
        "",
        "bb\t1\t0\t1\t0\t10\t0\t0\t\t\t\tt\tn",
    ];
    for version in 1..=3_u32 {
        let mut file = format!("# format {version}\n");
        let expected: Vec<bool> = lines
            .iter()
            .map(|l| is_row(l.as_bytes(), version))
            .collect();
        for line in lines {
            file.push_str(line);
            file.push('\n');
        }
        let storage = put(&file);
        let counted = rows(&storage, &wide()).unwrap().len();
        assert_eq!(
            counted,
            expected.iter().filter(|r| **r).count(),
            "version {version}"
        );
    }
    assert!(!is_row(b"# b\t1", 3));
    assert!(!is_row(&[b'b'; LINE_MAX + 1], 3));
}
