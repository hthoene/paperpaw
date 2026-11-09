//! Browsing a catalog with the real kernel: its feed is loaded and shown,
//! an entry leads to a further feed and *Back* to the one before, a book
//! is downloaded into the catalog's folder in the library (or stopped
//! with *Back*, leaving nothing), and what goes wrong is said. The catalog's password goes to the catalog only, and a
//! list that cannot be read is never saved over.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::{Device, Shared, authorized, catalogs_path};
use paperpaw_core::HttpCall;
use paperpaw_plugin_opds::OPEN;
use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::{Instant, Params, Storage};

/// The catalog the list starts with.
const START: &str = "https://www.gutenberg.org/ebooks.opds/";

const FEED: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
<title>Project Gutenberg</title>
<link rel="search" type="application/atom+xml" href="/ebooks/search.opds/?query={searchTerms}"/>
<link rel="next" type="application/atom+xml;profile=opds-catalog" href="/ebooks.opds/?page=2"/>
<entry>
  <title>Popular books</title>
  <link rel="subsection" type="application/atom+xml;profile=opds-catalog" href="/ebooks/popular.opds/"/>
</entry>
<entry>
  <title>Alice's Adventures in Wonderland</title>
  <author><name>Lewis Carroll</name></author>
  <link rel="http://opds-spec.org/acquisition" type="application/epub+zip" href="/ebooks/11.epub"/>
</entry>
</feed>"#;

const POPULAR: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
<title>Popular books</title>
<entry>
  <title>Die Verwandlung</title>
  <author><name>Franz Kafka</name></author>
  <link rel="http://opds-spec.org/acquisition" type="application/epub+zip" href="/ebooks/5200.epub"/>
</entry>
</feed>"#;

/// A catalog of two books: one on its own server, one elsewhere.
const TWO_BOOKS: &str = r#"<feed xmlns="http://www.w3.org/2005/Atom">
<title>Mine</title>
<entry>
  <title>Own</title>
  <link rel="http://opds-spec.org/acquisition" type="application/epub+zip" href="/own.epub"/>
</entry>
<entry>
  <title>Elsewhere</title>
  <link rel="http://opds-spec.org/acquisition" type="application/epub+zip" href="https://elsewhere.example/b.epub"/>
</entry>
</feed>"#;

/// The first catalog opened: its feed is asked for.
fn browse(storage: Shared) -> (Device, HttpCall) {
    let mut device = Device::start(storage);
    device.run(OPEN);
    device.run(actions::CONFIRM);
    let call = device.request();
    (device, call)
}

/// The catalog that starts the list opened.
fn browse_start() -> (Device, HttpCall) {
    let (device, call) = browse(Shared::default());
    assert_eq!(call.url, START);
    (device, call)
}

/// A storage with one catalog, which needs an account.
fn with_account(url: &str) -> Shared {
    let storage = Shared::default();
    let line = format!("Mine\t{url}\tme\t\t\t\t\n");
    storage.write(&catalogs_path(), line.as_bytes()).unwrap();
    storage
}

#[test]
fn a_feed_is_shown_with_search_and_more() {
    let (mut browsing, call) = browse_start();
    browsing.answer(&call, 200, FEED);
    let shown = browsing.shown();
    let at = |text: &str| shown.iter().position(|l| l.contains(text));
    let (search, popular, alice) = (at("Search"), at("Popular books"), at("Alice"));
    assert!(
        search.is_some() && popular.is_some() && alice.is_some(),
        "{shown:?}"
    );
    assert!(search < popular && popular < alice, "{shown:?}");
    assert!(at("Lewis Carroll").is_some(), "{shown:?}");
    assert!(at("More").is_some(), "{shown:?}");
}

#[test]
fn entries_lead_on_and_back() {
    let (mut browsing, call) = browse_start();
    browsing.answer(&call, 200, FEED);
    // Past *Search*, to *Popular books*.
    browsing.run(actions::DOWN);
    browsing.run(actions::CONFIRM);
    let call = browsing.request();
    // Relative to the feed it came in.
    assert_eq!(call.url, "https://www.gutenberg.org/ebooks/popular.opds/");
    browsing.answer(&call, 200, POPULAR);
    assert!(
        browsing
            .shown()
            .iter()
            .any(|l| l.contains("Die Verwandlung"))
    );
    // Back: the first feed again.
    browsing.run(actions::BACK);
    let call = browsing.request();
    assert_eq!(call.url, START);
    browsing.answer(&call, 200, FEED);
    assert!(browsing.shown().iter().any(|l| l.contains("Popular books")));
}

#[test]
fn a_book_is_downloaded_into_the_library() {
    let (mut browsing, call) = browse_start();
    browsing.answer(&call, 200, FEED);
    // Search, Popular books, then the book.
    browsing.run(actions::DOWN);
    browsing.run(actions::DOWN);
    browsing.run(actions::CONFIRM);
    let call = browsing.request();
    assert_eq!(call.url, "https://www.gutenberg.org/ebooks/11.epub");
    assert!(call.save_to.is_some());
    browsing.answer(&call, 200, "PK the book");
    // In the catalog's folder in the library (here, without the files
    // plugin, the card's top), named after the book.
    let mut saved = Vec::new();
    browsing
        .storage
        .list("Project Gutenberg", &mut |entry| {
            saved.push(entry.name.to_string());
        })
        .unwrap();
    assert_eq!(saved.len(), 1, "{saved:?}");
    assert!(saved[0].ends_with(".epub"), "{saved:?}");
    let book = format!("Project Gutenberg/{}", saved[0]);
    assert_eq!(browsing.storage.get(&book).unwrap(), b"PK the book");
}

/// While a book downloads, the line below the feed says how far it came:
/// its percent and a bar where the size is known, the bytes so far where
/// not; only that line is drawn again, with a fast refresh, at most every
/// 10 % or 5 seconds.
#[test]
fn a_download_says_how_far_it_came() {
    let (mut browsing, call) = browse_start();
    browsing.answer(&call, 200, FEED);
    browsing.run(actions::DOWN);
    browsing.run(actions::DOWN);
    browsing.run(actions::CONFIRM);
    let call = browsing.request();
    assert!(call.progress);
    let _ = browsing.shown();
    let effects = browsing
        .kernel
        .http_progress(Instant(10_000), call.id, 250, Some(1000));
    let (area, refresh) = effects.refresh().expect("the line drawn again");
    assert_eq!(refresh, paperpaw_sdk::RefreshMode::Fast);
    // The line at the foot only, not the feed.
    assert!(area.y > 600 && area.height < 200, "{area:?}");
    let shown = browsing.shown();
    assert!(shown.iter().any(|l| l.contains("25 %")), "{shown:?}");
    // Not again for a little more a moment later: each refresh holds
    // the device, and the download with it; 10 % further, or 5 s later.
    let effects = browsing
        .kernel
        .http_progress(Instant(12_100), call.id, 300, Some(1000));
    assert!(effects.refresh().is_none(), "{:?}", effects.refresh());
    let effects = browsing
        .kernel
        .http_progress(Instant(14_200), call.id, 350, Some(1000));
    assert!(effects.refresh().is_some(), "10 % further");
    let effects = browsing
        .kernel
        .http_progress(Instant(19_300), call.id, 400, Some(1000));
    assert!(effects.refresh().is_some(), "5 s later");
    // Without a size: the bytes so far.
    let (mut browsing, call) = browse_start();
    browsing.answer(&call, 200, FEED);
    browsing.run(actions::DOWN);
    browsing.run(actions::DOWN);
    browsing.run(actions::CONFIRM);
    let call = browsing.request();
    let _ = browsing
        .kernel
        .http_progress(Instant(10_000), call.id, 3 << 20, None);
    let shown = browsing.shown();
    assert!(
        shown.iter().any(|l| l.contains("3.0 MB so far")),
        "{shown:?}"
    );
    // Done: saved as before, and said so.
    browsing.answer(&call, 200, "PK the book");
    let shown = browsing.shown();
    assert!(!shown.iter().any(|l| l.contains("so far")), "{shown:?}");
}

/// *Back* while a book is downloading stops the download: the feed stays
/// and says so, and an answer that still comes (the transfer went on)
/// leaves no file, in the library nor of the plugin's own.
#[test]
fn back_stops_a_download_and_leaves_nothing() {
    let (mut browsing, call) = browse_start();
    browsing.answer(&call, 200, FEED);
    browsing.run(actions::DOWN);
    browsing.run(actions::DOWN);
    browsing.run(actions::CONFIRM);
    let call = browsing.request();
    let shown = browsing.shown();
    assert!(
        shown.iter().any(|l| l.contains("Back stops it")),
        "{shown:?}"
    );
    browsing.run(actions::BACK);
    let shown = browsing.shown();
    assert!(
        shown.iter().any(|l| l.contains("Download stopped")),
        "{shown:?}"
    );
    assert!(shown.iter().any(|l| l.contains("Alice")), "{shown:?}");
    browsing.answer(&call, 200, "PK the book");
    assert!(!browsing.storage.exists("Project Gutenberg"));
    let mut left = Vec::new();
    let tmp = format!("{}/tmp", paperpaw_sdk::Env::plugin_dir("opds"));
    let _ = browsing.storage.list(&tmp, &mut |entry| {
        left.push(entry.name.to_string());
    });
    assert!(left.is_empty(), "{left:?}");
    // Back again leaves the catalog, as without a download.
    browsing.run(actions::BACK);
    assert!(!browsing.shown().iter().any(|l| l.contains("Alice")));
}

/// A page the server answers with instead of the book is not saved as
/// one.
#[test]
fn an_answer_that_is_no_book_is_not_saved() {
    let (mut browsing, call) = browse_start();
    browsing.answer(&call, 200, FEED);
    browsing.run(actions::DOWN);
    browsing.run(actions::DOWN);
    browsing.run(actions::CONFIRM);
    let call = browsing.request();
    browsing.answer(&call, 200, "<html><body>Please sign in</body></html>");
    let mut saved = 0;
    let _listed = browsing
        .storage
        .list("Project Gutenberg", &mut |_| saved += 1);
    assert_eq!(saved, 0);
    assert!(!browsing.storage.exists(call.save_to.as_deref().unwrap()));
    let shown = browsing.shown().join(" | ");
    assert!(shown.contains("no book"), "{shown}");
}

#[test]
fn what_goes_wrong_is_said() {
    let (mut browsing, call) = browse_start();
    browsing.answer(&call, 401, "");
    let shown = browsing.shown().join(" | ");
    assert!(
        shown.contains("password") || shown.contains("allowed"),
        "{shown}"
    );

    let (mut browsing, call) = browse_start();
    browsing.answer(&call, 200, "<html><body>Not a feed</body></html>");
    let shown = browsing.shown().join(" | ");
    assert!(shown.to_lowercase().contains("catalog"), "{shown}");

    let (mut browsing, call) = browse_start();
    browsing.answer(&call, 0, "no network");
    let shown = browsing.shown().join(" | ");
    assert!(shown.contains("gutenberg.org"), "{shown}");
}

#[test]
fn credentials_go_only_to_the_catalog() {
    let storage = with_account("https://books.example/opds");
    let (mut browsing, call) = browse(storage);
    assert_eq!(call.url, "https://books.example/opds");
    assert!(authorized(&call));
    browsing.answer(&call, 200, TWO_BOOKS);
    browsing.run(actions::CONFIRM);
    let own = browsing.request();
    assert_eq!(own.url, "https://books.example/own.epub");
    assert!(authorized(&own));
    browsing.run(actions::DOWN);
    browsing.run(actions::CONFIRM);
    let elsewhere = browsing.request();
    assert_eq!(elsewhere.url, "https://elsewhere.example/b.epub");
    assert!(!authorized(&elsewhere), "{elsewhere:?}");
}

#[test]
fn two_downloads_at_once_are_both_saved() {
    let (mut browsing, call) = browse(with_account("https://books.example/opds"));
    browsing.answer(&call, 200, TWO_BOOKS);
    // Both tapped before either has come.
    browsing.run(actions::CONFIRM);
    browsing.run(actions::DOWN);
    browsing.run(actions::CONFIRM);
    let calls = browsing.requests();
    assert_eq!(calls.len(), 2, "{calls:?}");
    let (own, elsewhere) = (&calls[0], &calls[1]);
    assert_ne!(own.save_to, elsewhere.save_to, "each to a file of its own");
    browsing.answer(elsewhere, 200, "PK elsewhere");
    browsing.answer(own, 200, "PK own");
    let book = |name: &str| browsing.storage.get(&format!("Mine/{name}"));
    assert_eq!(book("Own.epub").as_deref(), Some(&b"PK own"[..]));
    assert_eq!(
        book("Elsewhere.epub").as_deref(),
        Some(&b"PK elsewhere"[..])
    );
    for call in &calls {
        assert!(!browsing.storage.exists(call.save_to.as_deref().unwrap()));
    }
}

#[test]
fn a_list_that_cannot_be_read_is_not_saved_over() {
    let storage = with_account("https://books.example/opds");
    let kept = storage.get(&catalogs_path()).unwrap();
    let mut device = Device::start(storage.clone());
    device.open("opds.edit", Params::new().with("id", "0"));
    assert!(device.shown().iter().any(|l| l == "Mine"));
    // The card fails, then the catalog is marked for syncing.
    storage.break_file(&catalogs_path());
    for _ in 0..6 {
        device.run(actions::DOWN);
    }
    device.run(actions::CONFIRM);
    assert_eq!(storage.get(&catalogs_path()), Some(kept.clone()));
    // The list shows nothing it could save over, and says why.
    device.run(actions::BACK);
    device.run(OPEN);
    let shown = device.shown().join(" | ");
    assert!(!shown.contains("Project Gutenberg"), "{shown}");
    assert!(shown.contains("could not be read"), "{shown}");
    assert_eq!(storage.get(&catalogs_path()), Some(kept));
}

#[test]
fn passwords_over_http_are_warned_of() {
    let warned = |url: &str| {
        let mut device = Device::start(with_account(url));
        device.open("opds.edit", Params::new().with("id", "0"));
        device.shown().join(" | ").contains("unencrypted")
    };
    assert!(warned("http://books.example/opds"));
    assert!(!warned("https://books.example/opds"));
}
