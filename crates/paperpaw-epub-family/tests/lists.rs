//! Lists: markers as `list-style-type`, the `list-style` shorthand and
//! the old `type` and `value` attributes ask, and items whose lines hang
//! after their marker.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::{blocks, book_with};
use paperpaw_epub_family::EpubDocument;
use paperpaw_sdk::MemoryStorage;
use paperpaw_sdk::document::Block;

/// The blocks of a book of one chapter of `body`, with stylesheet `css`.
fn read(body: &str, css: &str) -> Vec<Block> {
    let epub = book_with(&[body.into()], css, &[], false);
    let storage = MemoryStorage::new().with("book.epub", epub);
    let doc = EpubDocument::open(&storage, "book.epub").unwrap();
    blocks(&doc, &storage).into_iter().map(|(_, b)| b).collect()
}

fn texts(body: &str, css: &str) -> Vec<String> {
    read(body, css).into_iter().map(|b| b.text).collect()
}

#[test]
fn each_list_style_type_writes_its_markers() {
    let css = ".n { list-style-type: none } .c { list-style-type: circle } \
               .s { list-style-type: square } .d { list-style-type: decimal } \
               .la { list-style-type: lower-alpha } .ua { list-style-type: upper-latin } \
               .lr { list-style-type: lower-roman } .ur { list-style-type: upper-roman } \
               .odd { list-style-type: hiragana }";
    let list = |tag: &str, class: &str| {
        format!("<{tag} class=\"{class}\"><li>{class}t</li><li>{class}t</li></{tag}>")
    };
    let body: String = [
        list("ul", ""),
        list("ol", ""),
        list("ul", "n"),
        list("ul", "c"),
        list("ul", "s"),
        list("ul", "d"),
        list("ol", "la"),
        list("ol", "ua"),
        list("ol", "lr"),
        list("ol", "ur"),
        list("ol", "odd"),
        list("ul", "odd"),
    ]
    .concat();
    let got = texts(&body, css);
    let want = [
        "•\u{a0}t",
        "•\u{a0}t",
        "1.\u{a0}t",
        "2.\u{a0}t",
        "nt",
        "nt",
        "◦\u{a0}ct",
        "◦\u{a0}ct",
        "▪\u{a0}st",
        "▪\u{a0}st",
        "1.\u{a0}dt",
        "2.\u{a0}dt",
        "a.\u{a0}lat",
        "b.\u{a0}lat",
        "A.\u{a0}uat",
        "B.\u{a0}uat",
        "i.\u{a0}lrt",
        "ii.\u{a0}lrt",
        "I.\u{a0}urt",
        "II.\u{a0}urt",
        // A value not known: the list's own markers.
        "1.\u{a0}oddt",
        "2.\u{a0}oddt",
        "•\u{a0}oddt",
        "•\u{a0}oddt",
    ];
    assert_eq!(got, want);
}

#[test]
fn shorthand_attributes_and_inheritance_choose_markers_too() {
    let css = ".sq { list-style: square inside } .no { list-style: none } \
               .roman { list-style-type: upper-roman }";
    let body = "<ul class=\"sq\"><li>a</li></ul>\
                <ol class=\"no\"><li>b</li></ol>\
                <ol type=\"a\" start=\"27\"><li>c</li></ol>\
                <ol type=\"I\"><li>d</li><li value=\"9\">e</li><li>f</li></ol>\
                <ol start=\"4\" type=\"i\"><li type=\"A\">g</li></ol>\
                <ul type=\"circle\"><li>h</li></ul>\
                <ol class=\"roman\"><li>i</li></ol>\
                <ol style=\"list-style-type: lower-roman\" type=\"A\"><li>j</li></ol>\
                <ol><li>k<ul><li>l</li></ul></li></ol>";
    assert_eq!(
        texts(body, css),
        [
            "▪\u{a0}a",
            "b",
            "aa.\u{a0}c",
            "I.\u{a0}d",
            "IX.\u{a0}e",
            "X.\u{a0}f",
            "D.\u{a0}g",
            "◦\u{a0}h",
            "I.\u{a0}i",
            // The stylesheet over the attribute.
            "i.\u{a0}j",
            // A list's own markers, not those of the list around it.
            "1.\u{a0}k",
            "•\u{a0}l",
        ]
    );
}

#[test]
fn an_items_lines_hang_after_its_marker() {
    let got = read(
        "<ul><li>Bullet</li></ul><ol><li>Number</li></ol>\
         <ol style=\"list-style: none\"><li>Plain</li></ol>",
        "",
    );
    // Set in by the list (1.5 em) and by the marker, the first line back
    // by the marker.
    assert_eq!((got[0].margins, got[0].indent), ((220, 0), Some(-70)));
    assert_eq!((got[1].margins, got[1].indent), ((250, 0), Some(-100)));
    // No marker, no hanging.
    assert_eq!((got[2].margins, got[2].indent), ((150, 0), None));
}

#[test]
fn an_item_of_paragraphs_has_its_marker_on_the_first() {
    let got = read(
        "<ol><li><p>First</p><p>Second</p></li><li>\n  <div><p>Third</p></div></li></ol>\
         <ul><li><ul><li>Inner</li></ul></li><li></li></ul>",
        "p { text-indent: 1.5em }",
    );
    let shown: Vec<_> = got
        .iter()
        .map(|b| (b.text.as_str(), b.margins, b.indent))
        .collect();
    assert_eq!(
        shown,
        [
            ("1.\u{a0}First", (250, 0), Some(-100)),
            // The item's next paragraph under the text, as indented as
            // the book's.
            ("Second", (250, 0), Some(150)),
            ("2.\u{a0}Third", (250, 0), Some(-100)),
            // An item before whose marker no text came keeps the marker,
            // on its own line; an empty item shows none.
            ("•", (220, 0), Some(-70)),
            ("•\u{a0}Inner", (440, 0), Some(-70)),
        ]
    );
}

#[test]
fn hostile_numbers_and_values_neither_panic_nor_grow() {
    let body = format!(
        "<ol start=\"4294967295\" type=\"a\"><li>a</li><li>b</li></ol>\
         <ol start=\"3999\" type=\"I\"><li>c</li><li>d</li></ol>\
         <ol start=\"-5\"><li value=\"0\">e</li><li value=\"x\">f</li></ol>\
         <ol type=\"{}\" style=\"list-style-type: {}\"><li>g</li></ol>",
        "z".repeat(200),
        "q".repeat(200)
    );
    let got = read(&body, "");
    let shown: Vec<_> = got.iter().map(|b| b.text.as_str()).collect();
    assert_eq!(
        shown,
        [
            "mwlqkwu.\u{a0}a",
            // The count stays at the largest number.
            "mwlqkwu.\u{a0}b",
            "MMMCMXCIX.\u{a0}c",
            // Roman numerals end at 3999: then numbers.
            "4000.\u{a0}d",
            "0.\u{a0}e",
            "1.\u{a0}f",
            "1.\u{a0}g",
        ]
    );
    // A long marker sets its item in by at most 4 em.
    assert!(got.iter().all(|b| b.margins.0 <= 150 + 400), "{got:?}");
}
