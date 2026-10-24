//! Hostile and unusual markup: deep nesting, unclosed tags, unquoted and
//! valueless attributes, entities out of range, surrogates, unknown and
//! overlong, CDATA, comments and processing instructions, byte order
//! marks and invalid UTF-8. Properties: no input panics, and feeding a
//! document in chunks of any size gives the same tokens as feeding it
//! whole; escaped text and attribute values decode to what was escaped.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_xml::{Token, Tokenizer, decode, escape};
use proptest::prelude::*;

/// A token that outlives the tokenizer's buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Tok {
    Start(String, Vec<(String, String)>, bool),
    End(String),
    Text(String),
    CData(String),
}

impl From<Token<'_>> for Tok {
    fn from(token: Token<'_>) -> Self {
        match token {
            Token::Start {
                name,
                attrs,
                self_closing,
            } => Tok::Start(
                name.into(),
                attrs.iter().map(|(k, v)| (k.into(), v.into())).collect(),
                self_closing,
            ),
            Token::End { name } => Tok::End(name.into()),
            Token::Text(text) => Tok::Text(text.into_owned()),
            Token::CData(text) => Tok::CData(text.into()),
        }
    }
}

/// The tokens of `data` fed in pieces of `chunk` bytes, adjacent text
/// and CDATA merged (where they are cut depends on the chunks); also the text as a
/// reader gets it, each text token decoded on its own.
fn run(data: &[u8], chunk: usize, html: bool) -> (Vec<Tok>, String) {
    let mut tokens: Vec<Tok> = Vec::new();
    let mut decoded = String::new();
    let mut tokenizer = if html {
        Tokenizer::html()
    } else {
        Tokenizer::new()
    };
    let mut emit = |token: Token<'_>| {
        if let Token::Text(text) = &token {
            decoded.push_str(&decode(text));
        }
        match (tokens.last_mut(), Tok::from(token)) {
            (Some(Tok::Text(last)), Tok::Text(more)) => last.push_str(&more),
            (Some(Tok::CData(last)), Tok::CData(more)) => last.push_str(&more),
            (_, token) => tokens.push(token),
        }
    };
    for piece in data.chunks(chunk.max(1)) {
        tokenizer.feed(piece, &mut emit);
    }
    tokenizer.finish(&mut emit);
    (tokens, decoded)
}

fn tokens(data: &str) -> Vec<Tok> {
    run(data.as_bytes(), usize::MAX, false).0
}

fn start(name: &str, attrs: &[(&str, &str)], self_closing: bool) -> Tok {
    Tok::Start(
        name.into(),
        attrs
            .iter()
            .map(|(k, v)| ((*k).into(), (*v).into()))
            .collect(),
        self_closing,
    )
}

fn text(t: &str) -> Tok {
    Tok::Text(t.into())
}

fn end(name: &str) -> Tok {
    Tok::End(name.into())
}

/// The decoded text of `data`, fed whole.
fn plain_text(data: &str) -> String {
    run(data.as_bytes(), usize::MAX, false).1
}

#[test]
fn deep_nesting_needs_no_recursion() {
    let depth = 100_000;
    let data = format!("{}x{}", "<d>".repeat(depth), "</d>".repeat(depth));
    let (tokens, _) = run(data.as_bytes(), 4096, false);
    assert_eq!(tokens.len(), 2 * depth + 1);
    assert_eq!(tokens[depth], text("x"));
    assert!(tokens[..depth].iter().all(|t| *t == start("d", &[], false)));
    assert!(tokens[depth + 1..].iter().all(|t| *t == end("d")));
}

#[test]
fn unclosed_markup_at_the_end_is_text() {
    assert_eq!(
        tokens("<p>abc<em class=\"x"),
        [start("p", &[], false), text("abc<em class=\"x")]
    );
    assert_eq!(tokens("<!-- never closed"), [text("<!-- never closed")]);
    assert_eq!(tokens("<![CDATA[open"), [text("<![CDATA[open")]);
    assert_eq!(tokens("<?pi"), [text("<?pi")]);
    assert_eq!(tokens("<"), [text("<")]);
}

#[test]
fn stray_closing_and_empty_tags_are_harmless() {
    // A `<` without a name after it is text, not a tag.
    assert_eq!(
        tokens("</p>a<>b< >c</>"),
        [end("p"), text("a<>b< >c"), end("")]
    );
}

#[test]
fn attributes_without_quotes_or_values() {
    let found = tokens("<input disabled name=q value=a&amp;b checked type = 'text' =x>");
    assert_eq!(
        found,
        [start(
            "input",
            &[("name", "q"), ("value", "a&amp;b"), ("type", "text")],
            false
        )]
    );
    let mut value = None;
    Tokenizer::parse(b"<a href=x.html?a=1&amp;b=2>", &mut |t| {
        if let Token::Start { attrs, .. } = t {
            value = attrs.get("href").map(|v| v.into_owned());
        }
    });
    assert_eq!(value.as_deref(), Some("x.html?a=1&b=2"));
}

#[test]
fn quoted_values_keep_markup_characters_and_the_first_duplicate_wins() {
    let mut seen = Vec::new();
    Tokenizer::parse(
        b"<img alt='a > b' title=\"it's\" alt=\"second\" data-x=\"<p>\"/>",
        &mut |t| {
            if let Token::Start {
                attrs,
                self_closing,
                ..
            } = t
            {
                seen.push((
                    attrs.get("alt").unwrap().into_owned(),
                    attrs.get("title").unwrap().into_owned(),
                    attrs.get("data-x").unwrap().into_owned(),
                    self_closing,
                ));
            }
        },
    );
    assert_eq!(
        seen,
        [(
            "a > b".to_string(),
            "it's".to_string(),
            "<p>".to_string(),
            true
        )]
    );
}

#[test]
fn an_unterminated_attribute_quote_swallows_the_rest_as_text() {
    assert_eq!(
        tokens("<a href=\"x>link</a>"),
        [text("<a href=\"x>link</a>")]
    );
}

#[test]
fn numeric_references_out_of_range_or_malformed_are_kept() {
    for kept in [
        "&#x110000;",
        "&#1114112;",
        "&#xD800;",
        "&#xDFFF;",
        "&#55296;",
        "&#0;",
        "&#x0;",
        "&#99999999999999999999;",
        "&#xFFFFFFFFF;",
        "&#;",
        "&#x;",
        "&#-1;",
        "&#12a;",
        "&# 65;",
    ] {
        assert_eq!(decode(kept), kept);
    }
    assert_eq!(decode("&#x10FFFF;"), "\u{10ffff}");
    assert_eq!(decode("&#X41;&#x00041;&#65;"), "AAA");
    assert_eq!(decode("&#xd7ff;&#xe000;"), "\u{d7ff}\u{e000}");
}

#[test]
fn named_references_unknown_unterminated_or_overlong_are_kept() {
    assert_eq!(decode("&AMP; &Amp;"), "&AMP; &Amp;");
    assert_eq!(decode("&amp &lt"), "&amp &lt");
    assert_eq!(decode("&notanentity;"), "&notanentity;");
    let long = format!("&{};", "a".repeat(40));
    assert_eq!(decode(&long), long);
    // A reference is decoded once, not again.
    assert_eq!(decode("&amp;amp; &amp;lt;"), "&amp; &lt;");
    assert_eq!(decode("&&amp;;"), "&&;");
    assert_eq!(decode("&"), "&");
    assert_eq!(decode("a&"), "a&");
    // Multi-byte text around and after a reference.
    assert_eq!(decode("ä&auml;ö&#x1F4D6;ü"), "ääö\u{1f4d6}ü");
    assert_eq!(decode("&ä;"), "&ä;");
}

#[test]
fn cdata_comments_and_processing_instructions() {
    assert_eq!(
        tokens(
            "<?xml version=\"1.0\"?><?pi a>b?><!-- <p>not a tag</p> --><r>\
             <![CDATA[<b>&amp;]]] ]]><!---->x</r>"
        ),
        [
            start("r", &[], false),
            Tok::CData("<b>&amp;]]] ".into()),
            text("x"),
            end("r"),
        ]
    );
    // A CDATA section that is not UTF-8 is dropped, what follows stays.
    let (found, _) = run(b"<r><![CDATA[\xff\xfe]]>after</r>", usize::MAX, false);
    assert_eq!(found, [start("r", &[], false), text("after"), end("r")]);
}

#[test]
fn a_byte_order_mark_does_not_hide_the_markup_after_it() {
    let found = tokens("\u{feff}<?xml version=\"1.0\"?><html><p>x</p></html>");
    assert_eq!(found[0], text("\u{feff}"));
    assert_eq!(
        found[1..],
        [
            start("html", &[], false),
            start("p", &[], false),
            text("x"),
            end("p"),
            end("html")
        ]
    );
}

#[test]
fn invalid_utf8_in_text_is_replaced_and_in_tags_dropped() {
    let (found, _) = run(b"<p>a\xffb</p><\xfe x=1>c</\xfe>", usize::MAX, false);
    assert_eq!(
        found,
        [
            start("p", &[], false),
            text("a\u{fffd}b"),
            end("p"),
            text("c")
        ]
    );
}

#[test]
fn utf16_documents_do_not_panic() {
    let utf16: Vec<u8> = "\u{feff}<p>Größe</p>"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    for chunk in [1, 3, 1000] {
        let _ = run(&utf16, chunk, false);
        let _ = run(&utf16, chunk, true);
    }
}

/// A document with a bit of everything, below the markup limit.
const RICH: &str = "\u{feff}<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
    <!DOCTYPE html>\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xml:lang=\"de\">\
    <head><title>Größe &amp; Maß</title><style>p > b { content: \"ä\" }</style></head>\
    <body><!-- a > comment with <b>tags</b> --><p class=\"x y\" id='p1' hidden>Grüße &#x1F4D6; \
    und &quot;Zitate&quot; mit &nbsp;Leerzeichen&hellip;<br/>\
    <a href=x.html#frag title=\"a > b\">Verweis</a></p><![CDATA[1 < 2 && 3 > 2]]>\
    <img src=\"data:image/png;base64,AAAA\" alt=\"\"/><unknown:tag a:b=\"c\"/>\
    Ende&#8212;über&#x2014;alles &unknown; &amp</body></html>";

#[test]
fn every_chunk_size_gives_the_same_tokens() {
    let whole = run(RICH.as_bytes(), usize::MAX, false);
    for chunk in 1..=RICH.len() + 1 {
        assert_eq!(run(RICH.as_bytes(), chunk, false), whole, "chunk {chunk}");
    }
}

#[test]
fn every_chunk_size_gives_the_same_tokens_in_html() {
    let page = "<p>a<script>if (a<b && c>d) { s = '</p>'; }</script>\
                <style>p > b { }</style>b</p><SCRIPT type=x>x</Script>c";
    let whole = run(page.as_bytes(), usize::MAX, true);
    assert!(whole.0.contains(&text("if (a<b && c>d) { s = '</p>'; }")));
    for chunk in 1..=page.len() + 1 {
        assert_eq!(run(page.as_bytes(), chunk, true), whole, "chunk {chunk}");
    }
}

#[test]
fn html_raw_text_keeps_multibyte_characters_in_any_chunking() {
    let page = "<style>p::after { content: \"äöü→\" }</style>";
    let whole = run(page.as_bytes(), usize::MAX, true);
    assert!(whole.0.contains(&text("p::after { content: \"äöü→\" }")));
    for chunk in 1..=page.len() {
        assert_eq!(run(page.as_bytes(), chunk, true), whole, "chunk {chunk}");
    }
}

#[test]
fn an_overlong_comment_is_skipped_whole() {
    let commented = format!(
        "<p>a</p><!-- {} --><p>b</p>",
        "<div class=\"old\">hidden</div>\n".repeat(200)
    );
    assert!(commented.len() > 5000);
    for chunk in [100, 1000, 5000, usize::MAX] {
        let (found, decoded) = run(commented.as_bytes(), chunk, false);
        assert_eq!(decoded, "ab", "chunk {chunk}");
        assert!(!found.contains(&start("div", &[("class", "old")], false)));
    }
    // A script in CDATA, as XHTML writes it.
    let script = format!(
        "<p>a</p><script>//<![CDATA[\n{}//]]></script><p>b</p>",
        "if (a > b && c < d) { e(); }\n".repeat(200)
    );
    for chunk in [100, 1000, 5000, usize::MAX] {
        let (_, decoded) = run(script.as_bytes(), chunk, false);
        assert_eq!(decoded, "a//b", "chunk {chunk}");
    }
}

#[test]
fn overlong_tags_are_skipped_and_what_follows_survives() {
    let attr = "A".repeat(1 << 20);
    let data = format!("<p>a</p><img src=\"{attr}\"/><p>b</p>");
    for chunk in [1000, 4096, 100_000] {
        let (found, decoded) = run(data.as_bytes(), chunk, false);
        assert_eq!(decoded, "ab", "chunk {chunk}");
        assert_eq!(
            found.len(),
            6,
            "chunk {chunk}: {:?}",
            &found[..found.len().min(8)]
        );
    }
}

#[test]
fn unbroken_text_streams_in_pieces_without_splitting_characters_or_references() {
    let data = format!("<p>{}</p>", "ä&amp;€&#x1F4D6;".repeat(2000));
    let expected = "ä&€\u{1f4d6}".repeat(2000);
    for chunk in [1, 2, 3, 5, 7, 11, 1023, 1024, 1025, 4096] {
        assert_eq!(
            run(data.as_bytes(), chunk, false).1,
            expected,
            "chunk {chunk}"
        );
    }
}

#[test]
fn text_runs_as_long_as_the_limits_stay_whole_after_merging() {
    // Around EAGER_TEXT (256) and MAX_TEXT (1024).
    for len in [255, 256, 257, 1023, 1024, 1025, 1026] {
        let body = "x".repeat(len);
        let data = format!("<p>{body}</p>");
        for chunk in [1, 100, 1024] {
            assert_eq!(
                run(data.as_bytes(), chunk, false).0,
                [start("p", &[], false), text(&body), end("p")],
                "len {len} chunk {chunk}"
            );
        }
    }
}

#[test]
fn escape_writes_the_four_characters_as_references() {
    let mut out = String::new();
    escape("<a href=\"x\">&'</a>", &mut out);
    assert_eq!(out, "&lt;a href=&quot;x&quot;&gt;&amp;'&lt;/a&gt;");
    assert_eq!(plain_text(&format!("<p>{out}</p>")), "<a href=\"x\">&'</a>");
}

/// Pieces documents are made of in the properties.
fn piece() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("<p>".to_string()),
        Just("</p>".to_string()),
        Just("<br/>".to_string()),
        Just("<a href=\"x.html\" title='t > u'>".to_string()),
        Just("<img alt=a src=b.png />".to_string()),
        Just("<!-- c > d -->".to_string()),
        Just("<![CDATA[x < y]]>".to_string()),
        // Long sections are read in pieces, with `]` and `>` in them.
        r"[a-zäö€ \n\]>]{0,700}".prop_map(|c| format!("<![CDATA[{c}]]>")),
        (0usize..300).prop_map(|n| format!("<![CDATA[{}]]>", "wort] ".repeat(n))),
        Just("<?pi data?>".to_string()),
        Just("<!DOCTYPE html>".to_string()),
        Just("&amp;".to_string()),
        Just("&#x1F4D6;".to_string()),
        Just("&unknown;".to_string()),
        Just("& ".to_string()),
        Just("<".to_string()),
        Just(">".to_string()),
        Just("\"".to_string()),
        "[a-zäöü€ \n]{0,40}",
        // Names without an `s`, so no `script` or `style` in HTML.
        "[a-r]{1,8}".prop_map(|n| format!("<{n}>")),
        "[a-r]{1,8}".prop_map(|n| format!("</{n}>")),
        (0usize..1100).prop_map(|n| "w".repeat(n)),
        (0usize..300).prop_map(|n| "wort ".repeat(n)),
    ]
}

/// Documents of at most `max` bytes. Below the markup limit (4096), no
/// markup is ever overlong; past it, overlong markup is skipped, and
/// where the skipping starts depends on the chunks.
fn document(max: usize) -> impl Strategy<Value = String> {
    proptest::collection::vec(piece(), 0..24)
        .prop_map(|pieces| pieces.concat())
        .prop_filter("too long", move |doc| doc.len() <= max)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(128)))]

    #[test]
    fn arbitrary_bytes_never_panic(
        data in proptest::collection::vec(any::<u8>(), 0..3000),
        chunk in 1usize..2000,
        html in any::<bool>(),
    ) {
        let (tokens, _) = run(&data, chunk, html);
        for token in tokens {
            if let Tok::Start(_, attrs, _) = token {
                for (_, value) in attrs {
                    let _ = decode(&value);
                }
            }
        }
    }

    #[test]
    fn chunking_never_changes_the_tokens(doc in document(4096), chunk in 1usize..300) {
        let whole = run(doc.as_bytes(), usize::MAX, false);
        prop_assert_eq!(run(doc.as_bytes(), chunk, false), whole);
    }

    #[test]
    fn chunking_never_changes_the_tokens_in_html_with_ascii_scripts(
        doc in document(2000),
        script in "[ -~]{0,300}",
        chunk in 1usize..300,
    ) {
        let script = script.replace("</", "<");
        let doc = format!("{doc}<script>{script}</script>{doc}");
        let whole = run(doc.as_bytes(), usize::MAX, true);
        prop_assert_eq!(run(doc.as_bytes(), chunk, true), whole);
    }

    #[test]
    fn escaped_text_reads_back_as_it_was(s in "\\PC{0,3000}", chunk in 1usize..2000) {
        let mut escaped = String::new();
        escape(&s, &mut escaped);
        let data = format!("<p>{escaped}</p>");
        let (_, decoded) = run(data.as_bytes(), chunk, false);
        prop_assert_eq!(decoded, s);
    }

    #[test]
    fn escaped_attribute_values_read_back_as_they_were(s in "\\PC{0,200}") {
        let mut escaped = String::new();
        escape(&s, &mut escaped);
        let data = format!("<a x=\"1\" title=\"{escaped}\" y=2>");
        let mut found = None;
        Tokenizer::parse(data.as_bytes(), &mut |t| {
            if let Token::Start { attrs, .. } = t {
                found = attrs.get("title").map(|v| v.into_owned());
            }
        });
        prop_assert_eq!(found, Some(s));
    }

    #[test]
    fn decoding_text_without_references_borrows_it(s in "[^&]{0,100}") {
        prop_assert!(matches!(decode(&s), std::borrow::Cow::Borrowed(t) if t == s));
    }
}
