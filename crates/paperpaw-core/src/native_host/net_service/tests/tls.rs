//! `core.net` and the TLS session it shares with `core.sockets`: an https
//! request holds a session from its start to its end, on every way it ends,
//! and waits for one in the order it asked.
use super::*;
use crate::native_host::net_pool::{TlsHolder, TlsLease};
use crate::native_host::net_service::table::cancel;

fn one_session() -> (Rc<NetPool>, Fake) {
    let pool = Rc::new(NetPool::with_tls_sessions(NetProfile::X4.pool, 1));
    let fake = Fake::with_pool(NetProfile::X4, pool.clone());
    (pool, fake)
}

/// A small request (the pool of an X4 holds one default body only).
fn small(token: u32, url: &'static str) -> Vec<u8> {
    let mut h = head(token, url);
    h.max_body = 1_024;
    batch(&[Record::ServiceNetRequest(h)])
}

fn plain(token: u32) -> Vec<u8> {
    small(token, "http://example.org/a")
}

fn get(token: u32) -> Vec<u8> {
    small(token, "https://example.org/a")
}

fn net(id: u32) -> TlsHolder {
    TlsHolder::Net(id)
}

#[test]
fn an_https_request_holds_the_session_while_it_runs_and_a_plain_one_does_not() {
    let (pool, fake) = one_session();
    let p = request(&fake, OWNER, &plain(1)).unwrap();
    assert_eq!(pool.tls_held(), 0, "http needs no session");
    assert_eq!(*fake.started.borrow(), [p]);
    let a = request(&fake, OWNER, &get(2)).unwrap();
    assert!(pool.tls_holds(net(a)));
    // Upper case is the same scheme.
    finish(&fake, a, 200, b"");
    assert_eq!(pool.tls_held(), 0, "back at the end");
    let b = request(&fake, OWNER, &small(3, "HTTPS://example.org/")).unwrap();
    assert!(pool.tls_holds(net(b)));
}

#[test]
fn an_https_request_waits_in_the_order_asked_and_never_runs_beside_another() {
    let (pool, fake) = one_session();
    let a = request(&fake, OWNER, &get(1)).unwrap();
    let b = request(&fake, OWNER, &get(2)).unwrap();
    let c = request(&fake, OTHER, &get(3)).unwrap();
    assert_eq!(*fake.started.borrow(), [a], "one session: one at a time");
    assert!(!pool.tls_holds(net(b)));
    finish(&fake, a, 200, b"");
    assert_eq!(*fake.started.borrow(), [a, b], "b asked before c");
    assert!(pool.tls_holds(net(b)) && !pool.tls_holds(net(c)));
    finish(&fake, b, 200, b"");
    assert_eq!(*fake.started.borrow(), [a, b, c]);
    finish(&fake, c, 200, b"");
    assert_eq!(pool.tls_held(), 0);
    assert!(!pool.tls_waiting());
}

#[test]
fn a_socket_holding_the_session_makes_https_wait_and_it_starts_when_it_is_free() {
    let (pool, fake) = one_session();
    assert_eq!(pool.tls_request(TlsHolder::Sock(1)), TlsLease::Held);
    let a = request(&fake, OWNER, &get(1)).unwrap();
    assert!(fake.started.borrow().is_empty());
    // A plain request behind it waits too: the queue is in order.
    let p = request(&fake, OWNER, &plain(2)).unwrap();
    assert!(fake.started.borrow().is_empty());
    tick(&fake).unwrap();
    assert!(
        fake.started.borrow().is_empty(),
        "keeps asking, still waits"
    );
    pool.tls_drop(TlsHolder::Sock(1));
    tick(&fake).unwrap();
    assert_eq!(*fake.started.borrow(), [a, p]);
}

#[test]
fn a_request_that_gives_up_while_it_waits_leaves_the_queue_and_blocks_nobody() {
    let (pool, fake) = one_session();
    assert_eq!(pool.tls_request(TlsHolder::Sock(1)), TlsLease::Held);
    let a = request(&fake, OWNER, &get(1)).unwrap();
    let b = request(&fake, OTHER, &get(2)).unwrap();
    // The head waiter (a) is cancelled.
    cancel(&fake, OWNER, 1).unwrap();
    pool.tls_drop(TlsHolder::Sock(1));
    tick(&fake).unwrap();
    assert!(!pool.tls_holds(net(a)));
    assert_eq!(*fake.started.borrow(), [b], "b is not stuck behind a");
}

#[test]
fn every_way_a_request_ends_gives_the_session_back() {
    for how in 0..6 {
        let (pool, fake) = one_session();
        let a = request(&fake, OWNER, &get(1)).unwrap();
        assert!(pool.tls_holds(net(a)), "way {how}");
        match how {
            0 => finish(&fake, a, 200, b"body"),
            1 => {
                // An error answer.
                finish(&fake, a, 500, b"");
            }
            2 => {
                // Too long.
                fake.advance(u64::from(MEMORY_TIMEOUT_MS));
                tick(&fake).unwrap();
            }
            3 => cancel(&fake, OWNER, 1).unwrap(),
            4 => owner_stopped(&fake, OWNER).unwrap(),
            _ => {
                // No network at all.
                network_lost(&fake).unwrap();
                fake.advance(1);
            }
        }
        if how == 5 {
            // A lost network does not end a running request by itself: the
            // transport reports it; the table then ends with its reason.
            finish(&fake, a, 0, b"");
        }
        assert_eq!(pool.tls_held(), 0, "way {how}");
        assert!(!pool.tls_waiting(), "way {how}");
        drain(&fake, OWNER);
        owner_stopped(&fake, OWNER).unwrap();
        assert_eq!(pool.tls_held(), 0, "way {how}");
    }
}

#[test]
fn a_transport_that_cannot_start_an_https_request_gives_the_session_back() {
    let (pool, fake) = one_session();
    fake.start_fails
        .set(Some(paperpaw_native::host_glue::ERR_IO));
    let a = request(&fake, OWNER, &get(1)).unwrap();
    assert_eq!(pool.tls_held(), 0);
    assert!(!pool.tls_holds(net(a)));
    fake.start_fails.set(None);
    let b = request(&fake, OWNER, &get(2)).unwrap();
    assert!(pool.tls_holds(net(b)));
}

#[test]
fn a_table_that_goes_with_requests_gives_their_sessions_back() {
    let (pool, fake) = one_session();
    let a = request(&fake, OWNER, &get(1)).unwrap();
    let _b = request(&fake, OWNER, &get(2)).unwrap();
    assert!(pool.tls_holds(net(a)));
    drop(fake);
    assert_eq!(pool.tls_held(), 0);
    assert!(!pool.tls_waiting());
}

#[test]
fn net_and_sockets_hold_one_session_at_a_time_and_nobody_starves() {
    let (pool, fake) = one_session();
    let (s1, s2) = (TlsHolder::Sock(1), TlsHolder::Sock(2));
    // The order they ask in: a (holds), s1, b, s2.
    let a = request(&fake, OWNER, &get(1)).unwrap();
    assert_eq!(pool.tls_request(s1), TlsLease::Wait);
    let b = request(&fake, OWNER, &get(2)).unwrap();
    assert_eq!(pool.tls_request(s2), TlsLease::Wait);
    let mut order = Vec::new();
    let mut done = [false, false];
    for _ in 0..6 {
        // Everybody asks at each turn, the last in the queue first: asking
        // out of turn gets no one ahead.
        if !done[1] {
            pool.tls_request(s2);
        }
        if !done[0] {
            pool.tls_request(s1);
        }
        tick(&fake).unwrap();
        assert!(pool.tls_held() <= 1, "never two at once");
        if pool.tls_holds(net(a)) {
            order.push("a");
            finish(&fake, a, 200, b"");
        } else if pool.tls_holds(s1) {
            order.push("s1");
            pool.tls_drop(s1);
            done[0] = true;
        } else if pool.tls_holds(net(b)) {
            order.push("b");
            finish(&fake, b, 200, b"");
        } else if pool.tls_holds(s2) {
            order.push("s2");
            pool.tls_drop(s2);
            done[1] = true;
        }
    }
    assert_eq!(order, ["a", "s1", "b", "s2"], "in the order they asked");
    assert_eq!(pool.tls_held(), 0);
    assert!(!pool.tls_waiting());
}

#[test]
fn an_allocation_that_fails_while_an_https_request_starts_leaves_no_session_held() {
    for nth in 1..=8 {
        let (pool, fake) = one_session();
        fail_nth(nth);
        let started = request(&fake, OWNER, &get(1));
        fail_nth(0);
        owner_stopped(&fake, OWNER).unwrap();
        assert_eq!(pool.tls_held(), 0, "gate {nth} ({started:?})");
        assert!(!pool.tls_waiting(), "gate {nth}");
    }
}
