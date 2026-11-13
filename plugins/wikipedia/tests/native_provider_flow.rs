//! The real plugin as a lookup provider, driven by the scripted host: an ask
//! is answered *pending* and fetched once, the end of the request keeps the
//! answer and tells the askers (`lookup_ready`), and the next ask is answered
//! from what was kept, with the summary screen to open (Beta 3's dictionary
//! panel: *Looking it up …*, then the summary with *Open Wikipedia ›*). Where
//! the network is not reached the answer says so, as in Beta 3.
#![cfg(feature = "linked")]
#![allow(clippy::unwrap_used, clippy::expect_used, reason = "test assertions")]
#[path = "support/provider.rs"]
mod support;
use paperpaw_plugin_wikipedia::{NATIVE_ENTRY, declarations::FETCHED_TOKEN};
use paperpaw_scripted_host::ScriptedHost;
use support::{
    EXTRACT, NOT_FOUND, PENDING, UNREACHABLE, ask, done, done_with, host_with, host_with_net,
    open_screen, suspend, ui_handing_over,
};

fn summary_url(word: &str) -> String {
    format!("https://de.wikipedia.org/api/rest_v1/page/summary/{word} {FETCHED_TOKEN}")
}

#[test]
fn an_ask_is_pending_then_ready_and_answered_from_what_was_kept() {
    let (host, log) = host_with_net(200);
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    let (title, text, flags, _) = ask(&p, "Katze", "de");
    assert_eq!((title.as_str(), text.as_str()), ("Encyclopedia", ""));
    assert_eq!(flags & PENDING, PENDING, "answered later");
    assert_eq!(log.borrow().as_slice(), [summary_url("Katze")]);
    // Asked again while it is fetched: still pending, not fetched twice.
    assert_eq!(ask(&p, "Katze", "de").2 & PENDING, PENDING);
    assert_eq!(log.borrow().len(), 1);
    done(&p, 1, 200, 0);
    let (title, text, flags, opens) = ask(&p, "Katze", "de");
    assert_eq!(title, "Encyclopedia");
    assert_eq!(text, EXTRACT);
    assert_eq!(flags & PENDING, 0);
    assert_eq!(opens, "wikipedia.lookup", "Open Wikipedia ›");
    assert_eq!(log.borrow().len(), 1, "answered from what was kept");
}

#[test]
fn without_a_network_service_the_answer_says_the_network_was_not_reached() {
    let host = ScriptedHost::new();
    host.add_catalogs_in("i18n").unwrap();
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    let (title, text, flags, opens) = ask(&p, "Katze", "de");
    assert_eq!(title, "Encyclopedia");
    assert_eq!(text, UNREACHABLE);
    assert_eq!((flags & PENDING, opens.as_str()), (0, ""));
}

#[test]
fn a_request_that_does_not_get_through_is_kept_as_not_reached() {
    let (host, _log) = host_with_net(0);
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    assert_eq!(ask(&p, "Katze", "de").2 & PENDING, PENDING);
    // Reason 1: no network came.
    done(&p, 1, 0, 1);
    let (_, text, flags, opens) = ask(&p, "Katze", "de");
    assert_eq!(text, UNREACHABLE);
    assert_eq!((flags & PENDING, opens.as_str()), (0, ""));
}

/// Beta 3's 404: the entry says there is no article, and opens nothing.
#[test]
fn a_word_without_an_article_is_answered_so() {
    let (host, log) = host_with(404, "");
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    assert_eq!(ask(&p, "Katze", "de").2 & PENDING, PENDING);
    done_with(&p, 1, 404, 0, 0);
    let (_, text, flags, opens) = ask(&p, "Katze", "de");
    assert_eq!(text, NOT_FOUND);
    assert_eq!((flags & PENDING, opens.as_str()), (0, ""));
    assert_eq!(log.borrow().len(), 1, "kept, not fetched again");
}

/// A different word replaces the answer kept: the old one is dropped as
/// soon as the new request is accepted, so asking for it fetches again.
#[test]
fn a_different_word_replaces_the_answer_kept() {
    let (host, log) = host_with_net(200);
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    ask(&p, "Katze", "de");
    done(&p, 1, 200, 0);
    assert_eq!(ask(&p, "Katze", "de").1, EXTRACT, "kept");
    assert_eq!(ask(&p, "Hund", "de").2 & PENDING, PENDING);
    assert_eq!(ask(&p, "Katze", "de").2 & PENDING, PENDING, "dropped");
    assert_eq!(
        log.borrow().as_slice(),
        [
            summary_url("Katze"),
            summary_url("Hund"),
            summary_url("Katze")
        ]
    );
}

/// A suspend drops the answer kept: the next ask fetches again.
#[test]
fn a_suspend_drops_the_answer_kept() {
    let (host, log) = host_with_net(200);
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    ask(&p, "Katze", "de");
    done(&p, 1, 200, 0);
    suspend(&p);
    assert_eq!(ask(&p, "Katze", "de").2 & PENDING, PENDING);
    assert_eq!(log.borrow().len(), 2);
}

#[test]
fn another_request_ending_is_not_the_providers() {
    let (host, _log) = host_with_net(200);
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    assert_eq!(ask(&p, "Katze", "de").2 & PENDING, PENDING);
    assert!(
        !done(&p, 7, 200, 0),
        "no ready for a request it did not make"
    );
    assert_eq!(ask(&p, "Katze", "de").2 & PENDING, PENDING);
}

/// The screen an entry opens shows the summary the provider kept for its
/// word (`summary_for`) and fetches nothing.
#[test]
fn the_screen_for_a_kept_word_shows_it_without_fetching() {
    let (host, log) = host_with_net(200);
    ui_handing_over(&host, "Katze", String::new());
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    ask(&p, "Katze", "de");
    done(&p, 1, 200, 0);
    assert_eq!(open_screen(&p), 0);
    assert_eq!(log.borrow().len(), 1, "not fetched again");
}

/// The entry's text handed over (`core.ui` op 10, `document.md` 7) is
/// shown and not fetched; without one, or with a text longer than an
/// entry's, the screen fetches its summary.
#[test]
fn the_screen_shows_the_text_handed_over() {
    for (text, fetched) in [
        (EXTRACT.to_string(), 0),
        (String::new(), 1),
        ("x".repeat(2049), 1),
    ] {
        let (host, log) = host_with_net(200);
        ui_handing_over(&host, "Katze", text);
        let p = host.load(NATIVE_ENTRY.entry()).unwrap();
        assert_eq!(open_screen(&p), 0);
        assert_eq!(log.borrow().len(), fetched);
    }
}

/// The end of the request tells the askers: `command.lookup_ready` with the
/// ask's text and language. The host does not stage the command yet (it is
/// refused, and the provider keeps its answer all the same).
#[test]
#[ignore = "needs the host to stage command.lookup_ready (separate step after #1387)"]
fn the_end_of_the_request_tells_the_askers() {
    let (host, _log) = host_with_net(200);
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    assert_eq!(ask(&p, "Katze", "de").2 & PENDING, PENDING);
    assert!(done(&p, 1, 200, 0), "lookup_ready staged");
}
