//! What shells report about HTTP requests, servers, sockets, broadcasts
//! and speech, when it is unexpected: late (after the work failed or was
//! closed), twice, for numbers never given out, or out of order. Plugins
//! hear of each piece of work once, and nothing is left behind on the
//! storage.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]
// Tests panic on purpose when something is wrong.

mod common;

use common::AddDriver;

use common::{DEVICE, Harness, REPLY};
use paperpaw_core::Ask;
use paperpaw_core::sdk::wifi::{NETWORK_WAIT_MS, WifiState};
use paperpaw_core::sdk::{
    CertificateHash, HttpError, HttpRequest, Id, KeyHash, MemoryStorage, Params, ServerResponse,
    SocketError, SocketRead, Speech, Tls,
};
use paperpaw_core::{
    HttpResponse, Io, Kernel, KernelBuilder, ServerCall, ServerEvent, SocketCall, SocketEvent,
    SpeechCall, WifiEvent,
};
use paperpaw_platform::Instant;

const SERVER: Id = Id::new_static("drv.server");
const SOCKET: Id = Id::new_static("drv.socket");

fn kernel(harness: &Harness, setup: impl FnOnce(KernelBuilder) -> KernelBuilder) -> Kernel {
    let builder = Kernel::builder(DEVICE)
        .driver(harness.plugin())
        .storage(MemoryStorage::new());
    let mut kernel = setup(builder).build().unwrap();
    kernel.start(Instant(0));
    common::render(&mut kernel);
    harness.take();
    kernel
}

fn replies(harness: &Harness) -> Vec<String> {
    harness.lines("event drv.reply")
}

fn get(id: &str) -> Ask {
    Ask::http(
        HttpRequest::get("http://example.org/", REPLY)
            .with(Params::new().with("n", String::from(id))),
    )
}

fn http_calls(kernel: &mut Kernel) -> Vec<paperpaw_core::HttpCall> {
    kernel
        .take_io()
        .into_iter()
        .filter_map(|io| match io {
            Io::Http(call) => Some(call),
            _ => None,
        })
        .collect()
}

fn ok(id: u32, body: &str) -> HttpResponse {
    HttpResponse {
        id,
        status: 200,
        body: body.as_bytes().to_vec(),
        ..HttpResponse::default()
    }
}

#[test]
fn an_http_answer_given_twice_or_for_no_request_is_heard_once() {
    let harness = Harness::new();
    let mut kernel = kernel(&harness, |b| b.network(true));
    harness.run(&mut kernel, 10, [get("a"), get("b")]);
    let calls = http_calls(&mut kernel);
    assert_eq!(calls.len(), 2);
    assert_ne!(calls[0].id, calls[1].id);
    // Out of order, twice, and for numbers never given out.
    for (at, response) in [
        (20, ok(calls[1].id, "B")),
        (21, ok(calls[1].id, "B again")),
        (22, ok(calls[0].id, "A")),
        (23, ok(calls[0].id, "A again")),
        (24, ok(0, "nobody")),
        (25, ok(u32::MAX, "nobody")),
        (
            26,
            HttpResponse::failed(calls[0].id, HttpError::Timeout, "late failure"),
        ),
    ] {
        kernel.http_response(Instant(at), response);
    }
    let replies = replies(&harness);
    assert_eq!(replies.len(), 2, "{replies:?}");
    assert!(replies[0].contains("n=b") && replies[0].contains("body=B"));
    assert!(replies[1].contains("n=a") && replies[1].contains("body=A"));
}

#[test]
fn an_answer_that_is_not_utf8_reaches_the_plugin_whole() {
    let harness = Harness::new();
    let mut kernel = kernel(&harness, |b| b.network(true));
    harness.run(&mut kernel, 10, [get("a")]);
    let call = http_calls(&mut kernel).remove(0);
    kernel.http_response(
        Instant(20),
        HttpResponse {
            id: call.id,
            status: 200,
            body: vec![0xff, 0xfe, b'o', b'k', 0xc3],
            ..HttpResponse::default()
        },
    );
    let replies = replies(&harness);
    assert_eq!(replies.len(), 1);
    assert!(
        replies[0].contains("ok") && replies[0].contains("bytes=5"),
        "{replies:?}"
    );
}

#[test]
fn shells_with_memory_to_spare_load_a_file_to_send_whole() {
    let harness = Harness::new();
    let mut kernel = kernel(&harness, |b| b.network(true));
    let data: Vec<u8> = (0..=255).cycle().take(70_000).collect();
    kernel.storage().write("up.bin", &data).unwrap();
    kernel.storage().write("empty.bin", b"").unwrap();
    let put = |path: &str| {
        Ask::http(HttpRequest::new("PUT", "http://example.org/up", REPLY).body_from(path))
    };
    harness.run(&mut kernel, 10, [put("up.bin"), put("empty.bin")]);
    let mut calls = http_calls(&mut kernel);
    assert_eq!(calls.len(), 2);
    calls[0].load_body(kernel.storage()).unwrap();
    assert_eq!(calls[0].body.as_deref(), Some(&data[..]));
    assert_eq!(calls[0].body_from.as_deref(), Some("up.bin"));
    calls[1].load_body(kernel.storage()).unwrap();
    assert_eq!(calls[1].body.as_deref(), Some(&[][..]));
    // Gone since it was asked for: an error, for the shell to answer.
    let mut call = calls[0].clone();
    call.body_from = Some(String::from("up.bin"));
    kernel.storage().remove("up.bin").unwrap();
    assert!(call.load_body(kernel.storage()).is_err());
}

#[test]
fn a_failed_download_writes_nothing_where_the_file_was_to_go() {
    let harness = Harness::new();
    let mut kernel = kernel(&harness, |b| b.network(true));
    kernel
        .storage()
        .write("books/kept.epub", b"the old book")
        .unwrap();
    harness.run(
        &mut kernel,
        10,
        [Ask::http(
            HttpRequest::get("http://example.org/book", REPLY).save_to("books/kept.epub"),
        )],
    );
    let call = http_calls(&mut kernel).remove(0);
    kernel.http_response(
        Instant(20),
        HttpResponse::failed(call.id, HttpError::Timeout, "timed out"),
    );
    let kept = paperpaw_core::sdk::read_file(kernel.storage(), "books/kept.epub", 1024).unwrap();
    assert_eq!(kept, b"the old book");
    assert!(replies(&harness)[0].contains("status=0"));
}

/// Where PaperPaw manages Wi-Fi: work waits for a network, then fails;
/// what the shell answers after that is not heard again.
#[test]
fn work_that_waited_too_long_fails_once_and_late_answers_are_dropped() {
    let harness = Harness::new();
    let mut kernel = kernel(&harness, |b| b.wifi(true));
    harness.run(
        &mut kernel,
        1000,
        [
            get("a"),
            Ask::serve(SERVER, 8080, REPLY),
            Ask::connect(SOCKET, String::from("calibre.local"), 9090, REPLY),
        ],
    );
    // Nothing for the shell but the radio.
    assert!(
        kernel.take_io().iter().all(|io| matches!(io, Io::Wifi(_))),
        "network work went out without a network"
    );
    let effects = kernel.tick(Instant(1000 + NETWORK_WAIT_MS - 1));
    assert!(replies(&harness).is_empty());
    assert_eq!(effects.wake_at(), Some(Instant(1000 + NETWORK_WAIT_MS)));
    kernel.tick(Instant(1000 + NETWORK_WAIT_MS));
    let failed = replies(&harness);
    assert_eq!(failed.len(), 3, "{failed:?}");
    // The network comes after all: nothing waits any more.
    let online = WifiState::Online {
        ssid: String::from("Home"),
        address: String::from("10.0.0.2"),
        name: None,
    };
    kernel.wifi_event(Instant(40_000), WifiEvent::State(online));
    let io = kernel.take_io();
    assert!(
        io.iter()
            .all(|io| !matches!(io, Io::Http(_) | Io::Server(_) | Io::Socket(_))),
        "{io:?}"
    );
    // A shell that made them anyway answers late: not heard.
    kernel.http_response(Instant(40_001), ok(1, "late"));
    kernel.server_event(
        Instant(40_002),
        ServerEvent::Started {
            server: 1,
            address: String::from("10.0.0.2:8080"),
        },
    );
    kernel.socket_event(Instant(40_003), SocketEvent::Connected { socket: 1 });
    assert_eq!(replies(&harness).len(), 3);
    assert!(!kernel.serving() && !kernel.sockets_open());
}

/// The number the shell was given for the one server running.
fn listening(kernel: &mut Kernel) -> u32 {
    kernel
        .take_io()
        .into_iter()
        .find_map(|io| match io {
            Io::Server(ServerCall::Listen { server, .. }) => Some(server),
            _ => None,
        })
        .expect("a server to listen")
}

fn serve() -> Ask {
    Ask::serve(SERVER, 8080, REPLY)
}

fn request(server: u32, request: u32, target: &str, body: bool) -> ServerEvent {
    ServerEvent::Request {
        server,
        request,
        method: String::from("PUT"),
        target: String::from(target),
        body,
    }
}

fn body(request: u32, data: &[u8], last: bool) -> ServerEvent {
    ServerEvent::Body {
        request,
        data: data.to_vec(),
        last,
    }
}

/// The files kept for requests' bodies.
fn bodies(kernel: &Kernel) -> Vec<String> {
    let mut names = Vec::new();
    let _ = kernel.storage().list(".paperpaw/tmp", &mut |entry| {
        names.push(String::from(entry.name));
    });
    names
}

#[test]
fn server_reports_for_servers_never_started_or_stopped_are_ignored() {
    let harness = Harness::new();
    let mut kernel = kernel(&harness, |b| b.network(true));
    harness.run(&mut kernel, 10, [serve()]);
    let number = listening(&mut kernel);
    harness.run(&mut kernel, 20, [Ask::StopServing(SERVER)]);
    let stopped = replies(&harness);
    assert_eq!(stopped.len(), 1, "{stopped:?}");
    for (at, event) in [
        (
            30,
            ServerEvent::Started {
                server: number,
                address: String::from("late"),
            },
        ),
        (
            31,
            ServerEvent::Failed {
                server: number,
                reason: String::from("late"),
            },
        ),
        (32, request(number, 1, "/late", false)),
        (33, request(number + 100, 2, "/nobody", false)),
        (34, body(99, b"for no request", true)),
        (35, ServerEvent::Closed { request: 12345 }),
    ] {
        kernel.server_event(Instant(at), event);
    }
    assert_eq!(replies(&harness), stopped);
    assert!(!kernel.serving());
}

#[test]
fn a_body_for_a_server_that_stopped_is_not_kept() {
    let harness = Harness::new();
    let mut kernel = kernel(&harness, |b| b.network(true));
    harness.run(&mut kernel, 10, [serve()]);
    let number = listening(&mut kernel);
    kernel.server_event(Instant(20), request(number, 7, "/up", true));
    kernel.server_event(Instant(21), body(7, b"half", false));
    harness.run(&mut kernel, 22, [Ask::StopServing(SERVER)]);
    kernel.server_event(Instant(23), body(7, b" and the rest", true));
    assert!(bodies(&kernel).is_empty(), "{:?}", bodies(&kernel));
    assert!(
        harness
            .lines("event drv.reply")
            .iter()
            .all(|l| !l.contains("kind=request")),
        "{:?}",
        harness.log.borrow()
    );
}

#[test]
fn a_connection_lost_while_its_body_comes_leaves_nothing() {
    let harness = Harness::new();
    let mut kernel = kernel(&harness, |b| b.network(true));
    harness.run(&mut kernel, 10, [serve()]);
    let number = listening(&mut kernel);
    kernel.server_event(Instant(20), request(number, 7, "/up", true));
    kernel.server_event(Instant(21), body(7, b"half", false));
    assert_eq!(bodies(&kernel).len(), 1);
    kernel.server_event(Instant(22), ServerEvent::Closed { request: 7 });
    assert!(bodies(&kernel).is_empty());
    // What comes after is dropped, and no file comes back.
    kernel.server_event(Instant(23), body(7, b"rest", true));
    kernel.server_event(Instant(24), ServerEvent::Closed { request: 7 });
    assert!(bodies(&kernel).is_empty());
    assert!(
        replies(&harness)
            .iter()
            .all(|l| !l.contains("kind=request")),
        "{:?}",
        replies(&harness)
    );
}

#[test]
fn a_request_answered_twice_is_passed_on_twice_but_its_body_goes_once() {
    let harness = Harness::new();
    let mut kernel = kernel(&harness, |b| b.network(true));
    harness.run(&mut kernel, 10, [serve()]);
    let number = listening(&mut kernel);
    kernel.server_event(Instant(20), request(number, 7, "/up", true));
    kernel.server_event(Instant(21), body(7, b"data", true));
    let told = replies(&harness);
    assert_eq!(told.len(), 1);
    assert!(
        told[0].contains("body-file=.paperpaw/tmp/request-7"),
        "{told:?}"
    );
    let respond = || Ask::respond(ServerResponse::new(7, 200, "text/plain", b"ok".to_vec()));
    harness.run(&mut kernel, 22, [respond(), respond()]);
    assert!(bodies(&kernel).is_empty());
    let responds = kernel
        .take_io()
        .into_iter()
        .filter(|io| matches!(io, Io::Server(ServerCall::Respond { request: 7, .. })))
        .count();
    assert_eq!(responds, 2);
}

#[test]
fn an_answer_from_a_file_that_is_not_there_is_not_found() {
    let harness = Harness::new();
    let mut kernel = kernel(&harness, |b| b.network(true));
    harness.run(&mut kernel, 10, [serve()]);
    let number = listening(&mut kernel);
    kernel.server_event(Instant(20), request(number, 3, "/missing", false));
    let mut response = ServerResponse::new(3, 200, "application/epub+zip", Vec::new());
    response = response.body_from(String::from("books/gone.epub"));
    harness.run(&mut kernel, 21, [Ask::respond(response)]);
    let answers: Vec<(u16, Vec<u8>, bool)> = kernel
        .take_io()
        .into_iter()
        .filter_map(|io| match io {
            Io::Server(ServerCall::Respond {
                status, body, file, ..
            }) => Some((status, body, file.is_some())),
            _ => None,
        })
        .collect();
    assert_eq!(answers, [(404, b"not found".to_vec(), false)]);
}

/// A shell that reports a request twice (or reuses its number) must not
/// have a later request reported with the first one's method and target.
#[test]
fn a_request_reported_twice_does_not_confuse_a_later_one() {
    let harness = Harness::new();
    let mut kernel = kernel(&harness, |b| b.network(true));
    harness.run(&mut kernel, 10, [serve()]);
    let number = listening(&mut kernel);
    kernel.server_event(Instant(20), request(number, 7, "/first", true));
    kernel.server_event(Instant(21), request(number, 7, "/first", true));
    kernel.server_event(Instant(22), body(7, b"one", true));
    harness.run(
        &mut kernel,
        23,
        [Ask::respond(ServerResponse::new(
            7,
            200,
            "text/plain",
            Vec::new(),
        ))],
    );
    harness.take();
    // Much later the shell's numbers come round again.
    kernel.server_event(Instant(30), request(number, 7, "/second", true));
    kernel.server_event(Instant(31), body(7, b"two", true));
    let told = replies(&harness);
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(told[0].contains("target=/second"), "{told:?}");
}

fn connect() -> Ask {
    Ask::connect(SOCKET, String::from("calibre.local"), 9090, REPLY)
}

/// The number the shell was given for the socket.
fn connecting(kernel: &mut Kernel) -> u32 {
    kernel
        .take_io()
        .into_iter()
        .find_map(|io| match io {
            Io::Socket(SocketCall::Connect { socket, .. }) => Some(socket),
            _ => None,
        })
        .expect("a connection")
}

#[test]
fn socket_reports_after_it_closed_or_for_other_numbers_are_ignored() {
    let harness = Harness::new();
    let mut kernel = kernel(&harness, |b| b.network(true));
    harness.run(&mut kernel, 10, [connect()]);
    let number = connecting(&mut kernel);
    kernel.socket_event(Instant(20), SocketEvent::Connected { socket: number });
    // Data never asked for.
    kernel.socket_event(
        Instant(21),
        SocketEvent::Data {
            socket: number,
            data: b"unasked".to_vec(),
        },
    );
    harness.run(&mut kernel, 22, [Ask::Disconnect(SOCKET)]);
    let before = replies(&harness);
    assert_eq!(before.len(), 2, "connected, closed: {before:?}");
    for (at, event) in [
        (30, SocketEvent::Connected { socket: number }),
        (
            31,
            SocketEvent::Data {
                socket: number,
                data: b"late".to_vec(),
            },
        ),
        (
            32,
            SocketEvent::Failed {
                socket: number,
                error: SocketError::Refused,
                reason: String::from("late"),
            },
        ),
        (33, SocketEvent::Connected { socket: number + 7 }),
        (
            34,
            SocketEvent::Answer {
                broadcast: 3,
                from: String::from("10.0.0.9"),
                data: b"nobody asked".to_vec(),
            },
        ),
        (35, SocketEvent::Done { broadcast: 3 }),
    ] {
        kernel.socket_event(Instant(at), event);
    }
    assert_eq!(replies(&harness), before);
    assert!(!kernel.sockets_open());
}

#[test]
fn a_socket_failing_twice_is_heard_of_once() {
    let harness = Harness::new();
    let mut kernel = kernel(&harness, |b| b.network(true));
    harness.run(&mut kernel, 10, [connect()]);
    let number = connecting(&mut kernel);
    for at in [20, 21] {
        kernel.socket_event(
            Instant(at),
            SocketEvent::Failed {
                socket: number,
                error: SocketError::Refused,
                reason: String::from("refused"),
            },
        );
    }
    let told = replies(&harness);
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(told[0].contains("kind=failed"));
    // Asks for it now do nothing.
    harness.run(
        &mut kernel,
        30,
        [
            Ask::socket_send(SOCKET, b"x".to_vec()),
            Ask::socket_read(SOCKET, SocketRead::Text { max: 10 }),
            Ask::Disconnect(SOCKET),
        ],
    );
    assert!(kernel.take_io().is_empty());
}

#[test]
fn a_tls_socket_is_heard_of_once_secure() {
    let harness = Harness::new();
    let mut kernel = kernel(&harness, |b| b.network(true).tls_sessions(1));
    let connect = Ask::connect_tls(SOCKET, "mail.example.org", 993, REPLY, Tls::new());
    harness.run(&mut kernel, 10, [connect]);
    let calls = kernel.take_io();
    let tls = calls.iter().find_map(|io| match io {
        Io::Socket(SocketCall::Connect {
            tls: Some(tls),
            socket,
            ..
        }) => Some((*socket, tls.server_name.clone())),
        _ => None,
    });
    let Some((number, name)) = tls else {
        panic!("{calls:?}");
    };
    assert_eq!(name, "mail.example.org");
    let certificate = CertificateHash::new([1; 32]);
    kernel.socket_event(
        Instant(20),
        // From a shell that does not know the time yet.
        SocketEvent::Secured {
            socket: number,
            certificate,
            key: KeyHash::new([2; 32]),
            time_checked: false,
        },
    );
    let told = replies(&harness);
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(told[0].contains("kind=secured"), "{told:?}");
    assert!(told[0].contains("time-checked=false"), "{told:?}");
    assert!(
        told[0].contains(&format!("certificate={certificate}")),
        "{told:?}"
    );
}

#[test]
fn tls_sockets_fail_at_once_where_the_shell_does_no_tls() {
    let harness = Harness::new();
    let mut kernel = kernel(&harness, |b| b.network(true));
    let connect = Ask::connect_tls(SOCKET, "mail.example.org", 993, REPLY, Tls::new());
    harness.run(&mut kernel, 10, [connect]);
    assert!(kernel.take_io().is_empty());
    let told = replies(&harness);
    assert!(
        told[0].contains("kind=failed") && told[0].contains("error=unsupported"),
        "{told:?}"
    );
}

#[test]
fn a_file_read_from_a_socket_closed_halfway_is_reported_closed() {
    let harness = Harness::new();
    let mut kernel = kernel(&harness, |b| b.network(true));
    harness.run(&mut kernel, 10, [connect()]);
    let number = connecting(&mut kernel);
    kernel.socket_event(Instant(20), SocketEvent::Connected { socket: number });
    harness.run(
        &mut kernel,
        21,
        [Ask::socket_read(
            SOCKET,
            SocketRead::File {
                path: String::from("books/new.epub"),
                len: 10,
            },
        )],
    );
    let reads: Vec<u32> = kernel
        .take_io()
        .into_iter()
        .filter_map(|io| match io {
            Io::Socket(SocketCall::Read { max, .. }) => Some(max),
            _ => None,
        })
        .collect();
    assert_eq!(reads, [10]);
    kernel.socket_event(
        Instant(22),
        SocketEvent::Data {
            socket: number,
            data: b"1234".to_vec(),
        },
    );
    // The other side closes: an empty read.
    kernel.socket_event(
        Instant(23),
        SocketEvent::Data {
            socket: number,
            data: Vec::new(),
        },
    );
    let told = replies(&harness);
    assert!(told.last().unwrap().contains("kind=closed"), "{told:?}");
    assert!(!kernel.sockets_open());
    // Data after it closed: nothing more is written.
    kernel.socket_event(
        Instant(24),
        SocketEvent::Data {
            socket: number,
            data: b"56".to_vec(),
        },
    );
    assert_eq!(kernel.storage().size("books/new.epub").unwrap(), 4);
}

#[test]
fn a_broadcast_done_twice_is_heard_of_once_and_answers_after_are_dropped() {
    let harness = Harness::new();
    let mut kernel = kernel(&harness, |b| b.network(true));
    harness.run(
        &mut kernel,
        10,
        [Ask::broadcast(
            SOCKET,
            vec![54982],
            b"hello".to_vec(),
            500,
            REPLY,
        )],
    );
    let number = kernel
        .take_io()
        .into_iter()
        .find_map(|io| match io {
            Io::Socket(SocketCall::Broadcast { broadcast, .. }) => Some(broadcast),
            _ => None,
        })
        .unwrap();
    kernel.socket_event(Instant(20), SocketEvent::Done { broadcast: number });
    kernel.socket_event(Instant(21), SocketEvent::Done { broadcast: number });
    kernel.socket_event(
        Instant(22),
        SocketEvent::Answer {
            broadcast: number,
            from: String::from("10.0.0.5"),
            data: b"late".to_vec(),
        },
    );
    let told = replies(&harness);
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(told[0].contains("kind=done"));
    assert!(!kernel.sockets_open());
}

#[test]
fn speech_reported_done_twice_or_for_texts_replaced_is_heard_once() {
    let harness = Harness::new();
    let mut kernel = kernel(&harness, |b| b.speech(true));
    let say = |text: &str, queue: bool| {
        let params = Params::new().with("n", String::from(text));
        Ask::speak(Speech::new(text, REPLY).with(params).queued(queue))
    };
    harness.run(&mut kernel, 10, [say("one", false), say("two", false)]);
    let ids: Vec<u32> = kernel
        .take_io()
        .into_iter()
        .filter_map(|io| match io {
            Io::Speech(SpeechCall::Say { id, .. }) => Some(id),
            _ => None,
        })
        .collect();
    assert_eq!(ids.len(), 2);
    assert!(kernel.speaking());
    // "one" was replaced: its end is not reported.
    kernel.speech_done(Instant(20), ids[0], true);
    assert!(replies(&harness).is_empty());
    kernel.speech_done(Instant(21), ids[1], true);
    kernel.speech_done(Instant(22), ids[1], false);
    kernel.speech_done(Instant(23), u32::MAX, true);
    let told = replies(&harness);
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(told[0].contains("n=two") && !told[0].contains("failed"));
    assert!(!kernel.speaking());
}

#[test]
fn without_the_shells_help_everything_fails_at_once() {
    let harness = Harness::new();
    let mut kernel = kernel(&harness, |b| b);
    harness.run(
        &mut kernel,
        10,
        [
            get("a"),
            serve(),
            connect(),
            Ask::speak(Speech::new("hello", REPLY)),
        ],
    );
    let told = replies(&harness);
    assert_eq!(told.len(), 4, "{told:?}");
    assert!(kernel.take_io().is_empty());
    assert!(!kernel.serving() && !kernel.sockets_open() && !kernel.speaking());
}

#[test]
fn clipboard_text_nobody_asked_for_reaches_no_plugin() {
    let harness = Harness::new();
    let mut kernel = kernel(&harness, |b| b.system_clipboard(true));
    kernel.clipboard_text(Instant(10), Some(String::from("unasked")));
    kernel.clipboard_text(Instant(11), None);
    assert!(replies(&harness).is_empty());
    // Asked once, answered twice: heard once, cut to what fits at a
    // character's boundary.
    harness.run(&mut kernel, 20, [Ask::ReadClipboard(REPLY, Params::new())]);
    assert_eq!(kernel.take_io(), [Io::ReadClipboard]);
    let long = "\u{e4}".repeat(paperpaw_core::sdk::MAX_CLIPBOARD);
    kernel.clipboard_text(Instant(21), Some(long));
    kernel.clipboard_text(Instant(22), Some(String::from("again")));
    let told = replies(&harness);
    assert_eq!(told.len(), 1, "{}", told.len());
    assert!(told[0].len() <= paperpaw_core::sdk::MAX_CLIPBOARD + 100);
}
