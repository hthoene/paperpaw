//! The reading of the notes and the JSON of a highlight (`native_readwise`) on
//! the notes of every shape the grammar has, as a transcript kept in
//! `testdata/parser.v1`: the title and author, and for each highlight its place
//! and its JSON (a long text by its length and a hash). The transcript was
//! written while the interface-6 parser was still there and equal to it on
//! every line; a change here is a change of what Readwise is sent, so it is
//! reviewed (`PAPERPAW_BLESS=1 cargo test` writes the file anew).
use crate::fail_alloc::sweep;
use crate::native_readwise as native;
use crate::native_readwise::MAX_TEXT;
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

const TRANSCRIPT: &str = include_str!("../testdata/parser.v1");

pub(crate) const NOTES: &str = "# Alice's Adventures\n\nLewis Carroll\n\n## Highlights\n\n\
    > Curiouser and curiouser!\n>\n> 12 %\n\n\
    > Who are you?\n>\n> 40 %\n\nThe caterpillar asks.\nTwice.\n\n\
    ## Bookmarks\n\n- 50 %: Down the rabbit hole\n";

/// Notes of every shape the grammar has, good and bad.
pub(crate) fn corpus() -> Vec<String> {
    let mut out: Vec<String> = vec![
        NOTES.into(),
        NOTES[..NOTES.find("## Bookmarks").unwrap()].into(),
        "# Title\n\n## Bookmarks\n\n- 5 %: x\n".into(),
        "".into(),
        "# Only a title".into(),
        "Author first\n# Title\nAuthor\nSecond line\n## H\n> a\n> 1 %\n".into(),
        "# T\n# T2\nA\n## H\n> a\n> b\n> 3%\nnote\n\nmore note\n> c\n> 4 %\n".into(),
        "# T\n## H\n>\n> 5 %\n> x\n".into(),
        "# T\n## H\n> a\nnote only\n> b\n## Later\n> c\n".into(),
        "# T\n## H\nnote before any quote\n> a\n> 9 %\n".into(),
        "# ü\nÄ\n## H\n> ö ä\n> 7 %\nÜ\n".into(),
        "# T\n## H\n> x\n> -3 %\n> 99999999999 %\n> 4 %\n".into(),
        "# T\r\n\r\nA\r\n## H\r\n> a\r\n> 1 %\r\n".into(),
    ];
    let long = "\u{e4}".repeat(MAX_TEXT);
    out.push(format!(
        "# T\n\n## Highlights\n\n> {long}\n> more\n>\n> 5 %\n\n{long}\n"
    ));
    out.push(format!("# {}\n\n{}\n", "t".repeat(4096), "a".repeat(4096)));
    out.push(format!(
        "# {}\n\n{}\n",
        "\u{e4}".repeat(2000),
        "\u{e4}".repeat(2000)
    ));
    let ends = "x".repeat(MAX_TEXT - 3);
    out.push(format!(
        "# T\n## H\n> {ends}\n> \u{e4}\u{e4}\n> yy\n> 1 %\n{ends}\nzz\n"
    ));
    out.push(format!("# T\n## H\n> {ends}\n> yyy\n> yy\n> 1 %\n"));
    out
}

fn short(text: &str) -> String {
    if text.len() <= 120 {
        return format!("{text:?}");
    }
    let hash = text.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3)
    });
    format!("<{} bytes {hash:016x}>", text.len())
}

pub(crate) fn read(
    text: &str,
) -> Result<(native::Parser, Vec<native::Highlight>), paperpaw_sdk::native::Error> {
    let mut parser = native::Parser::default();
    let mut highlights = Vec::new();
    for line in text.lines() {
        highlights.extend(parser.line(line)?);
    }
    highlights.extend(parser.finish());
    Ok((parser, highlights))
}

fn transcript() -> String {
    let mut out = String::new();
    for (i, text) in corpus().iter().enumerate() {
        let (parser, highlights) = read(text).unwrap();
        out.push_str(&format!(
            "== {i} title {} author {} highlights {}\n",
            short(parser.title()),
            short(parser.author()),
            highlights.len()
        ));
        for h in &highlights {
            out.push_str(&format!(
                "{} {}\n",
                h.percent,
                short(&parser.json(h).unwrap())
            ));
        }
    }
    let mut parser = native::Parser::default();
    for line in ["# \"Quoted\" \\ title", "An\tauthor\u{1}"] {
        parser.line(line).unwrap();
    }
    for text in [
        "plain",
        "q\"uo\\te",
        "line\nbreak\r\ttab",
        "\u{0}\u{1f}\u{7f}ü😀",
        "",
    ] {
        let h = native::Highlight {
            text: text.into(),
            percent: 4_294_967_295,
            note: text.into(),
        };
        out.push_str(&format!("escape {}\n", short(&parser.json(&h).unwrap())));
    }
    out.push_str(&format!(
        "empty {}\n",
        short(&parser.json(&native::Highlight::default()).unwrap())
    ));
    out
}

#[test]
fn the_notes_are_read_and_written_as_interface_6_did() {
    let got = transcript();
    if std::env::var_os("PAPERPAW_BLESS").is_some() {
        std::fs::write(
            concat!(env!("CARGO_MANIFEST_DIR"), "/testdata/parser.v1"),
            &got,
        )
        .unwrap();
        return;
    }
    assert_eq!(got.lines().count(), TRANSCRIPT.lines().count());
    for (i, (got, want)) in got.lines().zip(TRANSCRIPT.lines()).enumerate() {
        assert_eq!(got, want, "line {i}");
    }
}

#[test]
fn refused_allocations_end_in_no_memory() {
    // Counted, not collected: a test's own list would abort.
    sweep(|| {
        let mut parser = native::Parser::default();
        let mut count = 0;
        for line in NOTES.lines() {
            count += usize::from(parser.line(line)?.is_some());
        }
        Ok(count + usize::from(parser.finish().is_some()))
    });
    let (parser, highlights) = read(NOTES).unwrap();
    sweep(|| parser.json(&highlights[1]).map(|json| json.len()));
}

#[test]
fn the_json_of_a_long_text_is_valid_and_bounded() {
    let long = "\u{e4}\"".repeat(native::MAX_TEXT);
    let text = format!("# T\n\n## Highlights\n\n> {long}\n>\n> 5 %\n");
    let (parser, highlights) = read(&text).unwrap();
    assert_eq!(highlights.len(), 1);
    let json = parser.json(&highlights[0]).unwrap();
    assert!(json.starts_with("{\"text\":\""));
    assert!(json.ends_with("\"location\":5,\"location_type\":\"order\"}"));
}

#[test]
fn any_notes_are_read_without_panic() {
    let mut texts: Vec<String> = vec![String::new(), "\u{0}".into(), ">".into()];
    for text in corpus() {
        for cut in 0..=text.len().min(400) {
            if text.is_char_boundary(cut) {
                texts.push(text[..cut].into());
            }
        }
    }
    for text in texts {
        let _ = read(&text).unwrap();
    }
}
