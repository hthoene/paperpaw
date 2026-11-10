#![allow(clippy::all, clippy::pedantic, missing_docs, dead_code, unused)]
//! The native mail sessions give the same dialogue as the interface-6 ones on
//! the same fake servers: the same lines sent, the same events, the same files
//! on the card, in pieces of any size, over TLS from the start and with
//! STARTTLS, and where they differ (a refused allocation, a socket that says
//! no) they say so.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests unwrap what they just made"
)]

mod fake;

use fake::{Fake, Proto, Script};
use paperpaw_plugin_mail::native_model::fail::Fail;
use paperpaw_plugin_mail::native_model::mem::{Error, Secret};
use paperpaw_plugin_mail::native_model::session as new;
use paperpaw_plugin_mail::native_model::store as new_store;
use paperpaw_plugin_mail::session as old;
use paperpaw_plugin_mail::store as old_store;
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
        old::Io::connect_tls(self.0, host, port, tls_of(pin));
        Ok(())
    }
    fn connect_plain(&mut self, host: &str, port: u16) -> Result<(), Error> {
        old::Io::connect_plain(self.0, host, port);
        Ok(())
    }
    fn start_tls(&mut self, pin: Option<[u8; 32]>) -> Result<(), Error> {
        old::Io::start_tls(self.0, tls_of(pin));
        Ok(())
    }
    fn send(&mut self, data: &[u8]) -> Result<(), Error> {
        old::Io::send(self.0, data.to_vec());
        Ok(())
    }
    fn send_message(&mut self, data: Vec<u8>) -> Result<(), Error> {
        old::Io::send(self.0, data);
        Ok(())
    }
    fn read_bytes(&mut self, max: u32) -> Result<(), Error> {
        old::Io::read_bytes(self.0, max);
        Ok(())
    }
    fn read_file(&mut self, path: &str, len: u64) -> Result<(), Error> {
        old::Io::read_file(self.0, path, len);
        Ok(())
    }
    fn close(&mut self) {
        old::Io::close(self.0);
    }
}

/// The notice the fake made, as the native session takes it.
fn to_new(input: &old::Input) -> new::Input<'_> {
    match input {
        old::Input::Connected => new::Input::Connected,
        old::Input::Secured {
            certificate,
            key,
            time_checked,
        } => new::Input::Secured {
            certificate: *certificate.sha256(),
            key: *key.sha256(),
            time_checked: *time_checked,
        },
        old::Input::Untrusted {
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
        old::Input::Bytes(bytes) => new::Input::Bytes(bytes),
        old::Input::Saved(path) => new::Input::Saved(path),
        old::Input::Closed => new::Input::Closed,
        old::Input::Failed(..) => new::Input::Failed(Reason::Io),
    }
}

/// An event as text, the same for both: a certificate is its hashes.
fn old_told(event: &old::Event) -> String {
    match event {
        old::Event::Untrusted { why, .. } => format!("Untrusted({why:?})"),
        other => format!("{other:?}"),
    }
}

fn new_told(event: &new::Event) -> String {
    match event {
        new::Event::Untrusted { why, .. } => format!("Untrusted({why:?})"),
        other => format!("{other:?}"),
    }
}

fn old_endpoint(security: old::Security, port: u16) -> old::Endpoint {
    old::Endpoint {
        host: "mail.example.org".into(),
        port,
        security,
        user: "ada".into(),
        password: "secret".into(),
        pin: None,
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

static DUMP: std::sync::Mutex<std::collections::BTreeMap<String, Vec<String>>> =
    std::sync::Mutex::new(std::collections::BTreeMap::new());

fn dump(label: &str, run: &Run, files: Option<Vec<(String, Option<Vec<u8>>)>>, got: bool) {
    let mut lines = Vec::new();
    for e in &run.events {
        lines.push(format!("event: {e}"));
    }
    for s in &run.sent {
        lines.push(format!("sent: {s:?}"));
    }
    lines.push(format!("closed: {}", run.closed));
    lines.push(format!("switches: {}", run.switches));
    for (name, content) in files.unwrap_or_default() {
        lines.push(format!(
            "file {name}: {:?}",
            content.map(|b| String::from_utf8_lossy(&b).into_owned())
        ));
    }
    if got {
        lines.push(format!("got: {:?}", run.fake.smtp_got));
    }
    let mut map = DUMP.lock().unwrap();
    map.insert(label.to_string(), lines);
    let mut text = String::new();
    for (label, lines) in map.iter() {
        for l in lines {
            text.push_str(&format!("{label}\t{l}\n"));
        }
    }
    std::fs::write(
        format!("{}/testdata/session.golden", env!("CARGO_MANIFEST_DIR")),
        text,
    )
    .unwrap();
}

fn finished(events: &[String]) -> bool {
    events
        .iter()
        .any(|e| e == "Done" || e.starts_with("Failed") || e.starts_with("Untrusted"))
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Which {
    Old,
    New,
}

fn tasks_old(account: u8, messages: usize, all: bool) -> Vec<old::Task> {
    let mut tasks = vec![
        old::Task::Folders {
            out: old_store::folders_file(account),
        },
        old::Task::Sync {
            folder: "INBOX".into(),
            count: 50,
            index: old_store::index_file(account, "INBOX"),
            status: old_store::status_file(account, "INBOX"),
            now: 1_793_693_700,
        },
    ];
    if all {
        tasks.push(old::Task::Body {
            folder: "INBOX".into(),
            uid: (messages as u32) * 10,
            out: old_store::raw_file(account, "INBOX", (messages as u32) * 10),
        });
        tasks.push(old::Task::Flag {
            folder: "INBOX".into(),
            uid: 10,
            flag: "\\Seen",
            on: true,
        });
    }
    tasks
}

/// The same tasks, with the paths the native store keeps.
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

fn run_imap(
    which: Which,
    script: Script,
    security: old::Security,
    tasks: &(Vec<old::Task>, Vec<new::Task>),
) -> Run {
    let port = if security == old::Security::Implicit {
        993
    } else {
        143
    };
    let mut fake = Fake::new(Proto::Imap, script);
    let mut events: Vec<String> = Vec::new();
    match which {
        Which::Old => {
            let mut session = old::Imap::new(old_endpoint(security, port), tasks.0.clone());
            events.extend(session.start(&mut fake).iter().map(old_told));
            for _ in 0..200_000 {
                if finished(&events) {
                    break;
                }
                let Some(input) = fake.next_input() else {
                    break;
                };
                if matches!(input, old::Input::Secured { .. }) {
                    fake.switched();
                }
                let storage = fake.storage.clone();
                events.extend(
                    session
                        .step(input, &mut fake, &*storage)
                        .iter()
                        .map(old_told),
                );
            }
        }
        Which::New => {
            let security = match security {
                old::Security::Implicit => new::Security::Implicit,
                _ => new::Security::StartTls,
            };
            let mut session = new::Imap::new(new_endpoint(security, port), tasks.1.clone());
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
                if matches!(input, old::Input::Secured { .. }) {
                    fake.switched();
                }
                let storage = fake.storage.clone();
                let told = session
                    .step(to_new(&input), &mut NativeIo(&mut fake), &*storage)
                    .unwrap();
                events.extend(told.iter().map(new_told));
            }
        }
    }
    Run {
        events,
        sent: fake.sent.clone(),
        closed: fake.closed,
        switches: fake.tls_switches,
        fake,
    }
}

/// The files both sessions leave, by name.
fn files(run: &Run, which: Which, account: u8) -> Vec<(String, Option<Vec<u8>>)> {
    let st = run.fake.storage.clone();
    let (folders, index, status, raw) = match which {
        Which::Old => (
            old_store::folders_file(account),
            old_store::index_file(account, "INBOX"),
            old_store::status_file(account, "INBOX"),
            old_store::raw_file(account, "INBOX", 30),
        ),
        Which::New => (
            new_store::folders_file(account).unwrap(),
            new_store::index_file(account, "INBOX").unwrap(),
            new_store::status_file(account, "INBOX").unwrap(),
            new_store::raw_file(account, "INBOX", 30).unwrap(),
        ),
    };
    [
        ("folders", folders),
        ("index", index),
        ("status", status),
        ("raw", raw),
    ]
    .into_iter()
    .map(|(name, path)| (name.to_string(), st.get(&path)))
    .collect()
}

/// Runs both and says where they differ.
fn same_imap(
    script: Script,
    security: old::Security,
    messages: usize,
    all: bool,
    label: &str,
) -> (Run, Run) {
    let tasks = (tasks_old(1, messages, all), tasks_new(1, messages, all));
    let old = run_imap(Which::Old, script.clone(), security, &tasks);
    dump(
        &format!("imap {label}"),
        &old,
        Some(files(&old, Which::Old, 1)),
        false,
    );
    let new = run_imap(Which::New, script, security, &tasks);
    assert_eq!(old.events, new.events, "{label}: events");
    assert_eq!(old.sent, new.sent, "{label}: lines sent");
    assert_eq!(old.closed, new.closed, "{label}: closed");
    assert_eq!(old.switches, new.switches, "{label}: TLS switches");
    assert_eq!(
        files(&old, Which::Old, 1),
        files(&new, Which::New, 1),
        "{label}: files"
    );
    (old, new)
}

#[test]
fn the_whole_read_is_the_same_in_pieces_of_any_size() {
    for chunk in [1usize, 3, 17, 100, 1000, 100_000] {
        for security in [old::Security::Implicit, old::Security::StartTls] {
            let mut script = Script {
                chunk,
                ..Script::default()
            };
            script.messages = vec![
                message(1, "\\Seen"),
                message(2, ""),
                message(3, "\\Flagged"),
            ];
            let (old, new) = same_imap(
                script,
                security,
                3,
                true,
                &format!("chunk {chunk} {security:?}"),
            );
            assert_eq!(old.events.last().map(String::as_str), Some("Done"));
            assert!(!new.fake.sent_plain_password);
            // The files really were written (a native read gives them back).
            let st = new.fake.storage.clone();
            let rows =
                new_store::read_index(&*st, &new_store::index_file(1, "INBOX").unwrap()).unwrap();
            assert_eq!(rows.iter().map(|r| r.uid).collect::<Vec<_>>(), [30, 20, 10]);
        }
    }
}

#[test]
fn the_logins_the_failures_and_the_empty_folder_are_the_same() {
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
        let (old, _) = same_imap(script, old::Security::Implicit, 0, false, label);
        assert!(finished(&old.events), "{label}: {:?}", old.events);
    }
}

#[test]
fn a_large_message_is_the_same_cut_at_what_was_asked() {
    let big = format!(
        "From: a@b.c\r\nSubject: big\r\n\r\n{}",
        "line of text for a big message\r\n".repeat(9000)
    );
    let script = Script {
        messages: vec![(10, "".into(), big)],
        ..Script::default()
    };
    let tasks = (
        vec![old::Task::Body {
            folder: "INBOX".into(),
            uid: 10,
            out: old_store::raw_file(1, "INBOX", 10),
        }],
        vec![new::Task::Body {
            folder: "INBOX".into(),
            uid: 10,
            out: new_store::raw_file(1, "INBOX", 10).unwrap(),
        }],
    );
    let old = run_imap(Which::Old, script.clone(), old::Security::Implicit, &tasks);
    dump("imap large", &old, None, false);
    let new = run_imap(Which::New, script, old::Security::Implicit, &tasks);
    assert_eq!(old.events, new.events);
    assert_eq!(old.events.last().map(String::as_str), Some("Done"));
    let a = old
        .fake
        .storage
        .get(&old_store::raw_file(1, "INBOX", 10))
        .unwrap();
    let b = new
        .fake
        .storage
        .get(&new_store::raw_file(1, "INBOX", 10).unwrap())
        .unwrap();
    assert_eq!(a.len(), old_store::MAX_FETCH);
    assert_eq!(a, b);
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
}

#[test]
fn a_hung_up_connection_and_the_stages_are_the_same() {
    let script = Script {
        messages: vec![message(1, "")],
        ..Script::default()
    };
    let (old, _) = same_imap(script.clone(), old::Security::Implicit, 1, true, "stages");
    let stages: Vec<&str> = old
        .events
        .iter()
        .filter(|e| e.starts_with("Stage"))
        .map(String::as_str)
        .collect();
    assert_eq!(stages.len(), 6);
    // The server hangs up after five inputs.
    let mut told = Vec::new();
    for which in [Which::Old, Which::New] {
        let mut fake = Fake::new(Proto::Imap, script.clone());
        let st = MemoryStorage::new();
        let mut events: Vec<String> = Vec::new();
        let mut inputs = 0;
        match which {
            Which::Old => {
                let mut session = old::Imap::new(
                    old_endpoint(old::Security::Implicit, 993),
                    tasks_old(1, 1, true),
                );
                events.extend(session.start(&mut fake).iter().map(old_told));
                while let Some(input) = fake.next_input() {
                    inputs += 1;
                    let input = if inputs == 5 {
                        old::Input::Closed
                    } else {
                        input
                    };
                    events.extend(session.step(input, &mut fake, &st).iter().map(old_told));
                    if inputs == 5 {
                        break;
                    }
                }
            }
            Which::New => {
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
                    let input = if inputs == 5 {
                        old::Input::Closed
                    } else {
                        input
                    };
                    let told = session
                        .step(to_new(&input), &mut NativeIo(&mut fake), &st)
                        .unwrap();
                    events.extend(told.iter().map(new_told));
                    if inputs == 5 {
                        break;
                    }
                }
            }
        }
        told.push(events);
    }
    {
        let mut map = DUMP.lock().unwrap();
        map.insert(
            "imap hung up".into(),
            told[0].iter().map(|e| format!("event: {e}")).collect(),
        );
        let mut text = String::new();
        for (label, lines) in map.iter() {
            for l in lines {
                text.push_str(&format!("{label}\t{l}\n"));
            }
        }
        std::fs::write(
            format!("{}/testdata/session.golden", env!("CARGO_MANIFEST_DIR")),
            text,
        )
        .unwrap();
    }
    assert_eq!(told[0], told[1]);
    assert_eq!(
        told[0].last().map(String::as_str),
        Some("Failed(Network(Lost))")
    );
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
    let draft = paperpaw_plugin_mail::compose::Draft {
        from: "ada@example.org".into(),
        to: "bob@example.org, carol@example.org".into(),
        subject: "Hi".into(),
        body: "Line\n.dot first\n".into(),
        ..Default::default()
    };
    let to = draft.check().unwrap();
    let data =
        paperpaw_plugin_mail::smtp::data(&draft.message("Tue, 3 Nov 2026 09:15:00 +0000", "1"));
    (draft.from.clone(), to, data)
}

fn run_smtp(which: Which, script: Script, security: old::Security) -> Run {
    let port = if security == old::Security::Implicit {
        465
    } else {
        587
    };
    let (from, to, data) = outgoing_parts();
    let mut fake = Fake::new(Proto::Smtp, script);
    let mut events: Vec<String> = Vec::new();
    match which {
        Which::Old => {
            let mut session = old::Smtp::new(
                old_endpoint(security, port),
                old::Outgoing { from, to, data },
            );
            events.extend(session.start(&mut fake).iter().map(old_told));
            for _ in 0..100_000 {
                if finished(&events) {
                    break;
                }
                let Some(input) = fake.next_input() else {
                    break;
                };
                if matches!(input, old::Input::Secured { .. }) {
                    fake.switched();
                }
                events.extend(session.step(input, &mut fake).iter().map(old_told));
            }
        }
        Which::New => {
            let security = match security {
                old::Security::Implicit => new::Security::Implicit,
                _ => new::Security::StartTls,
            };
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
                if matches!(input, old::Input::Secured { .. }) {
                    fake.switched();
                }
                let told = session
                    .step(to_new(&input), &mut NativeIo(&mut fake))
                    .unwrap();
                events.extend(told.iter().map(new_told));
            }
        }
    }
    Run {
        events,
        sent: fake.sent.clone(),
        closed: fake.closed,
        switches: fake.tls_switches,
        fake,
    }
}

fn same_smtp(script: Script, security: old::Security, label: &str) -> Run {
    let old = run_smtp(Which::Old, script.clone(), security);
    dump(&format!("smtp {label}"), &old, None, true);
    let new = run_smtp(Which::New, script, security);
    assert_eq!(old.events, new.events, "{label}: events");
    assert_eq!(old.sent, new.sent, "{label}: lines sent");
    assert_eq!(old.closed, new.closed, "{label}: closed");
    assert_eq!(old.switches, new.switches, "{label}: TLS switches");
    assert_eq!(old.fake.smtp_got, new.fake.smtp_got, "{label}: the message");
    new
}

#[test]
fn a_message_is_sent_the_same_in_pieces_of_any_size() {
    for chunk in [1usize, 5, 1000] {
        for security in [old::Security::Implicit, old::Security::StartTls] {
            let script = Script {
                chunk,
                ..Script::default()
            };
            let new = same_smtp(script, security, &format!("chunk {chunk} {security:?}"));
            assert_eq!(new.events.last().map(String::as_str), Some("Done"));
            assert!(new.fake.smtp_got.is_some());
            assert!(!new.fake.sent_plain_password);
        }
    }
}

#[test]
fn the_smtp_failures_are_the_same() {
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
        ("no starttls", no_starttls, old::Security::StartTls),
        ("login only", login_only.clone(), old::Security::Implicit),
        ("login only starttls", login_only, old::Security::StartTls),
        ("wrong login password", wrong, old::Security::Implicit),
        ("refused recipient", refused, old::Security::Implicit),
        ("too large", small, old::Security::Implicit),
        ("untrusted", untrusted, old::Security::Implicit),
        ("no clock", no_clock, old::Security::Implicit),
    ] {
        let run = same_smtp(script, security, label);
        assert!(finished(&run.events), "{label}: {:?}", run.events);
    }
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
