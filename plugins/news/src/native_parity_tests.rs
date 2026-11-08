//! The native decisions (`native_news`) against the interface-6 ones
//! (`lib.rs`, `convert.rs`): equal on the same input, and refusing instead of
//! aborting when memory runs short. The read caps are those of interface 6.
use crate::fail_alloc::sweep;
use crate::native_news as native;
use crate::{convert, data, headlines_file, load, mark, save, short_title, without_mark};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use paperpaw_sdk::native::Error;
use paperpaw_sdk::{MemoryStorage, Storage};

const URLS: [&str; 6] = [
    "",
    "https://a.example/rss",
    "https://www.tagesschau.de/xml/rss2",
    "http://user:pw@feeds.example.org:8080/a?b=c#d",
    "feeds.example/ü",
    "x",
];

#[test]
fn marks_are_those_of_interface_6() {
    for url in URLS {
        assert_eq!(native::Mark::of(url).as_str(), mark(url), "{url:?}");
    }
    for name in [
        "Tagesschau (3fa9c2)",
        "Tagesschau (3FA9C2)",
        "Tagesschau (3fa9c)",
        "Tagesschau (3fa9cz)",
        "(3fa9c2)",
        " (3fa9c2)",
        "A (b) (3fa9c2)",
        "",
        "plain",
    ] {
        assert_eq!(native::without_mark(name), without_mark(name), "{name:?}");
    }
}

#[test]
fn titles_are_cut_as_before() {
    for title in [
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
    ] {
        assert_eq!(
            native::short_title(title).unwrap(),
            short_title(title),
            "{title:?}"
        );
    }
}

#[test]
fn feeds_are_read_and_written_as_before() {
    for text in [
        "",
        "https://a.example/rss\tA\nhttps://b.example/\n",
        "\n  \n\t\nhttps://a.example\t  Name with spaces  \n",
        "https://a.example/rss\tA\tB\r\nhttps://b.example/\tC\r\n",
        "\u{feff}https://a.example\tA\n",
    ] {
        let storage = MemoryStorage::new().with(&data("feeds.txt"), text);
        let old = load(&storage).unwrap();
        let new = native::feeds(text).unwrap();
        assert_eq!(old.len(), new.len(), "{text:?}");
        for (o, n) in old.iter().zip(&new) {
            assert_eq!((&o.url, &o.name), (&n.url, &n.name));
            assert_eq!(n.shown(), o.shown(),);
        }
        let kept: Vec<native::Feed> = old
            .iter()
            .map(|f| native::Feed {
                url: f.url.clone(),
                name: f.name.clone(),
            })
            .collect();
        let storage = MemoryStorage::new();
        save(&storage, &old);
        let written = String::from_utf8(storage.get(&data("feeds.txt")).unwrap()).unwrap();
        assert_eq!(native::feeds_text(&kept).unwrap(), written);
    }
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
    assert_eq!(native::MAX_FEED_FILE, crate::MAX_FEED_FILE);
    assert_eq!(
        (native::MAX_TEXT_SMALL, native::MAX_TEXT),
        (crate::MAX_TEXT_SMALL, crate::MAX_TEXT)
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
    assert!(load(&MemoryStorage::new().with(&data("feeds.txt"), over.as_str())).is_err());
}

fn old_headlines(titles: &[String], book: &str, old: Option<&[u8]>) -> Vec<u8> {
    let storage = MemoryStorage::new();
    if let Some(old) = old {
        storage.write(&headlines_file(), old).unwrap();
    }
    let env_less = storage;
    // `keep_headlines` takes an `Env`; the same steps without one.
    let mut lines: Vec<String> = titles
        .iter()
        .map(|t| format!("{}\t{book}", t.replace(['\t', '\n'], " ")))
        .collect();
    if let Ok(old) = paperpaw_sdk::read_file(&env_less, &headlines_file(), 32 * 1024) {
        for line in String::from_utf8_lossy(&old).lines() {
            if !line.ends_with(&format!("\t{book}")) && lines.len() < 40 {
                lines.push(line.to_string());
            }
        }
    }
    lines.join("\n").into_bytes()
}

#[test]
fn headlines_are_kept_as_before() {
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
    for old in old_texts {
        for new in &titles {
            let want = old_headlines(new, "/b/mine.html", old);
            let got = native::headlines(new, "/b/mine.html", old).unwrap();
            assert_eq!(got.as_bytes(), want, "{:?}", old.map(<[u8]>::len));
        }
    }
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
fn dates_and_text_are_those_of_interface_6() {
    for date in [
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
    ] {
        assert_eq!(
            native::read_date(date),
            convert::read_date(date),
            "{date:?}"
        );
    }
    for text in ["", "short", "ünï cödé", "0123456789", "a😀b😀c"] {
        for max in 0..14 {
            assert_eq!(
                native::cut(text, max).unwrap(),
                convert::cut(text, max),
                "{text:?} {max}"
            );
        }
    }
    for text in ["", "  a   b\n\tc  ", "one", " \u{a0} x \u{2003} y "] {
        assert_eq!(native::one_line(text).unwrap(), convert::one_line(text));
    }
    for text in ["", "a & b < c > \"d\"", "&amp;", "ünï <b>"] {
        assert_eq!(
            native::escape(text).unwrap(),
            convert::escape(text),
            "{text:?}"
        );
    }
    for name in [
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
    ] {
        assert_eq!(native::is_block(name), convert::is_block(name), "{name:?}");
    }
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
