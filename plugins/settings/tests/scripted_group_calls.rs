//! How often a settings page asks the registry for the list of settings: a
//! group's page asks for the group's own settings and, for each of its parts,
//! whether it holds one, and never walks the whole list; the page of a setting
//! to focus asks for that one setting. The registry is scripted, with many
//! settings in groups the page does not show (426 on a real device).
#![cfg(feature = "linked")]
#![allow(clippy::unwrap_used, reason = "test assertions")]
use paperpaw_core::native_host::environment::{
    SNAPSHOT_BYTES, SNAPSHOT_VERSION, SettingValueRef, settings_encode,
};
use paperpaw_core::native_host::registrations::{
    EventScreenOpened, EventScreenShown, RawRecord, Record, ServiceEnd, ServiceRegGroup,
    ServiceRegQuery, ServiceRegSetting, ServiceRegSettingExtra, ServiceUiHandle, ServiceUiParam,
    record_encode_one,
};
use paperpaw_plugin_settings::NATIVE_ENTRY;
use paperpaw_scripted_host::{ScriptedHost, event_batch};
use std::cell::RefCell;
use std::rc::Rc;

const GROUP: u32 = 11;
const LIST: u32 = 10;
const INSTANCE: u32 = 7;
/// `reg_setting.flags`: has a value, shown.
const SHOWN: u32 = (1 << 6) | (1 << 9);
/// Rows of a page.
const PAGE: usize = 32;
/// Settings in groups that no page here shows: seven pages of the whole list.
const FAR: usize = 220;

/// (key, group) of every setting, in registry order.
fn settings() -> Vec<(String, String)> {
    let mut all: Vec<(String, String)> = Vec::new();
    for i in 0..3 {
        all.push((format!("t.own{i}"), "t.parent".into()));
    }
    for i in 0..FAR {
        all.push((format!("t.far{i}"), "t.far".into()));
    }
    for i in 0..2 {
        all.push((format!("t.part{i}"), "t.part".into()));
    }
    all
}

/// (id, parent) of every group.
const GROUPS: [(&str, &str); 4] = [
    ("t.parent", ""),
    ("t.part", "t.parent"),
    ("t.empty", "t.parent"),
    ("t.far", ""),
];

fn stream(records: &[Record<'_>]) -> Vec<u8> {
    let mut out = Vec::new();
    for record in records {
        let mut one = vec![0; 4096];
        let used = record_encode_one(record, &mut one).unwrap();
        out.extend_from_slice(&one[..used]);
    }
    out
}

fn end(total: usize, next: usize) -> Record<'static> {
    Record::ServiceEnd(ServiceEnd {
        flags: u32::from(next == total),
        total: u32::try_from(total).unwrap(),
        generation: 1,
        next: u32::try_from(next).unwrap(),
        cursor: None,
    })
}

/// A switch's value (off) as the host encodes one.
fn off() -> Vec<u8> {
    let mut bytes = [core::mem::MaybeUninit::<u8>::uninit(); 64];
    let len = settings_encode(SettingValueRef::Bool(false), &mut bytes).unwrap();
    bytes[..len]
        .iter()
        // SAFETY: `settings_encode` initialized the first `len` bytes.
        .map(|byte| unsafe { byte.assume_init() })
        .collect()
}

/// (scope, cursor) of every page of the list of settings that was asked for.
type Asked = Rc<RefCell<Vec<(String, u32)>>>;

fn host(asked: &Asked, params: &'static [(&'static str, &'static str)]) -> ScriptedHost {
    let host = ScriptedHost::for_plugin("settings");
    for folder in [
        concat!(env!("CARGO_MANIFEST_DIR"), "/i18n"),
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../crates/paperpaw-core/i18n"
        ),
    ] {
        host.add_catalogs_in(folder).unwrap();
    }
    let mut bytes = vec![0_u8; SNAPSHOT_BYTES];
    bytes[0..4].copy_from_slice(&SNAPSHOT_VERSION.to_le_bytes());
    bytes[4..8].copy_from_slice(&u32::try_from(SNAPSHOT_BYTES).unwrap().to_le_bytes());
    bytes[32..34].copy_from_slice(&480_u16.to_le_bytes());
    bytes[34..36].copy_from_slice(&800_u16.to_le_bytes());
    bytes[36..38].copy_from_slice(&220_u16.to_le_bytes());
    bytes[42] = 1;
    host.set_env(&bytes);
    let registry = host.registry();
    let seen = asked.clone();
    registry.on(1, move |input| {
        let ServiceRegQuery { scope, cursor, .. } =
            RawRecord::single(input)?.decode::<ServiceRegQuery<'_>>()?;
        seen.borrow_mut().push((scope.to_string(), cursor));
        let rows: Vec<(String, String)> = settings()
            .into_iter()
            .filter(|(_, group)| scope.is_empty() || group == scope)
            .collect();
        let from = cursor as usize;
        let upto = (from + PAGE).min(rows.len());
        let value = off();
        let mut page: Vec<Record<'_>> = rows[from..upto]
            .iter()
            .map(|(key, group)| {
                Record::ServiceRegSetting(ServiceRegSetting {
                    key,
                    group,
                    kind: 0,
                    flags: SHOWN,
                    source: 0,
                    title: key,
                    value: &value,
                })
            })
            .collect();
        page.push(end(rows.len(), upto));
        Ok(stream(&page))
    });
    registry.on(2, move |input| {
        let (key, group) = settings()
            .into_iter()
            .find(|(key, _)| {
                input
                    .windows(key.len())
                    .any(|window| window == key.as_bytes())
            })
            .ok_or(-1)?;
        let value = off();
        Ok(stream(&[
            Record::ServiceRegSetting(ServiceRegSetting {
                key: &key,
                group: &group,
                kind: 0,
                flags: SHOWN,
                source: 0,
                title: &key,
                value: &value,
            }),
            Record::ServiceRegSettingExtra(ServiceRegSettingExtra {
                min: 0,
                max: 0,
                step: 0,
                unit: "",
                section: "",
                follows: "",
                shown_id: "",
                shown_value: "",
                editor_kind: 0,
                editor: "",
                choices: 0,
            }),
        ]))
    });
    registry.on(4, move |input| {
        let ServiceRegQuery { scope, .. } =
            RawRecord::single(input)?.decode::<ServiceRegQuery<'_>>()?;
        // The children of a group; every group where the scope is empty.
        let listed: Vec<&(&str, &str)> = GROUPS
            .iter()
            .filter(|(_, parent)| scope.is_empty() || *parent == scope)
            .collect();
        let mut page: Vec<Record<'_>> = listed
            .iter()
            .map(|(id, parent)| {
                Record::ServiceRegGroup(ServiceRegGroup {
                    id,
                    parent,
                    page: "",
                    order: 0,
                    title: id,
                })
            })
            .collect();
        page.push(end(listed.len(), listed.len()));
        Ok(stream(&page))
    });
    // No page of a plugin adds a row to a group.
    registry.on(12, |_| Ok(stream(&[end(0, 0)])));
    host.service("core.ui", 1, 0, move |op, _| match op {
        16 => Ok(stream(&[Record::ServiceUiHandle(ServiceUiHandle {
            handle: 9,
        })])),
        17 => {
            let mut rows: Vec<Record<'_>> = params
                .iter()
                .map(|(key, value)| Record::ServiceUiParam(ServiceUiParam { key, value }))
                .collect();
            rows.push(Record::ServiceEnd(ServiceEnd {
                flags: 0,
                total: u32::try_from(params.len()).unwrap(),
                generation: 0,
                next: 0,
                cursor: None,
            }));
            Ok(stream(&rows))
        }
        _ => Err(-1),
    });
    host
}

/// Opens `screen` with the hand-over's `params`, and what the list of
/// settings was asked for while it opened.
fn opened(
    screen: &'static str,
    token: u32,
    params: &'static [(&'static str, &'static str)],
) -> Vec<(String, u32)> {
    let asked = Asked::default();
    let host = host(&asked, params);
    let plugin = host.load(NATIVE_ENTRY.entry()).unwrap();
    let open = event_batch(&Record::EventScreenOpened(EventScreenOpened {
        now_ms: 0,
        screen,
        instance: INSTANCE,
        slot: "",
        flags: 0,
        handover: 7,
        params: 0,
        text_bytes: 0,
    }))
    .unwrap();
    assert_eq!(plugin.event(token, &open).unwrap().status, 0);
    let shown = event_batch(&Record::EventScreenShown(EventScreenShown {
        now_ms: 0,
        screen,
        width: 480,
        height: 800,
        instance: Some(INSTANCE),
        last_input: 0,
    }))
    .unwrap();
    assert_eq!(plugin.event(token, &shown).unwrap().status, 0);
    let asked = asked.borrow().clone();
    asked
}

#[test]
fn a_groups_page_asks_for_its_own_settings_and_its_parts_not_the_whole_list() {
    let asked = opened("settings.group", GROUP, &[("id", "t.parent")]);
    // The group's own settings (3: one page), then each part: `t.part` holds
    // one (found, so only that group is asked), `t.empty` nothing.
    assert!(
        asked.iter().all(|(scope, _)| !scope.is_empty()),
        "the whole list was walked: {asked:?}"
    );
    assert_eq!(asked.len(), 3, "{asked:?}");
    assert_eq!(asked[0], ("t.parent".to_string(), 0));
}

#[test]
fn the_page_of_a_setting_to_focus_asks_for_that_setting_and_its_group_only() {
    let asked = opened("settings.list", LIST, &[("focus", "t.own1")]);
    assert!(
        asked.iter().all(|(scope, _)| !scope.is_empty()),
        "the whole list was walked: {asked:?}"
    );
    // The group of the setting: its settings, then its part.
    assert_eq!(asked.len(), 3, "{asked:?}");
}
