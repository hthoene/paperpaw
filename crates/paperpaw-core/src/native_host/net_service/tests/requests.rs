//! The request table: the pool, the queue, the ends, the answer hold, the
//! events.
use super::*;
use paperpaw_native::host_glue::{ERR_IO, ERR_NO_MEMORY, ERR_NOT_FOUND};

fn done(id: u32, token: u32, status: u32, why: u32, flags: u32, body_len: u32) -> Event {
    Event::Done {
        id,
        token,
        status,
        reason: why,
        flags,
        body_len,
    }
}

fn answer_of(fake: &Fake, owner: u64, id: u32) -> Result<wire::ServiceNetAnswer<'static>, i32> {
    let mut out = std::vec![0; 1536];
    let used = table::answer(fake, owner, id, &mut out)?;
    let Record::ServiceNetAnswer(a) = wire::record_decode_one(&out[..used]).unwrap() else {
        panic!()
    };
    // Copy the strings out of the scratch buffer for the test.
    let leak = |s: &str| -> &'static str { std::boxed::Box::leak(s.to_string().into_boxed_str()) };
    Ok(wire::ServiceNetAnswer {
        status: a.status,
        flags: a.flags,
        body_len: a.body_len,
        etag: leak(a.etag),
        content_type: leak(a.content_type),
        final_url: leak(a.final_url),
    })
}

fn piece(fake: &Fake, owner: u64, id: u32, offset: u32) -> Result<Vec<u8>, i32> {
    let mut out = std::vec![0; 4096];
    let used = table::body(fake, owner, id, offset, &mut out)?;
    let Record::ServiceNetBytes(b) = wire::record_decode_one(&out[..used]).unwrap() else {
        panic!()
    };
    Ok(b.data.to_vec())
}

#[test]
fn the_pool_is_reserved_before_use_and_a_request_waits_for_room() {
    let fake = Fake::x4();
    let (ia, ib) = (big(1, 45_056), big(2, 4_000));
    let (la, lb) = (ia.len() as u32, ib.len() as u32);
    let a = request(&fake, OWNER, &ia).unwrap();
    assert_eq!(fake.pool_used(), 45_056 + la, "input and body reserved");
    let b = request(&fake, OWNER, &ib).unwrap();
    assert_eq!(*fake.started.borrow(), [a], "b waits for room");
    assert_eq!(fake.pool_used(), 45_056 + la + lb, "its input is held");
    // a ends with 2 bytes: the pool keeps only those, its input and the rest
    // of its body are free; a held answer counts until its callback returns.
    finish(&fake, a, 200, b"hi");
    assert_eq!(
        fake.pool_used(),
        2 + lb + 4_000,
        "b started on the room that came free"
    );
    assert_eq!(*fake.started.borrow(), [a, b]);
    assert_eq!(drain(&fake, OWNER), [done(a, 1, 200, 0, 0, 2)]);
    assert_eq!(fake.pool_used(), lb + 4_000);
    finish(&fake, b, 200, b"");
    drain(&fake, OWNER);
    assert_eq!(fake.pool_used(), 0, "everything is back");
}

#[test]
fn the_queue_is_strictly_first_in_first_out() {
    let fake = Fake::x4();
    let a = request(&fake, OWNER, &big(1, 40_000)).unwrap();
    let b = request(&fake, OWNER, &big(2, 20_000)).unwrap();
    let c = request(&fake, OTHER, &big(3, 1_000)).unwrap();
    // c would fit in the room left, but b waits for room before it.
    assert_eq!(*fake.started.borrow(), [a]);
    finish(&fake, a, 200, b"x");
    drain(&fake, OWNER);
    assert_eq!(
        *fake.started.borrow(),
        [a, b, c],
        "in the order they were started"
    );
}

#[test]
fn the_input_of_a_request_is_held_in_the_pool_at_its_exact_size() {
    let fake = Fake::x4();
    let one = get(1);
    let _id = request(&fake, OWNER, &one).unwrap();
    assert_eq!(fake.pool_used(), 45_056 + one.len() as u32);
    table::cancel(&fake, OWNER, 1).unwrap();
    assert_eq!(fake.pool_used(), 0, "input and body are back when it ends");
    // A queue of requests whose inputs fill the pool: the next call is
    // refused and holds nothing. Each carries a 1,024-byte header value.
    let value = "v".repeat(1_000);
    let long: Vec<u8> = {
        let h = Record::ServiceNetHeader(wire::ServiceNetHeader {
            name: "X-A",
            value: &value,
        });
        let mut h4 = std::vec![rec_head(9)];
        h4.extend((0..3).map(|_| h));
        batch(&h4)
    };
    let profile = NetProfile {
        pool: 10_000,
        max_body: 100,
        memory_bodies: 8,
        connections: 8,
        cable: false,
    };
    let fake = Fake::new(profile);
    let long_len = long.len() as u32;
    for owner in 10..13 {
        request(&fake, owner, &long).unwrap();
    }
    assert_eq!(fake.pool_used(), 3 * (long_len + 100));
    // The fourth input does not fit: refused, and the pool is as it was.
    assert_eq!(
        request(&fake, 13, &long),
        Err(paperpaw_native::host_glue::ERR_NO_MEMORY)
    );
    assert_eq!(fake.pool_used(), 3 * (long_len + 100));
    assert_eq!(fake.started.borrow().len(), 3);
}

fn rec_head(token: u32) -> Record<'static> {
    Record::ServiceNetRequest(head(token, "http://a.b/"))
}

#[test]
fn two_memory_bodies_exist_at_once_counting_the_undelivered() {
    let fake = Fake::x4();
    let ids: Vec<u32> = (1..=3)
        .map(|t| request(&fake, OWNER, &big(t, 100)).unwrap())
        .collect();
    assert_eq!(
        *fake.started.borrow(),
        ids[..2],
        "the third waits for a body slot"
    );
    finish(&fake, ids[0], 200, b"x");
    // Finished but not delivered: still counted.
    assert_eq!(*fake.started.borrow(), ids[..2]);
    assert_eq!(drain(&fake, OWNER), [done(ids[0], 1, 200, 0, 0, 1)]);
    assert_eq!(
        *fake.started.borrow(),
        ids[..],
        "its slot is free once delivered"
    );
}

#[test]
fn saving_requests_need_a_buffer_not_a_body_and_share_two_connections() {
    let fake = Fake::x4();
    let paths = ["a.bin", "b.bin", "c.bin"];
    let ids: Vec<u32> = paths
        .iter()
        .enumerate()
        .map(|(n, p)| request(&fake, OWNER, &save_to(n as u32, 0, p)).unwrap())
        .collect();
    assert_eq!(
        *fake.started.borrow(),
        ids[..2],
        "two connections on the X4"
    );
    let inputs: u32 = [("a.bin", 0), ("b.bin", 1), ("c.bin", 2)]
        .iter()
        .map(|(p, n)| save_to(*n, 0, p).len() as u32)
        .sum();
    assert_eq!(
        fake.pool_used(),
        2 * IO_BUFFER + inputs,
        "a 4 KiB buffer each, and the inputs"
    );
    // A saving request does not use a memory body slot: two bodies run too.
    let body = request(&fake, OTHER, &big(9, 10)).unwrap();
    assert_eq!(fake.started.borrow().len(), 2, "the connections are full");
    finished(
        &fake,
        ids[0],
        Finished {
            status: 200,
            reason: 0,
            saved: true,
            truncated: false,
            etag: "\"v1\"",
            content_type: "application/zip",
            final_url: "https://example.org/z",
            body: std::vec![1, 2, 3],
        },
    )
    .unwrap();
    assert_eq!(
        *fake.started.borrow(),
        [ids[0], ids[1], ids[2]],
        "c is next in line"
    );
    let events = drain(&fake, OWNER);
    assert_eq!(
        events,
        [done(ids[0], 0, 200, 0, DONE_SAVED, 0)],
        "a saved body is in the file"
    );
    assert!(
        !fake.started.borrow().contains(&body),
        "the body request waits behind c"
    );
    assert!(
        fake.released.borrow().iter().any(|r| r.2 == "a.bin"),
        "the claim is freed"
    );
}

#[test]
fn every_request_ends_exactly_once() {
    let fake = Fake::x4();
    let id = request(&fake, OWNER, &get(5)).unwrap();
    table::cancel(&fake, OWNER, 5).unwrap();
    table::cancel(&fake, OWNER, 5).unwrap();
    assert_eq!(*fake.aborted.borrow(), [id], "the transfer is dropped once");
    // An answer that comes later is ignored.
    finish(&fake, id, 200, b"late");
    assert_eq!(
        drain(&fake, OWNER),
        [done(id, 5, 0, reason::CANCELLED, 0, 0)]
    );
    finish(&fake, id, 200, b"later");
    assert!(drain(&fake, OWNER).is_empty());
    // A timeout and then an answer, then a second answer.
    let id = request(&fake, OWNER, &get(6)).unwrap();
    fake.advance(30_000);
    tick(&fake).unwrap();
    finish(&fake, id, 200, b"x");
    finish(&fake, id, 200, b"y");
    assert_eq!(drain(&fake, OWNER), [done(id, 6, 0, reason::TIMEOUT, 0, 0)]);
    // An answer and then a cancel.
    let id = request(&fake, OWNER, &get(7)).unwrap();
    finish(&fake, id, 204, b"");
    table::cancel(&fake, OWNER, 7).unwrap();
    tick(&fake).unwrap();
    assert_eq!(drain(&fake, OWNER), [done(id, 7, 204, 0, 0, 0)]);
}

#[test]
fn cancel_ends_only_the_plugins_own_requests_with_the_token() {
    let fake = Fake::x4();
    let a = request(&fake, OWNER, &get(5)).unwrap();
    let b = request(&fake, OWNER, &get(5)).unwrap();
    let c = request(&fake, OWNER, &get(6)).unwrap();
    let d = request(&fake, OTHER, &get(5)).unwrap();
    table::cancel(&fake, OWNER, 5).unwrap();
    assert_eq!(
        drain(&fake, OWNER),
        [done(a, 5, 0, 8, 0, 0), done(b, 5, 0, 8, 0, 0)],
        "one event each, in order"
    );
    assert!(drain(&fake, OTHER).is_empty());
    assert!(!fake.aborted.borrow().contains(&c) && !fake.aborted.borrow().contains(&d));
    table::cancel(&fake, OWNER, 99).unwrap();
}

#[test]
fn time_limits_count_from_the_start_and_a_moving_file_never_ends() {
    let fake = Fake::x4();
    let mem = request(&fake, OWNER, &get(1)).unwrap();
    fake.advance(29_999);
    tick(&fake).unwrap();
    assert!(drain(&fake, OWNER).is_empty(), "30,000 ms is the default");
    fake.advance(1);
    tick(&fake).unwrap();
    assert_eq!(
        drain(&fake, OWNER),
        [done(mem, 1, 0, reason::TIMEOUT, 0, 0)]
    );
    // A file: 60,000 ms without a byte moving.
    let file = request(&fake, OWNER, &save_to(2, 0, "f.bin")).unwrap();
    for _ in 0..4 {
        fake.advance(59_999);
        table::progress(&fake, file, 200, 1, None).unwrap();
        tick(&fake).unwrap();
    }
    assert!(drain(&fake, OWNER).is_empty(), "it moves, so it never ends");
    fake.advance(60_000);
    tick(&fake).unwrap();
    assert_eq!(
        drain(&fake, OWNER),
        [done(file, 2, 0, reason::TIMEOUT, 0, 0)]
    );
    // The plugin's own timeout.
    let mut h = head(3, "http://a.b/");
    h.timeout_ms = 1_000;
    let short = request(&fake, OWNER, &batch(&[Record::ServiceNetRequest(h)])).unwrap();
    fake.advance(1_000);
    tick(&fake).unwrap();
    assert_eq!(
        drain(&fake, OWNER),
        [done(short, 3, 0, reason::TIMEOUT, 0, 0)]
    );
}

#[test]
fn a_request_that_waits_for_room_times_out_without_holding_more() {
    let fake = Fake::x4();
    let a = request(&fake, OWNER, &big(1, 45_056)).unwrap();
    let b = request(&fake, OTHER, &get(2)).unwrap();
    assert_eq!(*fake.started.borrow(), [a]);
    fake.advance(30_000);
    tick(&fake).unwrap();
    // Both ran out of time (the clock counts from each one's start).
    assert_eq!(drain(&fake, OTHER), [done(b, 2, 0, reason::TIMEOUT, 0, 0)]);
    assert_eq!(drain(&fake, OWNER), [done(a, 1, 0, reason::TIMEOUT, 0, 0)]);
    assert_eq!(fake.pool_used(), 0);
}

#[test]
fn a_request_without_a_network_waits_30_seconds_then_ends() {
    let fake = Fake::x4();
    fake.network.set(false);
    let mut h = head(1, "http://a.b/");
    h.timeout_ms = 600_000;
    let id = request(&fake, OWNER, &batch(&[Record::ServiceNetRequest(h)])).unwrap();
    assert!(fake.started.borrow().is_empty());
    assert_eq!(
        *fake.demand.borrow(),
        [true],
        "the manager is asked for a network"
    );
    fake.advance(29_999);
    tick(&fake).unwrap();
    assert!(drain(&fake, OWNER).is_empty());
    // The network comes up in time: it starts.
    fake.network.set(true);
    tick(&fake).unwrap();
    assert_eq!(*fake.started.borrow(), [id]);
    // And the other way: waiting too long.
    let fake = Fake::x4();
    fake.network.set(false);
    let mut h = head(1, "http://a.b/");
    h.timeout_ms = 600_000;
    let id = request(&fake, OWNER, &batch(&[Record::ServiceNetRequest(h)])).unwrap();
    fake.advance(30_000);
    tick(&fake).unwrap();
    assert_eq!(
        drain(&fake, OWNER),
        [done(id, 1, 0, reason::NO_NETWORK, 0, 0)]
    );
    assert_eq!(*fake.demand.borrow(), [true, false], "idle again");
}

#[test]
fn a_network_that_goes_away_ends_the_running_requests() {
    let fake = Fake::x4();
    let id = request(&fake, OWNER, &get(1)).unwrap();
    network_lost(&fake).unwrap();
    assert_eq!(
        drain(&fake, OWNER),
        [done(id, 1, 0, reason::NO_NETWORK, 0, 0)]
    );
    assert_eq!(*fake.aborted.borrow(), [id]);
}

#[test]
fn power_saving_ends_a_request_before_a_byte_goes_out() {
    for (end, flags) in [
        (
            PowerEnd {
                reason: reason::OFFLINE,
                later: true,
            },
            DONE_LATER,
        ),
        (
            PowerEnd {
                reason: reason::SAVING,
                later: true,
            },
            DONE_LATER,
        ),
        (
            PowerEnd {
                reason: reason::NETWORK_OFF,
                later: false,
            },
            0,
        ),
    ] {
        let fake = Fake::x4();
        fake.power.set(Some(end));
        let id = request(&fake, OWNER, &get(1)).unwrap();
        assert!(fake.started.borrow().is_empty());
        assert_eq!(drain(&fake, OWNER), [done(id, 1, 0, end.reason, flags, 0)]);
        assert_eq!(fake.pool_used(), 0);
    }
}

#[test]
fn a_transport_that_cannot_begin_ends_the_request_with_a_reason() {
    let fake = Fake::x4();
    fake.start_fails.set(Some(ERR_NO_MEMORY));
    let a = request(&fake, OWNER, &get(1)).unwrap();
    fake.start_fails.set(Some(ERR_IO));
    let b = request(&fake, OWNER, &get(2)).unwrap();
    assert_eq!(
        drain(&fake, OWNER),
        [
            done(a, 1, 0, reason::TOO_LARGE, 0, 0),
            done(b, 2, 0, reason::REFUSED, 0, 0)
        ]
    );
    assert_eq!(fake.pool_used(), 0, "nothing stays reserved");
}

#[test]
fn the_answer_is_readable_only_during_its_callback() {
    let fake = Fake::x4();
    let id = request(&fake, OWNER, &get(1)).unwrap();
    let body: Vec<u8> = (0..10_000_u32).map(|n| (n % 251) as u8).collect();
    finished(
        &fake,
        id,
        Finished {
            status: 200,
            reason: 0,
            saved: false,
            truncated: false,
            etag: "\"abc\"",
            content_type: "text/html",
            final_url: "https://example.org/final",
            body: body.clone(),
        },
    )
    .unwrap();
    // Told by nothing yet: not readable.
    assert_eq!(answer_of(&fake, OWNER, id).err(), Some(ERR_NOT_FOUND));
    assert_eq!(next(&fake, OWNER), Some(done(id, 1, 200, 0, 0, 10_000)));
    let a = answer_of(&fake, OWNER, id).unwrap();
    assert_eq!(
        (
            a.status,
            a.flags,
            a.body_len,
            a.etag,
            a.content_type,
            a.final_url
        ),
        (
            200,
            0,
            10_000,
            "\"abc\"",
            "text/html",
            "https://example.org/final"
        )
    );
    // Another plugin's id is not found, whatever it is.
    assert_eq!(answer_of(&fake, OTHER, id).err(), Some(ERR_NOT_FOUND));
    assert_eq!(piece(&fake, OTHER, id, 0), Err(ERR_NOT_FOUND));
    // The body in pieces of 3,840: 18 calls for 64 KiB, 3 for 10,000 bytes.
    let mut got = Vec::new();
    let mut offset = 0;
    let mut calls = 0;
    loop {
        let part = piece(&fake, OWNER, id, offset).unwrap();
        calls += 1;
        assert!(part.len() <= BODY_PIECE);
        if part.is_empty() {
            break;
        }
        offset += part.len() as u32;
        got.extend_from_slice(&part);
    }
    assert_eq!((got, calls), (body, 4));
    assert_eq!(piece(&fake, OWNER, id, 10_000).unwrap(), Vec::<u8>::new());
    assert_eq!(piece(&fake, OWNER, id, u32::MAX).unwrap(), Vec::<u8>::new());
    assert_eq!(
        piece(&fake, OWNER, id, 9_999).unwrap().len(),
        1,
        "limit and one past"
    );
    event_delivered(&fake, OWNER).unwrap();
    assert_eq!(answer_of(&fake, OWNER, id).err(), Some(ERR_NOT_FOUND));
    assert_eq!(piece(&fake, OWNER, id, 0), Err(ERR_NOT_FOUND));
    assert_eq!(fake.pool_used(), 0);
}

#[test]
fn a_failure_keeps_an_error_body_of_at_most_4096_and_none_without_a_status() {
    let fake = Fake::x4();
    fake.storage.set(true);
    // A saving request whose answer is not 2xx: the error body is kept in
    // memory, cut at 4,096. (Not saved: reason 0 with its status.)
    let a = request(&fake, OWNER, &save_to(1, 0, "e.bin")).unwrap();
    let b = request(&fake, OWNER, &big(2, 100)).unwrap();
    // The transport cuts at the cap (a buffer above it is refused, see below)
    // and says so.
    finished(
        &fake,
        a,
        Finished {
            status: 500,
            reason: 0,
            saved: false,
            truncated: true,
            etag: "",
            content_type: "",
            final_url: "",
            body: std::vec![7; 4_096],
        },
    )
    .unwrap();
    // A failure without a status holds no body, whatever the transport kept.
    finished(
        &fake,
        b,
        Finished {
            status: 0,
            reason: reason::DNS,
            saved: false,
            truncated: false,
            etag: "",
            content_type: "",
            final_url: "",
            body: std::vec![1; 10],
        },
    )
    .unwrap();
    assert_eq!(
        drain(&fake, OWNER),
        [
            done(a, 1, 500, 0, DONE_TRUNCATED, 4_096),
            done(b, 2, 0, reason::DNS, 0, 0)
        ]
    );
}

#[test]
fn a_memory_body_is_cut_at_max_body_and_says_so() {
    let fake = Fake::x4();
    let id = request(&fake, OWNER, &big(1, 100)).unwrap();
    finished(
        &fake,
        id,
        Finished {
            status: 200,
            reason: 0,
            saved: false,
            truncated: true,
            etag: "",
            content_type: "",
            final_url: "",
            body: std::vec![1; 100],
        },
    )
    .unwrap();
    assert_eq!(
        drain(&fake, OWNER),
        [done(id, 1, 200, 0, DONE_TRUNCATED, 100)]
    );
    // Exactly max_body is not cut.
    let id = request(&fake, OWNER, &big(1, 100)).unwrap();
    finish(&fake, id, 200, &[1; 100]);
    assert_eq!(drain(&fake, OWNER), [done(id, 1, 200, 0, 0, 100)]);
}

#[test]
fn events_come_in_the_order_the_requests_ended() {
    let fake = Fake::x4();
    let a = request(&fake, OWNER, &big(1, 100)).unwrap();
    let b = request(&fake, OWNER, &big(2, 100)).unwrap();
    finish(&fake, b, 200, b"");
    finish(&fake, a, 200, b"");
    assert_eq!(
        drain(&fake, OWNER),
        [done(b, 2, 200, 0, 0, 0), done(a, 1, 200, 0, 0, 0)]
    );
    assert!(drain(&fake, OWNER).is_empty(), "each is told once");
}

#[test]
fn progress_is_first_then_five_percent_or_two_seconds_newest_wins() {
    let fake = Fake::x4();
    let mut h = head(1, "http://a.b/");
    h.flags = 1;
    let id = request(&fake, OWNER, &batch(&[Record::ServiceNetRequest(h)])).unwrap();
    let prog = |received, total| table::progress(&fake, id, 200, received, total).unwrap();
    prog(100, Some(1_000));
    prog(120, Some(1_000)); // 2 %: no
    prog(140, Some(1_000)); // 4 %: no
    assert_eq!(
        drain_progress(&fake),
        [Event::Progress {
            id,
            received: 100,
            total: 1_000,
            flags: 1
        }]
    );
    prog(150, Some(1_000)); // 5 % of 1,000: yes
    prog(300, Some(1_000)); // coalesced: the newest replaces
    assert_eq!(
        drain_progress(&fake),
        [Event::Progress {
            id,
            received: 300,
            total: 1_000,
            flags: 1
        }]
    );
    // Two seconds with more bytes, no total known.
    fake.advance(2_000);
    prog(301, None);
    assert_eq!(
        drain_progress(&fake),
        [Event::Progress {
            id,
            received: 301,
            total: 0,
            flags: 0
        }]
    );
    // A non-2xx answer never reports progress; nor after the end.
    table::progress(&fake, id, 404, 1_000, None).unwrap();
    finish(&fake, id, 200, b"");
    prog(900, None);
    assert_eq!(drain(&fake, OWNER), [done(id, 1, 200, 0, 0, 0)]);
}

fn drain_progress(fake: &Fake) -> Vec<Event> {
    let mut all = Vec::new();
    while let Some(e) = next(fake, OWNER) {
        all.push(e);
    }
    all
}

#[test]
fn a_request_without_the_progress_flag_has_none() {
    let fake = Fake::x4();
    let id = request(&fake, OWNER, &get(1)).unwrap();
    table::progress(&fake, id, 200, 10, Some(100)).unwrap();
    assert!(next(&fake, OWNER).is_none());
}

#[test]
fn the_end_of_a_work_run_ends_its_requests_with_reason_14() {
    let fake = Fake::x4();
    fake.run.set(Some(3));
    let in_run = request(&fake, OWNER, &get(1)).unwrap();
    fake.run.set(None);
    let user = request(&fake, OWNER, &get(2)).unwrap();
    work_ended(&fake, OWNER, 3).unwrap();
    work_ended(&fake, OWNER, 3).unwrap();
    assert_eq!(
        drain(&fake, OWNER),
        [done(in_run, 1, 0, reason::WORK_ENDED, 0, 0)]
    );
    assert!(!fake.aborted.borrow().contains(&user));
}

#[test]
fn a_stopped_plugin_loses_its_requests_without_events_and_returns_its_memory() {
    let fake = Fake::x4();
    fake.storage.set(true);
    let a = request(&fake, OWNER, &save_to(1, 0, "a.bin")).unwrap();
    let _held = request(&fake, OWNER, &big(2, 1_000)).unwrap();
    let kept = request(&fake, OTHER, &get(3)).unwrap();
    finish(&fake, _held, 200, b"body");
    assert_eq!(next(&fake, OWNER), Some(done(_held, 2, 200, 0, 0, 4)));
    owner_stopped(&fake, OWNER).unwrap();
    owner_stopped(&fake, OWNER).unwrap();
    assert!(next(&fake, OWNER).is_none(), "no events: it is not running");
    assert_eq!(*fake.aborted.borrow(), [a]);
    assert!(fake.claims.borrow().is_empty());
    assert_eq!(
        fake.pool_used(),
        45_056 + get(3).len() as u32,
        "only the other plugin's request holds a reservation"
    );
    finish(&fake, kept, 200, b"");
    assert_eq!(drain(&fake, OTHER), [done(kept, 3, 200, 0, 0, 0)]);
    assert_eq!(fake.pool_used(), 0);
}

#[test]
fn the_network_manager_hears_demand_and_idle_once_each() {
    let fake = Fake::x4();
    let a = request(&fake, OWNER, &get(1)).unwrap();
    let b = request(&fake, OWNER, &get(2)).unwrap();
    assert_eq!(*fake.demand.borrow(), [true]);
    finish(&fake, a, 200, b"");
    assert_eq!(*fake.demand.borrow(), [true]);
    finish(&fake, b, 200, b"");
    assert_eq!(*fake.demand.borrow(), [true, false]);
}

#[test]
fn the_http_done_failures_and_a_busy_table_never_panic() {
    // A reentrant call finds the table borrowed.
    let fake = Fake::x4();
    let _guard = fake.table.lock().unwrap();
    assert_eq!(
        request(&fake, OWNER, &get(1)),
        Err(paperpaw_native::host_glue::ERR_BUSY)
    );
    assert_eq!(tick(&fake), Err(paperpaw_native::host_glue::ERR_BUSY));
    assert_eq!(
        next_event(&fake, OWNER, &mut [0; 64]),
        Err(paperpaw_native::host_glue::ERR_BUSY)
    );
}

#[test]
fn an_answer_that_cannot_be_held_ends_with_too_large_and_leaks_nothing() {
    for nth in 1..=4 {
        let fake = Fake::x4();
        let id = request(&fake, OWNER, &get(1)).unwrap();
        FAIL_NTH.with(|n| n.set(nth));
        finish(&fake, id, 200, b"body");
        FAIL_NTH.with(|n| n.set(0));
        let events = drain(&fake, OWNER);
        assert_eq!(events.len(), 1, "allocation {nth}");
        let Event::Done {
            reason: why,
            body_len,
            ..
        } = events[0]
        else {
            panic!()
        };
        assert!(
            (why == reason::TOO_LARGE && body_len == 0) || (why == 0 && body_len == 4),
            "allocation {nth}"
        );
        assert_eq!(fake.pool_used(), 0, "allocation {nth}");
    }
}

#[test]
fn a_committed_save_stays_a_success_when_its_head_cannot_be_held() {
    for nth in 1..=4 {
        let fake = Fake::x4();
        let id = request(&fake, OWNER, &save_to(0, 0, "a.bin")).unwrap();
        FAIL_NTH.with(|n| n.set(nth));
        finished(
            &fake,
            id,
            Finished {
                status: 200,
                reason: 0,
                saved: true,
                truncated: false,
                etag: "\"v1\"",
                content_type: "application/zip",
                final_url: "https://example.org/z",
                body: Vec::new(),
            },
        )
        .unwrap();
        FAIL_NTH.with(|n| n.set(0));
        let events = drain(&fake, OWNER);
        assert_eq!(events.len(), 1, "allocation {nth}");
        let Event::Done {
            reason: why,
            status,
            flags,
            ..
        } = events[0]
        else {
            panic!()
        };
        assert_eq!(
            (why, status, flags),
            (0, 200, DONE_SAVED),
            "allocation {nth}"
        );
    }
}

#[test]
fn a_held_body_is_charged_by_capacity_and_an_oversized_buffer_is_refused() {
    let answer = |fake: &Fake, id: u32, status: u32, body: Vec<u8>, saved: bool| {
        finished(
            fake,
            id,
            Finished {
                status,
                reason: 0,
                saved,
                truncated: false,
                etag: "",
                content_type: "",
                final_url: "",
                body,
            },
        )
        .unwrap();
    };
    let used_by_held = |body: Vec<u8>, saved: bool| {
        let fake = Fake::x4();
        let id = request(&fake, OWNER, &big(1, 5_000)).unwrap();
        answer(&fake, id, 200, body, saved);
        let used = fake.pool_used();
        let events = drain(&fake, OWNER);
        (used, events)
    };
    // A short body in a roomy buffer holds the whole buffer.
    let mut roomy = Vec::with_capacity(900);
    roomy.extend_from_slice(b"abc");
    let (used, events) = used_by_held(roomy, false);
    assert_eq!(used, 900, "capacity, not length");
    assert!(matches!(events[0], Event::Done { body_len: 3, .. }));
    // A truncated or cleared buffer keeps its capacity and stays charged.
    let mut cut = Vec::with_capacity(4_000);
    cut.extend_from_slice(&[7; 4_000]);
    cut.truncate(10);
    assert_eq!(used_by_held(cut, false).0, 4_000);
    // A saved answer's body is dropped: nothing stays charged.
    let mut cleared = Vec::with_capacity(500);
    cleared.push(1);
    assert_eq!(used_by_held(cleared, true).0, 0);
    // A buffer above the reservation is the transport's fault: refused, and
    // nothing is held.
    let (used, events) = used_by_held(Vec::with_capacity(5_001), false);
    assert_eq!(used, 0);
    assert!(matches!(
        events[0],
        Event::Done { reason: r, status: 0, body_len: 0, .. } if r == reason::TOO_LARGE
    ));
    // An error answer with a roomier buffer keeps its real status: its bytes
    // are copied to an exact-size buffer, which is what the pool is charged.
    let fake = Fake::x4();
    let id = request(&fake, OWNER, &big(1, 100)).unwrap();
    let mut roomy = Vec::with_capacity(8_192);
    roomy.extend_from_slice(&[3; 40]);
    answer(&fake, id, 404, roomy, false);
    assert_eq!(fake.pool_used(), 40);
    assert_eq!(drain(&fake, OWNER), [done(id, 1, 404, 0, 0, 40)]);
    // Bytes beyond the cap (100 here) are cut and said so; the status stays.
    let id = request(&fake, OWNER, &big(1, 100)).unwrap();
    let mut over = Vec::with_capacity(8_192);
    over.extend_from_slice(&[3; 5_000]);
    answer(&fake, id, 404, over, false);
    assert_eq!(
        drain(&fake, OWNER),
        [done(id, 1, 404, 0, DONE_TRUNCATED, 100)]
    );
    assert_eq!(fake.pool_used(), 0);
    // A failed copy (allocation 1) loses the body, a failed head (allocation 2)
    // loses the metadata; the status stays either way.
    for nth in 1..=2 {
        let id = request(&fake, OWNER, &big(1, 100)).unwrap();
        let mut roomy = Vec::with_capacity(8_192);
        roomy.extend_from_slice(&[3; 40]);
        FAIL_NTH.with(|n| n.set(nth));
        answer(&fake, id, 404, roomy, false);
        FAIL_NTH.with(|n| n.set(0));
        let events = drain(&fake, OWNER);
        let Event::Done {
            status,
            reason: why,
            ..
        } = events[0]
        else {
            panic!()
        };
        assert_eq!((status, why), (404, 0), "allocation {nth}");
        assert_eq!(fake.pool_used(), 0, "allocation {nth}");
    }
    // A success body in a buffer above the reservation is still refused.
    let id = request(&fake, OWNER, &big(1, 100)).unwrap();
    answer(&fake, id, 200, Vec::with_capacity(101), false);
    assert_eq!(fake.pool_used(), 0);
    // ... while a buffer that fits the reservation is charged as it is.
    let id = request(&fake, OWNER, &big(2, 100)).unwrap();
    answer(&fake, id, 500, Vec::with_capacity(100), false);
    assert_eq!(fake.pool_used(), 100);
}

#[test]
fn a_memory_requests_error_body_keeps_at_most_min_4096_and_max_body() {
    let fake = Fake::x4();
    let id = request(&fake, OWNER, &big(1, 100)).unwrap();
    assert_eq!(fake.pool_used() - 100, big(1, 100).len() as u32);
    finished(
        &fake,
        id,
        Finished {
            status: 503,
            reason: 0,
            saved: false,
            truncated: true,
            etag: "",
            content_type: "",
            final_url: "",
            body: std::vec![9; 100],
        },
    )
    .unwrap();
    assert_eq!(
        drain(&fake, OWNER),
        [done(id, 1, 503, 0, DONE_TRUNCATED, 100)]
    );
}

#[test]
fn a_committed_save_stays_a_success_when_its_head_is_overlong() {
    let fake = Fake::x4();
    let id = request(&fake, OWNER, &save_to(0, 0, "a.bin")).unwrap();
    finished(
        &fake,
        id,
        Finished {
            status: 200,
            reason: 0,
            saved: true,
            truncated: false,
            etag: &"e".repeat(257),
            content_type: "application/zip",
            final_url: "https://example.org/z",
            body: Vec::new(),
        },
    )
    .unwrap();
    assert_eq!(drain(&fake, OWNER), [done(id, 0, 200, 0, DONE_SAVED, 0)]);
}

#[test]
fn a_device_without_a_pool_keeps_every_answer_within_its_limits() {
    let fake = Fake::new(NetProfile::OTHER);
    let (a, b) = (
        request(&fake, OWNER, &big(1, 1_000)).unwrap(),
        request(&fake, OWNER, &big(2, 100)).unwrap(),
    );
    finish(&fake, a, 200, &[1; 700]);
    finished(
        &fake,
        b,
        Finished {
            status: 500,
            reason: 0,
            saved: false,
            truncated: true,
            etag: "",
            content_type: "",
            final_url: "",
            body: std::vec![2; 100],
        },
    )
    .unwrap();
    assert_eq!(
        drain(&fake, OWNER),
        [
            done(a, 1, 200, 0, 0, 700),
            done(b, 2, 500, 0, DONE_TRUNCATED, 100)
        ]
    );
}

#[test]
fn ended_requests_the_plugin_has_not_heard_of_count_against_its_limit() {
    // Offline: every request ends at once. A loop in one callback cannot pile
    // up ended requests without bound.
    let fake = Fake::x4();
    fake.power.set(Some(PowerEnd {
        reason: reason::OFFLINE,
        later: true,
    }));
    for token in 1..=4 {
        request(&fake, OWNER, &get(token)).unwrap();
    }
    assert_eq!(
        request(&fake, OWNER, &get(5)),
        Err(paperpaw_native::host_glue::ERR_BUSY)
    );
    // Another plugin has its own room; telling the events frees the first's.
    assert!(request(&fake, OTHER, &get(1)).is_ok());
    assert_eq!(drain(&fake, OWNER).len(), 4);
    assert!(request(&fake, OWNER, &get(5)).is_ok());
    assert_eq!(fake.pool_used(), 0, "ended requests hold no input");
}

#[test]
fn a_non_2xx_answer_keeps_at_most_4096_bytes() {
    let fake = Fake::x4();
    let id = request(&fake, OWNER, &get(1)).unwrap();
    finish(&fake, id, 404, &[7; 5_000]);
    assert_eq!(
        drain(&fake, OWNER),
        [done(id, 1, 404, 0, DONE_TRUNCATED, 4_096)]
    );
    // A 2xx answer keeps up to max_body.
    let id = request(&fake, OWNER, &get(2)).unwrap();
    finish(&fake, id, 200, &[7; 5_000]);
    assert_eq!(drain(&fake, OWNER), [done(id, 2, 200, 0, 0, 5_000)]);
}

#[test]
fn an_overlong_head_from_a_hostile_server_ends_the_request_at_once() {
    for (etag, final_url) in [
        ("e".repeat(257), String::new()),
        (String::new(), "u".repeat(1_025)),
    ] {
        let fake = Fake::x4();
        let id = request(&fake, OWNER, &get(1)).unwrap();
        finished(
            &fake,
            id,
            Finished {
                status: 200,
                reason: 0,
                saved: false,
                truncated: false,
                etag: &etag,
                content_type: "",
                final_url: &final_url,
                body: std::vec![1; 10],
            },
        )
        .unwrap();
        // No tick, no timeout: it has ended.
        assert_eq!(drain(&fake, OWNER), [done(id, 1, 0, reason::REFUSED, 0, 0)]);
        assert_eq!(fake.pool_used(), 0);
    }
    // At the limits it is fine.
    let fake = Fake::x4();
    let id = request(&fake, OWNER, &get(1)).unwrap();
    finished(
        &fake,
        id,
        Finished {
            status: 200,
            reason: 0,
            saved: false,
            truncated: false,
            etag: &"e".repeat(256),
            content_type: &"c".repeat(128),
            final_url: &"u".repeat(1_024),
            body: Vec::new(),
        },
    )
    .unwrap();
    assert_eq!(drain(&fake, OWNER), [done(id, 1, 200, 0, 0, 0)]);
}

#[test]
fn failed_requests_hold_no_memory_body_and_block_nobody() {
    let fake = Fake::x4();
    let (a, b) = (
        request(&fake, OWNER, &big(1, 100)).unwrap(),
        request(&fake, OWNER, &big(2, 100)).unwrap(),
    );
    for id in [a, b] {
        finished(
            &fake,
            id,
            Finished {
                status: 0,
                reason: reason::DNS,
                saved: false,
                truncated: false,
                etag: "",
                content_type: "",
                final_url: "",
                body: Vec::new(),
            },
        )
        .unwrap();
    }
    // The first plugin is busy and has not heard of either failure; the
    // other plugin's request starts at once.
    let c = request(&fake, OTHER, &big(3, 100)).unwrap();
    assert!(fake.started.borrow().contains(&c));
}

#[test]
fn no_network_comes_before_the_default_timeout_and_a_recovered_network_resets_the_wait() {
    let fake = Fake::x4();
    fake.network.set(false);
    let id = request(&fake, OWNER, &get(1)).unwrap();
    fake.advance(30_000);
    tick(&fake).unwrap();
    assert_eq!(
        drain(&fake, OWNER),
        [done(id, 1, 0, reason::NO_NETWORK, 0, 0)]
    );
    // A request that waits for the pool while the network is up is not
    // ended for a wait that ended.
    let fake = Fake::x4();
    let mut long = head(9, "http://a.b/");
    long.max_body = 45_056;
    long.timeout_ms = 600_000;
    let a = request(&fake, OTHER, &batch(&[Record::ServiceNetRequest(long)])).unwrap();
    fake.network.set(false);
    let mut h = head(2, "http://a.b/");
    h.max_body = 45_056;
    h.timeout_ms = 600_000;
    let b = request(&fake, OWNER, &batch(&[Record::ServiceNetRequest(h)])).unwrap();
    fake.advance(20_000);
    tick(&fake).unwrap();
    fake.network.set(true);
    tick(&fake).unwrap();
    fake.advance(11_000);
    tick(&fake).unwrap();
    assert!(
        drain(&fake, OWNER).is_empty(),
        "b still waits for room, not for a network"
    );
    assert_eq!(*fake.started.borrow(), [a]);
    finish(&fake, a, 200, b"");
    drain(&fake, OTHER);
    assert!(fake.started.borrow().contains(&b));
}

#[test]
fn a_network_back_before_the_first_tick_ends_the_wait_even_at_the_boundary() {
    let fake = Fake::x4();
    fake.network.set(false);
    let mut h = head(1, "http://a.b/");
    h.timeout_ms = 600_000;
    let id = request(&fake, OWNER, &batch(&[Record::ServiceNetRequest(h)])).unwrap();
    // The network is back at exactly the wait's end, before any tick.
    fake.advance(30_000);
    fake.network.set(true);
    tick(&fake).unwrap();
    assert!(drain(&fake, OWNER).is_empty(), "no NO_NETWORK");
    assert_eq!(*fake.started.borrow(), [id], "it started on the network");
}

#[test]
fn a_refused_replacement_leaves_the_older_request_untouched() {
    let fake = Fake::x4();
    let old = request(&fake, OWNER, &save_to(1, 0, "f.bin")).unwrap();
    fake.claim_fails
        .set(Some(paperpaw_native::host_glue::ERR_BUSY));
    assert_eq!(
        request(&fake, OWNER, &save_to(2, 0, "F.BIN")),
        Err(paperpaw_native::host_glue::ERR_BUSY)
    );
    assert!(
        fake.aborted.borrow().is_empty(),
        "the older transfer goes on"
    );
    assert!(next(&fake, OWNER).is_none(), "and nothing was told");
    fake.claim_fails.set(None);
    // And the pool is as one request left it.
    assert_eq!(
        fake.pool_used(),
        IO_BUFFER + save_to(1, 0, "f.bin").len() as u32
    );
    table::cancel(&fake, OWNER, 1).unwrap();
    assert_eq!(
        drain(&fake, OWNER),
        [done(old, 1, 0, reason::CANCELLED, 0, 0)]
    );
}

#[test]
fn a_stopped_plugin_has_its_received_folder_emptied() {
    let fake = Fake::x4();
    owner_stopped(&fake, OWNER).unwrap();
    assert!(fake.cleared.borrow().contains(&(OWNER, None)));
}

#[test]
fn sockets_and_requests_charge_the_same_pool() {
    // What `core.sockets` does: reserve at connect, release at the end, on the
    // pool it takes from the NET table (the same `Rc`).
    let fake = Fake::x4();
    let sock_pool = fake.table.pool().clone();
    let pool = NetProfile::X4.pool;
    // A memory body fills most of the pool: a socket's 8 KiB no longer fits.
    let a = request(&fake, OWNER, &big(1, 45_056)).unwrap();
    assert!(
        !sock_pool.reserve(8_192),
        "NO_MEMORY for the socket, nothing held"
    );
    assert_eq!(sock_pool.used(), fake.pool_used());
    finish(&fake, a, 200, b"");
    drain(&fake, OWNER);
    // Two sockets' reservations (SOCK_QUEUE + SOCK_READ) are held; a request
    // for a large body waits behind them instead of overcommitting.
    assert!(sock_pool.reserve(8_192) && sock_pool.reserve(8_192));
    let b = request(&fake, OWNER, &big(2, 45_056)).unwrap();
    assert!(fake.started.borrow().iter().all(|id| *id != b), "b waits");
    assert!(sock_pool.used() <= pool);
    // A socket ends: its room goes back, and the waiting request starts.
    sock_pool.release(8_192);
    sock_pool.release(8_192);
    tick(&fake).unwrap();
    assert!(fake.started.borrow().contains(&b), "b started on the room");
    assert!(!sock_pool.reserve(8_192), "and the pool is full again");
}

#[test]
fn an_error_answer_keeps_status_and_body_when_its_head_cannot_be_held() {
    let fake = Fake::x4();
    let id = request(&fake, OWNER, &big(1, 1_000)).unwrap();
    FAIL_NTH.with(|n| n.set(1));
    finished(
        &fake,
        id,
        Finished {
            status: 503,
            reason: 0,
            saved: false,
            truncated: false,
            etag: "\"v\"",
            content_type: "text/plain",
            final_url: "https://example.org/x",
            body: std::vec![5; 20],
        },
    )
    .unwrap();
    FAIL_NTH.with(|n| n.set(0));
    assert_eq!(drain(&fake, OWNER), [done(id, 1, 503, 0, 0, 20)]);
    assert_eq!(fake.pool_used(), 0);
}

#[test]
fn dropping_the_net_tables_gives_back_only_their_own_charges() {
    let pool = Rc::new(NetPool::new(NetProfile::X4.pool));
    // core.sockets holds 8 KiB for the whole test.
    assert!(pool.reserve(8_192));
    {
        let fake = Fake::with_pool(NetProfile::X4, pool.clone());
        // Running, queued (waiting for the pool), a done and held answer: all
        // hold charges.
        let a = request(&fake, OWNER, &big(1, 20_000)).unwrap();
        let _b = request(&fake, OWNER, &big(2, 30_000)).unwrap();
        finish(&fake, a, 200, b"hello");
        assert!(pool.used() > 8_192);
    }
    assert_eq!(pool.used(), 8_192, "the sockets' reservation is untouched");
    pool.release(8_192);
    assert_eq!(pool.used(), 0);
}
