//! The native declarations are those pinned in `tests/pinned/declarations.txt`
//! (written while the interface-6 registration was still compared with them,
//! line for line): the group, the nine settings (kinds, defaults, choices,
//! titles, help, editors, units; the picker as the editor of the picture and
//! the widget over the page), the screens (the picker and the sleep screen),
//! the slot, the icon, the needs and the two places of widgets, and the sleep
//! screen's hidden button hints.
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
    ] {
        all.extend(batch);
    }
    let native = declared::native("sleep", &all);
    declared::assert_pinned("declarations", &native);
    assert_eq!(native.specs.len(), 9);
    assert_eq!(native.screens.len(), 2);
    assert_eq!(native.widget_places.len(), 2);
    assert_eq!(native.needs.len(), 10);
    assert!(native.events.is_empty());
    assert!(native.others.is_empty(), "{:?}", native.others);
    assert_eq!(batches(|d| declarations::screens(d).unwrap()).len(), 1);
    assert_ne!(declarations::PICK, declarations::SCREEN);
}

/// The sleep screen shows no row of button hints, as Beta 3's had none
/// (`ScreenDecl::hints_hidden`, flag bit 8); the picker keeps its hints.
#[test]
fn the_sleep_screen_hides_the_button_hints_and_the_picker_keeps_them() {
    use paperpaw_core::native_host::registrations::{Record, records_decode};
    const HINTS_HIDDEN: u32 = 1 << 8;
    let mut flags = Vec::new();
    for batch in batches(|d| declarations::screens(d).unwrap()) {
        for record in records_decode(&batch).unwrap().iter() {
            if let Record::ScreenSpec(spec) = record {
                flags.push((spec.name.to_string(), spec.flags & HINTS_HIDDEN != 0));
            }
        }
    }
    assert_eq!(
        flags,
        [
            ("pick-overlay".to_string(), false),
            ("screen".to_string(), true)
        ]
    );
}
