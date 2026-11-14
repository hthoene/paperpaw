//! The job runs whole IMAP and SMTP sessions over the link on the fake server:
//! the socket's notices, the bytes in pieces of any size, what is sent through
//! the queue, the files saved, and every way a connection can end.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests unwrap what they just made"
)]

mod fake;

use fake::{Fake, Input, Proto, Script};
use paperpaw_plugin_mail::native_model::job::{Job, Run};
use paperpaw_plugin_mail::native_model::link::{Net, Notice, Peer};
use paperpaw_plugin_mail::native_model::mem::{Error, Secret};
use paperpaw_plugin_mail::native_model::session::{Endpoint, Failure, Outgoing, Security, Task};
use paperpaw_plugin_mail::native_model::store;
use paperpaw_sdk::native::sockets::{Reason, SocketKind, Untrusted};
use paperpaw_sdk::{KeyHash, Tls};

/// What the double socket keeps between callbacks.
#[derive(Default)]
struct Wire {
    came: Vec<u8>,
    peer: Option<Peer>,
    /// The host refuses to give what came.
    refuse_data: bool,
}

/// The fake server as the link's socket.
struct FakeNet<'a> {
    fake: &'a mut Fake,
    wire: &'a mut Wire,
}

fn tls_of(pin: Option<[u8; 32]>) -> Tls {
    match pin {
        Some(key) => Tls::pinned_keys(vec![KeyHash::new(key)]),
        None => Tls::new(),
    }
}

impl Net for FakeNet<'_> {
    fn connect(
        &mut self,
        host: &str,
        port: u16,
        tls: bool,
        pin: Option<[u8; 32]>,
    ) -> Result<(), Error> {
        if tls {
            self.fake.connect_tls(host, port, tls_of(pin));
        } else {
            self.fake.connect_plain(host, port);
        }
        Ok(())
    }
    fn start_tls(&mut self, pin: Option<[u8; 32]>) -> Result<(), Error> {
        self.fake.start_tls(tls_of(pin));
        Ok(())
    }
    fn send(&mut self, piece: &[u8]) -> Result<(), Error> {
        self.fake.send(piece.to_vec());
        Ok(())
    }
    fn read_bytes(&mut self, max: u32) -> Result<(), Error> {
        self.fake.read_bytes(max);
        Ok(())
    }
    fn read_file(&mut self, path: &str, len: u64) -> Result<(), Error> {
        self.fake.read_file(path, len);
        Ok(())
    }
    fn data(&mut self, offset: u32, out: &mut [u8]) -> Result<usize, Error> {
        if self.wire.refuse_data {
            return Err(Error::State);
        }
        let from = (offset as usize).min(self.wire.came.len());
        let n = (self.wire.came.len() - from).min(out.len());
        out[..n].copy_from_slice(&self.wire.came[from..from + n]);
        Ok(n)
    }
    fn peer(&mut self) -> Result<Peer, Error> {
        self.wire.peer.ok_or(Error::NotFound)
    }
    fn close(&mut self) {
        self.fake.close();
    }
}

/// The fake's notice as the host would tell it.
fn notice_of(input: &Input, wire: &mut Wire) -> Notice {
    let (kind, len) = match input {
        Input::Connected => (SocketKind::Connected, 0),
        Input::Secured {
            certificate,
            key,
            time_checked,
        } => {
            wire.peer = Some(Peer {
                certificate: *certificate.sha256(),
                key: *key.sha256(),
                time_checked: *time_checked,
                why: Untrusted::UnknownIssuer,
            });
            (SocketKind::Secured, 0)
        }
        Input::Untrusted {
            certificate, key, ..
        } => {
            wire.peer = Some(Peer {
                certificate: *certificate.sha256(),
                key: *key.sha256(),
                time_checked: true,
                why: Untrusted::UnknownIssuer,
            });
            (SocketKind::Untrusted, 0)
        }
        Input::Bytes(bytes) => {
            wire.came = bytes.clone();
            (SocketKind::Received, bytes.len() as u32)
        }
        Input::Saved(_) => (SocketKind::Saved, 0),
        Input::Closed => (SocketKind::Closed, 0),
        Input::Failed(..) => (SocketKind::Failed, 0),
    };
    Notice {
        kind,
        reason: Reason::Io,
        len,
    }
}

fn endpoint(security: Security, port: u16, password: &str) -> Endpoint {
    let mut secret = Secret::with_capacity(password.len()).unwrap();
    secret.push_str(password).unwrap();
    Endpoint::new(
        "mail.example.org".into(),
        port,
        security,
        "ada".into(),
        secret,
    )
}

fn message(n: u32) -> (u32, String, String) {
    (
        n * 10,
        String::new(),
        format!(
            "From: ada@example.org\r\nTo: bob@example.org\r\nSubject: Subject {n}\r\nDate: Tue, 3 Nov 2026 09:15:00 +0100\r\nMessage-ID: <{n}@example.org>\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nHello number {n}.\r\n"
        ),
    )
}

/// Drives the job on the fake until it is not working or the fake is silent.
fn drive(job: &mut Job, fake: &mut Fake) {
    let mut wire = Wire::default();
    for _ in 0..200_000 {
        if !job.working() {
            return;
        }
        let Some(input) = fake.next_input() else {
            return;
        };
        if matches!(input, Input::Secured { .. }) {
            fake.switched();
        }
        let notice = notice_of(&input, &mut wire);
        let storage = fake.storage.clone();
        job.notice(
            FakeNet {
                fake,
                wire: &mut wire,
            },
            notice,
            &*storage,
        )
        .unwrap();
    }
    panic!("the job never ended");
}

fn imap_tasks() -> Vec<Task> {
    vec![
        Task::Folders {
            out: store::folders_file(1).unwrap(),
        },
        Task::Sync {
            folder: "INBOX".into(),
            count: 50,
            index: store::index_file(1, "INBOX").unwrap(),
            status: store::status_file(1, "INBOX").unwrap(),
            now: 1_793_693_700,
        },
        Task::Body {
            folder: "INBOX".into(),
            uid: 20,
            out: store::raw_file(1, "INBOX", 20).unwrap(),
        },
    ]
}

fn start_imap(job: &mut Job, fake: &mut Fake, security: Security, port: u16, password: &str) {
    let mut wire = Wire::default();
    job.start_imap(
        FakeNet {
            fake,
            wire: &mut wire,
        },
        endpoint(security, port, password),
        imap_tasks(),
    )
    .unwrap();
}

#[test]
fn a_whole_imap_session_runs_in_pieces_of_every_size() {
    for chunk in [1, 7, 100, 1000, 100_000] {
        for (security, port) in [(Security::Implicit, 993), (Security::StartTls, 143)] {
            let script = Script {
                chunk,
                messages: vec![message(1), message(2)],
                ..Script::default()
            };
            let mut fake = Fake::new(Proto::Imap, script);
            let mut job = Job::new();
            start_imap(&mut job, &mut fake, security, port, "secret");
            assert!(job.working());
            drive(&mut job, &mut fake);
            assert_eq!(job.run, Run::Done, "chunk {chunk} {security:?}");
            assert!(!job.working());
            let storage = fake.storage.clone();
            assert!(
                paperpaw_sdk::Storage::size(&*storage, &store::raw_file(1, "INBOX", 20).unwrap())
                    .is_ok()
            );
            assert!(
                fake.sent
                    .iter()
                    .any(|l| l.contains("LOGIN") || l.contains("AUTHENTICATE"))
            );
        }
    }
}

#[test]
fn a_wrong_password_ends_the_job_as_a_failed_login() {
    let mut fake = Fake::new(Proto::Imap, Script::default());
    let mut job = Job::new();
    start_imap(&mut job, &mut fake, Security::Implicit, 993, "wrong");
    drive(&mut job, &mut fake);
    assert!(
        matches!(job.run, Run::Failed(Failure::Login(_))),
        "{:?}",
        job.run
    );
}

#[test]
fn a_server_that_is_not_trusted_is_told_with_its_hashes() {
    let script = Script {
        untrusted: true,
        ..Script::default()
    };
    let mut fake = Fake::new(Proto::Imap, script);
    let mut job = Job::new();
    start_imap(&mut job, &mut fake, Security::Implicit, 993, "secret");
    drive(&mut job, &mut fake);
    assert!(
        matches!(job.run, Run::Untrusted { smtp: false, .. }),
        "{:?}",
        job.run
    );
}

#[test]
fn a_device_without_a_clock_does_not_send_the_password() {
    let script = Script {
        time_checked: false,
        ..Script::default()
    };
    let mut fake = Fake::new(Proto::Imap, script);
    let mut job = Job::new();
    start_imap(&mut job, &mut fake, Security::Implicit, 993, "secret");
    drive(&mut job, &mut fake);
    assert_eq!(job.run, Run::Failed(Failure::ClockNotSet));
    assert!(!fake.sent_plain_password);
    assert!(!fake.sent.iter().any(|l| l.contains("secret")));
}

#[test]
fn a_message_is_sent_through_the_smtp_server() {
    for (security, port) in [(Security::Implicit, 465), (Security::StartTls, 587)] {
        let mut fake = Fake::new(Proto::Smtp, Script::default());
        let mut job = Job::new();
        let data = paperpaw_plugin_mail::native_model::smtp::data("Subject: hi\r\n\r\nHello.\r\n")
            .unwrap();
        let mut wire = Wire::default();
        job.start_smtp(
            FakeNet {
                fake: &mut fake,
                wire: &mut wire,
            },
            endpoint(security, port, "secret"),
            Outgoing::new(
                "ada@example.org".into(),
                vec!["bob@example.org".into()],
                data,
            ),
        )
        .unwrap();
        drive(&mut job, &mut fake);
        assert_eq!(job.run, Run::Done, "{security:?} {:?}", fake.sent);
        let (from, to, body) = fake.smtp_got.clone().unwrap();
        assert_eq!(from, "ada@example.org");
        assert_eq!(to, ["bob@example.org"]);
        assert!(body.contains("Hello."), "{body}");
    }
}

#[test]
fn a_recipient_the_server_refuses_ends_the_send_with_its_words() {
    let script = Script {
        refuse_rcpt: Some("bob@example.org".into()),
        ..Script::default()
    };
    let mut fake = Fake::new(Proto::Smtp, script);
    let mut job = Job::new();
    let mut wire = Wire::default();
    job.start_smtp(
        FakeNet {
            fake: &mut fake,
            wire: &mut wire,
        },
        endpoint(Security::Implicit, 465, "secret"),
        Outgoing::new(
            "ada@example.org".into(),
            vec!["bob@example.org".into()],
            b"x\r\n".to_vec(),
        ),
    )
    .unwrap();
    drive(&mut job, &mut fake);
    assert!(
        matches!(job.run, Run::Failed(Failure::Recipient(..))),
        "{:?}",
        job.run
    );
}

#[test]
fn nothing_starts_while_a_connection_is_open() {
    let mut fake = Fake::new(Proto::Imap, Script::default());
    let mut job = Job::new();
    start_imap(&mut job, &mut fake, Security::Implicit, 993, "secret");
    let mut wire = Wire::default();
    let again = job.start_imap(
        FakeNet {
            fake: &mut fake,
            wire: &mut wire,
        },
        endpoint(Security::Implicit, 993, "secret"),
        imap_tasks(),
    );
    assert_eq!(again, Err(Error::Busy));
}

#[test]
fn a_notice_with_no_session_is_not_the_jobs() {
    let mut fake = Fake::new(Proto::Imap, Script::default());
    let mut job = Job::new();
    let mut wire = Wire::default();
    let notice = Notice {
        kind: SocketKind::Closed,
        reason: Reason::Io,
        len: 0,
    };
    let storage = fake.storage.clone();
    job.notice(
        FakeNet {
            fake: &mut fake,
            wire: &mut wire,
        },
        notice,
        &*storage,
    )
    .unwrap();
    assert_eq!(job.run, Run::Idle);
}

#[test]
fn a_socket_that_fails_ends_the_job_as_a_network_failure() {
    let mut fake = Fake::new(Proto::Imap, Script::default());
    let mut job = Job::new();
    start_imap(&mut job, &mut fake, Security::Implicit, 993, "secret");
    let mut wire = Wire::default();
    let notice = Notice {
        kind: SocketKind::Failed,
        reason: Reason::Timeout,
        len: 0,
    };
    let storage = fake.storage.clone();
    job.notice(
        FakeNet {
            fake: &mut fake,
            wire: &mut wire,
        },
        notice,
        &*storage,
    )
    .unwrap();
    assert!(
        matches!(job.run, Run::Failed(Failure::Network(Reason::Timeout))),
        "{:?}",
        job.run
    );
}

#[test]
fn a_host_error_ends_the_job_and_closes_the_socket() {
    let mut fake = Fake::new(Proto::Imap, Script::default());
    let mut job = Job::new();
    start_imap(&mut job, &mut fake, Security::Implicit, 993, "secret");
    let mut wire = Wire {
        refuse_data: true,
        ..Wire::default()
    };
    let notice = Notice {
        kind: SocketKind::Received,
        reason: Reason::Io,
        len: 12,
    };
    let storage = fake.storage.clone();
    let told = job.notice(
        FakeNet {
            fake: &mut fake,
            wire: &mut wire,
        },
        notice,
        &*storage,
    );
    assert_eq!(told, Err(Error::State));
    assert!(matches!(job.run, Run::Failed(_)), "{:?}", job.run);
    assert!(
        fake.closed,
        "the socket is closed with the job, not left to idle"
    );
}

#[test]
fn a_cancelled_connection_closes_its_socket_and_is_idle() {
    let mut fake = Fake::new(Proto::Imap, Script::default());
    let mut job = Job::new();
    start_imap(&mut job, &mut fake, Security::Implicit, 993, "secret");
    assert!(job.working());
    let mut wire = Wire::default();
    job.cancel(FakeNet {
        fake: &mut fake,
        wire: &mut wire,
    });
    assert!(fake.closed);
    assert_eq!(job.run, Run::Idle);
    assert!(!job.working());
    // A new connection can start.
    start_imap(&mut job, &mut fake, Security::Implicit, 993, "secret");
    assert!(job.working());
}
