//! Feeds as hostile servers might send them: whatever comes, the page
//! made of it is whole, holds no markup of the feed's (only its own
//! headings and paragraphs), at most the articles asked for, and text
//! within its limits.

extern crate std;

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use paperpaw_sdk::{MemoryStorage, Storage};
use proptest::prelude::*;

use super::{HEADLINES, MAX_FIELD, convert};

/// How these tests write a day.
fn day(year: i32, month: u8, day: u8) -> String {
    format!("{day}.{month}.{year}")
}

/// Converts `feed`; the result and the page.
fn page(feed: &[u8], articles: usize, text: usize) -> (super::Converted, String) {
    let storage = MemoryStorage::new();
    storage.write("feed.xml", feed).unwrap();
    let converted = convert(&storage, "feed.xml", "out.html", articles, text, &day).unwrap();
    let data = paperpaw_sdk::read_file(&storage, "out.html", 1 << 24).unwrap();
    (converted, String::from_utf8(data).unwrap())
}

/// The tags in `page`, by name.
fn tags(page: &str) -> Vec<String> {
    page.split('<')
        .skip(1)
        .map(|t| {
            t.split(['>', ' ', '/'])
                .find(|n| !n.is_empty())
                .unwrap_or("")
                .to_ascii_lowercase()
        })
        .collect()
}

fn whole(page: &str) {
    assert!(
        page.starts_with("<!DOCTYPE html>\n<html>\n<head>\n"),
        "{page}"
    );
    assert!(page.ends_with("</body>\n</html>\n"), "{page}");
    for tag in tags(page) {
        assert!(
            [
                "!doctype", "html", "head", "meta", "title", "body", "h1", "h2", "p", "i"
            ]
            .contains(&tag.as_str()),
            "<{tag}> in {page}"
        );
    }
}

fn fragment() -> impl Strategy<Value = String> {
    prop_oneof![
        Just(String::from("<rss><channel>")),
        Just(String::from("<feed xmlns=\"http://www.w3.org/2005/Atom\">")),
        Just(String::from("<item>")),
        Just(String::from("</item>")),
        Just(String::from("<entry>")),
        Just(String::from("</entry>")),
        Just(String::from("<title>")),
        Just(String::from("</title>")),
        Just(String::from("<description>")),
        Just(String::from("</description>")),
        Just(String::from("<content type=\"xhtml\"><div>")),
        Just(String::from("</div></content>")),
        Just(String::from(
            "<content:encoded><![CDATA[<script>alert(1)</script><p>x</p>]]></content:encoded>"
        )),
        Just(String::from("&lt;script&gt;alert(2)&lt;/script&gt;")),
        Just(String::from(
            "&lt;a href=&quot;javascript:x&quot;&gt;link&lt;/a&gt;"
        )),
        Just(String::from(
            "<link rel=\"alternate\" href=\"javascript:alert(3)\"/>"
        )),
        Just(String::from("<pubDate>Tue, 13 Oct 2026</pubDate>")),
        Just(String::from("&#0;&#xD800;&bogus;&amp;")),
        Just(String::from("<![CDATA[")),
        Just(String::from("]]>")),
        Just(String::from("<!--")),
        "[a-zA-Z <>/&;\"'=]{0,30}",
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(200)))]

    #[test]
    fn any_bytes_make_a_whole_page(data in prop::collection::vec(any::<u8>(), 0..4000)) {
        let (converted, page) = page(&data, 10, 2000);
        whole(&page);
        prop_assert!(converted.articles <= 10);
    }

    #[test]
    fn pieces_of_feeds_make_whole_pages_without_their_markup(
        parts in prop::collection::vec(fragment(), 0..120),
        articles in 1usize..8,
        text in 1usize..500,
    ) {
        let (converted, page) = page(parts.concat().as_bytes(), articles, text);
        whole(&page);
        prop_assert!(converted.articles <= articles);
        prop_assert_eq!(page.matches("<h2>").count(), converted.articles);
        prop_assert!(converted.headlines.len() <= HEADLINES.min(converted.articles));
        // At most a field and one more piece of markup (see the ignored
        // test below).
        prop_assert!(converted.title.len() <= MAX_FIELD + 4096 + 4);
        prop_assert!(!converted.title.contains('\n'));
        for p in page.split("<p>").skip(1) {
            let p = p.split("</p>").next().unwrap_or("");
            prop_assert!(p.len() <= 6 * (text + 1024) + MAX_FIELD, "{}", p.len());
        }
    }
}

#[test]
fn a_huge_feed_stops_reading_once_the_articles_are_there() {
    let mut feed = String::from("<rss><channel><title>Many</title>");
    for n in 0..20_000 {
        feed.push_str(&format!(
            "<item><title>N{n}</title><description>d</description></item>"
        ));
    }
    feed.push_str("</channel></rss>");
    let (converted, page) = page(feed.as_bytes(), 3, 100);
    assert_eq!(converted.articles, 3);
    assert_eq!(converted.headlines, ["N0", "N1", "N2"]);
    assert!(page.len() < 1000, "{}", page.len());
}

#[test]
fn deeply_nested_inline_markup_is_flattened() {
    let feed = format!(
        "<feed><title>Deep</title><entry><title>D</title><content type=\"xhtml\">{}words{}</content></entry></feed>",
        "<div><span>".repeat(10_000),
        "</span></div>".repeat(10_000)
    );
    let (converted, page) = page(feed.as_bytes(), 5, 1 << 20);
    assert_eq!(converted.articles, 1);
    assert!(
        page.contains("<p>words</p>"),
        "{}",
        &page[..page.len().min(500)]
    );
    whole(&page);
}

#[test]
fn titles_and_dates_are_cut_at_a_character_boundary() {
    let title = "ä".repeat(5000);
    let feed = format!(
        "<rss><channel><title>{title}</title><item><title>{title}</title><pubDate>{title}</pubDate><description>x</description></item></channel></rss>"
    );
    let (converted, page) = page(feed.as_bytes(), 5, 100);
    assert_eq!(converted.articles, 1);
    assert!(converted.title.chars().all(|c| c == 'ä'));
    assert!(converted.headlines[0].len() <= MAX_FIELD);
    whole(&page);
}

#[test]
fn a_long_channel_title_is_cut_like_other_titles() {
    let feed = format!(
        "<rss><channel><title><![CDATA[{}]]></title><item><title>{}</title><description>x</description></item></channel></rss>",
        "x".repeat(3000),
        "y".repeat(3000)
    );
    let (converted, _) = page(feed.as_bytes(), 5, 100);
    assert!(converted.headlines[0].len() <= MAX_FIELD);
    assert!(
        converted.title.len() <= MAX_FIELD,
        "{}",
        converted.title.len()
    );
}

#[test]
fn storage_that_fails_is_an_error_not_a_panic() {
    let storage = MemoryStorage::new();
    storage
        .write(
            "feed.xml",
            b"<rss><channel><item><title>x</title></item></channel></rss>",
        )
        .unwrap();
    // A page that cannot be written (an invalid path).
    assert!(convert(&storage, "feed.xml", "../out.html", 5, 100, &day).is_err());
    // A feed that is not there.
    assert!(convert(&storage, "missing.xml", "out.html", 5, 100, &day).is_err());
}
