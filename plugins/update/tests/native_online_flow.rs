//! The real plugin driven by the scripted host: the work that looks for
//! updates (declared at start where the setting is on, taken back where it is
//! off, declared again when the setting changes), and the check a rule asks for
//! (declared work at a cheap moment). The question and the answer are tested
//! without the host (`native_online`); the scripted host has no card or data
//! folder, so the files of the flow are not here.
#![cfg(feature = "linked")]
#![allow(clippy::unwrap_used, reason = "test assertions")]
use paperpaw_core::native_host::environment::{SNAPSHOT_BYTES, SNAPSHOT_VERSION, SettingValueRef};
use paperpaw_core::native_host::registrations::{
    EventActionRun, EventSettingChanged, EventStarted, Record, ServiceDevFirmware,
    record_encode_one, records_decode,
};
use paperpaw_plugin_update::NATIVE_ENTRY;
use paperpaw_scripted_host::{Callback, ScriptedHost, event_batch};
use std::cell::RefCell;
use std::rc::Rc;

const CHECK_TOKEN: u32 = 2;
const STARTED: u32 = 3;
const SETTING_CHANGED: u32 = 9;
const WORK_DUE: u32 = 8;

type Plug<'a> = paperpaw_scripted_host::Plugin<'a>;

fn stream(record: &Record<'_>) -> Vec<u8> {
    let mut one = vec![0; 4096];
    let used = record_encode_one(record, &mut one).unwrap();
    one.truncate(used);
    one
}

/// What the work service was asked, in order.
fn world(host: &ScriptedHost, look: bool) -> Rc<RefCell<Vec<String>>> {
    let log = Rc::new(RefCell::new(Vec::new()));
    host.add_catalogs_in("i18n").unwrap();
    host.set_setting("update.look", SettingValueRef::Bool(look));
    let mut bytes = vec![0_u8; SNAPSHOT_BYTES];
    bytes[0..4].copy_from_slice(&SNAPSHOT_VERSION.to_le_bytes());
    bytes[4..8].copy_from_slice(&u32::try_from(SNAPSHOT_BYTES).unwrap().to_le_bytes());
    bytes[32..34].copy_from_slice(&480_u16.to_le_bytes());
    bytes[34..36].copy_from_slice(&800_u16.to_le_bytes());
    bytes[36..38].copy_from_slice(&220_u16.to_le_bytes());
    bytes[42] = 1;
    host.set_env(&bytes);
    host.service("core.device", 1, 0, move |op, _| match op {
        3 => Ok(stream(&Record::ServiceDevFirmware(ServiceDevFirmware {
            version: "0.4.0",
            project: "paperpaw-xteink",
            chip: 5,
            room: 0,
            undone: 0,
            state: 0,
            done: 0,
            total: 0,
            reason: 0,
            request: 0,
            flags: 0,
        }))),
        _ => Err(-1),
    });
    let work = log.clone();
    host.service("core.work", 1, 0, move |op, input| {
        let batch = records_decode(input).map_err(|_| -1)?;
        match (op, batch.iter().next()) {
            (1, Some(Record::ServiceWorkSpec(spec))) => {
                work.borrow_mut().push(format!(
                    "declare {} token {} needs {} flags {} moments {} after {} within {} every {}",
                    spec.key,
                    spec.token,
                    spec.needs,
                    spec.flags,
                    spec.moments,
                    spec.after_ms,
                    spec.within_ms,
                    spec.every_ms
                ));
                Ok(Vec::new())
            }
            (2, Some(Record::ServiceWorkKey(key))) => {
                work.borrow_mut().push(format!("cancel {}", key.key));
                Ok(Vec::new())
            }
            _ => Err(-1),
        }
    });
    host.reply_bound("core.work", 1, 64);
    host.reply_bound("core.work", 2, 64);
    log
}

fn event(p: &Plug<'_>, token: u32, record: Record<'_>) -> Callback {
    p.event(token, &event_batch(&record).unwrap()).unwrap()
}

fn started(p: &Plug<'_>) -> Callback {
    event(p, STARTED, Record::EventStarted(EventStarted { now_ms: 0 }))
}

#[test]
fn the_weekly_look_is_declared_at_start_where_the_setting_is_on() {
    let host = ScriptedHost::new();
    let log = world(&host, true);
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    // The scripted host has no data folder, so the card's part of the start
    // fails; the look is declared all the same.
    started(&p);
    // Network work, only when cheap, never looked for yet: at once; every week,
    // at the moments the network is up (2) or charging begins (4).
    assert_eq!(
        *log.borrow(),
        [
            "declare update.look-due token 8 needs 1 flags 1 moments 6 after 0 within 0 every 604800000"
        ]
    );
}

#[test]
fn the_weekly_look_is_taken_back_where_the_setting_is_off() {
    let host = ScriptedHost::new();
    let log = world(&host, false);
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    started(&p);
    assert_eq!(*log.borrow(), ["cancel update.look-due"]);
}

#[test]
fn the_setting_turns_the_look_on_and_off_and_no_other_does() {
    let host = ScriptedHost::new();
    let log = world(&host, false);
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    let change = |key: &str| {
        event(
            &p,
            SETTING_CHANGED,
            Record::EventSettingChanged(EventSettingChanged {
                now_ms: 0,
                key,
                old: &[],
            }),
        )
    };
    assert_eq!(change("update.betas").status, 0);
    assert_eq!(change("weather.every").status, 0);
    assert!(log.borrow().is_empty());
    host.set_setting("update.look", SettingValueRef::Bool(true));
    assert_eq!(change("update.look").status, 0);
    assert_eq!(log.borrow().len(), 1);
    assert!(log.borrow()[0].starts_with("declare update.look-due"));
    host.set_setting("update.look", SettingValueRef::Bool(false));
    assert_eq!(change("update.look").status, 0);
    assert_eq!(log.borrow()[1], "cancel update.look-due");
}

#[test]
fn a_check_a_rule_asks_for_waits_for_a_cheap_moment() {
    let host = ScriptedHost::new();
    let log = world(&host, false);
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    let run = event(
        &p,
        CHECK_TOKEN,
        Record::EventActionRun(EventActionRun {
            now_ms: 0,
            action: "update.check",
            flags: 0,
        }),
    );
    assert_eq!(run.status, 0);
    // Network work within the hour, when the network is up (not optional).
    assert_eq!(
        *log.borrow(),
        [
            "declare update.check-due token 8 needs 1 flags 0 moments 2 after 0 within 3600000 every 0"
        ]
    );
}

/// Work that comes due without the setting on (it was turned off meanwhile)
/// asks nothing; a check due asks, and with no network or card at all in the
/// scripted host it ends without a request and without failing.
#[test]
fn due_work_of_a_look_that_is_off_does_nothing() {
    let host = ScriptedHost::new();
    let log = world(&host, false);
    let p = host.load(NATIVE_ENTRY.entry()).unwrap();
    let due = event(
        &p,
        WORK_DUE,
        Record::EventWorkDue(paperpaw_core::native_host::registrations::EventWorkDue {
            now_ms: 0,
            token: WORK_DUE,
            why: 3,
            key: "update.look-due",
        }),
    );
    assert_eq!(due.status, 0);
    assert!(log.borrow().is_empty());
}
