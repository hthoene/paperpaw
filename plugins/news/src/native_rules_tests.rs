//! The native decisions (`native_news`) against what the interface-6 ones
//! (`lib.rs`, `convert.rs`, removed with their plugin) gave, recorded in
//! `native_golden`: equal on the same input, and refusing instead of aborting
//! when memory runs short. The read caps are those of interface 6.
use crate::fail_alloc::sweep;
use crate::native_convert_tests::fnv;
use crate::native_golden::{
    BLOCKS, CUTS, DATES, ESCAPES, FEEDS, HEADLINES, MARKS, ONE_LINES, SHORT_TITLES, WITHOUT_MARK,
};
use crate::native_news as native;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use paperpaw_sdk::native::Error;

#[test]
fn marks_are_those_of_interface_6() {
    for &(url, want) in MARKS {
        assert_eq!(native::Mark::of(url).as_str(), want, "{url:?}");
    }
    for &(name, want) in WITHOUT_MARK {
        assert_eq!(native::without_mark(name), want, "{name:?}");
    }
}

#[test]
fn titles_are_cut_as_before() {
    for &(title, want) in SHORT_TITLES {
        assert_eq!(native::short_title(title).unwrap(), want, "{title:?}");
    }
}

#[test]
fn feeds_are_read_and_written_as_before() {
    for &(text, rows, written) in FEEDS {
        let new = native::feeds(text).unwrap();
        assert_eq!(new.len(), rows.len(), "{text:?}");
        for (n, &(url, name, shown)) in new.iter().zip(rows) {
            assert_eq!((n.url.as_str(), n.name.as_str()), (url, name));
            assert_eq!(n.shown(), shown);
        }
        assert_eq!(native::feeds_text(&new).unwrap(), written);
    }
    let odd = [native::Feed {
        url: "https://a".into(),
        name: "A\tB\nC".into(),
    }];
    assert_eq!(native::feeds_text(&odd).unwrap(), "https://a\tA B C\n");
}

#[test]
fn feeds_of_version_1_are_written_alike() {
    let v1 = include_str!("../testdata/feeds.txt.v1");
    let list = native::feeds(v1).unwrap();
    assert_eq!(native::feeds_text(&list).unwrap(), v1);
    // Any list of feeds round trips, one line a feed, whatever the names hold.
    let odd = [
        native::Feed {
            url: "https://a.example/rss".into(),
            name: "A\tB\nC".into(),
        },
        native::Feed {
            url: "https://b.example/".into(),
            name: String::new(),
        },
    ];
    let again = native::feeds(&native::feeds_text(&odd).unwrap()).unwrap();
    assert_eq!(again.len(), 2);
    assert_eq!(again[0].url, "https://a.example/rss");
    assert!(!again[0].name.contains(['\t', '\r', '\n']));
    assert_eq!(again[1].shown(), "b.example");
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
fn headlines_are_kept_as_before() {
    let many: Vec<u8> = (0..60)
        .map(|i| format!("Title {i}\t/b/{i}.html\n"))
        .collect::<String>()
        .into_bytes();
    let old_texts: [Option<Vec<u8>>; 6] = [
        None,
        Some(Vec::new()),
        Some(b"Old\t/b/other.html\nOlder\t/b/mine.html\nOldest\t/b/third.html".to_vec()),
        Some(b"Bad \xff\t/b/other.html\n\nNo tab line\n".to_vec()),
        Some(many),
        Some(vec![b'x'; 32 * 1024 + 1]),
    ];
    let titles = [
        vec![],
        vec!["One".to_string(), "Two\twith\ntab".to_string()],
        (0..5).map(|i| format!("Headline {i}")).collect::<Vec<_>>(),
    ];
    for &(old, new, len, hash) in HEADLINES {
        let got =
            native::headlines(&titles[new], "/b/mine.html", old_texts[old].as_deref()).unwrap();
        assert_eq!((got.len(), fnv(got.as_bytes())), (len, hash), "{old} {new}");
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
    for &(date, want) in DATES {
        assert_eq!(native::read_date(date), want, "{date:?}");
    }
    for &(text, max, want) in CUTS {
        assert_eq!(native::cut(text, max).unwrap(), want, "{text:?} {max}");
    }
    for &(text, want) in ONE_LINES {
        assert_eq!(native::one_line(text).unwrap(), want, "{text:?}");
    }
    for &(text, want) in ESCAPES {
        assert_eq!(native::escape(text).unwrap(), want, "{text:?}");
    }
    for &(name, want) in BLOCKS {
        assert_eq!(native::is_block(name), want, "{name:?}");
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
