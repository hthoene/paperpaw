//! The native converter (`native_convert`) against the pages the interface-6
//! converter made (`testdata/pinned_pages.txt`, a hash of each page): the same
//! page and the same result for the same feed, at any size of the pieces it
//! comes in, and refusing instead of aborting when memory runs short.
use crate::fail_alloc::sweep;
use crate::native_convert::{Converter, HEAD, TAIL, paragraphs};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use paperpaw_sdk::native::Error;

/// The date as the plugin's own text would be, kept fallibly: the sweeps
/// must see a refusal here as one, not as an abort.
pub(crate) fn day_text(year: i32, month: u8, day: u8) -> Result<String, Error> {
    use core::fmt::Write;
    let mut out = String::new();
    out.try_reserve_exact(24).map_err(|_| Error::NoMemory)?;
    write!(out, "{day}.{month}.{year}").map_err(|_| Error::NoMemory)?;
    Ok(out)
}

/// FNV-1a of a page, as the pinned file keeps it.
pub(crate) fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100_0000_01b3)
    })
}

/// What the pinned file says of feed `feed` read with these limits.
pub(crate) fn pinned(feed: usize, max_articles: usize, max_text: usize) -> &'static str {
    let key = format!("{feed} {max_articles} {max_text}: ");
    include_str!("../testdata/pinned_pages.txt")
        .lines()
        .find_map(|line| line.strip_prefix(key.as_str()))
        .expect("a pinned case")
}

/// The same line from what the native code made.
pub(crate) fn pinned_line(
    title: &str,
    articles: usize,
    headlines: &[String],
    page: &[u8],
) -> String {
    format!(
        "{title:?} {articles} {headlines:?} page={}:{:016x}",
        page.len(),
        fnv(page)
    )
}

fn new(
    feed: &[u8],
    piece: usize,
    max_articles: usize,
    max_text: usize,
) -> Result<(crate::native_convert::Converted, Vec<u8>), Error> {
    let date = day_text;
    let mut converter = Converter::new(max_articles, max_text, &date);
    let mut page = Vec::new();
    for chunk in feed.chunks(piece) {
        converter.feed(chunk)?;
        page.extend_from_slice(converter.taken());
        converter.clear();
        if converter.is_done() {
            break;
        }
    }
    let converted = converter.finish()?;
    page.extend_from_slice(converter.taken());
    Ok((converted, page))
}

pub(crate) fn feeds() -> Vec<String> {
    let items: String = (0..30)
        .map(|n| {
            format!(
                "<item><title>N{n} &amp; <b>x</b></title><description>{}</description></item>",
                "word ".repeat(500)
            )
        })
        .collect();
    let paragraph = "<p>Wörter &amp; <em>Zeichen</em>, ]] und ]> dazwischen.</p>\n";
    vec![
        r#"<?xml version="1.0"?>
<rss version="2.0" xmlns:content="http://purl.org/rss/1.0/modules/content/">
<channel><title>Rabbit &amp; Co</title><link>https://example.org/</link>
<item><title>First &lt;b&gt;news&lt;/b&gt;</title><pubDate>Tue, 13 Oct 2026 10:00:00 GMT</pubDate>
<description>&lt;p&gt;Down the &lt;a href="x"&gt;hole&lt;/a&gt;.&lt;/p&gt;&lt;p&gt;Second one.&lt;/p&gt;</description></item>
<item><title>Second</title><description>Short</description>
<content:encoded><![CDATA[<p>The full <em>text</em>.</p><script>alert(1)</script><p>More &amp; more.</p>]]></content:encoded></item>
</channel></rss>"#
            .into(),
        r#"<feed xmlns="http://www.w3.org/2005/Atom"><title>Atom news</title>
<entry><title type="html">One</title><updated>2026-10-14T08:00:00Z</updated>
<link rel="alternate" href="https://example.org/1?a=1&amp;b=2"/>
<content type="xhtml"><div xmlns="http://www.w3.org/1999/xhtml"><p>Inline &amp; fine.</p><p>Two<br/>lines</p><img src="x"/></div></content></entry>
<entry><title>Two</title><summary>Only a summary</summary></entry>
<entry><link rel="enclosure" href="https://example.org/f.mp3"/><title>Three</title><published>last Tuesday</published></entry>
<entry><title></title></entry></feed>"#
            .into(),
        r#"<rdf:RDF xmlns="http://purl.org/rss/1.0/" xmlns:dc="http://purl.org/dc/elements/1.1/"><channel><title>RSS 1</title></channel>
<item><title>Old style</title><dc:date>2026-03-01</dc:date><description>Text &lt;style&gt;p{}&lt;/style&gt; kept</description></item></rdf:RDF>"#
            .into(),
        format!("<rss><channel><title>Many</title>{items}</channel></rss>"),
        format!(
            "<rss xmlns:content=\"x\"><channel><title>T</title><item><title>Long</title><description>Summary only</description><content:encoded><![CDATA[{}]]></content:encoded></item></channel></rss>",
            paragraph.repeat(280)
        ),
        format!(
            "<rss><channel><title>{}</title><item><title>{}</title><description>d</description></item></channel></rss>",
            "T\u{e4}".repeat(400),
            "\u{1f4da}".repeat(150)
        ),
        "<rss><channel><item><title>No channel title</title></item></channel></rss>".into(),
        "<rss><channel><title>Empty</title></channel></rss>".into(),
        "<html><body>Sign in to the Wi-Fi</body></html>".into(),
        "<rss><channel><title>Cut off</title><item><title>Half".into(),
        "<rss><channel><title>Inline</title><item><title>A</title><description>&lt;p&gt;x&lt;/p&gt;&lt;P&gt;Y&lt;/P&gt;&lt;SCRIPT&gt;no&lt;/SCRIPT&gt;&lt;span&gt;z&lt;/span&gt;</description></item></channel></rss>".into(),
        "<rss><channel><title>Deep</title><item><title>A</title><content>".to_string()
            + &"<div>".repeat(200)
            + "text"
            + &"</div>".repeat(200)
            + "</content></item></channel></rss>",
        String::new(),
    ]
}

#[test]
fn feeds_become_the_pages_of_interface_6() {
    for (index, feed) in feeds().iter().enumerate() {
        for (max_articles, max_text) in [
            (10, 10_000),
            (5, 100),
            (1, 8 * 1024),
            (200, 64 * 1024),
            (0, 100),
        ] {
            let want = pinned(index, max_articles, max_text);
            for piece in [1, 5, 64, 512, 2048] {
                let (got, page) = new(feed.as_bytes(), piece, max_articles, max_text).unwrap();
                assert_eq!(
                    pinned_line(&got.title, got.articles, &got.headlines, &page),
                    want,
                    "{piece} {max_articles} {max_text} {feed:.60}"
                );
            }
        }
    }
}

#[test]
fn a_page_begins_and_ends_as_ever() {
    let (_, page) = new(b"<rss><channel><title>T</title><item><title>A</title><description>x</description></item></channel></rss>", 64, 10, 1000).unwrap();
    let page = String::from_utf8(page).unwrap();
    assert!(page.starts_with(HEAD));
    assert!(page.ends_with(TAIL));
}

#[test]
fn a_spent_converter_stays_spent() {
    let feed = &feeds()[0];
    let date = day_text;
    let mut failed = false;
    sweep(|| {
        let mut converter = Converter::new(10, 10_000, &date);
        for piece in feed.as_bytes().chunks(50) {
            if let Err(error) = converter.feed(piece) {
                failed = true;
                assert_eq!(converter.feed(b" "), Err(error));
                assert_eq!(converter.finish().err(), Some(error));
                return Err(error);
            }
        }
        converter.finish().map(|c| c.articles)
    });
    assert!(failed);
}

#[test]
fn refused_allocations_end_in_no_memory() {
    let feeds = feeds();
    let date = day_text;
    for feed in [&feeds[0], &feeds[1], &feeds[2]] {
        sweep(|| {
            let mut converter = Converter::new(10, 10_000, &date);
            for piece in feed.as_bytes().chunks(300) {
                converter.feed(piece)?;
                converter.clear();
            }
            converter.finish().map(|c| c.articles)
        });
    }
    sweep(|| paragraphs("<p>One</p><p>Two &amp; <b>three</b></p>", 100).map(|p| p.len()));
}
