//! The native mail sessions give the dialogue interface 6 gave on the same fake
//! servers (kept in `testdata/pins/`): the same lines sent, the same events,
//! the same files on the card, in pieces of any size, over TLS from the start
//! and with STARTTLS, and where they differ (a refused allocation, a socket
//! that says no) they say so.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests unwrap what they just made"
)]

mod fake;
mod pin;

use fake::{Fake, Input, Proto, Script};
use paperpaw_plugin_mail::native_model::fail::Fail;
use paperpaw_plugin_mail::native_model::mem::{Error, Secret};
use paperpaw_plugin_mail::native_model::session as new;
use paperpaw_plugin_mail::native_model::store as new_store;
use paperpaw_sdk::native::sockets::{Reason, Untrusted};
use paperpaw_sdk::{CertificateHash, Distrust, KeyHash, MemoryStorage, SocketError, Tls};

fn message(n: u32, flags: &str) -> (u32, String, String) {
    let body = format!("Hello number {n}.\r\n.A line with a dot.\r\nÜbung = fertig\r\n");
    (
        n * 10,
        flags.into(),
        format!(
            "From: =?utf-8?Q?J=C3=BCrgen?= <j{n}@example.org>\r\nTo: ada@example.org\r\nSubject: Subject {n}\r\nDate: Tue, 3 Nov 2026 09:15:00 +0100\r\nMessage-ID: <{n}@example.org>\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n{body}"
        ),
    )
}

// ---- the native session over the fake socket ----

/// The fake socket as the native `Io`.
struct NativeIo<'a>(&'a mut Fake);

fn tls_of(pin: Option<[u8; 32]>) -> Tls {
    match pin {
        Some(key) => Tls::pinned_keys(vec![KeyHash::new(key)]),
        None => Tls::new(),
    }
}

impl new::Io for NativeIo<'_> {
    fn connect_tls(&mut self, host: &str, port: u16, pin: Option<[u8; 32]>) -> Result<(), Error> {
        self.0.connect_tls(host, port, tls_of(pin));
        Ok(())
    }
    fn connect_plain(&mut self, host: &str, port: u16) -> Result<(), Error> {
        self.0.connect_plain(host, port);
        Ok(())
    }
    fn start_tls(&mut self, pin: Option<[u8; 32]>) -> Result<(), Error> {
        self.0.start_tls(tls_of(pin));
        Ok(())
    }
    fn send(&mut self, data: &[u8]) -> Result<(), Error> {
        self.0.send(data.to_vec());
        Ok(())
    }
    fn send_message(&mut self, data: Vec<u8>) -> Result<(), Error> {
        self.0.send(data);
        Ok(())
    }
    fn read_bytes(&mut self, max: u32) -> Result<(), Error> {
        self.0.read_bytes(max);
        Ok(())
    }
    fn read_file(&mut self, path: &str, len: u64) -> Result<(), Error> {
        self.0.read_file(path, len);
        Ok(())
    }
    fn close(&mut self) {
        self.0.close();
    }
}

/// The notice the fake made, as the native session takes it.
fn to_new(input: &Input) -> new::Input<'_> {
    match input {
        Input::Connected => new::Input::Connected,
        Input::Secured {
            certificate,
            key,
            time_checked,
        } => new::Input::Secured {
            certificate: *certificate.sha256(),
            key: *key.sha256(),
            time_checked: *time_checked,
        },
        Input::Untrusted {
            certificate,
            key,
            why,
        } => new::Input::Untrusted {
            certificate: *certificate.sha256(),
            key: *key.sha256(),
            why: match why {
                Distrust::NotPinned => Untrusted::NotPinned,
                _ => Untrusted::UnknownIssuer,
            },
        },
        Input::Bytes(bytes) => new::Input::Bytes(bytes),
        Input::Saved(path) => new::Input::Saved(path),
        Input::Closed => new::Input::Closed,
        Input::Failed(..) => new::Input::Failed(Reason::Io),
    }
}

/// An event as text: a certificate is its hashes.
fn new_told(event: &new::Event) -> String {
    match event {
        new::Event::Untrusted { why, .. } => format!("Untrusted({why:?})"),
        other => format!("{other:?}"),
    }
}

fn new_endpoint(security: new::Security, port: u16) -> new::Endpoint {
    let mut password = Secret::with_capacity(6).unwrap();
    password.push_str("secret").unwrap();
    new::Endpoint::new(
        "mail.example.org".into(),
        port,
        security,
        "ada".into(),
        password,
    )
}

/// What a run left: the events as text, the lines the client sent, and whether
/// it closed.
struct Run {
    events: Vec<String>,
    sent: Vec<String>,
    closed: bool,
    switches: u32,
    fake: Fake,
}

fn finished(events: &[String]) -> bool {
    events
        .iter()
        .any(|e| e == "Done" || e.starts_with("Failed") || e.starts_with("Untrusted"))
}

/// The tasks of a read, with the paths the native store keeps.
fn tasks_new(account: u8, messages: usize, all: bool) -> Vec<new::Task> {
    let mut tasks = vec![
        new::Task::Folders {
            out: new_store::folders_file(account).unwrap(),
        },
        new::Task::Sync {
            folder: "INBOX".into(),
            count: 50,
            index: new_store::index_file(account, "INBOX").unwrap(),
            status: new_store::status_file(account, "INBOX").unwrap(),
            now: 1_793_693_700,
        },
    ];
    if all {
        tasks.push(new::Task::Body {
            folder: "INBOX".into(),
            uid: (messages as u32) * 10,
            out: new_store::raw_file(account, "INBOX", (messages as u32) * 10).unwrap(),
        });
        tasks.push(new::Task::Flag {
            folder: "INBOX".into(),
            uid: 10,
            flag: "\\Seen",
            on: true,
        });
    }
    tasks
}

fn run_imap(script: Script, security: new::Security, tasks: &[new::Task]) -> Run {
    let port = if security == new::Security::Implicit {
        993
    } else {
        143
    };
    let mut fake = Fake::new(Proto::Imap, script);
    let mut events: Vec<String> = Vec::new();
    let mut session = new::Imap::new(new_endpoint(security, port), tasks.to_vec());
    events.extend(
        session
            .start(&mut NativeIo(&mut fake))
            .unwrap()
            .iter()
            .map(new_told),
    );
    for _ in 0..200_000 {
        if finished(&events) {
            break;
        }
        let Some(input) = fake.next_input() else {
            break;
        };
        if matches!(input, Input::Secured { .. }) {
            fake.switched();
        }
        let storage = fake.storage.clone();
        let told = session
            .step(to_new(&input), &mut NativeIo(&mut fake), &*storage)
            .unwrap();
        events.extend(told.iter().map(new_told));
    }
    Run {
        events,
        sent: fake.sent.clone(),
        closed: fake.closed,
        switches: fake.tls_switches,
        fake,
    }
}

/// The files a session leaves, by name.
fn files(run: &Run, account: u8) -> Vec<(String, Option<Vec<u8>>)> {
    let st = run.fake.storage.clone();
    [
        ("folders", new_store::folders_file(account).unwrap()),
        ("index", new_store::index_file(account, "INBOX").unwrap()),
        ("status", new_store::status_file(account, "INBOX").unwrap()),
        ("raw", new_store::raw_file(account, "INBOX", 30).unwrap()),
    ]
    .into_iter()
    .map(|(name, path)| (name.to_string(), st.get(&path)))
    .collect()
}

/// Runs a read and gives what it did to `pin`.
fn pinned_imap(
    pin: &mut pin::Pin,
    script: Script,
    security: new::Security,
    messages: usize,
    all: bool,
    label: &str,
) -> Run {
    let run = run_imap(script, security, &tasks_new(1, messages, all));
    pin.check(format!("{label}: events"), &run.events);
    pin.check(format!("{label}: lines sent"), &run.sent);
    pin.check(format!("{label}: closed"), run.closed);
    pin.check(format!("{label}: TLS switches"), run.switches);
    pin.check(format!("{label}: files"), files(&run, 1));
    run
}

#[test]
fn the_whole_read_is_what_interface_6_did_in_pieces_of_any_size() {
    let mut pin = pin::Pin::new("the_whole_read_is_what_interface_6_did_in_pieces_of_any_size");
    for chunk in [1usize, 3, 17, 100, 1000, 100_000] {
        for security in [new::Security::Implicit, new::Security::StartTls] {
            let mut script = Script {
                chunk,
                ..Script::default()
            };
            script.messages = vec![
                message(1, "\\Seen"),
                message(2, ""),
                message(3, "\\Flagged"),
            ];
            let new = pinned_imap(
                &mut pin,
                script,
                security,
                3,
                true,
                &format!("chunk {chunk} {security:?}"),
            );
            assert_eq!(new.events.last().map(String::as_str), Some("Done"));
            assert!(!new.fake.sent_plain_password);
            // The files really were written (a native read gives them back).
            let st = new.fake.storage.clone();
            let rows =
                new_store::read_index(&*st, &new_store::index_file(1, "INBOX").unwrap()).unwrap();
            assert_eq!(rows.iter().map(|r| r.uid).collect::<Vec<_>>(), [30, 20, 10]);
        }
    }
    pin.finish();
}

#[test]
fn the_logins_the_failures_and_the_empty_folder_are_what_interface_6_did() {
    let mut pin =
        pin::Pin::new("the_logins_the_failures_and_the_empty_folder_are_what_interface_6_did");
    let sasl = Script {
        capability: "IMAP4rev1 AUTH=PLAIN SASL-IR".into(),
        ..Script::default()
    };
    let login = Script {
        capability: "IMAP4rev1".into(),
        ..Script::default()
    };
    let disabled = Script {
        capability: "IMAP4rev1 LOGINDISABLED".into(),
        ..Script::default()
    };
    let wrong = Script {
        password: "other".into(),
        ..Script::default()
    };
    let untrusted = Script {
        untrusted: true,
        ..Script::default()
    };
    let no_clock = Script {
        time_checked: false,
        ..Script::default()
    };
    for (label, script) in [
        ("sasl", sasl),
        ("login", login),
        ("disabled", disabled),
        ("wrong password", wrong),
        ("untrusted", untrusted),
        ("no clock", no_clock),
        ("empty", Script::default()),
    ] {
        let run = pinned_imap(&mut pin, script, new::Security::Implicit, 0, false, label);
        assert!(finished(&run.events), "{label}: {:?}", run.events);
    }
    pin.finish();
}

#[test]
fn a_large_message_is_cut_at_what_was_asked_as_interface_6_cut_it() {
    let mut pin = pin::Pin::new("a_large_message_is_cut_at_what_was_asked_as_interface_6_cut_it");
    let big = format!(
        "From: a@b.c\r\nSubject: big\r\n\r\n{}",
        "line of text for a big message\r\n".repeat(9000)
    );
    let script = Script {
        messages: vec![(10, "".into(), big)],
        ..Script::default()
    };
    let tasks = vec![new::Task::Body {
        folder: "INBOX".into(),
        uid: 10,
        out: new_store::raw_file(1, "INBOX", 10).unwrap(),
    }];
    let new = run_imap(script, new::Security::Implicit, &tasks);
    pin.check("events", &new.events);
    assert_eq!(new.events.last().map(String::as_str), Some("Done"));
    let b = new
        .fake
        .storage
        .get(&new_store::raw_file(1, "INBOX", 10).unwrap())
        .unwrap();
    assert_eq!(b.len(), new_store::MAX_FETCH);
    pin.check("the message", &b);
    assert!(new.fake.reads < 100);
    // The scratch file of the rest is gone.
    let st = new.fake.storage.clone();
    assert!(
        st.get(&format!(
            "{}.1",
            new_store::raw_file(1, "INBOX", 10).unwrap()
        ))
        .is_none()
    );
    pin.finish();
}

#[test]
fn a_hung_up_connection_and_the_stages_are_what_interface_6_did() {
    let mut pin = pin::Pin::new("a_hung_up_connection_and_the_stages_are_what_interface_6_did");
    let script = Script {
        messages: vec![message(1, "")],
        ..Script::default()
    };
    let run = pinned_imap(
        &mut pin,
        script.clone(),
        new::Security::Implicit,
        1,
        true,
        "stages",
    );
    let stages: Vec<&str> = run
        .events
        .iter()
        .filter(|e| e.starts_with("Stage"))
        .map(String::as_str)
        .collect();
    assert_eq!(stages.len(), 6);
    // The server hangs up after five inputs.
    let mut fake = Fake::new(Proto::Imap, script);
    let st = MemoryStorage::new();
    let mut events: Vec<String> = Vec::new();
    let mut inputs = 0;
    let mut session = new::Imap::new(
        new_endpoint(new::Security::Implicit, 993),
        tasks_new(1, 1, true),
    );
    events.extend(
        session
            .start(&mut NativeIo(&mut fake))
            .unwrap()
            .iter()
            .map(new_told),
    );
    while let Some(input) = fake.next_input() {
        inputs += 1;
        let input = if inputs == 5 { Input::Closed } else { input };
        let told = session
            .step(to_new(&input), &mut NativeIo(&mut fake), &st)
            .unwrap();
        events.extend(told.iter().map(new_told));
        if inputs == 5 {
            break;
        }
    }
    pin.check("hung up", &events);
    assert_eq!(
        events.last().map(String::as_str),
        Some("Failed(Network(Lost))")
    );
    pin.finish();
}

#[test]
fn a_pinned_key_is_what_the_native_session_asks_for() {
    let mut endpoint = new_endpoint(new::Security::Implicit, 993);
    endpoint.pin = Some([9; 32]);
    let mut fake = Fake::new(Proto::Imap, Script::default());
    let mut session = new::Imap::new(endpoint, vec![]);
    session.start(&mut NativeIo(&mut fake)).unwrap();
    assert_eq!(fake.tls_used.len(), 1);
    assert!(format!("{:?}", fake.tls_used[0]).contains("PinnedKeys"));
}

// ---- SMTP ----

fn outgoing_parts() -> (String, Vec<String>, Vec<u8>) {
    let mut draft =
        paperpaw_plugin_mail::native_model::compose::Draft::with_sender("", "ada@example.org")
            .unwrap();
    draft.to = "bob@example.org, carol@example.org".into();
    draft.subject = "Hi".into();
    draft.body = "Line\n.dot first\n".into();
    let to = draft.check().unwrap();
    let data = paperpaw_plugin_mail::native_model::smtp::data(
        &draft
            .message("Tue, 3 Nov 2026 09:15:00 +0000", "1")
            .unwrap(),
    )
    .unwrap();
    (draft.from.clone(), to, data)
}

fn run_smtp(script: Script, security: new::Security) -> Run {
    let port = if security == new::Security::Implicit {
        465
    } else {
        587
    };
    let (from, to, data) = outgoing_parts();
    let mut fake = Fake::new(Proto::Smtp, script);
    let mut events: Vec<String> = Vec::new();
    let mut session = new::Smtp::new(
        new_endpoint(security, port),
        new::Outgoing::new(from, to, data),
    );
    events.extend(
        session
            .start(&mut NativeIo(&mut fake))
            .unwrap()
            .iter()
            .map(new_told),
    );
    for _ in 0..100_000 {
        if finished(&events) {
            break;
        }
        let Some(input) = fake.next_input() else {
            break;
        };
        if matches!(input, Input::Secured { .. }) {
            fake.switched();
        }
        let told = session
            .step(to_new(&input), &mut NativeIo(&mut fake))
            .unwrap();
        events.extend(told.iter().map(new_told));
    }
    Run {
        events,
        sent: fake.sent.clone(),
        closed: fake.closed,
        switches: fake.tls_switches,
        fake,
    }
}

/// Runs a send and gives what it did to `pin`.
fn pinned_smtp(pin: &mut pin::Pin, script: Script, security: new::Security, label: &str) -> Run {
    let run = run_smtp(script, security);
    pin.check(format!("{label}: events"), &run.events);
    pin.check(format!("{label}: lines sent"), &run.sent);
    pin.check(format!("{label}: closed"), run.closed);
    pin.check(format!("{label}: TLS switches"), run.switches);
    pin.check(format!("{label}: the message"), &run.fake.smtp_got);
    run
}

#[test]
fn a_message_is_sent_as_interface_6_sent_it_in_pieces_of_any_size() {
    let mut pin = pin::Pin::new("a_message_is_sent_as_interface_6_sent_it_in_pieces_of_any_size");
    for chunk in [1usize, 5, 1000] {
        for security in [new::Security::Implicit, new::Security::StartTls] {
            let script = Script {
                chunk,
                ..Script::default()
            };
            let new = pinned_smtp(
                &mut pin,
                script,
                security,
                &format!("chunk {chunk} {security:?}"),
            );
            assert_eq!(new.events.last().map(String::as_str), Some("Done"));
            assert!(new.fake.smtp_got.is_some());
            assert!(!new.fake.sent_plain_password);
        }
    }
    pin.finish();
}

#[test]
fn the_smtp_failures_are_what_interface_6_did() {
    let mut pin = pin::Pin::new("the_smtp_failures_are_what_interface_6_did");
    let no_starttls = Script {
        smtp_extensions: vec!["AUTH PLAIN".into()],
        ..Script::default()
    };
    let login_only = Script {
        smtp_extensions: vec!["AUTH LOGIN".into()],
        ..Script::default()
    };
    let wrong = Script {
        password: "other".into(),
        smtp_extensions: vec!["AUTH LOGIN".into()],
        ..Script::default()
    };
    let refused = Script {
        refuse_rcpt: Some("carol@example.org".into()),
        ..Script::default()
    };
    let small = Script {
        smtp_extensions: vec!["STARTTLS".into(), "AUTH PLAIN".into(), "SIZE 10".into()],
        ..Script::default()
    };
    let untrusted = Script {
        untrusted: true,
        ..Script::default()
    };
    let no_clock = Script {
        time_checked: false,
        ..Script::default()
    };
    for (label, script, security) in [
        ("no starttls", no_starttls, new::Security::StartTls),
        ("login only", login_only.clone(), new::Security::Implicit),
        ("login only starttls", login_only, new::Security::StartTls),
        ("wrong login password", wrong, new::Security::Implicit),
        ("refused recipient", refused, new::Security::Implicit),
        ("too large", small, new::Security::Implicit),
        ("untrusted", untrusted, new::Security::Implicit),
        ("no clock", no_clock, new::Security::Implicit),
    ] {
        let run = pinned_smtp(&mut pin, script, security, label);
        assert!(finished(&run.events), "{label}: {:?}", run.events);
    }
    pin.finish();
}

// ---- where the native session differs ----

/// A socket that refuses what it is asked, or hangs up.
struct Refusing {
    error: Error,
    closed: bool,
    sent: Vec<Vec<u8>>,
}

impl new::Io for Refusing {
    fn connect_tls(&mut self, _: &str, _: u16, _: Option<[u8; 32]>) -> Result<(), Error> {
        Err(self.error)
    }
    fn connect_plain(&mut self, _: &str, _: u16) -> Result<(), Error> {
        Err(self.error)
    }
    fn start_tls(&mut self, _: Option<[u8; 32]>) -> Result<(), Error> {
        Err(self.error)
    }
    fn send(&mut self, data: &[u8]) -> Result<(), Error> {
        self.sent.push(data.to_vec());
        Err(self.error)
    }
    fn send_message(&mut self, _: Vec<u8>) -> Result<(), Error> {
        Err(self.error)
    }
    fn read_bytes(&mut self, _: u32) -> Result<(), Error> {
        Err(self.error)
    }
    fn read_file(&mut self, _: &str, _: u64) -> Result<(), Error> {
        Err(self.error)
    }
    fn close(&mut self) {
        self.closed = true;
    }
}

#[test]
fn a_socket_that_says_no_ends_the_session_as_what_it_says() {
    for (error, expected) in [
        (Error::Unsupported, "Failed(NoTls)"),
        (Error::Busy, "Failed(Busy)"),
        (Error::NotFound, "Failed(Network(Io))"),
    ] {
        let mut io = Refusing {
            error,
            closed: false,
            sent: Vec::new(),
        };
        let mut session = new::Imap::new(new_endpoint(new::Security::Implicit, 993), vec![]);
        let events = session.start(&mut io).unwrap();
        let told: Vec<String> = events.iter().map(new_told).collect();
        assert_eq!(told, ["Stage(Connecting)", expected], "{error:?}");
        assert!(io.closed);
        // And it does nothing more.
        let more = session
            .step(new::Input::Connected, &mut io, &MemoryStorage::new())
            .unwrap();
        assert!(more.is_empty());
        let mut smtp = new::Smtp::new(
            new_endpoint(new::Security::Implicit, 465),
            new::Outgoing::new("a@b.c".into(), vec!["d@e.f".into()], vec![]),
        );
        let told: Vec<String> = smtp.start(&mut io).unwrap().iter().map(new_told).collect();
        assert_eq!(told, ["Stage(Connecting)", expected]);
    }
}

#[test]
fn no_memory_from_the_socket_is_told_as_no_memory() {
    let mut io = Refusing {
        error: Error::NoMemory,
        closed: false,
        sent: Vec::new(),
    };
    let mut session = new::Imap::new(new_endpoint(new::Security::Implicit, 993), vec![]);
    let told: Vec<String> = session
        .start(&mut io)
        .unwrap()
        .iter()
        .map(new_told)
        .collect();
    assert_eq!(told, ["Failed(NoMemory)"]);
    assert!(io.closed);
}

#[test]
fn a_line_past_the_bounds_ends_the_session_as_a_protocol_failure() {
    for proto in [Proto::Imap, Proto::Smtp] {
        let mut fake = Fake::new(proto, Script::default());
        let st = MemoryStorage::new();
        let long = vec![b'x'; 20_000];
        let told: Vec<String> = match proto {
            Proto::Imap => {
                let mut session =
                    new::Imap::new(new_endpoint(new::Security::Implicit, 993), vec![]);
                session.start(&mut NativeIo(&mut fake)).unwrap();
                session
                    .step(new::Input::Bytes(&long), &mut NativeIo(&mut fake), &st)
                    .unwrap()
                    .iter()
                    .map(new_told)
                    .collect()
            }
            Proto::Smtp => {
                let mut session = new::Smtp::new(
                    new_endpoint(new::Security::Implicit, 465),
                    new::Outgoing::new("a@b.c".into(), vec!["d@e.f".into()], vec![]),
                );
                session.start(&mut NativeIo(&mut fake)).unwrap();
                session
                    .step(new::Input::Bytes(&long), &mut NativeIo(&mut fake))
                    .unwrap()
                    .iter()
                    .map(new_told)
                    .collect()
            }
        };
        assert_eq!(
            told.last().map(String::as_str),
            Some("Failed(Protocol)"),
            "{proto:?}"
        );
        assert!(fake.closed);
    }
}

#[test]
fn the_password_is_not_in_a_debug_text() {
    let endpoint = new_endpoint(new::Security::Implicit, 993);
    assert!(!format!("{endpoint:?}").contains("secret"));
    let session = new::Imap::new(endpoint, vec![]);
    assert!(!format!("{session:?}").contains("secret"));
}

/// Unused here, kept so that both stacks' unused imports stay honest.
#[allow(dead_code, reason = "keeps the imports of both stacks honest")]
fn _types(_: CertificateHash, _: SocketError, _: Fail) {}
