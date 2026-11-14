//! The native declarations are those pinned in `tests/pinned/declarations.txt`
//! (written while the interface-6 registration was still compared with them,
//! line for line): the group and its 24 settings (kinds, defaults, levels, the
//! editor of a page), the screens, the slot, the actions with their icons and
//! menu places, the swipe up, the pages as a place of widgets, the need and
//! the four events. The built-in widgets are pinned by `widgets_declarations.rs`.
#![allow(clippy::unwrap_used, reason = "test assertions")]
#[path = "../declarations.rs"]
#[allow(dead_code, reason = "tokens the plugin uses later")]
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
    for batch in [
        batches(|d| declarations::settings(d).unwrap()),
        batches(|d| declarations::screens(d).unwrap()),
        batches(|d| declarations::navigation(d).unwrap()),
        batches(|d| declarations::events(d).unwrap()),
    ] {
        all.extend(batch);
    }
    let native = declared::native("home", &all);
    declared::assert_pinned("declarations", &native);
    assert_eq!(native.specs.len(), 24);
    assert_eq!(native.screens.len(), 4, "{:?}", native.screens);
    assert_eq!(native.slots, ["core.home|home.dashboard|0"]);
    assert_eq!(native.widget_places.len(), 1);
    assert_eq!(native.keys, ["core.home|swipe.up|home.all-tools"]);
    assert_eq!(native.menu_entries.len(), 2);
    assert_eq!(native.needs, ["home.focus-at-once|NoTouch"]);
    assert_eq!(native.actions.len(), 2);
    assert_eq!(native.events.len(), 4, "{:?}", native.events);
    for token in [
        declarations::SETTING_CHANGED,
        declarations::TICK,
        declarations::WIDGET_CHANGED,
        declarations::STACK_CHANGED,
    ] {
        assert!(
            native
                .events
                .iter()
                .any(|e| e.ends_with(&format!(":{token}"))),
            "{token} in {:?}",
            native.events
        );
    }
    assert!(native.others.is_empty(), "{:?}", native.others);
    assert_eq!(
        declarations::DEFAULTS[0],
        "home.clock, home.continue, home.recent"
    );
}

#[test]
fn the_dashboards_help_hints_are_declared_and_worded_in_both_languages() {
    // The nine hints of interface 6 (five for touch, four for buttons), in batches
    // the host admits.
    let batches = batches(|d| declarations::help(d).unwrap());
    assert!(!batches.is_empty());
    for key in [
        "quick", "back", "home", "tools", "arrange", "move", "select", "menu", "panel",
    ] {
        let key = format!("home.tips.{key}");
        assert!(
            batches
                .iter()
                .any(|b| b.windows(key.len()).any(|w| w == key.as_bytes())),
            "{key} declared"
        );
    }
    for text in [
        include_str!("../i18n/en.ini"),
        include_str!("../i18n/de.ini"),
    ] {
        for key in [
            "quick", "back", "home", "tools", "arrange", "move", "select", "menu", "panel",
        ] {
            let key = format!("home.tips.{key} =");
            assert!(text.lines().any(|l| l.starts_with(&key)), "{key}");
        }
    }
}
