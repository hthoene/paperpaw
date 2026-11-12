//! `event.screen_hidden` (111): sent to a native screen a full screen covers,
//! after its `screen_opened` and before the cover's own.
use super::*;

fn events() -> Vec<(u32, u32, u32, u32)> {
    EVENTS.with(|events| events.borrow().clone())
}

#[test]
fn a_full_screen_opened_above_a_native_screen_tells_it_before_the_cover_opens() {
    let mut kernel = kernel(0);
    let depth = device_depth(&kernel);
    kernel
        .register_native(host_named("base", 0), &depth, |_, _, _, _| {})
        .unwrap();
    kernel.open_screen(
        paperpaw_platform::Instant(1),
        Id::new_static("base.screen"),
        Params::new(),
    );
    let base = kernel.stack.last().unwrap().serial;
    assert!(events().iter().any(|e| e.0 == 79 && e.1 == base));
    assert!(
        !events().iter().any(|e| e.0 == 111),
        "nothing covers it yet"
    );
    open(&mut kernel, Params::new());
    let cover = kernel.stack.last().unwrap().serial;
    let all = events();
    let told = all.iter().position(|e| *e == (111, base, 0, 0));
    let opened = all.iter().position(|e| e.0 == 79 && e.1 == cover);
    assert!(told.is_some(), "told: {all:?}");
    assert!(told < opened, "before the cover opens: {all:?}");
    assert_eq!(all.iter().filter(|e| e.0 == 111).count(), 1, "once");
    // Closing the cover shows the screen again: no second `screen_hidden`.
    kernel.apply(Command::Close(paperpaw_sdk::ScreenKey::new(cover)));
    assert_eq!(events().iter().filter(|e| e.0 == 111).count(), 1);
}
