//! What the plugin declares: the group and its three settings, the page, the
//! two actions, the entry, the icons, the device needs and the first-run step;
//! the host's rules accept or refuse the batches as they should. They are read
//! back and compared with what is written here (what the interface-6
//! registration said, before it was removed) and pinned byte for byte by a
//! reviewed golden (`PAPERPAW_BLESS=1` writes it anew).
#![allow(clippy::unwrap_used, reason = "test assertions")]
#![allow(
    clippy::redundant_closure,
    reason = "the declaring functions are generic over the sink: a closure fixes it"
)]
#[path = "../declarations.rs"]
#[allow(dead_code, reason = "tokens the build script and the plugin use")]
mod declarations;
#[path = "../../test-support/declared.rs"]
mod declared;
use paperpaw_core::native_host::registrations::{Record, records_decode};
use paperpaw_sdk::native::declare::{Declarations, DeclareError};

fn batches(
    declare: impl FnOnce(
        &mut Declarations<&mut dyn FnMut(&[u8]) -> Result<(), i32>>,
    ) -> Result<(), DeclareError>,
) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    {
        let mut sink = |b: &[u8]| -> Result<(), i32> {
            out.push(b.to_vec());
            Ok(())
        };
        let mut d = Declarations::new(&mut sink as &mut dyn FnMut(&[u8]) -> Result<(), i32>);
        declare(&mut d).unwrap();
    }
    out
}

fn all() -> Vec<Vec<u8>> {
    let mut all = Vec::new();
    all.extend(batches(|d| declarations::settings(d)));
    all.extend(batches(|d| declarations::screens(d)));
    all.extend(batches(|d| declarations::navigation(d)));
    all
}

#[test]
fn the_declarations_are_the_reviewed_golden() {
    let mut framed = Vec::new();
    for batch in all()
        .into_iter()
        .chain(batches(|d| declarations::events(d)))
    {
        framed.extend_from_slice(&u32::try_from(batch.len()).unwrap().to_le_bytes());
        framed.extend_from_slice(&batch);
    }
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/testdata/declarations.bin.v1");
    if std::env::var_os("PAPERPAW_BLESS").is_some() {
        std::fs::write(path, &framed).unwrap();
    }
    assert!(
        framed == std::fs::read(path).unwrap(),
        "the declarations changed: review, then PAPERPAW_BLESS=1 cargo test"
    );
}

/// What the batches say, decoded and checked by the host's rules: the group
/// (order 94), the three settings (the server's address in the developers'
/// group), the page, the two actions, the page's row first in the group, the
/// icons, and the `Firmware` need of the group, the server's address, looking
/// by itself and both actions' rows.
#[test]
fn the_decoded_batches_say_what_the_registration_said() {
    let native = declared::native("update", &all());
    assert_eq!(native.groups, ["group|update.group|94||"]);
    assert_eq!(
        native.specs,
        [
            "server|3|0|core.group.developer|update.setting.server|update.setting.server.help|t:https://paperpaw.org|0|0|\"\"",
            "look|0|0|update.group|update.setting.look|update.setting.look.help|b:0|0|0|\"\"",
            "betas|0|0|update.group|update.setting.betas|update.setting.betas.help|b:0|0|0|\"\"",
        ]
    );
    assert!(native.extras.is_empty());
    assert_eq!(native.screens, ["page|update.title"]);
    assert_eq!(native.actions, ["check|update.check", "open|update.title"]);
    assert_eq!(native.menu_entries, ["update.group|update.open|0"]);
    assert_eq!(
        native.icons,
        [
            "update.check|cloud-download",
            "update.group|cloud-download",
            "update.open|circle-arrow-up",
        ]
    );
    assert_eq!(
        native.needs,
        [
            "update.check|Firmware",
            "update.group|Firmware",
            "update.look|Firmware",
            "update.open|Firmware",
            "update.server|Firmware",
        ]
    );
    // The first-run step is a record the host checks and then refuses as
    // unsupported until its setup takes it.
    assert_eq!(native.others, ["SetupStep"], "{:?}", native.others);
}

/// The page opens one at a time: the decoded `screen.spec` flags (bits 6 and
/// 7: 0 per parameters, 1 single, 2 many).
#[test]
fn the_page_opens_one_at_a_time() {
    let mut new = Vec::new();
    for batch in batches(|d| declarations::screens(d)) {
        for record in records_decode(&batch).unwrap().iter() {
            if let Record::ScreenSpec(s) = record {
                new.push((format!("update.{}", s.name), (s.flags >> 6) & 3));
            }
        }
    }
    assert_eq!(new, [("update.page".to_string(), 1)]);
}

/// The first-run step, field by field: the name, the page and its kind, the
/// order, and the title and help (which the interface-6 step left empty to
/// take the setting's, so they are the setting's keys here). Its
/// applicability, a closure on the environment there, is the `Firmware`
/// need here: set, and no condition on a setting.
#[test]
fn the_first_run_step_asks_the_look_setting_where_the_firmware_is_updated() {
    let mut found = Vec::new();
    for batch in batches(|d| declarations::navigation(d)) {
        for record in records_decode(&batch).unwrap().iter() {
            if let Record::SetupStep(s) = record {
                found.push((
                    s.name.to_string(),
                    s.title.to_string(),
                    s.help.to_string(),
                    s.order,
                    s.page_kind,
                    s.page.to_string(),
                    s.applies_key.to_string(),
                    s.applies_value.to_string(),
                    s.need,
                    s.flags,
                ));
            }
        }
    }
    let [new] = found.as_slice() else {
        panic!("one step expected");
    };
    assert_eq!(new.0, "setup");
    assert_eq!(new.1, "update.setting.look");
    assert_eq!(new.2, "update.setting.look.help");
    assert_eq!(new.3, 57);
    assert_eq!((new.4, new.5.as_str()), (0, "update.look"));
    // No condition on a setting; the host applies the need (flags: bit 0
    // applies, bit 1 a need).
    assert_eq!((new.6.as_str(), new.7.as_str()), ("", ""));
    assert_eq!(new.9, 0b11);
    assert_eq!(new.8, 8, "the Firmware need");
}

/// It hears the start (to say how an install went), the end of an install and
/// its progress, the answers and progress of its requests, the work that looks
/// for updates and the setting that turns that on: seven subscriptions, each
/// with its own token, in the batch the host's rules accept.
#[test]
fn the_events_are_the_card_flow_the_requests_the_work_and_the_setting() {
    let all = batches(|d| declarations::events(d));
    assert_eq!(all.len(), 1);
    let native = declared::native("update", &all);
    // The lines are sorted by event tag, not by token.
    let mut tokens: Vec<&str> = native
        .events
        .iter()
        .map(|event| event.rsplit(':').next().unwrap())
        .collect();
    tokens.sort_unstable();
    assert_eq!(
        tokens,
        [
            declarations::STARTED,
            declarations::INSTALL_DONE,
            declarations::FIRMWARE_CHANGED,
            declarations::HTTP_DONE,
            declarations::HTTP_PROGRESS,
            declarations::WORK_DUE,
            declarations::SETTING_CHANGED
        ]
        .map(|token| token.to_string())
    );
    assert!(native.others.is_empty(), "{:?}", native.others);
}
