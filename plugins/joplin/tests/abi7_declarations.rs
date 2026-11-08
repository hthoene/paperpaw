//! The native declarations name what the interface-6 registration names: the
//! four settings (kinds, defaults, flags, titles, help, the set-up screen as
//! their editor), the screen and its instance policy, the actions, the place
//! in the tools and the icons; and the host's rules accept or refuse them as
//! they should.
#![allow(clippy::unwrap_used, reason = "test assertions")]
#[path = "../declarations.rs"]
#[allow(dead_code, reason = "tokens the plugin uses later")]
mod declarations;
#[path = "../../test-support/declared.rs"]
mod declared;
use paperpaw_core::native_host::registrations::{Record, records_decode};
use paperpaw_plugin_joplin::Joplin;
use paperpaw_sdk::native::declare::Declarations;
use paperpaw_sdk::{Contribution, Id, Plugin, Registrar};

fn legacy() -> Vec<Contribution> {
    let mut r = Registrar::new(Id::new_static("joplin"));
    Joplin.register(&mut r).unwrap();
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
    let native = declared::native("joplin", &all);
    let old = declared::legacy("joplin", "core.group.library", legacy());
    assert!(native.groups.is_empty());
    assert_eq!(native.specs.len(), 4);
    assert_eq!(native.specs, old.specs);
    assert_eq!(native.extras, old.extras);
    assert_eq!(native.extras.len(), 4);
    assert!(native.extras.iter().all(|e| e.contains("setup")));
    assert_eq!(native.screens, old.screens);
    assert_eq!(native.actions, old.actions);
    assert_eq!(native.menu_entries, old.menu_entries);
    assert_eq!(native.menu_entries, ["core.tools|joplin.open|49"]);
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
                new.push((format!("joplin.{}", s.name), (s.flags >> 6) & 3));
            }
        }
    }
    assert_eq!(new, old);
    assert_eq!(new, [("joplin.setup".to_string(), 1)]);
}
