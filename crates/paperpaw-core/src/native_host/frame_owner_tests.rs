#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
extern crate std;
use super::*;

fn ctx(n: usize) -> *const c_void {
    (n * 8) as *const c_void
}

#[test]
fn there_is_no_owner_outside_a_frame() {
    assert_eq!(owner(), None);
    assert_eq!(context(), None);
}

#[test]
fn a_frame_names_its_plugin_and_the_previous_one_is_back_after_it() {
    within(7, ctx(1), || {
        assert_eq!(owner(), Some(7));
        assert_eq!(context(), Some(ctx(1)));
    });
    assert_eq!(owner(), None);
}

#[test]
fn nested_frames_restore_each_previous_owner() {
    within(1, ctx(1), || {
        within(2, ctx(2), || {
            within(3, ctx(3), || assert_eq!(owner(), Some(3)));
            assert_eq!(owner(), Some(2));
        });
        assert_eq!(owner(), Some(1));
        assert_eq!(context(), Some(ctx(1)));
    });
    assert_eq!(owner(), None);
}

#[test]
fn an_error_return_restores_the_owner() {
    let outcome: Result<(), i32> = within(5, ctx(5), || {
        assert_eq!(owner(), Some(5));
        Err(-3)
    });
    assert_eq!(outcome, Err(-3));
    assert_eq!(owner(), None);
    // And a nested error leaves the outer frame in place.
    within(1, ctx(1), || {
        let _: Result<(), i32> = within(2, ctx(2), || Err(-1));
        assert_eq!(owner(), Some(1));
    });
}

#[test]
fn an_unwinding_call_restores_the_owner() {
    let caught = std::panic::catch_unwind(|| {
        within(9, ctx(9), || panic!("plugin fault"));
    });
    assert!(caught.is_err());
    assert_eq!(owner(), None);
}

#[test]
fn threads_do_not_see_each_others_frames() {
    within(1, ctx(1), || {
        std::thread::spawn(|| assert_eq!(owner(), None))
            .join()
            .unwrap();
        assert_eq!(owner(), Some(1));
    });
}
