//! The native declarations name what the interface-6 registration names: the
//! setting for where articles are fetched from (kind, choices, default, flags,
//! title, help), the screen and its instance policy, the actions, the place
//! in the tools and the icons; and the host's rules accept or refuse them as
//! they should.
#![allow(clippy::unwrap_used, reason = "test assertions")]
#[path = "../declarations.rs"]
#[allow(dead_code, reason = "tokens the plugin uses later")]
mod declarations;
#[path = "../../test-support/declared.rs"]
mod declared;
use paperpaw_core::native_host::registrations::{Record, records_decode};
use paperpaw_plugin_read_later_cloud::ReadLaterCloud;
use paperpaw_sdk::native::declare::Declarations;
use paperpaw_sdk::{Contribution, Id, Plugin, Registrar};

fn legacy() -> Vec<Contribution> {
    let mut r = Registrar::new(Id::new_static("read-later-cloud"));
    ReadLaterCloud.register(&mut r).unwrap();
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
    let native = declared::native("read-later-cloud", &all);
    let old = declared::legacy("read-later-cloud", "core.group.library", legacy());
    assert!(native.groups.is_empty());
    assert_eq!(native.specs.len(), 1);
    assert_eq!(native.specs, old.specs);
    assert!(native.specs[0].contains("read-later-cloud.from.later"));
    assert_eq!(native.extras, old.extras);
    assert_eq!(native.screens, old.screens);
    assert_eq!(native.actions, old.actions);
    assert_eq!(native.menu_entries, old.menu_entries);
    assert_eq!(native.menu_entries, ["core.tools|read-later-cloud.open|49"]);
    assert_eq!(native.icons, old.icons);
    assert_eq!(native.icons.len(), 2);
    assert!(native.others.is_empty(), "{:?}", native.others);
}

/// The set-up opens one at a time: the decoded `screen.spec` flags (bits 6 and
/// 7: 0 per parameters, 1 single, 2 many) say what the registration says.
#[test]
fn the_set_up_opens_one_at_a_time() {
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
                new.push((format!("read-later-cloud.{}", s.name), (s.flags >> 6) & 3));
            }
        }
    }
    assert_eq!(new, old);
    assert_eq!(new, [("read-later-cloud.setup".to_string(), 1)]);
}
