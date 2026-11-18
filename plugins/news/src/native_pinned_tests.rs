//! The native decisions (`native_news`) against what the interface-6 code
//! decided on the same input (`testdata/pinned_text.txt`, written from it
//! before it was removed), and refusing instead of aborting when memory runs
//! short. The read caps are those of interface 6.
use crate::fail_alloc::sweep;
use crate::native_news as native;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::fmt::Write;
use paperpaw_sdk::native::Error;

const URLS: [&str; 6] = [
    "",
    "https://a.example/rss",
    "https://www.tagesschau.de/xml/rss2",
    "http://user:pw@feeds.example.org:8080/a?b=c#d",
    "feeds.example/ü",
    "x",
];

/// What the native code decides on the inputs of the pinned file, in its lines.
fn decided() -> String {
    let mut text = String::new();
    for date in DATES {
        writeln!(text, "date {date:?} {:?}", native::read_date(date)).unwrap();
    }
    for t in ["", "short", "ünï cödé", "0123456789", "a😀b😀c"] {
        for max in 0..14 {
            writeln!(text, "cut {t:?} {max} {:?}", native::cut(t, max).unwrap()).unwrap();
        }
    }
    for t in ["", "  a   b\n\tc  ", "one", " \u{a0} x \u{2003} y "] {
        writeln!(text, "line {t:?} {:?}", native::one_line(t).unwrap()).unwrap();
    }
    for t in ["", "a & b < c > \"d\"", "&amp;", "ünï <b>"] {
        writeln!(text, "escape {t:?} {:?}", native::escape(t).unwrap()).unwrap();
    }
    for n in BLOCKS {
        writeln!(text, "block {n:?} {}", native::is_block(n)).unwrap();
    }
    for markup in MARKUP {
        for max in [0, 10, 100, 10_000] {
            let paragraphs = crate::native_convert::paragraphs(markup, max).unwrap();
            writeln!(text, "paras {markup:?} {max} Some({paragraphs:?})").unwrap();
        }
    }
    for url in URLS {
        writeln!(text, "mark {url:?} {}", native::Mark::of(url).as_str()).unwrap();
    }
    for name in NAMES {
        writeln!(text, "unmark {name:?} {:?}", native::without_mark(name)).unwrap();
    }
    for title in TITLES {
        writeln!(
            text,
            "short {title:?} {:?}",
            native::short_title(title).unwrap()
        )
        .unwrap();
    }
    for (i, t) in FEEDS.iter().enumerate() {
        let list = native::feeds(t).unwrap();
        let shown: Vec<(&str, &str, &str)> = list
            .iter()
            .map(|f| (f.url.as_str(), f.name.as_str(), f.shown()))
            .collect();
        writeln!(
            text,
            "feeds {i} {shown:?} {:?}",
            native::feeds_text(&list).unwrap()
        )
        .unwrap();
    }
    let many: Vec<u8> = (0..60)
        .map(|i| format!("Title {i}\t/b/{i}.html\n"))
        .collect::<String>()
        .into_bytes();
    let old_texts: [Option<&[u8]>; 6] = [
        None,
        Some(b""),
        Some(b"Old\t/b/other.html\nOlder\t/b/mine.html\nOldest\t/b/third.html"),
        Some(b"Bad \xff\t/b/other.html\n\nNo tab line\n"),
        Some(&many),
        Some(&[b'x'; 32 * 1024 + 1]),
    ];
    let titles = [
        vec![],
        vec!["One".to_string(), "Two\twith\ntab".to_string()],
        (0..5).map(|i| format!("Headline {i}")).collect::<Vec<_>>(),
    ];
    for (i, old) in old_texts.iter().enumerate() {
        for (j, new) in titles.iter().enumerate() {
            let kept = native::headlines(new, "/b/mine.html", *old).unwrap();
            writeln!(text, "headlines {i} {j} {kept:?}").unwrap();
        }
    }
    text
}

const DATES: [&str; 17] = [
    "2026-10-24T14:32:44-04:00",
    "2026-10-24",
    "2026-02-30",
    "2026-13-01",
    "26-10-24",
    "Sat, 24 Oct 2026 14:32:44 GMT",
    "24 Oct 2026",
    "24 oct 98",
    "1 JAN 05 00:00",
    "Mon, 5 Feb 70",
    "31 Apr 2026",
    "29 Feb 2024",
    "29 Feb 2025",
    "not a date",
    "",
    "ü ü ü ü",
    "  2026-10-24  ",
];
const BLOCKS: [&str; 24] = [
    "p",
    "div",
    "br",
    "li",
    "h1",
    "h6",
    "h7",
    "blockquote",
    "tr",
    "section",
    "article",
    "ul",
    "ol",
    "pre",
    "figure",
    "figcaption",
    "hr",
    "table",
    "dd",
    "dt",
    "span",
    "a",
    "P",
    "",
];
const MARKUP: [&str; 7] = [
    "",
    "<p>One</p><p>Two &amp; <b>three</b></p>",
    "text <br> more <BR/> end",
    "<script>x</script>a<style>y</style>b<SCRIPT/>c",
    "<ul><li>a</li><li>b</li></ul>",
    "&lt;p&gt;not markup&lt;/p&gt;",
    "<![CDATA[raw <b>]]>",
];
const NAMES: [&str; 9] = [
    "Tagesschau (3fa9c2)",
    "Tagesschau (3FA9C2)",
    "Tagesschau (3fa9c)",
    "Tagesschau (3fa9cz)",
    "(3fa9c2)",
    " (3fa9c2)",
    "A (b) (3fa9c2)",
    "",
    "plain",
];
const TITLES: [&str; 10] = [
    "Site - all the news",
    "Site | news",
    "Si - news",
    "S – long",
    "Site \u{2014} news",
    "Name: the tagline",
    "  Plain title  ",
    "",
    " - x",
    "Ab: cd",
];
const FEEDS: [&str; 5] = [
    "",
    "https://a.example/rss\tA\nhttps://b.example/\n",
    "\n  \n\t\nhttps://a.example\t  Name with spaces  \n",
    "https://a.example/rss\tA\tB\r\nhttps://b.example/\tC\r\n",
    "\u{feff}https://a.example\tA\n",
];

#[test]
fn the_decisions_are_those_of_interface_6() {
    let want = include_str!("../testdata/pinned_text.txt");
    let got = decided();
    for (n, (w, g)) in want.lines().zip(got.lines()).enumerate() {
        assert_eq!(g, w, "line {}", n + 1);
    }
    assert_eq!(got.lines().count(), want.lines().count());
}

#[test]
fn a_feed_with_tabs_is_written_on_one_line() {
    let odd = [native::Feed {
        url: "https://a".into(),
        name: "A\tB\nC".into(),
    }];
    assert_eq!(native::feeds_text(&odd).unwrap(), "https://a\tA B C\n");
}

#[test]
fn the_read_caps_are_those_of_interface_6() {
    assert_eq!(native::FEEDS_CAP, 64 * 1024);
    assert_eq!(native::HEADLINES_CAP, 32 * 1024);
    assert_eq!(native::MAX_FEED_FILE, 4 * 1024 * 1024);
    assert_eq!(
        (native::MAX_TEXT_SMALL, native::MAX_TEXT),
        (8 * 1024, 64 * 1024)
    );
    assert_eq!(native::MAX_FIELD, 400);
    // Over the cap is an error, not an empty list, so nothing is written
    // over what the file holds; at the cap it is read.
    let line = "https://a.example/rss\tA\n";
    let at_cap =
        line.repeat(native::FEEDS_CAP / line.len()) + &"\n".repeat(native::FEEDS_CAP % line.len());
    assert_eq!(at_cap.len(), native::FEEDS_CAP);
    assert_eq!(
        native::feeds(&at_cap).unwrap().len(),
        native::FEEDS_CAP / line.len()
    );
    let over = at_cap + "\n";
    assert_eq!(native::feeds(&over).err(), Some(Error::NoSpace));
}

#[test]
fn the_old_headlines_file_is_read_up_to_its_cap() {
    // A file at the cap is read; one byte over is left out.
    let at = vec![b'y'; native::HEADLINES_CAP];
    let titles = ["T".to_string()];
    let text = native::headlines(&titles, "/b", Some(&at)).unwrap();
    assert_eq!(text.len(), "T\t/b\n".len() + native::HEADLINES_CAP);
    let over = vec![b'y'; native::HEADLINES_CAP + 1];
    assert_eq!(
        native::headlines(&titles, "/b", Some(&over)).unwrap(),
        "T\t/b"
    );
}

#[test]
fn refused_allocations_end_in_no_memory() {
    sweep(|| native::short_title("Site - all the news").map(|t| t.len()));
    sweep(|| native::feeds("https://a.example/rss\tA\nhttps://b.example/\n").map(|l| l.len()));
    let list = [native::Feed {
        url: "https://a".into(),
        name: "A\tB".into(),
    }];
    sweep(|| native::feeds_text(&list).map(|t| t.len()));
    let titles = ["One".to_string(), "Two".to_string()];
    sweep(|| {
        native::headlines(
            &titles,
            "/b/mine.html",
            Some(b"Old\t/b/other.html\nBad \xff\t/b/x"),
        )
        .map(|t| t.len())
    });
    sweep(|| native::cut("ünï cödé", 5).map(|t| t.len()));
    sweep(|| native::one_line("  a   b ").map(|t| t.len()));
    sweep(|| native::escape("a & b < c").map(|t| t.len()));
}
