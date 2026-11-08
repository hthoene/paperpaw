//! The native declarations name what the interface-6 registration names: the
//! three settings with their kinds, defaults, choices, flags, titles and help.
#![allow(clippy::unwrap_used, reason = "test assertions")]
#[path = "../declarations.rs"]
mod declarations;
use paperpaw_plugin_aids::{Aids, pictures};
use paperpaw_sdk::native::declare::Declarations;
use paperpaw_sdk::{Contribution, Id, Plugin, Registrar};

#[test]
fn the_settings_match_the_interface_6_registration() {
    let mut r = Registrar::new(Id::new_static("aids"));
    Aids.register(&mut r).unwrap();
    let mut found = Vec::new();
    for contribution in r.into_contributions() {
        if let Contribution::FixedSettings(settings) = contribution {
            for spec in settings.iter() {
                found.push(format!(
                    "{} {} {:?} {} {:?} {:?} level={:?} kept={}",
                    spec.id.as_str(),
                    spec.title,
                    spec.description,
                    spec.group.as_str(),
                    spec.default,
                    spec.kind,
                    spec.level,
                    spec.is_kept_on_reset(),
                ));
            }
        }
    }
    assert_eq!(found.len(), 3);
    assert!(found[0].starts_with("aids.bold-beginnings aids.setting.bold-beginnings Some(\"aids.setting.bold-beginnings.help\") core.group.reading-aids Bool(false)"));
    assert!(found[0].ends_with("level=Basic kept=false"));
    assert!(found[1].starts_with("aids.replace aids.setting.replace Some(\"aids.setting.replace.help\") core.group.reading-aids Text(\"\")"));
    assert!(found[1].ends_with("level=Advanced kept=true"));
    assert!(found[2].starts_with("aids.pictures aids.setting.pictures Some(\"aids.setting.pictures.help\") core.group.reading-aids Text(\"aids.pictures.show\")"));
    assert!(found[2].ends_with("level=Basic kept=false"));
    assert_eq!(declarations::PICTURES, pictures::CHOICES);
    let mut batches = 0;
    {
        let mut d = Declarations::new(|_: &[u8]| {
            batches += 1;
            Ok(())
        });
        declarations::settings(&mut d).unwrap();
    }
    assert_eq!(batches, 1);
}
