//! The real plugin driven by the scripted host where Offline mode holds the
//! fetch back (#459, as Beta 3): the reader's *Fetch the news* on the list
//! ends with `Reason::Offline` (11) and asks to go online (the host's
//! confirm); *Go online* sends `command.go_online` and fetches again, held back
//! once more it only says it needs a network; *Not now* says so and fetches
//! nothing; the background fetch (a due work) never asks.
//!
//! The scripted host has no storage: the entry here is the plugin's own with a
//! data folder in memory put into the host table it is made with.
#![cfg(feature = "linked")]
#![allow(clippy::unwrap_used, reason = "test assertions")]
use core::ffi::c_void;
use paperpaw_core::native_host::commands::{WireCommand, commands_decode};
use paperpaw_core::native_host::registrations::{
    EventHttpDone, EventScreenClosed, EventScreenOpened, EventScreenShown, EventWorkDue,
    InputAction, Record, ServiceNetStarted, record_encode_one, records_decode,
};
use paperpaw_native::host_glue::{
    ERR_NOT_FOUND, ERR_UNSUPPORTED, OK, PpBuffer, PpBytes, PpEntry, PpHost, PpPlugin,
};
use paperpaw_plugin_news::{
    NATIVE_ENTRY,
    declarations::{HTTP_TOKEN, SCREEN, WORK_TOKEN},
};
use paperpaw_scripted_host::{Answer, Callback, Dialogs, ScriptedHost, event_batch};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

type Plug<'a> = paperpaw_scripted_host::Plugin<'a>;
/// The requests sent, as `METHOD url`.
type Sent = Rc<RefCell<Vec<String>>>;

const FEED: &str = "https://a.example/feed";
const GET: &str = "GET https://a.example/feed";
const NO_NETWORK: &str = "Fetching the news needs a network.";

thread_local! {
    /// The files, by root and name (each test runs on a thread of its own).
    static FILES: RefCell<BTreeMap<(u32, String), Vec<u8>>> = const { RefCell::new(BTreeMap::new()) };
}

/// # Safety
/// `bytes` names `len` readable bytes, or is empty.
unsafe fn read_bytes(bytes: PpBytes) -> Vec<u8> {
    if bytes.len == 0 {
        return Vec::new();
    }
    // SAFETY: the plugin's live input of `len` bytes.
    unsafe { std::slice::from_raw_parts(bytes.ptr, bytes.len) }.to_vec()
}

/// # Safety
/// As [`read_bytes`].
unsafe fn name(root: u32, path: PpBytes) -> (u32, String) {
    // SAFETY: as the caller's.
    let path = unsafe { read_bytes(path) };
    (root, String::from_utf8(path).unwrap())
}

unsafe extern "C" fn size(_: *mut c_void, root: u32, path: PpBytes, out: *mut u64) -> i32 {
    // SAFETY: the plugin's path.
    let key = unsafe { name(root, path) };
    FILES.with_borrow(|files| match files.get(&key) {
        Some(data) => {
            // SAFETY: the plugin's output.
            unsafe { *out = data.len() as u64 };
            OK
        }
        None => ERR_NOT_FOUND,
    })
}

unsafe extern "C" fn read_at(
    _: *mut c_void,
    root: u32,
    path: PpBytes,
    offset: u64,
    out: PpBuffer,
) -> i32 {
    // SAFETY: the plugin's path.
    let key = unsafe { name(root, path) };
    FILES.with_borrow(|files| {
        let Some(data) = files.get(&key) else {
            return ERR_NOT_FOUND;
        };
        let from = usize::try_from(offset).unwrap().min(data.len());
        let n = (data.len() - from).min(out.cap);
        // SAFETY: the plugin's buffer of `cap` bytes.
        unsafe { core::ptr::copy_nonoverlapping(data[from..].as_ptr(), out.ptr, n) };
        i32::try_from(n).unwrap()
    })
}

unsafe extern "C" fn write(_: *mut c_void, root: u32, path: PpBytes, data: PpBytes) -> i32 {
    // SAFETY: the plugin's path and data.
    let (key, data) = unsafe { (name(root, path), read_bytes(data)) };
    FILES.with_borrow_mut(|files| files.insert(key, data));
    OK
}

unsafe extern "C" fn append(_: *mut c_void, root: u32, path: PpBytes, data: PpBytes) -> i32 {
    // SAFETY: the plugin's path and data.
    let (key, data) = unsafe { (name(root, path), read_bytes(data)) };
    FILES.with_borrow_mut(|files| files.entry(key).or_default().extend(data));
    OK
}

unsafe extern "C" fn rename(_: *mut c_void, root: u32, from: PpBytes, to: PpBytes) -> i32 {
    // SAFETY: the plugin's paths.
    let (from, to) = unsafe { (name(root, from), name(root, to)) };
    FILES.with_borrow_mut(|files| match files.remove(&from) {
        Some(data) => {
            files.insert(to, data);
            OK
        }
        None => ERR_NOT_FOUND,
    })
}

unsafe extern "C" fn remove(_: *mut c_void, root: u32, path: PpBytes) -> i32 {
    // SAFETY: the plugin's path.
    let key = unsafe { name(root, path) };
    FILES.with_borrow_mut(|files| files.remove(&key).map_or(ERR_NOT_FOUND, |_| OK))
}

unsafe extern "C" fn sync(_: *mut c_void) -> i32 {
    OK
}

/// The plugin's factory with the files above in the host's table.
unsafe extern "C" fn make(host: *const PpHost, ctx: *mut c_void, out: *mut *const PpPlugin) -> i32 {
    let Some(real) = NATIVE_ENTRY.entry().make else {
        return ERR_UNSUPPORTED;
    };
    // SAFETY: the scripted host's table, live for the call; a copy of it is
    // kept for the plugin's life (leaked: a test's few bytes).
    let mut table = unsafe { core::ptr::read(host) };
    table.storage_size = size;
    table.storage_read_at = read_at;
    table.storage_write = write;
    table.storage_append = append;
    table.storage_rename = rename;
    table.storage_remove = remove;
    table.storage_sync = sync;
    let table: &'static PpHost = Box::leak(Box::new(table));
    // SAFETY: the plugin's own factory with a table that outlives it.
    unsafe { real(table, ctx, out) }
}

fn entry() -> PpEntry {
    let real = NATIVE_ENTRY.entry();
    PpEntry {
        size: real.size,
        abi_major: real.abi_major,
        needs_minor: real.needs_minor,
        metadata: real.metadata,
        make: Some(make),
    }
}

fn stream(records: &[Record<'_>]) -> Vec<u8> {
    let mut out = Vec::new();
    for record in records {
        let mut one = vec![0; 4096];
        let used = record_encode_one(record, &mut one).unwrap();
        out.extend_from_slice(&one[..used]);
    }
    out
}

/// One feed in the data folder, the plugin's and the core's texts, questions (`core.ui`) and
/// a `core.net` that starts every request (ids 1, 2, ...).
fn world(host: &ScriptedHost) -> (Dialogs, Sent) {
    FILES.with_borrow_mut(|files| {
        files.clear();
        files.insert((0, "feeds.txt".to_string()), format!("{FEED}\tA\n").into());
    });
    host.add_catalogs_in("i18n").unwrap();
    // The core's words of the offer.
    host.add_catalog(include_str!("../../../crates/paperpaw-core/i18n/en.ini"))
        .unwrap();
    let sent = Sent::default();
    let log = sent.clone();
    host.service("core.net", 1, 0, move |op, input| {
        let batch = records_decode(input).map_err(|_| -1)?;
        match (op, batch.iter().next()) {
            (1, Some(Record::ServiceNetRequest(request))) => {
                let mut log = log.borrow_mut();
                log.push(format!("{} {}", request.method, request.url));
                Ok(stream(&[Record::ServiceNetStarted(ServiceNetStarted {
                    id: u32::try_from(log.len()).unwrap(),
                })]))
            }
            (2, _) => Ok(Vec::new()),
            _ => Err(-1),
        }
    });
    (host.dialogs(), sent)
}

fn event(p: &Plug<'_>, token: u32, record: Record<'_>) -> Callback {
    p.event(token, &event_batch(&record).unwrap()).unwrap()
}

/// The reader opens the list, chooses *Fetch the news* (its first row) and
/// leaves the list.
fn fetch_the_news(p: &Plug<'_>) -> Callback {
    let opened = event(
        p,
        SCREEN,
        Record::EventScreenOpened(EventScreenOpened {
            now_ms: 0,
            screen: "news.list",
            instance: 3,
            slot: "",
            flags: 0,
            handover: 0,
            params: 0,
            text_bytes: 0,
        }),
    );
    assert_eq!(opened.status, 0);
    // On top: its redraw is refused (the scripted host stages no screen
    // commands), what it notes is kept.
    event(
        p,
        SCREEN,
        Record::EventScreenShown(EventScreenShown {
            last_input: 0,
            now_ms: 0,
            screen: "news.list",
            width: 480,
            height: 800,
            instance: Some(3),
        }),
    );
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
    assert_eq!(chosen.status, 1, "taken");
    // The reader goes back to reading while it fetches (a screen on top would
    // be redrawn at the end, which the scripted host refuses).
    let closed = event(
        p,
        SCREEN,
        Record::EventScreenClosed(EventScreenClosed {
            now_ms: 0,
            screen: "news.list",
            instance: 3,
        }),
    );
    assert_eq!(closed.status, 0);
    chosen
}

/// The end of request `id`, held back by Offline mode (reason 11).
fn held_back(p: &Plug<'_>, id: u32) -> Callback {
    event(
        p,
        HTTP_TOKEN,
        Record::EventHttpDone(EventHttpDone {
            now_ms: 0,
            id,
            token: 1,
            status: 0,
            reason: 11,
            flags: 0,
            body_len: 0,
        }),
    )
}

/// Whether the callback submitted a command `wanted` takes.
fn sent(callback: &Callback, wanted: impl Fn(&WireCommand<'_>) -> bool) -> bool {
    callback
        .batches
        .iter()
        .any(|batch| commands_decode(batch).unwrap().iter().any(|c| wanted(&c)))
}

/// Whether the callback sent `command.go_online` (for 5 minutes).
fn went_online(callback: &Callback) -> bool {
    sent(callback, |c| {
        matches!(c, WireCommand::GoOnline { for_ms: 300_000 })
    })
}

/// Whether the callback said that fetching needs a network.
fn says_no_network(callback: &Callback) -> bool {
    sent(
        callback,
        |c| matches!(c, WireCommand::Notify { text, .. } if *text == NO_NETWORK),
    )
}

#[test]
fn offline_mode_offers_to_go_online_once_and_go_online_fetches_again() {
    let host = ScriptedHost::new();
    let (dialogs, sent) = world(&host);
    let p = host.load(&entry()).unwrap();
    fetch_the_news(&p);
    assert_eq!(*sent.borrow(), [GET]);
    // Held back: the host's question, not the notice.
    let held = held_back(&p, 1);
    assert_eq!(held.status, 0);
    assert!(!says_no_network(&held));
    let asked = dialogs.last().expect("the offer is asked");
    assert_eq!(asked.kind, 2, "a confirm");
    assert_eq!(asked.title, "Offline mode");
    assert_eq!(asked.text, "This needs the network. Go online for it?");
    assert_eq!(asked.choices, "Go online\nNot now");
    // *Go online*: the network is asked for and the feed is fetched again.
    let answer = dialogs
        .answer(asked.request, &Answer::Confirm(true))
        .unwrap();
    let online = p.event(SCREEN, &answer).unwrap();
    assert_eq!(online.status, 0);
    assert!(went_online(&online), "command.go_online is sent");
    assert_eq!(*sent.borrow(), [GET, GET]);
    // Held back again (the host did not go online): offered once, so it says
    // it needs a network.
    let again = held_back(&p, 2);
    assert_eq!(again.status, 0);
    assert!(says_no_network(&again));
    assert_eq!(dialogs.questions().len(), 1, "not asked again");
    assert_eq!(sent.borrow().len(), 2);
    // The reader's next fetch may offer again.
    fetch_the_news(&p);
    held_back(&p, 3);
    assert_eq!(dialogs.questions().len(), 2);
}

#[test]
fn not_now_says_it_needs_a_network_and_fetches_nothing() {
    let host = ScriptedHost::new();
    let (dialogs, sent) = world(&host);
    let p = host.load(&entry()).unwrap();
    fetch_the_news(&p);
    held_back(&p, 1);
    let asked = dialogs.last().unwrap();
    let answer = dialogs
        .answer(asked.request, &Answer::Confirm(false))
        .unwrap();
    let not_now = p.event(SCREEN, &answer).unwrap();
    assert_eq!(not_now.status, 0);
    assert!(!went_online(&not_now));
    assert!(says_no_network(&not_now));
    assert_eq!(*sent.borrow(), [GET], "nothing is fetched again");
}

#[test]
fn the_background_fetch_held_back_never_asks() {
    let host = ScriptedHost::new();
    let (dialogs, sent) = world(&host);
    let p = host.load(&entry()).unwrap();
    let due = event(
        &p,
        WORK_TOKEN,
        Record::EventWorkDue(EventWorkDue {
            now_ms: 0,
            token: WORK_TOKEN,
            why: 3,
            key: "news.fetch",
        }),
    );
    assert_eq!(due.status, 0);
    assert_eq!(*sent.borrow(), [GET]);
    let held = held_back(&p, 1);
    assert_eq!(held.status, 0);
    assert!(dialogs.questions().is_empty(), "no question");
    assert!(says_no_network(&held));
}
