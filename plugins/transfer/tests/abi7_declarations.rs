//! The native declarations name what the interface-6 registration names: the
//! three screens, the action, its place in the tools, its icon and the two
//! slots; and the host's rules accept what they declare.
#![allow(clippy::unwrap_used, reason = "test assertions")]
#[path = "../declarations.rs"]
#[allow(dead_code, reason = "tokens the plugin uses later")]
mod declarations;
#[path = "../../test-support/declared.rs"]
mod declared;
use paperpaw_plugin_transfer::Transfer;
use paperpaw_sdk::native::declare::Declarations;
use paperpaw_sdk::{Contribution, Id, Plugin, Registrar};

fn legacy() -> Vec<Contribution> {
    let mut r = Registrar::new(Id::new_static("transfer"));
    Transfer.register(&mut r).unwrap();
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
/// interface-6 registration says: the screens, the action, the menu entry,
/// the icon and the two slots.
#[test]
fn the_decoded_batches_say_what_the_registration_says() {
    let mut all = Vec::new();
    all.extend(batches(|d| declarations::screens(d).unwrap()));
    all.extend(batches(|d| declarations::navigation(d).unwrap()));
    let native = declared::native("transfer", &all);
    let old = declared::legacy("transfer", "", legacy());
    assert_eq!(native.screens, old.screens);
    assert_eq!(native.screens.len(), 3);
    assert_eq!(native.actions, old.actions);
    assert_eq!(native.actions, ["open|transfer.title"]);
    assert_eq!(native.menu_entries, old.menu_entries);
    assert_eq!(native.menu_entries, ["core.tools|transfer.open|46"]);
    assert_eq!(native.icons, old.icons);
    assert_eq!(native.icons, ["transfer.open|transfer"]);
    let mut slots = native.slots.clone();
    slots.sort();
    let mut want = old.slots.clone();
    want.sort();
    assert_eq!(slots, want);
    assert_eq!(
        slots,
        [
            "core.cable|transfer.cable|0",
            "core.remote-keyboard|transfer.phone|0"
        ]
    );
    assert!(native.groups.is_empty() && native.specs.is_empty());
    assert!(native.others.is_empty(), "{:?}", native.others);
}

/// The transfer and the cable open one at a time, as before; the phone's
/// keyboard screen is kept per parameters.
#[test]
fn the_instances_are_those_of_the_registration() {
    let mut instances = Vec::new();
    for contribution in legacy() {
        if let Contribution::FixedScreens(list) = contribution {
            for s in list.iter() {
                instances.push((
                    s.id().as_str().to_string(),
                    s.how_many() == paperpaw_sdk::Instances::SINGLE,
                ));
            }
        }
    }
    instances.sort();
    assert_eq!(
        instances,
        [
            ("transfer.cable".to_string(), true),
            ("transfer.phone".to_string(), false),
            ("transfer.screen".to_string(), true),
        ]
    );
}
