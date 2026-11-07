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
    let a = request(&fake, OWNER, &big(1, 49_152)).unwrap();
    assert_eq!(fake.pool_used(), 49_152, "reserved at the start");
    let b = request(&fake, OWNER, &big(2, 100)).unwrap();
    assert_eq!(*fake.started.borrow(), [a], "b waits for room");
    assert_eq!(fake.pool_used(), 49_152);
    // a ends with 2 bytes: the pool keeps only those, the rest is free, but a
    // held answer counts until its callback returns.
    finish(&fake, a, 200, b"hi");
    assert_eq!(
        fake.pool_used(),
        2 + 100,
        "b started on the room that came free"
    );
    assert_eq!(*fake.started.borrow(), [a, b]);
    assert_eq!(drain(&fake, OWNER), [done(a, 1, 200, 0, 0, 2)]);
    assert_eq!(fake.pool_used(), 100);
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
    // c would fit in the 9,152 bytes left, but b waits for room before it.
    assert_eq!(*fake.started.borrow(), [a]);
    assert_eq!(fake.pool_used(), 40_000);
    finish(&fake, a, 200, b"x");
    drain(&fake, OWNER);
    assert_eq!(
        *fake.started.borrow(),
        [a, b, c],
        "in the order they were started"
    );
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
    assert_eq!(fake.pool_used(), 2 * IO_BUFFER, "a 4 KiB buffer each");
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
    let a = request(&fake, OWNER, &big(1, 49_152)).unwrap();
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
    finish(&fake, a, 500, &[7; 5_000]);
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
            truncated: false,
            etag: "",
            content_type: "",
            final_url: "",
            body: std::vec![1; 150],
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
        49_152,
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
