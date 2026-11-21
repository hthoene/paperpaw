//! The real browser on the scripted host in Offline mode (#459): an address the
//! reader typed is held back (the request ends with reason `offline`, 11), the
//! host's question offers to go online, *Go online* sends `command.go_online`
//! and loads the address again, and *Not now* says it needs the network and
//! loads nothing. Offered once: held back again, it says so.
#![cfg(feature = "linked")]
#![allow(clippy::unwrap_used, reason = "test assertions")]
use paperpaw_core::native_host::commands::{WireCommand, commands_decode};
use paperpaw_core::native_host::registrations::{
    EventHttpDone, EventScreenOpened, EventScreenShown, InputAction, Record, ServiceNetStarted,
    record_encode_one, records_decode,
};
use paperpaw_native::host_glue::ERR_UNSUPPORTED;
use paperpaw_plugin_browser::{
    NATIVE_ENTRY,
    declarations::{LOADED, SCREEN},
};
use paperpaw_scripted_host::{Answer, Callback, Dialogs, ScriptedHost, Slot, event_batch};
use std::cell::RefCell;
use std::rc::Rc;

const INSTANCE: u32 = 3;
/// `event.http_done.reason`: held back by Offline mode.
const OFFLINE: u32 = 11;

type Plug<'a> = paperpaw_scripted_host::Plugin<'a>;

/// The addresses `core.net` was asked for, in order.
type Requests = Rc<RefCell<Vec<String>>>;

/// The texts (the browser's and the core's offer), `core.ui` for questions and
/// `core.net`, whose requests start and are logged (their ends are sent).
fn world(host: &ScriptedHost) -> (Dialogs, Requests) {
    host.add_catalogs_in("i18n").unwrap();
    host.add_catalog(include_str!("../../../crates/paperpaw-core/i18n/en.ini"))
        .unwrap();
    let dialogs = host.dialogs();
    let requests = Requests::default();
    let seen = requests.clone();
    host.service("core.net", 1, 0, move |op, input| {
        let batch = records_decode(input).map_err(|_| -1)?;
        match (op, batch.iter().next()) {
            (1, Some(Record::ServiceNetRequest(request))) => {
                seen.borrow_mut().push(request.url.to_string());
                let id = u32::try_from(seen.borrow().len()).unwrap();
                let mut out = vec![0; 64];
                let used = record_encode_one(
                    &Record::ServiceNetStarted(ServiceNetStarted { id }),
                    &mut out,
                )
                .unwrap();
                out.truncate(used);
                Ok(out)
            }
            (2, _) => Ok(Vec::new()),
            _ => Err(-1),
        }
    });
    (dialogs, requests)
}

fn event(p: &Plug<'_>, token: u32, record: Record<'_>) -> Callback {
    p.event(token, &event_batch(&record).unwrap()).unwrap()
}

/// The start screen opened from the menu and shown.
fn open(p: &Plug<'_>) {
    let opened = event(
        p,
        SCREEN,
        Record::EventScreenOpened(EventScreenOpened {
            now_ms: 0,
            screen: "browser.screen",
            instance: INSTANCE,
            slot: "",
            flags: 0,
            handover: 0,
            params: 0,
            text_bytes: 0,
        }),
    );
    assert_eq!(opened.status, 0, "{opened:?}");
    let shown = event(
        p,
        SCREEN,
        Record::EventScreenShown(EventScreenShown {
            last_input: 0,
            now_ms: 0,
            screen: "browser.screen",
            width: 480,
            height: 800,
            instance: Some(INSTANCE),
        }),
    );
    assert_eq!(shown.status, 0, "{shown:?}");
}

/// *Search or enter address* chosen and `typed` entered: the address loads.
fn enter(p: &Plug<'_>, dialogs: &Dialogs, typed: &str) {
    let chosen = p
        .input(
            SCREEN,
            &event_batch(&Record::InputAction(InputAction {
                now_ms: 0,
                action: "core.confirm",
            }))
            .unwrap(),
        )
        .unwrap();
    assert_eq!(chosen.status, 1, "{chosen:?}");
    let asked = dialogs.last().unwrap();
    assert_eq!(asked.kind, 0, "the address is typed");
    answer(p, dialogs, asked.request, &Answer::Text(typed.to_string()));
}

/// The dialog's answer to `request`, sent to the plugin.
fn answer(p: &Plug<'_>, dialogs: &Dialogs, request: u32, answer: &Answer) -> Callback {
    let batch = dialogs.answer(request, answer).unwrap();
    let done = p.event(0, &batch).unwrap();
    dialogs.release(request);
    settled(&done);
    done
}

/// Request `id` ended with `reason` and no answer.
fn ended(p: &Plug<'_>, id: u32, reason: u32) -> Callback {
    let done = event(
        p,
        LOADED,
        Record::EventHttpDone(EventHttpDone {
            now_ms: 0,
            id,
            token: id,
            status: 0,
            reason,
            flags: 0,
            body_len: 0,
        }),
    );
    settled(&done);
    done
}

/// Whether the callback submitted a command `which` matches.
fn submitted(callback: &Callback, which: impl Fn(&WireCommand<'_>) -> bool) -> bool {
    callback
        .batches
        .iter()
        .any(|batch| commands_decode(batch).unwrap().iter().any(|c| which(&c)))
}

/// Whether the callback submitted `command.go_online`.
fn went_online(callback: &Callback) -> bool {
    submitted(
        callback,
        |command| matches!(command, WireCommand::GoOnline { for_ms } if *for_ms > 0),
    )
}

/// Whether the callback asked for a redraw (the message line changed).
fn redrew(callback: &Callback) -> bool {
    submitted(callback, |command| {
        matches!(command, WireCommand::Redraw { .. })
    })
}

/// The callback ended well. This host stages no screen commands: the redraw
/// for the message line is refused (`ERR_UNSUPPORTED`) and the plugin passes
/// that on; the real host takes it.
fn settled(callback: &Callback) {
    if callback.status == ERR_UNSUPPORTED {
        let last = callback.batches.last().unwrap();
        let last = commands_decode(last).unwrap().iter().last().unwrap();
        assert!(matches!(last, WireCommand::Redraw { .. }), "{callback:?}");
    } else {
        assert_eq!(callback.status, 0, "{callback:?}");
    }
}

/// The offer: the host's confirm question with *Go online* and *Not now*.
fn offered(dialogs: &Dialogs, before: usize) -> u32 {
    let questions = dialogs.questions();
    assert_eq!(questions.len(), before + 1, "{questions:?}");
    let offer = questions.last().unwrap();
    assert_eq!(offer.kind, 2, "a confirm question: {offer:?}");
    assert_eq!(offer.title, "Offline mode");
    assert_eq!(offer.choices, "Go online\nNot now");
    offer.request
}

#[test]
fn offline_mode_offers_to_go_online_once_and_go_online_loads_again() {
    let host = ScriptedHost::new();
    let (dialogs, requests) = world(&host);
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    open(&p);
    enter(&p, &dialogs, "example.com");
    assert_eq!(*requests.borrow(), ["https://example.com"]);
    let before = dialogs.questions().len();
    // Held back by Offline mode: the offer, not the message.
    let held = ended(&p, 1, OFFLINE);
    assert_eq!(held.status, 0, "{held:?}");
    assert!(!went_online(&held) && !redrew(&held));
    let offer = offered(&dialogs, before);
    // Go online: the network is asked for and the address loaded again.
    let online = answer(&p, &dialogs, offer, &Answer::Confirm(true));
    assert!(went_online(&online), "{:?}", online.batches);
    assert_eq!(
        *requests.borrow(),
        ["https://example.com", "https://example.com"]
    );
    // That load was checked before the host went online (it does once the
    // answer's callback has returned): its end loads once more.
    let questions = dialogs.questions().len();
    let once_more = ended(&p, 2, OFFLINE);
    assert_eq!(dialogs.questions().len(), questions, "not offered again");
    assert!(!went_online(&once_more));
    assert_eq!(requests.borrow().len(), 3);
    // Held back again: offered once, so it says it needs the network.
    host.reset_counts();
    let again = ended(&p, 3, OFFLINE);
    assert_eq!(dialogs.questions().len(), questions, "not offered again");
    assert_eq!(host.calls(Slot::I18nCopy), 1, "the message is said");
    assert!(redrew(&again));
    assert!(!went_online(&again));
    assert_eq!(requests.borrow().len(), 3);
    // The reader enters an address anew: it may be offered again.
    enter(&p, &dialogs, "example.org");
    let before = dialogs.questions().len();
    ended(&p, 4, OFFLINE);
    offered(&dialogs, before);
}

#[test]
fn not_now_says_it_needs_the_network_and_loads_nothing_again() {
    let host = ScriptedHost::new();
    let (dialogs, requests) = world(&host);
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    open(&p);
    enter(&p, &dialogs, "example.com");
    let before = dialogs.questions().len();
    ended(&p, 1, OFFLINE);
    let offer = offered(&dialogs, before);
    host.reset_counts();
    let not_now = answer(&p, &dialogs, offer, &Answer::Confirm(false));
    assert!(!went_online(&not_now));
    assert_eq!(requests.borrow().len(), 1, "nothing is loaded again");
    // The message (`browser.no-network`) is copied and shown with a redraw.
    assert_eq!(host.calls(Slot::I18nCopy), 1);
    assert!(redrew(&not_now), "a redraw for the message");
}

#[test]
fn no_network_is_said_as_before_without_an_offer() {
    let host = ScriptedHost::new();
    let (dialogs, requests) = world(&host);
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    open(&p);
    enter(&p, &dialogs, "example.com");
    let before = dialogs.questions().len();
    // No network (reason 1) is not Offline mode: no offer.
    let done = ended(&p, 1, 1);
    assert_eq!(dialogs.questions().len(), before);
    assert!(redrew(&done), "the message is shown");
    assert!(!went_online(&done));
    assert_eq!(requests.borrow().len(), 1);
}

/// `command.go_online` (the offer's *Go online*) needs `uses.wifi`: the host
/// denies it to a plugin that did not declare it.
#[test]
fn the_metadata_names_the_feature_going_online_needs() {
    let entry = NATIVE_ENTRY.entry();
    // SAFETY: Linked owns immutable static metadata for the program's lifetime.
    let metadata = unsafe { std::slice::from_raw_parts(entry.metadata.ptr, entry.metadata.len) };
    assert!(metadata.windows(9).any(|window| window == b"uses.wifi"));
}
