//! The first deadline the table reports is the instant `tick` ends something.
use super::*;

#[test]
fn nothing_waiting_on_the_clock_has_no_deadline() {
    let fake = Fake::x4();
    assert_eq!(next_deadline(&fake), None);
}

#[test]
fn the_deadline_is_the_instant_the_tick_ends_the_request() {
    let fake = Fake::x4();
    let start = fake.now_ms();
    let id = request(&fake, OWNER, &get(1)).unwrap();
    assert_eq!(next_deadline(&fake), Some(start + 30_000));
    fake.advance(29_999);
    tick(&fake).unwrap();
    assert!(drain(&fake, OWNER).is_empty());
    assert_eq!(next_deadline(&fake), Some(start + 30_000));
    fake.advance(1);
    tick(&fake).unwrap();
    assert_eq!(drain(&fake, OWNER).len(), 1, "ended at the deadline");
    let _ = id;
    // The answer is held, not waiting on the clock.
    assert_eq!(next_deadline(&fake), None);
}

#[test]
fn the_earliest_of_several_requests_counts_and_a_wait_for_a_network_is_30_seconds() {
    let fake = Fake::x4();
    let start = fake.now_ms();
    let mut h = head(1, "http://a.b/");
    h.timeout_ms = 600_000;
    request(&fake, OWNER, &batch(&[Record::ServiceNetRequest(h)])).unwrap();
    assert_eq!(next_deadline(&fake), Some(start + 600_000));
    fake.network.set(false);
    fake.advance(1_000);
    let mut h = head(2, "http://a.b/");
    h.timeout_ms = 600_000;
    request(&fake, OWNER, &batch(&[Record::ServiceNetRequest(h)])).unwrap();
    // The second never started and has waited since its request.
    assert!(next_deadline(&fake).unwrap() <= start + 1_000 + 30_000);
}
