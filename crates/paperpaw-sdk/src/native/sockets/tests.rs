#![allow(clippy::unwrap_used, reason = "fixture refusals must fail tests")]
use super::*;
use crate::native::declare::records_wire::Record;
use alloc::{format, vec, vec::Vec};
use core::{
    cell::{Cell, RefCell},
    ffi::c_void,
};
use paperpaw_native::host_glue::{self as abi, PpBuffer, PpBytes, PpHost};

/// A scripted host: the replies queued are given in order, then `reply`
/// again and again; every call is recorded.
#[derive(Default)]
struct State {
    calls: RefCell<Vec<(u32, Vec<u8>)>>,
    queue: RefCell<Vec<Result<Vec<u8>, i32>>>,
    reply: RefCell<Vec<u8>>,
    error: Cell<i32>,
}
impl State {
    fn count(&self) -> usize {
        self.calls.borrow().len()
    }
    fn op(&self, at: usize) -> u32 {
        self.calls.borrow()[at].0
    }
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
        assert_eq!(
            core::slice::from_raw_parts(key.ptr, key.len),
            b"core.sockets"
        );
        out.write(9);
    }
    assert_eq!((major, minor), (1, 0));
    abi::OK
}
unsafe extern "C" fn call(ctx: *mut c_void, _: u32, op: u32, input: PpBytes, out: PpBuffer) -> i32 {
    // SAFETY: fixture state and SDK buffers remain live through this call.
    let state = unsafe { &*ctx.cast::<State>() };
    // SAFETY: SDK encoded a live input slice.
    let input = unsafe { core::slice::from_raw_parts(input.ptr, input.len) }.to_vec();
    state.calls.borrow_mut().push((op, input));
    if state.error.get() < 0 {
        return state.error.get();
    }
    let next = {
        let mut queue = state.queue.borrow_mut();
        (!queue.is_empty()).then(|| queue.remove(0))
    };
    let reply = match next {
        Some(Err(status)) => return status,
        Some(Ok(bytes)) => bytes,
        None => state.reply.borrow().clone(),
    };
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
fn encoded(rows: &[Record<'_>]) -> Vec<u8> {
    let mut bytes = vec![0; 8192];
    let mut n = 0;
    for row in rows {
        n += wire::record_encode_one(row, &mut bytes[n..]).unwrap();
    }
    bytes.truncate(n);
    bytes
}
fn reply(state: &State, rows: &[Record<'_>]) {
    state.reply.replace(encoded(rows));
}
fn then(state: &State, rows: &[Record<'_>]) {
    state.queue.borrow_mut().push(Ok(encoded(rows)));
}
fn started(socket: u32) -> Record<'static> {
    Record::ServiceSockStarted(wire::ServiceSockStarted { socket })
}
fn bytes(data: &[u8]) -> Record<'_> {
    Record::ServiceSockBytes(wire::ServiceSockBytes { data })
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
fn id(n: u32) -> SocketId {
    SocketId::new(n).unwrap()
}
/// The rows of call `at`, decoded.
fn rows(state: &State, at: usize) -> Vec<Record<'static>> {
    let calls = state.calls.borrow();
    let bytes: &'static [u8] = Vec::leak(calls[at].1.clone());
    wire::records_decode(bytes).unwrap().iter().collect()
}
const IN: usize = INPUT;

#[test]
fn a_plain_connect_goes_out_as_one_row_and_answers_the_id() {
    with_host(|host, state| {
        let sockets = Sockets::open(host).unwrap();
        let connect = Connect::new(5, "calibre.local", 9090)
            .unwrap()
            .timeout_ms(30_000)
            .unwrap()
            .idle_ms(120_000)
            .unwrap();
        reply(state, &[started(31)]);
        let (mut input, mut out) = ([0; IN], [0; SMALL_REPLY]);
        let got = sockets
            .connect(host, &connect, &mut input, &mut out)
            .unwrap();
        assert_eq!(got.get(), 31);
        assert_eq!(state.op(0), schema::OP_CONNECT);
        let rows = rows(state, 0);
        assert_eq!(rows.len(), 1);
        assert!(matches!(rows[0], Record::ServiceSockConnect(c)
            if (c.token, c.port, c.flags, c.timeout_ms, c.idle_ms) == (5, 9090, 0, 30_000, 120_000)
                && c.host == "calibre.local" && c.server_name.is_empty()));
    });
}

#[test]
fn a_tls_connect_carries_its_flag_its_name_and_its_pins_in_order() {
    with_host(|host, state| {
        let sockets = Sockets::open(host).unwrap();
        let pins = [Pin::public_key([1; 32]), Pin::certificate([2; 32])];
        let connect = Connect::new(1, "imap.example.org", 993)
            .unwrap()
            .tls()
            .server_name("mail.example.org")
            .unwrap()
            .pins(&pins)
            .unwrap();
        reply(state, &[started(2)]);
        let (mut input, mut out) = ([0; IN], [0; SMALL_REPLY]);
        sockets
            .connect(host, &connect, &mut input, &mut out)
            .unwrap();
        let rows = rows(state, 0);
        assert_eq!(rows.len(), 3);
        assert!(matches!(rows[0], Record::ServiceSockConnect(c)
            if c.flags == 1 && c.server_name == "mail.example.org" && c.port == 993));
        assert!(matches!(rows[1], Record::ServiceSockPin(p)
            if p.kind == 1 && p.hash == [1; 32]));
        assert!(matches!(rows[2], Record::ServiceSockPin(p)
            if p.kind == 0 && p.hash == [2; 32]));
    });
}

#[test]
fn a_connect_is_checked_as_the_host_checks_it_before_any_call() {
    with_host(|host, state| {
        let sockets = Sockets::open(host).unwrap();
        for host_name in ["", "a..b", "-.x y", "bad host", "[zz]", "[]", "[1::2"] {
            assert_eq!(
                Connect::new(1, host_name, 80).map(|_| ()),
                Err(Error::Invalid),
                "{host_name:?}"
            );
        }
        let long = "a".repeat(254);
        assert!(Connect::new(1, &long, 80).is_err());
        for good in ["a", "192.168.0.2", "[fe80::1]", "mail-1.example.org"] {
            assert!(Connect::new(1, good, 80).is_ok(), "{good}");
        }
        assert!(Connect::new(1, "a", 0).is_err());
        let base = Connect::new(1, "a", 1).unwrap();
        assert!(base.timeout_ms(60_000).is_ok());
        assert!(base.timeout_ms(60_001).is_err());
        assert!(base.idle_ms(1_800_000).is_ok());
        assert!(base.idle_ms(1_800_001).is_err());
        assert!(base.server_name("bad name").is_err());
        let five = [Pin::public_key([0; 32]); 5];
        assert!(base.pins(&five[..4]).is_ok());
        assert!(base.pins(&five).is_err());
        // Pins and a server name belong to TLS: refused before the host sees them.
        let (mut input, mut out) = ([0; IN], [0; SMALL_REPLY]);
        let pinned = base.pins(&five[..1]).unwrap();
        assert_eq!(
            sockets.connect(host, &pinned, &mut input, &mut out),
            Err(Error::Invalid)
        );
        let named = base.server_name("example.org").unwrap();
        assert_eq!(
            sockets.connect(host, &named, &mut input, &mut out),
            Err(Error::Invalid)
        );
        assert_eq!(state.count(), 0);
    });
}

#[test]
fn a_connect_reply_that_is_not_started_or_has_id_0_is_invalid() {
    with_host(|host, state| {
        let sockets = Sockets::open(host).unwrap();
        let connect = Connect::new(1, "a", 1).unwrap();
        let (mut input, mut out) = ([0; IN], [0; SMALL_REPLY]);
        reply(state, &[started(0)]);
        assert_eq!(
            sockets.connect(host, &connect, &mut input, &mut out),
            Err(Error::Invalid)
        );
        reply(
            state,
            &[Record::ServiceSockRef(wire::ServiceSockRef { socket: 3 })],
        );
        assert_eq!(
            sockets.connect(host, &connect, &mut input, &mut out),
            Err(Error::Invalid)
        );
    });
}

#[test]
fn the_hosts_refusals_come_back_unchanged() {
    with_host(|host, state| {
        let sockets = Sockets::open(host).unwrap();
        let connect = Connect::new(1, "a", 1).unwrap();
        let (mut input, mut out) = ([0; IN], [0; SMALL_REPLY]);
        for (status, error) in [
            (abi::ERR_BUSY, Error::Busy),
            (abi::ERR_NO_MEMORY, Error::NoMemory),
            (abi::ERR_STATE, Error::State),
            (abi::ERR_DENIED, Error::Denied),
            (abi::ERR_UNSUPPORTED, Error::Unsupported),
            (abi::ERR_NOT_FOUND, Error::NotFound),
            (abi::ERR_FULL, Error::Full),
        ] {
            state.error.set(status);
            assert_eq!(
                sockets.connect(host, &connect, &mut input, &mut out),
                Err(error)
            );
            assert_eq!(
                sockets.send(host, id(1), b"x", &mut input, &mut out),
                Err(error)
            );
        }
    });
}

#[test]
fn send_takes_one_to_a_piece_of_bytes_and_expects_an_empty_reply() {
    with_host(|host, state| {
        let sockets = Sockets::open(host).unwrap();
        let (mut input, mut out) = ([0; IN], [0; SMALL_REPLY]);
        sockets
            .send(host, id(7), b"A1 LOGIN\r\n", &mut input, &mut out)
            .unwrap();
        assert_eq!(state.op(0), schema::OP_SEND);
        assert!(matches!(rows(state, 0)[0], Record::ServiceSockSend(s)
            if s.socket == 7 && s.data == b"A1 LOGIN\r\n"));
        let full = [b'x'; PIECE];
        sockets
            .send(host, id(7), &full, &mut input, &mut out)
            .unwrap();
        assert_eq!(state.count(), 2);
        assert_eq!(
            sockets.send(host, id(7), &[], &mut input, &mut out),
            Err(Error::Invalid)
        );
        assert_eq!(
            sockets.send(host, id(7), &[0; PIECE + 1], &mut input, &mut out),
            Err(Error::Invalid)
        );
        assert_eq!(state.count(), 2, "refused before the host");
        reply(state, &[started(1)]);
        assert_eq!(
            sockets.send(host, id(7), b"x", &mut input, &mut out),
            Err(Error::Invalid)
        );
    });
}

#[test]
fn a_bytes_read_names_its_size_and_exactness() {
    with_host(|host, state| {
        let sockets = Sockets::open(host).unwrap();
        let (mut input, mut out) = ([0; IN], [0; SMALL_REPLY]);
        sockets
            .read_bytes(host, id(3), 1024, false, &mut input, &mut out)
            .unwrap();
        sockets
            .read_bytes(host, id(3), 4, true, &mut input, &mut out)
            .unwrap();
        assert!(matches!(rows(state, 0)[0], Record::ServiceSockRead(r)
            if (r.socket, r.mode, r.flags, r.max, r.len, r.root, r.path)
                == (3, 0, 0, 1024, 0, 0, "")));
        assert!(matches!(rows(state, 1)[0], Record::ServiceSockRead(r)
            if (r.mode, r.flags, r.max) == (0, 1, 4)));
        for bad in [0, MAX_READ + 1] {
            assert_eq!(
                sockets.read_bytes(host, id(3), bad, false, &mut input, &mut out),
                Err(Error::Invalid)
            );
        }
        assert_eq!(state.count(), 2);
    });
}

#[test]
fn a_file_read_names_root_path_and_length() {
    with_host(|host, state| {
        let sockets = Sockets::open(host).unwrap();
        let (mut input, mut out) = ([0; IN], [0; SMALL_REPLY]);
        let book = FileRead::new(Root::Files, "books/a.epub", 300_000).unwrap();
        sockets
            .read_file(host, id(3), &book, &mut input, &mut out)
            .unwrap();
        assert!(matches!(rows(state, 0)[0], Record::ServiceSockRead(r)
            if (r.socket, r.mode, r.flags, r.max, r.len, r.root, r.path)
                == (3, 1, 0, 0, 300_000, 1, "books/a.epub")));
        let most = FileRead::new(Root::Data, "m", MAX_FILE_LEN).unwrap();
        sockets
            .read_file(host, id(3), &most, &mut input, &mut out)
            .unwrap();
        assert!(matches!(rows(state, 1)[0], Record::ServiceSockRead(r) if r.root == 0));
        let long = "p".repeat(MAX_PATH + 1);
        for (path, len, error) in [
            ("a", 0, Error::Invalid),
            ("a", MAX_FILE_LEN + 1, Error::Invalid),
            ("", 1, Error::Invalid),
            (long.as_str(), 1, Error::Invalid),
        ] {
            assert_eq!(FileRead::new(Root::Data, path, len).map(|_| ()), Err(error));
        }
        assert_eq!(state.count(), 2);
    });
}

#[test]
fn data_is_borrowed_from_the_reply_and_never_longer_than_a_piece() {
    with_host(|host, state| {
        let sockets = Sockets::open(host).unwrap();
        let (mut input, mut out) = ([0; IN], [0; REPLY]);
        reply(state, &[bytes(b"* OK ready\r\n")]);
        let got = sockets.data(host, id(4), 6, &mut input, &mut out).unwrap();
        assert_eq!(got, b"* OK ready\r\n");
        assert_eq!(state.op(0), schema::OP_DATA);
        assert!(matches!(rows(state, 0)[0], Record::ServiceSockAt(a)
            if (a.socket, a.offset) == (4, 6)));
        reply(state, &[bytes(&[])]);
        assert_eq!(
            sockets.data(host, id(4), 99, &mut input, &mut out).unwrap(),
            b""
        );
    });
}

#[test]
fn a_peer_gives_the_hashes_and_the_pins_a_user_accepts() {
    with_host(|host, state| {
        let sockets = Sockets::open(host).unwrap();
        let (mut input, mut out) = ([0; IN], [0; SMALL_REPLY]);
        reply(
            state,
            &[Record::ServiceSockPeer(wire::ServiceSockPeer {
                flags: 1,
                reason: 1,
                certificate: &[3; 32],
                key: &[4; 32],
            })],
        );
        let peer = sockets.peer(host, id(2), &mut input, &mut out).unwrap();
        assert!(peer.time_checked());
        assert_eq!(peer.untrusted(), Untrusted::NotPinned);
        assert_eq!(peer.key(), Ok([4; 32]));
        assert_eq!(peer.certificate(), Ok([3; 32]));
        assert_eq!(peer.pin_key(), Ok(Pin::public_key([4; 32])));
        assert_eq!(peer.pin_certificate(), Ok(Pin::certificate([3; 32])));
        assert_eq!(state.op(0), schema::OP_PEER);
        assert!(!format!("{peer:?}").contains('4'));
        reply(
            state,
            &[Record::ServiceSockPeer(wire::ServiceSockPeer {
                flags: 0,
                reason: 9,
                certificate: &[3; 31],
                key: &[],
            })],
        );
        let peer = sockets.peer(host, id(2), &mut input, &mut out).unwrap();
        assert!(!peer.time_checked());
        assert_eq!(peer.untrusted(), Untrusted::Other(9));
        assert_eq!(peer.certificate(), Err(Error::Invalid));
        assert_eq!(peer.pin_key(), Err(Error::Invalid));
    });
}

#[test]
fn close_expects_an_empty_reply() {
    with_host(|host, state| {
        let sockets = Sockets::open(host).unwrap();
        let (mut input, mut out) = ([0; IN], [0; SMALL_REPLY]);
        sockets.close(host, id(6), &mut input, &mut out).unwrap();
        assert_eq!(state.op(0), schema::OP_CLOSE);
        assert!(matches!(rows(state, 0)[0], Record::ServiceSockRef(r) if r.socket == 6));
        reply(state, &[started(1)]);
        assert_eq!(
            sockets.close(host, id(6), &mut input, &mut out),
            Err(Error::Invalid)
        );
    });
}

#[test]
fn a_broadcast_goes_out_as_its_head_and_one_row_per_port() {
    with_host(|host, state| {
        let sockets = Sockets::open(host).unwrap();
        let ports = [54_982, 48_123, 39_001, 55_111, 44_444];
        let broadcast = Broadcast::new(2, b"hello", 1_500, &ports).unwrap();
        reply(state, &[started(8)]);
        let (mut input, mut out) = ([0; IN], [0; SMALL_REPLY]);
        let got = sockets
            .broadcast(host, &broadcast, &mut input, &mut out)
            .unwrap();
        assert_eq!(got.get(), 8);
        assert_eq!(state.op(0), schema::OP_BROADCAST);
        let rows = rows(state, 0);
        assert_eq!(rows.len(), 6);
        assert!(matches!(rows[0], Record::ServiceSockBroadcast(b)
            if (b.token, b.wait_ms, b.data) == (2, 1_500, &b"hello"[..])));
        for (row, port) in rows[1..].iter().zip(ports) {
            assert!(matches!(row, Record::ServiceSockPort(p) if p.port == u32::from(port)));
        }
    });
}

#[test]
fn a_broadcast_is_checked_before_any_call() {
    let ok = [9090];
    assert!(Broadcast::new(1, b"x", 100, &ok).is_ok());
    assert!(Broadcast::new(1, b"x", 10_000, &ok).is_ok());
    for (data, wait, ports, error) in [
        (&b""[..], 500, &ok[..], Error::Invalid),
        (&b"x"[..], 99, &ok[..], Error::Invalid),
        (&b"x"[..], 10_001, &ok[..], Error::Invalid),
        (&b"x"[..], 500, &[][..], Error::Invalid),
        (&b"x"[..], 500, &[0][..], Error::Invalid),
        (&[0; 513][..], 500, &ok[..], Error::Invalid),
        (&b"x"[..], 500, &[1; 9][..], Error::Invalid),
    ] {
        assert_eq!(
            Broadcast::new(1, data, wait, ports).map(|_| ()),
            Err(error),
            "{data:?} {wait} {ports:?}"
        );
    }
    assert!(Broadcast::new(1, &[0; 512], 500, &[1; 8]).is_ok());
}

#[test]
fn the_debug_of_a_connection_a_broadcast_and_an_outbox_keeps_data_out() {
    let connect = Connect::new(1, "mail.example.org", 993).unwrap().tls();
    let text = format!("{connect:?}");
    assert!(text.contains("mail.example.org") && text.contains("993"));
    let broadcast = Broadcast::new(1, b"SECRET", 500, &[9]).unwrap();
    assert!(!format!("{broadcast:?}").contains("SECRET"));
    let outbox = Outbox::new(b"AUTH PLAIN hunter2");
    assert!(!format!("{outbox:?}").contains("hunter2"));
    let mut buf = *b"hunter2";
    let mut reader = Reader::new(id(1), &mut buf);
    assert!(!format!("{reader:?}").contains("hunter2"));
    reader.clear();
}

// ---- STARTTLS --------------------------------------------------------------

#[test]
fn start_tls_goes_out_as_its_head_and_its_pins() {
    with_host(|host, state| {
        let sockets = Sockets::open(host).unwrap();
        let pins = [Pin::public_key([5; 32])];
        let tls = Tls::new()
            .server_name("smtp.example.org")
            .unwrap()
            .timeout_ms(25_000)
            .unwrap()
            .idle_ms(90_000)
            .unwrap()
            .pins(&pins)
            .unwrap();
        let mut none = [];
        let reader = Reader::new(id(5), &mut none);
        let (mut input, mut out) = ([0; IN], [0; SMALL_REPLY]);
        sockets
            .start_tls(host, id(5), &tls, &reader, &mut input, &mut out)
            .unwrap();
        assert_eq!(state.op(0), schema::OP_START_TLS);
        let rows = rows(state, 0);
        assert!(matches!(rows[0], Record::ServiceSockTls(t)
            if (t.socket, t.timeout_ms, t.idle_ms, t.server_name)
                == (5, 25_000, 90_000, "smtp.example.org")));
        assert!(matches!(rows[1], Record::ServiceSockPin(p) if p.kind == 1));
        assert!(Tls::new().pins(&[Pin::public_key([0; 32]); 5]).is_err());
        assert!(Tls::new().timeout_ms(60_001).is_err());
        assert!(Tls::new().idle_ms(1_800_001).is_err());
        assert!(Tls::new().server_name("no good").is_err());
    });
}

#[test]
fn a_reply_that_came_with_ready_is_never_read_after_the_switch() {
    with_host(|host, state| {
        let sockets = Sockets::open(host).unwrap();
        // The server's "ready" and an injected plaintext line arrive together.
        let arrived = b"220 ready\r\n250 AUTH PLAIN injected\r\n";
        then(state, &[bytes(arrived)]);
        let mut buf = [0; 128];
        let mut reader = Reader::new(id(5), &mut buf);
        let (mut input, mut out) = ([0; IN], [0; REPLY]);
        reader
            .fill(&sockets, host, arrived.len() as u32, &mut input, &mut out)
            .unwrap();
        assert_eq!(reader.next_line().unwrap(), Some("220 ready"));
        let calls = state.count();
        assert_eq!(
            sockets.start_tls(host, id(5), &Tls::new(), &reader, &mut input, &mut out),
            Err(Error::State)
        );
        assert_eq!(state.count(), calls, "refused before the host");
        // Using the rest up lets the switch go; dropping it does too.
        assert_eq!(reader.next_line().unwrap(), Some("250 AUTH PLAIN injected"));
        assert_eq!(reader.held(), 0);
        sockets
            .start_tls(host, id(5), &Tls::new(), &reader, &mut input, &mut out)
            .unwrap();
        assert_eq!(state.op(state.count() - 1), schema::OP_START_TLS);
    });
}

#[test]
fn a_reader_of_another_socket_does_not_clear_a_switch() {
    with_host(|host, state| {
        let sockets = Sockets::open(host).unwrap();
        let mut none = [];
        let reader = Reader::new(id(1), &mut none);
        let (mut input, mut out) = ([0; IN], [0; SMALL_REPLY]);
        assert_eq!(
            sockets.start_tls(host, id(2), &Tls::new(), &reader, &mut input, &mut out),
            Err(Error::Invalid)
        );
        assert_eq!(state.count(), 0);
    });
}

// ---- Reader ----------------------------------------------------------------

#[test]
fn fill_takes_a_long_arrival_in_pieces_by_offset() {
    with_host(|host, state| {
        let sockets = Sockets::open(host).unwrap();
        let first = [b'a'; PIECE];
        let second = [b'b'; 1160];
        then(state, &[bytes(&first)]);
        then(state, &[bytes(&second)]);
        let mut buf = [0; 5000];
        let mut reader = Reader::new(id(2), &mut buf);
        let (mut input, mut out) = ([0; IN], [0; REPLY]);
        let got = reader
            .fill(&sockets, host, 5000, &mut input, &mut out)
            .unwrap();
        assert_eq!(got, 5000);
        assert_eq!(reader.held(), 5000);
        assert_eq!(state.count(), 2);
        assert!(matches!(rows(state, 0)[0], Record::ServiceSockAt(a) if a.offset == 0));
        assert!(matches!(rows(state, 1)[0], Record::ServiceSockAt(a) if a.offset == PIECE as u32));
        assert_eq!(&reader.bytes()[PIECE - 1..PIECE + 1], b"ab");
        assert_eq!(reader.consume(10), 10);
        assert_eq!(reader.held(), 4990);
        assert_eq!(reader.consume(usize::MAX), 4990);
        assert_eq!(reader.held(), 0);
    });
}

#[test]
fn fill_takes_nothing_when_the_room_is_short() {
    with_host(|host, state| {
        let sockets = Sockets::open(host).unwrap();
        then(state, &[bytes(b"abcdef")]);
        let mut buf = [0; 8];
        let mut reader = Reader::new(id(2), &mut buf);
        let (mut input, mut out) = ([0; IN], [0; REPLY]);
        reader
            .fill(&sockets, host, 6, &mut input, &mut out)
            .unwrap();
        let calls = state.count();
        assert_eq!(
            reader.fill(&sockets, host, 3, &mut input, &mut out),
            Err(Error::NoSpace)
        );
        assert_eq!(state.count(), calls, "no host call, nothing lost halfway");
        assert_eq!(reader.held(), 6);
        // The room a used part gives back is there again.
        reader.consume(4);
        then(state, &[bytes(b"xyz")]);
        reader
            .fill(&sockets, host, 3, &mut input, &mut out)
            .unwrap();
        assert_eq!(reader.bytes(), b"efxyz");
    });
}

#[test]
fn a_zero_length_arrival_means_the_peer_closed_and_asks_nothing() {
    with_host(|host, state| {
        let sockets = Sockets::open(host).unwrap();
        let mut buf = [0; 8];
        let mut reader = Reader::new(id(2), &mut buf);
        let (mut input, mut out) = ([0; IN], [0; REPLY]);
        assert_eq!(reader.fill(&sockets, host, 0, &mut input, &mut out), Ok(0));
        assert_eq!(state.count(), 0);
    });
}

#[test]
fn a_host_that_sends_less_than_the_event_said_is_invalid_and_keeps_what_came() {
    with_host(|host, state| {
        let sockets = Sockets::open(host).unwrap();
        let (mut input, mut out) = ([0; IN], [0; REPLY]);
        // The event said 3,900 bytes; the host's pieces end after 3,840.
        then(state, &[bytes(&[7; PIECE])]);
        then(state, &[bytes(&[])]);
        let mut big = [0; 4000];
        let mut reader = Reader::new(id(2), &mut big);
        assert_eq!(
            reader.fill(&sockets, host, 3900, &mut input, &mut out),
            Err(Error::Invalid)
        );
        assert_eq!(reader.held(), PIECE);
        // A host that sends more than the event said is cut at what it said.
        then(state, &[bytes(b"abcdefgh")]);
        let mut small = [0; 16];
        let mut reader = Reader::new(id(2), &mut small);
        reader
            .fill(&sockets, host, 3, &mut input, &mut out)
            .unwrap();
        assert_eq!(reader.bytes(), b"abc");
    });
}

#[test]
fn lines_come_without_their_ends_and_a_partial_line_waits() {
    with_host(|host, state| {
        let sockets = Sockets::open(host).unwrap();
        let mut buf = [0; 64];
        let mut reader = Reader::new(id(2), &mut buf);
        let (mut input, mut out) = ([0; IN], [0; REPLY]);
        then(state, &[bytes(b"* OK one\r\n* two\nthr")]);
        reader
            .fill(&sockets, host, 19, &mut input, &mut out)
            .unwrap();
        assert_eq!(reader.next_line(), Ok(Some("* OK one")));
        assert_eq!(reader.next_line(), Ok(Some("* two")));
        assert_eq!(reader.next_line(), Ok(None));
        assert_eq!(reader.held(), 3);
        then(state, &[bytes(b"ee\r\n\r\n")]);
        reader
            .fill(&sockets, host, 6, &mut input, &mut out)
            .unwrap();
        assert_eq!(reader.next_line(), Ok(Some("three")));
        assert_eq!(reader.next_line(), Ok(Some("")));
        assert_eq!(reader.next_line(), Ok(None));
        assert_eq!(reader.held(), 0);
    });
}

#[test]
fn a_line_that_is_not_utf8_is_used_up_and_refused() {
    with_host(|host, state| {
        let sockets = Sockets::open(host).unwrap();
        let mut buf = [0; 32];
        let mut reader = Reader::new(id(2), &mut buf);
        let (mut input, mut out) = ([0; IN], [0; REPLY]);
        then(state, &[bytes(b"a\xff\xfe\nok\n")]);
        reader
            .fill(&sockets, host, 7, &mut input, &mut out)
            .unwrap();
        assert_eq!(reader.next_line(), Err(Error::Utf8));
        assert_eq!(reader.next_line(), Ok(Some("ok")));
    });
}

#[test]
fn a_line_longer_than_the_buffer_is_no_space_not_a_wait_for_ever() {
    with_host(|host, state| {
        let sockets = Sockets::open(host).unwrap();
        let mut buf = [0; 8];
        let mut reader = Reader::new(id(2), &mut buf);
        let (mut input, mut out) = ([0; IN], [0; REPLY]);
        then(state, &[bytes(b"12345678")]);
        reader
            .fill(&sockets, host, 8, &mut input, &mut out)
            .unwrap();
        assert_eq!(reader.next_line(), Err(Error::NoSpace));
        reader.clear();
        assert_eq!(reader.held(), 0);
        assert!(
            reader.raw().iter().all(|&b| b == 0),
            "cleared bytes are wiped"
        );
    });
}

#[test]
fn an_event_length_that_could_overflow_is_no_space_without_a_call() {
    with_host(|host, state| {
        let sockets = Sockets::open(host).unwrap();
        let mut buf = [0; 16];
        let mut reader = Reader::new(id(2), &mut buf);
        let (mut input, mut out) = ([0; IN], [0; REPLY]);
        then(state, &[bytes(b"abcd")]);
        reader
            .fill(&sockets, host, 4, &mut input, &mut out)
            .unwrap();
        let calls = state.count();
        for len in [u32::MAX, u32::MAX - 3, 13] {
            assert_eq!(
                reader.fill(&sockets, host, len, &mut input, &mut out),
                Err(Error::NoSpace),
                "{len}"
            );
        }
        assert_eq!(state.count(), calls);
        assert_eq!(reader.held(), 4);
        assert_eq!(reader.room(), 12);
    });
}

#[test]
fn a_read_never_asks_for_more_than_the_buffer_takes() {
    with_host(|host, state| {
        let sockets = Sockets::open(host).unwrap();
        let mut buf = [0; 10];
        let mut reader = Reader::new(id(2), &mut buf);
        let (mut input, mut out) = ([0; IN], [0; REPLY]);
        assert_eq!(reader.room(), 10);
        reader
            .read_bytes(&sockets, host, 1024, false, &mut input, &mut out)
            .unwrap();
        assert!(matches!(rows(state, 0)[0], Record::ServiceSockRead(r) if r.max == 10));
        // What a read of 10 brings fits, and fills the buffer.
        then(state, &[bytes(b"0123456789")]);
        reader
            .fill(&sockets, host, 10, &mut input, &mut out)
            .unwrap();
        assert_eq!(reader.room(), 0);
        let calls = state.count();
        assert_eq!(
            reader.read_bytes(&sockets, host, 4, false, &mut input, &mut out),
            Err(Error::NoSpace)
        );
        // Used bytes give room back; an exact read must fit whole.
        reader.consume(6);
        assert_eq!(reader.room(), 6);
        assert_eq!(
            reader.read_bytes(&sockets, host, 7, true, &mut input, &mut out),
            Err(Error::NoSpace)
        );
        assert_eq!(state.count(), calls, "refused before the host");
        reader
            .read_bytes(&sockets, host, 6, true, &mut input, &mut out)
            .unwrap();
        assert!(matches!(rows(state, calls)[0], Record::ServiceSockRead(r)
            if (r.max, r.flags) == (6, 1)));
        reader
            .read_bytes(&sockets, host, 3, false, &mut input, &mut out)
            .unwrap();
        assert!(matches!(rows(state, calls + 1)[0], Record::ServiceSockRead(r) if r.max == 3));
    });
}

#[test]
fn bytes_used_up_are_wiped_from_the_buffer() {
    with_host(|host, state| {
        let sockets = Sockets::open(host).unwrap();
        let (mut input, mut out) = ([0; IN], [0; REPLY]);
        let mut buf = [0xAA; 32];
        {
            let mut reader = Reader::new(id(2), &mut buf);
            reader.clear();
            then(state, &[bytes(b"LOGIN user hunter2\r\nnext")]);
            reader
                .fill(&sockets, host, 24, &mut input, &mut out)
                .unwrap();
            assert_eq!(reader.next_line(), Ok(Some("LOGIN user hunter2")));
            // The next call wipes the line it lent.
            assert_eq!(reader.next_line(), Ok(None));
            assert!(!reader.raw().windows(7).any(|w| w == b"hunter2"));
            // Used bytes are wiped at once.
            assert_eq!(reader.consume(2), 2);
            assert_eq!(reader.bytes(), b"xt");
            assert!(!reader.raw().windows(2).any(|w| w == b"ne"));
            // What moved down by the next fill leaves no copy behind.
            then(state, &[bytes(b"!")]);
            reader
                .fill(&sockets, host, 1, &mut input, &mut out)
                .unwrap();
            assert_eq!(reader.bytes(), b"xt!");
            assert!(reader.raw()[3..].iter().all(|&b| b == 0));
            then(state, &[bytes(b"pw\n")]);
            reader
                .fill(&sockets, host, 3, &mut input, &mut out)
                .unwrap();
            assert_eq!(reader.next_line(), Ok(Some("xt!pw")));
        }
        // A dropped reader leaves its buffer wiped.
        assert_eq!(buf, [0; 32]);
    });
}

// ---- Outbox ----------------------------------------------------------------

#[test]
fn an_outbox_queues_a_long_message_in_pieces() {
    with_host(|host, state| {
        let sockets = Sockets::open(host).unwrap();
        let message: Vec<u8> = (0..10_000_u32).map(|i| (i % 251) as u8).collect();
        let mut outbox = Outbox::new(&message);
        assert_eq!(outbox.remaining(), 10_000);
        let (mut input, mut out) = ([0; IN], [0; SMALL_REPLY]);
        assert_eq!(
            outbox.pump(&sockets, host, id(2), &mut input, &mut out),
            Ok(Progress::Queued)
        );
        assert!(outbox.is_queued());
        assert_eq!(state.count(), 3);
        let mut joined = Vec::new();
        let lens: Vec<usize> = (0..3)
            .map(|at| match rows(state, at)[0] {
                Record::ServiceSockSend(s) => {
                    joined.extend_from_slice(s.data);
                    s.data.len()
                }
                _ => panic!("a send row"),
            })
            .collect();
        assert_eq!(lens, [3840, 3840, 2320]);
        assert_eq!(joined, message);
    });
}

#[test]
fn a_full_queue_makes_the_outbox_wait_and_keep_the_rest() {
    with_host(|host, state| {
        let sockets = Sockets::open(host).unwrap();
        let message = [9_u8; 8000];
        let mut outbox = Outbox::new(&message);
        // The first piece fits, the second finds the queue full.
        state.queue.borrow_mut().push(Ok(Vec::new()));
        state.queue.borrow_mut().push(Err(abi::ERR_BUSY));
        let (mut input, mut out) = ([0; IN], [0; SMALL_REPLY]);
        assert_eq!(
            outbox.pump(&sockets, host, id(2), &mut input, &mut out),
            Ok(Progress::Waiting)
        );
        assert_eq!(outbox.remaining(), 8000 - 3840);
        assert!(!outbox.is_queued());
        // On the `Sent` event the same piece goes again, then the last.
        assert_eq!(
            outbox.pump(&sockets, host, id(2), &mut input, &mut out),
            Ok(Progress::Queued)
        );
        assert_eq!(state.count(), 4);
        let lens: Vec<usize> = (0..4)
            .map(|at| match rows(state, at)[0] {
                Record::ServiceSockSend(s) => s.data.len(),
                _ => 0,
            })
            .collect();
        assert_eq!(lens, [3840, 3840, 3840, 320]);
    });
}

#[test]
fn an_outbox_stops_at_a_refusal_other_than_busy() {
    with_host(|host, state| {
        let sockets = Sockets::open(host).unwrap();
        let mut outbox = Outbox::new(&[1; 100]);
        state.error.set(abi::ERR_STATE);
        let (mut input, mut out) = ([0; IN], [0; SMALL_REPLY]);
        assert_eq!(
            outbox.pump(&sockets, host, id(2), &mut input, &mut out),
            Err(Error::State)
        );
        assert_eq!(outbox.remaining(), 100);
        let mut empty = Outbox::new(&[]);
        let calls = state.count();
        assert_eq!(
            empty.pump(&sockets, host, id(2), &mut input, &mut out),
            Ok(Progress::Queued)
        );
        assert_eq!(state.count(), calls);
    });
}

// ---- Events ----------------------------------------------------------------

#[test]
fn a_socket_event_reads_its_kind_reason_and_length() {
    let event = |kind, reason, len| {
        SocketEvent::decode(wire::EventSocket {
            now_ms: 10,
            socket: 3,
            token: 8,
            kind,
            reason,
            len,
        })
        .unwrap()
    };
    let received = event(3, 0, 1024);
    assert_eq!(received.kind(), SocketKind::Received);
    assert_eq!((received.len(), received.is_empty()), (1024, false));
    assert_eq!(
        (received.id().get(), received.token(), received.now_ms()),
        (3, 8, 10)
    );
    assert!(event(3, 0, 0).is_empty());
    let kinds = [
        SocketKind::Connected,
        SocketKind::Secured,
        SocketKind::Untrusted,
        SocketKind::Received,
        SocketKind::Saved,
        SocketKind::Sent,
        SocketKind::Closed,
        SocketKind::Failed,
    ];
    for (wire_kind, kind) in (0..).zip(kinds) {
        assert_eq!(event(wire_kind, 0, 0).kind(), kind);
    }
    assert_eq!(event(99, 0, 0).kind(), SocketKind::Other(99));
    let reasons = [
        (0, Reason::None),
        (1, Reason::NoNetwork),
        (2, Reason::Dns),
        (3, Reason::Tls),
        (4, Reason::Timeout),
        (5, Reason::Refused),
        (9, Reason::Io),
        (10, Reason::Full),
        (11, Reason::Offline),
        (12, Reason::Saving),
        (13, Reason::NetworkOff),
        (20, Reason::Busy),
        (21, Reason::SessionNeeded),
        (22, Reason::Lost),
        (23, Reason::Name),
        (24, Reason::Expired),
        (25, Reason::ClosedEarly),
        (26, Reason::ScreenClosed),
        (27, Reason::WorkEnded),
        (28, Reason::NoMemory),
        (6, Reason::Other(6)),
        (99, Reason::Other(99)),
    ];
    for (wire_reason, reason) in reasons {
        assert_eq!(event(7, wire_reason, 0).reason(), reason);
    }
}

#[test]
fn an_event_without_a_socket_id_is_invalid() {
    assert_eq!(
        SocketEvent::decode(wire::EventSocket {
            now_ms: 0,
            socket: 0,
            token: 0,
            kind: 0,
            reason: 0,
            len: 0,
        }),
        Err(Error::Invalid)
    );
    assert_eq!(
        BroadcastEvent::decode(wire::EventBroadcast {
            now_ms: 0,
            socket: 0,
            token: 0,
            kind: 0,
            reason: 0,
            len: 0,
            from: "",
        }),
        Err(Error::Invalid)
    );
}

#[test]
fn a_broadcast_event_names_the_sender_of_an_answer() {
    let event = |kind, reason| {
        BroadcastEvent::decode(wire::EventBroadcast {
            now_ms: 5,
            socket: 4,
            token: 2,
            kind,
            reason,
            len: 40,
            from: "192.168.0.7:9090",
        })
        .unwrap()
    };
    let answer = event(0, 0);
    assert_eq!(answer.kind(), BroadcastKind::Answer);
    assert_eq!(answer.from(), "192.168.0.7:9090");
    assert_eq!((answer.len(), answer.is_empty()), (40, false));
    assert_eq!(
        (answer.id().get(), answer.token(), answer.now_ms()),
        (4, 2, 5)
    );
    assert_eq!(event(1, 1).kind(), BroadcastKind::Done);
    assert_eq!(event(1, 1).reason(), Reason::NoNetwork);
    assert_eq!(event(2, 11).kind(), BroadcastKind::Failed);
    assert_eq!(event(9, 0).kind(), BroadcastKind::Other(9));
}

#[test]
fn the_socket_events_decode_through_the_event_enum() {
    let mut socket = [0; 64];
    let n = wire::records_encode(
        &[Record::EventSocket(wire::EventSocket {
            now_ms: 1,
            socket: 6,
            token: 7,
            kind: 4,
            reason: 0,
            len: 0,
        })],
        &mut socket,
    )
    .unwrap();
    let Ok(crate::native::events::Event::Socket(event)) =
        crate::native::events::Event::decode(&socket[..n])
    else {
        panic!("a socket event");
    };
    assert_eq!((event.id().get(), event.kind()), (6, SocketKind::Saved));
    let mut cast = [0; 128];
    let n = wire::records_encode(
        &[Record::EventBroadcast(wire::EventBroadcast {
            now_ms: 1,
            socket: 6,
            token: 7,
            kind: 0,
            reason: 0,
            len: 3,
            from: "10.0.0.2:1",
        })],
        &mut cast,
    )
    .unwrap();
    let Ok(crate::native::events::Event::Broadcast(event)) =
        crate::native::events::Event::decode(&cast[..n])
    else {
        panic!("a broadcast event");
    };
    assert_eq!(event.from(), "10.0.0.2:1");
}

#[test]
fn the_two_events_are_subscribed_by_their_tags() {
    use crate::native::declare::EventKind;
    // The tags are the contract's 106 and 107.
    assert_eq!(wire::EVENT_SOCKET_TAG, 106);
    assert_eq!(wire::EVENT_BROADCAST_TAG, 107);
    let _ = (EventKind::Socket, EventKind::Broadcast);
}

#[test]
fn an_id_is_never_zero() {
    assert_eq!(SocketId::new(0), Err(Error::Invalid));
    assert_eq!(SocketId::new(9).map(SocketId::get), Ok(9));
}
