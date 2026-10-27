//! Hostile chapters: XHTML nested very deep, huge attributes, broken
//! entities and tags, pictures that are missing, damaged or named oddly,
//! and stylesheets with values, comments, strings and `@` rules meant to
//! trip a parser. The text must come through, the book's styles must stay
//! within their bounds, and nothing may panic.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::{Recording, blocks, book, book_with, text, words};
use paperpaw_plugin_format_epub::EpubDocument;
use paperpaw_sdk::MemoryStorage;
use paperpaw_sdk::document::{Align, BlockKind, Document, Locator};

const PNG: &[u8] = include_bytes!("../../../crates/paperpaw-image/tests/data/gray.png");

fn open(epub: Vec<u8>) -> (MemoryStorage, EpubDocument) {
    let storage = MemoryStorage::new().with("book.epub", epub);
    let doc = EpubDocument::open(&storage, "book.epub").unwrap();
    (storage, doc)
}

fn one(body: &str) -> (MemoryStorage, EpubDocument) {
    open(book(&[body.to_string()], &[]))
}

#[test]
fn deeply_nested_elements_keep_their_text_and_positions() {
    let depth = 3000;
    let mut body = String::new();
    for i in 0..depth {
        body.push_str(if i % 2 == 0 { "<div>" } else { "<span>" });
        if i % 500 == 0 {
            body.push_str(&format!("level{i} "));
        }
    }
    body.push_str("bottom");
    for i in (0..depth).rev() {
        body.push_str(if i % 2 == 0 { "</div>" } else { "</span>" });
    }
    body.push_str("<p>after</p>");
    let (storage, doc) = one(&body);
    assert_eq!(
        words(&doc, &storage),
        [
            "level0",
            "level500",
            "level1000",
            "level1500",
            "level2000",
            "level2500",
            "bottom",
            "after"
        ]
    );
    // The deepest word's position goes to KOReader's form and back; the
    // path is kept to its first levels, so it stays short.
    let read = blocks(&doc, &storage);
    let (_, block) = read
        .iter()
        .find(|(_, b)| b.text.contains("bottom"))
        .unwrap();
    let at = Locator::new(0, block.start + block.text.find("bottom").unwrap() as u32);
    let xpointer = doc.xpointer(&storage, at).unwrap();
    assert!(
        xpointer.starts_with("/body/DocFragment[1]/body/div[1]/span[1]/div[1]"),
        "{xpointer}"
    );
    assert!(xpointer.len() < 256, "{xpointer}");
    assert_eq!(doc.locate_xpointer(&storage, &xpointer), Some(at));
}

#[test]
fn deeply_nested_unclosed_inline_elements_keep_their_text() {
    // `<b><i>` opened and never closed: a common damage.
    let body = format!("<p>{}deep text</p><p>next</p>", "<b><i>".repeat(5000));
    let (storage, doc) = one(&body);
    assert_eq!(words(&doc, &storage), ["deep", "text", "next"]);
}

#[test]
fn huge_attribute_values_do_not_cost_the_text() {
    // Tags longer than the tokenizer keeps (4 KB) are skipped, their text
    // is not.
    let huge = "x".repeat(1 << 20);
    let body = format!(
        "<p class=\"{huge}\" id=\"{huge}\" style=\"{huge}\">First</p>\
         <p><a href=\"{huge}\">Linked</a> <a href=\"#{}\">Long</a> \
         <a href=\"#{}\">Longer</a></p>\
         <p title='{huge}' data-x=\"{}\">Last</p>",
        "y".repeat(1500),
        "y".repeat(3000),
        "&amp;".repeat(100_000),
    );
    let (storage, doc) = one(&body);
    assert_eq!(
        words(&doc, &storage),
        ["First", "Linked", "Long", "Longer", "Last"]
    );
    // The link of a tag that was kept is there; one whose target is over
    // 2000 bytes is left out (an anchor that long matches nothing anyway).
    let links = doc.links(&storage, 0, 0, u32::MAX);
    assert_eq!(links.len(), 1);
    assert!(links[0].href.ends_with(&format!("#{}", "y".repeat(1500))));
    assert_eq!(
        doc.link_target(&storage, &links[0].href),
        Some(Locator::new(0, 0))
    );
}

#[test]
fn broken_entities_are_kept_as_written() {
    let body = "<p>a &amp b</p><p>c &#99999999; d</p><p>e &#xD800; f</p>\
                <p>g &nosuch; h</p><p>i &#0; j</p><p>k &#x; l</p><p>m &; n</p>\
                <p>o &lt;tag&gt; &#x41;&#66; p</p><p>q &amp;amp; r</p>";
    let (storage, doc) = one(body);
    assert_eq!(
        text(&doc, &storage),
        "a &amp b\nc &#99999999; d\ne &#xD800; f\ng &nosuch; h\ni &#0; j\nk &#x; l\n\
         m &; n\no <tag> AB p\nq &amp; r"
    );
}

#[test]
fn unclosed_and_mismatched_tags_keep_the_text_in_order() {
    let body = "<p>one<p>two</div></div></span><div><b>three <i>four</b> five</i>\
                <h2>six</p></h2><ul><li>seven<li>eight</ul></body></html><p>nine";
    let (storage, doc) = one(body);
    let words = words(&doc, &storage);
    let words: Vec<&str> = words
        .iter()
        .map(String::as_str)
        .filter(|w| *w != "•")
        .collect();
    assert_eq!(
        words,
        [
            "one", "two", "three", "four", "five", "six", "seven", "eight", "nine"
        ]
    );
}

#[test]
fn a_chapter_cut_off_in_a_tag_keeps_what_came_before() {
    for cut in [
        "<p>kept</p><p class=\"x",
        "<p>kept</p><",
        "<p>kept</p><!-- open",
        "<p>kept</p><![CDATA[ x",
    ] {
        let (storage, doc) = one(cut);
        let words = words(&doc, &storage);
        assert_eq!(words.first().map(String::as_str), Some("kept"), "{cut}");
    }
}

#[test]
fn scripts_styles_and_hidden_parts_are_not_shown() {
    let body = "<script>var hidden = '<p>no</p>';</script><p>shown</p>\
                <style>p { color: red }</style><svg><text>vector</text></svg>\
                <p style=\"display:none\">invisible</p><p>also shown</p>";
    let (storage, doc) = one(body);
    assert_eq!(words(&doc, &storage), ["shown", "also", "shown"]);
}

#[test]
fn cdata_is_shown_as_written() {
    let (storage, doc) = one("<p><![CDATA[a &amp; b < c]]></p>");
    assert_eq!(text(&doc, &storage), "a &amp; b < c");
}

#[test]
fn missing_or_damaged_pictures_show_their_alternative_text() {
    let truncated = PNG[..PNG.len() / 2].to_vec();
    let garbage = b"\x89PNG\r\n\x1a\n not really a picture".to_vec();
    let body = "<p>Before</p>\
        <img src=\"missing.png\" alt=\"Missing picture\"/>\
        <img src=\"garbage.png\" alt=\"Garbage\"/>\
        <img src=\"empty.png\" alt=\"Empty\"/>\
        <img src=\"dir/\" alt=\"A folder\"/>\
        <img src=\"\" alt=\"No source\"/>\
        <img src=\"data:image/png;base64,AAAA\" alt=\"Inline\"/>\
        <img src=\"truncated.png\" alt=\"Truncated\"/>\
        <img src=\"good.png\" alt=\"Good\"/>\
        <p>After</p>";
    let epub = book(
        &[body.into()],
        &[
            ("OEBPS/garbage.png", garbage),
            ("OEBPS/empty.png", Vec::new()),
            ("OEBPS/truncated.png", truncated),
            ("OEBPS/good.png", PNG.to_vec()),
        ],
    );
    let (storage, doc) = open(epub);
    let read = blocks(&doc, &storage);
    let shown: Vec<(BlockKind, &str)> = read
        .iter()
        .map(|(_, b)| (b.kind, b.text.as_str()))
        .collect();
    // Pictures without a source or inline ones are left out.
    assert_eq!(
        shown,
        [
            (BlockKind::Paragraph, "Before"),
            (BlockKind::Paragraph, "Missing picture"),
            // A PNG signature: its size is read from what follows.
            (BlockKind::Image, "Garbage"),
            (BlockKind::Paragraph, "Empty"),
            (BlockKind::Paragraph, "A folder"),
            // Its size is in its first bytes: laid out as a picture, its
            // decoding fails only when its page shows it.
            (BlockKind::Image, "Truncated"),
            (BlockKind::Image, "Good"),
            (BlockKind::Paragraph, "After"),
        ]
    );
    let image = |alt: &str| {
        read.iter()
            .find(|(_, b)| b.text == alt)
            .and_then(|(_, b)| b.image.clone())
            .unwrap()
    };
    for alt in ["Garbage", "Truncated"] {
        let picture = image(alt);
        // Laid out no larger than pictures are decoded.
        assert!(
            picture.width <= 1600 && picture.height <= 1600,
            "{picture:?}"
        );
        assert_eq!(doc.prepare_image(&storage, &picture), None, "{alt}");
    }
    let good = image("Good");
    let ready = doc.prepare_image(&storage, &good).unwrap_or(good);
    assert!(ready.width > 0 && ready.height > 0);
}

/// An `<img>` whose alternative text is `alt`.
fn picture_with_alt(alt: &str) -> (MemoryStorage, EpubDocument) {
    let body = format!("<img src=\"good.png\" alt=\"{alt}\"/><p>After</p>");
    open(book(&[body], &[("OEBPS/good.png", PNG.to_vec())]))
}

#[test]
fn long_alternative_text_is_cut() {
    // As long as a tag the tokenizer keeps (4 KB) allows: with the
    // picture's cache name, more than a record holds.
    let (storage, doc) = picture_with_alt(&"a".repeat(4000));
    let read = blocks(&doc, &storage);
    assert_eq!(read.len(), 2);
    assert_eq!(read[0].1.kind, BlockKind::Image);
    assert!(read[0].1.text.len() < 4000);
    assert!(read[0].1.text.starts_with("aaaa"));
    assert_eq!(read[1].1.text, "After");
}

#[test]
fn long_alternative_text_is_cut_at_a_character() {
    // The record of a picture (its cache name, a tab, the alternative
    // text) is cut at 4000 bytes wherever that falls; three lengths before
    // three-byte characters, so one cut is inside one.
    for pad in 0..3 {
        let alt = format!("{}{}", "x".repeat(pad), "€".repeat(1330));
        let result = std::panic::catch_unwind(|| {
            let (storage, doc) = picture_with_alt(&alt);
            text(&doc, &storage)
        });
        let text = result.unwrap_or_else(|_| panic!("panicked with {pad} bytes before"));
        assert!(text.ends_with("After"));
    }
}

#[test]
fn pictures_named_oddly_stay_in_the_book() {
    let body = "<img src=\"../../../../../etc/passwd\" alt=\"Up\"/>\
        <img src=\"a%2F..%2F..%2Fx.png\" alt=\"Escaped\"/>\
        <img src=\"pic&#10;ture.png\" alt=\"Newline\"/>\
        <img src=\"pic&#9;ture.png\" alt=\"Tab\"/>\
        <image xlink:href=\"good.png\"/>";
    let epub = book(&[body.into()], &[("OEBPS/good.png", PNG.to_vec())]);
    let storage = Recording::new(MemoryStorage::new().with("book.epub", epub));
    let doc = EpubDocument::open(&storage, "book.epub").unwrap();
    let read = blocks(&doc, &storage);
    let texts: Vec<&str> = read.iter().map(|(_, b)| b.text.as_str()).collect();
    assert_eq!(texts, ["Up", "Escaped", "Newline", "Tab", ""]);
    assert_eq!(read[4].1.kind, BlockKind::Image);
    assert_eq!(storage.written_outside(&[]), Vec::<String>::new());
}

#[test]
fn hostile_stylesheets_keep_their_sane_rules_within_bounds() {
    let mut css = String::from(
        "@charset \"utf-8\";\n@import url(style.css);\n@import \"style.css\";\n\
         .centre { text-align: center }\n\
         .huge { text-indent: 99999999999999999999em; margin-top: 1e30px; \
           padding-top: 99999px; font-size: 99999999% }\n\
         .tiny { text-indent: -99999999em; margin-top: -5em; font-size: 0.0001em }\n\
         .nan { text-indent: NaNem; font-size: inf%; margin-top: infem }\n\
         .str { content: \"unterminated; text-align: right }\n\
         .after-string { text-align: right }\n",
    );
    // Deeply nested `@media`, unbalanced braces, a long selector and
    // many rules.
    css.push_str(&"@media screen {".repeat(10_000));
    css.push_str(".inside { text-align: right }");
    css.push_str(&"}".repeat(10_000));
    css.push_str("\n.after-media { text-align: end }\n");
    // Stray braces belong to the next rule's selector, which is dropped
    // (as browsers do); the one after it applies.
    css.push_str("}}}}}} .lost { font-weight: bold } .after-braces { font-weight: bold }\n");
    css.push_str(&format!(
        "{} {{ text-align: right }}\n",
        ".".repeat(100_000)
    ));
    for i in 0..2000 {
        css.push_str(&format!(".r{i} {{ text-align: right }}\n"));
    }
    css.push_str(".late { text-align: center }\n/* unterminated comment .x { color: red }");
    let body = "<p class=\"centre\">Centred</p><p class=\"huge\">Huge</p>\
        <p class=\"tiny\">Tiny</p><p class=\"nan\">Nan</p><p class=\"after-string\">String</p>\
        <p class=\"inside\">Inside</p><p class=\"after-media\">Media</p>\
        <p class=\"after-braces\">Braces</p><p>Plain</p>";
    let epub = book_with(&[body.into()], &css, &[], false);
    let (storage, doc) = open(epub);
    let read = blocks(&doc, &storage);
    let block = |text: &str| &read.iter().find(|(_, b)| b.text == text).unwrap().1;
    assert_eq!(read.len(), 9);
    assert_eq!(block("Centred").align, Align::Center);
    assert_eq!(block("Media").align, Align::End);
    // Rules in `@media` are skipped.
    assert_eq!(block("Inside").align, Align::Auto);
    assert!(block("Braces").style_at(0).bold());
    for (_, b) in &read {
        assert!(b.indent.is_none_or(|i| (-500..=500).contains(&i)), "{b:?}");
        assert!(b.margins.0 <= 1000 && b.margins.1 <= 1000, "{b:?}");
        assert!(b.space.is_none_or(|s| s <= 1600), "{b:?}");
        assert!(b.size.is_none_or(|s| (50..=300).contains(&s)), "{b:?}");
    }
    assert_eq!(block("Huge").indent, Some(500));
    // Hanging as far as it may.
    assert_eq!(block("Tiny").indent, Some(-500));
    assert_eq!(block("Plain").align, Align::Auto);
}

#[test]
fn hostile_style_elements_and_attributes_keep_the_text() {
    let body = format!(
        "<style>{}p {{ text-align: right }}</style>\
         <style>/* never closed</style>\
         <p style=\"text-align: center; ;;; :: font-weight\">One</p>\
         <p style=\"{}\">Two</p>\
         <p style=\"text-indent: 1e400em; font-size: -5em; margin-top: --1px\">Three</p>",
        "{".repeat(50_000),
        "text-align: center;".repeat(200),
    );
    let (storage, doc) = one(&body);
    let read = blocks(&doc, &storage);
    let texts: Vec<&str> = read.iter().map(|(_, b)| b.text.as_str()).collect();
    assert_eq!(texts, ["One", "Two", "Three"]);
    assert_eq!(read[1].1.align, Align::Center);
    for (_, b) in &read {
        assert!(b.indent.is_none_or(|i| (0..=500).contains(&i)), "{b:?}");
        assert!(b.size.is_none_or(|s| (50..=300).contains(&s)), "{b:?}");
    }
}

#[test]
fn stylesheets_that_are_not_text_are_skipped() {
    let css: String = (0..=255u8).map(char::from).collect::<String>().repeat(50);
    let mut bytes = css.into_bytes();
    bytes.extend_from_slice(&[0xff, 0xfe, 0xc3, 0x28, 0xe2, 0x82]);
    let mut epub_css = String::from_utf8_lossy(&bytes).into_owned();
    epub_css.push_str(".c { text-align: center }");
    let epub = book_with(&["<p class=\"c\">Text</p>".into()], &epub_css, &[], true);
    let (storage, doc) = open(epub);
    assert_eq!(text(&doc, &storage), "Text");
}

#[test]
fn a_chapter_of_only_markup_has_no_blocks() {
    let (storage, doc) = one("<div><span></span><p>   </p><br/><b>\n\t</b></div>");
    assert_eq!(doc.block_at(&storage, Locator::new(0, 0)).unwrap(), None);
    assert_eq!(doc.progress(Locator::new(0, 0)), 0.0);
}

#[test]
fn locators_past_the_end_or_in_other_sections_find_nothing() {
    let (storage, doc) = one("<p>only</p>");
    assert_eq!(
        doc.block_at(&storage, Locator::new(0, u32::MAX)).unwrap(),
        None
    );
    assert_eq!(doc.block_at(&storage, Locator::new(1, 0)).unwrap(), None);
    assert_eq!(
        doc.block_at(&storage, Locator::new(u32::MAX, 0)).unwrap(),
        None
    );
    assert!(doc.progress(Locator::new(u32::MAX, u32::MAX)) <= 1.0);
}

#[test]
fn small_capitals_of_the_books_stylesheet_apply() {
    let body = "<p class=\"sc\">From the sheet</p>\
                <p style=\"font-variant: small-caps\">From the attribute</p>";
    let epub = book_with(
        &[body.into()],
        ".sc { font-variant: small-caps }",
        &[],
        false,
    );
    let (storage, doc) = open(epub);
    let read = blocks(&doc, &storage);
    assert!(read[1].1.style_at(0).small_caps());
    assert!(read[0].1.style_at(0).small_caps());
}

/// The body's own margin and padding are the page's, which the reader's
/// margins already are: they do not come on top of those, while a wrapper
/// inside the body still sets its text in.
#[test]
fn the_bodys_sides_are_not_added_to_the_readers_margins() {
    let body = "<p>Plain</p><div class=\"wrap\"><p>Wrapped</p></div>";
    let epub = book_with(
        &[body.into()],
        "body { margin: 0 5%; padding: 0 1em } .wrap { margin-left: 2em }",
        &[],
        false,
    );
    let (storage, doc) = open(epub);
    let read = blocks(&doc, &storage);
    let block = |text: &str| &read.iter().find(|(_, b)| b.text == text).unwrap().1;
    assert_eq!(block("Plain").margins, (0, 0));
    assert_eq!(block("Wrapped").margins, (200, 0));
}

/// Block quotes, poems and hanging indents keep their room at the sides,
/// lists their numbers and nesting, footnote marks and struck words their
/// marks, and a book's line height comes along.
#[test]
fn sides_lists_marks_and_line_heights_of_the_book_come_along() {
    let body = "<blockquote><p>Quoted</p></blockquote>\
                <p class=\"hang\">Hanging</p>\
                <ol start=\"3\"><li>Third</li><li>Fourth<ul><li>Inner</li></ul></li></ol>\
                <p>Note<sup>1</sup> H<sub>2</sub>O <u>under</u> <del>gone</del></p>\
                <p class=\"airy\">Airy</p>";
    let epub = book_with(
        &[body.into()],
        ".hang { margin-left: 2em; text-indent: -2em } .airy { line-height: 1.8 }",
        &[],
        false,
    );
    let (storage, doc) = open(epub);
    let read = blocks(&doc, &storage);
    let block = |text: &str| {
        &read
            .iter()
            .find(|(_, b)| b.text.starts_with(text))
            .unwrap_or_else(|| panic!("{text}: {read:?}"))
            .1
    };
    assert_eq!(block("Quoted").margins, (150, 150));
    assert_eq!(block("Hanging").margins, (200, 0));
    assert_eq!(block("Hanging").indent, Some(-200));
    assert_eq!(block("3. Third").margins, (150, 0));
    assert!(block("4. Fourth").text.starts_with("4. "));
    // A list in a list: set in twice, with bullets.
    assert_eq!(block("• Inner").margins, (300, 0));
    let note = block("Note");
    let at = |word: &str| note.style_at(note.text.find(word).unwrap());
    assert!(at("1").superscript());
    assert!(at("2").subscript());
    assert!(at("under").underline());
    assert!(at("gone").strike());
    assert!(!at("Note").superscript());
    assert_eq!(block("Airy").line_height, Some(180));
    assert_eq!(block("Quoted").line_height, None);
}
