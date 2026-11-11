//! `event.stack_changed` (81): a hint after the stack differed, coalesced per
//! plugin, told between callbacks, only to running subscribers, and nothing
//! for what happened before a plugin first looked.
#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
extern crate std;
use crate::native_host::{
    HostResources,
    native_instances::{NativeInstance, NativeLease},
    native_screen::Dispatch,
    registrations as wire,
    stack_rows::StackRow,
};
use alloc::{rc::Rc, vec::Vec};
use core::cell::{Cell, RefCell};
use paperpaw_native::host_glue::{ERR_INVALID, OK};
use paperpaw_sdk::Id;

/// A plugin that logs the times of the `stack_changed` events it hears.
struct Listener {
    log: Rc<RefCell<Vec<u64>>>,
    subscribed: bool,
    /// Whether its next event callback changes the stack (it cannot, but the
    /// test can: it bumps the count through the rows).
    rows: Rc<crate::native_host::stack_rows::StackRows>,
    changes_stack: Rc<Cell<bool>>,
}

impl NativeInstance for Listener {
    fn running(&self) -> bool {
        true
    }
    fn stop(&self) {}
    fn subscribed(&self, tag: u32) -> bool {
        self.subscribed && tag == wire::EVENT_STACK_CHANGED_TAG
    }
    fn service_event(&self, bytes: &[u8], _: &paperpaw_sdk::Env<'_>) -> Result<i32, i32> {
        let wire::Record::EventStackChanged(event) =
            crate::native_host::codec::decode_any_single(bytes)?
        else {
            return Err(ERR_INVALID);
        };
        self.log.borrow_mut().push(event.now_ms);
        if self.changes_stack.replace(false) {
            self.rows.update([row(9)]);
        }
        Ok(OK)
    }
}

impl Dispatch for Listener {
    fn event(&self, _: u32, _: &[u8], _: &paperpaw_sdk::Env<'_>) -> Result<i32, i32> {
        Err(ERR_INVALID)
    }
    fn input(&self, _: u32, _: &[u8], _: &paperpaw_sdk::Env<'_>) -> Result<i32, i32> {
        Err(ERR_INVALID)
    }
    fn draw(
        &self,
        _: u32,
        _: &wire::DrawFrame<'_>,
        _: &paperpaw_sdk::Env<'_>,
        _: &mut paperpaw_graphics::Canvas<'_>,
    ) -> Result<i32, i32> {
        Err(ERR_INVALID)
    }
    fn dropped(&self, _: u32, _: i32) {}
}

fn row(instance: u32) -> StackRow {
    StackRow {
        instance,
        owner: 0,
        flags: 0,
    }
}

struct World {
    pool: Rc<HostResources>,
    kernel: crate::Kernel,
    log: Rc<RefCell<Vec<u64>>>,
    changes_stack: Rc<Cell<bool>>,
}

/// A kernel with `names.len()` plugins sharing one log; `subscribed` says which
/// of them subscribed.
fn world(subscribed: &[bool]) -> World {
    let pool = Rc::new(HostResources::try_new().unwrap());
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let log = Rc::new(RefCell::new(Vec::new()));
    let changes_stack = Rc::new(Cell::new(false));
    for (n, subscribed) in subscribed.iter().enumerate() {
        let owner = pool.try_owner().unwrap();
        let lease = NativeLease::try_new(Listener {
            log: Rc::clone(&log),
            subscribed: *subscribed,
            rows: Rc::clone(&kernel.shared.stack_rows),
            changes_stack: Rc::clone(&changes_stack),
        })
        .unwrap();
        let id = ["help", "menu", "home", "keys"][n];
        kernel
            .shared
            .native_instances
            .insert(
                owner.try_view().unwrap(),
                Id::new_static(id),
                Some(lease),
                true,
            )
            .unwrap();
    }
    World {
        pool,
        kernel,
        log,
        changes_stack,
    }
}

impl World {
    fn rows(&self) -> &crate::native_host::stack_rows::StackRows {
        &self.kernel.shared.stack_rows
    }
}

#[test]
fn an_update_that_changes_nothing_is_no_change() {
    let w = world(&[]);
    let before = w.rows().changes();
    w.rows().update([row(1)]);
    assert_eq!(w.rows().changes(), before.wrapping_add(1));
    w.rows().update([row(1)]);
    assert_eq!(w.rows().changes(), before.wrapping_add(1), "same rows");
    w.rows().update([row(1), row(2)]);
    w.rows().update([row(1)]);
    assert_eq!(w.rows().changes(), before.wrapping_add(3), "push, pop");
    w.rows().update([row(2)]);
    assert_eq!(w.rows().changes(), before.wrapping_add(4), "replace");
}

#[test]
fn a_subscriber_hears_once_of_any_number_of_changes_and_not_of_earlier_ones() {
    let mut w = world(&[true]);
    w.rows().update([row(1)]);
    // The first look only records the count.
    assert!(!w.kernel.native_stack_pump());
    assert!(w.log.borrow().is_empty());
    w.rows().update([row(1), row(2)]);
    w.rows().update([row(1), row(2), row(3)]);
    w.rows().update([row(1)]);
    assert!(!w.kernel.native_stack_pump());
    assert_eq!(w.log.borrow().len(), 1, "three changes, one hint");
    assert!(!w.kernel.native_stack_pump());
    assert_eq!(w.log.borrow().len(), 1, "told once");
}

#[test]
fn only_a_running_subscriber_is_told() {
    let mut w = world(&[true, false]);
    w.kernel.native_stack_pump();
    w.rows().update([row(1)]);
    w.kernel.native_stack_pump();
    assert_eq!(w.log.borrow().len(), 1, "the one that subscribed");
    // A plugin that stopped is not told and its entry goes.
    let key = w.kernel.shared.native_instances.find("help").unwrap();
    w.kernel.shared.native_instances.stop(key).unwrap();
    w.rows().update([row(2)]);
    w.kernel.native_stack_pump();
    assert_eq!(w.log.borrow().len(), 1);
    assert!(w.rows().told.borrow().is_empty());
}

#[test]
fn every_subscriber_hears_and_a_change_made_in_a_callback_is_told_again() {
    let mut w = world(&[true, true]);
    w.kernel.native_stack_pump();
    w.rows().update([row(1)]);
    w.changes_stack.set(true);
    // The first is told, and its callback changes the stack again: it is told
    // once more (never inside the callback), the second once for both changes.
    let mut passes = 0;
    while w.kernel.native_stack_pump() {
        passes += 1;
        assert!(passes < 5, "the pump settles");
    }
    assert_eq!(w.log.borrow().len(), 3);
    assert!(!w.kernel.native_stack_pump());
    assert_eq!(w.log.borrow().len(), 3);
    let _ = &w.pool;
}
