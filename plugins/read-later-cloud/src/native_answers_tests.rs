//! The native readers of Reader's answers (`native_answers`): each text read
//! as interface 6 read it (the results are pinned, taken from the interface-6
//! reader when it was still here), at any piece size, refusing instead of
//! aborting when memory runs short; and the caps and the spent copier of
//! `native_read_later`.
use crate::fail_alloc::sweep;
use crate::native_answers::{Answer, Asked, FIELD_CAP, Listed};
use crate::native_read_later::{self as native, ARTICLES_CAP, FINISHED_CAP, MAX_ANSWER, MAX_LIST};
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
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

/// The texts of the list's tests.
pub(crate) fn list_texts() -> Vec<String> {
    let many: Vec<(String, String)> = (0..30)
        .map(|i| (format!("a{i}"), format!("Article {i}")))
        .collect();
    let many: Vec<(&str, &str)> = many.iter().map(|(a, b)| (&a[..], &b[..])).collect();
    vec![
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
    ]
}

/// What each text of [`list_texts`] gives for the most 0, 1 and 20 (the
/// `Debug` of the articles, `None` for an answer that is not a whole list).
const LISTED: [&str; 27] = [
    "Some([])",
    "Some([])",
    "Some([])",
    "Some([])",
    "Some([(\"01gm6ceb0\", \"Slow \\\"news\\\"\")])",
    "Some([(\"01gm6ceb0\", \"Slow \\\"news\\\"\"), (\"01gm6cec1\", \"Zwei ä\")])",
    "Some([])",
    "Some([(\"ok1\", \"Fine\")])",
    "Some([(\"ok1\", \"Fine\")])",
    "Some([])",
    "Some([(\"a0\", \"Article 0\")])",
    "Some([(\"a0\", \"Article 0\"), (\"a1\", \"Article 1\"), (\"a2\", \"Article 2\"), (\"a3\", \"Article 3\"), (\"a4\", \"Article 4\"), (\"a5\", \"Article 5\"), (\"a6\", \"Article 6\"), (\"a7\", \"Article 7\"), (\"a8\", \"Article 8\"), (\"a9\", \"Article 9\"), (\"a10\", \"Article 10\"), (\"a11\", \"Article 11\"), (\"a12\", \"Article 12\"), (\"a13\", \"Article 13\"), (\"a14\", \"Article 14\"), (\"a15\", \"Article 15\"), (\"a16\", \"Article 16\"), (\"a17\", \"Article 17\"), (\"a18\", \"Article 18\"), (\"a19\", \"Article 19\")])",
    "None",
    "None",
    "None",
    "None",
    "None",
    "None",
    "None",
    "None",
    "None",
    "Some([])",
    "Some([(\"c2\", \"t\")])",
    "Some([(\"c2\", \"t\")])",
    "None",
    "None",
    "None",
];

#[test]
fn the_list_is_read_as_before() {
    let texts = list_texts();
    for (t, text) in texts.iter().enumerate() {
        for (m, max) in [0, 1, 20].into_iter().enumerate() {
            for size in [2048, 1, 3, 64] {
                assert_eq!(
                    format!("{:?}", pieces(text, size, max)),
                    LISTED[t * 3 + m],
                    "{max} {size} {text:?}"
                );
            }
        }
    }
    assert_eq!(pieces(&texts[3], 64, 20).unwrap().len(), 20);
}

/// The texts of the asked article's tests.
pub(crate) fn asked_texts() -> Vec<String> {
    let body = |location: &str| {
        format!(
            r#"{{"count":1,"results":[{{"id":"a1","location":"{location}","author":"Jo","html_content":"{}"}}]}}"#,
            "x".repeat(20_000)
        )
    };
    vec![
        body("archive"),
        String::from(r#"{"count":0,"results":[]}"#),
        String::from("<html>Login</html>"),
        String::from(r#"{"results":[{"id":"a1""#),
        String::from(
            r#"{"results":[{"id":"a1","location":"later"},{"id":"a2","location":"new"}]}"#,
        ),
        String::from(r#"{"results":[{"location":"new","author":"Ä","id":"a1"}]}"#),
        String::new(),
    ]
}

/// What each text of [`asked_texts`] gives for the ids `a1`, `a2` and none.
const ASKED: [&str; 21] = [
    "Some(Known { location: \"archive\", author: \"Jo\" })",
    "None",
    "None",
    "Some(Gone)",
    "Some(Gone)",
    "Some(Gone)",
    "None",
    "None",
    "None",
    "None",
    "None",
    "None",
    "Some(Known { location: \"later\", author: \"\" })",
    "None",
    "None",
    "Some(Known { location: \"new\", author: \"Ä\" })",
    "None",
    "None",
    "None",
    "None",
    "None",
];

#[test]
fn the_asked_article_is_read_as_before() {
    for (t, text) in asked_texts().iter().enumerate() {
        for (i, id) in ["a1", "a2", ""].into_iter().enumerate() {
            let expected = ASKED[t * 3 + i];
            assert_eq!(
                format!("{:?}", Asked::read(id, text.as_bytes()).unwrap()),
                expected,
                "{id} {text:.60}"
            );
            let mut reading = Asked::new(id);
            for piece in text.as_bytes().chunks(5) {
                reading.feed(piece).unwrap();
            }
            assert_eq!(
                format!("{:?}", reading.finish().unwrap()),
                expected,
                "{id} {text:.60}"
            );
        }
    }
}

#[test]
fn the_read_caps_are_those_of_interface_6() {
    assert_eq!((MAX_LIST, MAX_ANSWER), (512 * 1024, 4 * 1024 * 1024));
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
    let ids = "abc\n".repeat(FINISHED_CAP / 4);
    assert_eq!(native::finished(&ids).unwrap().len(), FINISHED_CAP / 4);
    assert_eq!(native::finished(&(ids + "\n")).err(), Some(Error::NoSpace));
    let marked = "\u{feff}a\tx\n";
    assert_eq!(native::articles(marked).unwrap().len(), 1);
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
