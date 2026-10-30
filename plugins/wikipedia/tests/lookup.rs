//! Looking a word up on Wikipedia with the real kernel and canned
//! answers: the address for any word and language stays on the right
//! wiki with the word encoded; JSON answers that are malformed, empty or
//! not objects read as "no article", like a 404; Unicode titles and
//! escapes show; other failures are told apart; long summaries scroll
//! and the panel closes at their end.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_core::{HttpCall, HttpResponse, Io, Kernel};
use paperpaw_platform::Rotation;
use paperpaw_plugin_wikipedia::Wikipedia;
use paperpaw_sdk::builtin::{actions, params, slots};
use paperpaw_sdk::{
    DeviceInfo, DisplayInfo, Id, InputCaps, Instant, MemoryClass, MemoryStorage, Params,
    PixelFormat,
};

const DEVICE: DeviceInfo = DeviceInfo::new(
    "test",
    "Test device",
    DisplayInfo::new(480, 800, 220, PixelFormat::Gray4, Rotation::Deg0, None),
    InputCaps::new(true, false, false, &[]),
    MemoryClass::Large,
);

struct Lookup {
    kernel: Kernel,
    now: u64,
}

impl Lookup {
    /// `word` looked up in `language`, with or without a network.
    fn open(word: &str, language: Option<&str>, network: bool) -> Self {
        let mut kernel = Kernel::builder(DEVICE)
            .storage(MemoryStorage::new())
            .user_config("[core]\nlanguage = en\n")
            .network(network)
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(Wikipedia)
            .build()
            .unwrap();
        kernel.start(Instant(0));
        let mut p = Params::new().with(params::WORD, word);
        if let Some(language) = language {
            p = p.with(params::LANGUAGE, language);
        }
        kernel.open_slot(Instant(1), slots::LOOKUP_ONLINE, p);
        Self { kernel, now: 1 }
    }

    fn requests(&mut self) -> Vec<HttpCall> {
        self.kernel
            .take_io()
            .into_iter()
            .filter_map(|io| match io {
                Io::Http(call) => Some(call),
                _ => None,
            })
            .collect()
    }

    fn request(&mut self) -> HttpCall {
        let mut calls = self.requests();
        assert_eq!(calls.len(), 1, "{calls:?}");
        calls.remove(0)
    }

    /// The word looked up, answered with `status` and `body`.
    fn answered(word: &str, status: u16, body: &str) -> Self {
        let mut lookup = Self::open(word, Some("en"), true);
        let call = lookup.request();
        lookup.now += 100;
        lookup.kernel.http_response(
            Instant(lookup.now),
            HttpResponse {
                id: call.id,
                status,
                body: body.as_bytes().to_vec(),
                ..HttpResponse::default()
            },
        );
        lookup
    }

    fn run(&mut self, action: Id) {
        self.now += 100;
        self.kernel.run_action(Instant(self.now), action);
    }

    /// The screen as drawn now.
    fn frame(&mut self) -> Vec<u8> {
        self.kernel.render(&mut |_, _| {});
        let mut pixels = Vec::new();
        self.kernel.capture(&mut |_, view| {
            for y in 0..view.height {
                for x in 0..view.width {
                    pixels.push(view.pixel(x, y).luma());
                }
            }
        });
        pixels
    }

    fn said(&mut self) -> String {
        self.kernel.render(&mut |_, _| {});
        let labels: Vec<String> = self
            .kernel
            .describe()
            .into_iter()
            .map(|p| p.label)
            .collect();
        labels.join(" | ")
    }
}

#[test]
fn any_word_and_language_ask_the_right_wiki_with_the_word_encoded() {
    for (word, language, expected) in [
        (
            "white rabbit",
            Some("en-GB"),
            "https://en.wikipedia.org/api/rest_v1/page/summary/white_rabbit",
        ),
        (
            "Größe",
            Some("de_AT"),
            "https://de.wikipedia.org/api/rest_v1/page/summary/Gr%C3%B6%C3%9Fe",
        ),
        (
            "a/b?c#d&e",
            Some("fr"),
            "https://fr.wikipedia.org/api/rest_v1/page/summary/a%2Fb%3Fc%23d%26e",
        ),
        (
            "x",
            Some("evil.example/../"),
            "https://evilexample.wikipedia.org/api/rest_v1/page/summary/x",
        ),
        (
            "x",
            Some("../../"),
            "https://en.wikipedia.org/api/rest_v1/page/summary/x",
        ),
        (
            "x",
            Some(""),
            "https://en.wikipedia.org/api/rest_v1/page/summary/x",
        ),
        (
            "  spaced  ",
            Some("EN"),
            "https://en.wikipedia.org/api/rest_v1/page/summary/spaced",
        ),
    ] {
        let mut lookup = Lookup::open(word, language, true);
        let call = lookup.request();
        assert_eq!(call.url, expected, "{word} {language:?}");
        assert_eq!(call.method, "GET");
        assert!(
            call.headers
                .contains(&(String::from("Accept"), String::from("application/json")))
        );
    }
    // Without a language given, PaperPaw's own.
    let mut lookup = Lookup::open("Hund", None, true);
    assert!(
        lookup
            .request()
            .url
            .starts_with("https://en.wikipedia.org/")
    );
}

#[test]
fn the_summary_shows_under_the_articles_own_title() {
    let mut lookup = Lookup::answered(
        "grosse",
        200,
        r#"{"type": "standard", "title": "Gr\u00f6\u00dfe \ud83d\udcd6", "extract": "A summary.\nSecond paragraph."}"#,
    );
    let said = lookup.said();
    assert!(said.contains("Größe 📖"), "{said}");
    assert!(!said.contains("grosse"), "{said}");
}

#[test]
fn answers_without_a_summary_read_as_no_article() {
    let not_found = Lookup::answered("x", 404, "").frame();
    for body in [
        "",
        "not json",
        "[1, 2, 3]",
        r#"{"title": "Only a title"}"#,
        r#"{"extract": ""}"#,
        r#"{"extract": "   \n  "}"#,
        r#"{"extract": 42}"#,
        r#"{"extract": null}"#,
        r#"{"extract": "cut"#,
        "{\"extract\": \"\\ud800\"",
    ] {
        let frame = Lookup::answered("x", 200, body).frame();
        assert!(frame == not_found, "{body}");
    }
}

#[test]
fn other_failures_are_told_apart() {
    let not_found = Lookup::answered("x", 404, "").frame();
    let unreachable = Lookup::answered("x", 0, "timed out").frame();
    let failed = Lookup::answered("x", 500, "").frame();
    assert!(unreachable != not_found);
    assert!(failed != not_found && failed != unreachable);
    assert!(Lookup::answered("x", 503, "<html>busy</html>").frame() == failed);
    assert!(Lookup::answered("x", 301, "").frame() == failed);
    let found = Lookup::answered("x", 200, r#"{"extract": "Found."}"#).frame();
    assert!(found != not_found && found != failed);
}

#[test]
fn without_a_network_nothing_is_asked() {
    let mut lookup = Lookup::open("x", Some("en"), false);
    assert!(lookup.requests().is_empty());
    let offline = lookup.frame();
    assert!(offline != Lookup::answered("x", 0, "").frame());
}

#[test]
fn a_long_summary_scrolls_and_closes_at_its_end() {
    // Lines that differ, so a page scrolled looks different.
    let words: String = (0..3000).map(|n| format!("w{n} ")).collect();
    let body = format!(r#"{{"title": "Long", "extract": "{words}"}}"#);
    let mut lookup = Lookup::answered("long", 200, &body);
    let first = lookup.frame();
    lookup.run(actions::NEXT_PAGE);
    let second = lookup.frame();
    assert!(first != second, "it scrolls");
    lookup.run(actions::PREV_PAGE);
    assert!(lookup.frame() == first, "and back");
    lookup.run(actions::PREV_PAGE);
    assert!(lookup.frame() == first, "not before the start");
    // To the end, then no further.
    let mut last = first.clone();
    for _ in 0..200 {
        lookup.run(actions::NEXT_PAGE);
        let now = lookup.frame();
        if now == last {
            break;
        }
        last = now;
    }
    assert!(last != first);
    assert!(lookup.kernel.active_screen().is_some());
    // Select closes it.
    lookup.run(actions::CONFIRM);
    assert_ne!(
        lookup.kernel.active_screen().map(Id::as_str),
        Some("wikipedia.lookup")
    );
}
