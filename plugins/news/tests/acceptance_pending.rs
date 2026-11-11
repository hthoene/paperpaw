//! Acceptance of the interface-7 port, pending on the host. The interface-6
//! plugin was pinned by `fetch.rs`, `fetching.rs` and `widgets.rs` on a kernel
//! (removed with it, #244); these cases are the same behaviours through the
//! production loader with the native plugin. They never pass by themselves.
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 4] = [
    // The feeds are listed, added by their address (typed or from a phone),
    // fetched alone or all, and removed.
    Case {
        name: "news-feeds",
        golden: None,
        blocked_by: &["H1", "H2", "KEYBOARD-slot"],
    },
    // Each feed is fetched and made into a book of its newest articles, one
    // feed at a time, from RSS 2.0, RSS 1.0 or Atom.
    Case {
        name: "news-fetch",
        golden: None,
        blocked_by: &["NET", "FS"],
    },
    // The headlines widget shows the newest headlines of the feeds fetched
    // last, and a tap opens the feed's book.
    Case {
        name: "news-headlines-widget",
        golden: None,
        blocked_by: &["H1", "H3"],
    },
    // A fetch started by a rule runs as declared work at a cheap moment.
    Case {
        name: "news-fetch-due",
        golden: None,
        blocked_by: &["WORK-declaration", "NET"],
    },
];

#[test]
fn the_cases_name_what_they_wait_for() {
    for case in &CASES {
        assert!(case.golden_exists());
        let error = case.refuse().unwrap_err();
        assert!(error.starts_with("BLOCKED "), "{error}");
        assert!(error.contains(case.blocked_by[0]), "{error}");
    }
}

#[test]
#[ignore = "BLOCKED H1/H2/KEYBOARD-slot: screen commands, dialogs and the keyboard slot are not qualified"]
fn news_feeds() {
    CASES[0].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED NET/FS: network and files are not qualified for the native plugin"]
fn news_fetch() {
    CASES[1].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H3: screen commands and composition are not qualified for the native plugin"]
fn news_headlines_widget() {
    CASES[2].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED WORK-declaration/NET: declared work and the network are not qualified for the native plugin"]
fn news_fetch_due() {
    CASES[3].refuse().unwrap();
}
