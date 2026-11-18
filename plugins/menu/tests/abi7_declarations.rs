//! The native declarations are those pinned in `tests/pinned/declarations.txt`
//! (written while the interface-6 registration was still compared with them):
//! the menu style setting (id, title, help, group, choices), the list and
//! panel screens and the slots they fill. The notice is not declared (see
//! `PORTING-7.md`).
#![allow(clippy::unwrap_used, reason = "test assertions")]
#[path = "../declarations.rs"]
mod declarations;
#[path = "../../test-support/declared.rs"]
mod declared;
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

#[test]
fn the_declarations_are_the_pinned_ones() {
    let mut all = Vec::new();
    all.extend(batches(|d| declarations::settings(d).unwrap()));
    all.extend(batches(|d| declarations::screens(d).unwrap()));
    all.extend(batches(|d| declarations::navigation(d).unwrap()));
    all.extend(batches(|d| declarations::events(d).unwrap()));
    let native = declared::native("menu", &all);
    declared::assert_pinned("declarations", &native);
    assert_eq!(native.specs.len(), 1);
    assert_eq!(batches(|d| declarations::settings(d).unwrap()).len(), 1);
    assert_eq!(batches(|d| declarations::screens(d).unwrap()).len(), 1);
    assert_eq!(batches(|d| declarations::navigation(d).unwrap()).len(), 1);
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
