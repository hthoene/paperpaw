//! STARTTLS on the native sessions: what arrives between the server's OK and
//! the TLS being up is never parsed, and a switch the socket refuses for the
//! bytes of the plain stream still held ends the session as unsafe.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "tests"
)]

use paperpaw_plugin_mail::native_model::mem::{Error, Secret};
use paperpaw_plugin_mail::native_model::session as new;
use paperpaw_sdk::MemoryStorage;

/// A socket that accepts everything and records it.
#[derive(Default)]
struct Wire {
    sent: Vec<String>,
    tls_started: bool,
    closed: bool,
    /// What `start_tls` answers.
    switch: Option<Error>,
}

impl new::Io for Wire {
    fn connect_tls(&mut self, _: &str, _: u16, _: Option<[u8; 32]>) -> Result<(), Error> {
        Ok(())
    }
    fn connect_plain(&mut self, _: &str, _: u16) -> Result<(), Error> {
        Ok(())
    }
    fn start_tls(&mut self, _: Option<[u8; 32]>) -> Result<(), Error> {
        self.tls_started = true;
        self.switch.map_or(Ok(()), Err)
    }
    fn send(&mut self, data: &[u8]) -> Result<(), Error> {
        // A command may come in pieces: one entry per line.
        let piece = String::from_utf8_lossy(data);
        match self.sent.last_mut() {
            Some(last) if !last.ends_with('\n') => last.push_str(&piece),
            _ => self.sent.push(piece.into_owned()),
        }
        Ok(())
    }
    fn send_message(&mut self, _: Vec<u8>) -> Result<(), Error> {
        Ok(())
    }
    fn read_bytes(&mut self, _: u32) -> Result<(), Error> {
        Ok(())
    }
    fn read_file(&mut self, _: &str, _: u64) -> Result<(), Error> {
        Ok(())
    }
    fn close(&mut self) {
        self.closed = true;
    }
}

fn endpoint(port: u16) -> new::Endpoint {
    let mut password = Secret::with_capacity(6).unwrap();
    password.push_str("secret").unwrap();
    new::Endpoint::new(
        "mail.example.org".into(),
        port,
        new::Security::StartTls,
        "ada".into(),
        password,
    )
}

fn told(events: &[new::Event]) -> Vec<String> {
    events.iter().map(|e| format!("{e:?}")).collect()
}

/// An IMAP session that has sent STARTTLS: the socket and the tag it used.
fn imap_at_starttls(switch: Option<Error>) -> (new::Imap, Wire, String) {
    let st = MemoryStorage::new();
    let mut io = Wire {
        switch,
        ..Wire::default()
    };
    let mut session = new::Imap::new(endpoint(143), vec![]);
    session.start(&mut io).unwrap();
    session.step(new::Input::Connected, &mut io, &st).unwrap();
    session
        .step(new::Input::Bytes(b"* OK ready\r\n"), &mut io, &st)
        .unwrap();
    let last = io.sent.last().unwrap().clone();
    assert!(last.contains("STARTTLS"), "{last:?}");
    let tag = last.split(' ').next().unwrap().to_owned();
    (session, io, tag)
}

fn smtp_at_starttls(switch: Option<Error>) -> (new::Smtp, Wire) {
    let mut io = Wire {
        switch,
        ..Wire::default()
    };
    let mut session = new::Smtp::new(
        endpoint(587),
        new::Outgoing::new("a@b.c".into(), vec!["d@e.f".into()], vec![]),
    );
    session.start(&mut io).unwrap();
    session.step(new::Input::Connected, &mut io).unwrap();
    session
        .step(new::Input::Bytes(b"220 hi\r\n"), &mut io)
        .unwrap();
    session
        .step(new::Input::Bytes(b"250-mail\r\n250 STARTTLS\r\n"), &mut io)
        .unwrap();
    assert!(io.sent.last().unwrap().contains("STARTTLS"));
    (session, io)
}

#[test]
fn imap_bytes_behind_the_starttls_ok_end_the_session_as_unsafe() {
    let (mut session, mut io, tag) = imap_at_starttls(None);
    let reply = format!("{tag} OK begin\r\n* CAPABILITY IMAP4rev1 AUTH=PLAIN injected\r\n");
    let events = session
        .step(
            new::Input::Bytes(reply.as_bytes()),
            &mut io,
            &MemoryStorage::new(),
        )
        .unwrap();
    assert_eq!(told(&events).last().unwrap(), "Failed(Unsafe)");
    assert!(io.closed);
    assert!(!io.tls_started, "no switch for a tainted stream");
}

#[test]
fn imap_bytes_while_the_switch_is_under_way_end_the_session_as_unsafe() {
    let (mut session, mut io, tag) = imap_at_starttls(None);
    let st = MemoryStorage::new();
    let ok = format!("{tag} OK begin\r\n");
    session
        .step(new::Input::Bytes(ok.as_bytes()), &mut io, &st)
        .unwrap();
    assert!(io.tls_started);
    assert!(!io.closed);
    let sent = io.sent.len();
    let events = session
        .step(new::Input::Bytes(b"* OK injected\r\n"), &mut io, &st)
        .unwrap();
    assert_eq!(told(&events).last().unwrap(), "Failed(Unsafe)");
    assert!(io.closed);
    assert_eq!(io.sent.len(), sent, "nothing is answered");
}

#[test]
fn imap_a_switch_refused_for_held_bytes_is_unsafe_not_a_network_failure() {
    let (mut session, mut io, tag) = imap_at_starttls(Some(Error::State));
    let ok = format!("{tag} OK begin\r\n");
    let events = session
        .step(
            new::Input::Bytes(ok.as_bytes()),
            &mut io,
            &MemoryStorage::new(),
        )
        .unwrap();
    assert_eq!(told(&events).last().unwrap(), "Failed(Unsafe)");
    assert!(io.closed);
}

#[test]
fn smtp_bytes_behind_the_starttls_220_end_the_session_as_unsafe() {
    let (mut session, mut io) = smtp_at_starttls(None);
    let events = session
        .step(
            new::Input::Bytes(b"220 begin\r\n250-injected\r\n250 AUTH PLAIN\r\n"),
            &mut io,
        )
        .unwrap();
    assert_eq!(told(&events).last().unwrap(), "Failed(Unsafe)");
    assert!(io.closed);
    assert!(!io.tls_started);
}

#[test]
fn smtp_bytes_while_the_switch_is_under_way_end_the_session_as_unsafe() {
    let (mut session, mut io) = smtp_at_starttls(None);
    session
        .step(new::Input::Bytes(b"220 begin\r\n"), &mut io)
        .unwrap();
    assert!(io.tls_started);
    assert!(!io.closed);
    let sent = io.sent.len();
    let events = session
        .step(new::Input::Bytes(b"250 injected\r\n"), &mut io)
        .unwrap();
    assert_eq!(told(&events).last().unwrap(), "Failed(Unsafe)");
    assert!(io.closed);
    assert_eq!(io.sent.len(), sent, "nothing is answered");
}

#[test]
fn smtp_a_switch_refused_for_held_bytes_is_unsafe_not_a_network_failure() {
    let (mut session, mut io) = smtp_at_starttls(Some(Error::State));
    let events = session
        .step(new::Input::Bytes(b"220 begin\r\n"), &mut io)
        .unwrap();
    assert_eq!(told(&events).last().unwrap(), "Failed(Unsafe)");
    assert!(io.closed);
}
