//! The real plugin on the scripted host: what it declares is taken, the
//! actions open its screens, *Sync now* without an address leads to the
//! account, and with one a run is declared as network work that ends with a
//! notice where no network answers. What the lists, the file and the runs do
//! is tested beside them (`model`, `keep`, `sync`); the screens and widgets
//! run on the emulator (`tools/pps/tasks.pps`).
#![cfg(feature = "linked")]
#![allow(clippy::unwrap_used, reason = "test assertions")]
use paperpaw_core::native_host::environment::{SNAPSHOT_BYTES, SNAPSHOT_VERSION, SettingValueRef};
use paperpaw_core::native_host::registrations::{
    EventActionRun, EventStarted, EventWidgetMade, EventWorkDue, Record, records_decode,
};
use paperpaw_plugin_tasks::{
    NATIVE_ENTRY,
    declarations::{LIFECYCLE_TOKEN, OPEN_TOKEN, SYNC_TOKEN, WORK_TOKEN},
};
use paperpaw_scripted_host::{Callback, ScriptedHost, event_batch};
use std::cell::RefCell;
use std::rc::Rc;

/// The environment the plugin reads: the host's clock and the time of day.
fn snapshot(host: &ScriptedHost) {
    let mut bytes = vec![0_u8; SNAPSHOT_BYTES];
    bytes[0..4].copy_from_slice(&SNAPSHOT_VERSION.to_le_bytes());
    bytes[4..8].copy_from_slice(&u32::try_from(SNAPSHOT_BYTES).unwrap().to_le_bytes());
    // Presence: bit 2 is the Unix time.
    bytes[8..12].copy_from_slice(&4_u32.to_le_bytes());
    bytes[16..24].copy_from_slice(&1_000_u64.to_le_bytes());
    bytes[24..32].copy_from_slice(&1_795_000_000_i64.to_le_bytes());
    bytes[32..34].copy_from_slice(&480_u16.to_le_bytes());
    bytes[34..36].copy_from_slice(&800_u16.to_le_bytes());
    bytes[36..38].copy_from_slice(&220_u16.to_le_bytes());
    bytes[42] = 1;
    host.set_env(&bytes);
}

/// What `core.work` was asked, in order.
type Log = Rc<RefCell<Vec<String>>>;

fn world(host: &ScriptedHost) -> Log {
    let log: Log = Rc::default();
    host.add_catalogs_in("i18n").unwrap();
    host.set_clock(1_000_000);
    snapshot(host);
    let seen = log.clone();
    host.service("core.work", 1, 0, move |op, input| {
        let batch = records_decode(input).map_err(|_| -1)?;
        match (op, batch.iter().next()) {
            (1, Some(Record::ServiceWorkSpec(spec))) => seen.borrow_mut().push(format!(
                "declare {} needs {} within {}",
                spec.key, spec.needs, spec.within_ms
            )),
            (2, Some(Record::ServiceWorkKey(key))) => {
                seen.borrow_mut().push(format!("cancel {}", key.key));
            }
            _ => return Err(-1),
        }
        Ok(Vec::new())
    });
    host.reply_bound("core.work", 1, 64);
    host.reply_bound("core.work", 2, 64);
    log
}

type Plug<'a> = paperpaw_scripted_host::Plugin<'a>;

fn event(p: &Plug<'_>, token: u32, record: Record<'_>) -> Callback {
    p.event(token, &event_batch(&record).unwrap()).unwrap()
}

fn started(p: &Plug<'_>) -> Callback {
    event(
        p,
        LIFECYCLE_TOKEN,
        Record::EventStarted(EventStarted { now_ms: 0 }),
    )
}

fn action(p: &Plug<'_>, token: u32) -> Callback {
    event(
        p,
        token,
        Record::EventActionRun(EventActionRun {
            now_ms: 0,
            action: "",
            flags: 0,
        }),
    )
}

fn asked(log: &Log) -> Vec<String> {
    log.borrow_mut().drain(..).collect()
}

#[test]
fn it_is_made_and_its_actions_open_its_screens() {
    let host = ScriptedHost::new();
    let log = world(&host);
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    let first = started(&p);
    assert_eq!(first.status, 0, "{first:?}");
    assert!(asked(&log).is_empty(), "no account: no work is declared");
    // (This host records a screen to open and then says it opens none: the
    // command counts, not the status.)
    let open = action(&p, OPEN_TOKEN);
    assert!(open.batches_contain(b"tasks.screen"));
    // Sync now without an address: said, and the account is opened.
    let sync = action(&p, SYNC_TOKEN);
    assert!(sync.batches_contain(b"tasks.account"));
    assert!(
        sync.batches_contain(b"Set the server's address"),
        "{:?}",
        sync.batches
    );
    assert!(asked(&log).is_empty());
}

#[test]
fn with_an_account_a_run_is_network_work_and_a_dead_network_is_said_not_an_error() {
    let host = ScriptedHost::new();
    let log = world(&host);
    host.set_setting(
        "tasks.address",
        SettingValueRef::Text(" cloud.example/remote.php/dav "),
    );
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    assert_eq!(started(&p).status, 0);
    // Nothing waits: only the reading when the radio is on anyway.
    assert_eq!(asked(&log), ["declare tasks.refresh needs 1 within 0"]);
    assert_eq!(action(&p, SYNC_TOKEN).status, 0);
    assert_eq!(asked(&log), ["declare tasks.sync needs 1 within 0"]);
    // The work is due, and this host has no network service: the run ends
    // with words, the callback with no error.
    let due = event(
        &p,
        WORK_TOKEN,
        Record::EventWorkDue(EventWorkDue {
            now_ms: 0,
            token: WORK_TOKEN,
            why: 3,
            key: "tasks.sync",
        }),
    );
    assert_eq!(due.status, 0);
    assert!(
        due.batches_contain(b"The sync failed (status 0)"),
        "{:?}",
        due.batches
    );
}

#[test]
fn what_is_not_ours_is_left_alone() {
    let host = ScriptedHost::new();
    let log = world(&host);
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    started(&p);
    let made = event(
        &p,
        LIFECYCLE_TOKEN,
        Record::EventWidgetMade(EventWidgetMade {
            now_ms: 0,
            widget: "weather.now",
            instance: 9,
            place: "",
        }),
    );
    assert_eq!(made.status, 0);
    assert!(asked(&log).is_empty());
    // One of ours: the day's turn is watched from now on.
    let made = event(
        &p,
        LIFECYCLE_TOKEN,
        Record::EventWidgetMade(EventWidgetMade {
            now_ms: 0,
            widget: "tasks.today",
            instance: 4,
            place: "",
        }),
    );
    assert_eq!(made.status, 0);
    assert_eq!(asked(&log), ["declare tasks.day needs 0 within 0"]);
}
