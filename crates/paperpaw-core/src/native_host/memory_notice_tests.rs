//! The notice for a lack of memory (`memory_notice`, #248): made at start,
//! shown without allocating when a native screen cannot open for a lack,
//! and kept for the next time once it ends.
use super::native_screen_install::kernel;
use super::*;
use paperpaw_sdk::{Id, Params};

fn live() -> usize {
    HEAP_LIVE.with(Cell::get)
}

/// The heap's most since now.
fn peak_from_now() -> usize {
    let now = live();
    HEAP_PEAK.with(|peak| peak.set(now));
    now
}

/// Opens the test plugin's screen with its first allocation refused.
fn open_without_memory(kernel: &mut crate::Kernel) {
    arm_failure(1);
    kernel.open(Id::new_static("factory.screen"), None, &Params::new());
    arm_failure(0);
}

#[test]
fn a_screen_that_cannot_open_for_a_lack_of_memory_shows_the_notice_made_ahead() {
    let mut kernel = kernel(2 << 6);
    kernel.start(paperpaw_platform::Instant(0));
    assert!(
        kernel.waiting_notices.memory_notice_ready(),
        "made at start"
    );
    let stack = kernel.stack.len();
    open_without_memory(&mut kernel);
    assert_eq!(kernel.stack.len(), stack, "nothing opened");
    assert!(kernel.notice.is_some(), "the user is told");
    assert!(!kernel.waiting_notices.memory_notice_ready(), "it shows");
    // It ends, and is there for the next time.
    kernel.end_notice();
    assert!(kernel.notice.is_none());
    assert!(kernel.waiting_notices.memory_notice_ready());
    open_without_memory(&mut kernel);
    assert!(kernel.notice.is_some(), "told again");
}

#[test]
fn showing_and_drawing_it_and_its_end_allocate_nothing() {
    let mut kernel = kernel(0);
    kernel.start(paperpaw_platform::Instant(0));
    kernel.render(&mut |_, _| {});
    let before = peak_from_now();
    kernel.show_memory_notice();
    assert!(kernel.notice.is_some());
    assert_eq!(
        HEAP_PEAK.with(Cell::get),
        before,
        "shown without an allocation"
    );
    kernel.end_notice();
    assert_eq!(
        HEAP_PEAK.with(Cell::get),
        before,
        "and taken back without one"
    );
    assert!(kernel.waiting_notices.memory_notice_ready());
    kernel.show_memory_notice();
    kernel.render(&mut |_, _| {});
    assert_eq!(HEAP_PEAK.with(Cell::get), before, "drawn without one");
}

#[test]
fn it_holds_little_while_it_waits() {
    let mut kernel = kernel(0);
    kernel.start(paperpaw_platform::Instant(0));
    kernel.waiting_notices.keep_memory_notice(None);
    let before = live();
    kernel.reserve_memory_notice();
    let held = live() - before;
    std::eprintln!("memory notice held: {held} B");
    assert!(held <= 512, "{held} B");
}

#[test]
fn a_new_language_makes_it_again_and_one_showing_then_is_not_kept() {
    let mut kernel = kernel(0);
    kernel.start(paperpaw_platform::Instant(0));
    kernel.show_memory_notice();
    kernel
        .shared
        .settings
        .set(
            crate::core_settings::LANGUAGE.as_str(),
            paperpaw_sdk::SettingValue::Text("de".into()),
        )
        .unwrap();
    kernel.setting_changed(&crate::core_settings::LANGUAGE.into());
    assert!(kernel.waiting_notices.memory_notice_ready(), "made anew");
    kernel.end_notice();
    assert!(kernel.waiting_notices.memory_notice_ready());
    // Before start there is none to make again.
    let mut fresh = super::native_screen_install::kernel(0);
    fresh.setting_changed(&crate::core_settings::LANGUAGE.into());
    assert!(!fresh.waiting_notices.has_memory_notice());
}
