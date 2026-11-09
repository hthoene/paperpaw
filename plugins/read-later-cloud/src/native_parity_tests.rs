//! The native decisions (`native_read_later`) against the interface-6 ones
//! (`lib.rs`, `html.rs`, `answers.rs`): equal on the same input, and refusing
//! instead of aborting when memory runs short.
use crate::fail_alloc::sweep;
use crate::native_read_later as native;
use crate::{account, answers, articles, data, escaped, finished, head, html};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::MemoryStorage;

#[test]
fn ids_are_those_of_interface_6() {
    for id in [
        "",
        "a",
        "01h8x7y9z0abcdefghij123456",
        &"a".repeat(40),
        &"a".repeat(41),
        "a-b",
        "a b",
        "ä",
        "9\n",
        "A1",
    ] {
        assert_eq!(native::is_id(id), answers::is_id(id), "{id:?}");
    }
}

#[test]
fn accounts_heads_and_lists_are_those_of_interface_6() {
    for token in ["", "token", "a longer token with ünï", &"t".repeat(300)] {
        assert_eq!(native::account(token).unwrap(), account(token), "{token:?}");
    }
    for (title, author) in [
        ("Title", "Author"),
        ("A & B <c> \"d\"", ""),
        ("", "Ä <b>"),
        ("ünï 😀", "&&&"),
    ] {
        assert_eq!(
            native::head(title, author).unwrap(),
            head(title, author),
            "{title:?}"
        );
    }
    assert_eq!(native::TAIL, crate::TAIL);
    assert_eq!(escaped("a&b"), "a&amp;b");
    for text in [
        "",
        "abc\t/books/Reader/a.html\nxyz\t/books/Reader/b (xyz).html\n",
        "a\tx\nno tab\n\n\t\nb\ty\ttab\n",
        "a\tx\r\nb\ty\r\n",
    ] {
        let storage = MemoryStorage::new().with(&data("articles.txt"), text);
        let old = articles(&storage).unwrap();
        let new = native::articles(text).unwrap();
        assert_eq!(old.len(), new.len(), "{text:?}");
        for (o, n) in old.iter().zip(&new) {
            assert_eq!((&o.id, &o.path), (&n.id, &n.path));
        }
        let storage = MemoryStorage::new().with(&data("finished.txt"), text);
        assert_eq!(finished(&storage).unwrap(), native::finished(text).unwrap());
    }
    let new = [native::Article {
        id: "a".into(),
        path: "/b/a.html".into(),
    }];
    assert_eq!(native::articles_text(&new).unwrap(), "a\t/b/a.html\n");
    assert_eq!(native::location("read-later-cloud.from.inbox"), "new");
    assert_eq!(native::location("read-later-cloud.from.later"), "later");
    assert_eq!(native::location("anything else"), "later");
}

#[test]
fn an_article_counts_as_read_as_before() {
    assert_eq!(native::READ_PPM, (crate::READ * 10_000.0) as u32);
    for (page, pages) in [
        (Some(5), Some(5)),
        (Some(4), Some(5)),
        (None, Some(5)),
        (Some(5), None),
        (None, None),
    ] {
        for percent in [0.0f32, 50.0, 94.9, 95.0, 95.1, 100.0] {
            let old = match (page, pages) {
                (Some(page), Some(pages)) => page >= pages,
                _ => f64::from(percent) >= crate::READ,
            };
            let ppm = (percent * 10_000.0).round() as u32;
            assert_eq!(
                native::is_read(page, pages, ppm),
                old,
                "{page:?} {pages:?} {percent}"
            );
        }
    }
}

/// What the interface-6 copier makes of `json` and how it ends.
fn old_copy(json: &[u8]) -> (html::Copied, Vec<u8>) {
    let storage = MemoryStorage::new().with("a.json", json);
    let copied = html::copy_article(&storage, "a.json", "book.html").unwrap();
    (copied, storage.get("book.html").unwrap_or_default())
}

/// The same with the native copier, fed in pieces of `piece` bytes.
fn new_copy(json: &[u8], piece: usize) -> (native::Copied, Vec<u8>) {
    let mut copier = native::Copier::default();
    let mut book = Vec::new();
    for chunk in json.chunks(piece) {
        copier.feed(chunk).unwrap();
        book.extend_from_slice(copier.taken());
        copier.clear();
        if copier.is_done() {
            break;
        }
    }
    (copier.outcome(), book)
}

fn same_end(old: html::Copied, new: native::Copied) -> bool {
    matches!(
        (old, new),
        (html::Copied::Whole, native::Copied::Whole)
            | (html::Copied::Missing, native::Copied::Missing)
            | (html::Copied::Cut, native::Copied::Cut)
    )
}

#[test]
fn an_article_is_copied_as_the_interface_6_copier_copies_it() {
    let long = "ä<p>x</p>\\n".repeat(2000);
    let bodies: Vec<String> = [
        r#"{"id":"1","html_content":"<p>Hi</p>","x":1}"#,
        r#"{"html_content": null}"#,
        r#"{"title":"no key"}"#,
        r#"{"html_content":"cut off"#,
        r#"{"html_content":"a\u00e4\u20ac\ud83d\ude00b\ud83dX\ude00 \n\t\r\b\f\"\\\/\q"}"#,
        r#"{"html_content":"\ud83d"}"#,
        r#"{"html_content":"\u12"}"#,
        r#"{"html_content":"\uZZZZ rest"}"#,
        r#"{"html_content":"\u0000x"}"#,
        r#"{"a":"\"html_content\"","html_content":"found"}"#,
        r#"{"x":"html_content","html_content" : "spaced"}"#,
        r#"{"html_content"x:"no","html_content":"second"}"#,
        r#"{"html_content":5,"html_content":"later"}"#,
        r#""html_content""#,
        "",
    ]
    .iter()
    .map(|s| String::from(*s))
    .chain([format!(r#"{{"html_content":"{long}"}}"#)])
    .collect();
    for body in &bodies {
        let (old_end, old_text) = old_copy(body.as_bytes());
        for piece in [1, 2, 3, 5, 7, 64, 4096, 100_000] {
            let (new_end, new_text) = new_copy(body.as_bytes(), piece);
            assert!(
                same_end(old_end, new_end),
                "{body:.60?} {piece}: {old_end:?} {new_end:?}"
            );
            assert_eq!(old_text, new_text, "{body:.60?} {piece}");
        }
    }
}

#[test]
fn a_very_long_article_is_cut_at_the_most() {
    let body = format!(
        r#"{{"html_content":"{}"}}"#,
        "x".repeat(native::MAX_TEXT + 100)
    );
    let (old_end, old_text) = old_copy(body.as_bytes());
    let (new_end, new_text) = new_copy(body.as_bytes(), 4096);
    assert!(same_end(old_end, new_end));
    assert_eq!(old_text.len(), native::MAX_TEXT);
    assert_eq!(old_text, new_text);
}

#[test]
fn refused_allocations_end_in_no_memory() {
    sweep(|| native::account("token").map(|a| a.len()));
    sweep(|| native::head("A & B", "Au <thor>").map(|h| h.len()));
    sweep(|| native::articles("a\tx\nb\ty\n").map(|l| l.len()));
    sweep(|| native::finished("a\nb\n").map(|l| l.len()));
    let list = [native::Article {
        id: "a".into(),
        path: "/b/a.html".into(),
    }];
    sweep(|| native::articles_text(&list).map(|t| t.len()));
    let json = br#"{"html_content":"a\u00e4\ud83d\ude00 text <p>x</p>"}"#;
    sweep(|| {
        let mut copier = native::Copier::default();
        copier.feed(json)?;
        Ok(copier.taken().len())
    });
}
