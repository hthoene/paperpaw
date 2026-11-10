//! The native declarations name what the interface-6 registration names: the
//! group and its three settings, the page, the two actions, the entry, the
//! icons, the device needs and the first-run step; and the host's rules
//! accept or refuse them as they should.
#![allow(clippy::unwrap_used, reason = "test assertions")]
#[path = "../declarations.rs"]
#[allow(dead_code, reason = "tokens the build script and the plugin use")]
mod declarations;
#[path = "../../test-support/declared.rs"]
mod declared;
use paperpaw_core::native_host::registrations::{Record, records_decode};
use paperpaw_plugin_update::Update;
use paperpaw_sdk::native::declare::Declarations;
use paperpaw_sdk::{Contribution, Id, Plugin, Registrar};

fn legacy() -> Vec<Contribution> {
    let mut r = Registrar::new(Id::new_static("update"));
    Update.register(&mut r).unwrap();
    r.into_contributions()
}

fn batches(
    declare: impl FnOnce(&mut Declarations<&mut dyn FnMut(&[u8]) -> Result<(), i32>>),
) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    {
        let mut sink = |b: &[u8]| -> Result<(), i32> {
            out.push(b.to_vec());
            Ok(())
        };
        let mut d = Declarations::new(&mut sink as &mut dyn FnMut(&[u8]) -> Result<(), i32>);
        declare(&mut d);
    }
    out
}

/// What the batches say, decoded and checked by the host's rules, is what the
/// interface-6 registration says.
#[test]
fn the_decoded_batches_say_what_the_registration_says() {
    let mut all = Vec::new();
    all.extend(batches(|d| declarations::settings(d).unwrap()));
    all.extend(batches(|d| declarations::screens(d).unwrap()));
    all.extend(batches(|d| declarations::navigation(d).unwrap()));
    let native = declared::native("update", &all);
    let old = declared::legacy("update", "update.group", legacy());
    assert_eq!(native.groups, old.groups);
    assert_eq!(native.groups.len(), 1);
    // The comparison reads the settings of the group; the server's address is
    // in the developers' group.
    let server: Vec<&String> = native
        .specs
        .iter()
        .filter(|s| s.starts_with("server|"))
        .collect();
    assert_eq!(server.len(), 1);
    assert!(
        server[0].contains("https://paperpaw.org") && server[0].contains("core.group.developer")
    );
    let in_group: Vec<String> = native
        .specs
        .iter()
        .filter(|s| !s.starts_with("server|"))
        .cloned()
        .collect();
    assert_eq!(in_group, old.specs);
    assert_eq!(in_group.len(), 2);
    assert_eq!(native.screens, old.screens);
    assert_eq!(native.actions, old.actions);
    assert_eq!(native.menu_entries, old.menu_entries);
    assert_eq!(native.icons, old.icons);
    assert_eq!(native.needs, old.needs);
    assert_eq!(native.needs.len(), 5);
    assert!(native.needs.iter().all(|n| n.ends_with("|Firmware")));
    // The first-run step is a record the host checks and then refuses as
    // unsupported until its setup takes it.
    assert_eq!(native.others, ["SetupStep"], "{:?}", native.others);
}

/// The page opens one at a time: the decoded `screen.spec` flags (bits 6 and
/// 7: 0 per parameters, 1 single, 2 many) say what the registration says.
#[test]
fn the_page_opens_one_at_a_time() {
    let mut old = Vec::new();
    for contribution in legacy() {
        if let Contribution::FixedScreens(list) = contribution {
            for s in list.iter() {
                let policy = match s.how_many() {
                    i if i == paperpaw_sdk::Instances::PER_PARAMS => 0,
                    i if i == paperpaw_sdk::Instances::SINGLE => 1,
                    i if i == paperpaw_sdk::Instances::MANY => 2,
                    other => panic!("{other:?}"),
                };
                old.push((s.id().as_str().to_string(), policy));
            }
        }
    }
    let mut new = Vec::new();
    for batch in batches(|d| declarations::screens(d).unwrap()) {
        for record in records_decode(&batch).unwrap().iter() {
            if let Record::ScreenSpec(s) = record {
                new.push((format!("update.{}", s.name), (s.flags >> 6) & 3));
            }
        }
    }
    assert_eq!(new, old);
    assert_eq!(new, [("update.page".to_string(), 1)]);
}

/// The first-run step, field by field: the name, the page and its kind, the
/// order, and the title and help (which the interface-6 step left empty to
/// take the setting's, so they are the setting's keys here). Its
/// applicability, a closure on the environment there, is the `Firmware`
/// need here: set, and no condition on a setting.
#[test]
fn the_first_run_step_is_the_registrations() {
    let mut steps = Vec::new();
    let mut look = None;
    for contribution in legacy() {
        match contribution {
            Contribution::SetupStep(step) => steps.push(step),
            Contribution::FixedSettings(list) => {
                look = list
                    .iter()
                    .find(|s| s.id.as_str() == "update.look")
                    .map(|s| {
                        (
                            s.title.to_string(),
                            s.description.as_deref().map(str::to_string),
                        )
                    });
            }
            _ => {}
        }
    }
    let [old] = steps.as_slice() else {
        panic!("one step expected");
    };
    let (look_title, look_help) = look.unwrap();
    let mut found = Vec::new();
    for batch in batches(|d| declarations::navigation(d).unwrap()) {
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
    assert_eq!(format!("update.{}", new.0), old.id.as_str());
    // An empty title or help in the old step stood for the setting's.
    assert_eq!(old.title, "");
    assert_eq!(old.help, "");
    assert_eq!(new.1, look_title);
    assert_eq!(Some(new.2.clone()), look_help);
    assert_eq!(new.3, old.order);
    match &old.page {
        paperpaw_sdk::setup::SetupPage::Setting(id) => {
            assert_eq!((new.4, new.5.as_str()), (0, id.as_str()));
        }
        other => panic!("{other:?}"),
    }
    // Applicability: a closure there, the `Firmware` need here, no condition
    // on a setting; the host applies it (flags: bit 0 applies, bit 1 a need).
    assert!(old.applies.is_some());
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
    let all = batches(|d| declarations::events(d).unwrap());
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
