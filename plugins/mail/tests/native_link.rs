//! The link between a session and its socket: what is sent is queued and
//! handed over in pieces as the socket's queue has room (a full queue waits for
//! `Sent`), bytes that come reach the session a piece at a time, the notices
//! become the session's inputs, and what carried a password is wiped.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests unwrap what they just made"
)]

use paperpaw_plugin_mail::native_model::link::{Link, LinkState, Net, Notice, Peer};
use paperpaw_plugin_mail::native_model::mem::Error;
use paperpaw_plugin_mail::native_model::session::{Input, Io};
use paperpaw_sdk::native::sockets::{PIECE, Reason, SocketKind, Untrusted};

/// A socket that records what it is asked and takes `room` bytes before it
/// says `Busy`.
#[derive(Default)]
struct Double {
    calls: Vec<String>,
    taken: Vec<u8>,
    room: usize,
    came: Vec<u8>,
    tls: Option<Error>,
    peer: Option<Peer>,
}

impl Net for Double {
    fn connect(
        &mut self,
        host: &str,
        port: u16,
        tls: bool,
        pin: Option<[u8; 32]>,
    ) -> Result<(), Error> {
        self.calls.push(format!(
            "connect {host}:{port} tls={tls} pin={}",
            pin.is_some()
        ));
        Ok(())
    }
    fn start_tls(&mut self, _pin: Option<[u8; 32]>) -> Result<(), Error> {
        match self.tls {
            Some(error) => Err(error),
            None => {
                self.calls.push("start_tls".into());
                Ok(())
            }
        }
    }
    fn send(&mut self, piece: &[u8]) -> Result<(), Error> {
        assert!(!piece.is_empty() && piece.len() <= PIECE);
        if self.room < piece.len() {
            return Err(Error::Busy);
        }
        self.room -= piece.len();
        self.taken.extend_from_slice(piece);
        Ok(())
    }
    fn read_bytes(&mut self, max: u32) -> Result<(), Error> {
        self.calls.push(format!("read {max}"));
        Ok(())
    }
    fn read_file(&mut self, path: &str, len: u64) -> Result<(), Error> {
        self.calls.push(format!("file {path} {len}"));
        Ok(())
    }
    fn data(&mut self, offset: u32, out: &mut [u8]) -> Result<usize, Error> {
        let from = (offset as usize).min(self.came.len());
        let n = (self.came.len() - from).min(out.len()).min(PIECE);
        out[..n].copy_from_slice(&self.came[from..from + n]);
        Ok(n)
    }
    fn peer(&mut self) -> Result<Peer, Error> {
        self.peer.ok_or(Error::NotFound)
    }
    fn close(&mut self) {
        self.calls.push("close".into());
    }
}

fn notice(kind: SocketKind, len: u32) -> Notice {
    Notice {
        kind,
        reason: Reason::Io,
        len,
    }
}

fn double() -> Double {
    Double {
        room: usize::MAX,
        ..Double::default()
    }
}

#[test]
fn what_a_session_asks_reaches_the_socket() {
    let mut state = LinkState::default();
    let mut link = Link::new(&mut state, double());
    link.connect_tls("imap.example.org", 993, Some([7; 32]))
        .unwrap();
    link.connect_plain("smtp.example.org", 587).unwrap();
    link.read_bytes(1024).unwrap();
    link.read_file("incoming.1", 5000).unwrap();
    link.start_tls(None).unwrap();
    link.close();
    assert_eq!(
        link.net().calls,
        [
            "connect imap.example.org:993 tls=true pin=true",
            "connect smtp.example.org:587 tls=false pin=false",
            "read 1024",
            "file incoming.1 5000",
            "start_tls",
            "close",
        ]
    );
}

#[test]
fn a_refused_start_tls_for_held_bytes_reaches_the_session_as_it_is() {
    let mut state = LinkState::default();
    let mut link = Link::new(&mut state, double());
    let net = link.net();
    net.tls = Some(Error::State);
    assert_eq!(link.start_tls(None), Err(Error::State));
}

#[test]
fn what_is_sent_goes_in_pieces_and_waits_for_room() {
    let mut state = LinkState::default();
    let mut link = Link::new(&mut state, double());
    let big = vec![b'x'; PIECE * 2 + 100];
    let net = link.net();
    net.room = PIECE + 10;
    link.send(&big).unwrap();
    // One piece went; the next would not fit: the rest waits.
    assert_eq!(link.net().taken.len(), PIECE);
    assert!(link.waiting());
    // `Sent`: the queue drained, more goes.
    let net = link.net();
    net.room = usize::MAX;
    link.notice(notice(SocketKind::Sent, 0), |_, _| true)
        .unwrap();
    assert_eq!(link.net().taken, big);
    assert!(!link.waiting());
}

#[test]
fn a_send_while_one_waits_follows_it_in_order() {
    let mut state = LinkState::default();
    let mut link = Link::new(&mut state, double());
    let net = link.net();
    net.room = 3;
    link.send(b"abcdef").unwrap();
    assert_eq!(link.net().taken, b"");
    link.send(b"ghi").unwrap();
    let net = link.net();
    net.room = usize::MAX;
    link.notice(notice(SocketKind::Sent, 0), |_, _| true)
        .unwrap();
    assert_eq!(link.net().taken, b"abcdefghi");
}

#[test]
fn a_message_is_sent_whole_in_order() {
    let mut state = LinkState::default();
    let mut link = Link::new(&mut state, double());
    link.send_message(b"Subject: x\r\n\r\nbody\r\n.\r\n".to_vec())
        .unwrap();
    assert_eq!(link.net().taken, b"Subject: x\r\n\r\nbody\r\n.\r\n");
}

#[test]
fn a_socket_that_refuses_ends_the_send_with_its_words() {
    struct Refuses;
    impl Net for Refuses {
        fn connect(&mut self, _: &str, _: u16, _: bool, _: Option<[u8; 32]>) -> Result<(), Error> {
            Ok(())
        }
        fn start_tls(&mut self, _: Option<[u8; 32]>) -> Result<(), Error> {
            Ok(())
        }
        fn send(&mut self, _: &[u8]) -> Result<(), Error> {
            Err(Error::NotFound)
        }
        fn read_bytes(&mut self, _: u32) -> Result<(), Error> {
            Ok(())
        }
        fn read_file(&mut self, _: &str, _: u64) -> Result<(), Error> {
            Ok(())
        }
        fn data(&mut self, _: u32, _: &mut [u8]) -> Result<usize, Error> {
            Ok(0)
        }
        fn peer(&mut self) -> Result<Peer, Error> {
            Err(Error::NotFound)
        }
        fn close(&mut self) {}
    }
    let mut state = LinkState::default();
    let mut link = Link::new(&mut state, Refuses);
    assert_eq!(link.send(b"x"), Err(Error::NotFound));
}

#[test]
fn bytes_that_came_reach_the_session_a_piece_at_a_time() {
    let mut state = LinkState::default();
    let mut link = Link::new(&mut state, double());
    let net = link.net();
    net.came = (0..=255u8).cycle().take(PIECE * 2 + 5).collect();
    let mut got: Vec<Vec<u8>> = Vec::new();
    link.notice(
        notice(SocketKind::Received, (PIECE * 2 + 5) as u32),
        |input, _| {
            let Input::Bytes(bytes) = input else {
                panic!("{input:?}")
            };
            got.push(bytes.to_vec());
            true
        },
    )
    .unwrap();
    assert_eq!(
        got.iter().map(Vec::len).collect::<Vec<_>>(),
        [PIECE, PIECE, 5]
    );
    assert_eq!(
        got.concat(),
        (0..=255u8).cycle().take(PIECE * 2 + 5).collect::<Vec<_>>()
    );
}

#[test]
fn a_session_that_is_finished_stops_the_reading() {
    let mut state = LinkState::default();
    let mut link = Link::new(&mut state, double());
    let net = link.net();
    net.came = vec![1; PIECE * 3];
    let mut pieces = 0;
    link.notice(notice(SocketKind::Received, (PIECE * 3) as u32), |_, _| {
        pieces += 1;
        false
    })
    .unwrap();
    assert_eq!(pieces, 1);
}

#[test]
fn the_other_side_closing_is_an_empty_piece() {
    let mut state = LinkState::default();
    let mut link = Link::new(&mut state, double());
    let mut seen = Vec::new();
    link.notice(notice(SocketKind::Received, 0), |input, _| {
        seen.push(matches!(input, Input::Bytes(b) if b.is_empty()));
        true
    })
    .unwrap();
    assert_eq!(seen, [true]);
}

#[test]
fn the_notices_become_the_sessions_inputs() {
    let mut state = LinkState::default();
    let mut link = Link::new(&mut state, double());
    let net = link.net();
    net.peer = Some(Peer {
        certificate: [1; 32],
        key: [2; 32],
        time_checked: true,
        why: Untrusted::UnknownIssuer,
    });
    let mut seen = Vec::new();
    for kind in [
        SocketKind::Connected,
        SocketKind::Secured,
        SocketKind::Untrusted,
        SocketKind::Closed,
        SocketKind::Failed,
    ] {
        link.notice(notice(kind, 0), |input, _| {
            seen.push(format!("{input:?}"));
            true
        })
        .unwrap();
    }
    assert!(seen[0].starts_with("Connected"));
    assert!(seen[1].starts_with("Secured") && seen[1].contains("time_checked: true"));
    assert!(seen[2].starts_with("Untrusted"));
    assert!(seen[3].starts_with("Closed"));
    assert!(seen[4].starts_with("Failed(Io)"), "{}", seen[4]);
}

#[test]
fn a_saved_file_is_named_to_the_session() {
    let mut state = LinkState::default();
    let mut link = Link::new(&mut state, double());
    link.read_file("incoming.2", 10).unwrap();
    let mut seen = Vec::new();
    link.notice(notice(SocketKind::Saved, 0), |input, _| {
        if let Input::Saved(path) = input {
            seen.push(path.to_string());
        }
        true
    })
    .unwrap();
    assert_eq!(seen, ["incoming.2"]);
}

#[test]
fn a_kind_a_newer_host_added_is_nothing_to_the_session() {
    let mut state = LinkState::default();
    let mut link = Link::new(&mut state, double());
    link.notice(notice(SocketKind::Other(99), 0), |_, _| panic!("no input"))
        .unwrap();
}

#[test]
fn a_secret_that_waits_is_wiped_when_the_link_closes() {
    let mut state = LinkState::default();
    let mut link = Link::new(&mut state, double());
    let net = link.net();
    net.room = 0;
    link.send(b"LOGIN user hunter2").unwrap();
    assert!(link.waiting());
    link.close();
    assert!(!link.waiting());
}
