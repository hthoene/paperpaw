//! Starting, waking and *Lock now* lock first, whatever else fails: the kind
//! put back with the code is asked for in the same callback but takes effect
//! after it, and neither it nor the tries nor the screen's refusal may leave
//! the device open.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
use core::cell::{Cell, RefCell};
use paperpaw_plugin_lock::native_flow::Lockstate;
use paperpaw_plugin_lock::native_gate::{self as gate, Device, Door};
use paperpaw_plugin_lock::native_model::{self as model, Code};
use paperpaw_sdk::native::Error;

/// A device whose answers and refusals the test sets.
struct Fake {
    stored: Option<String>,
    stored_error: Cell<Option<Error>>,
    repair_error: Option<Error>,
    tries: Result<u8, Error>,
    wake: Result<bool, Error>,
    /// Refusals of the screen still to come.
    open_refusals: Cell<u32>,
    /// What was asked: the kinds repaired for, screens opened.
    repaired: RefCell<Vec<Option<String>>>,
    opened: Cell<u32>,
}

impl Fake {
    fn with_code(kind: &str) -> Self {
        let salt = model::salt(Some("abcdefghijklmnop"), "", 0).unwrap();
        Self {
            stored: Some(model::kept_value(kind, &salt, "1234").unwrap()),
            ..Self::none()
        }
    }

    fn none() -> Self {
        Self {
            stored: None,
            stored_error: Cell::new(None),
            repair_error: None,
            tries: Ok(0),
            wake: Ok(true),
            open_refusals: Cell::new(0),
            repaired: RefCell::new(Vec::new()),
            opened: Cell::new(0),
        }
    }
}

impl Device for Fake {
    fn stored(&self) -> Result<Option<Code>, Error> {
        if let Some(error) = self.stored_error.get() {
            return Err(error);
        }
        Ok(self.stored.clone().map(Code::new))
    }

    fn repair_kind(&self, stored: Option<&str>) -> Result<(), Error> {
        self.repaired.borrow_mut().push(stored.map(str::to_owned));
        self.repair_error.map_or(Ok(()), Err)
    }

    fn tries(&self) -> Result<u8, Error> {
        self.tries
    }

    fn on_wake(&self) -> Result<bool, Error> {
        self.wake
    }

    fn open_unlock(&self) -> Result<(), Error> {
        if self.open_refusals.get() > 0 {
            self.open_refusals.set(self.open_refusals.get() - 1);
            return Err(Error::Busy);
        }
        self.opened.set(self.opened.get() + 1);
        Ok(())
    }
}

fn fresh() -> (Lockstate, Door) {
    (Lockstate::default(), Door::default())
}

#[test]
fn a_code_kept_locks_at_the_start_whatever_the_kind_setting_says() {
    // The kind setting says `off` (a reset): the repair is queued, but a
    // command takes effect after the callback, so the lock must not wait for
    // it.
    let device = Fake::with_code("pin");
    let (mut lock, mut door) = fresh();
    gate::started(&device, &mut lock, &mut door, 0).unwrap();
    assert!(lock.locked);
    assert_eq!(device.opened.get(), 1);
    assert_eq!(device.repaired.borrow().len(), 1);
}

#[test]
fn no_code_locks_nothing() {
    let device = Fake::none();
    let (mut lock, mut door) = fresh();
    gate::started(&device, &mut lock, &mut door, 0).unwrap();
    gate::woke(&device, &mut lock, &mut door).unwrap();
    gate::lock_now(&device, &mut lock, &mut door).unwrap();
    assert!(!lock.locked);
    assert_eq!(device.opened.get(), 0);
}

#[test]
fn a_damaged_code_locks_nothing() {
    let mut device = Fake::none();
    device.stored = Some("pin::".to_owned());
    let (mut lock, mut door) = fresh();
    gate::started(&device, &mut lock, &mut door, 0).unwrap();
    assert!(!lock.locked);
}

#[test]
fn the_start_locks_before_the_repair_and_the_tries_can_fail() {
    let mut device = Fake::with_code("pattern");
    device.repair_error = Some(Error::NoMemory);
    device.tries = Err(Error::Io);
    let (mut lock, mut door) = fresh();
    // The first refusal is reported, after the lock.
    assert_eq!(
        gate::started(&device, &mut lock, &mut door, 0),
        Err(Error::NoMemory)
    );
    assert!(lock.locked);
    assert_eq!(device.opened.get(), 1);
}

#[test]
fn the_tries_are_kept_when_only_the_repair_fails() {
    let mut device = Fake::with_code("pin");
    device.repair_error = Some(Error::Io);
    device.tries = Ok(5);
    let (mut lock, mut door) = fresh();
    assert!(gate::started(&device, &mut lock, &mut door, 1_000).is_err());
    assert!(lock.locked);
    assert!(lock.attempts.waiting(1_001));
}

#[test]
fn a_code_that_cannot_be_read_is_reported_and_the_rest_is_done() {
    let mut device = Fake::with_code("pin");
    device.stored_error.set(Some(Error::Io));
    device.tries = Ok(5);
    let (mut lock, mut door) = fresh();
    assert_eq!(
        gate::started(&device, &mut lock, &mut door, 0),
        Err(Error::Io)
    );
    assert!(lock.attempts.waiting(1));
    assert!(device.repaired.borrow().is_empty());
}

#[test]
fn a_refused_screen_leaves_the_device_locked() {
    let device = Fake::with_code("pin");
    device.open_refusals.set(1);
    let (mut lock, mut door) = fresh();
    assert_eq!(
        gate::started(&device, &mut lock, &mut door, 0),
        Err(Error::Busy)
    );
    assert!(lock.locked);
    assert!(!door.opening);
    // Locked and no screen: the next occasion opens it.
    gate::lock_now(&device, &mut lock, &mut door).unwrap();
    assert_eq!(device.opened.get(), 1);
    assert!(door.opening);
}

#[test]
fn a_screen_that_was_closed_and_not_reopened_is_opened_by_lock_now() {
    let device = Fake::with_code("password");
    let (mut lock, mut door) = fresh();
    gate::lock_now(&device, &mut lock, &mut door).unwrap();
    // It showed, then something closed it, and reopening it was refused.
    door.opening = false;
    door.shown = false;
    device.open_refusals.set(1);
    assert_eq!(gate::reopen(&device, &lock, &mut door), Err(Error::Busy));
    assert!(lock.locked);
    gate::lock_now(&device, &mut lock, &mut door).unwrap();
    assert_eq!(device.opened.get(), 2);
}

#[test]
fn a_screen_that_is_there_or_on_its_way_is_not_opened_twice() {
    let device = Fake::with_code("pin");
    let (mut lock, mut door) = fresh();
    gate::lock_now(&device, &mut lock, &mut door).unwrap();
    gate::lock_now(&device, &mut lock, &mut door).unwrap();
    assert_eq!(device.opened.get(), 1);
    door.opening = false;
    door.shown = true;
    gate::lock_now(&device, &mut lock, &mut door).unwrap();
    gate::reopen(&device, &lock, &mut door).unwrap();
    assert_eq!(device.opened.get(), 1);
}

#[test]
fn waking_locks_unless_it_is_set_not_to_and_where_that_cannot_be_read_it_does() {
    let mut device = Fake::with_code("pin");
    device.wake = Ok(false);
    let (mut lock, mut door) = fresh();
    gate::woke(&device, &mut lock, &mut door).unwrap();
    assert!(!lock.locked);

    device.wake = Err(Error::Io);
    device.repair_error = Some(Error::NoMemory);
    let (mut lock, mut door) = fresh();
    // Reported (the first refusal is the lock's own read), but locked.
    assert!(gate::woke(&device, &mut lock, &mut door).is_err());
    assert!(lock.locked);
    assert_eq!(device.opened.get(), 1);
}

#[test]
fn waking_locks_before_the_repair_and_it_failing_changes_nothing() {
    let mut device = Fake::with_code("password");
    device.repair_error = Some(Error::NoMemory);
    let (mut lock, mut door) = fresh();
    assert_eq!(
        gate::woke(&device, &mut lock, &mut door),
        Err(Error::NoMemory)
    );
    assert!(lock.locked);
    assert_eq!(device.opened.get(), 1);
}
