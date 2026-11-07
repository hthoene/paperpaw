#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
use super::*;
use alloc::string::{String, ToString};

fn id(text: &'static str) -> Id {
    Id::new_static(text)
}

fn waiting(table: &mut RadioRequests, owner: &Id, request: u32, ssid: &str) {
    table.reserve(owner, request, false, ssid).unwrap();
    table.supersede_waiting();
    table.activate(owner.as_str(), request, 10);
}

#[test]
fn reserve_refuses_request_zero_duplicates_and_the_fifth_entry() {
    let (a, b) = (id("a"), id("b"));
    let mut table = RadioRequests::new();
    assert_eq!(table.reserve(&a, 0, false, "net"), Err(ERR_INVALID));
    table.reserve(&a, 1, false, "net").unwrap();
    assert_eq!(table.reserve(&a, 1, true, "net"), Err(ERR_INVALID));
    // The same number from another plugin is another request.
    table.reserve(&b, 1, false, "net").unwrap();
    table.reserve(&a, 2, false, "net").unwrap();
    table.reserve(&b, 2, false, "net").unwrap();
    assert_eq!(table.len(), MAX_REQUESTS);
    assert_eq!(table.reserve(&a, 3, false, "net"), Err(ERR_FULL));
    assert_eq!(
        table.len(),
        MAX_REQUESTS,
        "a refused reserve changes nothing"
    );
    assert_eq!(table.reserve(&a, 3, false, &"x".repeat(33)), Err(ERR_FULL));
}

#[test]
fn a_full_table_still_delivers_the_first_four_and_frees_room_per_delivery() {
    let a = id("a");
    let mut table = RadioRequests::new();
    for request in 1..=4 {
        table.reserve(&a, request, false, "net").unwrap();
    }
    assert_eq!(table.reserve(&a, 5, false, "net"), Err(ERR_FULL));
    for request in 1..=4 {
        assert!(table.end(a.as_str(), request, Outcome::success(STATE_ONLINE)));
    }
    for request in 1..=4 {
        let delivery = table.next_outcome("a").unwrap();
        assert_eq!(delivery.request, request, "oldest first");
        assert_eq!(delivery.outcome, Outcome::success(STATE_ONLINE));
        assert!(table.delivered("a", request));
        assert!(!table.delivered("a", request), "released exactly once");
    }
    assert!(table.is_empty());
    table.reserve(&a, 5, false, "net").unwrap();
}

#[test]
fn a_newer_join_supersedes_the_waiting_one_and_each_ends_exactly_once() {
    let (a, b) = (id("a"), id("b"));
    let mut table = RadioRequests::new();
    waiting(&mut table, &a, 1, "old");
    waiting(&mut table, &b, 7, "new");
    let first = table.next_outcome("a").unwrap().outcome;
    assert_eq!(first, Outcome::failed(reason::SUPERSEDED));
    assert_eq!(first.status, ERR_CANCELLED);
    assert!(table.next_outcome("b").is_none(), "the newer one waits");
    // The radio now reports the old network failing: that is not the old
    // request's outcome again, and not the new request's either.
    table.on_wifi_event(&WifiEvent::Failed {
        ssid: "old".to_string(),
        reason: JoinFailure::Timeout,
    });
    assert_eq!(table.next_outcome("a").unwrap().outcome, first);
    assert!(table.next_outcome("b").is_none());
    table.on_wifi_event(&WifiEvent::State(WifiState::Online {
        ssid: "new".to_string(),
        address: "10.0.0.2".to_string(),
        name: None,
    }));
    assert_eq!(
        table.next_outcome("b").unwrap().outcome,
        Outcome::success(STATE_ONLINE)
    );
    assert_eq!(
        table.next_outcome("a").unwrap().outcome,
        first,
        "a never ends twice"
    );
    assert!(!table.end("a", 1, Outcome::success(STATE_ONLINE)));
    assert!(!table.end("b", 7, Outcome::failed(reason::OTHER)));
}

#[test]
fn a_late_online_or_hosting_report_of_another_ssid_does_not_complete_the_waiting_request() {
    let (a, b) = (id("a"), id("b"));
    let online = |ssid: &str| {
        WifiEvent::State(WifiState::Online {
            ssid: ssid.to_string(),
            address: String::new(),
            name: None,
        })
    };
    let hosting = |ssid: &str| {
        WifiEvent::State(WifiState::Hosting {
            ssid: ssid.to_string(),
            address: String::new(),
            guests: 0,
        })
    };
    // Join A is superseded by join B; the radio then reports A online.
    let mut table = RadioRequests::new();
    waiting(&mut table, &a, 1, "net-a");
    waiting(&mut table, &b, 7, "net-b");
    table.on_wifi_event(&online("net-a"));
    assert!(table.next_outcome("b").is_none(), "B is still waiting");
    table.on_wifi_event(&online("net-b"));
    assert_eq!(
        table.next_outcome("b").unwrap().outcome,
        Outcome::success(STATE_ONLINE)
    );
    // The same for a host request and a stale hosting report.
    let mut table = RadioRequests::new();
    waiting(&mut table, &a, 1, "ap-old");
    table.reserve(&b, 7, true, "ap-new").unwrap();
    table.supersede_waiting();
    table.activate("b", 7, 20);
    table.on_wifi_event(&hosting("ap-old"));
    assert!(table.next_outcome("b").is_none());
    // A hosting report completes no join, an online report no host.
    table.on_wifi_event(&online("ap-new"));
    assert!(table.next_outcome("b").is_none());
    table.on_wifi_event(&hosting("ap-new"));
    assert_eq!(
        table.next_outcome("b").unwrap().outcome,
        Outcome::success(STATE_HOSTING)
    );
}

#[test]
fn radio_reports_map_to_statuses_and_reasons() {
    let a = id("a");
    for (failure, expected) in [
        (JoinFailure::NotFound, (ERR_NOT_FOUND, 1)),
        (JoinFailure::Password, (ERR_DENIED, 2)),
        (JoinFailure::Timeout, (ERR_TIMEOUT, 3)),
        (JoinFailure::Other, (ERR_IO, 0)),
    ] {
        let mut table = RadioRequests::new();
        waiting(&mut table, &a, 1, "net");
        table.on_wifi_event(&WifiEvent::Failed {
            ssid: "net".to_string(),
            reason: failure,
        });
        let outcome = table.next_outcome("a").unwrap().outcome;
        assert_eq!(
            (outcome.status, outcome.reason, outcome.state),
            (expected.0, expected.1, 0)
        );
    }
    assert_eq!(Outcome::failed(99), Outcome::failed(reason::OTHER));
    assert_eq!(Outcome::failed(reason::UNSUPPORTED).status, ERR_UNSUPPORTED);
    assert_eq!(Outcome::failed(reason::NEEDS_USER).status, ERR_DENIED);
}

#[test]
fn a_host_ends_on_hosting_a_join_on_online_and_the_radio_off_ends_both() {
    let a = id("a");
    let mut table = RadioRequests::new();
    table.reserve(&a, 1, true, "ap").unwrap();
    table.activate("a", 1, 0);
    table.on_wifi_event(&WifiEvent::State(WifiState::Online {
        ssid: "x".to_string(),
        address: String::new(),
        name: None,
    }));
    assert!(table.next_outcome("a").is_none(), "online is not hosting");
    table.on_wifi_event(&WifiEvent::State(WifiState::Hosting {
        ssid: "ap".to_string(),
        address: String::new(),
        guests: 0,
    }));
    assert_eq!(
        table.next_outcome("a").unwrap().outcome.state,
        STATE_HOSTING
    );

    let mut table = RadioRequests::new();
    waiting(&mut table, &a, 1, "net");
    table.on_wifi_event(&WifiEvent::State(WifiState::Off));
    assert_eq!(
        table.next_outcome("a").unwrap().outcome,
        Outcome::failed(reason::OTHER)
    );
}

#[test]
fn leave_and_off_cancel_a_waiting_request() {
    let a = id("a");
    let mut table = RadioRequests::new();
    waiting(&mut table, &a, 1, "net");
    table.radio_stopped();
    assert_eq!(
        table.next_outcome("a").unwrap().outcome,
        Outcome::failed(reason::SUPERSEDED)
    );
}

#[test]
fn the_backstop_ends_a_request_the_radio_never_reports() {
    let a = id("a");
    let mut table = RadioRequests::new();
    waiting(&mut table, &a, 1, "net");
    table.tick(10 + BACKSTOP_MS - 1);
    assert!(table.next_outcome("a").is_none());
    table.tick(10 + BACKSTOP_MS);
    assert_eq!(
        table.next_outcome("a").unwrap().outcome,
        Outcome::failed(reason::TIMEOUT)
    );
    table.tick(u64::MAX);
    assert_eq!(table.len(), 1);
}

#[test]
fn a_stopped_sender_releases_everything_without_delivery() {
    let (a, b) = (id("a"), id("b"));
    let mut table = RadioRequests::new();
    waiting(&mut table, &a, 1, "net");
    table.reserve(&a, 2, false, "net").unwrap();
    table.end("a", 2, Outcome::failed(reason::NEEDS_USER));
    table.reserve(&b, 1, false, "net").unwrap();
    table.sender_stopped("a");
    assert_eq!(table.len(), 1);
    assert!(table.next_outcome("a").is_none());
    assert!(!table.holds("a", 1));
    assert!(table.holds("b", 1));
    // A late radio report finds nothing of a's to end.
    table.on_wifi_event(&WifiEvent::State(WifiState::Off));
    assert!(table.next_outcome("a").is_none());
}

#[test]
fn only_unapplied_reservations_are_given_back() {
    let a = id("a");
    let mut table = RadioRequests::new();
    waiting(&mut table, &a, 1, "net");
    table.reserve(&a, 2, false, "net").unwrap();
    table.release_reserved("a", 1);
    assert!(table.holds("a", 1), "a waiting request stays");
    table.discard_reserved("a");
    assert!(table.holds("a", 1));
    assert!(!table.holds("a", 2));
}

#[test]
fn a_delivery_encodes_event_wifi_done_for_its_sender() {
    let a = id("a");
    let mut table = RadioRequests::new();
    table.reserve(&a, 9, false, "home").unwrap();
    table.end("a", 9, Outcome::failed(reason::NEEDS_USER));
    let mut bytes = [0; 128];
    let len = table
        .next_outcome("a")
        .unwrap()
        .encode(77, &mut bytes)
        .unwrap();
    let wire::Record::EventWifiDone(done) = wire::records_decode_single(&bytes[..len]).unwrap()
    else {
        panic!("not a wifi_done");
    };
    assert_eq!(
        (
            done.now_ms,
            done.request,
            done.ssid,
            done.status,
            done.reason,
            done.state
        ),
        (77, 9, "home", ERR_DENIED, 6, 0)
    );
}

#[test]
fn debug_shows_counts_only() {
    let a = id("a");
    let mut table = RadioRequests::new();
    waiting(&mut table, &a, 1, "secret-ssid");
    let text = alloc::format!("{table:?}");
    assert!(text.contains("entries: 1") && text.contains("waiting: 1"));
    assert!(!text.contains("secret-ssid"));
}
