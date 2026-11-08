//! The native declarations name what the interface-6 registration names for
//! what is ported: the menu style setting (id, title, help, group, choices),
//! the list screen and the slot it fills. The panel and the notice are not
//! declared yet (see `PORTING-7.md`).
#![allow(clippy::unwrap_used, reason = "test assertions")]
#[path = "../declarations.rs"]
mod declarations;
use paperpaw_plugin_menu::Menu;
use paperpaw_sdk::native::declare::Declarations;
use paperpaw_sdk::{Contribution, Id, Plugin, Registrar, SettingKind};

fn legacy() -> Vec<Contribution> {
    let mut r = Registrar::new(Id::new_static("menu"));
    Menu.register(&mut r).unwrap();
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
fn the_style_setting_matches_the_interface_6_registration() {
    let mut found = Vec::new();
    for contribution in legacy() {
        if let Contribution::FixedSettings(settings) = contribution {
            for spec in settings.iter() {
                let SettingKind::Choice(options) = &spec.kind else {
                    panic!("the style is a choice");
                };
                let default = format!("{:?}", spec.default);
                found.push((
                    spec.id.as_str().to_string(),
                    spec.title.to_string(),
                    spec.description.as_deref().map(str::to_string),
                    spec.group.as_str().to_string(),
                    options.iter().map(ToString::to_string).collect::<Vec<_>>(),
                    default,
                ));
            }
        }
    }
    assert_eq!(
        found,
        [(
            "menu.style".to_string(),
            "menu.setting.style".to_string(),
            Some("menu.setting.style.help".to_string()),
            "core.group.look".to_string(),
            vec![
                "menu.style.list".to_string(),
                "menu.style.tiles".to_string()
            ],
            "Text(\"menu.style.list\")".to_string(),
        )]
    );
    assert_eq!(batches(|d| declarations::settings(d).unwrap()).len(), 1);
}

#[test]
fn the_list_screen_and_its_slot_match() {
    let mut screens = Vec::new();
    let mut slots = Vec::new();
    for contribution in legacy() {
        match contribution {
            Contribution::FixedScreens(list) => {
                for s in list.iter() {
                    screens.push((s.id().as_str().to_string(), s.title().to_string()));
                }
            }
            Contribution::SlotBinding { slot, screen, .. } => {
                slots.push((slot.to_string(), screen.to_string()));
            }
            _ => {}
        }
    }
    assert!(screens.contains(&("menu.list".to_string(), "menu.title".to_string())));
    assert!(slots.contains(&("core.menu".to_string(), "menu.list".to_string())));
    assert_eq!(batches(|d| declarations::screens(d).unwrap()).len(), 1);
    assert_eq!(batches(|d| declarations::navigation(d).unwrap()).len(), 1);
    assert_eq!(batches(|d| declarations::events(d).unwrap()).len(), 1);
}

#[test]
fn the_plugin_and_the_declarations_share_their_tokens() {
    use paperpaw_plugin_menu::native_plugin as plugin;
    assert_eq!(
        [plugin::LIST, plugin::EVENTS],
        [declarations::LIST, declarations::EVENTS]
    );
}
