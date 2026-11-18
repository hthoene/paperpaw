//! The readers of Wallabag's answers (`native_fetch`): the same result of the
//! same text at any piece size, and refusing instead of aborting when memory
//! runs short. The read caps of the lists are those of interface 6.
use crate::fail_alloc::sweep;
use crate::native_fetch::{Archived, Unread};
use crate::native_wallabag::{self as native, ARTICLES_CAP, FINISHED_CAP};
use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;
use paperpaw_sdk::native::Error;

fn entries(items: &[(&str, &str)]) -> String {
    let mut out =
        String::from(r#"{"page":1,"limit":30,"pages":1,"total":9,"_embedded":{"items":["#);
    for (i, (id, title)) in items.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&format!(
            r#"{{"is_archived":0,"id":{id},"title":"{title}","url":"https://example.org/{id}","tags":[{{"id":3,"label":"x"}}],"_links":{{"self":{{"href":"/api/entries/{id}"}}}}}}"#
        ));
    }
    out.push_str("]}}");
    out
}

type Names = Vec<(String, String)>;

/// A title as the book's name is made of it.
fn name(title: &str) -> String {
    paperpaw_sdk::web::clean_name(title, native::NAME_UNITS).unwrap_or_default()
}

fn new_unread(text: &str) -> Option<Names> {
    Unread::read(text.as_bytes())
        .unwrap()
        .map(|list| list.into_iter().map(|e| (e.id, e.title)).collect())
}

fn pair(id: &str, title: &str) -> (String, String) {
    (id.to_string(), name(title))
}

#[test]
fn the_unread_list_is_read() {
    let many: Vec<(String, String)> = (0..45)
        .map(|i| (format!("{}", 100 + i), format!("Article {i}")))
        .collect();
    let many: Vec<(&str, &str)> = many.iter().map(|(a, b)| (&a[..], &b[..])).collect();
    let texts: [(String, Option<Names>); 13] = [
        (entries(&[]), Some(vec![])),
        (
            entries(&[("12", "One"), ("13", "Zwei \\u00e4 \\\"q\\\"")]),
            Some(vec![pair("12", "One"), pair("13", "Zwei \u{e4} \"q\"")]),
        ),
        // An id that is not a number is left out.
        (
            entries(&[("12", "One"), ("\"x\"", "Not a number"), ("14", "Three")]),
            Some(vec![pair("12", "One"), pair("14", "Three")]),
        ),
        (
            entries(&[("\"-1\"", "a"), ("\"1.5\"", "b"), ("\"\"", "c"), ("7", "d")]),
            Some(vec![pair("7", "d")]),
        ),
        // The newest 30 only.
        (
            entries(&many),
            Some(many.iter().take(30).map(|(i, t)| pair(i, t)).collect()),
        ),
        // Not whole: the items never end.
        (
            String::from(r#"{"_embedded":{"items":[{"id":1,"title":"a"},{"id":2"#),
            None,
        ),
        (
            String::from(r#"{"_embedded":{"items":[{"id":1,"title":"a"}"#),
            None,
        ),
        // Another shape; a title before the id and an id twice (the last
        // counts); an id inside a tag does not count.
        (String::from(r#"{"items":[{"id":1}]}"#), None),
        (
            String::from(r#"{"_embedded":{"items":[{"title":"t","id":5,"id":6}]}}"#),
            Some(vec![pair("6", "t")]),
        ),
        (
            String::from(r#"{"_embedded":{"items":[{"id":8,"tags":[{"id":9,"title":"deep"}]}]}}"#),
            Some(vec![pair("8", "")]),
        ),
        (String::new(), None),
        (String::from("not json"), None),
        (String::from(r#"{"_embedded":{"items":[]}}"#), Some(vec![])),
    ];
    for (text, expected) in &texts {
        assert_eq!(&new_unread(text), expected, "{text:?}");
        // Whatever the size of the pieces the file is read in.
        for size in [1, 2, 7, 64, 2048] {
            let mut reading = Unread::new();
            for piece in text.as_bytes().chunks(size) {
                reading.feed(piece).unwrap();
            }
            let new = reading.finish().unwrap().map(|list| {
                list.into_iter()
                    .map(|e| (e.id, e.title))
                    .collect::<Vec<_>>()
            });
            assert_eq!(&new, expected, "{size} {text:?}");
        }
    }
    // Items inside items: whatever the reading makes of them, in any pieces
    // it is the same.
    let odd = r#"{"_embedded":{"items":[[{"id":8}],{"id":10}]}}"#;
    let whole = new_unread(odd);
    for size in [1, 3, 64] {
        let mut reading = Unread::new();
        for piece in odd.as_bytes().chunks(size) {
            reading.feed(piece).unwrap();
        }
        let split = reading.finish().unwrap().map(|list| {
            list.into_iter()
                .map(|e| (e.id, e.title))
                .collect::<Vec<_>>()
        });
        assert_eq!(split, whole, "{size}");
    }
}

#[test]
fn the_archived_flag_is_read() {
    for (text, id, expected) in [
        (r#"{"id":7,"is_archived":1,"title":"t"}"#, "7", Some(true)),
        (r#"{"id":7,"is_archived":0}"#, "7", Some(false)),
        (r#"{"is_archived":true,"id":7}"#, "7", Some(true)),
        (r#"{"is_archived":false,"id":7}"#, "7", Some(false)),
        // Another article's answer says nothing of this one.
        (r#"{"id":8,"is_archived":1}"#, "7", None),
        (r#"{"id":7,"is_archived":1}"#, "8", None),
        (r#"{"id":7,"is_archived":1}"#, "", None),
        (r#"{"id":7}"#, "7", None),
        (r#"{"is_archived":1}"#, "7", None),
        (r#"{"id":7,"is_archived":"yes"}"#, "7", None),
        (r#"{"id":7,"is_archived":null}"#, "7", None),
        // Only the top of the answer counts.
        (
            r#"{"id":7,"tags":[{"id":8,"is_archived":1}],"is_archived":0}"#,
            "7",
            Some(false),
        ),
        (r#"[{"id":7,"is_archived":1}]"#, "7", None),
        (r#"{"id":"7","is_archived":"1"}"#, "7", Some(true)),
        ("", "7", None),
        ("{", "7", None),
    ] {
        assert_eq!(
            Archived::read(id, text.as_bytes()).unwrap(),
            expected,
            "{id} {text}"
        );
        let mut reading = Archived::new(id);
        for piece in text.as_bytes().chunks(3) {
            reading.feed(piece).unwrap();
        }
        assert_eq!(reading.finish().unwrap(), expected, "{id} {text}");
    }
}

#[test]
fn the_lists_have_the_read_caps_of_interface_6() {
    assert_eq!((ARTICLES_CAP, FINISHED_CAP), (64 * 1024, 16 * 1024));
    // Over the cap is an error, not an empty list, so nothing is written
    // over what the file holds; at the cap it is read.
    let line = "7\t/books/Wallabag/a.epub\n";
    let at_cap = line.repeat(ARTICLES_CAP / line.len()) + &"\n".repeat(ARTICLES_CAP % line.len());
    assert_eq!(at_cap.len(), ARTICLES_CAP);
    assert_eq!(
        native::articles(&at_cap).unwrap().len(),
        ARTICLES_CAP / line.len()
    );
    let over = at_cap + "\n";
    assert_eq!(native::articles(&over).err(), Some(Error::NoSpace));

    let ids = "12345\n".repeat(FINISHED_CAP / 6);
    let at_cap = ids.clone() + &"\n".repeat(FINISHED_CAP - ids.len());
    assert_eq!(native::finished(&at_cap).unwrap().len(), FINISHED_CAP / 6);
    assert_eq!(
        native::finished(&(at_cap + "\n")).err(),
        Some(Error::NoSpace)
    );
}

#[test]
fn a_byte_order_mark_is_left_out_of_the_lists() {
    let list = native::articles("\u{feff}7\tx\n8\ty\n").unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!((list[0].id.as_str(), list[0].path.as_str()), ("7", "x"));
    assert_eq!(native::finished("\u{feff}7\n8\n").unwrap(), ["7", "8"]);
    // Only one is left out.
    let list = native::articles("\u{feff}\u{feff}7\tx\n").unwrap();
    assert_eq!(list[0].id, "\u{feff}7");
}

#[test]
fn refused_allocations_end_in_no_memory() {
    let text = entries(&[
        ("12", "One"),
        ("13", "Two with a longer title"),
        ("14", "Three"),
    ]);
    sweep(|| Unread::read(text.as_bytes()).map(|list| list.map(|l| l.len())));
    // Pieces that split a string: the parser's text is kept between them.
    sweep(|| {
        let mut reading = Unread::new();
        for piece in text.as_bytes().chunks(5) {
            reading.feed(piece)?;
        }
        reading.finish().map(|list| list.map(|l| l.len()))
    });
    let entry = r#"{"id":7,"is_archived":1,"title":"A title","tags":[{"id":1}]}"#;
    sweep(|| Archived::read("7", entry.as_bytes()));
}

#[test]
fn a_title_is_kept_as_the_name_of_its_book_and_makes_the_same_book() {
    // Long, with characters the cleaning drops, a name Windows keeps, wide
    // characters: the book's name is the same made of the kept title as of
    // the whole one, and 30 long titles stay small.
    let long = "x".repeat(4000);
    let blanks = format!("a{}b{}", "/".repeat(90), "c".repeat(200));
    let wide = "\u{1F600}".repeat(300);
    let titles = [
        long.as_str(),
        blanks.as_str(),
        wide.as_str(),
        "CON.txt",
        "  ..hidden. ",
        "?*",
        "Ordinary title",
    ];
    for title in titles {
        let text = entries(&[("5", title)]);
        let list = Unread::read(text.as_bytes()).unwrap().unwrap();
        let kept = &list[0].title;
        assert!(kept.len() <= 4 * native::NAME_UNITS, "{} bytes", kept.len());
        let name = |title: &str| paperpaw_sdk::web::clean_name(title, native::NAME_UNITS);
        assert_eq!(name(kept), name(title), "{title:.30}");
    }
    let many: Vec<(String, String)> = (1..=40)
        .map(|n| (format!("{n}"), "y".repeat(4000)))
        .collect();
    let pairs: Vec<(&str, &str)> = many.iter().map(|(i, t)| (i.as_str(), t.as_str())).collect();
    let list = Unread::read(entries(&pairs).as_bytes()).unwrap().unwrap();
    assert_eq!(list.len(), native::MAX_ARTICLES);
    assert!(list.iter().map(|e| e.title.len()).sum::<usize>() <= 30 * native::NAME_UNITS);
}
