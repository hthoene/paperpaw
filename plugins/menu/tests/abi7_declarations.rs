//! The native declarations: the menu style setting (id, title, help, group,
//! choices), the list and panel screens and the slots they fill, the events
//! the menu follows and the panel's help hints. The interface-6 registration
//! these were compared with is gone (`PORTING-7.md`).
#![allow(clippy::unwrap_used, reason = "test assertions")]
#[path = "../declarations.rs"]
mod declarations;
use paperpaw_sdk::native::declare::Declarations;

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

/// The text of every batch, for what the declarations name.
fn text(batches: &[Vec<u8>]) -> String {
    batches
        .iter()
        .flat_map(|b| {
            b.iter().map(|byte| {
                if byte.is_ascii_graphic() {
                    *byte as char
                } else {
                    ' '
                }
            })
        })
        .collect()
}

#[test]
fn the_style_setting_names_its_title_help_group_and_choices() {
    let batches = batches(|d| declarations::settings(d).unwrap());
    assert_eq!(batches.len(), 1);
    let text = text(&batches);
    for name in [
        "menu.setting.style",
        "menu.setting.style.help",
        "core.group.look",
        "menu.style.list",
        "menu.style.tiles",
    ] {
        assert!(text.contains(name), "{name}");
    }
}

#[test]
fn the_screens_slots_and_events_are_declared() {
    let screens = batches(|d| declarations::screens(d).unwrap());
    assert_eq!(screens.len(), 1);
    let text_of_screens = text(&screens);
    assert!(text_of_screens.contains("menu.title"));
    let navigation = batches(|d| declarations::navigation(d).unwrap());
    assert_eq!(navigation.len(), 1);
    let slots = text(&navigation);
    for name in ["core.menu", "menu.list", "core.panel", "menu.panel"] {
        assert!(slots.contains(name), "{name}");
    }
    assert_eq!(batches(|d| declarations::events(d).unwrap()).len(), 1);
}

#[test]
fn the_plugin_and_the_declarations_share_their_tokens() {
    use paperpaw_plugin_menu::native_plugin as plugin;
    assert_eq!(
        [plugin::LIST, plugin::PANEL, plugin::EVENTS],
        [
            declarations::LIST,
            declarations::PANEL,
            declarations::EVENTS
        ]
    );
}

#[test]
fn the_panels_help_hints_are_declared_and_worded_in_both_languages() {
    assert_eq!(batches(|d| declarations::help(d).unwrap()).len(), 1);
    for text in [
        include_str!("../i18n/en.ini"),
        include_str!("../i18n/de.ini"),
    ] {
        for key in [
            "menu.help.panel",
            "menu.help.panel-tap",
            "menu.help.panel-close",
            "menu.help.panel-buttons",
        ] {
            assert!(
                text.lines().any(|l| l.starts_with(&format!("{key} ="))),
                "{key}"
            );
        }
    }
}
