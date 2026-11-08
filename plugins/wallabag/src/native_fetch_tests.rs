//! The native readers of Wallabag's answers (`native_fetch`) against the
//! interface-6 ones (`unread` and `is_archived` in `lib.rs`): equal on the
//! same text, at any piece size, and refusing instead of aborting when
//! memory runs short. The read caps of the lists are those of interface 6.
use crate::fail_alloc::sweep;
use crate::native_fetch::{Archived, Unread};
use crate::native_wallabag::{self as native, ARTICLES_CAP, FINISHED_CAP};
use crate::{data, is_archived, unread};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::MemoryStorage;
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

fn old_unread(text: &str) -> Option<Vec<(String, String)>> {
    let storage = MemoryStorage::new().with(&data("entries.json"), text);
    unread(&storage)
}

fn new_unread(text: &str) -> Option<Vec<(String, String)>> {
    Unread::read(text.as_bytes())
        .unwrap()
        .map(|list| list.into_iter().map(|e| (e.id, e.title)).collect())
}

#[test]
fn the_unread_list_is_read_as_before() {
    let many: Vec<(String, String)> = (0..45)
        .map(|i| (format!("{}", 100 + i), format!("Article {i}")))
        .collect();
    let many: Vec<(&str, &str)> = many.iter().map(|(a, b)| (&a[..], &b[..])).collect();
    let texts = [
        entries(&[]),
        entries(&[("12", "One"), ("13", "Zwei \\u00e4 \\\"q\\\"")]),
        entries(&[("12", "One"), ("\"x\"", "Not a number"), ("14", "Three")]),
        entries(&[("\"-1\"", "a"), ("\"1.5\"", "b"), ("\"\"", "c"), ("7", "d")]),
        entries(&many),
        // Not whole: the items never end.
        String::from(r#"{"_embedded":{"items":[{"id":1,"title":"a"},{"id":2"#),
        String::from(r#"{"_embedded":{"items":[{"id":1,"title":"a"}"#),
        // Another shape, other members, an id twice, a title before the id.
        String::from(r#"{"items":[{"id":1}]}"#),
        String::from(r#"{"_embedded":{"items":[{"title":"t","id":5,"id":6}]}}"#),
        String::from(r#"{"_embedded":{"items":[{"id":8,"tags":[{"id":9,"title":"deep"}]}]}}"#),
        String::from(r#"{"_embedded":{"items":[[{"id":8}],{"id":10}]}}"#),
        String::new(),
        String::from("not json"),
    ];
    for text in &texts {
        assert_eq!(old_unread(text), new_unread(text), "{text:?}");
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
            assert_eq!(old_unread(text), new, "{size} {text:?}");
        }
    }
    let list = new_unread(&texts[4]).unwrap();
    assert_eq!(list.len(), native::MAX_ARTICLES);
    assert_eq!(list[0], ("100".into(), "Article 0".into()));
}

#[test]
fn the_archived_flag_is_read_as_before() {
    for text in [
        r#"{"id":7,"is_archived":1,"title":"t"}"#,
        r#"{"id":7,"is_archived":0}"#,
        r#"{"is_archived":true,"id":7}"#,
        r#"{"is_archived":false,"id":7}"#,
        r#"{"id":8,"is_archived":1}"#,
        r#"{"id":7}"#,
        r#"{"is_archived":1}"#,
        r#"{"id":7,"is_archived":"yes"}"#,
        r#"{"id":7,"is_archived":null}"#,
        r#"{"id":7,"tags":[{"id":8,"is_archived":1}],"is_archived":0}"#,
        r#"[{"id":7,"is_archived":1}]"#,
        r#"{"id":"7","is_archived":"1"}"#,
        "",
        "{",
    ] {
        let storage = MemoryStorage::new().with(&data("entry.json"), text);
        for id in ["7", "8", ""] {
            let old = is_archived(&storage, id);
            assert_eq!(
                old,
                Archived::read(id, text.as_bytes()).unwrap(),
                "{id} {text}"
            );
            let mut reading = Archived::new(id);
            for piece in text.as_bytes().chunks(3) {
                reading.feed(piece).unwrap();
            }
            assert_eq!(old, reading.finish().unwrap(), "{id} {text}");
        }
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
    let storage = MemoryStorage::new().with(&data("articles.txt"), over.as_str());
    assert!(crate::articles(&storage).is_err());

    let ids = "12345\n".repeat(FINISHED_CAP / 6);
    let at_cap = ids.clone() + &"\n".repeat(FINISHED_CAP - ids.len());
    assert_eq!(native::finished(&at_cap).unwrap().len(), FINISHED_CAP / 6);
    assert_eq!(
        native::finished(&(at_cap + "\n")).err(),
        Some(Error::NoSpace)
    );
    let storage = MemoryStorage::new().with(&data("finished.txt"), "\n".repeat(FINISHED_CAP + 1));
    assert!(crate::finished(&storage).is_err());
}

#[test]
fn a_byte_order_mark_is_left_out_of_the_lists() {
    for text in ["\u{feff}7\tx\n8\ty\n", "\u{feff}\u{feff}7\tx\n"] {
        let storage = MemoryStorage::new().with(&data("articles.txt"), text);
        let old = crate::articles(&storage).unwrap();
        let new = native::articles(text).unwrap();
        assert_eq!(old.len(), new.len(), "{text:?}");
        for (o, n) in old.iter().zip(&new) {
            assert_eq!((&o.id, &o.path), (&n.id, &n.path));
        }
        let storage = MemoryStorage::new().with(&data("finished.txt"), text);
        assert_eq!(
            crate::finished(&storage).unwrap(),
            native::finished(text).unwrap()
        );
    }
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
