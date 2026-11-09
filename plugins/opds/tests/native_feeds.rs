//! The readers of catalogs: OPDS 1 (Atom), OPDS 2 (JSON), WebDAV listings and
//! the search templates, on the feeds of real servers (Project Gutenberg,
//! Calibre-Web, Kavita, Komga), on feeds as hostile servers might send them
//! (any bytes, pieces of feeds, entity bombs, endless lists and addresses) and
//! at their limits by device class: never a panic, never more than a page, links
//! made absolute against where the feed came from.
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "fixtures and assertions"
)]
use paperpaw_plugin_opds::native_dav as dav;
use paperpaw_plugin_opds::native_json as json;
use paperpaw_plugin_opds::native_model::{
    self as model, Book, CHUNK, Feed, Limits, MAX_AUTHORS, MAX_BOOKS, MAX_ENTRIES, MAX_MIME,
    MAX_PAGE, MAX_TEXT, MAX_URL, Search, Source,
};
use paperpaw_plugin_opds::native_search as search;
use paperpaw_sdk::native::Error;
use proptest::prelude::*;

const BASE: &str = "https://cat.example/opds/root.xml";

/// The bytes as a source, in the pieces a file is read in.
struct Bytes<'a>(&'a [u8]);

impl Source for Bytes<'_> {
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<usize, Error> {
        let from = usize::try_from(offset)
            .unwrap_or(usize::MAX)
            .min(self.0.len());
        let n = buf.len().min(self.0.len() - from);
        buf[..n].copy_from_slice(&self.0[from..from + n]);
        Ok(n)
    }
}

fn atom(text: &str, base: &str) -> Feed {
    model::read(&mut Bytes(text.as_bytes()), base, Limits::default()).unwrap()
}

fn opds2(text: &str, base: &str) -> Feed {
    json::read(&mut Bytes(text.as_bytes()), base, Limits::default()).unwrap()
}

fn webdav(text: &str, base: &str) -> Feed {
    dav::read(&mut Bytes(text.as_bytes()), base, Limits::default()).unwrap()
}

const NAVIGATION: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<feed xmlns="http://www.w3.org/2005/Atom" xmlns:opensearch="http://a9.com/-/spec/opensearch/1.1/">
<id>http://www.gutenberg.org/ebooks.opds/</id>
<title>Project  Gutenberg</title>
<author><name>Project Gutenberg</name></author>
<link rel="search" type="application/opensearchdescription+xml" href="https://www.gutenberg.org/catalog/osd-books.xml"/>
<link rel="next" type="application/atom+xml;profile=opds-catalog" href="/ebooks/search.opds/?sort_order=downloads&amp;start_index=26"/>
<entry>
<id>https://www.gutenberg.org/ebooks/1342.opds</id>
<title>Pride and Prejudice</title>
<content type="text">Jane Austen</content>
<link type="application/atom+xml;profile=opds-catalog" rel="subsection" href="/ebooks/1342.opds"/>
<link type="image/png" rel="http://opds-spec.org/image/thumbnail" href="data:image/png;base64,iVBORw0KGgo"/>
</entry>
<entry><title>Nothing to open</title><id>x</id></entry>
</feed>"#;

const BOOKS: &str = r#"<feed xmlns="http://www.w3.org/2005/Atom">
<title>Pride and Prejudice by Jane Austen</title>
<entry>
<title>Pride and Prejudice</title>
<content type="xhtml"><div xmlns="http://www.w3.org/1999/xhtml"><p>Long text</p></div></content>
<id>urn:gutenberg:1342:2</id>
<author><name>Austen, Jane</name></author>
<link type="application/epub+zip" rel="http://opds-spec.org/acquisition" title="EPUB (no images)" href="https://www.gutenberg.org/ebooks/1342.epub.noimages"/>
<link type="application/x-mobipocket-ebook" rel="http://opds-spec.org/acquisition" href="https://www.gutenberg.org/ebooks/1342.kindle.noimages"/>
<link type="application/atom+xml;profile=opds-catalog" rel="related" href="/ebooks/author/68.opds"/>
</entry>
<entry>
<title><![CDATA[Fish & Chips]]></title>
<id>urn:b</id>
<link type="application/x-mobipocket-ebook" rel="http://opds-spec.org/acquisition" href="b.mobi"/>
<link type="application/pdf" rel="http://opds-spec.org/acquisition/buy" href="b.pdf"/>
</entry>
</feed>"#;

#[test]
fn navigation_feeds() {
    let feed = atom(NAVIGATION, "https://www.gutenberg.org/ebooks.opds/");
    assert_eq!(feed.title, "Project Gutenberg");
    assert_eq!(
        feed.next.as_deref(),
        Some("https://www.gutenberg.org/ebooks/search.opds/?sort_order=downloads&start_index=26")
    );
    assert_eq!(
        feed.search,
        Some(Search::Description(
            "https://www.gutenberg.org/catalog/osd-books.xml".to_string()
        ))
    );
    // Entries leading nowhere are left out.
    assert_eq!(feed.entries.len(), 1);
    let entry = &feed.entries[0];
    assert_eq!(entry.title, "Pride and Prejudice");
    assert_eq!(entry.summary, "Jane Austen");
    assert_eq!(entry.authors, "");
    assert_eq!(
        entry.feed.as_deref(),
        Some("https://www.gutenberg.org/ebooks/1342.opds")
    );
    assert!(entry.books.is_empty());
    assert_eq!(entry.thumbnail, None, "a data address is no cover");
    assert!(!feed.memory);
}

#[test]
fn acquisition_feeds() {
    let feed = atom(BOOKS, "https://www.gutenberg.org/ebooks/1342.opds");
    assert_eq!(feed.entries.len(), 2);
    let entry = &feed.entries[0];
    assert_eq!(entry.id, "urn:gutenberg:1342:2");
    assert_eq!(entry.authors, "Austen, Jane");
    assert_eq!(entry.summary, "", "only plain text");
    assert_eq!(entry.feed, None, "related feeds are not where it leads");
    assert_eq!(entry.books.len(), 2);
    let (book, ext) = entry.readable_book(|ext| ext == "epub").unwrap();
    assert_eq!(ext, "epub");
    assert_eq!(
        book.url,
        "https://www.gutenberg.org/ebooks/1342.epub.noimages"
    );
    assert_eq!(book.title, "EPUB (no images)");
    let other = &feed.entries[1];
    assert_eq!(other.title, "Fish & Chips");
    assert_eq!(other.books.len(), 1, "books to buy are left out");
    assert_eq!(
        other.books[0].url,
        "https://www.gutenberg.org/ebooks/b.mobi"
    );
    assert!(other.readable_book(|ext| ext == "epub").is_none());
}

#[test]
fn feeds_of_the_servers_in_use_are_read() {
    let corpus = [
        (
            include_str!("../testdata/corpus/gutenberg-nav.xml"),
            "https://www.gutenberg.org/ebooks.opds/",
        ),
        (
            include_str!("../testdata/corpus/gutenberg-books.xml"),
            "https://www.gutenberg.org/ebooks/search.opds/",
        ),
        (
            include_str!("../testdata/corpus/calibre-web.xml"),
            "http://192.168.1.2:8083/opds",
        ),
        (
            include_str!("../testdata/corpus/kavita.xml"),
            "https://kavita.example/api/opds/KEY",
        ),
        (
            include_str!("../testdata/corpus/komga.xml"),
            "https://komga.example/opds/v1.2/catalog",
        ),
    ];
    for (text, base) in corpus {
        let feed = atom(text, base);
        assert!(!feed.memory, "{base}");
        assert!(!feed.title.is_empty() || !feed.entries.is_empty(), "{base}");
        for entry in &feed.entries {
            for url in entry.feed.iter().chain(entry.books.iter().map(|b| &b.url)) {
                assert!(url.starts_with("http"), "{url}");
            }
        }
    }
    // Entities in titles, links and addresses are decoded.
    let feed = atom(
        include_str!("../testdata/corpus/entities.xml"),
        "https://cat.example/",
    );
    assert_eq!(
        feed.next.as_deref(),
        Some("https://cat.example/n?a=1&b=2&c=3")
    );
    let entry = &feed.entries[0];
    assert_eq!(entry.title, "\"Quoted\" 'title'");
    assert_eq!(entry.authors, "O'Brien & Sons");
    assert_eq!(entry.books[0].title, "T & U");
    assert_eq!(entry.books[0].url, "https://cat.example/d?id=1&fmt=epub");
    assert_eq!(
        entry.thumbnail.as_deref(),
        Some("https://cat.example/t?a=1&b=2")
    );
    // Nothing at all is no feed, and not an error either.
    let empty = atom("", "https://cat.example/");
    assert!(empty.entries.is_empty() && empty.title.is_empty() && !empty.memory);
    let odd = atom(
        include_str!("../testdata/corpus/odd.xml"),
        "https://cat.example/",
    );
    assert!(!odd.memory);
}

#[test]
fn long_lists_are_cut() {
    let mut text = String::from("<feed><title>Many</title>");
    for n in 0..MAX_ENTRIES + 5 {
        text.push_str(&format!(
            "<entry><title>{n}</title><link rel=\"subsection\" type=\"application/atom+xml\" href=\"/{n}\"/></entry>"
        ));
    }
    text.push_str("</feed>");
    let feed = atom(&text, "https://a.org/");
    assert_eq!(feed.entries.len(), MAX_ENTRIES);
    assert_eq!(feed.entries[3].feed.as_deref(), Some("https://a.org/3"));
    // A small device keeps a page of fewer entries and less text.
    let small = model::read(
        &mut Bytes(text.as_bytes()),
        "https://a.org/",
        Limits::of_class(true),
    )
    .unwrap();
    assert!(small.entries.len() < MAX_ENTRIES);
    assert_eq!(small.entries.len(), Limits::of_class(true).entries);
    assert!(Limits::of_class(true).page < MAX_PAGE);
}

#[test]
fn hostile_feeds_are_kept_small() {
    let mut text = String::from("<feed><title>Big</title><entry><title>One</title>");
    for n in 0..1000 {
        text.push_str(&format!(
            "<author><name>A{n}</name></author><link rel=\"http://opds-spec.org/acquisition\" type=\"application/epub+zip\" href=\"/{n}.epub\"/>"
        ));
    }
    text.push_str("</entry></feed>");
    let feed = atom(&text, "https://a.org/");
    let entry = &feed.entries[0];
    assert_eq!(entry.books.len(), MAX_BOOKS);
    assert_eq!(entry.authors.split(" & ").count(), MAX_AUTHORS);
}

#[test]
fn a_huge_title_is_cut() {
    let text = format!(
        "<feed><entry><title>{}</title><link rel=\"subsection\" type=\"application/atom+xml\" href=\"/x\"/></entry></feed>",
        "word ".repeat(50_000)
    );
    let feed = atom(&text, BASE);
    assert_eq!(feed.entries.len(), 1);
    assert!(feed.entries[0].title.ends_with('…'));
    assert_eq!(feed.entries[0].title.chars().count(), MAX_TEXT + 1);
}

fn acquisition(url: &str, mime: &str) -> String {
    format!(
        "<link rel=\"http://opds-spec.org/acquisition\" type=\"{mime}\" href=\"{url}\" title=\"T\"/>"
    )
}

#[test]
fn a_long_address_or_type_is_not_kept() {
    let long_url = format!("/{}.epub", "a".repeat(MAX_URL));
    let long_mime = format!("application/{}", "x".repeat(MAX_MIME));
    let feed = format!(
        "<feed><entry><title>T</title>{}{}{}{}</entry></feed>",
        acquisition(&long_url, "application/epub+zip"),
        acquisition("/b.epub", &long_mime),
        acquisition("/c.epub", "application/epub+zip"),
        format_args!("<link rel=\"next\" href=\"{long_url}\"/>"),
    );
    let read = atom(&feed, BASE);
    let urls: Vec<&str> = read.entries[0]
        .books
        .iter()
        .map(|b| b.url.as_str())
        .collect();
    assert_eq!(urls, ["https://cat.example/c.epub"]);
    assert_eq!(read.next, None);
}

#[test]
fn a_page_of_full_entries_is_cut_at_the_page_budget() {
    // 100 entries of 16 files, each file with the longest address and type.
    let url = format!("/{}", "u".repeat(MAX_URL - 60));
    let mime = format!("application/{}", "m".repeat(MAX_MIME - 16));
    let mut feed = String::from("<feed>");
    for n in 0..MAX_ENTRIES {
        feed.push_str(&format!("<entry><title>{n}</title>"));
        for f in 0..MAX_BOOKS {
            feed.push_str(&acquisition(&format!("{url}{f}"), &mime));
        }
        feed.push_str("</entry>");
    }
    feed.push_str("</feed>");
    let read = atom(&feed, BASE);
    assert!(read.memory, "the cut is said");
    assert!(!read.entries.is_empty() && read.entries.len() < MAX_ENTRIES);
    let held: usize = read
        .entries
        .iter()
        .flat_map(|e| &e.books)
        .map(|b| b.mime.len() + b.url.len() + b.title.len())
        .sum();
    assert!(held <= MAX_PAGE, "{held} bytes");
}

/// A source that fails after `good` bytes.
struct Failing<'a> {
    data: &'a [u8],
    good: usize,
}

impl Source for Failing<'_> {
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<usize, Error> {
        let from = usize::try_from(offset).unwrap_or(usize::MAX);
        if from >= self.good {
            return Err(Error::Io);
        }
        let end = (from + buf.len()).min(self.good).min(self.data.len());
        let n = end.saturating_sub(from);
        buf[..n].copy_from_slice(&self.data[from..from + n]);
        Ok(n)
    }
}

#[test]
fn a_failed_read_is_an_error_not_a_short_feed() {
    let feed = format!(
        "<feed><title>Cat</title><entry><title>A</title>{}</entry></feed>",
        acquisition("/a.epub", "application/epub+zip")
    );
    for good in [0, 10, CHUNK, feed.len() - 1] {
        let good = good.min(feed.len() - 1);
        let mut source = Failing {
            data: feed.as_bytes(),
            good,
        };
        // Past `good` bytes the source fails; it does not give 0 (the end).
        let read = model::read(&mut source, BASE, Limits::default());
        assert_eq!(read, Err(Error::Io), "failing after {good}");
        let mut source = Failing {
            data: feed.as_bytes(),
            good,
        };
        assert_eq!(
            dav::read(&mut source, BASE, Limits::default()),
            Err(Error::Io)
        );
        let mut source = Failing {
            data: feed.as_bytes(),
            good,
        };
        assert_eq!(
            json::read(&mut source, BASE, Limits::default()),
            Err(Error::Io)
        );
    }
}

#[test]
fn file_types_are_told_by_type_then_by_name() {
    let kind = |mime: &str, url: &str| {
        Book {
            mime: mime.into(),
            url: url.into(),
            title: String::new(),
        }
        .extension()
    };
    assert_eq!(kind("application/epub+zip", "/x"), Some("epub"));
    assert_eq!(kind("application/epub+zip; charset=x", "/x"), Some("epub"));
    assert_eq!(
        kind("application/octet-stream", "/dir/Book.EPUB?dl=1"),
        Some("epub")
    );
    assert_eq!(kind("", "/a/b.htm#x"), Some("htm"));
    assert_eq!(kind("", "/a/b.html"), Some("html"));
    assert_eq!(kind("", "/a/epub"), None);
    assert_eq!(kind("", "/a/.epub"), Some("epub"));
    assert_eq!(kind("x/y", "/é.txt"), Some("txt"));
    assert_eq!(kind("application/fb2+zip", ""), Some("fb2.zip"));
    assert_eq!(kind("", ""), None);
    assert_eq!(kind("", "/ünï.cbz"), Some("cbz"));
}

const FEED2: &str = r#"{
  "metadata": {"title": "Koméga 📚 library", "numberOfItems": 2},
  "links": [
    {"rel": "self", "href": "/opds/v2/catalog", "type": "application/opds+json"},
    {"rel": ["next"], "href": "?page=2", "type": "application/opds+json"},
    {"rel": "search", "href": "/opds/v2/search{?query}", "type": "application/opds+json", "templated": true}
  ],
  "navigation": [
    {"href": "/opds/v2/series", "title": "Series", "type": "application/opds+json", "rel": "subsection"}
  ],
  "publications": [
    {
      "metadata": {
        "title": "Alice's Adventures",
        "identifier": "urn:uuid:1",
        "author": [{"name": "Lewis Carroll"}, "Someone \"Else\""],
        "description": "<p>Down the <b>rabbit</b> hole.</p>"
      },
      "links": [
        {"rel": "http://opds-spec.org/acquisition", "href": "/books/1/file", "type": "application/epub+zip", "title": "EPUB"}
      ],
      "images": [{"href": "/books/1/thumb", "type": "image/jpeg"}]
    }
  ],
  "groups": [
    {"metadata": {"title": "New"}, "publications": [
      {"metadata": {"title": "Grouped", "author": "A. Writer"},
       "links": [{"rel": ["http://opds-spec.org/acquisition/open-access"], "href": "https://x.org/g.epub", "type": "application/epub+zip"}]}
    ]}
  ]
}"#;

#[test]
fn opds_2_feeds_are_read() {
    assert!(json::is_json(&mut Bytes(FEED2.as_bytes())).unwrap());
    let feed = opds2(FEED2, "https://komga.example/opds/v2/catalog");
    assert_eq!(feed.title, "Koméga 📚 library");
    assert_eq!(
        feed.next.as_deref(),
        Some("https://komga.example/opds/v2/catalog?page=2")
    );
    assert_eq!(
        feed.search,
        Some(Search::Template(
            "https://komga.example/opds/v2/search?query={searchTerms}".to_string()
        ))
    );
    assert_eq!(feed.entries.len(), 3);
    let series = &feed.entries[0];
    assert_eq!(series.title, "Series");
    assert_eq!(
        series.feed.as_deref(),
        Some("https://komga.example/opds/v2/series")
    );
    let alice = &feed.entries[1];
    assert_eq!(alice.authors, "Lewis Carroll & Someone \"Else\"");
    assert_eq!(alice.summary, "Down the rabbit hole.");
    assert_eq!(alice.books[0].url, "https://komga.example/books/1/file");
    assert_eq!(alice.books[0].extension(), Some("epub"));
    assert_eq!(
        alice.thumbnail.as_deref(),
        Some("https://komga.example/books/1/thumb")
    );
    let grouped = &feed.entries[2];
    assert_eq!(grouped.authors, "A. Writer");
    assert_eq!(grouped.books[0].url, "https://x.org/g.epub");
    assert!(!feed.memory);
}

#[test]
fn opds_2_hostile_feeds_are_kept_small() {
    let link = r#"{"rel": ["http://opds-spec.org/acquisition"], "href": "/b.epub", "type": "application/epub+zip"}"#;
    let links = vec![link; 1000].join(",");
    let rels = vec![r#""x""#; 1000].join(",") + r#","next""#;
    let authors = vec![r#""A""#; 1000].join(",");
    let text = format!(
        r#"{{"links": [{{"rel": [{rels}], "href": "/next"}}],
        "publications": [{{"metadata": {{"title": "One", "author": [{authors}]}}, "links": [{links}]}}]}}"#
    );
    let feed = opds2(&text, "https://a.org/");
    let entry = &feed.entries[0];
    assert_eq!(entry.books.len(), MAX_BOOKS);
    assert_eq!(entry.authors.split(" & ").count(), MAX_AUTHORS);
    // A link's relations past the limit are not read.
    assert!(feed.next.is_none());
    // A long title is cut as in OPDS 1, and a long address is not kept.
    let long = format!(
        r#"{{"publications": [{{"metadata": {{"title": "{}"}}, "links": [{{"rel": "http://opds-spec.org/acquisition", "href": "/{}.epub", "type": "application/epub+zip"}}]}}]}}"#,
        "word ".repeat(2000),
        "a".repeat(MAX_URL)
    );
    let feed = opds2(&long, "https://a.org/");
    assert_eq!(feed.entries[0].title.chars().count(), MAX_TEXT + 1);
    assert!(feed.entries[0].books.is_empty());
    // A hundred thousand entries keep a page.
    let many: Vec<String> = (0..2000)
        .map(|n| format!(r#"{{"metadata": {{"title": "{n}"}}, "links": [{{"rel": "http://opds-spec.org/acquisition", "href": "/{n}.epub"}}]}}"#))
        .collect();
    let feed = opds2(
        &format!(r#"{{"publications": [{}]}}"#, many.join(",")),
        "https://a.org/",
    );
    assert_eq!(feed.entries.len(), MAX_ENTRIES);
}

#[test]
fn atom_is_no_json() {
    assert!(!json::is_json(&mut Bytes(b"<?xml version=\"1.0\"?><feed/>")).unwrap());
    assert!(!json::is_json(&mut Bytes(b"")).unwrap());
    assert!(json::is_json(&mut Bytes(b"\xef\xbb\xbf \n {\"a\":1}")).unwrap());
}

const LISTING: &str = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:">
<d:response><d:href>/remote.php/dav/files/me/Books/</d:href><d:propstat><d:prop><d:displayname>Books</d:displayname><d:resourcetype><d:collection/></d:resourcetype></d:prop></d:propstat></d:response>
<d:response><d:href>/remote.php/dav/files/me/Books/Die%20Verwandlung.epub</d:href><d:propstat><d:prop><d:displayname>Die Verwandlung.epub</d:displayname><d:resourcetype/><d:getcontenttype>application/octet-stream</d:getcontenttype></d:prop></d:propstat></d:response>
<d:response><d:href>/remote.php/dav/files/me/Books/Krimis/</d:href><d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop></d:propstat></d:response>
</d:multistatus>"#;

#[test]
fn webdav_folders() {
    let feed = webdav(
        LISTING,
        "https://cloud.example.org/remote.php/dav/files/me/Books/",
    );
    assert_eq!(feed.title, "Books");
    assert_eq!(feed.entries.len(), 2, "not the folder itself");
    assert_eq!(feed.entries[0].title, "Krimis", "folders first");
    assert_eq!(
        feed.entries[0].feed.as_deref(),
        Some("https://cloud.example.org/remote.php/dav/files/me/Books/Krimis/")
    );
    let book = &feed.entries[1];
    assert_eq!(book.title, "Die Verwandlung");
    assert_eq!(
        book.readable_book(|ext| ext == "epub").map(|(_, e)| e),
        Some("epub")
    );
}

#[test]
fn webdav_entries_with_hostile_names_stay_titles() {
    let listing = r#"<d:multistatus xmlns:d="DAV:">
<d:response><d:href>/dav/Books/</d:href><d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop></d:propstat></d:response>
<d:response><d:href>/dav/Books/..%2F..%2Fescape.epub</d:href></d:response>
<d:response><d:href></d:href></d:response>
<d:response><d:href>https://elsewhere.example/x.epub</d:href></d:response>
<d:response><d:href>/dav/Books/Sub/</d:href><d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop></d:propstat></d:response>
</d:multistatus>"#;
    let feed = webdav(listing, "https://cloud.example/dav/Books/");
    let titles: Vec<&str> = feed.entries.iter().map(|e| e.title.as_str()).collect();
    assert_eq!(titles, ["Sub", "escape", "x"], "{feed:?}");
    assert_eq!(
        feed.entries[1].books[0].url,
        "https://cloud.example/dav/Books/..%2F..%2Fescape.epub"
    );
    // A WebDAV address that does not end leads nowhere.
    let endless = "a".repeat(100_000);
    let listing = format!(
        "<d:multistatus xmlns:d=\"DAV:\">\
         <d:response><d:href>/dav/{endless}.epub</d:href></d:response>\
         <d:response><d:href>/dav/b.epub</d:href></d:response>\
         </d:multistatus>"
    );
    let feed = webdav(&listing, "https://a.org/dav/");
    assert_eq!(feed.entries.len(), 1);
    assert_eq!(feed.entries[0].title, "b");
}

#[test]
fn searches_by_description_and_by_template() {
    let description = r#"<OpenSearchDescription xmlns="http://a9.com/-/spec/opensearch/1.1/">
<Url type="text/html" template="http://www.gutenberg.org/ebooks/search/?query={searchTerms}"/>
<Url type="application/atom+xml" template="http://m.gutenberg.org/ebooks/search.opds/?query={searchTerms}"/>
</OpenSearchDescription>"#;
    let template = search::template(
        &mut Bytes(description.as_bytes()),
        "https://www.gutenberg.org/",
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        template,
        "http://m.gutenberg.org/ebooks/search.opds/?query={searchTerms}"
    );
    assert_eq!(
        search::url(&template, "Jane Austen").unwrap(),
        "http://m.gutenberg.org/ebooks/search.opds/?query=Jane+Austen"
    );
    assert_eq!(
        search::url("https://a.org/s?q={searchTerms}&p={startPage?}", "x").unwrap(),
        "https://a.org/s?q=x&p="
    );
    // Words are encoded: they bring no syntax of their own.
    assert_eq!(
        search::url("https://a.org/s?q={searchTerms}", "a&b=c#d é").unwrap(),
        "https://a.org/s?q=a%26b%3Dc%23d+%C3%A9"
    );
    // A description without an OPDS address, or an address that is relative.
    let none = r#"<OpenSearchDescription><Url type="text/html" template="/x?q={searchTerms}"/></OpenSearchDescription>"#;
    assert_eq!(
        search::template(&mut Bytes(none.as_bytes()), "https://a.org/").unwrap(),
        None
    );
    let relative = r#"<OpenSearchDescription><Url type="application/atom+xml;profile=opds-catalog" template="/s?q={searchTerms}"/></OpenSearchDescription>"#;
    assert_eq!(
        search::template(&mut Bytes(relative.as_bytes()), "https://a.org/x/d.xml")
            .unwrap()
            .as_deref(),
        Some("https://a.org/s?q={searchTerms}")
    );
    // Words that would make an address too long are not searched for.
    assert_eq!(
        search::url("https://a.org/?q={searchTerms}", &"é".repeat(MAX_URL)),
        Err(Error::NoSpace)
    );
}

/// Pieces of Atom feeds, put together at random.
fn fragment() -> impl Strategy<Value = String> {
    prop_oneof![
        Just(String::from("<feed xmlns=\"http://www.w3.org/2005/Atom\">")),
        Just(String::from("</feed>")),
        Just(String::from("<entry>")),
        Just(String::from("</entry>")),
        Just(String::from("<title>")),
        Just(String::from("</title>")),
        Just(String::from("<author><name>")),
        Just(String::from("</name></author>")),
        Just(String::from("<content type=\"text\">")),
        Just(String::from("</content>")),
        Just(String::from("<![CDATA[x]]>")),
        Just(String::from("&amp;&#0;&#xFFFFFFFF;&bogus;&")),
        Just(String::from(
            "<link rel=\"subsection\" type=\"application/atom+xml\" href=\"../../x\"/>"
        )),
        Just(String::from(
            "<link rel=\"http://opds-spec.org/acquisition\" type=\"application/epub+zip\" href=\"javascript:alert(1)\"/>"
        )),
        Just(String::from(
            "<link rel=\"http://opds-spec.org/acquisition\" type=\"application/epub+zip\" href=\"/b.epub\"/>"
        )),
        Just(String::from("<link rel=\"next\" href=\"?page=2\"/>")),
        Just(String::from(
            "<link rel=\"search\" href=\"/s?q={searchTerms}\"/>"
        )),
        Just(String::from("<link href=\"\"/>")),
        Just(String::from("<!DOCTYPE x [<!ENTITY a \"aaaa\">]>")),
        Just(String::from("<div><div><div>")),
        "[a-z <>/\"=&;]{0,20}",
    ]
}

fn bounded(feed: &Feed) {
    assert!(feed.entries.len() <= MAX_ENTRIES);
    for e in &feed.entries {
        assert!(e.title.chars().count() <= MAX_TEXT + 1, "{}", e.title.len());
        assert!(e.summary.chars().count() <= MAX_TEXT + 1);
        assert!(e.feed.is_some() || !e.books.is_empty() || !e.title.is_empty());
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(200)))]

    #[test]
    fn any_bytes_read_as_a_feed_without_panic(data in prop::collection::vec(any::<u8>(), 0..3000)) {
        bounded(&model::read(&mut Bytes(&data), BASE, Limits::default()).unwrap());
        bounded(&dav::read(&mut Bytes(&data), BASE, Limits::default()).unwrap());
        bounded(&json::read(&mut Bytes(&data), BASE, Limits::default()).unwrap());
        let _ = search::template(&mut Bytes(&data), BASE).unwrap();
        let _ = json::is_json(&mut Bytes(&data)).unwrap();
    }

    #[test]
    fn pieces_of_feeds_read_without_panic(parts in prop::collection::vec(fragment(), 0..80)) {
        let text = parts.concat();
        let feed = atom(&text, BASE);
        bounded(&feed);
        // Links within the catalog are made absolute against it.
        for e in &feed.entries {
            let urls = e.feed.iter().chain(e.books.iter().map(|b| &b.url));
            for url in urls {
                prop_assert!(
                    url.starts_with("https://cat.example/") || url.starts_with("javascript:"),
                    "{}", url
                );
            }
        }
    }

    #[test]
    fn search_addresses_are_made_of_any_template(template in ".{0,60}", terms in ".{0,30}") {
        if let Ok(url) = search::url(&template, &terms) {
            // The terms never bring their own syntax into the address.
            prop_assert!(!url.contains("{searchTerms}"), "{}", url);
        }
    }

    #[test]
    fn links_are_joined_like_a_browser_does(
        path in "(/[a-z.]{0,5}){0,4}",
        href in "(\\.\\./|\\./|/|\\?|#|//)?[a-z./?#&=]{0,20}",
    ) {
        let base = format!("https://h.example{path}");
        let joined = model::join_url(&base, &href).unwrap();
        prop_assert!(joined.starts_with("https://") || joined.contains("://") || joined == href);
    }

    #[test]
    fn texts_are_cleaned_to_one_spaced_short_line(text in "(\\PC|\\s){0,700}") {
        let cleaned = model::clean(&text).unwrap();
        prop_assert!(cleaned.chars().count() <= MAX_TEXT + 1);
        prop_assert!(!cleaned.contains("  ") && !cleaned.starts_with(' '));
        prop_assert!(!cleaned.chars().any(|c| c.is_whitespace() && c != ' '));
    }
}
