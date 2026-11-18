//! The native HTML reader gives the text interface 6 gave on fixed mails (kept
//! in `testdata/html.golden`, written from the interface-6 code before it was
//! removed), and the same text for a mail cut into pieces anywhere. It
//! differs, on purpose, where interface 6 panicked (a non-ASCII character
//! inside `<style>` or `<script>`), lost the rest of the mail (a comment
//! longer than 1,024 bytes) or grew without end (the text limit).
#![allow(clippy::unwrap_used, reason = "test assertions")]
use paperpaw_plugin_mail::native_model::html::{self as new, HtmlText as New, MAX_TEXT};

/// A small deterministic generator, so that failures repeat.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// The lines of the golden file with the tag `tag`.
fn golden(tag: &str) -> Vec<&'static str> {
    include_str!("../testdata/html.golden")
        .lines()
        .filter_map(|line| line.strip_prefix(tag)?.strip_prefix('\t'))
        .collect()
}

fn new_text(html: &str) -> String {
    new::to_text(html).unwrap()
}

const FIXED: &[&str] = &[
    "",
    "plain text",
    "<p>Hello &amp; welcome</p><p>Second</p>",
    "a<br>b<br/>c<BR />d",
    "<ul><li>one</li><li>two<li>three</ul>",
    "<table><tr><td>a</td><td>b</td></tr><tr><td>c</td></tr></table>",
    "<style>p { color: red }</style><p>x</p>",
    "<SCRIPT type=text/javascript>if (a < b) { x = '</p>' }</SCRIPT>after",
    "<head><title>t</title></head><body>body</body>",
    "<script/>kept<style/>kept too",
    "<!-- a > b --><p>after comment</p>",
    "<!----><!-- -- --><!--->x",
    "<!-->x",
    "&lt;tag&gt; &quot;q&quot; &apos;a&apos; &nbsp;|&nbsp; &#39; &#x41; &#X42;",
    "&auml;&ouml;&uuml;&Auml;&Ouml;&Uuml;&szlig;&euro;&copy;&reg;&eacute;&egrave;&agrave;&ccedil;",
    "&unknown; &; &#; &#xZZ; &#99999999999; &#0; &amp &lt;x",
    "&&&&;;;&#&#;",
    "<<<<script>&&&;;&#99999999999;</style>",
    "a   b \t\n c \u{a0}\u{a0} d \u{200b} e",
    "tail & open",
    "tail <open tag",
    "<p>  leading</p>   <p>trailing  </p>   ",
    "<div><div><p>nest</p></div></div><hr><h1>H</h1><h6>h</h6>",
    "<pre>pre\n  formatted</pre><blockquote>quote</blockquote>",
    "<a href=\"x>y\">link</a> <b>bold</b> <img src=x alt=\"\">",
    "caf\u{e9} \u{4e16}\u{754c} \u{1f600} <p>\u{e4}\u{f6}\u{fc}</p>",
    "\u{1}\u{2}control\u{7f}chars\r\nline",
    "</p></div></li>stray ends",
    "<>< ><  p  >text</  p  >",
];

#[test]
fn fixed_mails_give_the_text_interface_6_gave() {
    let expected = golden("fixed");
    assert_eq!(expected.len(), FIXED.len());
    for (html, expected) in FIXED.iter().zip(expected) {
        assert_eq!(format!("{html:?}\t{:?}", new_text(html)), expected);
    }
}

/// A random mail of tags, entities, comments and text (ASCII inside what is
/// skipped, and comments below the tag limit, where interface 6 is right).
fn random_mail(rng: &mut Rng) -> String {
    const PIECES: &[&str] = &[
        "<p>",
        "</p>",
        "<br>",
        "<br/>",
        "<li>",
        "</li>",
        "<ul>",
        "</ul>",
        "<tr>",
        "<td>",
        "<div>",
        "</div>",
        "<b>",
        "</b>",
        "<a href=\"x\">",
        "</a>",
        "<style>",
        "</style>",
        "<script>",
        "</script>",
        "<head>",
        "</head>",
        "<!--",
        "-->",
        "<!-- x -->",
        "&amp;",
        "&lt;",
        "&gt;",
        "&nbsp;",
        "&auml;",
        "&#65;",
        "&#x1F600;",
        "&bogus;",
        "&",
        ";",
        "<",
        ">",
        "#",
        "-",
        "--",
        "/",
        " ",
        "  ",
        "\n",
        "\t",
        "word",
        "Text",
        "caf\u{e9}",
        "\u{4e16}",
        "<<",
        ">>",
        "\"",
        "'",
        "=",
        "x",
        "y",
        "</",
        "<!",
        "<!-",
    ];
    let mut html = String::new();
    for _ in 0..rng.below(40) {
        html.push_str(PIECES[rng.below(PIECES.len())]);
    }
    html
}

#[test]
fn pieces_give_the_same_text_as_the_whole() {
    let mut rng = Rng(5);
    let mut mails: Vec<String> = FIXED.iter().map(|s| (*s).to_owned()).collect();
    for _ in 0..300 {
        mails.push(random_mail(&mut rng));
    }
    for html in &mails {
        let whole = new_text(html);
        let chars: Vec<char> = html.chars().collect();
        for _ in 0..4 {
            let mut parser = New::new();
            let mut out = String::new();
            let mut at = 0;
            while at < chars.len() {
                let take = 1 + rng.below(6);
                let end = (at + take).min(chars.len());
                let piece: String = chars[at..end].iter().collect();
                parser.push(&piece, &mut out).unwrap();
                at = end;
            }
            parser.finish(&mut out).unwrap();
            out.truncate(out.trim_end().len());
            assert_eq!(out, whole, "{html:?}");
        }
    }
}

#[test]
fn pieces_read_as_interface_6_read_them() {
    // The way `render` feeds the reader: a piece at a time, then `finish`.
    let mut parser = New::new();
    let mut out = String::new();
    let mut seen = Vec::new();
    for piece in [
        "<p>Hello &am",
        "p; world</p><ul><li>a</li></ul>&#x1F6",
        "00; <sty",
    ] {
        parser.push(piece, &mut out).unwrap();
        seen.push(format!("{piece:?}\t{out:?}"));
    }
    parser.finish(&mut out).unwrap();
    seen.push(format!("finish\t{out:?}"));
    assert_eq!(seen, golden("pieces"));
}

#[test]
fn a_non_ascii_character_in_a_skipped_block_is_no_panic() {
    // Interface 6 cut its watch for the end tag inside a character (and
    // panicked).
    let html = "<style>\u{e9}aaaaaaa</style>visible<script>\u{4e16}\u{4e16}\u{4e16}</script>end";
    assert_eq!(new_text(html), "visibleend");
}

#[test]
fn a_long_comment_ends_at_its_end() {
    let filler = "x".repeat(5000);
    let html = format!("before<!-- {filler} --> after <p>more</p>");
    assert_eq!(new_text(&html), "before after \n\nmore");
    // Interface 6 never found the end and dropped the rest of the mail
    // ("before" was all it gave).
}

#[test]
fn the_text_is_cut_at_the_limit() {
    let mut parser = New::with_limit(10);
    let mut out = String::new();
    parser
        .push("0123456789 more text <p>and more</p>", &mut out)
        .unwrap();
    parser.finish(&mut out).unwrap();
    assert_eq!(out, "0123456789");
    assert!(parser.is_cut());
    // A multi-byte character that would not fit is dropped whole.
    let mut parser = New::with_limit(4);
    let mut out = String::new();
    parser.push("ab\u{4e16}\u{4e16}", &mut out).unwrap();
    assert_eq!(out, "ab");
    assert!(parser.is_cut());
    // The default holds a long mail whole, and cuts a huge one.
    let big = "a ".repeat(MAX_TEXT);
    let text = new_text(&big);
    assert!(text.len() <= MAX_TEXT);
    let mut parser = New::new();
    parser.push(&big, &mut String::new()).unwrap();
    assert!(parser.is_cut());
}

#[test]
fn hostile_text_is_read_without_a_panic() {
    let mut rng = Rng(99);
    let alphabet: Vec<char> =
        "<>&;#/-!= \n\t\"'abcpxXstyleripthead0123456789\u{e9}\u{4e16}\u{1f600}\u{a0}\u{200b}\u{1}"
            .chars()
            .collect();
    for len in [0usize, 1, 2, 9, 100, 1000, 5000] {
        for _ in 0..40 {
            let html: String = (0..len)
                .map(|_| alphabet[rng.below(alphabet.len())])
                .collect();
            let _ = new_text(&html);
        }
    }
    // A tag, an entity and a comment never closed.
    for html in [
        "<".repeat(5000),
        "&".repeat(5000),
        format!("<!--{}", "-".repeat(5000)),
        format!("<a {}", "x".repeat(5000)),
        format!("<style>{}", "\u{e9}".repeat(5000)),
        format!("&{}", "a".repeat(5000)),
    ] {
        let _ = new_text(&html);
    }
}
