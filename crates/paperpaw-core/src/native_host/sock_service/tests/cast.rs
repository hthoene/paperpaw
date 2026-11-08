//! Broadcast: the call, the limits, the answers (also hostile ones), the
//! `done` and everything that ends a broadcast early.
use super::*;
use crate::native_host::sock_service::table::broadcast as start_cast;

#[derive(Debug, PartialEq, Eq, Clone)]
struct CEv {
    socket: u32,
    token: u32,
    kind: u32,
    reason: u32,
    len: u32,
    from: String,
}

fn cev(socket: u32, token: u32, kind: u32, reason: u32, len: u32, from: &str) -> CEv {
    CEv {
        socket,
        token,
        kind,
        reason,
        len,
        from: from.to_string(),
    }
}

fn input(token: u32, wait_ms: u32, data: &[u8], ports: &[u32]) -> Vec<u8> {
    let mut records = std::vec![Record::ServiceSockBroadcast(wire::ServiceSockBroadcast {
        token,
        wait_ms,
        data,
    })];
    for port in ports {
        records.push(Record::ServiceSockPort(wire::ServiceSockPort {
            port: *port,
        }));
    }
    batch(&records)
}

fn cast(f: &Fake, owner: u64, token: u32) -> Result<u32, i32> {
    cast_with(f, owner, &input(token, 1_500, b"hello", &[9_090, 9_091]))
}

fn cast_with(f: &Fake, owner: u64, bytes: &[u8]) -> Result<u32, i32> {
    let hosts = HostServices::new();
    start_cast(f, &caller(owner, &hosts), bytes)
}

fn next_cast(f: &Fake, owner: u64) -> Option<CEv> {
    let mut out = std::vec![0; 256];
    let used = next_event(f, owner, &mut out).unwrap()?;
    match wire::record_decode_one(&out[..used]).unwrap() {
        Record::EventBroadcast(e) => Some(cev(e.socket, e.token, e.kind, e.reason, e.len, e.from)),
        other => panic!("not event 107: {other:?}"),
    }
}

fn drain_cast(f: &Fake, owner: u64) -> Vec<CEv> {
    let mut all = Vec::new();
    while let Some(e) = next_cast(f, owner) {
        all.push(e);
        event_delivered(f, owner).unwrap();
    }
    all
}

#[test]
fn a_broadcast_sends_collects_answers_and_ends_with_done() {
    let f = Fake::x4();
    let id = cast(&f, OWNER, 7).unwrap();
    assert_eq!(
        &*f.casts.borrow(),
        &[(id, b"hello".to_vec(), std::vec![9_090, 9_091])]
    );
    assert_eq!(f.used(), 8_192, "eight answers are charged at the call");
    answered(&f, id, b"192.168.1.5:9090", b"I am calibre").unwrap();
    answered(&f, id, b"192.168.1.9:9091", b"").unwrap();
    assert_eq!(
        next_cast(&f, OWNER),
        Some(cev(id, 7, 0, 0, 12, "192.168.1.5:9090"))
    );
    assert_eq!(data_of(&f, OWNER, id, 0).unwrap(), b"I am calibre");
    assert_eq!(data_of(&f, OWNER, id, 5).unwrap(), b"calibre");
    assert_eq!(data_of(&f, OWNER, id, 99).unwrap(), b"");
    assert_eq!(data_of(&f, OTHER, id, 0), Err(ERR_NOT_FOUND));
    event_delivered(&f, OWNER).unwrap();
    assert_eq!(
        data_of(&f, OWNER, id, 0),
        Err(ERR_STATE),
        "only during the event"
    );
    assert_eq!(
        next_cast(&f, OWNER),
        Some(cev(id, 7, 0, 0, 0, "192.168.1.9:9091"))
    );
    assert_eq!(data_of(&f, OWNER, id, 0).unwrap(), b"");
    event_delivered(&f, OWNER).unwrap();
    // The time is not over: no `done` yet.
    f.advance(1_499);
    tick(&f).unwrap();
    assert_eq!(next_cast(&f, OWNER), None);
    f.advance(1);
    tick(&f).unwrap();
    assert_eq!(drain_cast(&f, OWNER), std::vec![cev(id, 7, 1, 0, 0, "")]);
    assert_eq!(f.used(), 0);
    assert_eq!(f.table.held(), (0, None));
    assert_eq!(
        f.closes.borrow().as_slice(),
        &[id],
        "the transport stopped listening"
    );
    assert_eq!(&*f.demand.borrow(), &[true, false]);
}

#[test]
fn every_refusal_changes_nothing() {
    let f = Fake::x4();
    let bad: [(Vec<u8>, i32); 10] = [
        (input(1, 1_000, b"", &[9_090]), ERR_INVALID),
        (input(1, 99, b"x", &[9_090]), ERR_INVALID),
        (input(1, 10_001, b"x", &[9_090]), ERR_INVALID),
        (input(1, 1_000, b"x", &[]), ERR_INVALID),
        (input(1, 1_000, b"x", &[0]), ERR_INVALID),
        (input(1, 1_000, b"x", &[65_536]), ERR_INVALID),
        (
            input(1, 1_000, b"x", &[1, 2, 3, 4, 5, 6, 7, 8, 9]),
            ERR_INVALID,
        ),
        (
            batch(&[
                Record::ServiceSockPort(wire::ServiceSockPort { port: 1 }),
                Record::ServiceSockBroadcast(wire::ServiceSockBroadcast {
                    token: 1,
                    wait_ms: 1_000,
                    data: b"x",
                }),
            ]),
            ERR_INVALID,
        ),
        (Vec::new(), ERR_INVALID),
        (input(1, 1_000, &[7; 512], &[1, 2, 3, 4, 5, 6, 7, 8]), 0),
    ];
    for (bytes, want) in bad {
        let got = cast_with(&f, OWNER, &bytes);
        if want == 0 {
            let id = got.unwrap();
            close(&f, OWNER, id).unwrap();
            drain_cast(&f, OWNER);
        } else {
            assert!(got.is_err(), "{want}");
            assert_ne!(got, Err(ERR_STATE));
        }
    }
    f.subscribed.set(false);
    assert_eq!(cast(&f, OWNER, 1), Err(ERR_STATE), "no subscription");
    f.subscribed.set(true);
    assert_eq!(f.used(), 0);
    assert_eq!(
        f.casts.borrow().len(),
        1,
        "only the good one reached the transport"
    );
    assert_eq!(f.table.held().0, 0);
}

#[test]
fn a_broadcast_is_one_of_the_plugins_sockets_while_it_runs() {
    let f = Fake::x4();
    let a = cast(&f, OWNER, 1).unwrap();
    let b = connect(&f, OWNER, 2).unwrap();
    assert_eq!(cast(&f, OWNER, 3), Err(ERR_BUSY), "two per plugin");
    assert_eq!(connect(&f, OWNER, 4), Err(ERR_BUSY));
    assert_eq!(cast(&f, OTHER, 5), Err(ERR_BUSY), "two per X4");
    assert_eq!(f.used(), 16_384);
    close(&f, OWNER, a).unwrap();
    assert_eq!(f.used(), 8_192, "closing gives the answers' room back");
    assert!(cast(&f, OTHER, 6).is_ok());
    let _ = b;
    // The other way round: sockets fill the room, a broadcast is refused.
    let g = Fake::x4();
    connect(&g, OWNER, 1).unwrap();
    connect(&g, OTHER, 2).unwrap();
    assert_eq!(cast(&g, 9, 3), Err(ERR_BUSY));
}

#[test]
fn a_pool_full_of_net_bodies_refuses_a_broadcast_and_holds_nothing() {
    let pool = Rc::new(NetPool::new(49_152));
    assert!(pool.reserve(49_152 - 8_191));
    let f = Fake::with_pool(SockProfile::X4, pool.clone());
    assert_eq!(cast(&f, OWNER, 1), Err(ERR_NO_MEMORY));
    assert_eq!(pool.used(), 49_152 - 8_191);
    assert!(f.casts.borrow().is_empty());
    assert_eq!(f.table.held().0, 0);
    pool.release(1);
    assert!(cast(&f, OWNER, 1).is_ok());
}

#[test]
fn power_no_network_and_a_transport_that_cannot_begin_end_it_at_once() {
    let f = Fake::x4();
    f.power.set(Some(PowerEnd {
        reason: reason::SAVING,
        later: false,
    }));
    let id = cast(&f, OWNER, 1).unwrap();
    assert!(f.casts.borrow().is_empty(), "no byte went out");
    assert_eq!(
        drain_cast(&f, OWNER),
        std::vec![cev(id, 1, 2, reason::SAVING, 0, "")]
    );
    f.power.set(None);
    f.network.set(false);
    let id = cast(&f, OWNER, 2).unwrap();
    assert!(f.casts.borrow().is_empty());
    assert_eq!(
        drain_cast(&f, OWNER),
        std::vec![cev(id, 2, 1, reason::NO_NETWORK, 0, "")]
    );
    f.network.set(true);
    for (error, why) in [
        (ERR_NO_MEMORY, reason::NO_MEMORY),
        (paperpaw_native::host_glue::ERR_IO, reason::IO),
    ] {
        f.cast_fails.set(Some(error));
        let id = cast(&f, OWNER, 3).unwrap();
        assert_eq!(drain_cast(&f, OWNER), std::vec![cev(id, 3, 2, why, 0, "")]);
    }
    assert_eq!(f.used(), 0);
    assert_eq!(f.table.held().0, 0);
}

#[test]
fn malformed_answers_are_dropped_whole_and_do_not_count() {
    let f = Fake::x4();
    let id = cast(&f, OWNER, 1).unwrap();
    // Unknown ids are not a broadcast's.
    answered(&f, 999, b"x", b"y").unwrap();
    assert_eq!(f.table.take_dropped(), 0);
    // 1,024 bytes are delivered whole, 1,025 are dropped, not cut.
    answered(&f, id, b"10.0.0.1:1", &[1; 1_025]).unwrap();
    answered(&f, id, b"10.0.0.1:1", &[2; 1_024]).unwrap();
    // A `from` of 64 bytes is kept; one of 65 bytes, or not UTF-8, is dropped.
    let long = [b'a'; 64];
    answered(&f, id, &long, b"ok").unwrap();
    answered(&f, id, &[b'a'; 65], b"no").unwrap();
    answered(&f, id, &[0xff, 0xfe], b"no").unwrap();
    answered(&f, id, "é".repeat(33).as_bytes(), b"no").unwrap();
    assert_eq!(f.table.take_dropped(), 4, "and the count starts over");
    assert_eq!(f.table.take_dropped(), 0);
    let all = drain_cast(&f, OWNER);
    assert_eq!(
        all,
        std::vec![
            cev(id, 1, 0, 0, 1_024, "10.0.0.1:1"),
            cev(id, 1, 0, 0, 2, &"a".repeat(64))
        ]
    );
    // The dropped ones did not use up the eight: six more fit before `done`.
    for _ in 0..6 {
        answered(&f, id, b"10.0.0.2:2", b"x").unwrap();
    }
    assert_eq!(drain_cast(&f, OWNER).len(), 6 + 1, "six answers, then done");
    assert_eq!(f.used(), 0);
}

#[test]
fn the_eighth_answer_ends_the_broadcast_early() {
    let f = Fake::x4();
    let id = cast(&f, OWNER, 1).unwrap();
    for n in 0..7_u8 {
        answered(&f, id, b"10.0.0.1:1", &[n; 1_024]).unwrap();
    }
    assert!(f.closes.borrow().is_empty(), "seven: it still listens");
    answered(&f, id, b"10.0.0.1:1", &[7; 1_024]).unwrap();
    assert_eq!(
        f.closes.borrow().as_slice(),
        &[id],
        "the eighth: it stops listening"
    );
    // A ninth, and a late one, change nothing.
    answered(&f, id, b"10.0.0.1:1", b"late").unwrap();
    assert_eq!(f.used(), 8_192, "the answers are still to be told");
    let all = drain_cast(&f, OWNER);
    assert_eq!(all.len(), 9);
    assert_eq!(all[7], cev(id, 1, 0, 0, 1_024, "10.0.0.1:1"));
    assert_eq!(
        all[8],
        cev(id, 1, 1, 0, 0, ""),
        "answers first, then done 0"
    );
    assert_eq!(f.used(), 0);
    assert_eq!(f.demand.borrow().last(), Some(&false));
}

#[test]
fn answers_not_yet_told_keep_their_room_until_they_are() {
    let f = Fake::x4();
    let id = cast(&f, OWNER, 1).unwrap();
    answered(&f, id, b"10.0.0.1:1", b"abc").unwrap();
    close(&f, OWNER, id).unwrap();
    assert_eq!(f.used(), 8_192, "the answer is still to be told");
    assert_eq!(
        next_cast(&f, OWNER),
        Some(cev(id, 1, 0, 0, 3, "10.0.0.1:1"))
    );
    assert_eq!(data_of(&f, OWNER, id, 0).unwrap(), b"abc");
    event_delivered(&f, OWNER).unwrap();
    assert_eq!(f.used(), 8_192, "the `done` is still to be told");
    assert_eq!(drain_cast(&f, OWNER), std::vec![cev(id, 1, 1, 0, 0, "")]);
    assert_eq!(f.used(), 0);
    assert_eq!(f.table.held(), (0, None));
}

#[test]
fn whatever_ends_a_broadcast_gives_everything_back() {
    for how in 0..6 {
        let f = Fake::x4();
        let id = cast(&f, OWNER, 1).unwrap();
        answered(&f, id, b"10.0.0.1:1", b"x").unwrap();
        let (want_kind, want_reason) = match how {
            0 => {
                close(&f, OWNER, id).unwrap();
                (1, reason::NONE)
            }
            1 => {
                network_lost(&f).unwrap();
                (1, reason::NO_NETWORK)
            }
            2 => {
                screen_closed(&f, OWNER, 5).unwrap();
                (1, reason::SCREEN_CLOSED)
            }
            3 => {
                f.run.set(None);
                // The run of another plugin changes nothing.
                work_ended(&f, OTHER, 1).unwrap();
                screen_closed(&f, OWNER, 6).unwrap();
                assert_eq!(drain_cast(&f, OWNER).len(), 1, "only the answer");
                f.advance(1_500);
                tick(&f).unwrap();
                (1, reason::NONE)
            }
            4 => {
                let g = Fake::x4();
                g.screen.set(None);
                g.run.set(Some(4));
                let id = cast(&g, OWNER, 1).unwrap();
                work_ended(&g, OWNER, 4).unwrap();
                assert_eq!(
                    drain_cast(&g, OWNER),
                    std::vec![cev(id, 1, 1, reason::WORK_ENDED, 0, "")]
                );
                assert_eq!(g.used(), 0);
                (1, reason::NONE)
            }
            _ => {
                owner_stopped(&f, OWNER).unwrap();
                assert_eq!(next_cast(&f, OWNER), None, "no events: it is not running");
                assert_eq!(f.used(), 0);
                assert_eq!(f.closes.borrow().as_slice(), &[id]);
                continue;
            }
        };
        if how == 4 {
            continue;
        }
        let all = drain_cast(&f, OWNER);
        let last = all.last().unwrap();
        assert_eq!(
            (last.kind, last.reason),
            (want_kind, want_reason),
            "how {how}"
        );
        assert_eq!(f.used(), 0, "how {how}");
        owner_stopped(&f, OWNER).unwrap();
        assert_eq!(f.table.held(), (0, None), "how {how}");
        assert_eq!(f.demand.borrow().last(), Some(&false), "how {how}");
    }
}

#[test]
fn socket_and_broadcast_events_come_in_the_order_they_happened() {
    let f = Fake::x4();
    let s = connect(&f, OWNER, 1).unwrap();
    let b = cast(&f, OWNER, 2).unwrap();
    answered(&f, b, b"10.0.0.1:1", b"a").unwrap();
    connected(&f, s).unwrap();
    answered(&f, b, b"10.0.0.2:1", b"b").unwrap();
    let mut seen = Vec::new();
    let mut out = std::vec![0; 256];
    while let Some(used) = next_event(&f, OWNER, &mut out).unwrap() {
        seen.push(match wire::record_decode_one(&out[..used]).unwrap() {
            Record::EventSocket(e) => (e.socket, e.kind),
            Record::EventBroadcast(e) => (e.socket, 10 + e.kind),
            other => panic!("{other:?}"),
        });
        event_delivered(&f, OWNER).unwrap();
    }
    assert_eq!(seen, std::vec![(b, 10), (s, 0), (b, 10)]);
}

#[test]
fn an_allocation_that_fails_anywhere_in_a_broadcast_leaves_nothing_held() {
    for nth in 1..=12 {
        let f = Fake::x4();
        fail_nth(nth);
        let started = cast(&f, OWNER, 1);
        if let Ok(id) = started {
            let _ = answered(&f, id, b"10.0.0.1:1", b"abc");
            let _ = next_cast(&f, OWNER);
            let _ = event_delivered(&f, OWNER);
        } else {
            assert_eq!(started, Err(ERR_NO_MEMORY), "gate {nth}");
            assert_eq!(f.used(), 0, "gate {nth}");
            assert!(f.casts.borrow().is_empty(), "gate {nth}");
        }
        fail_nth(0);
        owner_stopped(&f, OWNER).unwrap();
        assert_eq!(f.used(), 0, "gate {nth}");
        assert_eq!(f.table.held(), (0, None), "gate {nth}");
        assert!(cast(&f, OWNER, 2).is_ok(), "gate {nth}");
    }
}

#[test]
fn two_plugins_broadcasts_keep_their_answers_events_and_ends_apart() {
    let f = Fake::new(SockProfile::OTHER, 0);
    let a = cast(&f, OWNER, 1).unwrap();
    let b = cast(&f, OTHER, 2).unwrap();
    answered(&f, a, b"10.0.0.1:1", b"for a").unwrap();
    answered(&f, b, b"10.0.0.2:2", b"for b").unwrap();
    // Each plugin hears only its own answer, and reads only its own bytes.
    assert_eq!(next_cast(&f, OWNER), Some(cev(a, 1, 0, 0, 5, "10.0.0.1:1")));
    assert_eq!(data_of(&f, OWNER, a, 0).unwrap(), b"for a");
    assert_eq!(data_of(&f, OWNER, b, 0), Err(ERR_NOT_FOUND));
    assert_eq!(next_cast(&f, OTHER), Some(cev(b, 2, 0, 0, 5, "10.0.0.2:2")));
    assert_eq!(data_of(&f, OTHER, b, 0).unwrap(), b"for b");
    assert_eq!(data_of(&f, OTHER, a, 0), Err(ERR_NOT_FOUND));
    // One plugin cannot close the other's broadcast; stopping one leaves the
    // other running with its answers.
    close(&f, OTHER, a).unwrap();
    assert!(f.closes.borrow().is_empty(), "a foreign close does nothing");
    event_delivered(&f, OWNER).unwrap();
    event_delivered(&f, OTHER).unwrap();
    owner_stopped(&f, OWNER).unwrap();
    assert_eq!(drain_cast(&f, OWNER), std::vec![]);
    answered(&f, b, b"10.0.0.3:3", b"more").unwrap();
    assert_eq!(
        drain_cast(&f, OTHER),
        std::vec![cev(b, 2, 0, 0, 4, "10.0.0.3:3")]
    );
    assert_eq!(f.closes.borrow().as_slice(), &[a]);
}
