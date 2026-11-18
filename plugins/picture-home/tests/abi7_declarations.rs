//! The native declarations are those pinned in `tests/pinned/declarations.txt`
//! (written while the interface-6 registration was still compared with them):
//! the group and its 28 settings (kinds, defaults, levels, editors, the
//! condition of the bar's alignment), the screens, the `core.home` offer, the
//! icon, the need, the pages as a place of widgets and the bar's place of
//! status items; and the host's rules accept them.
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

/// What the batches say, decoded and checked by the host's rules, is what was
/// pinned, and what the host takes.
#[test]
fn the_decoded_batches_are_the_pinned_declarations() {
    let mut all = Vec::new();
    for batch in [
        batches(|d| declarations::settings(d).unwrap()),
        batches(|d| declarations::screens(d).unwrap()),
        batches(|d| declarations::navigation(d).unwrap()),
        batches(|d| declarations::events(d).unwrap()),
    ] {
        all.extend(batch);
    }
    let native = declared::native("picture-home", &all);
    declared::assert_pinned("declarations", &native);
    assert_eq!(native.specs.len(), 28);
    assert_eq!(native.slots, ["core.home|picture-home.dashboard|100"]);
    assert_eq!(native.widget_places.len(), 1);
    assert!(native.widget_places[0].contains("picture-home.page-4|Grid"));
    assert_eq!(native.status_places.len(), 1);
    assert_eq!(native.icons, ["picture-home.group|layout-grid"]);
    assert_eq!(native.needs, ["picture-home.focus-at-once|NoTouch"]);
    assert_eq!(
        declarations::DEFAULTS[0],
        "home.clock, home.continue, home.recent"
    );
    // The minute tick (the status bar), a widget that changed
    // (`event.widget_changed`, its cell) and a prepared picture
    // (`event.image_ready`, which the host drops for a plugin that doesn't
    // subscribe), in any order, and nothing else.
    assert_eq!(native.events.len(), 3, "{:?}", native.events);
    for token in [
        declarations::TICK,
        declarations::WIDGET_CHANGED,
        declarations::IMAGE_READY,
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
}
