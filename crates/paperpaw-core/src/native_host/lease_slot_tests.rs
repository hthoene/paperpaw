#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
use super::*;
use crate::native_host::{native_callbacks::CallbackClock, native_screen::Dispatch};
use alloc::rc::Rc;
use paperpaw_native::host_glue::{ERR_BUSY, ERR_CANCELLED};
use paperpaw_sdk::Env;

/// A made instance that counts its calls.
struct Made {
    calls: Rc<Cell<u32>>,
    active: Cell<bool>,
}
impl NativeInstance for Made {
    fn running(&self) -> bool {
        self.active.get()
    }
    fn stop(&self) {
        self.active.set(false);
    }
    fn subscribed(&self, tag: u32) -> bool {
        tag == 7
    }
}
impl Dispatch for Made {
    fn event(&self, _: u32, _: &[u8], _: &Env<'_>) -> Result<i32, i32> {
        self.calls.set(self.calls.get() + 1);
        Ok(0)
    }
    fn input(&self, _: u32, _: &[u8], _: &Env<'_>) -> Result<i32, i32> {
        Ok(0)
    }
    fn draw(
        &self,
        _: u32,
        _: &super::super::registrations::DrawFrame<'_>,
        _: &Env<'_>,
        _: &mut paperpaw_graphics::Canvas<'_>,
    ) -> Result<i32, i32> {
        Ok(0)
    }
    fn dropped(&self, _: u32, _: i32) {}
}

/// A recipe whose outcomes the test sets, one per unpark.
struct Recipe {
    outcomes: Rc<core::cell::RefCell<alloc::vec::Vec<Result<(), Unparked>>>>,
    made: Rc<Cell<u32>>,
    calls: Rc<Cell<u32>>,
}
impl Unpark for Recipe {
    fn unpark(&self) -> Result<Box<dyn NativeInstance>, Unparked> {
        self.made.set(self.made.get() + 1);
        let outcome = self.outcomes.borrow_mut().remove(0);
        outcome.map(|()| {
            Box::new(Made {
                calls: self.calls.clone(),
                active: Cell::new(true),
            }) as Box<dyn NativeInstance>
        })
    }
    fn subscribed(&self, tag: u32) -> bool {
        tag == 7
    }
    fn with_image(&self, visit: &mut dyn FnMut(Option<&[u8]>)) {
        visit(Some(b"image"));
    }
}

struct World {
    clock: CallbackClock,
    slot: Slot,
    outcomes: Rc<core::cell::RefCell<alloc::vec::Vec<Result<(), Unparked>>>>,
    made: Rc<Cell<u32>>,
}

fn world(outcomes: &[Result<(), Unparked>]) -> World {
    let mut clock = CallbackClock::default();
    let runtime = clock.runtime().unwrap();
    let outcomes = Rc::new(core::cell::RefCell::new(outcomes.to_vec()));
    let made = Rc::new(Cell::new(0));
    let slot = Slot::parked(
        Box::new(Recipe {
            outcomes: outcomes.clone(),
            made: made.clone(),
            calls: Rc::new(Cell::new(0)),
        }),
        runtime,
    );
    World {
        clock,
        slot,
        outcomes,
        made,
    }
}

#[test]
fn a_parked_plugin_is_running_and_answers_from_its_recipe_without_being_made() {
    let w = world(&[]);
    assert!(w.slot.running());
    assert!(w.slot.is_parked());
    assert!(w.slot.peek().is_none());
    assert!(w.slot.subscribed(7) && !w.slot.subscribed(8));
    let mut image = None;
    w.slot
        .with_image(&mut |bytes| image = bytes.map(<[u8]>::to_vec));
    assert_eq!(image.as_deref(), Some(&b"image"[..]));
    assert_eq!(w.made.get(), 0, "peeking makes nothing");
}

#[test]
fn the_accessor_makes_it_at_a_safe_point_once_and_the_recipe_is_gone() {
    let w = world(&[Ok(())]);
    assert!(w.slot.live().is_ok());
    assert!(w.slot.live().is_ok());
    assert_eq!(w.made.get(), 1);
    assert!(!w.slot.is_parked());
    assert!(
        w.slot.parked.borrow().is_none(),
        "the recipe gave its heap back"
    );
    assert!(w.slot.subscribed(7), "the made instance answers now");
}

#[test]
fn inside_a_callback_or_a_hold_it_is_busy_never_stopped_and_starts_next_pass() {
    let w = world(&[Ok(())]);
    w.clock.enter_outer().unwrap();
    assert_eq!(w.slot.live().err(), Some(ERR_BUSY));
    w.clock.leave();
    {
        let _hold = w.clock.hold().unwrap();
        assert_eq!(w.slot.live().err(), Some(ERR_BUSY));
    }
    assert_eq!(w.made.get(), 0);
    assert!(w.slot.running(), "busy is never stopped");
    assert!(w.slot.take_start(), "asked to start");
    assert!(!w.slot.take_start(), "once");
    assert!(w.slot.live().is_ok());
}

#[test]
fn no_memory_keeps_it_parked_and_waits_one_then_two_seconds() {
    let w = world(&[Err(Unparked::NoMemory), Err(Unparked::NoMemory), Ok(())]);
    w.clock.set_now(10_000);
    assert_eq!(w.slot.live().err(), Some(ERR_BUSY));
    assert!(w.slot.is_parked() && w.slot.running());
    w.clock.set_now(10_999);
    assert_eq!(w.slot.live().err(), Some(ERR_BUSY));
    assert!(!w.slot.take_start(), "not due before the wait");
    assert_eq!(w.made.get(), 1, "no try within the wait");
    w.clock.set_now(11_000);
    assert!(w.slot.take_start());
    assert_eq!(w.slot.live().err(), Some(ERR_BUSY));
    assert_eq!(w.made.get(), 2);
    w.clock.set_now(12_999);
    assert_eq!(w.slot.live().err(), Some(ERR_BUSY));
    w.clock.set_now(13_000);
    assert!(w.slot.live().is_ok());
    assert_eq!(w.made.get(), 3);
    assert!(w.outcomes.borrow().is_empty());
}

#[test]
fn the_wait_after_no_memory_stops_growing_at_a_minute() {
    let outcomes = [Err(Unparked::NoMemory); 10];
    let w = world(&outcomes);
    let mut now = 0;
    for _ in 0..10 {
        w.clock.set_now(now);
        assert_eq!(w.slot.live().err(), Some(ERR_BUSY));
        now = w.slot.parked.borrow().as_ref().unwrap().retry_at.get();
    }
    let parked = w.slot.parked.borrow();
    assert_eq!(parked.as_ref().unwrap().wait.get(), RETRY_LAST_MS);
}

#[test]
fn a_refused_batch_is_cancelled_and_waits_for_the_withdraw() {
    let w = world(&[Err(Unparked::Refused)]);
    assert_eq!(w.slot.live().err(), Some(ERR_CANCELLED));
    assert!(w.slot.refused());
    assert!(!w.slot.running() && !w.slot.is_parked());
    assert!(
        w.slot.take_start(),
        "the row withdraws it at the next safe point"
    );
    assert_eq!(w.slot.live().err(), Some(ERR_CANCELLED));
    assert_eq!(w.made.get(), 1, "never tried again");
}

#[test]
fn a_parked_plugin_stopped_or_retired_is_never_made() {
    let w = world(&[Ok(())]);
    w.slot.retire(1).unwrap();
    assert!(!w.slot.running());
    assert_eq!(w.slot.live().err(), Some(ERR_CANCELLED));
    assert_eq!(w.made.get(), 0);
    assert!(!w.slot.subscribed(7) || !w.slot.running());
}

#[test]
fn a_live_slot_behaves_as_the_lease_did() {
    let calls = Rc::new(Cell::new(0));
    let slot = Slot::live_now(Box::new(Made {
        calls,
        active: Cell::new(true),
    }));
    assert!(slot.live().is_ok() && slot.running() && !slot.is_parked());
    slot.stop();
    assert_eq!(slot.live().err(), Some(ERR_CANCELLED));
    assert!(!slot.take_start());
}
