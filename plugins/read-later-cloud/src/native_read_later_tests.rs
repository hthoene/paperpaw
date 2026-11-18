//! The native decisions (`native_read_later`) as interface 6 made them (the
//! values are pinned, taken from the interface-6 code when it was still here),
//! and refusing instead of aborting when memory runs short.
use crate::fail_alloc::sweep;
use crate::native_read_later as native;
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

#[test]
fn ids_are_letters_and_digits_of_a_length() {
    for (id, is) in [
        ("", false),
        ("a", true),
        ("01h8x7y9z0abcdefghij123456", true),
        (&"a".repeat(40), true),
        (&"a".repeat(41), false),
        ("a-b", false),
        ("a b", false),
        ("ä", false),
        ("9\n", false),
        ("A1", true),
    ] {
        assert_eq!(native::is_id(id), is, "{id:?}");
    }
}

#[test]
fn accounts_heads_and_lists_are_those_of_interface_6() {
    // The FNV-1a 64 of the token, sixteen hex digits.
    for (token, account) in [
        ("", "cbf29ce484222325"),
        ("token", "26c4b17d50b3c152"),
        ("a longer token with ünï", "bce97a3623ad64a9"),
    ] {
        assert_eq!(native::account(token).unwrap(), account, "{token:?}");
    }
    assert_eq!(native::account(&"t".repeat(300)).unwrap().len(), 16);
    let doc = |title: &str| {
        format!(
            "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>{title}</title></head><body><h1>{title}</h1>"
        )
    };
    assert_eq!(
        native::head("Title", "Author").unwrap(),
        doc("Title") + "<p>Author</p>"
    );
    assert_eq!(
        native::head("A & B <c> \"d\"", "").unwrap(),
        doc("A &amp; B &lt;c&gt; \"d\"")
    );
    assert_eq!(
        native::head("", "Ä <b>").unwrap(),
        doc("") + "<p>Ä &lt;b&gt;</p>"
    );
    assert_eq!(
        native::head("ünï 😀", "&&&").unwrap(),
        doc("ünï 😀") + "<p>&amp;&amp;&amp;</p>"
    );
    assert_eq!(native::TAIL, "</body></html>");
    for (text, pairs) in [
        ("", vec![]),
        (
            "abc\t/books/Reader/a.html\nxyz\t/books/Reader/b (xyz).html\n",
            vec![
                ("abc", "/books/Reader/a.html"),
                ("xyz", "/books/Reader/b (xyz).html"),
            ],
        ),
        // Lines without a tab are left out; the path is what follows the first tab.
        (
            "a\tx\nno tab\n\n\t\nb\ty\ttab\n",
            vec![("a", "x"), ("", ""), ("b", "y\ttab")],
        ),
        ("a\tx\r\nb\ty\r\n", vec![("a", "x"), ("b", "y")]),
    ] {
        let list = native::articles(text).unwrap();
        let got: Vec<(&str, &str)> = list.iter().map(|a| (&a.id[..], &a.path[..])).collect();
        assert_eq!(got, pairs, "{text:?}");
        let ids = native::finished(text).unwrap();
        assert_eq!(
            ids.len(),
            text.lines().filter(|l| !l.is_empty()).count(),
            "{text:?}"
        );
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
    assert_eq!(native::READ_PPM, 950_000);
    for (page, pages) in [
        (Some(5), Some(5)),
        (Some(4), Some(5)),
        (None, Some(5)),
        (Some(5), None),
        (None, None),
    ] {
        for ppm in [0, 500_000, 949_000, 950_000, 951_000, 1_000_000] {
            // On the last page, or near the end where pages are not counted.
            let expected = match (page, pages) {
                (Some(page), Some(pages)) => page >= pages,
                _ => ppm >= 950_000,
            };
            assert_eq!(
                native::is_read(page, pages, ppm),
                expected,
                "{page:?} {pages:?} {ppm}"
            );
        }
    }
}

/// What the native copier makes of `json` and how it ends, fed in pieces of
/// `piece` bytes.
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

/// The answers the copier is tried on.
pub(crate) fn bodies() -> Vec<String> {
    let long = "ä<p>x</p>\\n".repeat(2000);
    [
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
    .collect()
}

/// What each body of [`bodies`] is copied to, and how the copy ends, as the
/// `Debug` of the end and the text (the text of the last, a long one, is
/// cut here to its first 40 bytes and its length).
const COPIED: [&str; 16] = [
    "Whole \"<p>Hi</p>\"",
    "Missing \"\"",
    "Missing \"\"",
    "Cut \"cut off\"",
    "Whole \"aä€😀b�X� \\n\\t\\r\\u{8}\\u{c}\\\"\\\\/q\"",
    "Whole \"�\"",
    "Whole \"�\"",
    "Whole \"�ZZZZ rest\"",
    "Whole \"x\"",
    "Whole \"found\"",
    "Whole \"spaced\"",
    "Whole \"second\"",
    "Whole \"later\"",
    "Missing \"\"",
    "Missing \"\"",
    "Whole \"ä<p>x</p>\\nä<p>x</p>\\n\" 22000",
];

#[test]
fn an_article_is_copied_as_the_interface_6_copier_copies_it() {
    for (n, body) in bodies().iter().enumerate() {
        for piece in [1, 2, 3, 5, 7, 64, 4096, 100_000] {
            let (end, text) = new_copy(body.as_bytes(), piece);
            let text = String::from_utf8_lossy(&text);
            let shown = if n == 15 {
                format!(
                    "{end:?} {:?} {}",
                    text.chars().take(20).collect::<String>(),
                    text.len()
                )
            } else {
                format!("{end:?} {text:?}")
            };
            assert_eq!(shown, COPIED[n], "{body:.60?} {piece}");
        }
    }
}

#[test]
fn a_very_long_article_is_cut_at_the_most() {
    let body = format!(
        r#"{{"html_content":"{}"}}"#,
        "x".repeat(native::MAX_TEXT + 100)
    );
    let (end, text) = new_copy(body.as_bytes(), 4096);
    assert_eq!(end, native::Copied::Whole);
    assert_eq!(text.len(), native::MAX_TEXT);
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
