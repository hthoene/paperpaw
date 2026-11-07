//! The native declarations name what the interface-6 registration names:
//! screens (ids, titles), the advanced switch and the slots it fills. The
//! passwords-and-keys screens wait for the secrets service (see
//! `acceptance_pending.rs`).
#![allow(clippy::unwrap_used, reason = "test assertions")]
#[path = "../declarations.rs"]
#[allow(
    dead_code,
    reason = "the build script also uses the event declarations"
)]
mod declarations;
use paperpaw_plugin_settings::SettingsPlugin;
use paperpaw_sdk::native::declare::Declarations;
use paperpaw_sdk::{Contribution, Id, Plugin, Registrar};

fn legacy() -> Vec<Contribution> {
    let mut r = Registrar::new(Id::new_static("settings"));
    SettingsPlugin.register(&mut r).unwrap();
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

#[test]
fn screen_ids_and_titles_match_the_interface_6_registration() {
    let mut expected = Vec::new();
    for contribution in legacy() {
        if let Contribution::FixedScreens(screens) = contribution {
            for screen in screens {
                let name = screen.id().as_str().strip_prefix("settings.").unwrap();
                // Passwords and keys wait for the secrets service.
                if name.starts_with("secret") {
                    continue;
                }
                expected.push((name.to_string(), screen.title().to_string()));
            }
        }
    }
    // Declared in the order of the native screens' tokens.
    let native = [
        ("list", "settings.title", declarations::LIST),
        ("group", "settings.title", declarations::GROUP),
        ("themes", "settings.theme", declarations::THEMES),
        ("choose", "settings.title", declarations::CHOOSE),
        ("search", "settings.search", declarations::SEARCH),
    ];
    assert_eq!(expected.len(), native.len());
    for (name, title, _) in native {
        assert!(
            expected.contains(&(name.to_string(), title.to_string())),
            "{name}: {title}"
        );
    }
    let actual = batches(|d| declarations::screens(d).unwrap());
    assert_eq!(actual.len(), 1, "one batch");
}

#[test]
fn the_advanced_switch_and_the_slots_match() {
    let mut found = (false, Vec::new());
    for contribution in legacy() {
        match contribution {
            Contribution::FixedSettings(settings) => {
                for spec in settings {
                    assert_eq!(spec.id.as_str(), "settings.advanced");
                    assert_eq!(spec.title, "settings.advanced");
                    assert_eq!(spec.group.as_str(), "core.group.system");
                    assert_eq!(spec.description.as_deref(), Some("settings.advanced.help"));
                    found.0 = true;
                }
            }
            Contribution::SlotBinding { slot, screen, .. } => {
                found.1.push((slot.to_string(), screen.to_string()));
            }
            _ => {}
        }
    }
    assert!(found.0, "the advanced switch is declared");
    found.1.sort();
    assert_eq!(
        found.1,
        [
            (
                "core.choose-value".to_string(),
                "settings.choose".to_string()
            ),
            ("core.settings".to_string(), "settings.list".to_string()),
        ]
    );
    // The native declarations encode without a refusal.
    assert_eq!(batches(|d| declarations::settings(d).unwrap()).len(), 1);
    assert_eq!(batches(|d| declarations::navigation(d).unwrap()).len(), 1);
    assert_eq!(batches(|d| declarations::events(d).unwrap()).len(), 1);
}

#[test]
fn the_plugin_and_the_declarations_share_their_tokens() {
    use paperpaw_plugin_settings::native_plugin as plugin;
    assert_eq!(
        [
            plugin::LIST,
            plugin::GROUP,
            plugin::CHOOSE,
            plugin::THEMES,
            plugin::SEARCH,
            plugin::EVENTS
        ],
        [
            declarations::LIST,
            declarations::GROUP,
            declarations::CHOOSE,
            declarations::THEMES,
            declarations::SEARCH,
            declarations::EVENTS
        ]
    );
}
