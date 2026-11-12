//! Parked native plugins in the kernel (#248): a parked subscriber is made by
//! the pump that tells it, keeps its event where it cannot be made, and is
//! withdrawn where its batch was refused.
#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
extern crate std;
use crate::native_host::{
    HostResources,
    lease_slot::{Unpark, Unparked},
    native_instances::{InstanceState, NativeInstance, NativeLease},
    native_screen::Dispatch,
    registrations as wire,
    stack_rows::StackRow,
};
use alloc::{boxed::Box, rc::Rc, vec::Vec};
use core::cell::{Cell, RefCell};
use paperpaw_native::host_glue::{ERR_INVALID, OK};
use paperpaw_sdk::Id;

/// The made plugin: logs the `stack_changed` events it hears.
struct Listener {
    log: Rc<RefCell<Vec<u64>>>,
}
impl NativeInstance for Listener {
    fn running(&self) -> bool {
        true
    }
    fn stop(&self) {}
    fn subscribed(&self, tag: u32) -> bool {
        tag == wire::EVENT_STACK_CHANGED_TAG
    }
    fn service_event(&self, bytes: &[u8], _: &paperpaw_sdk::Env<'_>) -> Result<i32, i32> {
        let wire::Record::EventStackChanged(event) =
            crate::native_host::codec::decode_any_single(bytes)?
        else {
            return Err(ERR_INVALID);
        };
        self.log.borrow_mut().push(event.now_ms);
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

/// Makes a [`Listener`]; each unpark takes the next outcome.
struct Recipe {
    outcomes: Rc<RefCell<Vec<Result<(), Unparked>>>>,
    made: Rc<Cell<u32>>,
    log: Rc<RefCell<Vec<u64>>>,
}
impl Unpark for Recipe {
    fn unpark(&self) -> Result<Box<dyn NativeInstance>, Unparked> {
        self.made.set(self.made.get() + 1);
        self.outcomes.borrow_mut().remove(0).map(|()| {
            Box::new(Listener {
                log: Rc::clone(&self.log),
            }) as Box<dyn NativeInstance>
        })
    }
    fn subscribed(&self, tag: u32) -> bool {
        tag == wire::EVENT_STACK_CHANGED_TAG
    }
    fn with_image(&self, visit: &mut dyn FnMut(Option<&[u8]>)) {
        visit(None);
    }
}

struct World {
    _pool: Rc<HostResources>,
    kernel: crate::Kernel,
    log: Rc<RefCell<Vec<u64>>>,
    made: Rc<Cell<u32>>,
    key: u64,
}

fn world(outcomes: &[Result<(), Unparked>]) -> World {
    let pool = Rc::new(HostResources::try_new().unwrap());
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let log = Rc::new(RefCell::new(Vec::new()));
    let made = Rc::new(Cell::new(0));
    let clock = kernel.shared.native_instances.unpark_clock().unwrap();
    kernel.shared.native_instances.set_now(kernel.now.millis());
    let lease = NativeLease::parked(
        Box::new(Recipe {
            outcomes: Rc::new(RefCell::new(outcomes.to_vec())),
            made: Rc::clone(&made),
            log: Rc::clone(&log),
        }),
        clock,
    )
    .unwrap();
    let owner = pool.try_owner().unwrap();
    let key = kernel
        .shared
        .native_instances
        .insert(
            owner.try_view().unwrap(),
            Id::new_static("menu"),
            Some(lease),
            true,
        )
        .unwrap();
    World {
        _pool: pool,
        kernel,
        log,
        made,
        key,
    }
}

fn row(instance: u32) -> StackRow {
    StackRow {
        instance,
        owner: 0,
        flags: 0,
    }
}

impl World {
    /// The first look records the count; then the stack changes.
    fn change_stack(&mut self) {
        self.kernel.native_stack_pump();
        let changes = self.kernel.shared.stack_rows.changes();
        self.kernel
            .shared
            .stack_rows
            .update([row(u32::try_from(changes % 7).unwrap() + 1)]);
    }
}

#[test]
fn looking_at_a_parked_plugin_makes_nothing() {
    let w = world(&[Ok(())]);
    let instances = &w.kernel.shared.native_instances;
    assert!(instances.subscribed(w.key, wire::EVENT_STACK_CHANGED_TAG));
    assert!(matches!(
        instances.state(w.key),
        Ok(InstanceState::Running(_))
    ));
    assert_eq!(instances.running_nth(0).map(|(key, _)| key), Some(w.key));
    assert_eq!(w.made.get(), 0);
}

#[test]
fn a_parked_subscriber_is_made_by_the_pump_and_hears_its_event() {
    let mut w = world(&[Ok(())]);
    w.change_stack();
    assert!(!w.kernel.native_stack_pump());
    assert_eq!(w.made.get(), 1);
    assert_eq!(w.log.borrow().len(), 1);
    // Made once: later events go to the made instance.
    w.change_stack();
    w.kernel.native_stack_pump();
    assert_eq!(w.made.get(), 1);
    assert_eq!(w.log.borrow().len(), 2);
}

#[test]
fn no_memory_keeps_the_event_asks_no_pass_and_delivers_after_the_wait() {
    let mut w = world(&[Err(Unparked::NoMemory), Ok(())]);
    w.change_stack();
    assert!(
        !w.kernel.native_stack_pump(),
        "a waiting parked plugin asks no pass"
    );
    assert_eq!(w.made.get(), 1);
    assert!(w.log.borrow().is_empty(), "not told, not lost");
    // Within the wait nothing is tried.
    assert!(!w.kernel.native_unpark_pending());
    assert_eq!(w.made.get(), 1);
    w.kernel.now = w.kernel.now.add_millis(1_000);
    assert!(
        w.kernel.native_unpark_pending(),
        "made once its wait is over"
    );
    assert!(!w.kernel.native_stack_pump());
    assert_eq!(w.log.borrow().len(), 1, "the kept event arrives");
}

#[test]
fn inside_a_registration_nothing_is_made_and_it_starts_after() {
    let mut w = world(&[Ok(())]);
    w.change_stack();
    {
        let _hold = w.kernel.shared.native_instances.hold_unparking().unwrap();
        w.kernel.native_stack_pump();
        assert_eq!(w.made.get(), 0);
        assert!(
            w.kernel
                .shared
                .native_instances
                .take_parked_start()
                .is_none()
        );
    }
    assert!(w.kernel.native_unpark_pending());
    assert_eq!(w.made.get(), 1);
    w.kernel.native_stack_pump();
    assert_eq!(w.log.borrow().len(), 1);
}

#[test]
fn a_refused_batch_withdraws_the_plugin_through_the_normal_path() {
    let mut w = world(&[Err(Unparked::Refused)]);
    w.change_stack();
    w.kernel.native_stack_pump();
    assert!(w.log.borrow().is_empty());
    w.kernel.native_unpark_pending();
    let instances = &w.kernel.shared.native_instances;
    assert!(instances.id(w.key).is_none(), "withdrawn");
    assert!(!instances.subscribed(w.key, wire::EVENT_STACK_CHANGED_TAG));
    assert_eq!(w.made.get(), 1, "never tried again");
}
