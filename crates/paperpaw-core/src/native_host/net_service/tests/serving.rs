//! The server table: `serve`, screen ties, served requests and their uploads,
//! `respond`, the 120 s answer time.
use super::*;
use paperpaw_native::host_glue::{
    ERR_BUSY, ERR_INVALID, ERR_NO_MEMORY, ERR_NO_SPACE, ERR_NOT_FOUND, ERR_STATE, ERR_UNSUPPORTED,
};

fn serve_rec(token: u32, port: u32) -> wire::ServiceNetServe {
    wire::ServiceNetServe {
        token,
        port,
        flags: 0,
        max_body: 0,
    }
}

/// `serve` as the plugin calls it: the shell's listener has not opened yet.
fn serve_call(fake: &Fake, owner: u64, s: &wire::ServiceNetServe) -> Result<(), i32> {
    let hosts = HostServices::new();
    serving::serve(fake, &caller(owner, &hosts), s)
}

/// `serve`, and the shell's next step where it listens: it reports the
/// listener open at the address it knew (so the plugin is told `started`).
fn serve(fake: &Fake, owner: u64, s: &wire::ServiceNetServe) -> Result<(), i32> {
    let before = fake.listens.borrow().len();
    serve_call(fake, owner, s)?;
    let listened = fake.listens.borrow().len() > before;
    let result = *fake.listen_result.borrow();
    if listened && !matches!(result, Some(Err(_))) {
        let address = result.and_then(Result::ok).unwrap_or("192.168.1.20:8080");
        serving::server_started(fake, key_of(fake), address).unwrap();
    }
    Ok(())
}

fn server_event(token: u32, state: u32, why: u32, address: &str) -> Event {
    Event::Server {
        token,
        state,
        reason: why,
        address: address.to_string(),
    }
}

/// A served request that came whole: told to the plugin, open for an answer.
fn arrive(fake: &Fake, key: u32, target: &str, body: u64) -> u32 {
    let Decision::Accept { request, .. } =
        served_head(fake, key, "POST", target, Some(body), false, 100).unwrap()
    else {
        panic!("accepted")
    };
    served_whole(fake, request, body).unwrap();
    request
}

fn key_of(fake: &Fake) -> u32 {
    fake.listens.borrow().last().unwrap().0
}

fn respond(fake: &Fake, owner: u64, records: &[Record<'_>]) -> Result<(), i32> {
    let hosts = HostServices::new();
    serving::respond(fake, &caller(owner, &hosts), &batch(records))
}

fn answer(request: u32, status: u32) -> Record<'static> {
    Record::ServiceNetResponse(wire::ServiceNetResponse {
        request,
        status,
        content_type: "text/plain",
    })
}

#[test]
fn serve_is_checked_and_tells_its_outcome_as_an_event() {
    let fake = Fake::servers(8);
    serve(&fake, OWNER, &serve_rec(1, 8080)).unwrap();
    assert_eq!(
        drain(&fake, OWNER),
        [server_event(1, 0, 0, "192.168.1.20:8080")]
    );
    // The cable has port 0 and no address.
    *fake.listen_result.borrow_mut() = Some(Ok(""));
    serve(&fake, OWNER, &serve_rec(2, 0)).unwrap();
    assert_eq!(drain(&fake, OWNER), [server_event(2, 0, 0, "")]);
    // A port the host could not get.
    *fake.listen_result.borrow_mut() = Some(Err(reason::REFUSED));
    serve(&fake, OTHER, &serve_rec(3, 80)).unwrap();
    assert_eq!(
        drain(&fake, OTHER),
        [server_event(3, 1, reason::REFUSED, "")]
    );
}

#[test]
fn serve_refuses_what_the_contract_refuses() {
    let fake = Fake::servers(8);
    let mut s = serve_rec(1, 8080);
    s.flags = 1;
    assert_eq!(serve(&fake, OWNER, &s), Err(ERR_UNSUPPORTED));
    assert_eq!(serve(&fake, OWNER, &serve_rec(1, 65_536)), Err(ERR_INVALID));
    assert!(serve(&fake, OWNER, &serve_rec(1, 65_535)).is_ok());
    // The cable exists on the X4 only.
    assert_eq!(
        serve(&Fake::new(NetProfile::OTHER), OWNER, &serve_rec(1, 0)),
        Err(ERR_UNSUPPORTED)
    );
    // max_body: 64 MiB at most.
    for (max, ok) in [(0, true), (64 << 20, true), ((64 << 20) + 1, false)] {
        let mut s = serve_rec(7, 8080);
        s.max_body = max;
        assert_eq!(
            serve(&Fake::servers(8), OWNER, &s).is_ok(),
            ok,
            "max_body {max}"
        );
    }
    // Only from a callback of a showing screen of the plugin.
    fake.screen.set(None);
    assert_eq!(serve(&fake, OWNER, &serve_rec(2, 8081)), Err(ERR_STATE));
    // And only with a subscription to its events.
    fake.screen.set(Some(5));
    fake.subscribed.set(false);
    assert_eq!(serve(&fake, OWNER, &serve_rec(2, 8081)), Err(ERR_STATE));
    assert_eq!(fake.listens.borrow().len(), 1, "nothing else was started");
}

#[test]
fn two_servers_per_plugin_and_serving_a_token_again_stops_the_old_one() {
    let fake = Fake::servers(8);
    serve(&fake, OWNER, &serve_rec(1, 8080)).unwrap();
    serve(&fake, OWNER, &serve_rec(2, 8081)).unwrap();
    assert_eq!(serve(&fake, OWNER, &serve_rec(3, 8082)), Err(ERR_BUSY));
    assert!(
        serve(&fake, OTHER, &serve_rec(1, 9000)).is_ok(),
        "per plugin"
    );
    drain(&fake, OWNER);
    // The token in use: the old server stops first, then the new one starts.
    serve(&fake, OWNER, &serve_rec(1, 8090)).unwrap();
    assert_eq!(
        drain(&fake, OWNER),
        [
            server_event(1, 2, 0, ""),
            server_event(1, 0, 0, "192.168.1.20:8080")
        ]
    );
    assert_eq!(fake.unlistened.borrow().len(), 1);
}

#[test]
fn a_server_that_cannot_start_fails_with_a_reason() {
    let fake = Fake::servers(8);
    *fake.listen_result.borrow_mut() = Some(Err(reason::REFUSED));
    serve(&fake, OWNER, &serve_rec(1, 80)).unwrap();
    fake.network.set(false);
    serve(&fake, OWNER, &serve_rec(2, 80)).unwrap();
    fake.network.set(true);
    fake.power.set(Some(PowerEnd {
        reason: reason::SAVING,
        later: true,
    }));
    serve(&fake, OWNER, &serve_rec(3, 80)).unwrap();
    assert_eq!(
        drain(&fake, OWNER),
        [
            server_event(1, 1, reason::REFUSED, ""),
            server_event(2, 1, reason::NO_NETWORK, ""),
            server_event(3, 1, reason::SAVING, ""),
        ]
    );
    assert_eq!(
        fake.listens.borrow().len(),
        1,
        "no listening for a refused one"
    );
    // The cable needs no network.
    fake.power.set(None);
    fake.network.set(false);
    *fake.listen_result.borrow_mut() = Some(Ok(""));
    serve(&fake, OWNER, &serve_rec(4, 0)).unwrap();
    assert_eq!(drain(&fake, OWNER), [server_event(4, 0, 0, "")]);
}

#[test]
fn a_server_lives_on_its_screen_and_stops_when_it_closes() {
    let fake = Fake::servers(8);
    serve(&fake, OWNER, &serve_rec(1, 8080)).unwrap();
    fake.screen.set(Some(6));
    serve(&fake, OWNER, &serve_rec(2, 8081)).unwrap();
    drain(&fake, OWNER);
    screen_closed(&fake, OWNER, 5).unwrap();
    assert_eq!(drain(&fake, OWNER), [server_event(1, 2, 0, "")]);
    screen_closed(&fake, OWNER, 5).unwrap();
    assert!(drain(&fake, OWNER).is_empty());
    stop_serving_as(&fake, OWNER, 2);
    assert_eq!(drain(&fake, OWNER), [server_event(2, 2, 0, "")]);
    stop_serving_as(&fake, OWNER, 2); // none: nothing to do
    assert!(drain(&fake, OWNER).is_empty());
}

#[test]
fn serve_outside_a_screen_callback_ties_the_server_to_the_topmost_own_screen() {
    let fake = Fake::servers(8);
    // No screen callback runs (a `wifi_changed` callback, say), no work run;
    // the plugin's screen 8 is on top and showing.
    fake.screen.set(None);
    fake.topmost.set(Some(8));
    serve(&fake, OWNER, &serve_rec(1, 8080)).unwrap();
    drain(&fake, OWNER);
    // Another screen closing leaves it; its own screen hiding stops it.
    screen_closed(&fake, OWNER, 5).unwrap();
    assert!(drain(&fake, OWNER).is_empty());
    screen_closed(&fake, OWNER, 8).unwrap();
    assert_eq!(drain(&fake, OWNER), [server_event(1, 2, 0, "")]);
}

#[test]
fn serve_outside_a_screen_callback_without_a_showing_own_screen_is_refused() {
    let fake = Fake::servers(8);
    fake.screen.set(None);
    fake.topmost.set(None);
    assert_eq!(serve(&fake, OWNER, &serve_rec(1, 8080)), Err(ERR_STATE));
    assert!(fake.listens.borrow().is_empty(), "nothing was started");
}

#[test]
fn serve_from_a_work_run_is_refused_even_with_a_showing_own_screen() {
    let fake = Fake::servers(8);
    fake.screen.set(None);
    fake.topmost.set(Some(8));
    fake.run.set(Some(3));
    assert_eq!(serve(&fake, OWNER, &serve_rec(1, 8080)), Err(ERR_STATE));
    assert!(fake.listens.borrow().is_empty(), "nothing was started");
}

#[test]
fn serve_outside_a_screen_callback_fails_closed_when_the_runs_cannot_be_told() {
    let fake = Fake::servers(8);
    fake.screen.set(None);
    fake.topmost.set(Some(8));
    fake.run_known.set(false);
    assert_eq!(serve(&fake, OWNER, &serve_rec(1, 8080)), Err(ERR_STATE));
    assert!(fake.listens.borrow().is_empty(), "nothing was started");
    fake.run_known.set(true);
    serve(&fake, OWNER, &serve_rec(1, 8080)).unwrap();
}

fn stop_serving_as(fake: &Fake, owner: u64, token: u32) {
    serving::stop_serving(fake, owner, token).unwrap();
}

#[test]
fn a_network_that_goes_away_stops_servers_on_it_but_not_the_cable() {
    let fake = Fake::servers(8);
    *fake.listen_result.borrow_mut() = Some(Ok(""));
    serve(&fake, OWNER, &serve_rec(1, 0)).unwrap();
    *fake.listen_result.borrow_mut() = None;
    serve(&fake, OWNER, &serve_rec(2, 8080)).unwrap();
    drain(&fake, OWNER);
    network_lost(&fake).unwrap();
    assert_eq!(
        drain(&fake, OWNER),
        [server_event(2, 2, reason::NO_NETWORK, "")]
    );
}

#[test]
fn a_serve_empties_only_its_own_received_folder_and_a_start_empties_all() {
    let fake = Fake::servers(8);
    serve(&fake, OWNER, &serve_rec(1, 8080)).unwrap();
    serve(&fake, OWNER, &serve_rec(2, 8081)).unwrap();
    assert_eq!(
        *fake.cleared.borrow(),
        [(OWNER, Some(1)), (OWNER, Some(2))],
        "a serve never touches another server's folder"
    );
    // A request body of server 1 is whole and told.
    let k1 = fake.listens.borrow()[0].0;
    let r = arrive(&fake, k1, "/up", 10);
    // Serving server 2 again does not remove what server 1 is receiving.
    serve(&fake, OWNER, &serve_rec(2, 8082)).unwrap();
    assert!(!fake.removed.borrow().iter().any(|e| e.2 == r));
    owner_started(&fake, OWNER);
    assert_eq!(fake.cleared.borrow().last(), Some(&(OWNER, None)));
}

#[test]
fn request_heads_are_judged_at_their_limits() {
    let fake = Fake::servers(8);
    serve(&fake, OWNER, &serve_rec(1, 8080)).unwrap();
    let k = key_of(&fake);
    let head = |target: &str, len: Option<u64>, chunked: bool, head_len: u32| {
        served_head(&fake, k, "GET", target, len, chunked, head_len).unwrap()
    };
    // 8 KiB of head is the most; one over is 431.
    assert!(matches!(
        head("/", None, false, 8_192),
        Decision::Accept { .. }
    ));
    served_gone(&fake, 1).unwrap();
    assert_eq!(head("/", None, false, 8_193), Decision::Reject(431));
    // A target of 1,024 bytes; 1,025 is 414.
    let t = "/".repeat(1_024);
    assert!(matches!(
        head(&t, None, false, 100),
        Decision::Accept { .. }
    ));
    served_gone(&fake, 2).unwrap();
    assert_eq!(
        head(&"/".repeat(1_025), None, false, 100),
        Decision::Reject(414)
    );
    assert_eq!(
        head("/", Some(5), true, 100),
        Decision::Reject(411),
        "chunked"
    );
    assert_eq!(
        served_head(&fake, k, "G T", "/", None, false, 100).unwrap(),
        Decision::Reject(400)
    );
    // The body: the server's max_body is 4 MiB; one over is 413 before a byte.
    assert!(matches!(
        head("/", Some(4 * 1024 * 1024), false, 100),
        Decision::Accept { .. }
    ));
    served_gone(&fake, 3).unwrap();
    assert_eq!(
        head("/", Some(4 * 1024 * 1024 + 1), false, 100),
        Decision::Reject(413)
    );
    assert_eq!(fake.pool_used(), 0, "a refused head holds nothing");
    assert_eq!(
        served_head(&fake, 999, "GET", "/", None, false, 1),
        Err(ERR_NOT_FOUND)
    );
}

#[test]
fn at_most_two_requests_per_server_are_open_the_rest_wait() {
    let fake = Fake::servers(8);
    serve(&fake, OWNER, &serve_rec(1, 8080)).unwrap();
    let k = key_of(&fake);
    let a = served_head(&fake, k, "GET", "/a", None, false, 10).unwrap();
    let b = served_head(&fake, k, "GET", "/b", None, false, 10).unwrap();
    assert!(matches!(
        (a, b),
        (Decision::Accept { .. }, Decision::Accept { .. })
    ));
    assert_eq!(
        served_head(&fake, k, "GET", "/c", None, false, 10).unwrap(),
        Decision::Busy
    );
    served_gone(&fake, 1).unwrap();
    assert!(matches!(
        served_head(&fake, k, "GET", "/c", None, false, 10).unwrap(),
        Decision::Accept { .. }
    ));
}

#[test]
fn an_upload_is_stopped_at_the_servers_cap_and_at_the_free_space_reserve() {
    let fake = Fake::servers(8);
    let mut s = serve_rec(1, 8080);
    s.max_body = 1_000;
    serve(&fake, OWNER, &s).unwrap();
    let k = key_of(&fake);
    // A Content-Length over the cap: 413 before a byte is written.
    assert_eq!(
        served_head(&fake, k, "PUT", "/", Some(1_001), false, 10).unwrap(),
        Decision::Reject(413)
    );
    // At the cap it is taken (a 4 KiB buffer from the pool), and the host
    // stops it again when the stream goes past it.
    let Decision::Accept { request, token } =
        served_head(&fake, k, "PUT", "/", Some(1_000), false, 10).unwrap()
    else {
        panic!()
    };
    assert_eq!(token, 1);
    assert_eq!(fake.pool_used(), IO_BUFFER);
    assert_eq!(
        served_body(&fake, request, 1_000, 500).unwrap(),
        Decision::Busy
    );
    assert_eq!(
        served_body(&fake, request, 1_001, 1).unwrap(),
        Decision::Reject(413)
    );
    assert_eq!(
        *fake.removed.borrow(),
        [(OWNER, 1, request)],
        "the part is removed"
    );
    assert_eq!(fake.pool_used(), 0);
    // The free-space reserve: the card is nearly full.
    fake.free.set(100);
    assert_eq!(
        served_head(&fake, k, "PUT", "/", Some(101), false, 10).unwrap(),
        Decision::Reject(507)
    );
    let Decision::Accept { request, .. } =
        served_head(&fake, k, "PUT", "/", Some(100), false, 10).unwrap()
    else {
        panic!()
    };
    fake.free.set(10);
    assert_eq!(
        served_body(&fake, request, 50, 50).unwrap(),
        Decision::Reject(507)
    );
    assert_eq!(fake.pool_used(), 0);
    assert_eq!(
        served_whole(&fake, request, 50),
        Err(ERR_NOT_FOUND),
        "it ended"
    );
}

#[test]
fn an_upload_buffer_that_does_not_fit_the_pool_is_answered_503() {
    let fake = Fake::servers(8);
    serve(&fake, OWNER, &serve_rec(1, 8080)).unwrap();
    let k = key_of(&fake);
    // A request holding the whole pool.
    request(&fake, OTHER, &big(1, 45_056)).unwrap();
    assert_eq!(fake.pool_used(), 45_056 + big(1, 45_056).len() as u32);
    assert_eq!(
        served_head(&fake, k, "PUT", "/", Some(10), false, 10).unwrap(),
        Decision::Reject(503)
    );
    // Without a body no buffer is needed.
    assert!(matches!(
        served_head(&fake, k, "GET", "/", None, false, 10).unwrap(),
        Decision::Accept { .. }
    ));
}

#[test]
fn a_served_request_is_told_once_and_answered_once() {
    let fake = Fake::servers(8);
    serve(&fake, OWNER, &serve_rec(1, 8080)).unwrap();
    drain(&fake, OWNER);
    let k = key_of(&fake);
    let Decision::Accept { request, .. } =
        served_head(&fake, k, "POST", "/upload?x=1", Some(5), false, 10).unwrap()
    else {
        panic!()
    };
    // Receiving: not told, and not answerable yet.
    assert!(next(&fake, OWNER).is_none());
    assert_eq!(
        respond(&fake, OWNER, &[answer(request, 200)]),
        Err(ERR_NOT_FOUND)
    );
    served_whole(&fake, request, 5).unwrap();
    assert_eq!(
        drain(&fake, OWNER),
        [Event::Served {
            server: 1,
            request,
            method: "POST".into(),
            target: "/upload?x=1".into(),
            flags: 1,
            body_len: 5
        }]
    );
    assert_eq!(
        fake.pool_used(),
        0,
        "the upload buffer is back when it is whole"
    );
    // Another plugin's answer is not found, never denied.
    assert_eq!(
        respond(&fake, OTHER, &[answer(request, 200)]),
        Err(ERR_NOT_FOUND)
    );
    respond(&fake, OWNER, &[answer(request, 201)]).unwrap();
    assert_eq!(*fake.replies.borrow(), [(request, 201, None)]);
    assert_eq!(
        *fake.removed.borrow(),
        [(OWNER, 1, request)],
        "the body goes at the answer"
    );
    assert_eq!(
        respond(&fake, OWNER, &[answer(request, 200)]),
        Err(ERR_NOT_FOUND)
    );
    served_gone(&fake, request).unwrap();
    served_gone(&fake, request).unwrap();
}

#[test]
fn a_request_the_plugin_leaves_unanswered_gets_503_after_120_seconds() {
    let fake = Fake::servers(8);
    serve(&fake, OWNER, &serve_rec(1, 8080)).unwrap();
    let r = arrive(&fake, key_of(&fake), "/slow", 0);
    drain(&fake, OWNER);
    fake.advance(119_999);
    tick(&fake).unwrap();
    assert!(fake.closed.borrow().is_empty());
    fake.advance(1);
    tick(&fake).unwrap();
    assert_eq!(*fake.closed.borrow(), [(r, 503)]);
    assert_eq!(respond(&fake, OWNER, &[answer(r, 200)]), Err(ERR_NOT_FOUND));
}

#[test]
fn respond_checks_its_status_headers_and_what_it_sends() {
    let fake = Fake::servers(8);
    serve(&fake, OWNER, &serve_rec(1, 8080)).unwrap();
    let k = key_of(&fake);
    let r = arrive(&fake, k, "/", 0);
    drain(&fake, OWNER);
    let hdr = |n: &'static str| {
        Record::ServiceNetHeader(wire::ServiceNetHeader {
            name: n,
            value: "v",
        })
    };
    let body = |d: &'static [u8]| Record::ServiceNetBody(wire::ServiceNetBody { data: d });
    for status in [99, 600, 0] {
        assert_eq!(
            respond(&fake, OWNER, &[answer(r, status)]),
            Err(ERR_INVALID),
            "{status}"
        );
    }
    for refused in ["Content-Length", "connection", "Content-Type", "Host"] {
        assert_eq!(
            respond(&fake, OWNER, &[answer(r, 200), hdr(refused)]),
            Err(ERR_INVALID),
            "{refused}"
        );
    }
    // Eight headers; the ninth is invalid.
    let mut eight = std::vec![answer(r, 200)];
    eight.extend((0..9).map(|_| hdr("X-A")));
    assert_eq!(respond(&fake, OWNER, &eight), Err(ERR_INVALID));
    // A file and a body together, a body before a header, a request first.
    let file = Record::ServiceNetFile(wire::ServiceNetFile {
        role: 0,
        root: 2,
        path: "page.html",
    });
    assert_eq!(
        respond(&fake, OWNER, &[answer(r, 200), file, body(b"x")]),
        Err(ERR_INVALID)
    );
    assert_eq!(
        respond(&fake, OWNER, &[answer(r, 200), body(b"x"), hdr("X-A")]),
        Err(ERR_INVALID)
    );
    assert_eq!(respond(&fake, OWNER, &[hdr("X-A")]), Err(ERR_INVALID));
    // A missing file is a 404 by the host and the call is fine.
    respond(&fake, OWNER, &[answer(r, 200), file]).unwrap();
    assert_eq!(
        *fake.replies.borrow(),
        [(r, 200, None)],
        "the transport sees no file"
    );
}

#[test]
fn a_held_answer_body_comes_from_the_pool_or_the_call_fails_and_holds_nothing() {
    let fake = Fake::servers(8);
    serve(&fake, OWNER, &serve_rec(1, 8080)).unwrap();
    let k = key_of(&fake);
    let (r1, r2) = (arrive(&fake, k, "/1", 0), arrive(&fake, k, "/2", 0));
    drain(&fake, OWNER);
    // A request holds the body room, leaving a little over 4 KiB.
    request(&fake, OTHER, &big(1, 45_056)).unwrap();
    let held_before = fake.pool_used();
    let body = Record::ServiceNetBody(wire::ServiceNetBody { data: &[1; 2_048] });
    respond(&fake, OWNER, &[answer(r1, 200), body]).unwrap();
    assert_eq!(fake.pool_used(), held_before + 2_048);
    // The second 2 KiB body does not fit what is left: no memory, nothing held.
    assert_eq!(
        respond(&fake, OWNER, &[answer(r2, 200), body]),
        Err(ERR_NO_MEMORY)
    );
    assert_eq!(fake.pool_used(), held_before + 2_048, "nothing was held");
    assert_eq!(fake.replies.borrow().len(), 1);
    // The request is still open: once the first is sent, it works.
    served_gone(&fake, r1).unwrap();
    respond(&fake, OWNER, &[answer(r2, 200), body]).unwrap();
    assert_eq!(fake.pool_used(), held_before + 2_048);
    served_gone(&fake, r2).unwrap();
    assert_eq!(fake.pool_used(), held_before);
}

#[test]
fn a_transport_that_cannot_answer_ends_the_request_with_its_error() {
    let fake = Fake::servers(8);
    serve(&fake, OWNER, &serve_rec(1, 8080)).unwrap();
    let r = arrive(&fake, key_of(&fake), "/", 0);
    drain(&fake, OWNER);
    fake.reply_fails
        .set(Some(paperpaw_native::host_glue::ERR_IO));
    assert_eq!(
        respond(&fake, OWNER, &[answer(r, 200)]),
        Err(paperpaw_native::host_glue::ERR_IO)
    );
    assert_eq!(respond(&fake, OWNER, &[answer(r, 200)]), Err(ERR_NOT_FOUND));
}

#[test]
fn stopping_a_server_ends_its_requests_and_removes_their_bodies_nothing_stays() {
    let fake = Fake::servers(8);
    serve(&fake, OWNER, &serve_rec(1, 8080)).unwrap();
    serve(&fake, OWNER, &serve_rec(2, 8081)).unwrap();
    let (k1, k2) = (fake.listens.borrow()[0].0, fake.listens.borrow()[1].0);
    let a = arrive(&fake, k1, "/a", 10);
    let b = arrive(&fake, k2, "/b", 10);
    drain(&fake, OWNER);
    serving::stop_serving(&fake, OWNER, 1).unwrap();
    assert_eq!(*fake.closed.borrow(), [(a, 503)]);
    assert_eq!(*fake.removed.borrow(), [(OWNER, 1, a)]);
    assert_eq!(respond(&fake, OWNER, &[answer(a, 200)]), Err(ERR_NOT_FOUND));
    // The other server's request is untouched.
    respond(&fake, OWNER, &[answer(b, 200)]).unwrap();
    // A stopped plugin loses all of it without a word.
    let c = arrive(&fake, k2, "/c", 0);
    owner_stopped(&fake, OWNER).unwrap();
    assert!(next(&fake, OWNER).is_none());
    assert!(fake.closed.borrow().contains(&(c, 503)));
    assert_eq!(fake.pool_used(), 0);
    assert_eq!(respond(&fake, OWNER, &[answer(c, 200)]), Err(ERR_NOT_FOUND));
}

#[test]
fn a_failed_allocation_in_serve_leaves_no_server_behind() {
    for nth in 1..=8 {
        let fake = Fake::servers(8);
        FAIL_NTH.with(|n| n.set(nth));
        // `serve`, then the shell's report that the listener opened.
        let result = serve_call(&fake, OWNER, &serve_rec(1, 8080)).and_then(|()| {
            match fake.listens.borrow().last() {
                Some(_) => serving::server_started(&fake, key_of(&fake), "192.168.1.20:8080"),
                None => Ok(()),
            }
        });
        FAIL_NTH.with(|n| n.set(0));
        match result {
            Err(error) => {
                assert_eq!(error, ERR_NO_MEMORY, "allocation {nth}");
                let listening = fake.listens.borrow().len();
                assert_eq!(
                    fake.unlistened.borrow().len(),
                    listening,
                    "allocation {nth}"
                );
                // Nothing, or the terminal `stopped` (never lost, never a
                // `started` for a server that is not kept).
                for event in drain(&fake, OWNER) {
                    assert!(
                        matches!(event, Event::Server { state: 2, .. }),
                        "allocation {nth}: {event:?}"
                    );
                }
            }
            Ok(()) => {
                assert!(matches!(next(&fake, OWNER), Some(Event::Server { .. })));
            }
        }
        // And the plugin can serve again.
        assert!(
            serve(&fake, OWNER, &serve_rec(1, 8080)).is_ok(),
            "allocation {nth}"
        );
    }
    let _ = ERR_NO_SPACE;
}

#[test]
fn an_upload_that_moves_no_byte_for_30_seconds_is_dropped_with_408() {
    let fake = Fake::servers(8);
    serve(&fake, OWNER, &serve_rec(1, 8080)).unwrap();
    let k = key_of(&fake);
    let Decision::Accept { request, .. } =
        served_head(&fake, k, "PUT", "/", Some(1_000), false, 10).unwrap()
    else {
        panic!()
    };
    fake.advance(29_999);
    tick(&fake).unwrap();
    assert!(fake.closed.borrow().is_empty());
    // A byte arrives: the clock starts again.
    assert_eq!(served_body(&fake, request, 10, 10).unwrap(), Decision::Busy);
    fake.advance(29_999);
    tick(&fake).unwrap();
    assert!(fake.closed.borrow().is_empty());
    fake.advance(1);
    tick(&fake).unwrap();
    assert_eq!(*fake.closed.borrow(), [(request, 408)]);
    assert_eq!(*fake.removed.borrow(), [(OWNER, 1, request)]);
    assert_eq!(fake.pool_used(), 0);
    assert_eq!(served_whole(&fake, request, 1_000), Err(ERR_NOT_FOUND));
}

#[test]
fn a_plugin_cannot_pile_up_server_events_by_looping() {
    let fake = Fake::servers(8);
    let mut accepted = 0;
    for port in 8000..8100 {
        match serve(&fake, OWNER, &serve_rec(1, port)) {
            Ok(()) => accepted += 1,
            Err(error) => {
                assert_eq!(error, ERR_BUSY);
                break;
            }
        }
    }
    assert!((2..=5).contains(&accepted), "{accepted}");
    // Telling the events makes room again.
    assert!(drain(&fake, OWNER).len() >= 2);
    assert!(serve(&fake, OWNER, &serve_rec(1, 9000)).is_ok());
}

#[test]
fn the_stopped_event_of_a_server_is_never_lost_to_an_allocation_failure() {
    let fake = Fake::servers(64);
    // Many plugins, each with a server, started and told.
    for owner in 100..140 {
        serve(&fake, owner, &serve_rec(1, 8080)).unwrap();
        assert!(matches!(
            next(&fake, owner),
            Some(Event::Server { state: 0, .. })
        ));
    }
    // The next allocation fails at every stop; the servers still stop and say so.
    for owner in 100..140 {
        FAIL_NTH.with(|n| n.set(1));
        screen_closed(&fake, owner, 5).unwrap();
    }
    FAIL_NTH.with(|n| n.set(0));
    for owner in 100..140 {
        assert_eq!(
            drain(&fake, owner),
            [server_event(1, 2, 0, "")],
            "owner {owner}"
        );
    }
    assert!(!fake.table.lock().unwrap().serving.any());
}

#[test]
fn turning_a_plugins_network_off_stops_its_server_at_the_next_pass() {
    let fake = Fake::servers(8);
    *fake.listen_result.borrow_mut() = Some(Ok("192.168.1.2:80"));
    serve(&fake, OWNER, &serve_rec(1, 80)).unwrap();
    drain(&fake, OWNER);
    fake.power.set(Some(PowerEnd {
        reason: reason::SAVING,
        later: true,
    }));
    tick(&fake).unwrap();
    assert!(
        fake.unlistened.borrow().is_empty(),
        "saving: left listening"
    );
    fake.power.set(Some(PowerEnd {
        reason: reason::NETWORK_OFF,
        later: false,
    }));
    tick(&fake).unwrap();
    assert_eq!(fake.unlistened.borrow().len(), 1, "it stopped listening");
    assert_eq!(
        drain(&fake, OWNER),
        [server_event(1, 2, reason::NETWORK_OFF, "")]
    );
}

#[test]
fn the_largest_served_request_event_fits_the_pump_buffer_as_a_batch() {
    let fake = Fake::servers(8);
    serve(&fake, OWNER, &serve_rec(1, 8080)).unwrap();
    let k = key_of(&fake);
    // A 1,024-byte target and the longest method (16 bytes).
    let target = "/".repeat(1_024);
    let method = "M".repeat(16);
    let Decision::Accept { request, .. } =
        served_head(&fake, k, &method, &target, Some(0), false, 100).unwrap()
    else {
        panic!("accepted")
    };
    served_whole(&fake, request, 0).unwrap();
    // Skip the server's own `listening` event first.
    let mut out = std::vec![0; crate::native_host::service_events::EVENT_BUFFER];
    let mut longest = 0;
    while let Some(used) = next_event(&fake, OWNER, &mut out).unwrap() {
        longest = longest.max(used);
        // Framed as the SDK reads it: one record in a batch of one.
        assert!(paperpaw_sdk::native::events::Incoming::new(&out[..used]).is_ok());
        event_delivered(&fake, OWNER).unwrap();
    }
    assert!(longest > 1_040, "the served request was among them");
}

#[test]
fn the_plugin_is_told_started_when_the_shell_reports_the_listener_not_when_listen_returns() {
    let fake = Fake::servers(8);
    serve_call(&fake, OWNER, &serve_rec(1, 8080)).unwrap();
    let key = key_of(&fake);
    assert_eq!(drain(&fake, OWNER), [], "listen returned: nothing told yet");
    assert!(fake.unlistened.borrow().is_empty());
    serving::server_started(&fake, key, "192.168.1.20:8080").unwrap();
    assert_eq!(
        drain(&fake, OWNER),
        [server_event(1, 0, 0, "192.168.1.20:8080")]
    );
    // Reported once; an unknown server is not found.
    assert_eq!(serving::server_started(&fake, key, "x"), Err(ERR_STATE));
    assert_eq!(serving::server_started(&fake, 999, "x"), Err(ERR_NOT_FOUND));
    assert_eq!(drain(&fake, OWNER), []);
}

#[test]
fn a_listener_that_cannot_open_is_told_failed_refused_and_frees_the_slot() {
    let fake = Fake::servers(8);
    // The plugin's servers are full (`PLUGIN_SERVERS`).
    for token in 1..=2 {
        serve_call(&fake, OWNER, &serve_rec(token, 8080)).unwrap();
    }
    assert_eq!(
        serve_call(&fake, OWNER, &serve_rec(3, 8080)),
        Err(ERR_BUSY),
        "no slot left"
    );
    let first = fake.listens.borrow()[0].0;
    serving::server_failed(&fake, first).unwrap();
    assert_eq!(
        drain(&fake, OWNER),
        [server_event(1, 1, reason::REFUSED, "")],
        "failed, reason refused, and no started before it"
    );
    assert_eq!(
        *fake.unlistened.borrow(),
        [first],
        "the shell lets go (a no-op)"
    );
    // The slot is free: a new server starts.
    serve(&fake, OWNER, &serve_rec(3, 8081)).unwrap();
    assert_eq!(
        drain(&fake, OWNER),
        [server_event(3, 0, 0, "192.168.1.20:8080")]
    );
    // Reported after the fact: not found; a second report too.
    assert_eq!(serving::server_failed(&fake, first), Err(ERR_NOT_FOUND));
    assert_eq!(
        serving::server_started(&fake, first, "x"),
        Err(ERR_NOT_FOUND)
    );
}

#[test]
fn a_server_stopped_before_its_listener_opened_is_told_stopped_and_the_late_report_is_ignored() {
    let fake = Fake::servers(8);
    serve_call(&fake, OWNER, &serve_rec(1, 8080)).unwrap();
    let key = key_of(&fake);
    stop_serving_as(&fake, OWNER, 1);
    assert_eq!(drain(&fake, OWNER), [server_event(1, 2, 0, "")]);
    // The shell was told to cancel the open that was still pending, so its
    // late report finds nothing and the listener is not left behind.
    assert_eq!(*fake.unlistened.borrow(), [key]);
    assert_eq!(serving::server_started(&fake, key, "x"), Err(ERR_NOT_FOUND));
    assert_eq!(serving::server_failed(&fake, key), Err(ERR_NOT_FOUND));
    assert_eq!(drain(&fake, OWNER), []);
}

#[test]
fn an_address_that_does_not_fit_fails_the_server_and_the_shell_stops_listening() {
    let fake = Fake::servers(8);
    serve_call(&fake, OWNER, &serve_rec(1, 8080)).unwrap();
    let key = key_of(&fake);
    serving::server_started(&fake, key, &"a".repeat(65)).unwrap();
    assert_eq!(
        drain(&fake, OWNER),
        [server_event(1, 1, reason::REFUSED, "")]
    );
    assert_eq!(*fake.unlistened.borrow(), [key]);
}

#[test]
fn the_device_has_a_cap_of_servers_over_all_plugins_and_a_serve_past_it_is_busy_at_once() {
    // The X4: one server at once, whichever plugin.
    let fake = Fake::x4();
    serve(&fake, OWNER, &serve_rec(1, 8080)).unwrap();
    assert_eq!(drain(&fake, OWNER).len(), 1);
    assert_eq!(
        serve(&fake, OWNER + 1, &serve_rec(1, 8081)),
        Err(ERR_BUSY),
        "a second plugin's serve"
    );
    assert_eq!(
        serve(&fake, OWNER, &serve_rec(2, 8082)),
        Err(ERR_BUSY),
        "the same plugin's second server"
    );
    assert_eq!(fake.listens.borrow().len(), 1, "nothing was opened");
    assert_eq!(drain(&fake, OWNER + 1), [], "a status, not an event");
    // The same token again replaces the server and keeps its place.
    serve(&fake, OWNER, &serve_rec(1, 8083)).unwrap();
    // The slot frees after a stop ...
    stop_serving_as(&fake, OWNER, 1);
    serve(&fake, OWNER + 1, &serve_rec(1, 8081)).unwrap();
    // ... and after the shell reported the listener failed.
    let key = key_of(&fake);
    assert_eq!(serve_call(&fake, OWNER, &serve_rec(1, 8084)), Err(ERR_BUSY));
    serving::server_failed(&fake, key).unwrap();
    serve(&fake, OWNER, &serve_rec(1, 8084)).unwrap();
    // The cable's server holds no place: served again on a network port
    // with the device's one taken, it is busy (Core F1 on #900).
    *fake.listen_result.borrow_mut() = Some(Ok(""));
    serve(&fake, OWNER + 1, &serve_rec(2, 0)).unwrap();
    assert_eq!(serve(&fake, OWNER + 1, &serve_rec(2, 8085)), Err(ERR_BUSY));
    // The X4 Pro and Classic have two; other devices eight.
    let fake = Fake::servers(2);
    serve(&fake, OWNER, &serve_rec(1, 8080)).unwrap();
    serve(&fake, OWNER + 1, &serve_rec(1, 8081)).unwrap();
    assert_eq!(serve(&fake, OWNER + 2, &serve_rec(1, 8082)), Err(ERR_BUSY));
    let fake = Fake::new(NetProfile::OTHER);
    for owner in 0..4 {
        serve(&fake, owner, &serve_rec(1, 8080)).unwrap();
        serve(&fake, owner, &serve_rec(2, 8081)).unwrap();
    }
    assert_eq!(
        serve(&fake, 9, &serve_rec(1, 8082)),
        Err(ERR_BUSY),
        "8 in all"
    );
}

#[test]
fn a_request_on_a_server_not_yet_reported_open_is_refused() {
    let fake = Fake::x4();
    serve_call(&fake, OWNER, &serve_rec(1, 8080)).unwrap();
    let key = key_of(&fake);
    assert_eq!(
        served_head(&fake, key, "GET", "/", None, false, 10).unwrap(),
        Decision::Reject(503)
    );
    assert_eq!(drain(&fake, OWNER), [], "the plugin heard of nothing");
    serving::server_started(&fake, key, "192.168.1.20:8080").unwrap();
    assert!(matches!(
        served_head(&fake, key, "GET", "/", None, false, 10).unwrap(),
        Decision::Accept { .. }
    ));
}
