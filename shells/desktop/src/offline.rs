//! A small world without a network: answers to the requests of
//! PaperPaw's own plugins (the plugin library, fonts to download, the
//! OPDS catalog it comes with, KOReader's progress sync, the weather,
//! Wikipedia, a news feed), from the files in `fixtures/network`, so that
//! the screens that show what a server sent can be drawn offline and
//! alike on every computer (`paperpaw-emu --network fixtures`, the
//! snapshot tests and `paperpaw-dev`). A request it has no answer for
//! fails as refused and is kept ([`unanswered`]), so a tool can tell a
//! screen that waited for data from one with no way in.

#![cfg_attr(
    target_arch = "wasm32",
    allow(
        dead_code,
        unused_macros,
        reason = "the browser's emulator has no answers (see `ANSWERS`)"
    )
)]

use std::cell::RefCell;

use paperpaw_core::sdk::HttpError;
use paperpaw_core::sdk::native::FINGERPRINT;
use paperpaw_core::{HttpCall, HttpResponse};

use crate::http::Route;

/// An answer: the method and address it answers (ending in `*`, every
/// one that starts so; the first that fits is taken), its status,
/// content type and body.
struct Answer {
    method: &'static str,
    url: &'static str,
    status: u16,
    content_type: &'static str,
    body: &'static [u8],
}

const JSON: &str = "application/json";
const PNG: &str = "image/png";
const TEXT: &str = "text/plain";
const ATOM: &str = "application/atom+xml;profile=opds-catalog";
const RSS: &str = "application/rss+xml";
const EPUB: &str = "application/epub+zip";

/// An answer from a file in `fixtures/network`: to a `GET` with status
/// 200, or as said.
macro_rules! answer {
    ($url:expr, $type:expr, $file:literal) => {
        answer!("GET", $url, 200, $type, $file)
    };
    ($method:literal, $url:expr, $status:literal, $type:expr, $file:literal) => {
        Answer {
            method: $method,
            url: $url,
            status: $status,
            content_type: $type,
            body: include_bytes!(concat!("../fixtures/network/", $file)),
        }
    };
}

/// A sample book of the emulator's library, to download.
macro_rules! book {
    ($url:expr, $file:literal) => {
        Answer {
            method: "GET",
            url: $url,
            status: 200,
            content_type: EPUB,
            body: include_bytes!(concat!("../../../samples/library/books/", $file)),
        }
    };
}

/// The plugin library's address (the store's `OFFICIAL_LIST` without its
/// file).
#[cfg(test)]
const LIBRARY: &str = "https://paperpaw.org/api/v1/plugins";

/// None in the browser's emulator: they would add to its download.
#[cfg(target_arch = "wasm32")]
const ANSWERS: &[Answer] = &[];

#[cfg(not(target_arch = "wasm32"))]
const ANSWERS: &[Answer] = &[
    // The plugin library: the list, a page a plugin, their files and a
    // plugin's screenshots. A data plugin with a long summary and
    // description, screenshots and a source; a code plugin (its builds
    // for this emulator's code, see `ABI`); a short one, a long name.
    answer!(
        "https://paperpaw.org/api/v1/plugins.json?*",
        JSON,
        "store/plugins.json"
    ),
    answer!(
        "https://paperpaw.org/api/v1/plugins/reading-aids.json?*",
        JSON,
        "store/reading-aids.json"
    ),
    answer!(
        "https://paperpaw.org/api/v1/plugins/reading-aids/1.2.0/plugin.ini",
        TEXT,
        "store/reading-aids.ini"
    ),
    answer!(
        "https://paperpaw.org/api/v1/plugins/shots/reading-aids-1.png",
        PNG,
        "store/reading-aids-1.png"
    ),
    answer!(
        "https://paperpaw.org/api/v1/plugins/shots/reading-aids-2.png",
        PNG,
        "store/reading-aids-2.png"
    ),
    answer!(
        "https://paperpaw.org/api/v1/plugins/word-clock.json?*",
        JSON,
        "store/word-clock.json"
    ),
    answer!(
        "https://paperpaw.org/api/v1/plugins/word-clock/1.0.0/plugin.ini",
        TEXT,
        "store/word-clock.ini"
    ),
    answer!(
        "https://paperpaw.org/api/v1/plugins/airy-theme.json?*",
        JSON,
        "store/airy-theme.json"
    ),
    answer!(
        "https://paperpaw.org/api/v1/plugins/airy-theme/1.0.0/plugin.ini",
        TEXT,
        "store/airy-theme.ini"
    ),
    answer!(
        "https://paperpaw.org/api/v1/plugins/long-notes.json?*",
        JSON,
        "store/long-notes.json"
    ),
    answer!(
        "https://paperpaw.org/api/v1/plugins/long-notes/0.3.1/plugin.ini",
        TEXT,
        "store/long-notes.ini"
    ),
    // Fonts to download: two, one with a long name; no samples nor files
    // (a font without a sample is shown without).
    answer!(
        "https://paperpaw.org/api/v1/fonts.json",
        JSON,
        "fonts/fonts.json"
    ),
    answer!(
        "GET",
        "https://paperpaw.org/api/v1/fonts/*",
        404,
        TEXT,
        "fonts/none.txt"
    ),
    // The catalog the OPDS plugin comes with: its start, a list of books
    // (the same for a search), their covers, two of them to download.
    answer!(
        "https://www.gutenberg.org/ebooks.opds/",
        ATOM,
        "opds/catalog.xml"
    ),
    answer!(
        "https://www.gutenberg.org/ebooks/search.opds/?query=*",
        ATOM,
        "opds/search.xml"
    ),
    answer!(
        "https://www.gutenberg.org/ebooks/search.opds/*",
        ATOM,
        "opds/popular.xml"
    ),
    answer!(
        "https://www.gutenberg.org/cache/epub/5200/cover.png",
        PNG,
        "opds/cover-5200.png"
    ),
    answer!(
        "https://www.gutenberg.org/cache/epub/11/cover.png",
        PNG,
        "opds/cover-11.png"
    ),
    book!(
        "https://www.gutenberg.org/ebooks/5200.epub3.images",
        "die-verwandlung.epub"
    ),
    book!(
        "https://www.gutenberg.org/ebooks/11.epub3.images",
        "alice-in-wonderland.epub"
    ),
    // KOReader's progress sync at its default server: signed in, no
    // progress kept there yet, progress sent.
    answer!(
        "https://sync.koreader.rocks/users/auth",
        JSON,
        "kosync/auth.json"
    ),
    answer!(
        "POST",
        "https://sync.koreader.rocks/users/create",
        201,
        JSON,
        "kosync/create.json"
    ),
    answer!(
        "https://sync.koreader.rocks/syncs/progress/*",
        JSON,
        "kosync/progress.json"
    ),
    answer!(
        "PUT",
        "https://sync.koreader.rocks/syncs/progress",
        200,
        JSON,
        "kosync/sent.json"
    ),
    // The weather (Berlin's, for any place) and the places found.
    answer!(
        "https://api.open-meteo.com/v1/forecast?*",
        JSON,
        "weather/forecast.json"
    ),
    answer!(
        "https://geocoding-api.open-meteo.com/v1/search?*",
        JSON,
        "weather/search.json"
    ),
    // A summary from Wikipedia, whatever the word, in English and German.
    answer!(
        "https://en.wikipedia.org/api/rest_v1/page/summary/*",
        JSON,
        "wikipedia/en.json"
    ),
    answer!(
        "https://de.wikipedia.org/api/rest_v1/page/summary/*",
        JSON,
        "wikipedia/de.json"
    ),
    // A news feed, for a feed added by this address.
    answer!("https://news.example.org/feed.xml", RSS, "news/feed.xml"),
];

/// Screens that show what these answers give, or that another screen opens
/// with its choice, with the parameters their plugins open them with:
/// opened alone (as tools that draw every screen open them) they would
/// have nothing to show, or their plugin would refuse the open.
pub const SCREENS: &[(&str, &[(&str, &str)])] = &[
    ("store.plugin", &[("id", "reading-aids")]),
    ("store.shots", &[("id", "reading-aids")]),
    (
        "store.source",
        &[("title", "Clock"), ("text", "https://x.example/c")],
    ),
    ("settings.choose", &[("id", "core.language")]),
    ("help.overlay", &[("id", "home.dashboard")]),
    ("home.status-items", &[("id", "home.footer.items")]),
    ("files.names", &[("kind", "authors")]),
    ("opds.browse", &[("id", "0")]),
    ("wikipedia.lookup", &[("word", "Wonderland")]),
];

/// Where a body says which build of PaperPaw's code a code plugin was
/// made for: this emulator's, so that it is listed.
const ABI: &str = "{abi}";

thread_local! {
    /// The requests no answer fitted, the thread's (a device runs on one).
    static UNANSWERED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// The addresses asked for that had no answer since the last call, and
/// forgets them.
pub fn unanswered() -> Vec<String> {
    UNANSWERED.with_borrow_mut(std::mem::take)
}

/// The answer to a request with `method` to `url`, if the world has
/// one.
fn find(method: &str, url: &str) -> Option<&'static Answer> {
    ANSWERS.iter().find(|a| {
        let address = match a.url.strip_suffix('*') {
            Some(start) => url.starts_with(start),
            None => url == a.url,
        };
        address && a.method.eq_ignore_ascii_case(method)
    })
}

/// The network as the world answers it (see [`crate::http::set_route`]).
#[derive(Debug, Default)]
pub struct Offline;

impl Route for Offline {
    fn start(&mut self, call: HttpCall) -> Option<HttpResponse> {
        let Some(answer) = find(&call.method, &call.url) else {
            UNANSWERED.with_borrow_mut(|kept| kept.push(call.url.clone()));
            let reason = "no answer in the emulator's fixtures";
            return Some(HttpResponse::failed(call.id, HttpError::Refused, reason));
        };
        let mut body = answer.body.to_vec();
        if answer.content_type == JSON {
            let text = String::from_utf8_lossy(&body);
            if text.contains(ABI) {
                body = text.replace(ABI, &FINGERPRINT.to_string()).into_bytes();
            }
        }
        if body.len() > call.max_response {
            if call.save_to.is_some() {
                // A file cut short would be saved as if whole.
                let reason = format!("the answer is larger than {} bytes", call.max_response);
                return Some(HttpResponse::failed(call.id, HttpError::Refused, &reason));
            }
            body.truncate(call.max_response);
        }
        Some(HttpResponse {
            id: call.id,
            status: answer.status,
            body,
            content_type: Some(answer.content_type.to_string()),
            ..HttpResponse::default()
        })
    }

    fn later(&mut self) -> Option<HttpResponse> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get(url: &str) -> HttpCall {
        HttpCall {
            id: 1,
            method: String::from("GET"),
            url: url.to_string(),
            headers: Vec::new(),
            body: None,
            body_from: None,
            max_response: usize::MAX,
            timeout_ms: 1000,
            want_headers: Vec::new(),
            save_to: None,
            progress: false,
        }
    }

    #[test]
    fn every_page_of_the_list_and_its_files_are_answered() {
        let list = String::from_utf8_lossy(ANSWERS[0].body);
        let ids: Vec<&str> = list
            .split("\"id\": \"")
            .skip(1)
            .filter_map(|rest| rest.split('"').next())
            .collect();
        assert!(ids.len() >= 4, "{ids:?}");
        for id in ids {
            let page = find("GET", &format!("{LIBRARY}/{id}.json?device=x4&lang=en"))
                .unwrap_or_else(|| panic!("no page for {id}"));
            let text = String::from_utf8_lossy(page.body);
            for url in text.split("\"url\": \"").skip(1) {
                let url = url.split('"').next().unwrap_or_default();
                if url.starts_with("http") || url.ends_with(".ppn") {
                    continue;
                }
                assert!(find("GET", &format!("{LIBRARY}/{url}")).is_some(), "{url}");
            }
        }
    }

    #[test]
    fn the_catalogs_books_and_covers_are_answered() {
        let feed = find(
            "GET",
            "https://www.gutenberg.org/ebooks/search.opds/?sort_order=downloads",
        );
        let feed = String::from_utf8_lossy(feed.unwrap().body);
        let mut answered = 0;
        for href in feed.split("href=\"/").skip(1) {
            let path = href.split('"').next().unwrap_or_default();
            if find("GET", &format!("https://www.gutenberg.org/{path}")).is_some() {
                answered += 1;
            }
        }
        // Both covers and two of the three books (the third one is not).
        assert_eq!(answered, 5, "{feed}");
    }

    #[test]
    fn a_code_plugin_is_made_for_this_build() {
        let answer = Offline.start(get(&format!("{LIBRARY}.json?device=x4&lang=en")));
        let body = String::from_utf8(answer.unwrap().body).unwrap();
        assert!(!body.contains(ABI));
        assert!(body.contains(&FINGERPRINT.to_string()));
    }

    #[test]
    fn what_has_no_answer_fails_and_is_kept() {
        let _ = unanswered();
        let answer = Offline.start(get("https://example.org/feed")).unwrap();
        assert_eq!(answer.status, 0);
        assert_eq!(unanswered(), ["https://example.org/feed"]);
        assert!(unanswered().is_empty());
        // Another method to an address answered is not answered.
        let mut put = get("https://sync.koreader.rocks/users/auth");
        put.method = String::from("PUT");
        assert_eq!(Offline.start(put).unwrap().status, 0);
    }
}
