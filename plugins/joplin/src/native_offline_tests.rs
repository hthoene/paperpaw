//! The offer to go online where Offline mode holds the sending back (#459, as
//! Beta 3's set-up screen), driven through the scripted host: the reader's
//! sending ends with `Reason::Offline` (11), the host's question is asked,
//! *Go online* sends `command.go_online` and the work that sends again; held
//! back once more it only says it needs the network; *Not now* says so and
//! sends nothing; a sending by itself never asks.
//!
//! The scripted host has no storage, so the plugin is wrapped: a test token
//! starts its sending over notes in memory (as the due work or *Send* would);
//! the end of the request, the question, its answer and what follows go
//! through the real plugin.
#![allow(clippy::unwrap_used, reason = "test assertions")]
use super::NativeJoplin;
use crate::declarations::{HTTP_TOKEN, SEND_TOKEN};
use crate::native_joplin::Service;
use crate::native_keep::Secret;
use crate::native_kept as kept;
use crate::native_send::{Account, Call, Start, Step, Wire};
use crate::test_volume::Mem;
use paperpaw_core::native_host::commands::{WireCommand, commands_decode};
use paperpaw_core::native_host::registrations::{
    EventActionRun, EventHttpDone, EventStarted, Record, records_decode,
};
use paperpaw_scripted_host::{Answer, Callback, Dialogs, ScriptedHost, event_batch};
use paperpaw_sdk::Handled;
use paperpaw_sdk::native::{
    Error, Host, Plugin,
    callbacks::{DrawFrame, Input},
    events::Incoming,
    net::RequestId,
    reply::Reply,
};
use paperpaw_ui_kit::files::Volume;
use std::cell::RefCell;
use std::rc::Rc;
use std::string::{String, ToString};
use std::vec::Vec;

/// The test tokens: the sending starts as the reader's (*Send*, the action's
/// work) or by itself (the retry's work).
const READERS: u32 = 9_001;
const BY_ITSELF: u32 = 9_002;
/// The English `joplin.no-network`.
const NO_NETWORK: &str = "Sending to Joplin needs a network.";

/// A WebDAV account and one book's notes waiting, in memory.
struct Notes {
    data: Mem,
    card: Mem,
    next: u32,
}

impl Wire for Notes {
    fn data(&self) -> &dyn Volume {
        &self.data
    }
    fn notes(&self) -> &dyn Volume {
        &self.card
    }
    fn account(&mut self) -> Result<Account, Error> {
        Ok(Account {
            service: Service::WebDav,
            base: "https://j.example".to_string(),
            user: "me".to_string(),
            notebook: "PaperPaw".to_string(),
        })
    }
    fn password(&mut self) -> Result<Option<Secret>, Error> {
        Secret::copy("pw").map(Some)
    }
    fn network(&mut self) -> Result<(), Error> {
        Ok(())
    }
    fn now_ms(&mut self) -> Option<i64> {
        Some(1_792_483_200_000)
    }
    fn answer(&mut self, _id: RequestId) -> Result<Secret, Error> {
        Err(Error::NotFound)
    }
    fn send(&mut self, _call: &Call<'_>) -> Result<RequestId, Error> {
        RequestId::new(self.next)
    }
}

/// The plugin, with the test tokens that start its sending.
struct Held {
    plugin: NativeJoplin,
    next: u32,
}

impl Plugin for Held {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        Ok(Self {
            plugin: NativeJoplin::make(host)?,
            next: 0,
        })
    }

    fn event(
        &mut self,
        host: &Host<'_>,
        token: u32,
        incoming: Incoming<'_>,
        reply: &mut Reply<'_>,
    ) -> Result<(), Error> {
        let start = match token {
            READERS => Start::Action,
            BY_ITSELF => Start::Retry,
            _ => return self.plugin.event(host, token, incoming, reply),
        };
        self.next += 1;
        let mut notes = Notes {
            data: Mem::default(),
            card: Mem::default(),
            next: self.next,
        };
        kept::add_waiting(&notes.data, "Notes/a.md")?;
        self.plugin.sender.start(&mut notes, start).map(|_| ())
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, frame: DrawFrame<'_>) -> Result<(), Error> {
        self.plugin.draw(host, token, frame)
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        self.plugin.input(host, token, input)
    }
}

paperpaw_sdk::native_plugin!(
    Held,
    metadata = crate::native_entry::JOPLIN_ABI_METADATA,
    linked = HELD
);

type Plug<'a> = paperpaw_scripted_host::Plugin<'a>;
/// The keys of the work declared, in order.
type Declared = Rc<RefCell<Vec<String>>>;

/// The plugin's and the core's texts, questions (`core.ui`) and a `core.work` that keeps
/// the keys declared.
fn world(host: &ScriptedHost) -> (Dialogs, Declared) {
    // The question's words are the core's.
    host.add_catalogs_in("i18n").unwrap();
    host.add_catalogs_in("../../crates/paperpaw-core/i18n")
        .unwrap();
    let declared = Declared::default();
    let seen = declared.clone();
    host.service("core.work", 1, 0, move |op, input| {
        let batch = records_decode(input).map_err(|_| -1)?;
        match (op, batch.iter().next()) {
            (1, Some(Record::ServiceWorkSpec(spec))) => {
                seen.borrow_mut().push(spec.key.to_string());
                Ok(Vec::new())
            }
            _ => Err(-1),
        }
    });
    host.reply_bound("core.work", 1, 64);
    (host.dialogs(), declared)
}

fn event(p: &Plug<'_>, token: u32, record: Record<'_>) -> Callback {
    p.event(token, &event_batch(&record).unwrap()).unwrap()
}

/// The sending starts (`READERS` or `BY_ITSELF`), request `id` in flight.
fn begin(p: &Plug<'_>, token: u32) {
    let started = event(p, token, Record::EventStarted(EventStarted { now_ms: 0 }));
    assert_eq!(started.status, 0);
}

/// The reader's run of the action *Send to Joplin*.
fn action(p: &Plug<'_>) -> Callback {
    event(
        p,
        SEND_TOKEN,
        Record::EventActionRun(EventActionRun {
            now_ms: 0,
            action: "joplin.send",
            flags: 0,
        }),
    )
}

/// The end of request `id` (`info.json`), held back by Offline mode.
fn held_back(p: &Plug<'_>, id: u32) -> Callback {
    event(
        p,
        HTTP_TOKEN,
        Record::EventHttpDone(EventHttpDone {
            now_ms: 0,
            id,
            token: Step::Info.token(),
            status: 0,
            reason: 11,
            flags: 0,
            body_len: 0,
        }),
    )
}

fn commands(callback: &Callback) -> Vec<WireCommand<'_>> {
    callback
        .batches
        .iter()
        .flat_map(|batch| commands_decode(batch).unwrap().iter().collect::<Vec<_>>())
        .collect()
}

fn went_online(callback: &Callback) -> bool {
    commands(callback)
        .iter()
        .any(|c| matches!(c, WireCommand::GoOnline { for_ms: 300_000 }))
}

/// Whether it said that it needs the network (`joplin.no-network`).
fn no_network(callback: &Callback) -> bool {
    commands(callback)
        .iter()
        .any(|c| matches!(c, WireCommand::Notify { text, .. } if text.starts_with(NO_NETWORK)))
}

fn answer(p: &Plug<'_>, dialogs: &Dialogs, request: u32, yes: bool) -> Callback {
    let batch = dialogs.answer(request, &Answer::Confirm(yes)).unwrap();
    p.event(0, &batch).unwrap()
}

#[test]
fn offline_mode_offers_to_go_online_once_and_go_online_sends_again() {
    let host = ScriptedHost::new();
    let (dialogs, declared) = world(&host);
    let p = host.load(HELD.entry()).unwrap();
    assert_eq!(action(&p).status, 0);
    assert_eq!(*declared.borrow(), ["joplin.send"]);
    begin(&p, READERS);
    // Held back: the host's question, not a notice.
    let held = held_back(&p, 1);
    assert_eq!(held.status, 0);
    assert!(!no_network(&held));
    let asked = dialogs.last().expect("the offer is asked");
    assert_eq!(asked.kind, 2, "a confirm");
    assert_eq!(asked.title, "Offline mode");
    assert_eq!(asked.text, "This needs the network. Go online for it?");
    // *Go online*: the network is asked for and the notes are sent again (the
    // action's work, at once).
    let online = answer(&p, &dialogs, asked.request, true);
    assert_eq!(online.status, 0);
    assert!(went_online(&online), "command.go_online is sent");
    assert_eq!(*declared.borrow(), ["joplin.send", "joplin.send"]);
    // Held back again (the host did not go online): offered once, it says it
    // needs the network.
    begin(&p, READERS);
    let questions = dialogs.questions().len();
    let again = held_back(&p, 2);
    assert!(no_network(&again));
    assert_eq!(dialogs.questions().len(), questions, "not offered again");
    assert_eq!(declared.borrow().len(), 2);
    // The reader sends anew: it may be offered again.
    action(&p);
    begin(&p, READERS);
    held_back(&p, 3);
    assert_eq!(dialogs.questions().len(), questions + 1);
}

#[test]
fn not_now_says_it_needs_the_network_and_sends_nothing() {
    let host = ScriptedHost::new();
    let (dialogs, declared) = world(&host);
    let p = host.load(HELD.entry()).unwrap();
    begin(&p, READERS);
    held_back(&p, 1);
    let asked = dialogs.last().unwrap();
    let not_now = answer(&p, &dialogs, asked.request, false);
    assert_eq!(not_now.status, 0);
    assert!(!went_online(&not_now));
    assert!(no_network(&not_now));
    assert!(declared.borrow().is_empty(), "nothing is sent again");
}

#[test]
fn a_sending_by_itself_never_asks() {
    let host = ScriptedHost::new();
    let (dialogs, declared) = world(&host);
    let p = host.load(HELD.entry()).unwrap();
    begin(&p, BY_ITSELF);
    let quiet = held_back(&p, 1);
    assert_eq!(quiet.status, 0);
    assert!(dialogs.questions().is_empty());
    assert!(!no_network(&quiet), "quiet");
    assert!(declared.borrow().is_empty());
}
