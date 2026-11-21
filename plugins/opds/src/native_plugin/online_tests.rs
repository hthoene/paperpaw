//! The offer to go online where Offline mode held a feed back (#459), through
//! the real plugin: the feed's end with reason Offline (11) asks the host's
//! question; *Go online* sends `command.go_online` and asks for the same feed
//! again; held back once more it only says it needs the network; *Not now*
//! says so and asks for nothing.
//!
//! The scripted host has no storage (no list of catalogs), so the plugin is
//! wrapped: a test token opens the browse screen on a catalog given here (as
//! its `ScreenOpened` does on one of the list); the end of the request, the
//! question, its answer and what follows go through the real plugin.
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test assertions"
)]
use super::{NativeOpds, View};
use crate::declarations::{BROWSE_SCREEN, HTTP_TOKEN};
use crate::native_catalogs::Catalog;
use crate::native_web::FEED;
use paperpaw_core::native_host::commands::{WireCommand, commands_decode};
use paperpaw_core::native_host::registrations::{
    EventHttpDone, EventScreenOpened, EventScreenShown, Record, ServiceNetStarted,
    record_encode_one, records_decode,
};
use paperpaw_scripted_host::{Answer, Callback, Dialogs, ScriptedHost, event_batch};
use paperpaw_sdk::Handled;
use paperpaw_sdk::native::{
    Error, Host, Plugin,
    callbacks::{DrawFrame, Input},
    events::{Event, Incoming, kinds},
    reply::Reply,
};
use paperpaw_ui_kit::read_event;
use std::cell::RefCell;
use std::rc::Rc;
use std::string::{String, ToString};
use std::vec::Vec;

/// The test token: the browse screen opens on [`URL`].
const BROWSE_AT: u32 = 9_001;
const URL: &str = "https://books.example.org/opds";
const INSTANCE: u32 = 4;
/// The English `opds.no-network`.
const NO_NETWORK: &str = "This needs a network connection.";

std::thread_local! {
    /// What the browse screen's line says, after each callback.
    static NOTE: RefCell<String> = const { RefCell::new(String::new()) };
}

/// The plugin, with the test token that opens its browse screen.
struct Held {
    plugin: NativeOpds,
}

impl Held {
    fn browse_at(&mut self, host: &Host<'_>, incoming: Incoming<'_>) -> Result<(), Error> {
        let Some(Event::ScreenOpened { instance, .. }) = read_event!(incoming; kinds::ScreenOpened)
        else {
            return Err(Error::Invalid);
        };
        let state = self.plugin.state()?;
        state.open(host)?;
        state.browse(host, instance, Catalog::new(URL)?, None)
    }
}

impl Plugin for Held {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        Ok(Self {
            plugin: NativeOpds::make(host)?,
        })
    }

    fn event(
        &mut self,
        host: &Host<'_>,
        token: u32,
        incoming: Incoming<'_>,
        reply: &mut Reply<'_>,
    ) -> Result<(), Error> {
        let result = if token == BROWSE_AT {
            self.browse_at(host, incoming)
        } else {
            self.plugin.event(host, token, incoming, reply)
        };
        if let Ok(state) = self.plugin.state()
            && let Some(View::Browse(view)) = state.screens.top(BROWSE_SCREEN)
        {
            let note = view.note().to_string();
            NOTE.with(|n| *n.borrow_mut() = note);
        }
        // The scripted host stages no screen commands: the redraw each of
        // these callbacks asks for last is refused as unsupported (what came
        // before it was submitted all the same, see `went_online`).
        match result {
            Err(Error::Unsupported) => Ok(()),
            result => result,
        }
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
    metadata = include_bytes!("../../native/metadata.host.bin"),
    linked = HELD
);

type Plug<'a> = paperpaw_scripted_host::Plugin<'a>;
/// The requests `core.net` was asked for, in order: `GET <url> #<token>`.
type Log = Rc<RefCell<Vec<String>>>;

fn stream(record: &Record<'_>) -> Vec<u8> {
    let mut one = std::vec![0; 256];
    let used = record_encode_one(record, &mut one).unwrap();
    one.truncate(used);
    one
}

/// The texts (the plugin's and the core's, whose are the offer's words),
/// questions (`core.ui`) and a `core.net` that starts requests 1, 2, ...
fn world(host: &ScriptedHost) -> (Dialogs, Log) {
    for folder in [
        concat!(env!("CARGO_MANIFEST_DIR"), "/i18n"),
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../crates/paperpaw-core/i18n"
        ),
    ] {
        host.add_catalogs_in(folder).unwrap();
    }
    let log = Log::default();
    let seen = log.clone();
    host.service("core.net", 1, 0, move |op, input| {
        let batch = records_decode(input).map_err(|_| -1)?;
        match (op, batch.iter().next()) {
            (1, Some(Record::ServiceNetRequest(request))) => {
                let mut seen = seen.borrow_mut();
                seen.push(std::format!(
                    "{} {} #{}",
                    request.method,
                    request.url,
                    request.token
                ));
                let id = u32::try_from(seen.len()).unwrap();
                Ok(stream(&Record::ServiceNetStarted(ServiceNetStarted { id })))
            }
            // Cancels: nothing to do.
            (2, _) => Ok(Vec::new()),
            _ => Err(-1),
        }
    });
    (host.dialogs(), log)
}

fn event(p: &Plug<'_>, token: u32, record: &Record<'_>) -> Callback {
    p.event(token, &event_batch(record).unwrap()).unwrap()
}

/// A browse screen opens: its first feed is asked for.
fn browse(p: &Plug<'_>, log: &Log) {
    let before = log.borrow().len();
    let opened = event(
        p,
        BROWSE_AT,
        &Record::EventScreenOpened(EventScreenOpened {
            now_ms: 0,
            screen: "opds.browse",
            instance: INSTANCE,
            slot: "",
            flags: 0,
            handover: 0,
            params: 0,
            text_bytes: 0,
        }),
    );
    assert_eq!(opened.status, 0);
    // On top, as the host says before its first draw.
    let shown = event(
        p,
        BROWSE_SCREEN,
        &Record::EventScreenShown(EventScreenShown {
            now_ms: 0,
            screen: "opds.browse",
            width: 480,
            height: 800,
            instance: Some(INSTANCE),
            last_input: 0,
        }),
    );
    assert_eq!(shown.status, 0);
    assert_eq!(log.borrow().len(), before + 1);
    assert_eq!(log.borrow()[before], std::format!("GET {URL} #{FEED}"));
}

/// The end of the feed's request `id`, held back by Offline mode.
fn held_back(p: &Plug<'_>, id: u32) -> Callback {
    event(
        p,
        HTTP_TOKEN,
        &Record::EventHttpDone(EventHttpDone {
            now_ms: 0,
            id,
            token: FEED,
            status: 0,
            reason: 11,
            flags: 0,
            body_len: 0,
        }),
    )
}

fn answer(p: &Plug<'_>, dialogs: &Dialogs, request: u32, answer: &Answer) -> Callback {
    let batch = dialogs.answer(request, answer).unwrap();
    p.event(BROWSE_SCREEN, &batch).unwrap()
}

fn went_online(callback: &Callback) -> bool {
    callback
        .batches
        .iter()
        .flat_map(|batch| commands_decode(batch).unwrap().iter().collect::<Vec<_>>())
        .any(|c| matches!(c, WireCommand::GoOnline { for_ms: 300_000 }))
}

fn note() -> String {
    NOTE.with(|n| n.borrow().clone())
}

/// Held back on a screen just opened: the host's question, with the core's
/// words, and nothing asked for meanwhile.
fn offered(p: &Plug<'_>, dialogs: &Dialogs, log: &Log) -> u32 {
    browse(p, log);
    assert_eq!(held_back(p, 1).status, 0);
    let asked = dialogs.questions();
    assert_eq!(asked.len(), 1, "the offer is asked: {asked:?}");
    assert_eq!(asked[0].kind, 2, "a confirm");
    assert_eq!(asked[0].title, "Offline mode");
    assert_eq!(asked[0].choices, "Go online\nNot now");
    assert_eq!(log.borrow().len(), 1, "nothing asked for before the answer");
    asked[0].request
}

#[test]
fn offline_mode_offers_to_go_online_once_and_go_online_loads_the_feed_again() {
    let host = ScriptedHost::new();
    let (dialogs, log) = world(&host);
    let p = host.load(HELD.entry()).unwrap();
    let request = offered(&p, &dialogs, &log);
    // *Go online* (choice 1): the network is asked for, the same feed again.
    let online = answer(&p, &dialogs, request, &Answer::Confirm(true));
    assert_eq!(online.status, 0);
    assert!(went_online(&online), "command.go_online is sent");
    assert_eq!(log.borrow().len(), 2);
    assert_eq!(log.borrow()[1], std::format!("GET {URL} #{FEED}"));
    assert!(
        note().starts_with("Loading books.example.org"),
        "{}",
        note()
    );
    // Held back again (the host did not go online): not offered again, the
    // line says it needs the network.
    assert_eq!(held_back(&p, 2).status, 0);
    assert_eq!(dialogs.questions().len(), 1, "offered once");
    assert_eq!(note(), NO_NETWORK);
    assert_eq!(log.borrow().len(), 2);
    // A screen opened anew may offer again.
    browse(&p, &log);
    assert_eq!(held_back(&p, 3).status, 0);
    assert_eq!(dialogs.questions().len(), 2);
}

#[test]
fn not_now_says_it_needs_the_network_and_asks_for_nothing() {
    for choice in [Answer::Confirm(false), Answer::Cancelled] {
        let host = ScriptedHost::new();
        let (dialogs, log) = world(&host);
        let p = host.load(HELD.entry()).unwrap();
        let request = offered(&p, &dialogs, &log);
        let not_now = answer(&p, &dialogs, request, &choice);
        assert_eq!(not_now.status, 0);
        assert!(!went_online(&not_now), "{choice:?}");
        assert_eq!(log.borrow().len(), 1, "{choice:?}: nothing asked for again");
        assert_eq!(note(), NO_NETWORK, "{choice:?}");
    }
}
