//! The native readers of Reader's answers (`native_answers`) against the
//! interface-6 ones (`answers.rs`): equal on the same text at any piece size,
//! refusing instead of aborting when memory runs short; and the caps and the
//! spent copier of `native_read_later`.
use crate::fail_alloc::sweep;
use crate::native_answers::{Answer, Asked, FIELD_CAP, Listed};
use crate::native_read_later::{self as native, ARTICLES_CAP, FINISHED_CAP, MAX_ANSWER, MAX_LIST};
use crate::{MAX_ARTICLE_ANSWER, MAX_LIST as OLD_LIST, answers, data};
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use paperpaw_sdk::MemoryStorage;
use paperpaw_sdk::native::Error;

fn list(items: &[(&str, &str)]) -> String {
    let mut out = String::from(r#"{"count":9,"nextPageCursor":null,"results":["#);
    for (i, (id, title)) in items.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&format!(
            r#"{{"id":"{id}","url":"https://r.example/{id}","title":"{title}","tags":{{"x":{{"id":"no"}}}},"location":"later"}}"#
        ));
    }
    out.push_str("]}");
    out
}

fn old_listed(text: &str, max: usize) -> Option<Vec<(String, String)>> {
    answers::listed(&MemoryStorage::new().with("a.json", text), "a.json", max)
}

fn pieces(text: &str, size: usize, max: usize) -> Option<Vec<(String, String)>> {
    let mut reading = Listed::new(max);
    for piece in text.as_bytes().chunks(size) {
        reading.feed(piece).unwrap();
    }
    reading
        .finish()
        .unwrap()
        .map(|l| l.into_iter().map(|e| (e.id, e.title)).collect())
}

#[test]
fn the_list_is_read_as_before() {
    let many: Vec<(String, String)> = (0..30)
        .map(|i| (format!("a{i}"), format!("Article {i}")))
        .collect();
    let many: Vec<(&str, &str)> = many.iter().map(|(a, b)| (&a[..], &b[..])).collect();
    let texts = [
        list(&[]),
        list(&[
            ("01gm6ceb0", "Slow \\\"news\\\""),
            ("01gm6cec1", "Zwei \\u00e4"),
        ]),
        list(&[
            ("x\\t.paperpaw/secrets.txt\\ny", "Planted"),
            ("../../x", "Out"),
            ("", "N"),
            ("ok1", "Fine"),
        ]),
        list(&many),
        String::from(r#"{"count": 2, "results": [{"id": "a1", "title": "Newer"}, {"id": "b"#),
        String::from(r#"{"detail": "Invalid token."}"#),
        String::from("<!DOCTYPE html><html><body>Down</body></html>"),
        String::from(r#"{"results":[{"title":"t","id":"c1","id":"c2"}]}"#),
        String::new(),
    ];
    for text in &texts {
        for max in [0, 1, 20] {
            assert_eq!(
                old_listed(text, max),
                pieces(text, 2048, max),
                "{max} {text:?}"
            );
            for size in [1, 3, 64] {
                assert_eq!(
                    old_listed(text, max),
                    pieces(text, size, max),
                    "{size} {text:?}"
                );
            }
        }
    }
    assert_eq!(pieces(&texts[3], 64, 20).unwrap().len(), 20);
}

#[test]
fn the_asked_article_is_read_as_before() {
    let body = |location: &str| {
        format!(
            r#"{{"count":1,"results":[{{"id":"a1","location":"{location}","author":"Jo","html_content":"{}"}}]}}"#,
            "x".repeat(20_000)
        )
    };
    let archive = body("archive");
    for text in [
        archive.as_str(),
        r#"{"count":0,"results":[]}"#,
        "<html>Login</html>",
        r#"{"results":[{"id":"a1""#,
        r#"{"results":[{"id":"a1","location":"later"},{"id":"a2","location":"new"}]}"#,
        r#"{"results":[{"location":"new","author":"Ä","id":"a1"}]}"#,
        "",
    ] {
        for id in ["a1", "a2", ""] {
            let storage = MemoryStorage::new().with("a.json", text);
            let old = answers::asked(&storage, "a.json", id);
            let same = |new: Option<Answer>| {
                new.map(|a| match a {
                    Answer::Gone => answers::Answer::Gone,
                    Answer::Known { location, author } => {
                        answers::Answer::Known { location, author }
                    }
                })
            };
            assert_eq!(
                old,
                same(Asked::read(id, text.as_bytes()).unwrap()),
                "{id} {text:.60}"
            );
            let mut reading = Asked::new(id);
            for piece in text.as_bytes().chunks(5) {
                reading.feed(piece).unwrap();
            }
            assert_eq!(old, same(reading.finish().unwrap()), "{id} {text:.60}");
        }
    }
}

#[test]
fn the_read_caps_are_those_of_interface_6() {
    assert_eq!(
        (MAX_LIST as u64, MAX_ANSWER as u64),
        (OLD_LIST, MAX_ARTICLE_ANSWER)
    );
    assert_eq!((ARTICLES_CAP, FINISHED_CAP), (64 * 1024, 16 * 1024));
    // The lists: over the cap is an error, not an empty list, so the file
    // is not written over what it holds; at the cap it is read.
    let line = "a\t/books/Reader/a.html\n";
    let at_cap = line.repeat(ARTICLES_CAP / line.len()) + &"\n".repeat(ARTICLES_CAP % line.len());
    assert_eq!(at_cap.len(), ARTICLES_CAP);
    assert_eq!(
        native::articles(&at_cap).unwrap().len(),
        ARTICLES_CAP / line.len()
    );
    let over = at_cap + "\n";
    assert_eq!(native::articles(&over).err(), Some(Error::NoSpace));
    assert!(
        crate::articles(&MemoryStorage::new().with(&data("articles.txt"), over.as_str())).is_err()
    );
    let ids = "abc\n".repeat(FINISHED_CAP / 4);
    assert_eq!(native::finished(&ids).unwrap().len(), FINISHED_CAP / 4);
    assert_eq!(native::finished(&(ids + "\n")).err(), Some(Error::NoSpace));
    let marked = "\u{feff}a\tx\n";
    let old = crate::articles(&MemoryStorage::new().with(&data("articles.txt"), marked)).unwrap();
    assert_eq!(old.len(), native::articles(marked).unwrap().len());
    // The answers: by the file's size before, and by the bytes fed.
    assert!(native::fits(MAX_ANSWER as u64, MAX_ANSWER).is_ok());
    assert_eq!(
        native::fits(MAX_ANSWER as u64 + 1, MAX_ANSWER),
        Err(Error::NoSpace)
    );
    assert_eq!(
        Listed::read(20, &vec![b' '; MAX_LIST + 1]).err(),
        Some(Error::NoSpace)
    );
    assert_eq!(
        Asked::read("a", &vec![b' '; MAX_LIST + 1]).err(),
        Some(Error::NoSpace)
    );
    let mut reading = Listed::new(20);
    let block = [b' '; 4096];
    let mut fed = 0;
    while fed < MAX_LIST {
        reading.feed(&block).unwrap();
        fed += block.len();
    }
    assert_eq!(reading.feed(&block), Err(Error::NoSpace));
    assert_eq!(reading.feed(b""), Err(Error::NoSpace));
    assert_eq!(reading.finish().err(), Some(Error::NoSpace));
    // The copier: an article is read to the cap, and no more.
    let mut copier = native::Copier::default();
    let mut fed = 0;
    let mut result = Ok(());
    while result.is_ok() && fed <= MAX_ANSWER {
        result = copier.feed(&block);
        fed += block.len();
    }
    assert_eq!(result, Err(Error::NoSpace));
    assert_eq!(copier.feed(b"x"), Err(Error::NoSpace));
}

#[test]
fn a_spent_copier_stays_spent() {
    // Memory refused in the middle of a piece leaves the state half-way:
    // every later piece says so too, and the fetch must end.
    let json = br#"{"html_content":"a text <p>of some length</p> and more"}"#;
    let mut refused = None;
    sweep(|| {
        let mut copier = native::Copier::default();
        let first = copier.feed(&json[..30]);
        if let Err(error) = first {
            refused = Some(error);
            assert_eq!(copier.feed(&json[30..]), Err(error));
            assert_eq!(copier.feed(b""), Err(error));
        }
        Ok(())
    });
    assert_eq!(refused, Some(Error::NoMemory));
}

#[test]
fn refused_allocations_end_in_no_memory() {
    let text = list(&[
        ("01gm6ceb0", "Slow news"),
        ("01gm6cec1", "Second, a longer title"),
    ]);
    sweep(|| Listed::read(20, text.as_bytes()).map(|l| l.map(|l| l.len())));
    sweep(|| {
        let mut reading = Listed::new(20);
        for piece in text.as_bytes().chunks(5) {
            reading.feed(piece)?;
        }
        reading.finish().map(|l| l.map(|l| l.len()))
    });
    let one =
        r#"{"results":[{"id":"a1","location":"archive","author":"Jo Ä","html_content":"x"}]}"#;
    sweep(|| Asked::read("a1", one.as_bytes()).map(|a| a.is_some()));
    sweep(|| native::articles("a\tx\n").map(|l| l.len()));
}

#[test]
fn titles_locations_and_authors_are_cut_when_kept() {
    // Multi-byte letters: a careless cut falls inside one.
    let long = "\u{e4}".repeat(2000);
    let titles: Vec<(String, String)> = (0..20).map(|i| (format!("a{i}"), long.clone())).collect();
    let items: Vec<(&str, &str)> = titles.iter().map(|(a, b)| (&a[..], &b[..])).collect();
    let listed = Listed::read(30, list(&items).as_bytes()).unwrap().unwrap();
    assert_eq!(listed.len(), 20);
    for entry in &listed {
        assert!(!entry.title.is_empty());
        assert!(entry.title.len() <= FIELD_CAP);
        assert!(entry.title.chars().all(|c| c == '\u{e4}'));
    }
    assert!(listed.iter().map(|e| e.title.len()).sum::<usize>() <= 20 * FIELD_CAP);
    let body = format!(r#"{{"results":[{{"id":"a1","location":"{long}","author":"{long}"}}]}}"#);
    let Some(Answer::Known { location, author }) = Asked::read("a1", body.as_bytes()).unwrap()
    else {
        panic!("a known article");
    };
    assert!(location.len() <= FIELD_CAP && !location.is_empty());
    assert!(author.len() <= FIELD_CAP && !author.is_empty());
    // Short values are untouched, and a refusal ends the reading.
    let short = Listed::read(30, list(&[("a1", "Short title")]).as_bytes())
        .unwrap()
        .unwrap();
    assert_eq!(short[0].title, "Short title");
    let body = list(&items);
    sweep(|| Listed::read(30, body.as_bytes()).map(|l| l.map(|l| l.len())));
}
