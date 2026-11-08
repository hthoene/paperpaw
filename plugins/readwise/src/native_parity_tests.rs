//! The native reading of the notes (`native_readwise`) against the
//! interface-6 one (`Parser` of `lib.rs`): equal on the same notes, and
//! refusing instead of aborting when memory runs short.
use crate::fail_alloc::sweep;
use crate::native_readwise as native;
use crate::{Highlight as Old, MAX_TEXT, Parser as OldParser};
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

const NOTES: &str = "# Alice's Adventures\n\nLewis Carroll\n\n## Highlights\n\n\
    > Curiouser and curiouser!\n>\n> 12 %\n\n\
    > Who are you?\n>\n> 40 %\n\nThe caterpillar asks.\nTwice.\n\n\
    ## Bookmarks\n\n- 50 %: Down the rabbit hole\n";

/// Notes of every shape the grammar has, good and bad.
fn corpus() -> Vec<String> {
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

fn old_read(text: &str) -> (OldParser, Vec<Old>) {
    let mut parser = OldParser::default();
    let mut highlights: Vec<Old> = text.lines().filter_map(|l| parser.line(l)).collect();
    highlights.extend(parser.finish());
    (parser, highlights)
}

fn new_read(
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

#[test]
fn notes_are_read_as_the_interface_6_parser_reads_them() {
    for text in corpus() {
        let (old, old_highlights) = old_read(&text);
        let (new, new_highlights) = new_read(&text).unwrap();
        assert_eq!(old.title(), new.title());
        assert_eq!(old.author(), new.author());
        assert_eq!(old_highlights.len(), new_highlights.len(), "{text:.60?}");
        for (o, n) in old_highlights.iter().zip(&new_highlights) {
            assert_eq!((&o.text, o.percent, &o.note), (&n.text, n.percent, &n.note));
            // And the JSON the same, byte for byte.
            assert_eq!(old.json(o), new.json(n).unwrap());
        }
    }
}

#[test]
fn the_json_escapes_as_the_interface_6_json_does() {
    let mut old = OldParser::default();
    let mut new = native::Parser::default();
    for line in ["# \"Quoted\" \\ title", "An\tauthor\u{1}"] {
        old.line(line);
        new.line(line).unwrap();
    }
    for text in [
        "plain",
        "q\"uo\\te",
        "line\nbreak\r\ttab",
        "\u{0}\u{1f}\u{7f}ü😀",
        "",
    ] {
        let o = Old {
            text: text.into(),
            percent: 4_294_967_295,
            note: text.into(),
        };
        let n = native::Highlight {
            text: text.into(),
            percent: 4_294_967_295,
            note: text.into(),
        };
        assert_eq!(old.json(&o), new.json(&n).unwrap(), "{text:?}");
    }
    let zero = native::Highlight::default();
    assert_eq!(old.json(&Old::default()), new.json(&zero).unwrap());
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
    let (parser, highlights) = new_read(NOTES).unwrap();
    sweep(|| parser.json(&highlights[1]).map(|json| json.len()));
}
