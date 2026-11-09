#![allow(clippy::unwrap_used, reason = "fixture refusals must fail tests")]
use super::*;
use crate::native::declare::records_wire::Record;
use alloc::{string::String, vec, vec::Vec};
use core::{
    cell::{Cell, RefCell},
    ffi::c_void,
};
use paperpaw_native::host_glue::{self as abi, PpBuffer, PpBytes, PpHost};
#[derive(Default)]
struct State {
    op: Cell<u32>,
    calls: Cell<u32>,
    input: RefCell<Vec<u8>>,
    reply: RefCell<Vec<u8>>,
    error: Cell<i32>,
}
unsafe extern "C" fn open(
    _: *mut c_void,
    key: PpBytes,
    major: u16,
    minor: u16,
    out: *mut u32,
) -> i32 {
    // SAFETY: SDK provides a live key and aligned output.
    unsafe {
        assert_eq!(core::slice::from_raw_parts(key.ptr, key.len), b"core.net");
        out.write(7);
    }
    assert_eq!((major, minor), (1, 0));
    abi::OK
}
unsafe extern "C" fn call(ctx: *mut c_void, _: u32, op: u32, input: PpBytes, out: PpBuffer) -> i32 {
    // SAFETY: fixture state and SDK buffers remain live through this call.
    let state = unsafe { &*ctx.cast::<State>() };
    state.op.set(op);
    state.calls.set(state.calls.get() + 1);
    // SAFETY: SDK encoded a live input slice.
    state
        .input
        .replace(unsafe { core::slice::from_raw_parts(input.ptr, input.len) }.to_vec());
    if state.error.get() < 0 {
        return state.error.get();
    }
    let reply = state.reply.borrow();
    assert!(reply.len() <= out.cap);
    // SAFETY: the fixture reply and exclusive SDK output are disjoint.
    unsafe {
        core::ptr::copy_nonoverlapping(reply.as_ptr(), out.ptr, reply.len());
    }
    i32::try_from(reply.len()).unwrap()
}
fn table() -> PpHost {
    let mut t = abi::host_table();
    t.service_open = open;
    t.service_call = call;
    t
}
fn reply(state: &State, rows: &[Record<'_>]) {
    let mut bytes = vec![0; 4096];
    let mut n = 0;
    for row in rows {
        n += wire::record_encode_one(row, &mut bytes[n..]).unwrap();
    }
    bytes.truncate(n);
    state.reply.replace(bytes);
}
fn started(id: u32) -> Record<'static> {
    Record::ServiceNetStarted(wire::ServiceNetStarted { id })
}
fn with_host<T>(run: impl FnOnce(&Host<'_>, &State) -> T) -> T {
    let table = table();
    let mut state = State::default();
    let host = Host {
        table: &table,
        ctx: core::ptr::from_mut(&mut state).cast(),
    };
    let state = unsafe { &*core::ptr::from_ref(&state) };
    run(&host, state)
}
#[test]
fn a_request_goes_out_as_request_headers_files_and_body_in_order() {
    with_host(|host, state| {
        let net = Net::open(host).unwrap();
        let headers = [
            Header::new("Authorization", "Token secret").unwrap(),
            Header::new("Accept", "application/json").unwrap(),
        ];
        let request = Request::new(5, "POST", "https://example.org/api?key=1")
            .unwrap()
            .headers(&headers)
            .unwrap()
            .body(b"{\"a\":1}")
            .unwrap()
            .save_to(FileRoot::Data, "answer.json")
            .unwrap()
            .progress()
            .timeout_ms(15_000)
            .unwrap()
            .max_body(8192)
            .max_file(1 << 20)
            .unwrap();
        reply(state, &[started(77)]);
        let (mut input, mut out) = ([0; REQUEST_INPUT], [0; 1536]);
        let id = net.request(host, &request, &mut input, &mut out).unwrap();
        assert_eq!(id.get(), 77);
        assert_eq!(state.op.get(), schema::OP_REQUEST);
        let bytes = state.input.borrow();
        let rows = wire::records_decode(&bytes).unwrap();
        let rows: Vec<_> = rows.iter().collect();
        assert_eq!(rows.len(), 5);
        assert!(matches!(rows[0], Record::ServiceNetRequest(r)
            if (r.token, r.method, r.flags, r.timeout_ms, r.max_body, r.max_file)
                == (5, "POST", 1, 15_000, 8192, 1 << 20)
                && r.url == "https://example.org/api?key=1"));
        assert!(matches!(rows[1], Record::ServiceNetHeader(h)
            if (h.name, h.value) == ("Authorization", "Token secret")));
        assert!(matches!(rows[2], Record::ServiceNetHeader(h) if h.name == "Accept"));
        assert!(matches!(rows[3], Record::ServiceNetFile(f)
            if (f.role, f.root, f.path) == (1, 0, "answer.json")));
        assert!(matches!(rows[4], Record::ServiceNetBody(b) if b.data == b"{\"a\":1}"));
    });
}

#[test]
fn a_file_to_send_has_role_0_and_the_user_files_root_1() {
    with_host(|host, state| {
        let net = Net::open(host).unwrap();
        let request = Request::get(1, "http://192.168.0.2:8080/up")
            .unwrap()
            .send_file(FileRoot::Files, "books/a.epub")
            .unwrap();
        reply(state, &[started(1)]);
        let (mut input, mut out) = ([0; REQUEST_INPUT], [0; 1536]);
        net.request(host, &request, &mut input, &mut out).unwrap();
        let bytes = state.input.borrow();
        let rows = wire::records_decode(&bytes).unwrap();
        let rows: Vec<_> = rows.iter().collect();
        assert_eq!(rows.len(), 2);
        assert!(matches!(rows[1], Record::ServiceNetFile(f) if (f.role, f.root) == (0, 1)));
    });
}

#[test]
fn a_request_reply_that_is_not_a_started_record_or_has_id_0_is_invalid() {
    with_host(|host, state| {
        let net = Net::open(host).unwrap();
        let request = Request::get(1, "https://example.org/").unwrap();
        let (mut input, mut out) = ([0; REQUEST_INPUT], [0; 1536]);
        reply(state, &[started(0)]);
        assert_eq!(
            net.request(host, &request, &mut input, &mut out),
            Err(Error::Invalid)
        );
        reply(
            state,
            &[Record::ServiceNetToken(wire::ServiceNetToken { token: 3 })],
        );
        assert_eq!(
            net.request(host, &request, &mut input, &mut out),
            Err(Error::Invalid)
        );
    });
}

#[test]
fn the_hosts_refusals_come_back_unchanged() {
    with_host(|host, state| {
        let net = Net::open(host).unwrap();
        let request = Request::get(1, "https://example.org/").unwrap();
        let (mut input, mut out) = ([0; REQUEST_INPUT], [0; 1536]);
        for (status, error) in [
            (abi::ERR_BUSY, Error::Busy),
            (abi::ERR_NO_MEMORY, Error::NoMemory),
            (abi::ERR_STATE, Error::State),
            (abi::ERR_DENIED, Error::Denied),
            (abi::ERR_NOT_FOUND, Error::NotFound),
        ] {
            state.error.set(status);
            assert_eq!(
                net.request(host, &request, &mut input, &mut out),
                Err(error)
            );
        }
    });
}

#[test]
fn cancel_names_the_token_and_expects_an_empty_reply() {
    with_host(|host, state| {
        let net = Net::open(host).unwrap();
        let (mut input, mut out) = ([0; REQUEST_INPUT], [0; 1536]);
        reply(state, &[]);
        net.cancel(host, 9, &mut input, &mut out).unwrap();
        assert_eq!(state.op.get(), schema::OP_CANCEL);
        assert!(matches!(
            wire::records_decode(&state.input.borrow()).unwrap().iter().next(),
            Some(Record::ServiceNetToken(t)) if t.token == 9
        ));
        reply(state, &[started(1)]);
        assert_eq!(
            net.cancel(host, 9, &mut input, &mut out),
            Err(Error::Invalid)
        );
    });
}

#[test]
fn the_answer_and_the_body_parts_are_borrowed_from_the_buffer() {
    with_host(|host, state| {
        let net = Net::open(host).unwrap();
        let id = RequestId::new(4).unwrap();
        let (mut input, mut out) = ([0; 64], [0; REPLY]);
        reply(
            state,
            &[Record::ServiceNetAnswer(wire::ServiceNetAnswer {
                status: 200,
                flags: 2,
                body_len: 5,
                etag: "\"v1\"",
                content_type: "application/json",
                final_url: "https://example.org/b",
            })],
        );
        let answer = net.answer(host, id, &mut input, &mut out).unwrap();
        assert_eq!(
            (
                answer.status(),
                answer.truncated(),
                answer.saved(),
                answer.body_len()
            ),
            (200, true, false, 5)
        );
        assert_eq!(
            (answer.etag(), answer.content_type(), answer.final_url()),
            ("\"v1\"", "application/json", "https://example.org/b")
        );
        let mut out = [0; REPLY];
        reply(
            state,
            &[Record::ServiceNetBytes(wire::ServiceNetBytes {
                data: b"hello",
            })],
        );
        let part = net.body(host, id, 0, &mut input, &mut out).unwrap();
        assert_eq!(part, b"hello");
        assert_eq!(state.op.get(), schema::OP_BODY);
        assert!(matches!(
            wire::records_decode(&state.input.borrow()).unwrap().iter().next(),
            Some(Record::ServiceNetRead(r)) if (r.id, r.offset) == (4, 0)
        ));
    });
}

#[test]
fn requests_are_checked_as_the_host_checks_them() {
    assert_eq!(
        Request::new(1, "", "https://a.b/").unwrap_err(),
        Error::Invalid
    );
    assert_eq!(
        Request::new(1, "GE T", "https://a.b/").unwrap_err(),
        Error::Invalid
    );
    assert_eq!(
        Request::new(1, "GET", "ftp://a.b/").unwrap_err(),
        Error::Invalid
    );
    assert_eq!(
        Request::new(1, "GET", "https://a.b/\n").unwrap_err(),
        Error::Invalid
    );
    assert!(Request::new(1, "PROPFIND", "HTTPS://a.b/").is_ok());
    assert_eq!(
        Request::new(1, "GETGETGETGETGETGET", "https://a.b/").unwrap_err(),
        Error::NoSpace
    );
    let long = alloc::format!("https://a.b/{}", "x".repeat(MAX_URL));
    assert_eq!(Request::get(1, &long).unwrap_err(), Error::NoSpace);
    let request = Request::get(1, "https://a.b/").unwrap();
    assert_eq!(request.timeout_ms(999).unwrap_err(), Error::Invalid);
    assert_eq!(request.timeout_ms(600_001).unwrap_err(), Error::Invalid);
    assert!(request.timeout_ms(0).is_ok() && request.timeout_ms(1_000).is_ok());
    assert_eq!(
        request.max_file((64 << 20) + 1).unwrap_err(),
        Error::NoSpace
    );
    assert!(request.max_file(64 << 20).is_ok());
    let body = [0; MAX_INLINE_BODY + 1];
    assert_eq!(request.body(&body).unwrap_err(), Error::NoSpace);
    assert!(request.body(&body[..MAX_INLINE_BODY]).is_ok());
    // A body and a file to send exclude each other.
    let with_file = request.send_file(FileRoot::Data, "a").unwrap();
    assert_eq!(with_file.body(b"x").unwrap_err(), Error::Invalid);
    let with_body = request.body(b"x").unwrap();
    assert_eq!(
        with_body.send_file(FileRoot::Data, "a").unwrap_err(),
        Error::Invalid
    );
    assert_eq!(
        request.save_to(FileRoot::Data, "").unwrap_err(),
        Error::Invalid
    );
    let many = [Header::new("A", "b").unwrap(); MAX_HEADERS + 1];
    assert_eq!(request.headers(&many).unwrap_err(), Error::NoSpace);
    assert!(request.headers(&many[..MAX_HEADERS]).is_ok());
}

#[test]
fn headers_refuse_what_the_host_refuses_and_hide_their_value() {
    assert_eq!(Header::new("", "x").unwrap_err(), Error::Invalid);
    assert_eq!(Header::new("X Y", "x").unwrap_err(), Error::Invalid);
    assert_eq!(Header::new("X", "a\r\nb").unwrap_err(), Error::Invalid);
    // A tab is a control byte too: the host refuses all of them.
    assert_eq!(Header::new("X", "a\tb").unwrap_err(), Error::Invalid);
    for own in ["Host", "connection", "Content-Length"] {
        assert_eq!(Header::new(own, "x").unwrap_err(), Error::Invalid);
    }
    assert_eq!(
        Header::new(&"n".repeat(65), "x").unwrap_err(),
        Error::NoSpace
    );
    assert_eq!(
        Header::new("n", &"v".repeat(1025)).unwrap_err(),
        Error::NoSpace
    );
    assert!(Header::new("n", &"v".repeat(1024)).is_ok());
    let debug = alloc::format!(
        "{:?}",
        Header::new("Authorization", "Bearer sekrit").unwrap()
    );
    assert!(debug.contains("Authorization") && !debug.contains("sekrit"));
}

#[test]
fn an_ended_request_reads_its_reason_and_flags() {
    let done = |reason, status, flags| {
        HttpDone::decode(wire::EventHttpDone {
            now_ms: 10,
            id: 3,
            token: 8,
            status,
            reason,
            flags,
            body_len: 12,
        })
        .unwrap()
    };
    let ok = done(0, 200, 0b110);
    assert!(ok.answered() && ok.succeeded() && ok.truncated() && ok.saved() && !ok.later());
    assert_eq!(
        (ok.id().get(), ok.token(), ok.status(), ok.body_len()),
        (3, 8, 200, 12)
    );
    let not_found = done(0, 404, 1);
    assert!(not_found.answered() && !not_found.succeeded() && not_found.later());
    assert_eq!(done(14, 0, 0).reason(), Reason::WorkEnded);
    assert_eq!(done(8, 0, 0).reason(), Reason::Cancelled);
    assert_eq!(done(99, 0, 0).reason(), Reason::Other(99));
    assert!(!done(99, 0, 0).answered());
    assert_eq!(
        HttpDone::decode(wire::EventHttpDone {
            now_ms: 0,
            id: 0,
            token: 0,
            status: 0,
            reason: 0,
            flags: 0,
            body_len: 0
        })
        .unwrap_err(),
        Error::Invalid
    );
}

#[test]
fn progress_knows_its_total_only_when_the_answer_said() {
    let progress = |flags| {
        HttpProgress::decode(wire::EventHttpProgress {
            now_ms: 1,
            id: 2,
            token: 3,
            received: 10,
            total: 100,
            flags,
        })
        .unwrap()
    };
    assert_eq!(progress(1).total(), Some(100));
    assert_eq!(progress(0).total(), None);
    assert_eq!(progress(1).received(), 10);
}

#[test]
fn a_request_hides_its_address_headers_and_body_in_debug() {
    let headers = [Header::new("Authorization", "Bearer sekrit-header").unwrap()];
    let request = Request::new(7, "POST", "https://a.b/v1?key=sekrit-query")
        .unwrap()
        .headers(&headers)
        .unwrap()
        .body(b"sekrit-body")
        .unwrap();
    let debug = alloc::format!("{request:?}");
    for secret in ["sekrit-query", "sekrit-header", "sekrit-body", "a.b"] {
        assert!(!debug.contains(secret), "{debug}");
    }
    assert!(debug.contains("POST"));
}

#[test]
fn a_file_may_be_sent_from_the_install_root_but_never_saved_to_it() {
    let request = Request::get(1, "https://a.b/").unwrap();
    assert_eq!(
        request.save_to(FileRoot::Install, "a").unwrap_err(),
        Error::Denied
    );
    with_host(|host, state| {
        let net = Net::open(host).unwrap();
        let request = request.send_file(FileRoot::Install, "a/b").unwrap();
        reply(state, &[started(1)]);
        let (mut input, mut out) = ([0; REQUEST_INPUT], [0; 1536]);
        net.request(host, &request, &mut input, &mut out).unwrap();
        let bytes = state.input.borrow();
        let rows: Vec<_> = wire::records_decode(&bytes).unwrap().iter().collect();
        assert!(matches!(rows[1], Record::ServiceNetFile(f) if (f.role, f.root) == (0, 2)));
    });
}

#[test]
fn a_request_with_the_most_rows_still_encodes_without_an_array_of_records() {
    with_host(|host, state| {
        let net = Net::open(host).unwrap();
        let headers = [Header::new("A", "b").unwrap(); MAX_HEADERS];
        let request = Request::get(1, "https://a.b/")
            .unwrap()
            .headers(&headers)
            .unwrap()
            .save_to(FileRoot::Data, "x")
            .unwrap()
            .body(b"x")
            .unwrap();
        reply(state, &[started(1)]);
        let (mut input, mut out) = ([0; REQUEST_INPUT], [0; 1536]);
        net.request(host, &request, &mut input, &mut out).unwrap();
        let bytes = state.input.borrow();
        // The leading count (a one-byte varint) is every row; the generic
        // decoder here holds 8 at most, so it is read by hand.
        assert_eq!(usize::from(bytes[0]), 1 + MAX_HEADERS + 1 + 1);
        // Too small an input is refused before anything is written.
        let mut small = [0u8; 8];
        assert_eq!(
            net.request(host, &request, &mut small, &mut out)
                .unwrap_err(),
            Error::NoSpace
        );
    });
}

fn sent_rows(state: &State) -> Vec<String> {
    let bytes = state.input.borrow();
    wire::records_decode(&bytes)
        .unwrap()
        .iter()
        .map(|row| alloc::format!("{row:?}"))
        .collect()
}

#[test]
fn serve_sends_the_token_port_and_cap_and_stop_serving_the_token() {
    with_host(|host, state| {
        let net = Net::open(host).unwrap();
        let (mut input, mut out) = ([0; 256], [0; 1536]);
        reply(state, &[]);
        let server = Server::new(4, 8080).unwrap().max_body(1 << 20).unwrap();
        net.serve(host, &server, &mut input, &mut out).unwrap();
        assert_eq!(state.op.get(), schema::OP_SERVE);
        assert!(matches!(
            wire::records_decode(&state.input.borrow()).unwrap().iter().next(),
            Some(Record::ServiceNetServe(s))
                if (s.token, s.port, s.flags, s.max_body) == (4, 8080, 0, 1 << 20)
        ));
        // The cable is port 0; the default cap is 0.
        net.serve(host, &Server::cable(5), &mut input, &mut out)
            .unwrap();
        assert!(matches!(
            wire::records_decode(&state.input.borrow()).unwrap().iter().next(),
            Some(Record::ServiceNetServe(s)) if (s.token, s.port, s.max_body) == (5, 0, 0)
        ));
        net.stop_serving(host, 4, &mut input, &mut out).unwrap();
        assert_eq!(state.op.get(), schema::OP_STOP_SERVING);
        assert!(matches!(
            wire::records_decode(&state.input.borrow()).unwrap().iter().next(),
            Some(Record::ServiceNetToken(t)) if t.token == 4
        ));
        // A reply that is not empty is not from these ops.
        reply(state, &[started(1)]);
        assert_eq!(
            net.serve(host, &server, &mut input, &mut out),
            Err(Error::Invalid)
        );
        assert_eq!(
            net.stop_serving(host, 4, &mut input, &mut out),
            Err(Error::Invalid)
        );
    });
}

#[test]
fn a_server_is_checked_before_the_host_and_the_hosts_refusals_come_back() {
    assert_eq!(Server::new(1, 0), Err(Error::Invalid));
    assert!(Server::new(1, 65535).is_ok());
    let server = Server::new(1, 80).unwrap();
    assert!(server.max_body(MAX_SERVED_BODY).is_ok());
    assert_eq!(server.max_body(MAX_SERVED_BODY + 1), Err(Error::NoSpace));
    with_host(|host, state| {
        let net = Net::open(host).unwrap();
        let (mut input, mut out) = ([0; 256], [0; 1536]);
        for (status, error) in [
            (abi::ERR_STATE, Error::State),
            (abi::ERR_BUSY, Error::Busy),
            (abi::ERR_UNSUPPORTED, Error::Unsupported),
            (abi::ERR_NO_SPACE, Error::NoSpace),
        ] {
            state.error.set(status);
            assert_eq!(net.serve(host, &server, &mut input, &mut out), Err(error));
        }
        assert_eq!(
            net.serve(host, &server, &mut [0; 4], &mut out),
            Err(Error::NoSpace)
        );
    });
}

#[test]
fn a_response_goes_out_as_head_headers_file_or_body_in_order() {
    with_host(|host, state| {
        let net = Net::open(host).unwrap();
        let headers = [
            Header::new("X-One", "1").unwrap(),
            Header::new("Cache", "none").unwrap(),
        ];
        let response = Response::new(31, 200)
            .unwrap()
            .content_type("text/html; charset=utf-8")
            .unwrap()
            .headers(&headers)
            .unwrap()
            .body(b"<p>hi</p>")
            .unwrap();
        reply(state, &[]);
        let (mut input, mut out) = ([0; REQUEST_INPUT], [0; 1536]);
        net.respond(host, &response, &mut input, &mut out).unwrap();
        assert_eq!(state.op.get(), schema::OP_RESPOND);
        {
            let bytes = state.input.borrow();
            let rows = wire::records_decode(&bytes).unwrap();
            let rows: Vec<_> = rows.iter().collect();
            assert_eq!(rows.len(), 4);
            assert!(matches!(rows[0], Record::ServiceNetResponse(r)
                if (r.request, r.status, r.content_type) == (31, 200, "text/html; charset=utf-8")));
            assert!(matches!(rows[1], Record::ServiceNetHeader(h) if h.name == "X-One"));
            assert!(matches!(rows[2], Record::ServiceNetHeader(h) if h.name == "Cache"));
            assert!(matches!(rows[3], Record::ServiceNetBody(b) if b.data == b"<p>hi</p>"));
        }
        // A file is role 0 and may come from the install root.
        let page = Response::new(32, 404)
            .unwrap()
            .send_file(FileRoot::Install, "page.html")
            .unwrap();
        net.respond(host, &page, &mut input, &mut out).unwrap();
        assert_eq!(sent_rows(state).len(), 2);
        assert!(matches!(
            wire::records_decode(&state.input.borrow()).unwrap().iter().nth(1),
            Some(Record::ServiceNetFile(f)) if (f.role, f.root, f.path) == (0, 2, "page.html")
        ));
        // No body, no file: the head alone.
        net.respond(host, &Response::new(33, 204).unwrap(), &mut input, &mut out)
            .unwrap();
        assert_eq!(sent_rows(state).len(), 1);
    });
}

#[test]
fn responses_are_checked_as_the_host_checks_them() {
    assert_eq!(Response::new(1, 99).unwrap_err(), Error::Invalid);
    assert_eq!(Response::new(1, 600).unwrap_err(), Error::Invalid);
    assert!(Response::new(1, 100).is_ok() && Response::new(1, 599).is_ok());
    let base = Response::new(1, 200).unwrap();
    assert!(base.content_type(&"t".repeat(128)).is_ok());
    assert_eq!(
        base.content_type(&"t".repeat(129)).unwrap_err(),
        Error::NoSpace
    );
    let ok = Header::new("X", "1").unwrap();
    let nine = [ok; 9];
    assert!(base.headers(&nine[..8]).is_ok());
    assert_eq!(base.headers(&nine).unwrap_err(), Error::NoSpace);
    let own = [Header::new("content-TYPE", "text/plain").unwrap()];
    assert_eq!(base.headers(&own).unwrap_err(), Error::Invalid);
    let body = [0_u8; MAX_RESPONSE_BODY + 1];
    assert!(base.body(&body[..MAX_RESPONSE_BODY]).is_ok());
    assert_eq!(base.body(&body).unwrap_err(), Error::NoSpace);
    // One of a body and a file.
    let with_body = base.body(b"x").unwrap();
    assert_eq!(
        with_body.send_file(FileRoot::Data, "a").unwrap_err(),
        Error::Invalid
    );
    let with_file = base.send_file(FileRoot::Data, "a").unwrap();
    assert_eq!(with_file.body(b"x").unwrap_err(), Error::Invalid);
    assert_eq!(
        base.send_file(FileRoot::Data, "").unwrap_err(),
        Error::Invalid
    );
    assert_eq!(
        base.send_file(FileRoot::Data, &"p".repeat(513))
            .unwrap_err(),
        Error::NoSpace
    );
}

#[test]
fn a_response_that_does_not_fit_or_is_refused_comes_back_as_an_error() {
    with_host(|host, state| {
        let net = Net::open(host).unwrap();
        let value = "v".repeat(1024);
        let headers = [Header::new("X", &value).unwrap(); 4];
        let response = Response::new(1, 200)
            .unwrap()
            .headers(&headers)
            .unwrap()
            .body(&[7; MAX_RESPONSE_BODY])
            .unwrap();
        let (mut input, mut out) = ([0; REQUEST_INPUT], [0; 1536]);
        // Four long headers and a 2 KiB body together are over the host's input.
        assert_eq!(
            net.respond(host, &response, &mut input, &mut out),
            Err(Error::NoSpace)
        );
        assert_eq!(state.calls.get(), 0);
        let small = Response::new(1, 200).unwrap();
        for (status, error) in [
            (abi::ERR_NOT_FOUND, Error::NotFound),
            (abi::ERR_NO_MEMORY, Error::NoMemory),
        ] {
            state.error.set(status);
            assert_eq!(net.respond(host, &small, &mut input, &mut out), Err(error));
        }
    });
}

#[test]
fn a_response_hides_its_headers_and_body_in_debug() {
    let secret = [Header::new("Set-Cookie", "SECRET-VALUE").unwrap()];
    let response = Response::new(1, 200)
        .unwrap()
        .headers(&secret)
        .unwrap()
        .body(b"SECRET-BODY")
        .unwrap();
    let debug = alloc::format!("{response:?}");
    assert!(!debug.contains("SECRET") && debug.contains("body_len: 11"));
}
