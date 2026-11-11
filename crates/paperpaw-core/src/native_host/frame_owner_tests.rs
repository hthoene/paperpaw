#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
extern crate std;
use super::*;
use core::any::TypeId;

fn ctx(n: usize) -> *const c_void {
    (n * 8) as *const c_void
}

/// The services type the frames of these tests are pushed for.
struct Ours;
struct Another;

fn table() -> TypeId {
    TypeId::of::<Ours>()
}

fn context() -> Option<*const c_void> {
    context_of(table())
}

#[test]
fn there_is_no_owner_outside_a_frame() {
    assert_eq!(owner(), None);
    assert_eq!(context(), None);
}

#[test]
fn a_frame_names_its_plugin_and_the_previous_one_is_back_after_it() {
    within(7, ctx(1), table(), || {
        assert_eq!(owner(), Some(7));
        assert_eq!(context(), Some(ctx(1)));
    });
    assert_eq!(owner(), None);
}

#[test]
fn nested_frames_restore_each_previous_owner() {
    within(1, ctx(1), table(), || {
        within(2, ctx(2), table(), || {
            within(3, ctx(3), table(), || assert_eq!(owner(), Some(3)));
            assert_eq!(owner(), Some(2));
        });
        assert_eq!(owner(), Some(1));
        assert_eq!(context(), Some(ctx(1)));
    });
    assert_eq!(owner(), None);
}

#[test]
fn an_error_return_restores_the_owner() {
    let outcome: Result<(), i32> = within(5, ctx(5), table(), || {
        assert_eq!(owner(), Some(5));
        Err(-3)
    });
    assert_eq!(outcome, Err(-3));
    assert_eq!(owner(), None);
    // And a nested error leaves the outer frame in place.
    within(1, ctx(1), table(), || {
        let _: Result<(), i32> = within(2, ctx(2), table(), || Err(-1));
        assert_eq!(owner(), Some(1));
    });
}

#[test]
fn an_unwinding_call_restores_the_owner() {
    let caught = std::panic::catch_unwind(|| {
        within(9, ctx(9), table(), || panic!("plugin fault"));
    });
    assert!(caught.is_err());
    assert_eq!(owner(), None);
}

#[test]
fn threads_do_not_see_each_others_frames() {
    within(1, ctx(1), table(), || {
        std::thread::spawn(|| assert_eq!(owner(), None))
            .join()
            .unwrap();
        assert_eq!(owner(), Some(1));
    });
}

#[test]
fn a_frame_pushed_for_another_services_type_gives_no_context() {
    let other = TypeId::of::<Another>();
    within(1, ctx(1), other, || {
        assert_eq!(owner(), Some(1), "the owner is the owner's");
        assert_eq!(
            context(),
            None,
            "the entries of another services type read none"
        );
        assert_eq!(context_of(other), Some(ctx(1)));
    });
}
