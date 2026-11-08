//! The native declarations name what the interface-6 registration names: the
//! group, the settings (kinds, choices, defaults, flags, titles, help, the
//! place's screen as its editor), the place's screen and its instance policy,
//! the actions and the icons; and the host's rules accept or refuse them as
//! they should. The two widgets are declared later; their groups and settings
//! are left out of the comparison (they are in groups of their own).
#![allow(clippy::unwrap_used, reason = "test assertions")]
#[path = "../declarations.rs"]
#[allow(dead_code, reason = "tokens the plugin uses later")]
mod declarations;
#[path = "../../test-support/declared.rs"]
mod declared;
use paperpaw_core::native_host::registrations::{Record, records_decode};
use paperpaw_plugin_weather::Weather;
use paperpaw_sdk::native::declare::Declarations;
use paperpaw_sdk::{Contribution, Id, Plugin, Registrar};

fn legacy() -> Vec<Contribution> {
    let mut r = Registrar::new(Id::new_static("weather"));
    Weather.register(&mut r).unwrap();
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
    let native = declared::native("weather", &all);
    let old = declared::legacy("weather", "weather.group", legacy());
    assert_eq!(native.groups, old.groups);
    assert_eq!(native.groups.len(), 1);
    assert_eq!(native.specs.len(), 5);
    assert_eq!(native.specs, old.specs);
    assert_eq!(native.extras, old.extras);
    assert_eq!(native.extras.len(), 1);
    assert!(native.extras[0].contains("place-screen"));
    assert_eq!(native.screens, old.screens);
    assert_eq!(native.actions, old.actions);
    assert_eq!(native.actions.len(), 2);
    assert_eq!(native.icons, old.icons);
    assert_eq!(native.icons.len(), 2);
    assert!(native.others.is_empty(), "{:?}", native.others);
}

/// The place's screen opens as the registration says: the decoded
/// `screen.spec` flags (bits 6 and 7: 0 per parameters, 1 single, 2 many).
#[test]
fn the_place_screen_opens_as_before() {
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
                new.push((format!("weather.{}", s.name), (s.flags >> 6) & 3));
            }
        }
    }
    assert_eq!(new, old);
    assert_eq!(new.len(), 1);
}
