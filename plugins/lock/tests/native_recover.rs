//! A screen the host opened whose view could not be made is made later, so
//! that the lock screen never stays blank: every allocation of the first try
//! may be refused, and the next one makes the view.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
#[path = "../../test-support/fail_alloc.rs"]
mod fail_alloc;
use core::cell::RefCell;
use paperpaw_plugin_lock::native_gate::{Bare, guard_due, recover};
use paperpaw_sdk::native::Error;

#[global_allocator]
static ALLOC: fail_alloc::Failing = fail_alloc::Failing;

#[test]
fn a_view_refused_in_opened_is_made_at_the_next_occasion() {
    fail_alloc::sweep(|| {
        let mut bare = Bare::default();
        let views: RefCell<Vec<(u32, bool)>> = RefCell::new(Vec::new());
        // `opened`: noted first, then the view made (and refused, in turn).
        bare.note(7_u32, true);
        let make = |instance: u32, unlock: bool| -> Result<(), Error> {
            let mut views = views.borrow_mut();
            views.try_reserve(1).map_err(|_| Error::NoMemory)?;
            views.push((instance, unlock));
            Ok(())
        };
        if let Err(error) = recover(&mut bare, make) {
            assert_eq!(error, Error::NoMemory);
            // Nothing was made and the screen still waits, as it opened.
            assert_eq!(bare.needs(), Some((7, true)));
            // The shown, draw or input that follows makes it.
            recover(&mut bare, make)?;
        }
        assert_eq!(bare.needs(), None);
        assert_eq!(*views.borrow(), [(7, true)]);
        // Made, a later occasion has nothing to do.
        recover(&mut bare, make)?;
        assert_eq!(views.borrow().len(), 1);
        Ok(())
    });
}

#[test]
fn a_closed_screen_is_no_longer_waited_for() {
    let mut bare = Bare::default();
    bare.note(3_u32, false);
    bare.forget(4);
    assert_eq!(bare.needs(), Some((3, false)));
    bare.forget(3);
    assert_eq!(bare.needs(), None);
    assert_eq!(recover(&mut bare, |_, _| Err(Error::Invalid)), Ok(()));
}

#[test]
fn a_refusal_keeps_the_screen_waiting_for_each_occasion() {
    let mut bare = Bare::default();
    bare.note(5_u32, true);
    for _ in 0..3 {
        assert_eq!(
            recover(&mut bare, |_, _| Err(Error::NoMemory)),
            Err(Error::NoMemory)
        );
        assert_eq!(bare.needs(), Some((5, true)));
    }
}

#[test]
fn draw_has_no_command_stage_so_the_policy_waits_for_shown_or_input() {
    // (unlock, guarded, commands): the policy is due only for an unlock screen
    // that has none, in a callback that can stage a command.
    assert!(guard_due(true, false, true));
    // `draw`: the view alone.
    assert!(!guard_due(true, false, false));
    // Already set, or not an unlock screen: nothing to do.
    assert!(!guard_due(true, true, true));
    assert!(!guard_due(false, false, true));
    assert!(!guard_due(false, true, false));
}
