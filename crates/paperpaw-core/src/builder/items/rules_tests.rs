//! The rules of what registers ([`Staging`]): ids only in the plugin's
//! namespace and once per kind (also for settings and presets kept in the
//! program), every kind staged in its list, and typed event handlers. Until
//! #300 these were integration tests over the old contribution list.

use alloc::borrow::Cow;
use alloc::format;

use super::Staging;
use crate::kit::action::ActionSpec;
use paperpaw_sdk::builtin::groups;
use paperpaw_sdk::presets::Preset;
use paperpaw_sdk::{
    Id, Params, Priority, RegisterError, SettingKind, SettingSpec, SettingValue, SlotSpec,
};

fn setting(id: &'static str) -> SettingSpec {
    SettingSpec::new(
        Id::new_static(id),
        "",
        SettingKind::Bool,
        SettingValue::Bool(false),
        paperpaw_sdk::builtin::groups::OTHER,
    )
}

/// A service of a count, for the tests.
#[derive(Debug, PartialEq)]
struct Count(u32);
paperpaw_sdk::service_type!(Count, "demo.count/1");

/// The version the test services are in.
const ONE: paperpaw_sdk::Version = paperpaw_sdk::Version::new(1, 0, 0);

fn registrar() -> Staging {
    Staging::new(Id::new_static("demo"))
}

#[test]
fn settings_kept_in_the_program_are_checked_like_others() {
    static OWN: [SettingSpec; 2] = [
        SettingSpec::switch("demo.a", "", false, groups::OTHER),
        SettingSpec::switch("demo.b", "", false, groups::OTHER),
    ];
    static FOREIGN: [SettingSpec; 1] = [SettingSpec::switch("other.a", "", false, groups::OTHER)];
    static TWICE: [SettingSpec; 2] = [
        SettingSpec::switch("demo.c", "", false, groups::OTHER),
        SettingSpec::switch("demo.c", "", true, groups::OTHER),
    ];
    static AGAIN: [SettingSpec; 1] = [SettingSpec::switch("demo.a", "", false, groups::OTHER)];
    static BUILT: [SettingSpec; 1] = [SettingSpec::switch("demo.built", "", false, groups::OTHER)];
    let mut r = registrar();
    r.add_settings(&OWN).unwrap();
    assert_eq!(
        r.add_settings(&FOREIGN),
        Err(RegisterError::Namespace {
            id: Id::new_static("other.a")
        })
    );
    assert!(matches!(
        r.add_settings(&TWICE),
        Err(RegisterError::Duplicate { .. })
    ));
    assert!(matches!(
        r.add_settings(&AGAIN),
        Err(RegisterError::Duplicate { .. })
    ));
    // One built at start with the id of one kept in the program, and the
    // other way round.
    assert!(matches!(
        r.add_setting(setting("demo.b")),
        Err(RegisterError::Duplicate { .. })
    ));
    r.add_setting(setting("demo.built")).unwrap();
    assert!(matches!(
        r.add_settings(&BUILT),
        Err(RegisterError::Duplicate { .. })
    ));
    assert!(matches!(
        r.add_setting(setting("other.x")),
        Err(RegisterError::Namespace { .. })
    ));
    // Refused lists are not added, not even in part.
    assert_eq!(r.settings.len(), 2);
}

#[test]
fn presets_kept_in_the_program_are_checked_like_others() {
    static OWN: [Preset; 1] = [Preset::fixed("demo.calm", "home.layout", "", "", "")];
    static FOREIGN: [Preset; 1] = [Preset::fixed("home.calm", "home.layout", "", "", "")];
    static TWICE: [Preset; 2] = [
        Preset::fixed("demo.x", "home.layout", "", "", ""),
        Preset::fixed("demo.x", "home.layout", "", "", ""),
    ];
    let mut r = registrar();
    r.add_presets(&OWN).unwrap();
    assert!(matches!(
        r.add_presets(&FOREIGN),
        Err(RegisterError::Namespace { .. })
    ));
    assert!(matches!(
        r.add_presets(&TWICE),
        Err(RegisterError::Duplicate { .. })
    ));
    let made = |id: &'static str| {
        Preset::new(
            Id::new_static(id),
            Id::new_static("home.layout"),
            "",
            "",
            "",
        )
    };
    assert!(matches!(
        r.add_preset(made("demo.calm")),
        Err(RegisterError::Duplicate { .. })
    ));
    r.add_preset(made("demo.other")).unwrap();
    assert!(matches!(r.add_presets(&[]), Ok(())));
    static LATER: [Preset; 1] = [Preset::fixed("demo.other", "home.layout", "", "", "")];
    assert!(matches!(
        r.add_presets(&LATER),
        Err(RegisterError::Duplicate { .. })
    ));
    // A preset may be of another plugin's kind.
    assert_eq!(r.fixed_presets.len() + r.presets.len(), 3);
}

#[test]
fn every_kind_is_staged_in_its_list() {
    let mut r = registrar();
    r.add_slot(SlotSpec::new(Id::new_static("demo.slot"), ""))
        .unwrap();
    r.bind_slot(
        Id::new_static("core.reader"),
        Id::new_static("demo.screen"),
        Priority::PREFERRED,
    )
    .unwrap();
    r.add_keymap(Cow::Borrowed("")).unwrap();
    r.add_keymap_preset(Id::new_static("demo.keys"), Cow::Borrowed(""))
        .unwrap();
    r.add_stylesheet(Cow::Borrowed("p {}")).unwrap();
    r.add_book_data(".paperpaw/cache", "demo-").unwrap();
    r.bind_font(
        Id::new_static("core.ui"),
        Id::new_static("x.font"),
        Priority::FALLBACK,
    )
    .unwrap();
    r.set_setting_default(Id::new_static("core.font-size"), Cow::Borrowed("12"))
        .unwrap();
    r.contribute(Id::new_static("other.point"), Cow::Borrowed("text"))
        .unwrap();
    r.add_translations(Cow::Borrowed("")).unwrap();
    r.provide(Id::new_static("demo.count"), Count(1), ONE)
        .unwrap();
    // Each in the list of its kind.
    let staged = [
        r.slots.len(),
        r.candidates.len(),
        r.keymaps.len(),
        r.keymap_presets.len(),
        r.stylesheets.len(),
        r.book_data.len(),
        r.font_roles.len(),
        r.setting_defaults.len(),
        r.contributions.len(),
        r.catalogs.len(),
        r.services.len(),
    ];
    assert_eq!(staged, [1; 11]);
    assert!(format!("{r:?}").contains("demo"));
    // A service of another namespace is refused.
    assert!(matches!(
        r.provide(Id::new_static("other.count"), Count(1), ONE),
        Err(RegisterError::Namespace { .. })
    ));
    assert_eq!(r.plugin(), Some(&Id::new_static("demo")));
}

#[test]
fn typed_handlers_hear_only_what_they_can_read() {
    paperpaw_sdk::event_data! {
        pub struct Count {
            pub n: u32,
        }
    }
    // What a handler reads of an event (`hear::data`): what it carries as
    // its type, nothing where that is missing or unreadable.
    let read = |params: Params| crate::hear::data::<Count>(&params).map(|c| c.n);
    assert_eq!(read(Params::new().with("n", "5")), Some(5));
    assert_eq!(read(Params::new().with("n", "five")), None);
    assert_eq!(read(Params::new()), None);
}

#[test]
fn actions_of_another_namespace_or_twice_are_refused() {
    let mut r = registrar();
    r.add_action(ActionSpec::new(Id::new_static("demo.go"), "x"))
        .unwrap();
    // A placed id is in another namespace.
    assert!(matches!(
        r.add_action(ActionSpec::new(Id::parse("demo@2.go").unwrap(), "x")),
        Err(RegisterError::Namespace { .. })
    ));
    // The plugin's own id is its namespace.
    r.add_action(ActionSpec::new(Id::new_static("demo"), "x"))
        .unwrap();
    assert!(matches!(
        r.add_action(ActionSpec::new(Id::new_static("demo"), "x")),
        Err(RegisterError::Duplicate { .. })
    ));
}
