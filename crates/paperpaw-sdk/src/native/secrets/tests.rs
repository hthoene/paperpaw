use super::*;
use alloc::{format, string::String};

#[test]
fn names_and_labels_preserve_ssid_controls_and_exact_utf8_byte_caps() {
    for name in ["token", " réseau\t\n\r\\ ", "名前", " secret "] {
        let key = SecretName::new(name).unwrap();
        assert_eq!((key.plugin(), key.name()), ("", name));
        let write = SecretWrite::new(key, "").unwrap().label(name).unwrap();
        assert_eq!((write.display_label(), write.value()), (name, ""));
        assert!(!write.is_changeable());
    }
    let exact = "é".repeat(32);
    assert!(SecretName::new(&exact).is_ok());
    assert!(
        SecretWrite::new(SecretName::new("key").unwrap(), "")
            .unwrap()
            .label(&exact)
            .is_ok()
    );
    let too_long = exact + "x";
    assert!(matches!(SecretName::new(&too_long), Err(Error::NoSpace)));
    assert!(matches!(
        SecretWrite::new(SecretName::new("key").unwrap(), "")
            .unwrap()
            .label(&too_long),
        Err(Error::NoSpace)
    ));
    assert!(matches!(SecretName::new(""), Err(Error::Invalid)));
    for c in (0..=0x9f)
        .filter_map(char::from_u32)
        .filter(|c| c.is_control())
    {
        if matches!(c, '\t' | '\n' | '\r') {
            continue;
        }
        let text = String::from(c);
        assert!(matches!(SecretName::new(&text), Err(Error::Invalid)));
        assert!(matches!(
            SecretWrite::new(SecretName::new("key").unwrap(), "")
                .unwrap()
                .label(&text),
            Err(Error::Invalid)
        ));
    }
}
#[test]
fn editor_targets_are_explicit_and_values_are_complete_and_redacted() {
    let name = SecretName::new("password").unwrap().owner("wifi").unwrap();
    assert_eq!((name.plugin(), name.name()), ("wifi", "password"));
    assert_eq!(name.owner("").unwrap().plugin(), "");
    for owner in ["two.parts", "nul\0name", "del\u{7f}"] {
        assert!(matches!(name.owner(owner), Err(Error::Invalid)));
    }
    assert!(matches!(name.owner(&"p".repeat(65)), Err(Error::NoSpace)));
    let value = "界".repeat(341) + "x";
    let write = SecretWrite::new(name, &value)
        .unwrap()
        .label("Network")
        .unwrap()
        .changeable();
    assert_eq!(write.value().len(), 1024);
    assert_eq!(write.value(), value);
    assert_eq!(write.target().plugin(), "wifi");
    assert!(write.is_changeable());
    assert!(!format!("{write:?}").contains(&value));
    assert!(format!("{write:?}").contains("[redacted]"));
    assert!(matches!(
        SecretWrite::new(name, &(value + "x")),
        Err(Error::NoSpace)
    ));
    let raw = wire::ServiceSecValue {
        flags: 1,
        label: "Network",
        value: "HIDDEN-VALUE",
    };
    let value = SecretValue(raw);
    assert_eq!(value.value(), "HIDDEN-VALUE");
    assert_eq!(value.label(), "Network");
    assert!(value.changeable());
    assert!(!format!("{value:?}").contains("HIDDEN-VALUE"));
}
#[test]
fn list_scope_and_cursor_do_not_conflate_own_and_editor_access() {
    let own = SecretList::own();
    assert!(!own.includes_all());
    assert_eq!(own.cursor(), 0);
    let all = SecretList::all().from(23);
    assert!(all.includes_all());
    assert_eq!(all.cursor(), 23);
    assert_eq!(all.from(u32::MAX).cursor(), u32::MAX);
    assert!(!SecretList::default().includes_all());
}

#[test]
fn random_views_preserve_bytes_and_strength_without_debug_exposure() {
    let bytes = b"HIDDEN-RANDOM";
    for flags in [0, 1] {
        let view = RandomBytes(wire::ServiceSecBytes { flags, data: bytes });
        assert_eq!(view.bytes(), bytes);
        assert_eq!(view.strong(), flags == 1);
        let debug = format!("{view:?}");
        assert!(!debug.contains("HIDDEN-RANDOM"));
        assert!(debug.contains("[redacted]"));
    }
}
