//! The first deadline the table reports is the instant `tick` ends something.
use super::*;

#[test]
fn nothing_waiting_on_the_clock_has_no_deadline() {
    let f = Fake::x4();
    assert_eq!(next_deadline(&f), None);
}

#[test]
fn connect_idle_and_network_waits_report_the_instant_they_end() {
    let f = Fake::x4();
    let start = f.now_ms();
    let id = connect_with(
        &f,
        OWNER,
        wire::ServiceSockConnect {
            timeout_ms: 5_000,
            idle_ms: 10_000,
            ..head(1)
        },
    )
    .unwrap();
    assert_eq!(next_deadline(&f), Some(start + 5_000));
    f.advance(2_000);
    connected(&f, id).unwrap();
    // Open: idle counts from the last move.
    assert_eq!(next_deadline(&f), Some(start + 2_000 + 10_000));
    drain(&f, OWNER);
    f.advance(10_000);
    assert_eq!(next_deadline(&f), Some(start + 12_000));
    tick(&f).unwrap();
    assert_eq!(drain(&f, OWNER).len(), 1, "ended at the deadline");
    assert_eq!(next_deadline(&f), None);
    // A socket waiting for a network that is away: 30 s from its connect.
    f.network.set(false);
    let at = f.now_ms();
    connect(&f, OWNER, 2).unwrap();
    assert_eq!(next_deadline(&f), Some(at + NO_NETWORK_MS));
}
