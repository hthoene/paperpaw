//! The background check (owner decision 52, `native_background.rs`): the choices of
//! *How often*, a round over the accounts one connection after the other, and
//! the heap one check takes (a counting allocator: the most the plugin's own
//! calls hold at once beyond what they found, the card double's copies of what
//! is written included).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests unwrap what they just made"
)]

mod fake;

#[path = "../declarations.rs"]
#[allow(dead_code, reason = "the builders are used by the build script too")]
mod declarations;

use fake::{Fake, Input, Proto, Script};
use paperpaw_plugin_mail::native_model::background::{self, Round};
use paperpaw_plugin_mail::native_model::browser::{self, Mode};
use paperpaw_plugin_mail::native_model::job::{Job, Run};
use paperpaw_plugin_mail::native_model::link::{Net, Notice, Peer};
use paperpaw_plugin_mail::native_model::mem::{Error, Secret};
use paperpaw_plugin_mail::native_model::session::{Endpoint, Security};
use paperpaw_plugin_mail::native_model::store;
use paperpaw_sdk::native::sockets::{Reason, SocketKind, Untrusted};
use paperpaw_sdk::{KeyHash, Storage, Tls};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};

/// The counters are the process's: one test at a time.
static SERIAL: Mutex<()> = Mutex::new(());

/// Counts the bytes live and the most live since the last reset.
struct Counting;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

// SAFETY: every call is the system allocator's; only counters are added.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller's contract, passed on.
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            let live = LIVE.fetch_add(layout.size(), Ordering::SeqCst) + layout.size();
            PEAK.fetch_max(live, Ordering::SeqCst);
        }
        ptr
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::SeqCst);
        // SAFETY: the caller's contract, passed on.
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new: usize) -> *mut u8 {
        // SAFETY: the caller's contract, passed on.
        let moved = unsafe { System.realloc(ptr, layout, new) };
        if !moved.is_null() {
            LIVE.fetch_sub(layout.size(), Ordering::SeqCst);
            let live = LIVE.fetch_add(new, Ordering::SeqCst) + new;
            PEAK.fetch_max(live, Ordering::SeqCst);
        }
        moved
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

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
        n,
        if n % 3 == 0 {
            "\\Seen".into()
        } else {
            String::new()
        },
        format!(
            "From: Ada Example <ada@example.org>\r\nTo: bob@example.org\r\nSubject: A subject of the kind people write, number {n}\r\nDate: Tue, 3 Nov 2026 09:15:00 +0100\r\nMessage-ID: <{n}@example.org>\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nHello number {n}.\r\n"
        ),
    )
}




/// Drives the job on the fake until it is not working; the most the job's own
/// calls held at once (above what was live when each began).
fn drive(job: &mut Job, fake: &mut Fake) -> usize {
    let mut wire = Wire::default();
    let mut most = 0;
    for _ in 0..200_000 {
        if !job.working() {
            return most;
        }
        let Some(input) = fake.next_input() else {
            return most;
        };
        if matches!(input, Input::Secured { .. }) {
            fake.switched();
        }
        let notice = notice_of(&input, &mut wire);
        let storage = fake.storage.clone();
        let before = LIVE.load(Ordering::SeqCst);
        PEAK.store(before, Ordering::SeqCst);
        job.notice(
            FakeNet {
                fake,
                wire: &mut wire,
            },
            notice,
            &*storage,
        )
        .unwrap();
        most = most.max(PEAK.load(Ordering::SeqCst).saturating_sub(before));
    }
    panic!("the job never ended");
}

/// What the background check does for account `number`: the folders and the
/// inbox, `fetch` mails of it.
fn check_of(number: u8, fetch: u32) -> Vec<paperpaw_plugin_mail::native_model::session::Task> {
    browser::tasks(&Mode::Boxes, number, fetch, 1_793_693_700).unwrap()
}

fn start(job: &mut Job, fake: &mut Fake, number: u8, fetch: u32) {
    let mut wire = Wire::default();
    job.start_imap(
        FakeNet {
            fake,
            wire: &mut wire,
        },
        endpoint(Security::Implicit, 993, "secret"),
        check_of(number, fetch),
    )
    .unwrap();
}

fn server(messages: u32) -> Fake {
    let script = Script {
        messages: (1..=messages).map(message).collect(),
        ..Script::default()
    };
    Fake::new(Proto::Imap, script)
}

#[test]
fn the_choices_are_fifteen_thirty_and_sixty_minutes_and_the_declared_ones() {
    let _one = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    assert_eq!(background::every_ms("mail.every.15"), 900_000);
    assert_eq!(background::every_ms("mail.every.30"), 1_800_000);
    assert_eq!(background::every_ms("mail.every.60"), 3_600_000);
    // What is none of them is the default and never under the minute the host
    // allows for `every_ms`.
    for odd in ["", "mail.every.5", "mail.every.0"] {
        assert_eq!(background::every_ms(odd), background::DEFAULT_MINUTES * 60_000);
        assert!(background::every_ms(odd) >= 60_000);
    }
    // The setting declares the same three (the default is the middle one).
    let declared: Vec<&str> = declarations::EVERY.to_vec();
    let known: Vec<&str> = background::EVERY.iter().map(|(key, _)| *key).collect();
    assert_eq!(declared, known);
    assert_eq!(background::minutes(declarations::EVERY[1]), background::DEFAULT_MINUTES);
}

#[test]
fn a_round_goes_through_the_accounts_in_order_and_a_failure_ends_it() {
    let _one = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let mut round = Round::new(&[1, 3]).unwrap();
    assert_eq!(round.front(), Some(1));
    round.pass();
    assert_eq!(round.front(), Some(3));
    assert!(!round.is_over());
    round.end();
    assert!(round.is_over());
    assert_eq!(round.front(), None);
    round.pass();
    assert!(Round::new(&[]).unwrap().is_over());
}

#[test]
fn a_round_checks_one_account_after_the_other_and_leaves_what_the_widgets_read() {
    let _one = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let mut round = Round::new(&[1, 2]).unwrap();
    let mut checked = Vec::new();
    while let Some(number) = round.front() {
        // One connection at a time: a job that has not ended takes no second.
        let mut job = Job::new();
        let mut fake = server(7 * u32::from(number));
        start(&mut job, &mut fake, number, 20);
        assert!(job.working());
        drive(&mut job, &mut fake);
        assert_eq!(job.run, Run::Done, "account {number}");
        let storage = fake.storage.clone();
        let status = store::status_file(number, "INBOX").unwrap();
        let counts = store::Counts::read(&*storage, &status).unwrap();
        // Two thirds of the mails are unseen: the number the widget shows.
        let all = 7 * u32::from(number);
        assert_eq!(counts.messages, all);
        assert_eq!(counts.unseen, all - all / 3);
        assert!(storage.size(&store::folders_file(number).unwrap()).is_ok());
        checked.push(number);
        round.pass();
    }
    assert_eq!(checked, [1, 2]);
    assert!(round.is_over());
}

#[test]
fn a_check_holds_a_bounded_heap() {
    let _one = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    // What one check asks of the heap at once, on this computer (64-bit
    // pointers; the card double's copy of the written files is in it), for
    // the default number of mails (20) and the most the setting allows (50).
    let mut peaks = Vec::new();
    for fetch in [20, 50] {
        let mut job = Job::new();
        let mut fake = server(120);
        start(&mut job, &mut fake, 1, fetch);
        let peak = drive(&mut job, &mut fake);
        assert_eq!(job.run, Run::Done);
        println!("peak of one check of {fetch} mails: {peak} bytes");
        peaks.push(peak);
    }
    assert!(peaks[0] < 32 * 1024, "{} bytes at once", peaks[0]);
    assert!(peaks[1] < 64 * 1024, "{} bytes at once", peaks[1]);
    // More mails fetched is more held, but not in proportion to the mailbox.
    assert!(peaks[1] > peaks[0]);
}
