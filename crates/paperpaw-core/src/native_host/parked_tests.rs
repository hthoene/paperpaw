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
    /// What `holds_nothing` answers (D3), and its drops counted.
    idle: Rc<Cell<bool>>,
    dropped: Rc<Cell<u32>>,
    /// Kept in the instance's own memory only: the counter fixture.
    count: Cell<u32>,
    counts: Rc<RefCell<Vec<u32>>>,
}
impl Drop for Listener {
    fn drop(&mut self) {
        self.dropped.set(self.dropped.get() + 1);
    }
}
impl NativeInstance for Listener {
    fn running(&self) -> bool {
        true
    }
    fn stop(&self) {}
    fn holds_nothing(&self) -> bool {
        self.idle.get()
    }
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
        self.count.set(self.count.get() + 1);
        self.counts.borrow_mut().push(self.count.get());
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
    idle: Rc<Cell<bool>>,
    dropped: Rc<Cell<u32>>,
    counts: Rc<RefCell<Vec<u32>>>,
}
impl Unpark for Recipe {
    fn unpark(&self) -> Result<Box<dyn NativeInstance>, Unparked> {
        self.made.set(self.made.get() + 1);
        self.outcomes.borrow_mut().remove(0).map(|()| {
            Box::new(Listener {
                log: Rc::clone(&self.log),
                idle: Rc::clone(&self.idle),
                dropped: Rc::clone(&self.dropped),
                count: Cell::new(0),
                counts: Rc::clone(&self.counts),
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
    idle: Rc<Cell<bool>>,
    dropped: Rc<Cell<u32>>,
    counts: Rc<RefCell<Vec<u32>>>,
}

fn world(outcomes: &[Result<(), Unparked>]) -> World {
    let pool = Rc::new(HostResources::try_new().unwrap());
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let log = Rc::new(RefCell::new(Vec::new()));
    let made = Rc::new(Cell::new(0));
    let idle = Rc::new(Cell::new(true));
    let dropped = Rc::new(Cell::new(0));
    let counts = Rc::new(RefCell::new(Vec::new()));
    let clock = kernel.shared.native_instances.unpark_clock().unwrap();
    kernel.shared.native_instances.set_now(kernel.now.millis());
    let lease = NativeLease::parked(
        Box::new(Recipe {
            outcomes: Rc::new(RefCell::new(outcomes.to_vec())),
            made: Rc::clone(&made),
            log: Rc::clone(&log),
            idle: Rc::clone(&idle),
            dropped: Rc::clone(&dropped),
            counts: Rc::clone(&counts),
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
        idle,
        dropped,
        counts,
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
        self.kernel.shared.stack_rows.update([row(changes % 7 + 1)]);
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

impl World {
    fn parked(&self) -> bool {
        self.kernel
            .shared
            .native_instances
            .lease(self.key)
            .is_some_and(|lease| lease.is_parked())
    }
}

#[test]
fn on_standby_an_idle_plugin_sleeps_again_and_is_made_at_its_next_use() {
    // D3, Decision 166: parked again on standby, made again on next use.
    let mut w = world(&[Ok(()), Ok(())]);
    w.change_stack();
    w.kernel.native_stack_pump();
    assert!(!w.parked());
    let now = w.kernel.now;
    let _ = w.kernel.suspend(now);
    assert!(w.parked(), "idle on standby: parked again");
    assert_eq!(w.dropped.get(), 1, "its instance (and `destroy`) is gone");
    w.change_stack();
    w.kernel.native_stack_pump();
    assert_eq!(w.made.get(), 2, "made again at its next use");
    assert_eq!(w.log.borrow().len(), 2);
}

#[test]
fn a_plugin_that_holds_something_stays_running_on_standby() {
    let mut w = world(&[Ok(())]);
    w.change_stack();
    w.kernel.native_stack_pump();
    w.idle.set(false);
    let now = w.kernel.now;
    let _ = w.kernel.suspend(now);
    assert!(!w.parked());
    assert_eq!(w.dropped.get(), 0);
}

#[test]
fn without_standby_or_pressure_nothing_sleeps_again() {
    // No idle timer (Decision 166): passes and time alone park nothing.
    let mut w = world(&[Ok(())]);
    w.change_stack();
    w.kernel.native_stack_pump();
    for _ in 0..3 {
        w.kernel.now = w.kernel.now.add_millis(600_000);
        w.kernel.native_unpark_pending();
    }
    assert!(!w.parked());
}

#[test]
fn memory_pressure_parks_one_idle_plugin_at_the_next_safe_point() {
    let mut w = world(&[Ok(())]);
    w.change_stack();
    w.kernel.native_stack_pump();
    // Another unpark found no memory (as `Slot::unpark` notes it).
    w.kernel
        .shared
        .native_instances
        .unpark_clock()
        .unwrap()
        .note_pressure();
    w.kernel.native_unpark_pending();
    assert!(w.parked(), "parked again under pressure");
    w.kernel.native_unpark_pending();
    assert_eq!(w.dropped.get(), 1, "once: the pressure was taken");
}

/// The same flow (an event, standby, an event) with the plugin held awake
/// and with it free to sleep again: what the plugin reported each time.
fn counter_flow(may_sleep: bool) -> Vec<u32> {
    let mut w = world(&[Ok(()), Ok(())]);
    w.idle.set(may_sleep);
    w.change_stack();
    w.kernel.native_stack_pump();
    let now = w.kernel.now;
    let _ = w.kernel.suspend(now);
    w.change_stack();
    w.kernel.native_stack_pump();
    w.counts.borrow().clone()
}

#[test]
fn a_plugin_that_keeps_state_in_memory_is_caught_by_comparing_runs() {
    // The deliberately broken fixture (Manager's condition for D3): a counter
    // kept only in the instance's memory starts over when it sleeps again,
    // so the record with re-park differs from the one held awake. This is
    // why a plugin sleeps only with a parity case that runs such a flow.
    assert_eq!(counter_flow(false), [1, 2]);
    assert_eq!(counter_flow(true), [1, 1]);
}

/// The screen queue's room and whether a batch waits after the safe point
/// that parks again ([`crate::Kernel::native_repark_idle`]), which standby
/// and memory pressure reach; a screen callback reserved the room before
/// (#248, lever 6). On standby the kernel runs a waiting batch first, so a
/// batch still waits only where the safe point is reached directly.
fn screen_room_after(batch_waits: bool, standby: bool) -> (usize, bool) {
    let mut w = world(&[Ok(())]);
    let key = w.key;
    {
        let stage = w.kernel.shared.native_instances.screen_stage().unwrap();
        let slot = stage.reserve().unwrap();
        if batch_waits {
            let staged =
                crate::native_host::event_call::Staged::new(key, Vec::new(), None, Vec::new());
            slot.put(staged).unwrap();
        }
    }
    if standby {
        let now = w.kernel.now;
        let _ = w.kernel.suspend(now);
    } else {
        let _ = w.kernel.native_repark_idle(usize::MAX);
    }
    let stage = w.kernel.shared.native_instances.screen_stage().unwrap();
    (stage.capacity(), stage.waiting())
}

#[test]
fn on_standby_an_idle_screen_queue_gives_its_room_back() {
    assert_eq!(screen_room_after(false, true), (0, false));
}

#[test]
fn on_standby_a_waiting_screen_batch_runs_and_the_room_goes_back() {
    assert_eq!(screen_room_after(true, true), (0, false));
}

#[test]
fn a_waiting_screen_batch_keeps_the_queues_room() {
    assert_eq!(screen_room_after(true, false), (1, true));
}
