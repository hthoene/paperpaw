//! The shell is told of every entry of a card plugin's code
//! (`Services::enter` and `leave`): `make`, callbacks and the destructor, and
//! may refuse all but the destructor.
use super::native_screen_install::{DESTROYED, install, kernel, open};
use super::*;
use paperpaw_sdk::{Id, Params};

fn counts() -> (u32, u32) {
    (ENTERED.with(Cell::get), LEFT.with(Cell::get))
}

#[test]
fn make_a_callback_and_the_destructor_are_announced_and_each_one_ends() {
    let mut kernel = kernel(0);
    let (entered, left) = counts();
    assert!(entered >= 1, "make was not announced");
    assert_eq!(entered, left, "an entry did not end");
    open(&mut kernel, Params::new());
    let (after_open, left_after_open) = counts();
    assert!(after_open > entered, "a callback was not announced");
    assert_eq!(after_open, left_after_open);
    kernel.unload_plugin(&Id::new_static("factory"));
    let (after_unload, left_after_unload) = counts();
    assert!(
        after_unload > after_open,
        "the destructor was not announced"
    );
    assert_eq!(after_unload, left_after_unload);
    assert_eq!(DESTROYED.with(Cell::get), 1);
}

#[test]
fn a_refused_make_admits_nothing_and_runs_no_plugin_code() {
    reset(NORMAL);
    DESTROYED.with(|value| value.set(0));
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    REFUSE_ENTRY.with(|refuse| refuse.set(true));
    let result = install(&mut kernel, 0);
    assert!(matches!(result, Err(Error::Refused)), "{result:?}");
    // Announced once, refused: no `leave`, no plugin, nothing registered.
    assert_eq!(counts(), (1, 0));
    assert!(kernel.shared.native_instances.find("factory").is_none());
    assert!(
        !kernel
            .shared
            .registry
            .screens
            .contains_key("factory.screen")
    );
    assert_eq!(DESTROYED.with(Cell::get), 0);
}

#[test]
fn a_refused_callback_does_not_run_and_the_destructor_still_does() {
    let mut kernel = kernel(0);
    let (entered, left) = counts();
    REFUSE_ENTRY.with(|refuse| refuse.set(true));
    open(&mut kernel, Params::new());
    let (now_entered, now_left) = counts();
    // Announced and refused: the plugin's code did not run, so nothing left.
    assert!(now_entered > entered);
    assert_eq!(now_left, left, "a refused entry must not be ended");
    // The destructor is announced but never refused.
    kernel.unload_plugin(&Id::new_static("factory"));
    assert_eq!(DESTROYED.with(Cell::get), 1);
    REFUSE_ENTRY.with(|refuse| refuse.set(false));
}
