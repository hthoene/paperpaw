use super::*;
use crate::MemoryStorage;
use crate::annotations::{self, try_rows};
use alloc::format;
use alloc::string::ToString;
use alloc::vec;

const BOOK: &str = "books/a.epub";
const TMP: &str = ".paperpaw/library/edits/t.new";

fn file() -> String {
    annotations::file(BOOK)
}

fn text(storage: &MemoryStorage) -> String {
    let mut buf = vec![0_u8; 1 << 16];
    let n = storage.read_at(&file(), 0, &mut buf).unwrap();
    String::from_utf8(buf[..n].to_vec()).unwrap()
}

/// Rows the reader sees: (id, start section, start offset, text).
fn rows(storage: &MemoryStorage) -> Vec<(u32, u32, u32, String)> {
    let windows = Windows::new().text(Window::new(0, 512));
    let mut scratch = vec![0_u8; 512];
    let mut found = Vec::new();
    try_rows(storage, BOOK, &windows, &mut scratch, &mut |_, row| {
        found.push((
            row.id,
            row.start.section,
            row.start.offset,
            row.text.text.to_string(),
        ));
        ControlFlow::Continue(())
    })
    .unwrap();
    found
}

/// What the test's host does: drops ids, adds rows.
struct Plan {
    drop: Vec<u32>,
    news: Vec<(u32, Locator, &'static str)>,
}

impl Edits for Plan {
    fn keeps(&mut self, row: &Existing<'_>) -> Result<bool, Error> {
        Ok(!self.drop.contains(&row.id))
    }
    fn next_new(
        &mut self,
        limit: Option<(Locator, Locator)>,
        write: &mut dyn FnMut(&Spec<'_>) -> Result<(), Error>,
    ) -> Result<bool, Error> {
        let Some(first) = self.news.first().copied() else {
            return Ok(false);
        };
        if limit.is_some_and(|(start, _)| first.1 > start) {
            return Ok(false);
        }
        self.news.remove(0);
        let mut spec = Spec::new(first.0, Kind::Highlight, first.1, first.1);
        spec.text = first.2;
        write(&spec)?;
        Ok(true)
    }
}

const V2: &str = "# format 2\n# book 10 abcd\n\
h\t1\t10\t1\t20\t5\t100\t200\t\t\t\tfirst\tnote one\n\
b\t3\t0\t3\t0\t50\t300\t300\t00000000000000ab\ta\tb\tsecond\t\n\
x\tfrom a later build\n";

#[test]
fn a_version_2_file_is_converted_with_ids_by_place_and_a_generation_of_3() {
    let storage = MemoryStorage::new().with(&file(), V2);
    let info = try_info(&storage, &file()).unwrap();
    assert_eq!(
        (info.version, info.generation, info.next_id, info.rows),
        (2, 1, 3, 2)
    );
    let mut plan = Plan {
        drop: vec![],
        news: vec![(3, Locator::new(2, 0), "added")],
    };
    let generation = rewrite(&storage, &file(), TMP, (&info, 1), &mut plan).unwrap();
    assert_eq!(generation, 3);
    let after = text(&storage);
    assert!(
        after.starts_with("# format 3\n# next-id 4\n# generation 3\n# book 10 abcd\n"),
        "{after}"
    );
    // The unknown kind is kept where it was, byte for byte.
    assert!(after.contains("x\tfrom a later build\n"));
    assert_eq!(
        rows(&storage),
        vec![
            (1, 1, 10, "first".into()),
            (3, 2, 0, "added".into()),
            (2, 3, 0, "second".into()),
        ]
    );
    assert!(storage.read_at(TMP, 0, &mut [0; 1]).is_err());
    let again = try_info(&storage, &file()).unwrap();
    assert_eq!((again.version, again.generation, again.next_id), (3, 3, 4));
}

/// A byte order mark before the mark of version 2 goes with that line: the
/// file of version 3 has no second, older mark that would say version 2.
#[test]
fn a_version_2_file_with_a_byte_order_mark_is_converted_whole() {
    let v2 = alloc::format!("\u{feff}{V2}").replace('\n', "\r\n");
    let storage = MemoryStorage::new().with(&file(), v2.as_str());
    let info = try_info(&storage, &file()).unwrap();
    assert_eq!((info.version, info.rows), (2, 2));
    rewrite(&storage, &file(), TMP, (&info, 0), &mut nothing()).unwrap();
    let after = text(&storage);
    assert!(
        after.starts_with("# format 3\n# next-id 3\n# generation 3\n# book 10 abcd\n"),
        "{after}"
    );
    assert!(
        !after.contains("format 2") && !after.contains('\r'),
        "{after}"
    );
    assert_eq!(try_info(&storage, &file()).unwrap().version, 3);
    assert_eq!(
        rows(&storage),
        vec![(1, 1, 10, "first".into()), (2, 3, 0, "second".into())]
    );
}

#[test]
fn a_version_3_file_keeps_its_rows_byte_for_byte_and_drops_the_removed() {
    let v3 = "# format 3\n# next-id 5\n# generation 7\n\
h\t1\t10\t1\t20\t5\t100\t200\t\t\t\tone\t\t1\t\n\
h\t2\t0\t2\t5\t5\t100\t200\t\t\t\ttwo\t\t4\tk\n\
junk line\n";
    let storage = MemoryStorage::new().with(&file(), v3);
    let info = try_info(&storage, &file()).unwrap();
    assert_eq!((info.version, info.generation, info.next_id), (3, 7, 5));
    let mut plan = Plan {
        drop: vec![1],
        news: vec![],
    };
    assert_eq!(
        rewrite(&storage, &file(), TMP, (&info, 0), &mut plan).unwrap(),
        8
    );
    assert_eq!(
        text(&storage),
        "# format 3\n# next-id 5\n# generation 8\nh\t2\t0\t2\t5\t5\t100\t200\t\t\t\ttwo\t\t4\tk\njunk line\n"
    );
}

#[test]
fn a_missing_file_is_created_at_generation_3_and_a_failed_write_changes_nothing() {
    let storage = MemoryStorage::new();
    let info = try_info(&storage, &file()).unwrap();
    assert_eq!(
        (info.version, info.generation, info.next_id, info.rows),
        (1, 1, 1, 0)
    );
    let mut plan = Plan {
        drop: vec![],
        news: vec![(1, Locator::new(1, 1), "x")],
    };
    assert_eq!(
        rewrite(&storage, &file(), TMP, (&info, 1), &mut plan).unwrap(),
        3
    );
    assert_eq!(rows(&storage), vec![(1, 1, 1, "x".into())]);
    let before = text(&storage);
    // A row that is not a line the reader takes: nothing is changed.
    let info = try_info(&storage, &file()).unwrap();
    let long = "y".repeat(LINE_MAX);
    let leaked: &'static str = alloc::boxed::Box::leak(long.into_boxed_str());
    let mut plan = Plan {
        drop: vec![],
        news: vec![(2, Locator::new(9, 9), leaked)],
    };
    let error = rewrite(&storage, &file(), TMP, (&info, 1), &mut plan).unwrap_err();
    assert_eq!(error, Error::Storage(StorageError::TooLarge));
    assert_eq!(text(&storage), before);
    assert!(!storage.exists(TMP));
}

#[test]
fn a_row_is_one_line_and_reads_back_whole() {
    let mut spec = Spec::new(7, Kind::Highlight, Locator::new(1, 2), Locator::new(3, 4));
    spec.percent = 42;
    spec.created = 5;
    spec.updated = 6;
    spec.conversion = Some(0xab);
    spec.start_xpointer = "/a/b";
    spec.end_xpointer = "/a/c";
    spec.text = "two\tlines\nof text";
    spec.note = "note";
    spec.key = "client-1";
    let mut line = Vec::new();
    format_row(&spec, &mut line).unwrap();
    let line = String::from_utf8(line).unwrap();
    assert_eq!(
        line,
        "h\t1\t2\t3\t4\t42\t5\t6\t00000000000000ab\t/a/b\t/a/c\ttwo lines of text\tnote\t7\tclient-1"
    );
    let storage = MemoryStorage::new().with(
        &file(),
        alloc::format!("# format 3\n# next-id 8\n# generation 2\n{line}\n"),
    );
    let windows = Windows::new()
        .text(Window::new(0, 64))
        .key(Window::new(0, 64));
    let mut scratch = vec![0_u8; 128];
    let mut seen = 0;
    try_rows(&storage, BOOK, &windows, &mut scratch, &mut |_, row| {
        assert_eq!(
            (row.id, row.key.text, row.text.text),
            (7, "client-1", "two lines of text")
        );
        assert_eq!(row.conversion, Some(0xab));
        seen += 1;
        ControlFlow::Continue(())
    })
    .unwrap();
    assert_eq!(seen, 1);
    // A key with a tab, a kind without a letter and id 0 are refused.
    let mut bad = spec;
    bad.key = "a\tb";
    assert!(format_row(&bad, &mut Vec::new()).is_err());
    bad = spec;
    bad.id = 0;
    assert!(format_row(&bad, &mut Vec::new()).is_err());
}

/// An end XPointer without a start is refused: the reader would take the
/// row for damage, and the committed annotation would never be seen.
#[test]
fn an_end_xpointer_without_a_start_is_refused() {
    let mut spec = Spec::new(1, Kind::Highlight, Locator::new(1, 2), Locator::new(1, 4));
    spec.end_xpointer = "/x";
    assert_eq!(format_row(&spec, &mut Vec::new()), Err(Error::Unreadable));
    spec.start_xpointer = "  ";
    assert_eq!(format_row(&spec, &mut Vec::new()), Err(Error::Unreadable));
    spec.start_xpointer = "/a";
    assert!(format_row(&spec, &mut Vec::new()).is_ok());
}

/// A row kept before conversions were (every one of version 1, one marked
/// `?` in version 2) stays one whose conversion is not known; one added
/// since (none) stays so, and a known one keeps it.
#[test]
fn a_conversion_not_known_stays_not_known_when_converted() {
    let conversions = |storage: &MemoryStorage| -> Vec<String> {
        let all = text(storage);
        all.lines()
            .filter(|line| !line.starts_with("# "))
            .map(|line| line.split('\t').nth(8).unwrap().to_string())
            .collect()
    };
    let v2 = "# format 2\n\
h\t1\t0\t1\t4\t5\t0\t0\t?\t/a\t/b\tkept before\t\n\
h\t2\t0\t2\t4\t5\t0\t0\t\t\t\tadded since\t\n\
h\t3\t0\t3\t4\t5\t0\t0\t00000000000000ab\t\t\tknown\t\n";
    let v1 = "h\t1\t0\t1\t4\t5\tversion 1\tnote\n";
    for (old, after) in [(v2, vec!["?", "", "00000000000000ab"]), (v1, vec!["?"])] {
        let storage = MemoryStorage::new().with(&file(), old);
        let info = try_info(&storage, &file()).unwrap();
        rewrite(&storage, &file(), TMP, (&info, 0), &mut nothing()).unwrap();
        assert_eq!(conversions(&storage), after, "{old}");
        assert_eq!(annotations::load(&storage, BOOK).len(), after.len());
    }
}

#[test]
fn a_file_of_a_later_version_is_not_written_over() {
    let storage = MemoryStorage::new().with(&file(), "# format 9\nh\t1\n");
    assert!(matches!(try_info(&storage, &file()), Err(Error::Newer(9))));
}

#[test]
fn the_file_of_a_key_is_the_file_of_the_path() {
    let key = u64::from_str_radix(&crate::library::key(BOOK), 16).unwrap();
    assert_eq!(file_of_key(key).unwrap(), file());
}

const ROW: &str = "h\t1\t10\t1\t20\t5\t100\t200\t\t\t\tone\t\t1\t\n";

fn nothing() -> Plan {
    Plan {
        drop: vec![],
        news: vec![],
    }
}

#[test]
fn a_line_longer_than_the_reader_takes_is_kept_unchanged_by_every_rewrite() {
    let junk = "j".repeat(LINE_MAX * 2 + 5);
    let v3 = format!("# format 3\n# next-id 2\n# generation 3\n{junk}\n{ROW}");
    let storage = MemoryStorage::new().with(&file(), v3.as_str());
    for generation in [4, 5] {
        let info = try_info(&storage, &file()).unwrap();
        let got = rewrite(&storage, &file(), TMP, (&info, 0), &mut nothing()).unwrap();
        assert_eq!(got, generation);
        let after = text(&storage);
        assert_eq!(
            after,
            format!("# format 3\n# next-id 2\n# generation {generation}\n{junk}\n{ROW}")
        );
        assert!(!after.contains("\n\n"));
    }
}

#[test]
fn an_unterminated_last_line_gets_one_newline_before_a_new_row() {
    let v3 = format!("# format 3\n# next-id 2\n# generation 3\n{ROW}junk");
    let storage = MemoryStorage::new().with(&file(), v3.as_str());
    let info = try_info(&storage, &file()).unwrap();
    let mut plan = Plan {
        drop: vec![],
        news: vec![(2, Locator::new(9, 0), "late")],
    };
    rewrite(&storage, &file(), TMP, (&info, 1), &mut plan).unwrap();
    let after = text(&storage);
    assert!(after.contains("junk\nh\t"), "{after}");
    assert!(
        !after.contains("\n\n") && !after.contains("junkh"),
        "{after}"
    );
    let info = try_info(&storage, &file()).unwrap();
    rewrite(&storage, &file(), TMP, (&info, 0), &mut nothing()).unwrap();
    assert!(text(&storage).ends_with('\n'));
}

#[test]
fn a_stale_tmp_file_is_not_appended_to() {
    let v3 = format!("# format 3\n# next-id 2\n# generation 3\n{ROW}");
    let storage = MemoryStorage::new()
        .with(&file(), v3.as_str())
        .with(TMP, "STALE\nh\tbroken\n");
    let info = try_info(&storage, &file()).unwrap();
    rewrite(&storage, &file(), TMP, (&info, 0), &mut nothing()).unwrap();
    assert_eq!(
        text(&storage),
        format!("# format 3\n# next-id 2\n# generation 4\n{ROW}")
    );
    assert!(!storage.exists(TMP));
}

#[test]
fn the_generation_wraps_past_the_top_to_2_never_0_or_1() {
    let v3 = format!("# format 3\n# next-id 2\n# generation {}\n{ROW}", u32::MAX);
    let storage = MemoryStorage::new().with(&file(), v3.as_str());
    let info = try_info(&storage, &file()).unwrap();
    assert_eq!(generation_after(&info), 2);
    assert_eq!(
        rewrite(&storage, &file(), TMP, (&info, 0), &mut nothing()).unwrap(),
        2
    );
    assert!(text(&storage).starts_with("# format 3\n# next-id 2\n# generation 2\n"));
}
