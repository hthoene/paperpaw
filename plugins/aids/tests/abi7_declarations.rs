//! The native declarations name what the interface-6 registration names: the
//! three settings with their kinds, defaults, choices, flags, titles and help.
#![allow(clippy::unwrap_used, reason = "test assertions")]
#[path = "../declarations.rs"]
mod declarations;
use paperpaw_core::native_host::registrations::{Record, records_decode};
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
    // The native batch, decoded, names the same settings: name, group, title,
    // help, the default (a 12-byte header, then the payload), the choices and
    // the flags (bit 2 kept on reset, bit 4 advanced).
    let mut batches = Vec::new();
    {
        let mut d = Declarations::new(|b: &[u8]| {
            batches.push(b.to_vec());
            Ok(())
        });
        declarations::settings(&mut d).unwrap();
    }
    assert_eq!(batches.len(), 1);
    let records = records_decode(&batches[0]).unwrap();
    let specs: Vec<_> = records
        .iter()
        .map(|r| match r {
            Record::SettingSpec(s) => s,
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(specs.len(), 3);
    let kind_and_payload = |s: &paperpaw_core::native_host::registrations::SettingSpec<'_>| {
        let kind = u32::from_le_bytes(s.default[4..8].try_into().unwrap());
        (kind, s.default[12..].to_vec())
    };
    for (spec, name) in specs.iter().zip(["bold-beginnings", "replace", "pictures"]) {
        assert_eq!(spec.name, name);
        assert_eq!(spec.group, "core.group.reading-aids");
        assert_eq!(spec.title, format!("aids.setting.{name}"));
        assert_eq!(spec.help, format!("aids.setting.{name}.help"));
        assert_eq!((spec.min, spec.max), (0, 0));
    }
    // Bold word beginnings: a switch, off, basic.
    assert_eq!((specs[0].kind, specs[0].flags), (0, 0));
    assert_eq!(kind_and_payload(&specs[0]), (1, vec![0]));
    assert_eq!(specs[0].choices, "");
    // Replacements: text, empty, advanced and kept on a reset.
    assert_eq!((specs[1].kind, specs[1].flags), (3, 1 << 2 | 1 << 4));
    assert_eq!(kind_and_payload(&specs[1]), (3, Vec::new()));
    // Pictures: a choice, `show`, basic, with the three values in order.
    assert_eq!((specs[2].kind, specs[2].flags), (2, 0));
    assert_eq!(
        kind_and_payload(&specs[2]),
        (3, b"aids.pictures.show".to_vec())
    );
    assert_eq!(specs[2].choices, pictures::CHOICES.join("\n"));
}
