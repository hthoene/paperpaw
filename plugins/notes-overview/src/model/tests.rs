use super::*;
use alloc::string::ToString;

fn note(
    kind: Kind,
    percent: u8,
    created: i64,
    text: &'static str,
    own: &'static str,
) -> Note<'static> {
    Note {
        kind,
        percent,
        created,
        updated: created,
        text,
        note: own,
    }
}

fn date(local: i64) -> String {
    let mut out = String::new();
    put_date(&mut out, local).unwrap();
    out
}

#[test]
fn dates_are_as_the_kindle_says_them() {
    assert_eq!(date(1_793_527_500), "Sunday, 1 November 2026 10:05:00");
    assert_eq!(date(0), "Thursday, 1 January 1970 00:00:00");
    assert_eq!(date(951_782_399), "Monday, 28 February 2000 23:59:59");
    assert_eq!(date(951_782_400), "Tuesday, 29 February 2000 00:00:00");
}

#[test]
fn a_highlight_with_a_note_is_two_entries_a_bookmark_has_no_text() {
    let all = [
        note(
            Kind::Highlight,
            42,
            1_793_527_500,
            "The wind\ncame  over",
            "so true",
        ),
        note(Kind::Bookmark, 50, 0, "p. 12", ""),
        note(Kind::Other, 60, 0, "x", ""),
    ];
    let mut out = String::new();
    let count = put_book(&mut out, &|t| t, "A Quiet Year", "Ann Reed", &all).unwrap();
    assert_eq!(count, 3);
    assert_eq!(
        out,
        "A Quiet Year (Ann Reed)\r\n\
         - Your Highlight at location 42-42 | Added on Sunday, 1 November 2026 10:05:00\r\n\
         \r\n\
         The wind came over\r\n\
         ==========\r\n\
         A Quiet Year (Ann Reed)\r\n\
         - Your Note at location 42 | Added on Sunday, 1 November 2026 10:05:00\r\n\
         \r\n\
         so true\r\n\
         ==========\r\n\
         A Quiet Year (Ann Reed)\r\n\
         - Your Bookmark at location 50 | Added on Thursday, 1 January 1970 00:00:00\r\n\
         \r\n\
         \r\n\
         ==========\r\n"
    );
}

#[test]
fn a_book_without_an_author_is_named_by_its_title() {
    let line = |t, a| {
        let mut out = String::new();
        put_book_line(&mut out, t, a).unwrap();
        out
    };
    assert_eq!(line("  Two\nlines ", ""), "Two lines");
    assert_eq!(line("T", " A \n B"), "T (A B)");
    assert_eq!(line("T", "  \n "), "T");
}

#[test]
fn the_local_time_is_asked_for_a_known_time_only() {
    let all = [note(Kind::Bookmark, 1, 0, "", "")];
    let mut out = String::new();
    put_book(&mut out, &|_| panic!("0 has no local time"), "T", "", &all).unwrap();
    assert!(out.contains("Thursday, 1 January 1970 00:00:00"));
    let updated = Note {
        created: 0,
        updated: 86_400,
        ..all[0]
    };
    let mut out = String::new();
    put_book(&mut out, &|t| t + 3600, "T", "", &[updated]).unwrap();
    assert!(out.contains("Friday, 2 January 1970 01:00:00"));
}

#[test]
fn counting_skips_the_kinds_it_does_not_know() {
    let all = [
        note(Kind::Highlight, 1, 0, "", ""),
        note(Kind::Bookmark, 1, 0, "", ""),
        note(Kind::Highlight, 1, 0, "", ""),
        note(Kind::Other, 1, 0, "", ""),
    ];
    assert_eq!(count(&all), (2, 1));
}

#[test]
fn the_list_keeps_200_books_by_title_and_says_when_it_left_some_out() {
    let mut list = Listing::new();
    assert_eq!(list.rows(), 0);
    list.add("/b", "banana", 1, 0).unwrap();
    list.add("/a", "Apple", 0, 2).unwrap();
    list.add("/n", "none", 0, 0).unwrap();
    list.add("/a2", "apple", 1, 1).unwrap();
    list.sort();
    let order: Vec<_> = list.books().iter().map(|b| b.path.as_str()).collect();
    assert_eq!(order, ["/a", "/a2", "/b"]);
    assert_eq!(list.rows(), 4);
    assert!(!list.more());
    for i in 0..MOST_BOOKS {
        list.add(&i.to_string(), "x", 1, 0).unwrap();
    }
    assert_eq!(list.books().len(), MOST_BOOKS);
    assert!(list.more());
}

#[test]
fn counts_show_only_what_there_is() {
    let book = |h, b| Counted {
        path: String::new(),
        title: String::new(),
        highlights: h,
        bookmarks: b,
    };
    assert_eq!(
        count_parts(&book(3, 1)),
        [
            Some(("notes-overview.highlights", 3)),
            Some(("notes-overview.bookmarks", 1))
        ]
    );
    assert_eq!(count_parts(&book(0, 2))[0], None);
    assert_eq!(count_parts(&book(2, 0))[1], None);
}

#[test]
fn the_export_goes_beside_the_library_folder() {
    assert_eq!(export_folder("Books").unwrap(), "Exports");
    assert_eq!(export_folder("/Books/").unwrap(), "Exports");
    assert_eq!(export_folder("card/Books").unwrap(), "card/Exports");
    assert_eq!(export_folder("a/b/Books").unwrap(), "a/b/Exports");
    assert_eq!(export_folder("").unwrap(), "Exports");
}

#[test]
fn numbers_are_padded_and_signed() {
    let put = |v, w| {
        let mut out = String::new();
        put_number(&mut out, v, w).unwrap();
        out
    };
    assert_eq!(put(5, 2), "05");
    assert_eq!(put(0, 1), "0");
    assert_eq!(put(2026, 1), "2026");
    assert_eq!(put(-44, 1), "-44");
    assert_eq!(put(i64::MIN, 1), "-9223372036854775808");
}

/// A sink that refuses after `room` bytes.
struct Tight {
    room: usize,
    got: String,
}

impl Sink for Tight {
    fn put(&mut self, text: &str) -> Result<(), Error> {
        if text.len() > self.room {
            return Err(Error::NoMemory);
        }
        self.room -= text.len();
        self.got.push_str(text);
        Ok(())
    }
}

#[test]
fn every_refusal_of_the_sink_comes_back_and_none_panics() {
    let all = [
        note(
            Kind::Highlight,
            42,
            1_793_527_500,
            "The wind came over",
            "so true",
        ),
        note(Kind::Bookmark, 50, 0, "", ""),
    ];
    let mut whole = String::new();
    put_book(&mut whole, &|t| t, "A Quiet Year", "Ann Reed", &all).unwrap();
    for room in 0..whole.len() {
        let mut tight = Tight {
            room,
            got: String::new(),
        };
        assert_eq!(
            put_book(&mut tight, &|t| t, "A Quiet Year", "Ann Reed", &all),
            Err(Error::NoMemory)
        );
        assert!(whole.starts_with(&tight.got));
    }
    let mut exact = Tight {
        room: whole.len(),
        got: String::new(),
    };
    assert_eq!(
        put_book(&mut exact, &|t| t, "A Quiet Year", "Ann Reed", &all),
        Ok(3)
    );
    assert_eq!(exact.got, whole);
}

#[test]
fn strange_dates_and_percentages_do_not_panic() {
    for local in [i64::MIN, i64::MAX, -1, 253_402_300_800] {
        let mut out = String::new();
        let _ = put_date(&mut out, local.clamp(-1 << 50, 1 << 50));
        assert!(!out.is_empty());
    }
    let wild = Note {
        percent: 255,
        ..note(Kind::Highlight, 0, i64::MAX, "t", "")
    };
    let mut out = String::new();
    put_book(&mut out, &|t| t.clamp(-1 << 50, 1 << 50), "T", "", &[wild]).unwrap();
}
