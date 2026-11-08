//! The native settings are the interface-6 settings, field by field: the
//! name, the title, the group, the help text, the kind, the default, and the
//! flags (advanced, kept on reset). The editor that opens the account screen
//! is the one difference, named below.
#![allow(clippy::unwrap_used, reason = "fixtures")]
use crate::{SETTINGS, declarations};
use alloc::{string::String, vec::Vec};
use paperpaw_core::native_host::registrations::{Record, records_decode};
use paperpaw_sdk::{
    SettingKind, SettingValue,
    native::declare::{Declarations, Kind, SpecDecl},
};

/// The batches `declarations::settings` submits.
fn batches() -> Vec<Vec<u8>> {
    let mut out: Vec<Vec<u8>> = Vec::new();
    {
        let mut d = Declarations::new(|batch: &[u8]| {
            out.push(batch.to_vec());
            Ok(())
        });
        declarations::settings(&mut d).unwrap();
    }
    out
}

/// The default bytes the host would hold for `kind`.
fn default_of(kind: Kind<'_>) -> Vec<u8> {
    let mut out: Vec<Vec<u8>> = Vec::new();
    {
        let mut d = Declarations::new(|batch: &[u8]| {
            out.push(batch.to_vec());
            Ok(())
        });
        d.spec(SpecDecl::new("x", kind, "g", "t")).unwrap();
        d.submit().unwrap();
    }
    let records = records_decode(&out[0]).unwrap();
    match records.iter().next().unwrap() {
        Record::SettingSpec(spec) => spec.default.to_vec(),
        other => panic!("{other:?}"),
    }
}

#[test]
fn every_interface_6_setting_is_declared_with_the_same_fields() {
    let mut specs = Vec::new();
    let mut extras = 0;
    let mut groups = Vec::new();
    for batch in batches() {
        let records = records_decode(&batch).unwrap();
        for record in records.iter() {
            match record {
                Record::SettingSpec(spec) => specs.push((
                    String::from(spec.name),
                    String::from(spec.group),
                    String::from(spec.title),
                    String::from(spec.help),
                    spec.kind,
                    spec.flags,
                    spec.default.to_vec(),
                )),
                Record::SettingExtra(_) => extras += 1,
                Record::SettingGroup(group) => groups.push((
                    String::from(group.name),
                    String::from(group.title),
                    group.order,
                )),
                other => panic!("{other:?}"),
            }
        }
    }
    assert_eq!(specs.len(), SETTINGS.len());
    for old in &SETTINGS {
        let name = old.id.as_str().strip_prefix("kosync.").unwrap();
        let (_, group, title, help, kind, flags, default) = specs
            .iter()
            .find(|(n, ..)| n == name)
            .unwrap_or_else(|| panic!("{name} is not declared"));
        assert_eq!(group.as_str(), old.group.as_str(), "{name}");
        assert_eq!(title.as_str(), &*old.title, "{name}");
        assert_eq!(Some(help.as_str()), old.description.as_deref(), "{name}");
        let want = match (&old.kind, &old.default) {
            (SettingKind::Text, SettingValue::Text(text)) => (3, default_of(Kind::Text(text))),
            (SettingKind::Bool, SettingValue::Bool(on)) => (0, default_of(Kind::Bool(*on))),
            other => panic!("{name}: {other:?}"),
        };
        assert_eq!((*kind, default.clone()), want, "{name}");
        // Bit 2 kept on reset, bit 4 advanced; nothing else is set.
        let advanced = old.level == paperpaw_sdk::Level::Advanced;
        let expected = u32::from(old.is_kept_on_reset()) << 2 | u32::from(advanced) << 4;
        assert_eq!(*flags, expected, "{name}");
        assert!(!old.scoped && !old.guarded, "{name}");
    }
    // The group: the same title key and order as interface 6's.
    assert_eq!(
        groups,
        [(String::from("group"), String::from("kosync.group"), 28)]
    );
    // The one difference: interface 6 opens the account screen from the
    // user and the server settings; the native plugin has no such screen yet
    // (`PORTING-7.md`), so it declares no editor.
    assert_eq!(extras, 0, "{extras} extras");
}
